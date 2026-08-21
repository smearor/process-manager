use crate::config::ProcessConfig;
use thiserror::Error;

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
