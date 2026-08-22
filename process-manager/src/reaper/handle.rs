use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

/// Handle to the reaper thread, allowing it to be stopped on `Drop`.
#[derive(Debug)]
pub(crate) struct ReaperHandle {
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl ReaperHandle {
    pub(crate) fn new(stop_flag: Arc<AtomicBool>, thread_handle: thread::JoinHandle<()>) -> Self {
        Self {
            stop_flag,
            thread_handle: Some(thread_handle),
        }
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
