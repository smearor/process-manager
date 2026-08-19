use thiserror::Error;

use crate::config::ProcessConfig;
use crate::process::ProcessId;

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
}

/// Error returned when a `ProcessConfig` is invalid.
#[derive(Debug, Error)]
pub enum ProcessConfigError {
    /// The command string is empty.
    #[error("Command cannot be empty")]
    EmptyCommand,
    /// The `ProcessConfig` is missing a required field.
    #[error("Missing required field: {0}")]
    MissingField(String),
}

impl ProcessConfigError {
    /// Validate a `ProcessConfig` before spawning.
    pub fn validate(config: &ProcessConfig) -> Result<(), ProcessConfigError> {
        if config.command.is_empty() {
            return Err(ProcessConfigError::EmptyCommand);
        }
        Ok(())
    }
}
