use crate::config::ProcessConfig;
use crate::process::Process;
use crate::process::ProcessId;
use crate::process::ProcessState;
use std::sync::Arc;

/// A lightweight snapshot of a managed process.
///
/// Contains metadata only — no `Child` handle. Returned by inspection methods
/// like `get_by_label()`. Cannot be used for `is_running()`, `wait()`, or
/// signal operations; use `stop()` / `stop_label()` for process control.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct ProcessInfo {
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
    pub config: Arc<ProcessConfig>,

    /// The current lifecycle state of this process.
    ///
    /// This is a snapshot taken at the time of the `ProcessInfo` creation.
    /// It may be stale if the process has since changed state.
    pub state: ProcessState,
}

impl From<&Process> for ProcessInfo {
    fn from(process: &Process) -> Self {
        Self {
            id: process.id,
            pid: process.pid,
            program_name: process.program_name.clone(),
            label: process.label.clone(),
            terminate_on_exit: process.terminate_on_exit,
            config: Arc::clone(&process.config),
            state: process.state,
        }
    }
}
