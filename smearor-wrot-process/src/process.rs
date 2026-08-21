use crate::config::ProcessConfig;
use crate::kill_signal::KillSignal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::process::Child;
use std::process::ExitStatus;

/// Unique identifier for a managed process.
///
/// Assigned by `ProcessManager` using an `AtomicU64` counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub u64);

impl ProcessId {
    /// Create a new `ProcessId` from a raw `u64`.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the raw `u64` value.
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A managed child process.
///
/// Represents a running or completed child process tracked by `ProcessManager`.
/// The `child` handle is always `Some` — even forked processes (with `setsid()`)
/// are tracked with their `Child` handle for stop/reaper operations.
#[derive(Debug)]
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
    pub config: ProcessConfig,

    /// The child process handle. Always `Some` — forked processes are also
    /// tracked (with `setsid()` applied via `pre_exec`).
    pub child: Option<Child>,
}

impl Process {
    /// Whether the child process is still running.
    ///
    /// Uses non-blocking `try_wait()` on the `Child` handle for all processes
    /// (forked and non-forked). No `/proc` polling needed since the `Child`
    /// handle is always available.
    pub fn is_running(&mut self) -> bool {
        match &mut self.child {
            Some(child) => match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            },
            None => false,
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
        let nix_signal = signal.to_signal();
        nix::sys::signal::kill(Pid::from_raw(self.pid as i32), nix_signal).map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Force-kill the child process with `SIGKILL`.
    ///
    /// This is the escalation step after `SIGTERM` + grace period.
    /// Uses `nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGKILL)`.
    pub fn force_kill(&mut self) -> std::io::Result<()> {
        nix::sys::signal::kill(Pid::from_raw(self.pid as i32), Signal::SIGKILL).map_err(|e| std::io::Error::other(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_process_id_new() {
        let id = ProcessId::new(42);
        assert_eq!(id.raw(), 42);
    }

    #[test]
    fn test_process_id_display() {
        let id = ProcessId::new(7);
        assert_eq!(format!("{}", id), "7");
    }

    #[test]
    fn test_process_id_equality() {
        let a = ProcessId::new(1);
        let b = ProcessId::new(1);
        let c = ProcessId::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

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
            config: ProcessConfig::builder().command("sleep".to_string()).build(),
            child: Some(child),
        };
        assert!(process.is_running());
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
            config: ProcessConfig::builder().command("true".to_string()).build(),
            child: Some(child),
        };
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
            config: ProcessConfig::builder().command("sleep".to_string()).build(),
            child: Some(child),
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
            config: ProcessConfig::builder().command("sleep".to_string()).build(),
            child: Some(child),
        };
        let result = process.force_kill();
        assert!(result.is_ok());
        let _ = process.child.as_mut().unwrap().wait();
    }
}
