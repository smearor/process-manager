use crate::config::BackoffConfig;
use crate::config::RestartPolicy;
use crate::config::RestartTrigger;
use crate::manager::SpawnResult;
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
fn perform_restart(processes: &DashMap<ProcessId, Process>, id: ProcessId, exit_sender: &Sender<ProcessExitEvent>) {
    let mut entry = match processes.get_mut(&id) {
        Some(entry) => entry,
        None => return,
    };
    let process = entry.value_mut();

    let config = Arc::clone(&process.config);
    let label = process.label.clone();

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

/// Spawn a new OS process from the given config and label.
fn spawn_process(config: &crate::config::ProcessConfig, label: &str) -> std::io::Result<SpawnResult> {
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
    let stderr_reader = if config.stderr == crate::config::StdioConfig::Piped
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

    Ok(SpawnResult {
        child,
        stdout_reader,
        stderr_reader,
    })
}
