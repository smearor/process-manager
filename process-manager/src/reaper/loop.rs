use crate::process::Process;
use crate::process::ProcessExitEvent;
use crate::process::ProcessId;
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

        // Collect exited processes with their exit status
        let mut exited: Vec<(ProcessId, String, u32, bool, Option<std::process::ExitStatus>)> = Vec::new();
        let mut to_remove: Vec<ProcessId> = Vec::new();

        for mut entry in processes.iter_mut() {
            let process = entry.value_mut();
            if let Some(exit_status) = process.try_wait_exit() {
                exited.push((process.id, process.label.clone(), process.pid, process.config.restart_on_exit, Some(exit_status)));
                to_remove.push(process.id);
            }
        }

        // Remove exited processes and emit events
        for (id, label, pid, restart_on_exit, exit_status) in exited {
            processes.remove(&id);
            let event = ProcessExitEvent {
                id,
                label,
                pid,
                restart_on_exit,
                exit_status,
            };
            debug!("Reaper: process {} (pid={}) exited, emitting event", event.id, event.pid);
            if exit_sender.send(event).is_err() {
                warn!("Reaper: exit event channel closed, stopping reaper thread");
                break;
            }
        }
    }
    debug!("Reaper thread stopped");
}
