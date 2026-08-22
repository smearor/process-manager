use crate::config::ProcessConfigError;
use crate::process::ProcessId;
use thiserror::Error;

/// Error returned by `ProcessManager` operations.
#[derive(Debug, Error)]
#[must_use]
pub enum ProcessManagerError {
    /// No process with the given ID was found in the manager.
    #[error("Process with ID {0} not found")]
    NotFound(ProcessId),
    /// The executable was not found in `PATH` and is not an absolute path.
    #[error("Executable '{0}' not found in PATH")]
    ExecutableNotFound(String),
    /// Failed to spawn the child process.
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(#[from] std::io::Error),
    /// Failed to send a signal to the process.
    #[error("Failed to send signal to process {0}: {1}")]
    SignalFailed(u32, String),
    /// A ProcessConfig validation error.
    #[error("Process config error: {0}")]
    ConfigError(#[from] ProcessConfigError),
    /// Failed to spawn the reaper thread.
    #[error("Failed to spawn reaper thread: {0}")]
    ReaperThreadFailed(std::io::Error),
    /// One or more processes failed to stop during a batch operation.
    #[error("{0}")]
    StopMany(#[from] StopManyError),
}

/// Aggregated errors from stopping multiple processes.
///
/// Returned by `stop_many` when one or more processes could not be stopped.
/// Each individual error is preserved so the caller can inspect all failures.
#[derive(Debug, Error)]
#[error("{count} error(s) stopping processes: {errors}", count = self.errors.len(), errors = self.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "))]
#[must_use]
pub struct StopManyError {
    /// All individual errors encountered during the batch stop.
    pub errors: Vec<ProcessManagerError>,
}

impl StopManyError {
    /// Create a `StopManyError` from a non-empty vector of errors.
    pub fn new(errors: Vec<ProcessManagerError>) -> Self {
        Self { errors }
    }

    /// Whether any errors were collected.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}
