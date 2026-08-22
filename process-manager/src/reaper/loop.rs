use crate::process::Process;
use crate::process::ProcessExitEvent;
use crate::process::ProcessId;
use crate::process::ProcessState;
use crate::reaper::ExitedProcess;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use tracing::debug;
use tracing::warn;

/// Polls all tracked processes via non-blocking `try_wait()` every
/// `poll_interval`. When a process has exited, it is removed from the
/// `DashMap` and a `ProcessExitEvent` is sent to the channel.
pub(crate) fn reaper_loop(
    processes: Arc<DashMap<ProcessId, Process>>,
    poll_interval: Duration,
    exit_sender: &Sender<ProcessExitEvent>,
    stop_flag: &AtomicBool,
) {
    debug!("Reaper thread started (poll_interval={:?})", poll_interval);
    while !stop_flag.load(Ordering::Relaxed) {
        thread::sleep(poll_interval);

        // Collect exited processes with their exit status and derived state
        let mut exited: Vec<ExitedProcess> = Vec::new();
        let mut to_remove: Vec<ProcessId> = Vec::new();

        for mut entry in processes.iter_mut() {
            let process = entry.value_mut();
            if let Some(exit_status) = process.try_wait_exit() {
                let state = if exit_status.success() { ProcessState::Stopped } else { ProcessState::Crashed };
                exited.push(ExitedProcess {
                    id: process.id,
                    label: process.label.clone(),
                    pid: process.pid,
                    restart_on_exit: process.config.restart_on_exit,
                    exit_status: Some(exit_status),
                    state,
                });
                to_remove.push(process.id);
            }
        }

        // Remove exited processes and emit events
        for exited_process in exited {
            processes.remove(&exited_process.id);
            let event = ProcessExitEvent {
                id: exited_process.id,
                label: exited_process.label,
                pid: exited_process.pid,
                restart_on_exit: exited_process.restart_on_exit,
                exit_status: exited_process.exit_status,
                state: exited_process.state,
            };
            debug!("Reaper: process {} (pid={}) exited (state={}), emitting event", event.id, event.pid, event.state);
            if exit_sender.send(event).is_err() {
                warn!("Reaper: exit event channel closed, stopping reaper thread");
                break;
            }
        }
    }
    debug!("Reaper thread stopped");
}
