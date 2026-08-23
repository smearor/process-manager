use crate::config::ProcessConfig;
use crate::process::ProcessId;
use crate::process::ProcessState;
use crate::process::RestartState;
use crate::signal::KillSignal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::process::Child;
use std::process::ExitStatus;
use std::sync::Arc;
use std::thread::JoinHandle;

/// A managed child process.
///
/// Represents a running or completed child process tracked by `ProcessManager`.
/// The `child` handle is always `Some` - even forked processes (with `setsid()`)
/// are tracked with their `Child` handle for stop/reaper operations.
#[derive(Debug)]
#[must_use]
pub struct Process {
    /// Unique identifier assigned by the `ProcessManager`.
    pub id: ProcessId,

    /// The PID of the child process.
    pub pid: u32,

    /// The program name (for error reporting).
    pub program_name: String,

    /// The label under which this process was started (for grouped operations).
    pub label: String,

    /// Whether to terminate this process when the `ProcessManager` is dropped.
    pub terminate_on_exit: bool,

    /// The configuration this process was started with.
    pub config: Arc<ProcessConfig>,

    /// The child process handle. Always `Some` - forked processes are also
    /// tracked (with `setsid()` applied via `pre_exec`).
    pub child: Option<Child>,

    /// Join handle for the stdout reader thread, if stdout was piped.
    pub stdout_reader: Option<JoinHandle<()>>,

    /// Join handle for the stderr reader thread, if stderr was piped.
    pub stderr_reader: Option<JoinHandle<()>>,

    /// The current lifecycle state of this process.
    ///
    /// Updated lazily by `state()` / `is_running()` via non-blocking
    /// `try_wait()`, and explicitly by `stop()` / `restart()`.
    pub state: ProcessState,

    /// Restart tracking state for automatic restarts with backoff.
    ///
    /// `Some` when `restart_on_exit` is `true`, `None` otherwise.
    /// Maintained by the reaper thread.
    pub restart_state: Option<RestartState>,
}

impl Process {
    /// The current lifecycle state of this process.
    ///
    /// Performs a non-blocking `try_wait()` to detect whether the process has
    /// exited since the last check. If it has, the state is updated to
    /// `Stopped` (success) or `Crashed` (failure) and returned.
    ///
    /// For processes in `Stopping`, `Stopped`, `Crashed`, `Restarting`, or
    /// `Failed`, the state is returned as-is without polling.
    pub fn state(&mut self) -> ProcessState {
        if self.state.is_terminated() || self.state == ProcessState::Stopping || self.state == ProcessState::Restarting {
            return self.state;
        }
        match &mut self.child {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    self.state = if status.success() { ProcessState::Stopped } else { ProcessState::Crashed };
                    self.state
                }
                Ok(None) => {
                    self.state = ProcessState::Running;
                    ProcessState::Running
                }
                Err(_) => {
                    self.state = ProcessState::Crashed;
                    ProcessState::Crashed
                }
            },
            None => {
                self.state = ProcessState::Stopped;
                ProcessState::Stopped
            }
        }
    }

    /// Whether the child process is still running.
    ///
    /// Delegates to `state()` and returns `true` if the state is `Running`.
    /// Uses non-blocking `try_wait()` on the `Child` handle for all processes
    /// (forked and non-forked). No `/proc` polling needed since the `Child`
    /// handle is always available.
    pub fn is_running(&mut self) -> bool {
        self.state().is_alive()
    }

    /// Non-blocking check whether the child has exited.
    ///
    /// Returns `Some(ExitStatus)` if the process has exited, `None` if it is
    /// still running or the status could not be determined.
    pub fn try_wait_exit(&mut self) -> Option<ExitStatus> {
        match &mut self.child {
            Some(child) => child.try_wait().ok().flatten(),
            None => None,
        }
    }

    /// Wait for the child process to exit. Returns the exit status.
    ///
    /// Blocks until the child process exits. Only available for non-forked
    /// processes (forked processes should use `is_running()` with the reaper).
    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match &mut self.child {
            Some(child) => child.wait(),
            None => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "No child handle")),
        }
    }

    /// Send the configured kill signal to the child process.
    ///
    /// Uses `nix::sys::signal::kill(Pid::from_raw(pid), signal)` for both
    /// forked and tracked processes, since `std::process::Child::kill()`
    /// always sends `SIGKILL` and does not respect `kill_signal`.
    pub fn send_signal(&mut self, signal: KillSignal) -> std::io::Result<()> {
        let nix_signal = signal.to_nix_signal();
        nix::sys::signal::kill(Pid::from_raw(self.pid as i32), nix_signal).map_err(|e| std::io::Error::from_raw_os_error(e as i32))
    }

    /// Force-kill the child process with `SIGKILL`.
    ///
    /// This is the escalation step after `SIGTERM` + grace period.
    /// Uses `nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGKILL)`.
    pub fn force_kill(&mut self) -> std::io::Result<()> {
        nix::sys::signal::kill(Pid::from_raw(self.pid as i32), Signal::SIGKILL).map_err(|e| std::io::Error::from_raw_os_error(e as i32))
    }

    /// Join reader threads with a timeout.
    ///
    /// After the child process exits, its stdout/stderr pipes are closed
    /// and the reader threads should terminate. This joins them with a
    /// short timeout to avoid lingering threads. Threads that don't finish
    /// within the timeout are detached (logged as warning).
    pub fn join_readers(&mut self, timeout: std::time::Duration) {
        for (name, handle) in [("stdout", self.stdout_reader.take()), ("stderr", self.stderr_reader.take())] {
            if let Some(handle) = handle {
                let start = std::time::Instant::now();
                while !handle.is_finished() && start.elapsed() < timeout {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                if handle.is_finished() {
                    let _ = handle.join();
                } else {
                    tracing::warn!("Process {} (pid={}) {} reader thread did not finish within {:?}, detaching", self.id, self.pid, name, timeout);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_process_is_running_true() {
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let mut process = Process {
            id: ProcessId::new(1),
            pid,
            program_name: "sleep".to_string(),
            label: "test".to_string(),
            terminate_on_exit: false,
            config: Arc::new(ProcessConfig::builder().command("sleep".to_string()).build()),
            child: Some(child),
            stdout_reader: None,
            stderr_reader: None,
            state: ProcessState::Starting,
            restart_state: None,
        };
        assert!(process.is_running());
        assert_eq!(process.state, ProcessState::Running);
        // Clean up
        let _ = process.send_signal(KillSignal::Sigkill);
        let _ = process.child.as_mut().unwrap().wait();
    }

    #[test]
    fn test_process_is_running_false_after_exit() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        // Wait for it to exit
        let _ = child.wait();
        let mut process = Process {
            id: ProcessId::new(1),
            pid,
            program_name: "true".to_string(),
            label: "test".to_string(),
            terminate_on_exit: false,
            config: Arc::new(ProcessConfig::builder().command("true".to_string()).build()),
            child: Some(child),
            stdout_reader: None,
            stderr_reader: None,
            state: ProcessState::Starting,
            restart_state: None,
        };
        assert!(!process.is_running());
        assert_eq!(process.state, ProcessState::Stopped);
    }

    #[test]
    fn test_process_state_crashed_after_failure() {
        let mut child = Command::new("false").spawn().unwrap();
        let pid = child.id();
        let _ = child.wait();
        let mut process = Process {
            id: ProcessId::new(1),
            pid,
            program_name: "false".to_string(),
            label: "test".to_string(),
            terminate_on_exit: false,
            config: Arc::new(ProcessConfig::builder().command("false".to_string()).build()),
            child: Some(child),
            stdout_reader: None,
            stderr_reader: None,
            state: ProcessState::Starting,
            restart_state: None,
        };
        let state = process.state();
        assert_eq!(state, ProcessState::Crashed);
        assert!(!process.is_running());
    }

    #[test]
    fn test_process_send_signal_sigterm() {
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let mut process = Process {
            id: ProcessId::new(1),
            pid,
            program_name: "sleep".to_string(),
            label: "test".to_string(),
            terminate_on_exit: false,
            config: Arc::new(ProcessConfig::builder().command("sleep".to_string()).build()),
            child: Some(child),
            stdout_reader: None,
            stderr_reader: None,
            state: ProcessState::Starting,
            restart_state: None,
        };
        let result = process.send_signal(KillSignal::Sigterm);
        assert!(result.is_ok());
        let _ = process.child.as_mut().unwrap().wait();
    }

    #[test]
    fn test_process_force_kill() {
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let mut process = Process {
            id: ProcessId::new(1),
            pid,
            program_name: "sleep".to_string(),
            label: "test".to_string(),
            terminate_on_exit: false,
            config: Arc::new(ProcessConfig::builder().command("sleep".to_string()).build()),
            child: Some(child),
            stdout_reader: None,
            stderr_reader: None,
            state: ProcessState::Starting,
            restart_state: None,
        };
        let result = process.force_kill();
        assert!(result.is_ok());
        let _ = process.child.as_mut().unwrap().wait();
    }
}
