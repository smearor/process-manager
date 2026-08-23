use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;

use crate::process::ProcessExitEvent;

/// Handle to the reaper thread, allowing it to be stopped on `Drop`.
///
/// Also holds a clone of the exit event sender so that `ProcessManager`
/// can emit `ProcessExitEvent`s for non-reaper paths (e.g. `stop()` on
/// a `Waiting` process).
#[derive(Debug)]
pub(crate) struct ReaperHandle {
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
    exit_sender: Sender<ProcessExitEvent>,
}

impl ReaperHandle {
    pub(crate) fn new(stop_flag: Arc<AtomicBool>, thread_handle: thread::JoinHandle<()>, exit_sender: Sender<ProcessExitEvent>) -> Self {
        Self {
            stop_flag,
            thread_handle: Some(thread_handle),
            exit_sender,
        }
    }

    /// Try to send an exit event through the exit channel.
    ///
    /// Returns `Err` if the channel is closed.
    pub(crate) fn try_send_exit_event(&self, event: ProcessExitEvent) -> Result<(), String> {
        self.exit_sender.send(event).map_err(|e| e.to_string())
    }
}

impl Drop for ReaperHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
