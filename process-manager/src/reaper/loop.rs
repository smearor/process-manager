use crate::config::BackoffConfig;
use crate::config::DependencyRef;
use crate::config::Label;
use crate::config::RestartPolicy;
use crate::config::RestartTrigger;
use crate::config::SupervisorStrategy;
use crate::manager::SpawnResult;
use crate::manager::all_deps_running;
use crate::manager::resolve_dependencies;
use crate::process::Process;
use crate::process::ProcessExitEvent;
use crate::process::ProcessId;
use crate::process::ProcessState;
use crate::reaper::ExitedProcess;
use dashmap::DashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use tracing::debug;
use tracing::warn;

/// Polls all tracked processes via non-blocking `try_wait()` every
/// `poll_interval`. When a process has exited, it handles automatic restart
/// with backoff and rate limiting, or removes it from the `DashMap` and emits
/// a `ProcessExitEvent`.
///
/// The reaper does not block. Backoff waits are handled by checking
/// `next_eligible_restart` on each poll cycle.
pub(crate) fn reaper_loop(
    processes: Arc<DashMap<ProcessId, Process>>,
    poll_interval: Duration,
    exit_sender: &Sender<ProcessExitEvent>,
    stop_flag: &AtomicBool,
) {
    debug!("Reaper thread started (poll_interval={:?})", poll_interval);
    while !stop_flag.load(Ordering::Relaxed) {
        thread::sleep(poll_interval);
        let now = Instant::now();

        // Phase 1: Detect exits and handle restart decisions
        let mut to_remove: Vec<ProcessId> = Vec::new();
        let mut exited: Vec<ExitedProcess> = Vec::new();
        // Collect (crashed_id, label, spawn_sequence, strategy) for cascade processing
        let mut cascade_origins: Vec<(ProcessId, Label, u64, SupervisorStrategy)> = Vec::new();

        for mut entry in processes.iter_mut() {
            let process = entry.value_mut();

            // Skip processes in Restarting state - they have no child to poll
            if process.state == ProcessState::Restarting {
                continue;
            }

            // Check for stable uptime reset on Running processes
            if process.state == ProcessState::Running
                && let Some(restart_state) = &mut process.restart_state
                && let Some(backoff_config) = get_backoff_config(&process.config)
            {
                restart_state.check_stable_uptime(backoff_config, now);
            }

            if let Some(exit_status) = process.try_wait_exit() {
                // If cascade_flag is set, this process was stopped as part of
                // a supervisor cascade. Emit Stopped event. If restart_on_exit
                // is true, schedule restart (the cascade origin triggered this
                // stop, but the process should still be restarted as part of
                // the group restart).
                if process.cascade_flag {
                    let should_restart_cascade = process.config.restart_on_exit;
                    let restart_config = if should_restart_cascade {
                        Some((Arc::clone(&process.config), process.label.clone()))
                    } else {
                        None
                    };
                    exited.push(ExitedProcess {
                        id: process.id,
                        label: process.label.clone(),
                        pid: process.pid,
                        restart_on_exit: process.config.restart_on_exit,
                        exit_status: Some(exit_status),
                        state: ProcessState::Stopped,
                        restart_config,
                    });
                    // Don't remove if it will restart
                    if !should_restart_cascade {
                        to_remove.push(process.id);
                    }
                    // Reset cascade_flag - the cascade is complete for this process
                    process.cascade_flag = false;
                    continue;
                }

                let base_state = if exit_status.success() {
                    ProcessState::Stopped
                } else {
                    ProcessState::Crashed
                };

                // Determine if restart should be triggered
                let restart_triggered = process.config.restart_on_exit && should_restart(&process.config.restart_trigger, base_state);

                // Determine final event state (rate-limit check before emission)
                let event_state = if restart_triggered {
                    if let Some(restart_state) = &process.restart_state {
                        if let Some(backoff_config) = get_backoff_config(&process.config) {
                            if restart_state.is_rate_limited(backoff_config) {
                                ProcessState::Failed
                            } else {
                                base_state
                            }
                        } else {
                            // Immediate policy - no rate limiting
                            base_state
                        }
                    } else {
                        base_state
                    }
                } else {
                    base_state
                };

                let restart_config = if restart_triggered && event_state != ProcessState::Failed {
                    Some((Arc::clone(&process.config), process.label.clone()))
                } else {
                    None
                };

                // If this is a crash (not clean exit) and restart is triggered,
                // record the cascade origin for Phase 1b
                if restart_triggered && event_state != ProcessState::Failed && process.config.supervisor_strategy != SupervisorStrategy::OneForOne {
                    cascade_origins.push((process.id, process.label.clone(), process.spawn_sequence, process.config.supervisor_strategy));
                }

                exited.push(ExitedProcess {
                    id: process.id,
                    label: process.label.clone(),
                    pid: process.pid,
                    restart_on_exit: process.config.restart_on_exit,
                    exit_status: Some(exit_status),
                    state: event_state,
                    restart_config,
                });

                // Mark for removal only if not restarting
                if event_state == ProcessState::Failed || !restart_triggered {
                    to_remove.push(process.id);
                }
            } else if process.state == ProcessState::Starting {
                // Process is alive and was in Starting state - transition to Running
                process.state = ProcessState::Running;
            }
        }

        // Phase 1b: Handle supervisor strategy cascades
        // For each crashed process with OneForAll or RestForOne, flag and
        // send kill signals to affected group members.
        let mut cascade_targets: Vec<ProcessId> = Vec::new();
        for (origin_id, origin_label, origin_seq, strategy) in &cascade_origins {
            let mut targets: Vec<ProcessId> = Vec::new();
            for entry in processes.iter() {
                let process = entry.value();
                // Skip the origin process itself
                if process.id == *origin_id {
                    continue;
                }
                // Only target processes in the same label group
                if process.label != *origin_label {
                    continue;
                }
                // Only target Running or Starting processes
                if process.state != ProcessState::Running && process.state != ProcessState::Starting {
                    continue;
                }
                match strategy {
                    SupervisorStrategy::OneForAll => {
                        targets.push(*entry.key());
                    }
                    SupervisorStrategy::RestForOne => {
                        // Only stop processes started after the crashed one
                        if process.spawn_sequence > *origin_seq {
                            targets.push(*entry.key());
                        }
                    }
                    SupervisorStrategy::OneForOne => {}
                }
            }
            cascade_targets.extend(targets);
        }

        // Flag and send kill signals to cascade targets (async, non-blocking)
        for target_id in &cascade_targets {
            if let Some(mut entry) = processes.get_mut(target_id) {
                let process = entry.value_mut();
                // Safety: pid 0 would broadcast the signal to the entire
                // process group. Waiting/Failed processes have pid 0 and
                // should never be cascade targets, but guard defensively.
                if process.pid == 0 {
                    warn!("Reaper: skipping cascade kill for process {} with pid=0", target_id);
                    continue;
                }
                process.cascade_flag = true;
                process.state = ProcessState::Stopping;
                let kill_signal = process.config.kill_signal;
                let pid = process.pid;
                // Send kill signal asynchronously - do not wait for exit
                if let Err(e) = process.send_signal(kill_signal)
                    && e.raw_os_error() != Some(libc::ESRCH)
                {
                    warn!("Reaper: failed to send cascade kill signal to process {} (pid={}): {}", target_id, pid, e);
                }
                debug!("Reaper: sent cascade kill signal to process {} (pid={})", target_id, pid);
            }
        }

        // Phase 2: Remove non-restarting processes and emit events
        for id in &to_remove {
            processes.remove(id);
        }

        for exited_process in &exited {
            let event = ProcessExitEvent {
                id: exited_process.id,
                label: exited_process.label.clone(),
                pid: exited_process.pid,
                restart_on_exit: exited_process.restart_on_exit,
                exit_status: exited_process.exit_status,
                state: exited_process.state,
            };
            debug!("Reaper: process {} (pid={}) exited (state={}), emitting event", event.id, event.pid, event.state);
            if exit_sender.send(event).is_err() {
                warn!("Reaper: exit event channel closed, stopping reaper thread");
                return;
            }
        }

        // Phase 3: Handle restart for processes that will restart
        for exited_process in &exited {
            if exited_process.restart_config.is_none() {
                continue;
            }

            let (config, _label) = exited_process.restart_config.as_ref().unwrap();
            let id = exited_process.id;

            // Get the process entry (it was not removed since it's restarting)
            let mut entry = match processes.get_mut(&id) {
                Some(entry) => entry,
                None => continue,
            };
            let process = entry.value_mut();

            // Release OS resources before backoff wait
            if let Some(child) = process.child.as_mut() {
                let _ = child.wait();
            }
            process.child = None;
            process.join_readers(Duration::from_millis(500));

            // Record restart and compute backoff delay
            if let Some(restart_state) = &mut process.restart_state {
                restart_state.record_restart();
                if let Some(backoff_config) = get_backoff_config(config) {
                    let delay = restart_state.current_delay(backoff_config);
                    restart_state.schedule_restart(now + delay);
                    debug!(
                        "Reaper: process {} scheduled for restart in {:?} (restart_count={})",
                        id,
                        delay,
                        restart_state.restart_count()
                    );
                } else {
                    // Immediate policy - eligible right away
                    restart_state.schedule_restart(now);
                    debug!("Reaper: process {} scheduled for immediate restart", id);
                }
            }

            // Set Restarting state
            process.state = ProcessState::Restarting;
        }

        // Phase 4: Check processes in Restarting state for eligible restarts
        let mut restart_ids: Vec<ProcessId> = Vec::new();
        for entry in processes.iter() {
            if entry.value().state == ProcessState::Restarting
                && let Some(restart_state) = &entry.value().restart_state
                && restart_state.is_eligible_for_restart(now)
            {
                restart_ids.push(*entry.key());
            }
        }

        for id in restart_ids {
            perform_restart(&processes, id, exit_sender);
        }

        // Phase 5: Check Waiting processes - spawn when deps are Running,
        // fail-fast when a dependency is terminal, timeout if deps not ready.
        //
        // We collect Waiting process IDs first, then process each one
        // individually. This avoids nested DashMap iteration which can
        // deadlock when the outer iterator holds a shard read lock and
        // the inner iteration (for label resolution) tries to acquire
        // the same shard.
        let waiting_ids: Vec<ProcessId> = processes
            .iter()
            .filter(|e| e.value().state == ProcessState::Waiting)
            .map(|e| *e.key())
            .collect();

        let mut waiting_to_spawn: Vec<ProcessId> = Vec::new();
        let mut waiting_to_fail: Vec<(ProcessId, ProcessState)> = Vec::new();

        for id in waiting_ids {
            let entry = match processes.get(&id) {
                Some(e) => e,
                None => continue,
            };
            let process = entry.value();

            // Check dependency timeout
            if let Some(since) = process.waiting_since {
                let elapsed = since.elapsed();
                let timeout = Duration::from_millis(process.config.dependency_timeout_ms);
                if elapsed >= timeout {
                    debug!("Reaper: Waiting process {} dependency timeout (elapsed={:?}, timeout={:?})", id, elapsed, timeout);
                    waiting_to_fail.push((id, ProcessState::Failed));
                    continue;
                }
            }

            let all_resolved = process.resolved_deps.len() == process.config.depends_on.len();

            if !all_resolved {
                // Some deps not yet resolved - try to resolve remaining labels.
                // Collect info needed for resolution without holding the borrow.
                let deps_to_resolve: Vec<DependencyRef> = process
                    .config
                    .depends_on
                    .iter()
                    .filter(|dep| match dep {
                        DependencyRef::Label(label) => !process
                            .resolved_deps
                            .iter()
                            .any(|rid| processes.get(rid).is_some_and(|e| e.value().label == *label)),
                        DependencyRef::Id(id) => !process.resolved_deps.contains(id),
                    })
                    .cloned()
                    .collect();
                let existing_resolved = process.resolved_deps.clone();

                drop(entry);

                let mut newly_resolved: Vec<ProcessId> = Vec::new();
                for dep in &deps_to_resolve {
                    match dep {
                        DependencyRef::Label(label) => {
                            // Try to find a Running process with this label
                            let found = processes
                                .iter()
                                .find(|e| e.value().label == *label && e.value().state == ProcessState::Running)
                                .map(|e| *e.key());
                            if let Some(dep_id) = found {
                                newly_resolved.push(dep_id);
                            }
                        }
                        DependencyRef::Id(dep_id) => {
                            newly_resolved.push(*dep_id);
                        }
                    }
                }

                if !newly_resolved.is_empty() {
                    if let Some(mut entry) = processes.get_mut(&id) {
                        entry.value_mut().resolved_deps.extend(newly_resolved);
                    }
                    // Re-check if all deps are now resolved and running
                    let process = match processes.get(&id) {
                        Some(e) => e,
                        None => continue,
                    };
                    let all_resolved = process.resolved_deps.len() == process.config.depends_on.len();
                    let all_running = process
                        .resolved_deps
                        .iter()
                        .all(|dep_id| processes.get(dep_id).is_some_and(|e| e.value().state == ProcessState::Running));
                    if all_resolved && all_running {
                        waiting_to_spawn.push(id);
                    }
                    continue;
                }

                // Check for terminal dependencies among already-resolved ones
                let terminal_dep = existing_resolved.iter().find_map(|dep_id| match processes.get(dep_id) {
                    Some(dep) => {
                        if dep.state == ProcessState::Failed {
                            return Some(*dep_id);
                        }
                        if dep.state == ProcessState::Stopped && !dep.config.restart_on_exit {
                            return Some(*dep_id);
                        }
                        None
                    }
                    None => Some(*dep_id),
                });

                if terminal_dep.is_some() {
                    waiting_to_fail.push((id, ProcessState::Failed));
                }
                continue;
            }

            // All deps resolved - check if they're all Running.
            // Once resolved, we NEVER re-resolve. This ensures label binding
            // persists: if a resolved dep is gone, we fail-fast rather than
            // binding to a new process with the same label.
            let all_running = process
                .resolved_deps
                .iter()
                .all(|dep_id| processes.get(dep_id).is_some_and(|e| e.value().state == ProcessState::Running));

            if all_running {
                waiting_to_spawn.push(id);
            } else {
                // Check for terminal dependencies
                let terminal_dep = process.resolved_deps.iter().find_map(|dep_id| match processes.get(dep_id) {
                    Some(dep) => {
                        if dep.state == ProcessState::Failed {
                            return Some(*dep_id);
                        }
                        if dep.state == ProcessState::Stopped && !dep.config.restart_on_exit {
                            return Some(*dep_id);
                        }
                        None
                    }
                    None => Some(*dep_id),
                });

                if terminal_dep.is_some() {
                    waiting_to_fail.push((id, ProcessState::Failed));
                }
            }
        }

        // Spawn Waiting processes whose deps are all Running
        for id in waiting_to_spawn {
            spawn_waiting_process(&processes, id, exit_sender);
        }

        // Fail Waiting processes with terminal dependencies
        for (id, fail_state) in waiting_to_fail {
            if let Some(mut entry) = processes.get_mut(&id) {
                let process = entry.value_mut();
                process.state = fail_state;
                let event = ProcessExitEvent {
                    id,
                    label: process.label.clone(),
                    pid: process.pid,
                    restart_on_exit: process.config.restart_on_exit,
                    exit_status: None,
                    state: fail_state,
                };
                debug!("Reaper: Waiting process {} failed (dependency terminal), emitting event", id);
                drop(entry);
                processes.remove(&id);
                if exit_sender.send(event).is_err() {
                    warn!("Reaper: exit event channel closed during Waiting fail-fast");
                    return;
                }
            }
        }

        // Phase 6: Check Running processes for terminal dependencies.
        // If a Running process has a resolved dependency that is gone (removed)
        // or in a terminal state (Failed, Stopped without restart), fail-fast
        // the dependent process. This ensures label binding persists: the
        // dependent does not switch to a new process with the same label.
        let running_with_deps: Vec<ProcessId> = processes
            .iter()
            .filter(|e| e.value().state == ProcessState::Running && !e.value().resolved_deps.is_empty())
            .map(|e| *e.key())
            .collect();

        for id in running_with_deps {
            let entry = match processes.get(&id) {
                Some(e) => e,
                None => continue,
            };
            let process = entry.value();
            let resolved_deps = process.resolved_deps.clone();

            let terminal_dep = resolved_deps.iter().find_map(|dep_id| match processes.get(dep_id) {
                Some(dep) => {
                    if dep.state == ProcessState::Failed {
                        return Some(*dep_id);
                    }
                    if dep.state == ProcessState::Stopped && !dep.config.restart_on_exit {
                        return Some(*dep_id);
                    }
                    None
                }
                None => Some(*dep_id),
            });

            if terminal_dep.is_some() {
                let label = process.label.clone();
                let pid = process.pid;
                let restart_on_exit = process.config.restart_on_exit;
                drop(entry);

                if let Some(mut entry) = processes.get_mut(&id) {
                    let process = entry.value_mut();
                    process.state = ProcessState::Failed;
                    let kill_signal = process.config.kill_signal;
                    if pid > 0 {
                        let _ = process.send_signal(kill_signal);
                    }
                    drop(entry);
                }
                processes.remove(&id);

                let event = ProcessExitEvent {
                    id,
                    label,
                    pid,
                    restart_on_exit,
                    exit_status: None,
                    state: ProcessState::Failed,
                };
                debug!("Reaper: Running process {} failed (dependency terminal), emitting event", id);
                if exit_sender.send(event).is_err() {
                    warn!("Reaper: exit event channel closed during Running fail-fast");
                    return;
                }
            }
        }
    }
    debug!("Reaper thread stopped");
}

/// Check if a restart should be triggered based on the trigger config and exit state.
fn should_restart(trigger: &RestartTrigger, state: ProcessState) -> bool {
    match trigger {
        RestartTrigger::CrashOnly => state == ProcessState::Crashed,
        RestartTrigger::Always => true,
    }
}

/// Get the `BackoffConfig` from the restart policy, if any.
fn get_backoff_config(config: &crate::config::ProcessConfig) -> Option<&BackoffConfig> {
    match &config.restart_policy {
        RestartPolicy::Backoff(config) => Some(config),
        RestartPolicy::Immediate => None,
    }
}

/// Attempt to restart a process that is in `Restarting` state and eligible.
///
/// If the process has `depends_on`, dependencies are re-resolved before
/// spawning. If dependencies are not yet `Running`, the process transitions
/// to `Waiting` instead of spawning. The reaper's Phase 5 will then spawn
/// it once all dependencies are `Running`.
fn perform_restart(processes: &DashMap<ProcessId, Process>, id: ProcessId, exit_sender: &Sender<ProcessExitEvent>) {
    let mut entry = match processes.get_mut(&id) {
        Some(entry) => entry,
        None => return,
    };
    let process = entry.value_mut();

    let config = Arc::clone(&process.config);
    let label = process.label.clone();

    // Check dependencies before restarting. If the process has depends_on
    // and not all dependencies are Running, transition to Waiting instead
    // of spawning. This ensures dependency-ordered restart in OneForAll
    // and RestForOne strategies.
    if !config.depends_on.is_empty() {
        let resolved = resolve_dependencies(processes, &config.depends_on).unwrap_or_default();
        let all_resolved = resolved.len() == config.depends_on.len();
        let all_running = all_resolved && all_deps_running(processes, &resolved);

        if !all_running {
            // Dependencies not ready - transition to Waiting
            process.state = ProcessState::Waiting;
            process.waiting_since = Some(Instant::now());
            process.resolved_deps = resolved;
            process.cascade_flag = false;
            debug!("Reaper: process {} transitioning to Waiting on restart (deps not ready)", id);
            return;
        }

        // Deps are ready - update resolved_deps for consistency
        process.resolved_deps = resolved;
    }

    // Spawn the new OS process
    let spawn_result = spawn_process(&config, &label);

    match spawn_result {
        Ok(spawn) => {
            let new_pid = spawn.child.id();

            // Update the existing entry in-place
            process.child = Some(spawn.child);
            process.pid = new_pid;
            process.stdout_reader = spawn.stdout_reader;
            process.stderr_reader = spawn.stderr_reader;
            process.state = ProcessState::Starting;

            // Mark started for uptime tracking
            if let Some(restart_state) = &mut process.restart_state {
                restart_state.mark_started(Instant::now());
                restart_state.cancel_pending_restart();
            }

            debug!("Reaper: process {} restarted successfully (new_pid={})", id, new_pid);
        }
        Err(err) => {
            // Spawn failed - terminal
            warn!("Reaper: failed to restart process {} (spawn error: {}), setting Failed", id, err);
            process.state = ProcessState::Failed;

            let event = ProcessExitEvent {
                id,
                label: process.label.clone(),
                pid: process.pid,
                restart_on_exit: process.config.restart_on_exit,
                exit_status: None,
                state: ProcessState::Failed,
            };
            drop(entry);
            processes.remove(&id);
            debug!("Reaper: process {} removed after spawn failure (state=Failed)", id);
            if exit_sender.send(event).is_err() {
                warn!("Reaper: exit event channel closed during spawn failure");
            }
        }
    }
}

/// Spawn a process that was in `Waiting` state now that its dependencies are `Running`.
///
/// Updates the process in-place: sets the child handle, PID, state to `Starting`,
/// and marks started for restart tracking.
fn spawn_waiting_process(processes: &DashMap<ProcessId, Process>, id: ProcessId, exit_sender: &Sender<ProcessExitEvent>) {
    let mut entry = match processes.get_mut(&id) {
        Some(entry) => entry,
        None => return,
    };
    let process = entry.value_mut();

    let config = Arc::clone(&process.config);
    let label = process.label.clone();

    let spawn_result = spawn_process(&config, &label);

    match spawn_result {
        Ok(spawn) => {
            let new_pid = spawn.child.id();
            process.child = Some(spawn.child);
            process.pid = new_pid;
            process.stdout_reader = spawn.stdout_reader;
            process.stderr_reader = spawn.stderr_reader;
            process.state = ProcessState::Starting;
            process.waiting_since = None;

            if let Some(restart_state) = &mut process.restart_state {
                restart_state.mark_started(Instant::now());
            }

            debug!("Reaper: Waiting process {} spawned (pid={}), transitioning to Starting", id, new_pid);
        }
        Err(err) => {
            warn!("Reaper: failed to spawn Waiting process {} (spawn error: {}), setting Failed", id, err);
            process.state = ProcessState::Failed;

            let event = ProcessExitEvent {
                id,
                label: process.label.clone(),
                pid: process.pid,
                restart_on_exit: process.config.restart_on_exit,
                exit_status: None,
                state: ProcessState::Failed,
            };
            drop(entry);
            processes.remove(&id);
            debug!("Reaper: Waiting process {} removed after spawn failure (state=Failed)", id);
            if exit_sender.send(event).is_err() {
                warn!("Reaper: exit event channel closed during Waiting spawn failure");
            }
        }
    }
}

/// Spawn a new OS process from the given config and label.
fn spawn_process(config: &crate::config::ProcessConfig, label: &Label) -> std::io::Result<SpawnResult> {
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
            which::which(&config.command).map_err(std::io::Error::other)?.to_string_lossy().to_string()
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
    let mut child = command.spawn()?;

    // 8. Spawn reader threads for piped stdout/stderr
    let stdout_reader = if config.stdout == crate::config::StdioConfig::Piped
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
    let stderr_reader = if config.stderr == crate::config::StdioConfig::Piped
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
