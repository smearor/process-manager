use crate::process::ProcessId;
use std::process::ExitStatus;

/// Information about a process that has exited.
///
/// Emitted by the reaper thread when it detects that a tracked process
/// has exited via non-blocking `try_wait()`.
#[derive(Debug, Clone)]
#[must_use]
pub struct ProcessExitEvent {
    /// The ID of the process that exited.
    pub id: ProcessId,

    /// The label of the process that exited.
    pub label: String,

    /// The PID of the process that exited.
    pub pid: u32,

    /// Whether the process should be restarted (from `config.restart_on_exit`).
    pub restart_on_exit: bool,

    /// The exit status of the process, if available.
    ///
    /// `None` if the child handle was missing or `try_wait()` returned an error.
    pub exit_status: Option<ExitStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_exit_event_construction() {
        let event = ProcessExitEvent {
            id: ProcessId::new(42),
            label: "test".to_string(),
            pid: 12345,
            restart_on_exit: true,
            exit_status: None,
        };
        assert_eq!(event.id.raw(), 42);
        assert_eq!(event.label, "test");
        assert_eq!(event.pid, 12345);
        assert!(event.restart_on_exit);
        assert!(event.exit_status.is_none());
    }

    #[test]
    fn test_process_exit_event_clone() {
        let event = ProcessExitEvent {
            id: ProcessId::new(1),
            label: "app".to_string(),
            pid: 999,
            restart_on_exit: false,
            exit_status: None,
        };
        let cloned = event.clone();
        assert_eq!(cloned.id, event.id);
        assert_eq!(cloned.label, event.label);
        assert_eq!(cloned.pid, event.pid);
        assert_eq!(cloned.restart_on_exit, event.restart_on_exit);
        assert_eq!(cloned.exit_status, event.exit_status);
    }

    #[test]
    fn test_process_exit_event_with_status() {
        use std::process::Command;
        let status = Command::new("true").status().unwrap();
        let event = ProcessExitEvent {
            id: ProcessId::new(7),
            label: "worker".to_string(),
            pid: 4321,
            restart_on_exit: false,
            exit_status: Some(status),
        };
        assert!(event.exit_status.is_some());
        assert!(event.exit_status.unwrap().success());
    }

    #[test]
    fn test_process_exit_event_with_failure_status() {
        use std::process::Command;
        let status = Command::new("false").status().unwrap();
        let event = ProcessExitEvent {
            id: ProcessId::new(8),
            label: "failing".to_string(),
            pid: 1111,
            restart_on_exit: false,
            exit_status: Some(status),
        };
        assert!(event.exit_status.is_some());
        assert!(!event.exit_status.unwrap().success());
    }
}
