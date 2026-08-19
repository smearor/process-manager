use crate::process::ProcessId;

/// Information about a process that has exited.
///
/// Emitted by the reaper thread when it detects that a tracked process
/// has exited via non-blocking `try_wait()`.
#[derive(Debug, Clone)]
pub struct ProcessExitEvent {
    /// The ID of the process that exited.
    pub id: ProcessId,

    /// The label of the process that exited.
    pub label: String,

    /// The PID of the process that exited.
    pub pid: u32,

    /// Whether the process should be restarted (from `config.restart_on_exit`).
    pub restart_on_exit: bool,
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
        };
        assert_eq!(event.id.raw(), 42);
        assert_eq!(event.label, "test");
        assert_eq!(event.pid, 12345);
        assert!(event.restart_on_exit);
    }

    #[test]
    fn test_process_exit_event_clone() {
        let event = ProcessExitEvent {
            id: ProcessId::new(1),
            label: "app".to_string(),
            pid: 999,
            restart_on_exit: false,
        };
        let cloned = event.clone();
        assert_eq!(cloned.id, event.id);
        assert_eq!(cloned.label, event.label);
        assert_eq!(cloned.pid, event.pid);
        assert_eq!(cloned.restart_on_exit, event.restart_on_exit);
    }
}
