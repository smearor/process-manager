use crate::config::Label;
use crate::config::ProcessConfig;
use crate::config::ProcessConfigError;
use crate::config::StdioConfig;
use crate::manager::ProcessManagerError;
use crate::manager::SpawnResult;
use crate::manager::StopManyError;
use crate::manager::all_deps_running;
use crate::manager::build_dependency_snapshot;
use crate::manager::detect_cycle;
use crate::manager::resolve_dependencies;
use crate::process::Process;
use crate::process::ProcessExitEvent;
use crate::process::ProcessId;
use crate::process::ProcessInfo;
use crate::process::ProcessState;
use crate::process::RestartState;
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
use std::time::Instant;
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
    next_spawn_sequence: AtomicU64,
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
            next_spawn_sequence: AtomicU64::new(1),
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
        let exit_sender_clone = exit_sender.clone();
        let thread_handle = thread::Builder::new()
            .name("process-reaper".to_string())
            .spawn(move || {
                reaper_loop(processes_clone, poll_interval, &exit_sender, &stop_flag_clone);
            })
            .map_err(ProcessManagerError::ReaperThreadFailed)?;

        Ok(Self {
            processes,
            next_id: AtomicU64::new(1),
            next_spawn_sequence: AtomicU64::new(1),
            reaper: Some(ReaperHandle::new(stop_flag, thread_handle, exit_sender_clone)),
        })
    }

    /// Spawn a new child process with the given label and configuration.
    ///
    /// Returns the assigned `ProcessId`.
    /// If `config.forked` is `true`, `setsid()` is applied via `pre_exec`
    /// to detach from the controlling terminal. The process is still tracked
    /// in the `DashMap` (with its `Child` handle) for stop/reaper operations.
    ///
    /// If `config.depends_on` is non-empty and one or more dependencies are
    /// not yet `Running`, the process is inserted with `ProcessState::Waiting`
    /// and spawned asynchronously by the reaper once dependencies become
    /// `Running`. The `ProcessId` is assigned immediately and returned.
    pub fn start<L: Into<Label>>(&self, label: L, config: &ProcessConfig) -> Result<ProcessId, ProcessManagerError> {
        let label = label.into();
        ProcessConfigError::validate(config).map_err(ProcessManagerError::from)?;
        let id = ProcessId::new(self.next_id.fetch_add(1, Ordering::Relaxed));

        // Cycle detection: build a snapshot of the dependency graph and check
        // for cycles before inserting anything into the DashMap.
        if !config.depends_on.is_empty() {
            let graph = build_dependency_snapshot(&self.processes, &label, &config.depends_on);
            detect_cycle(&graph, &label, &config.depends_on)?;
        }

        // Resolve dependencies (label -> ProcessId binding)
        let resolved_deps = if config.depends_on.is_empty() {
            Vec::new()
        } else {
            // Try to resolve - if any label is not found (no Running process),
            // we proceed with Waiting state. The reaper will re-resolve later.
            resolve_dependencies(&self.processes, &config.depends_on).unwrap_or_default()
        };

        // Check if all dependencies are Running
        let deps_ready = resolved_deps.len() == config.depends_on.len() && all_deps_running(&self.processes, &resolved_deps);

        if deps_ready || config.depends_on.is_empty() {
            // All deps Running (or no deps) - spawn immediately
            let spawn = self.spawn_internal(&label, config)?;
            let pid = spawn.child.id();
            let process = Process {
                id,
                pid,
                program_name: config.command.clone(),
                label: label.clone(),
                terminate_on_exit: config.terminate_on_exit,
                config: Arc::new(config.clone()),
                child: Some(spawn.child),
                stdout_reader: spawn.stdout_reader,
                stderr_reader: spawn.stderr_reader,
                state: ProcessState::Starting,
                restart_state: if config.restart_on_exit { Some(RestartState::new()) } else { None },
                spawn_sequence: self.next_spawn_sequence.fetch_add(1, Ordering::Relaxed),
                cascade_flag: false,
                resolved_deps,
                waiting_since: None,
            };
            self.processes.insert(id, process);

            // Mark started for restart tracking
            if let Some(mut entry) = self.processes.get_mut(&id)
                && let Some(restart_state) = &mut entry.restart_state
            {
                restart_state.mark_started(Instant::now());
            }

            debug!("Started process '{}' (pid={}, id={}, label='{}')", config.command, pid, id, label);
        } else {
            // Deps not ready - insert Waiting placeholder
            let unresolved_count = config.depends_on.len() - resolved_deps.len();
            let process = Process {
                id,
                pid: 0,
                program_name: config.command.clone(),
                label: label.clone(),
                terminate_on_exit: config.terminate_on_exit,
                config: Arc::new(config.clone()),
                child: None,
                stdout_reader: None,
                stderr_reader: None,
                state: ProcessState::Waiting,
                restart_state: if config.restart_on_exit { Some(RestartState::new()) } else { None },
                spawn_sequence: self.next_spawn_sequence.fetch_add(1, Ordering::Relaxed),
                cascade_flag: false,
                resolved_deps,
                waiting_since: Some(Instant::now()),
            };
            self.processes.insert(id, process);
            debug!(
                "Process '{}' (id={}, label='{}') inserted in Waiting state ({} deps not ready)",
                config.command, id, label, unresolved_count
            );
        }

        Ok(id)
    }

    /// Internal helper: spawn a child process from config and label.
    ///
    /// Returns the `Child`, stdout reader handle, and stderr reader handle.
    /// Does not insert into the `DashMap` or assign a `ProcessId`.
    fn spawn_internal(&self, label: &Label, config: &ProcessConfig) -> Result<SpawnResult, ProcessManagerError> {
        ProcessConfigError::validate(config).map_err(ProcessManagerError::from)?;
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

        // 8. Spawn reader threads for piped stdout/stderr
        let stdout_reader = if config.stdout == StdioConfig::Piped
            && let Some(stdout) = child.stdout.take()
        {
            let program_name_clone = program_name.clone();
            let label_clone = label.clone();
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
            let label_clone = label.clone();
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

        Ok(SpawnResult {
            child,
            stdout_reader,
            stderr_reader,
        })
    }

    /// Get a lightweight snapshot of a managed process by ID.
    ///
    /// Returns a `ProcessInfo` containing metadata only - no `Child` handle.
    /// Safe to hold across mutating calls since it does not hold a DashMap lock.
    pub fn get_info(&self, id: ProcessId) -> Option<ProcessInfo> {
        self.processes.get_mut(&id).map(|mut entry| {
            let _ = entry.state();
            ProcessInfo::from(entry.value())
        })
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

    /// The current lifecycle state of the process with the given ID.
    ///
    /// Performs a non-blocking `try_wait()` to detect whether the process
    /// has exited since the last check. Returns `None` if the process is not
    /// found.
    pub fn state(&self, id: ProcessId) -> Option<ProcessState> {
        self.processes.get_mut(&id).map(|mut entry| entry.state())
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
    pub fn get_label(&self, id: ProcessId) -> Option<Label> {
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
    /// remain in the manager. Snapshots contain metadata only - no `Child`
    /// handle. Use `stop()` / `stop_label()` for process control.
    pub fn get_by_label<L: Into<Label>>(&self, label: L) -> Vec<(ProcessId, ProcessInfo)> {
        let label = label.into();
        self.processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| (*entry.key(), ProcessInfo::from(entry.value())))
            .collect::<Vec<_>>()
    }

    /// Get all PIDs for processes with a given label.
    pub fn pids_by_label<L: Into<Label>>(&self, label: L) -> Vec<u32> {
        let label = label.into();
        self.processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| entry.value().pid)
            .collect()
    }

    /// Send a signal to a specific process by ID.
    ///
    /// The process remains in the manager - this does not stop or remove it.
    /// Use [`stop`](Self::stop) for termination with grace period and escalation.
    pub fn send_signal(&self, id: ProcessId, signal: crate::Signal) -> Result<(), ProcessManagerError> {
        let process = self.processes.get_mut(&id).ok_or(ProcessManagerError::NotFound(id))?;
        if process.state == ProcessState::Restarting {
            return Err(ProcessManagerError::ProcessInRestartingState(id));
        }
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
    pub fn send_signal_label<L: Into<Label>>(&self, label: L, signal: crate::Signal) -> Result<(), StopManyError> {
        let label = label.into();
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
        // Check if process is in Restarting state - just remove, no signal, no event
        {
            let process = self.processes.get(&id).ok_or(ProcessManagerError::NotFound(id))?;
            if process.state == ProcessState::Restarting {
                debug!("Process {} is in Restarting state, cancelling backoff and removing (no signal, no event)", id);
                drop(process);
                self.processes.remove(&id);
                return Ok(());
            }

            // Check if process is in Waiting state - remove without signal, emit Stopped event
            if process.state == ProcessState::Waiting {
                debug!("Process {} is in Waiting state, removing without signal, emitting Stopped event", id);
                let label = process.label.clone();
                let pid = process.pid;
                let restart_on_exit = process.config.restart_on_exit;
                let cascade_stop = process.config.cascade_stop;
                drop(process);
                self.processes.remove(&id);

                // Emit ProcessExitEvent with state: Stopped
                // The caller received a ProcessId and must be able to track
                // its lifecycle end, even if no OS process was ever spawned.
                if let Some(reaper) = &self.reaper {
                    let event = ProcessExitEvent {
                        id,
                        label,
                        pid,
                        restart_on_exit,
                        exit_status: None,
                        state: ProcessState::Stopped,
                    };
                    if let Err(e) = reaper.try_send_exit_event(event) {
                        warn!("Failed to send exit event for Waiting process {}: {}", id, e);
                    }
                }

                // Cascade stop dependents if configured
                if cascade_stop {
                    let dependents = self.dependents(id);
                    if !dependents.is_empty() {
                        debug!("Process {} (Waiting) has cascade_stop=true, stopping {} dependent(s)", id, dependents.len());
                        for dep_id in dependents {
                            if let Err(e) = self.stop(dep_id) {
                                warn!("Failed to cascade-stop dependent {}: {}", dep_id, e);
                            }
                        }
                    }
                }

                return Ok(());
            }
        }

        let mut process = self.processes.remove(&id).ok_or(ProcessManagerError::NotFound(id))?.1;

        let kill_signal = process.config.kill_signal;
        let timeout_ms = process.config.terminate_timeout_ms;
        let pid = process.pid;
        let cascade_stop = process.config.cascade_stop;

        // 0. Mark as Stopping
        process.state = ProcessState::Stopping;

        // 1. Send kill signal (ignore ESRCH - process may have already exited)
        if let Err(e) = process.send_signal(kill_signal) {
            if e.raw_os_error() == Some(libc::ESRCH) {
                debug!("Process {} (pid={}) already exited, skipping signal", id, pid);
                process.join_readers(Duration::from_millis(500));
                return Ok(());
            }
            return Err(ProcessManagerError::SignalFailed(pid, e.to_string()));
        }

        // 2. Grace period - poll is_running()
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() < deadline {
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

        // Wait for SIGKILL to take effect (bounded - process may be in D state)
        let kill_deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < kill_deadline {
            if process.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_some()) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if process.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_none()) {
            warn!("Process {} (pid={}) did not exit within 500ms after SIGKILL (may be in D state)", id, pid);
        }
        debug!("Process {} (pid={}) killed with SIGKILL", id, pid);
        process.join_readers(Duration::from_millis(500));

        // Cascade stop: if this process has cascade_stop = true, stop all
        // processes that depend on it (direct dependents via resolved_deps).
        if cascade_stop {
            let dependents = self.dependents(id);
            if !dependents.is_empty() {
                debug!("Process {} has cascade_stop=true, stopping {} dependent(s)", id, dependents.len());
                for dep_id in dependents {
                    // Recursively stop dependents - each dependent's cascade_stop
                    // is also checked, propagating the cascade through the chain.
                    if let Err(e) = self.stop(dep_id) {
                        warn!("Failed to cascade-stop dependent {}: {}", dep_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Stop multiple processes concurrently.
    ///
    /// Sends kill signals to all processes first, then waits for all grace
    /// periods in a single polling loop. Escalates to `SIGKILL` for any
    /// process that doesn't exit within its individual timeout. Worst case
    /// is a single max timeout, not N × timeout.
    fn stop_many(&self, ids: Vec<ProcessId>) -> Result<(), ProcessManagerError> {
        // 1. Remove all processes from the map.
        // Separate processes that have a real OS child (pid != 0) from
        // Waiting/Failed processes that were never spawned (pid == 0).
        // Sending a signal to pid 0 would broadcast it to the entire
        // process group, which is never intended.
        let mut entries: Vec<(Process, Instant)> = Vec::new();
        let mut no_signal: Vec<Process> = Vec::new();

        for id in ids {
            let Some((_, process)) = self.processes.remove(&id) else { continue };
            if process.pid == 0 {
                debug!("Process {} has pid=0 (Waiting/Failed), skipping signal in stop_many", process.id);
                no_signal.push(process);
            } else {
                let deadline = Instant::now() + Duration::from_millis(process.config.terminate_timeout_ms);
                entries.push((process, deadline));
            }
        }

        // Drop no-signal processes (no child to reap, no signal to send)
        drop(no_signal);

        if entries.is_empty() {
            return Ok(());
        }

        // 2. Send kill signal to all processes with a real PID.
        // Signal failures are logged - the process will escalate to SIGKILL
        // if still alive, so we don't record an error here.
        for (process, _) in &mut entries {
            process.state = ProcessState::Stopping;
            let kill_signal = process.config.kill_signal;
            let pid = process.pid;
            if let Err(e) = process.send_signal(kill_signal) {
                warn!("Failed to send signal to process {} (pid={}): {}, will escalate", process.id, pid, e);
            }
        }

        // 3. Poll: wait for each process until it exits or its deadline passes
        let mut to_escalate: Vec<Process> = Vec::new();
        while !entries.is_empty() {
            let now = Instant::now();
            let mut still_waiting = Vec::new();
            for (mut process, deadline) in entries.drain(..) {
                if !process.is_running() {
                    debug!("Process {} (pid={}) exited after signal", process.id, process.pid);
                    process.join_readers(Duration::from_millis(500));
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

        // 5. Wait for SIGKILL to take effect (bounded - process may be in D state)
        for process in &mut to_reap {
            let kill_deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < kill_deadline {
                if process.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_some()) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if process.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_none()) {
                warn!("Process {} (pid={}) did not exit within 500ms after SIGKILL (may be in D state)", process.id, process.pid);
            }
            debug!("Process {} (pid={}) killed with SIGKILL", process.id, process.pid);
            process.join_readers(Duration::from_millis(500));
        }

        // 6. Re-insert processes that couldn't be killed so they remain tracked
        for mut process in to_reinsert {
            process.state = ProcessState::Failed;
            warn!("Re-inserting process {} (pid={}) into manager after failed kill", process.id, process.pid);
            self.processes.insert(process.id, process);
        }

        if !errors.is_empty() {
            return Err(StopManyError::new(errors).into());
        }
        Ok(())
    }

    /// Stop all processes with a given label.
    pub fn stop_label<L: Into<Label>>(&self, label: L) -> Result<(), ProcessManagerError> {
        let label = label.into();
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
    /// If the process is in `Restarting` state (backoff wait), cancels the
    /// backoff, resets restart state, and spawns immediately.
    /// If the process is running, stops it (with grace period and SIGKILL
    /// escalation), then spawns a new process in-place with the same
    /// `ProcessId`. Returns the same `ProcessId`.
    pub fn restart(&self, id: ProcessId) -> Result<ProcessId, ProcessManagerError> {
        // Get config and label, and handle Restarting state
        let (config, label, is_restarting) = {
            let process = self.processes.get(&id).ok_or(ProcessManagerError::NotFound(id))?;
            (Arc::clone(&process.config), process.label.clone(), process.state == ProcessState::Restarting)
        };

        if is_restarting {
            // Cancel backoff, reset restart state, spawn in-place
            debug!("Process {} is in Restarting state, cancelling backoff and spawning immediately", id);
            let mut entry = self.processes.get_mut(&id).ok_or(ProcessManagerError::NotFound(id))?;
            let process = entry.value_mut();
            // Clean up any remaining OS resources (should already be None)
            if let Some(child) = process.child.as_mut() {
                let _ = child.wait();
            }
            process.child = None;
            process.join_readers(Duration::from_millis(500));
            if let Some(restart_state) = &mut process.restart_state {
                restart_state.reset();
            }
        } else {
            // Normal restart: stop the OS process, then spawn in-place
            self.stop(id)?;

            // Re-insert a placeholder so we can spawn in-place
            // stop() removed the entry, so we need to create a new one with the same ID
        }

        // Spawn the new process
        let spawn = self.spawn_internal(&label, &config)?;

        // Update the entry in-place (or re-insert with same ID)
        let process = Process {
            id,
            pid: spawn.child.id(),
            program_name: config.command.clone(),
            label: label.clone(),
            terminate_on_exit: config.terminate_on_exit,
            config: Arc::clone(&config),
            child: Some(spawn.child),
            stdout_reader: spawn.stdout_reader,
            stderr_reader: spawn.stderr_reader,
            state: ProcessState::Starting,
            restart_state: if config.restart_on_exit { Some(RestartState::new()) } else { None },
            spawn_sequence: self.next_spawn_sequence.fetch_add(1, Ordering::Relaxed),
            cascade_flag: false,
            resolved_deps: Vec::new(),
            waiting_since: None,
        };

        if is_restarting {
            // Update in-place
            let mut entry = self.processes.get_mut(&id).ok_or(ProcessManagerError::NotFound(id))?;
            let existing = entry.value_mut();
            existing.pid = process.pid;
            existing.child = process.child;
            existing.stdout_reader = process.stdout_reader;
            existing.stderr_reader = process.stderr_reader;
            existing.state = ProcessState::Starting;
            if let Some(restart_state) = &mut existing.restart_state {
                restart_state.mark_started(Instant::now());
            }
        } else {
            // Re-insert with same ID (stop() removed it)
            self.processes.insert(id, process);
            // Mark started for restart tracking
            if let Some(mut entry) = self.processes.get_mut(&id)
                && let Some(restart_state) = &mut entry.restart_state
            {
                restart_state.mark_started(Instant::now());
            }
        }

        debug!("Restarted process '{}' (id={}, label='{}')", config.command, id, label);
        Ok(id)
    }

    /// Restart all processes with a given label.
    ///
    /// Restarts each matching process in-place, preserving `ProcessId`s.
    /// Returns the `ProcessId`s of the restarted processes (same IDs).
    pub fn restart_label<L: Into<Label>>(&self, label: L) -> Result<Vec<ProcessId>, ProcessManagerError> {
        let label = label.into();
        let ids: Vec<ProcessId> = self
            .processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| *entry.key())
            .collect();

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut new_ids = Vec::with_capacity(ids.len());
        for id in ids {
            new_ids.push(self.restart(id)?);
        }
        Ok(new_ids)
    }

    /// Start a process and wait for all dependencies to be `Running`.
    ///
    /// If `depends_on` is non-empty, this blocks until all dependencies
    /// are `Running` or `dependency_timeout_ms` elapses. Returns
    /// `ProcessManagerError::DependencyTimeout` if a dependency is not
    /// ready in time.
    ///
    /// For non-blocking start, use `start()` which inserts a `Waiting`
    /// placeholder and spawns the process asynchronously once
    /// dependencies are ready.
    pub fn start_with_deps<L: Into<Label>>(&self, label: L, config: &ProcessConfig) -> Result<ProcessId, ProcessManagerError> {
        let id = self.start(label, config)?;

        // If the process is already Running (no deps or deps were ready),
        // return immediately.
        #[allow(clippy::collapsible_if)]
        if let Some(entry) = self.processes.get(&id) {
            if entry.state != ProcessState::Waiting {
                return Ok(id);
            }
        }

        // Block until deps are Running or timeout
        let timeout = Duration::from_millis(config.dependency_timeout_ms);
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                // Timeout - fail the process
                if let Some(mut entry) = self.processes.get_mut(&id) {
                    entry.state = ProcessState::Failed;
                }
                let dep = config.depends_on.first().cloned();
                return Err(ProcessManagerError::DependencyTimeout { id, dependency: dep });
            }

            // Check if the process has transitioned out of Waiting
            if let Some(entry) = self.processes.get(&id) {
                match entry.state {
                    ProcessState::Starting | ProcessState::Running => return Ok(id),
                    ProcessState::Failed => {
                        let dep = config.depends_on.first().cloned();
                        return Err(ProcessManagerError::DependencyTimeout { id, dependency: dep });
                    }
                    ProcessState::Waiting => {}
                    _ => return Ok(id),
                }
            } else {
                // Process was removed
                return Err(ProcessManagerError::NotFound(id));
            }

            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Get all processes that depend on the given process (direct dependents).
    ///
    /// Returns a list of `ProcessId`s for processes whose `resolved_deps`
    /// contains the given `ProcessId`.
    pub fn dependents(&self, id: ProcessId) -> Vec<ProcessId> {
        self.processes
            .iter()
            .filter(|entry| entry.value().resolved_deps.contains(&id))
            .map(|entry| *entry.key())
            .collect()
    }

    /// Get all processes in the same label group.
    ///
    /// Returns a list of `ProcessId`s for processes with the given label.
    pub fn group_members<L: Into<Label>>(&self, label: L) -> Vec<ProcessId> {
        let label = label.into();
        self.processes
            .iter()
            .filter(|entry| entry.value().label == label)
            .map(|entry| *entry.key())
            .collect()
    }

    /// List all managed process IDs.
    pub fn ids(&self) -> Vec<ProcessId> {
        self.processes.iter().map(|entry| *entry.key()).collect()
    }

    /// List all labels that have at least one process.
    pub fn labels(&self) -> Vec<Label> {
        let mut labels: Vec<Label> = self.processes.iter().map(|entry| entry.value().label.clone()).collect();
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
        assert_eq!(manager.get_label(id), Some(Label::new("my-label")));
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
        assert_eq!(labels, vec![Label::new("alpha"), Label::new("beta")]);
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
        let start = Instant::now();
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
        // Drop the manager - should terminate the process
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
        assert_eq!(id, new_id);
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

    #[test]
    fn test_state_running() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        assert_eq!(manager.state(id), Some(ProcessState::Running));
        assert_eq!(manager.state(ProcessId::new(999)), None);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_state_stopped_after_normal_exit() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(manager.state(id), Some(ProcessState::Stopped));
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_state_crashed_after_failure() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("false".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(manager.state(id), Some(ProcessState::Crashed));
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_state_in_process_info() {
        let manager = ProcessManager::new();
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let id = manager.start("test", &config).unwrap();
        let info = manager.get_info(id).unwrap();
        assert_eq!(info.state, ProcessState::Running);
        manager.stop(id).unwrap();
    }

    #[test]
    fn test_reaper_exit_event_state_stopped() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("true".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.state, ProcessState::Stopped);
    }

    #[test]
    fn test_reaper_exit_event_state_crashed() {
        let (sender, receiver) = std::sync::mpsc::channel::<ProcessExitEvent>();
        let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender).unwrap();
        let config = ProcessConfig::builder()
            .command("false".to_string())
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        let _ = manager.start("test", &config).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.state, ProcessState::Crashed);
    }
}
