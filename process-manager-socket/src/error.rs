use thiserror::Error;

/// Error returned by `SocketBuilder` operations.
#[derive(Debug, Clone, Error)]
#[must_use]
pub enum SocketBuilderError {
    /// A socket with the given name already exists in `XDG_RUNTIME_DIR`.
    #[error("Socket already exists")]
    SocketAlreadyExists,
    /// The `XDG_RUNTIME_DIR` environment variable is not set.
    #[error("XdgRuntimeDir not set")]
    XdgRuntimeDirNotSet,
    /// Failed to generate a unique socket name after exhausting all candidates.
    #[error("Failed to generate unique socket name")]
    GenerateUniqueSocketNameFailed,
}

/// Error returned by `SocketManager` operations.
#[derive(Debug, Clone, Error)]
#[must_use]
pub enum SocketManagerError {
    /// A socket with the given name is already registered in the manager.
    #[error("Socket with name '{0}' is already registered")]
    AlreadyRegistered(String),
    /// No socket with the given name was found in the manager.
    #[error("Socket with name '{0}' not found")]
    NotFound(String),
    /// A `SocketBuilder` operation failed.
    #[error(transparent)]
    BuilderError(#[from] SocketBuilderError),
}
