use serde::Deserialize;
use serde::Serialize;

/// Which exit states trigger an automatic restart.
///
/// Only applies when `restart_on_exit` is `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[must_use]
pub enum RestartTrigger {
    /// Restart only on `ProcessState::Crashed` (non-zero exit code or signal).
    /// A `ProcessState::Stopped` exit (success) is terminal - no restart.
    #[default]
    CrashOnly,
    /// Restart on any exit (`ProcessState::Stopped` or `ProcessState::Crashed`).
    Always,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restart_trigger_default() {
        assert_eq!(RestartTrigger::default(), RestartTrigger::CrashOnly);
    }

    #[test]
    fn test_restart_trigger_serde_roundtrip() {
        let trigger = RestartTrigger::Always;
        let json = serde_json::to_string(&trigger).unwrap();
        let deserialized: RestartTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(trigger, deserialized);
    }

    #[test]
    fn test_restart_trigger_crash_only_serde() {
        let trigger = RestartTrigger::CrashOnly;
        let json = serde_json::to_string(&trigger).unwrap();
        let deserialized: RestartTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(trigger, deserialized);
    }
}
