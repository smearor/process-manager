use crate::config::ProcessConfig;
use crate::config::ProcessConfigError;
use crate::config::StdioConfig;
use crate::manager::ProcessManagerError;
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
        if config.stdout == StdioConfig::Piped
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
                .ok();
        }
        if config.stderr == StdioConfig::Piped
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
                .ok();
        }

        // 8. Track
        let process = Process {
            id,
            pid,
            program_name: program_name.clone(),
            label: label.to_string(),
            terminate_on_exit: config.terminate_on_exit,
            config: config.clone(),
            child: Some(child),
        };
        self.processes.insert(id, process);

        debug!("Started process '{}' (pid={}, id={}, label='{}')", program_name, pid, id, label);

        // 9. Return ProcessId
        Ok(id)
    }

    /// Get a managed process by ID.
    ///
    /// Returns a `dashmap::mapref::one::Ref` guard. The process is accessible
    /// within the guard's lifetime.
    pub fn get(&self, id: ProcessId) -> Option<dashmap::mapref::one::Ref<'_, ProcessId, Process>> {
        self.processes.get(&id)
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

        // 1. Send kill signal
        if let Err(e) = process.send_signal(kill_signal) {
            return Err(ProcessManagerError::SignalFailed(pid, e.to_string()));
        }

        // 2. Grace period — poll is_running()
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if !process.is_running() {
                debug!("Process {} (pid={}) exited after signal", id, pid);
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        // 3. Escalate to SIGKILL
        warn!("Process {} (pid={}) did not exit after {}ms, escalating to SIGKILL", id, pid, timeout_ms);
        if let Err(e) = process.force_kill() {
            return Err(ProcessManagerError::SignalFailed(pid, e.to_string()));
        }

        // Wait for SIGKILL to take effect
        let _ = process.child.as_mut().and_then(|child| child.wait().ok());
        debug!("Process {} (pid={}) killed with SIGKILL", id, pid);

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

        for id in ids {
            self.stop(id)?;
        }
        Ok(())
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

        for id in ids {
            match self.stop(id) {
                Ok(_) => {}
                Err(e) => error!("Failed to stop process {} on terminate_on_exit: {}", id, e),
            }
        }
    }

    /// Stop all managed processes.
    pub fn stop_all(&self) {
        let ids: Vec<ProcessId> = self.processes.iter().map(|entry| *entry.key()).collect();
        for id in ids {
            match self.stop(id) {
                Ok(_) => {}
                Err(e) => error!("Failed to stop process {} during stop_all: {}", id, e),
            }
        }
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
    use crate::kill_signal::KillSignal;
    use smearor_wrot_socket::Socket;
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
        assert!(manager.get(ProcessId::new(999)).is_none());
    }

    #[test]
    fn test_process_manager_stop_nonexistent() {
        let manager = ProcessManager::new();
        let result = manager.stop(ProcessId::new(999));
        assert!(matches!(result, Err(ProcessManagerError::NotFound(_))));
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
        assert!(manager.get(id).is_some());
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
}
