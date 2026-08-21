use crate::config::ProcessConfigError;
use crate::process::ProcessId;
use thiserror::Error;

/// Error returned by `ProcessManager` operations.
#[derive(Debug, Error)]
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
}
