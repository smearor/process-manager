use std::process::Child;
use std::thread::JoinHandle;

/// Result of spawning a child process.
///
/// Contains the OS child handle and optional reader thread join handles
/// for stdout and stderr (present when `StdioConfig::Piped` is used).
#[derive(Debug)]
#[must_use]
pub(crate) struct SpawnResult {
    /// The spawned child process handle.
    pub child: Child,

    /// Join handle for the stdout reader thread, if stdout was piped.
    pub stdout_reader: Option<JoinHandle<()>>,

    /// Join handle for the stderr reader thread, if stderr was piped.
    pub stderr_reader: Option<JoinHandle<()>>,
}
