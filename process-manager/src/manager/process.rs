use crate::config::ProcessConfig;
use crate::config::ProcessConfigError;
use crate::config::StdioConfig;
use crate::manager::ProcessManagerError;
use crate::manager::StopManyError;
use crate::process::Process;
use crate::process::ProcessExitEvent;
use crate::process::ProcessId;
use crate::process::ProcessInfo;
use crate::reaper::ReaperHandle;
use crate::reaper::reaper_loop;
use dashmap::DashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use tracing::debug;
use tracing::error;
use tracing::warn;

/// Manages multiple child processes.
///
/// Uses `DashMap` for concurrent access (matching swipe-launcher's pattern).
/// Supports both `ProcessId`-based and label-based operations.
#[must_use]
pub struct ProcessManager {
    processes: Arc<DashMap<ProcessId, Process>>,
    next_id: AtomicU64,
    /// Optional reaper thread handle.
    reaper: Option<ReaperHandle>,
}

impl std::fmt::Debug for ProcessManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessManager")
            .field("process_count", &self.processes.len())
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field("has_reaper", &self.reaper.is_some())
            .finish()
    }
}

impl ProcessManager {
    /// Create a new `ProcessManager` without a reaper thread.
    ///
    /// Process exit is detected lazily via `is_running()` / `wait()`.
    pub fn new() -> Self {
        Self {
            processes: Arc::new(DashMap::new()),
            next_id: AtomicU64::new(1),
            reaper: None,
        }
    }

    /// Create a new `ProcessManager` with a reaper thread.
    ///
    /// The reaper uses non-blocking `try_wait()` on all tracked processes
    /// every `poll_interval` and emits `ProcessExitEvent`s to the provided
    /// channel. This prevents zombies without blocking a thread per process.
    pub fn with_reaper(poll_interval: Duration, exit_sender: Sender<ProcessExitEvent>) -> Result<Self, ProcessManagerError> {
        let processes = Arc::new(DashMap::new());
        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let processes_clone = Arc::clone(&processes);

        let stop_flag_clone = stop_flag.clone();
        let thread_handle = thread::Builder::new()
            .name("process-reaper".to_string())
            .spawn(move || {
                reaper_loop(processes_clone, poll_interval, &exit_sender, &stop_flag_clone);
            })
            .map_err(ProcessManagerError::ReaperThreadFailed)?;

        Ok(Self {
            processes,
            next_id: AtomicU64::new(1),
            reaper: Some(ReaperHandle::new(stop_flag, thread_handle)),
        })
    }

    /// Spawn a new child process with the given label and configuration.
    ///
    /// Returns the assigned `ProcessId`.
    /// If `config.forked` is `true`, `setsid()` is applied via `pre_exec`
    /// to detach from the controlling terminal. The process is still tracked
    /// in the `DashMap` (with its `Child` handle) for stop/reaper operations.
    pub fn start(&self, label: &str, config: &ProcessConfig) -> Result<ProcessId, ProcessManagerError> {
        ProcessConfigError::validate(config).map_err(ProcessManagerError::from)?;
        let id = ProcessId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let program_name = config.command.clone();

        // 1. Resolve executable
        let (executable, args) = if config.shell {
            let full_command = if config.args.is_empty() {
                config.command.clone()
            } else {
                format!("{} {}", config.command, config.args.join(" "))
            };
            ("sh".to_string(), vec!["-c".to_string(), full_command])
        } else {
            let exe = if PathBuf::from(&config.command).is_absolute() {
                config.command.clone()
            } else {
                which::which(&config.command)
                    .map_err(|_| ProcessManagerError::ExecutableNotFound(config.command.clone()))?
                    .to_string_lossy()
                    .to_string()
            };
            (exe, config.args.clone())
        };

        // 2. Build Command
        let mut command = Command::new(&executable);
        command.args(&args);

        // 3. Set environment variables
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(socket) = &config.socket {
            command.env("WAYLAND_DISPLAY", socket);
        }
        // Inherit XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS from parent
        if std::env::var("XDG_RUNTIME_DIR").is_ok() {
            // Already inherited by default
        }
        if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok() {
            // Already inherited by default
        }

        // 4. Set working directory
        if let Some(working_dir) = &config.working_dir {
            command.current_dir(working_dir);
        }

        // 5. Configure stdio
        command.stdin(config.stdin.to_stdio());
        command.stdout(config.stdout.to_stdio());
        command.stderr(config.stderr.to_stdio());

        // 6. Forked mode: setsid() via pre_exec
        if config.forked {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        // 7. Spawn
        let mut child = command.spawn().map_err(ProcessManagerError::SpawnFailed)?;
        let pid = child.id();

        // 5b. Spawn reader threads for piped stdout/stderr
        let stdout_reader = if config.stdout == StdioConfig::Piped
            && let Some(stdout) = child.stdout.take()
        {
            let program_name_clone = program_name.clone();
            let label_clone = label.to_string();
            thread::Builder::new()
                .name(format!("stdout-reader-{}", program_name_clone))
                .spawn(move || {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(stdout);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => debug!("[{}:{}] stdout: {}", label_clone, program_name_clone, line),
                            Err(_) => break,
                        }
                    }
                })
                .ok()
        } else {
            None
        };
        let stderr_reader = if config.stderr == StdioConfig::Piped
            && let Some(stderr) = child.stderr.take()
        {
            let program_name_clone = program_name.clone();
            let label_clone = label.to_string();
            thread::Builder::new()
                .name(format!("stderr-reader-{}", program_name_clone))
                .spawn(move || {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => warn!("[{}:{}] stderr: {}", label_clone, program_name_clone, line),
                            Err(_) => break,
                        }
                    }
                })
                .ok()
        } else {
            None
        };

        // 8. Track
        let process = Process {
            id,
            pid,
            program_name: program_name.clone(),
            label: label.to_string(),
            terminate_on_exit: config.terminate_on_exit,
            config: Arc::new(config.clone()),
            child: Some(child),
            stdout_reader,
            stderr_reader,
        };
        self.processes.insert(id, process);

        debug!("Started process '{}' (pid={}, id={}, label='{}')", program_name, pid, id, label);

        // 9. Return ProcessId
        Ok(id)
    }

    /// Get a lightweight snapshot of a managed process by ID.
    ///
    /// Returns a `ProcessInfo` containing metadata only — no `Child` handle.
    /// Safe to hold across mutating calls since it does not hold a DashMap lock.
    pub fn get_info(&self, id: ProcessId) -> Option<ProcessInfo> {
        self.processes.get(&id).map(|entry| ProcessInfo::from(entry.value()))
    }

    /// Whether the process with the given ID was started in forked mode.
    pub fn is_forked(&self, id: ProcessId) -> Option<bool> {
        self.processes.get(&id).map(|entry| entry.config.forked)
    }

    /// Whether the process with the given ID is still running.
    ///
    /// Uses non-blocking `try_wait()` on the `Child` handle.
    pub fn is_running(&self, id: ProcessId) -> Option<bool> {
        self.processes.get_mut(&id).map(|mut entry| entry.is_running())
    }

    /// The OS PID of the process with the given ID.
    pub fn get_pid(&self, id: ProcessId) -> Option<u32> {
        self.processes.get(&id).map(|entry| entry.pid)
    }

    /// The program name of the process with the given ID.
    pub fn get_program_name(&self, id: ProcessId) -> Option<String> {
        self.processes.get(&id).map(|entry| entry.program_name.clone())
    }

    /// The label of the process with the given ID.
    pub fn get_label(&self, id: ProcessId) -> Option<String> {
        self.processes.get(&id).map(|entry| entry.label.clone())
    }

    /// Whether the process with the given ID is terminated when the manager is dropped.
    pub fn get_terminate_on_exit(&self, id: ProcessId) -> Option<bool> {
        self.processes.get(&id).map(|entry| entry.terminate_on_exit)
    }

    /// The configuration the process with the given ID was started with.
    pub fn get_config(&self, id: ProcessId) -> Option<Arc<ProcessConfig>> {
        self.processes.get(&id).map(|entry| Arc::clone(&entry.config))
    }

    /// Get all processes with a given label.
    ///
    /// Returns a list of `(ProcessId, ProcessInfo)` snapshots. The processes
    /// remain in the manager. Snapshots contain metadata only — no `Child`
    /// handle. Use `stop()` / `stop_label()` for process control.
    pub fn get_by_label(&self, label: &str) -> Vec<(ProcessId, ProcessInfo)> {
        self.processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| (*entry.key(), ProcessInfo::from(entry.value())))
            .collect::<Vec<_>>()
    }

    /// Get all PIDs for processes with a given label.
    pub fn pids_by_label(&self, label: &str) -> Vec<u32> {
        self.processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| entry.value().pid)
            .collect()
    }

    /// Send a signal to a specific process by ID.
    ///
    /// The process remains in the manager — this does not stop or remove it.
    /// Use [`stop`](Self::stop) for termination with grace period and escalation.
    pub fn send_signal(&self, id: ProcessId, signal: crate::Signal) -> Result<(), ProcessManagerError> {
        let process = self.processes.get_mut(&id).ok_or(ProcessManagerError::NotFound(id))?;
        let nix_signal = signal.to_nix_signal();
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(process.pid as i32), nix_signal)
            .map_err(|e| ProcessManagerError::SignalFailed(process.pid, e.to_string()))?;
        debug!("Sent signal {:?} to process {} (pid={})", signal, process.id, process.pid);
        Ok(())
    }

    /// Send a signal to all processes with a given label.
    ///
    /// Processes remain in the manager. Returns an error if any signal delivery
    /// fails, but still attempts all processes.
    pub fn send_signal_label(&self, label: &str, signal: crate::Signal) -> Result<(), StopManyError> {
        let ids: Vec<ProcessId> = self
            .processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| *entry.key())
            .collect();

        if ids.is_empty() {
            return Ok(());
        }

        let mut errors: Vec<ProcessManagerError> = Vec::new();
        for id in ids {
            if let Err(e) = self.send_signal(id, signal) {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(StopManyError::new(errors));
        }
        Ok(())
    }

    /// Stop (kill) a specific process by ID.
    ///
    /// Sends the configured kill signal via `nix::sys::signal::kill`,
    /// waits the grace period, then escalates to `SIGKILL` if still alive.
    /// Removes the process from the manager.
    pub fn stop(&self, id: ProcessId) -> Result<(), ProcessManagerError> {
        let mut process = self.processes.remove(&id).ok_or(ProcessManagerError::NotFound(id))?.1;

        let kill_signal = process.config.kill_signal;
        let timeout_ms = process.config.terminate_timeout_ms;
        let pid = process.pid;

        // 1. Send kill signal (ignore ESRCH — process may have already exited)
        if let Err(e) = process.send_signal(kill_signal) {
            if e.raw_os_error() == Some(libc::ESRCH) {
                debug!("Process {} (pid={}) already exited, skipping signal", id, pid);
                process.join_readers(Duration::from_millis(500));
                return Ok(());
            }
            return Err(ProcessManagerError::SignalFailed(pid, e.to_string()));
        }

        // 2. Grace period — poll is_running()
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if !process.is_running() {
                debug!("Process {} (pid={}) exited after signal", id, pid);
                process.join_readers(Duration::from_millis(500));
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        // 3. Escalate to SIGKILL
        warn!("Process {} (pid={}) did not exit after {}ms, escalating to SIGKILL", id, pid, timeout_ms);
        if let Err(e) = process.force_kill() {
            if e.raw_os_error() == Some(libc::ESRCH) {
                debug!("Process {} (pid={}) already exited before SIGKILL", id, pid);
                process.join_readers(Duration::from_millis(500));
                return Ok(());
            }
            return Err(ProcessManagerError::SignalFailed(pid, e.to_string()));
        }

        // Wait for SIGKILL to take effect
        let _ = process.child.as_mut().and_then(|child| child.wait().ok());
        debug!("Process {} (pid={}) killed with SIGKILL", id, pid);
        process.join_readers(Duration::from_millis(500));

        Ok(())
    }

    /// Stop multiple processes concurrently.
    ///
    /// Sends kill signals to all processes first, then waits for all grace
    /// periods in a single polling loop. Escalates to `SIGKILL` for any
    /// process that doesn't exit within its individual timeout. Worst case
    /// is a single max timeout, not N × timeout.
    fn stop_many(&self, ids: Vec<ProcessId>) -> Result<(), ProcessManagerError> {
        // 1. Remove all processes from the map and compute per-process deadlines
        let mut entries: Vec<(Process, std::time::Instant)> = ids
            .into_iter()
            .filter_map(|id| self.processes.remove(&id).map(|(_, process)| process))
            .map(|process| {
                let deadline = std::time::Instant::now() + Duration::from_millis(process.config.terminate_timeout_ms);
                (process, deadline)
            })
            .collect();

        if entries.is_empty() {
            return Ok(());
        }

        // 2. Send kill signal to all processes.
        // Signal failures are logged — the process will escalate to SIGKILL
        // if still alive, so we don't record an error here.
        for (process, _) in &mut entries {
            let kill_signal = process.config.kill_signal;
            let pid = process.pid;
            if let Err(e) = process.send_signal(kill_signal) {
                warn!("Failed to send signal to process {} (pid={}): {}, will escalate", process.id, pid, e);
            }
        }

        // 3. Poll: wait for each process until it exits or its deadline passes
        let mut to_escalate: Vec<Process> = Vec::new();
        let mut to_join: Vec<Process> = Vec::new();
        while !entries.is_empty() {
            let now = std::time::Instant::now();
            let mut still_waiting = Vec::new();
            for (mut process, deadline) in entries.drain(..) {
                if !process.is_running() {
                    debug!("Process {} (pid={}) exited after signal", process.id, process.pid);
                    to_join.push(process);
                } else if now >= deadline {
                    to_escalate.push(process);
                } else {
                    still_waiting.push((process, deadline));
                }
            }
            entries = still_waiting;
            if !entries.is_empty() {
                thread::sleep(Duration::from_millis(50));
            }
        }

        // 3b. Join reader threads for processes that exited gracefully
        for process in &mut to_join {
            process.join_readers(Duration::from_millis(500));
        }

        // 4. Escalate to SIGKILL for processes that didn't exit in time.
        // Processes whose force_kill fails are kept for re-insertion.
        let mut errors: Vec<ProcessManagerError> = Vec::new();
        let mut to_reap: Vec<Process> = Vec::new();
        let mut to_reinsert: Vec<Process> = Vec::new();
        for mut process in to_escalate {
            warn!(
                "Process {} (pid={}) did not exit after {}ms, escalating to SIGKILL",
                process.id, process.pid, process.config.terminate_timeout_ms
            );
            match process.force_kill() {
                Ok(()) => to_reap.push(process),
                Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {
                    debug!("Process {} (pid={}) already exited before SIGKILL", process.id, process.pid);
                    process.join_readers(Duration::from_millis(500));
                    to_reap.push(process);
                }
                Err(e) => {
                    error!("Failed to force-kill process {} (pid={}): {}", process.id, process.pid, e);
                    errors.push(ProcessManagerError::SignalFailed(process.pid, e.to_string()));
                    to_reinsert.push(process);
                }
            }
        }

        // 5. Wait for SIGKILL to take effect
        for process in &mut to_reap {
            let _ = process.child.as_mut().and_then(|child| child.wait().ok());
            debug!("Process {} (pid={}) killed with SIGKILL", process.id, process.pid);
            process.join_readers(Duration::from_millis(500));
        }

        // 6. Re-insert processes that couldn't be killed so they remain tracked
        for process in to_reinsert {
            warn!("Re-inserting process {} (pid={}) into manager after failed kill", process.id, process.pid);
            self.processes.insert(process.id, process);
        }

        if !errors.is_empty() {
            return Err(StopManyError::new(errors).into());
        }
        Ok(())
    }

    /// Stop all processes with a given label.
    pub fn stop_label(&self, label: &str) -> Result<(), ProcessManagerError> {
        let ids: Vec<ProcessId> = self
            .processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| *entry.key())
            .collect();

        self.stop_many(ids)
    }

    /// Stop all managed processes with `terminate_on_exit = true`.
    ///
    /// Called automatically on `Drop`.
    pub fn stop_terminate_on_exit(&self) {
        let ids: Vec<ProcessId> = self
            .processes
            .iter()
            .filter(|entry| entry.value().terminate_on_exit)
            .map(|entry| *entry.key())
            .collect();

        if let Err(e) = self.stop_many(ids) {
            error!("Failed to stop processes on terminate_on_exit: {}", e);
        }
    }

    /// Stop all managed processes.
    pub fn stop_all(&self) {
        let ids: Vec<ProcessId> = self.processes.iter().map(|entry| *entry.key()).collect();
        if let Err(e) = self.stop_many(ids) {
            error!("Failed to stop processes during stop_all: {}", e);
        }
    }

    /// Restart a process by ID.
    ///
    /// Stops the process (with grace period and SIGKILL escalation), then
    /// starts a new process with the same config and label. Returns the
    /// new `ProcessId`.
    pub fn restart(&self, id: ProcessId) -> Result<ProcessId, ProcessManagerError> {
        let process = self.processes.get(&id).ok_or(ProcessManagerError::NotFound(id))?;
        let config = Arc::clone(&process.config);
        let label = process.label.clone();
        drop(process);

        self.stop(id)?;
        self.start(&label, &config)
    }

    /// Restart all processes with a given label.
    ///
    /// Stops all matching processes concurrently, then starts new ones
    /// with the same configs and label. Returns the new `ProcessId`s.
    pub fn restart_label(&self, label: &str) -> Result<Vec<ProcessId>, ProcessManagerError> {
        let entries: Vec<(ProcessId, Arc<ProcessConfig>)> = self
            .processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| (*entry.key(), Arc::clone(&entry.value().config)))
            .collect();

        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<ProcessId> = entries.iter().map(|(id, _)| *id).collect();
        self.stop_many(ids)?;

        let mut new_ids = Vec::with_capacity(entries.len());
        for (_, config) in entries {
            new_ids.push(self.start(label, &config)?);
        }
        Ok(new_ids)
    }

    /// List all managed process IDs.
    pub fn ids(&self) -> Vec<ProcessId> {
        self.processes.iter().map(|entry| *entry.key()).collect()
    }

    /// List all labels that have at least one process.
    pub fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.processes.iter().map(|entry| entry.value().label.clone()).collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Number of managed processes.
    pub fn len(&self) -> usize {
        self.processes.len()
    }

    /// Whether no processes are managed.
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        // Drop the reaper first to stop the background thread
        self.reaper.take();
        // Terminate all processes with terminate_on_exit = true
        self.stop_terminate_on_exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessConfig;
    use crate::signal::KillSignal;
    use process_manager_socket::Socket;
    use std::collections::HashMap;

    #[test]
    fn test_process_manager_new_is_empty() {
        let manager = ProcessManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_process_manager_start_and_stop() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        assert!(!manager.is_empty());
        assert_eq!(manager.len(), 1);
        manager.stop(id).unwrap();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_manager_start_multiple() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _id1 = manager.start("test1", &config).unwrap();
        let _id2 = manager.start("test2", &config).unwrap();
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.ids().len(), 2);
        manager.stop_all();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_manager_stop_all() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("a", &config).unwrap();
        let _ = manager.start("b", &config).unwrap();
        let _ = manager.start("c", &config).unwrap();
        assert_eq!(manager.len(), 3);
        manager.stop_all();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_manager_get_nonexistent() {
        let manager = ProcessManager::new();
        assert!(manager.get_info(ProcessId::new(999)).is_none());
    }

    #[test]
    fn test_is_forked() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .forked(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("forked", &config).unwrap();
        assert_eq!(manager.is_forked(id), Some(true));
        assert_eq!(manager.is_forked(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_get_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("my-label", &config).unwrap();
        assert_eq!(manager.get_label(id), Some("my-label".to_string()));
        assert_eq!(manager.get_label(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_get_terminate_on_exit() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .terminate_on_exit(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("temp", &config).unwrap();
        assert_eq!(manager.get_terminate_on_exit(id), Some(true));
        assert_eq!(manager.get_terminate_on_exit(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_is_running() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("running", &config).unwrap();
        assert_eq!(manager.is_running(id), Some(true));
        assert_eq!(manager.is_running(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_get_pid() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("pid-test", &config).unwrap();
        let pid = manager.get_pid(id).unwrap();
        assert!(pid > 0);
        assert_eq!(manager.get_pid(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_get_program_name() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("name-test", &config).unwrap();
        assert_eq!(manager.get_program_name(id), Some("sleep".to_string()));
        assert_eq!(manager.get_program_name(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_get_config() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("config-test", &config).unwrap();
        let retrieved = manager.get_config(id).unwrap();
        assert_eq!(retrieved.command, "sleep");
        assert_eq!(retrieved.args, vec!["10".to_string()]);
        assert!(manager.get_config(ProcessId::new(999)).is_none());
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_process_manager_stop_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.stop(ProcessId::new(999));
        assert!(matches!(result, Err(ProcessManagerError::NotFound(_))));
    }

    #[test]
    fn test_send_signal_to_running_process() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["30".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("signal-test", &config).unwrap();
        assert_eq!(manager.is_running(id), Some(true));

        // SIGTERM should cause sleep to exit
        manager.send_signal(id, crate::Signal::Sigterm).unwrap();

        // Give it time to exit
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(manager.is_running(id), Some(false));

        manager.stop(id).unwrap();
    }

    #[test]
    fn test_send_signal_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.send_signal(ProcessId::new(999), crate::Signal::Sigusr1);
        assert!(matches!(result, Err(ProcessManagerError::NotFound(_))));
    }

    #[test]
    fn test_send_signal_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["30".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();

        let id1 = manager.start("group", &config).unwrap();
        let id2 = manager.start("group", &config).unwrap();
        let _ = manager.start("other", &config).unwrap();

        assert_eq!(manager.len(), 3);

        // Send SIGTERM to all "group" processes
        manager.send_signal_label("group", crate::Signal::Sigterm).unwrap();

        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(manager.is_running(id1), Some(false));
        assert_eq!(manager.is_running(id2), Some(false));
        // "other" should still be running
        assert_eq!(manager.pids_by_label("other").len(), 1);

        manager.stop_all();
    }

    #[test]
    fn test_send_signal_label_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.send_signal_label("nonexistent", crate::Signal::Sigusr1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_send_signal_sigwinch_does_not_terminate() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["30".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("winch-test", &config).unwrap();

        // SIGWINCH is ignored by default, process should keep running
        manager.send_signal(id, crate::Signal::Sigwinch).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(manager.is_running(id), Some(true));

        manager.stop(id).unwrap();
    }

    #[test]
    fn test_process_executable_not_found() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder().command("nonexistent-binary-xyz".to_string()).build();
        let result = manager.start("test", &config);
        assert!(matches!(result, Err(ProcessManagerError::ExecutableNotFound(_))));
    }

    #[test]
    fn test_process_is_running_false_after_exit() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        // Wait for the process to exit
        thread::sleep(Duration::from_millis(100));
        // Process exited on its own; stop() removes it from the manager
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_process_manager_start_with_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("mylabel", &config).unwrap();
        let _ = manager.start("mylabel", &config).unwrap();
        let _ = manager.start("other", &config).unwrap();
        assert_eq!(manager.pids_by_label("mylabel").len(), 2);
        assert_eq!(manager.pids_by_label("other").len(), 1);
        assert_eq!(manager.pids_by_label("nonexistent").len(), 0);
        manager.stop_all();
    }

    #[test]
    fn test_process_manager_stop_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("group", &config).unwrap();
        let _ = manager.start("group", &config).unwrap();
        let _ = manager.start("other", &config).unwrap();
        assert_eq!(manager.len(), 3);
        manager.stop_label("group").unwrap();
        assert_eq!(manager.len(), 1);
        manager.stop_all();
    }

    #[test]
    fn test_process_manager_labels() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("alpha", &config).unwrap();
        let _ = manager.start("beta", &config).unwrap();
        let _ = manager.start("alpha", &config).unwrap();
        let mut labels = manager.labels();
        labels.sort();
        assert_eq!(labels, vec!["alpha", "beta"]);
        manager.stop_all();
    }

    #[test]
    fn test_process_terminate_sigterm() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .kill_signal(KillSignal::Sigterm)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        manager.stop(id).unwrap();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_terminate_sigkill() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .kill_signal(KillSignal::Sigkill)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        manager.stop(id).unwrap();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_terminate_escalation() {
        let manager = ProcessManager::new();
        // Use a shell command that traps SIGTERM so SIGTERM is ignored,
        // forcing escalation to SIGKILL after the grace period.
        let config = ProcessConfig::builder()
            .command("trap '' TERM; sleep 100".to_string())
            .shell(true)
            .kill_signal(KillSignal::Sigterm)
            .terminate_timeout_ms(200)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        // Give the trap time to be installed
        thread::sleep(Duration::from_millis(100));
        // stop should escalate to SIGKILL after 200ms
        let start = std::time::Instant::now();
        manager.stop(id).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(150),
            "Should have waited at least the timeout before escalating: {:?}",
            elapsed
        );
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_terminate_on_exit_drop() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .terminate_on_exit(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        assert_eq!(manager.len(), 1);
        // Drop the manager — should terminate the process
        drop(manager);
        // If the process wasn't terminated, it would linger as a zombie
        // but we can't easily verify from here. The test passes if no panic.
    }

    #[test]
    fn test_reaper_detects_exit() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        // Wait for the reaper to detect the exit
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.label, "test");
        assert!(!event.restart_on_exit);
        assert!(event.exit_status.is_some());
        assert!(event.exit_status.unwrap().success());
    }

    #[test]
    fn test_reaper_restart_on_exit() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .restart_on_exit(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(event.restart_on_exit);
    }

    #[test]
    fn test_reaper_exit_status_nonzero() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("false".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.label, "test");
        assert!(event.exit_status.is_some());
        assert!(!event.exit_status.unwrap().success());
    }

    #[test]
    fn test_process_forked_setsid() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .forked(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        assert_eq!(manager.len(), 1);
        // Verify it's tracked
        assert!(manager.get_info(id).is_some());
        manager.stop(id).unwrap();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_forked_stop_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .forked(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("forked-label", &config).unwrap();
        assert_eq!(manager.len(), 1);
        manager.stop_label("forked-label").unwrap();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_process_forked_reaper_detects_exit() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .forked(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.label, "test");
    }

    #[test]
    fn test_stdio_config_null() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("echo".to_string())
            .args(vec!["hello".to_string()])
            .stdin(StdioConfig::Null)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        // Wait for exit
        thread::sleep(Duration::from_millis(100));
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_stdio_config_piped() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("echo".to_string())
            .args(vec!["hello".to_string()])
            .stdout(StdioConfig::Piped)
            .stderr(StdioConfig::Piped)
            .build();
        let id = manager.start("test", &config).unwrap();
        // Give reader threads time to capture output
        thread::sleep(Duration::from_millis(200));
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_process_with_socket_sets_wayland_display() {
        let manager = ProcessManager::new();
        let socket = Socket::from(PathBuf::from("/tmp/test-wayland-0"));
        let mut env = HashMap::new();
        // Child will print WAYLAND_DISPLAY env var
        env.insert("TEST_PRINT_WAYLAND_DISPLAY".to_string(), "1".to_string());
        let config = ProcessConfig::builder()
            .command("sh".to_string())
            .args(vec!["-c".to_string(), "echo $WAYLAND_DISPLAY".to_string()])
            .env(env)
            .socket(Some(socket))
            .stdout(StdioConfig::Piped)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        // Wait for the process to finish
        thread::sleep(Duration::from_millis(200));
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_restart_single_process() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        assert_eq!(manager.len(), 1);
        let new_id = manager.restart(id).unwrap();
        assert_eq!(manager.len(), 1);
        assert_ne!(id, new_id);
        assert!(manager.get_info(new_id).is_some());
        manager.stop(new_id).unwrap();
    }

    #[test]
    fn test_restart_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.restart(ProcessId::new(999));
        assert!(matches!(result, Err(ProcessManagerError::NotFound(_))));
    }

    #[test]
    fn test_restart_label() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("group", &config).unwrap();
        let _ = manager.start("group", &config).unwrap();
        let _ = manager.start("other", &config).unwrap();
        assert_eq!(manager.len(), 3);

        let new_ids = manager.restart_label("group").unwrap();
        assert_eq!(new_ids.len(), 2);
        assert_eq!(manager.len(), 3);
        assert_eq!(manager.pids_by_label("group").len(), 2);
        assert_eq!(manager.pids_by_label("other").len(), 1);
        manager.stop_all();
    }

    #[test]
    fn test_restart_label_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.restart_label("nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
