/// The lifecycle state of a managed process.
///
/// Replaces the binary `is_running() -> bool` with an explicit state machine,
/// enabling consumers to make more precise decisions based on what phase the
/// process is in.
///
/// # State transitions
///
/// ```text
/// Starting ──► Running ──► Stopping ──► Stopped
///                │              ╰──► Crashed
///                │
///                ╰──► Crashed (exited unexpectedly)
///
/// Crashed ──► Restarting (backoff wait, no OS handle) ──► Starting ──► Running
/// Stopped ──► Restarting (if restart_trigger = Always) ──► Starting ──► Running
///
/// Waiting ──► Starting (all deps Running)
/// Waiting ──► Failed (dependency timeout or terminal dependency)
/// Waiting ──► Stopped (stop() called while Waiting)
///
/// Restarting can be cancelled by:
///   - manual stop() → process removed (no event, no signal)
///   - manual restart() → backoff cancelled, spawn immediately
///   - rate limit exceeded → Failed
///   - spawn failure during restart → Failed
///
/// (spawn failure) ──► Failed
/// (force-kill failure) ──► Failed
/// (rate limit exceeded) ──► Failed
/// (spawn failure during automatic restart) ──► Failed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[must_use]
pub enum ProcessState {
    /// The process is being spawned and has not yet been confirmed running.
    ///
    /// This is a transient state set between `spawn()` and insertion into the
    /// manager. In practice it is very brief since `start()` is synchronous.
    #[default]
    Starting,

    /// The process is alive and running.
    ///
    /// Confirmed via non-blocking `try_wait()` returning `Ok(None)`.
    Running,

    /// A stop signal has been sent and the manager is waiting for exit.
    ///
    /// Set when `stop()` / `stop_many()` sends the kill signal. The process
    /// transitions to `Stopped` (or `Crashed` if it exits with an error)
    /// once `try_wait()` confirms exit.
    Stopping,

    /// The process has exited normally.
    ///
    /// The exit status indicates success (exit code 0), or the process was
    /// stopped by the manager and exited within the grace period.
    Stopped,

    /// The process exited unexpectedly with a non-zero exit code or signal.
    ///
    /// Detected by the reaper or `try_wait()` when the exit status indicates
    /// failure.
    Crashed,

    /// A restart is in progress - the process is in backoff wait.
    ///
    /// Set by the reaper when an automatic restart is triggered and the
    /// backoff delay has not yet elapsed. The OS child handle is `None`
    /// (resources released). `send_signal()` returns an error in this state.
    /// `stop()` cancels the backoff and removes the process without
    /// emitting an event. `restart()` cancels the backoff and spawns
    /// immediately.
    Restarting,

    /// The process failed to start, could not be killed, or exhausted restarts.
    ///
    /// Set when `spawn()` fails (the process is not stored in the manager),
    /// when `force_kill()` fails and the process is re-inserted for tracking,
    /// when the restart rate limit is exceeded (`max_restarts` reached), or
    /// when `spawn()` fails during an automatic restart in the reaper.
    Failed,

    /// The process is queued but waiting for dependencies to become `Running`.
    ///
    /// Set by `start()` when `depends_on` is non-empty and one or more
    /// dependencies are not yet `Running`. The process transitions to
    /// `Starting` once all dependencies are `Running`, or to `Failed` if
    /// a dependency enters a terminal state (`Failed` or permanently
    /// `Stopped` without `restart_on_exit`) or the dependency timeout
    /// expires.
    ///
    /// `is_alive()` returns `true` for `Waiting` - the process is
    /// logically pending, not terminated.
    Waiting,
}

impl ProcessState {
    /// Whether the process is alive (`Starting`, `Running`, `Stopping`, `Restarting`, or `Waiting`).
    ///
    /// Equivalent to the old `is_running() -> bool` semantics for consumers
    /// that only need a binary alive/not-alive check.
    pub fn is_alive(self) -> bool {
        matches!(
            self,
            ProcessState::Starting | ProcessState::Running | ProcessState::Stopping | ProcessState::Restarting | ProcessState::Waiting
        )
    }

    /// Whether the process has terminated (`Stopped`, `Crashed`, or `Failed`).
    pub fn is_terminated(self) -> bool {
        matches!(self, ProcessState::Stopped | ProcessState::Crashed | ProcessState::Failed)
    }
}

impl std::fmt::Display for ProcessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessState::Starting => write!(f, "starting"),
            ProcessState::Running => write!(f, "running"),
            ProcessState::Stopping => write!(f, "stopping"),
            ProcessState::Stopped => write!(f, "stopped"),
            ProcessState::Crashed => write!(f, "crashed"),
            ProcessState::Restarting => write!(f, "restarting"),
            ProcessState::Failed => write!(f, "failed"),
            ProcessState::Waiting => write!(f, "waiting"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_state_is_alive() {
        assert!(ProcessState::Starting.is_alive());
        assert!(ProcessState::Running.is_alive());
        assert!(ProcessState::Stopping.is_alive());
        assert!(ProcessState::Restarting.is_alive());
        assert!(ProcessState::Waiting.is_alive());
        assert!(!ProcessState::Stopped.is_alive());
        assert!(!ProcessState::Crashed.is_alive());
        assert!(!ProcessState::Failed.is_alive());
    }

    #[test]
    fn test_process_state_is_terminated() {
        assert!(!ProcessState::Starting.is_terminated());
        assert!(!ProcessState::Running.is_terminated());
        assert!(!ProcessState::Stopping.is_terminated());
        assert!(!ProcessState::Restarting.is_terminated());
        assert!(!ProcessState::Waiting.is_terminated());
        assert!(ProcessState::Stopped.is_terminated());
        assert!(ProcessState::Crashed.is_terminated());
        assert!(ProcessState::Failed.is_terminated());
    }

    #[test]
    fn test_process_state_default() {
        assert_eq!(ProcessState::default(), ProcessState::Starting);
    }

    #[test]
    fn test_process_state_display() {
        assert_eq!(ProcessState::Starting.to_string(), "starting");
        assert_eq!(ProcessState::Running.to_string(), "running");
        assert_eq!(ProcessState::Stopping.to_string(), "stopping");
        assert_eq!(ProcessState::Stopped.to_string(), "stopped");
        assert_eq!(ProcessState::Crashed.to_string(), "crashed");
        assert_eq!(ProcessState::Restarting.to_string(), "restarting");
        assert_eq!(ProcessState::Failed.to_string(), "failed");
        assert_eq!(ProcessState::Waiting.to_string(), "waiting");
    }

    #[test]
    fn test_process_state_equality() {
        assert_eq!(ProcessState::Running, ProcessState::Running);
        assert_ne!(ProcessState::Running, ProcessState::Stopped);
    }

    #[test]
    fn test_process_state_clone_copy() {
        let state = ProcessState::Running;
        let cloned = state;
        assert_eq!(state, cloned);
    }
}
