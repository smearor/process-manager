use serde::Deserialize;
use serde::Serialize;

/// Strategy for restarting processes within a supervisor group.
///
/// Determines which processes are restarted when one process in the group
/// crashes. Follows the Erlang OTP supervisor model.
///
/// Only applies when `restart_on_exit` is `true` for the crashed process.
/// The strategy is read from the crashed process's config - all processes
/// in the same label group share the same strategy (enforced at config
/// validation time or documented as a convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[must_use]
pub enum SupervisorStrategy {
    /// Restart only the crashed process.
    ///
    /// Other processes in the group are unaffected. Suitable for
    /// independent processes that share a label for grouping but do
    /// not depend on each other.
    #[default]
    OneForOne,

    /// Restart all processes in the group when any one crashes.
    ///
    /// Suitable for tightly coupled processes where one crash invalidates
    /// the state of all others (e.g. compositor + clients).
    OneForAll,

    /// Restart the crashed process and all processes started after it
    /// within the same group.
    ///
    /// Processes are ordered by their `spawn_sequence` (a monotonically
    /// increasing counter assigned at `start()` time). Suitable for chains
    /// where later processes depend on earlier ones but not vice versa.
    RestForOne,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supervisor_strategy_default() {
        assert_eq!(SupervisorStrategy::default(), SupervisorStrategy::OneForOne);
    }

    #[test]
    fn test_supervisor_strategy_serde_roundtrip() {
        let strategy = SupervisorStrategy::OneForAll;
        let json = serde_json::to_string(&strategy).unwrap();
        let deserialized: SupervisorStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, deserialized);
    }

    #[test]
    fn test_supervisor_strategy_rest_for_one_serde() {
        let strategy = SupervisorStrategy::RestForOne;
        let json = serde_json::to_string(&strategy).unwrap();
        let deserialized: SupervisorStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, deserialized);
    }

    #[test]
    fn test_supervisor_strategy_one_for_one_serde() {
        let strategy = SupervisorStrategy::OneForOne;
        let json = serde_json::to_string(&strategy).unwrap();
        let deserialized: SupervisorStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, deserialized);
    }
}
