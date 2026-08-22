use crate::process::ProcessId;
use crate::process::ProcessState;
use std::process::ExitStatus;

/// Information about a process that the reaper has detected as exited.
///
/// Collected during the poll pass, then used to remove the process from the
/// `DashMap` and emit a `ProcessExitEvent`.
#[derive(Debug)]
#[must_use]
pub(crate) struct ExitedProcess {
    /// The ID of the process that exited.
    pub id: ProcessId,

    /// The label of the process that exited.
    pub label: String,

    /// The PID of the process that exited.
    pub pid: u32,

    /// Whether the process should be restarted (from `config.restart_on_exit`).
    pub restart_on_exit: bool,

    /// The exit status of the process, if available.
    pub exit_status: Option<ExitStatus>,

    /// The lifecycle state at the time of exit (`Stopped` or `Crashed`).
    pub state: ProcessState,
}
