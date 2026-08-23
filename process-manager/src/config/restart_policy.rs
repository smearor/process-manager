use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;

/// Restart policy for a process.
///
/// Controls automatic restart behavior when a process exits unexpectedly.
/// Only applies when `restart_on_exit` is `true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[must_use]
pub enum RestartPolicy {
    /// Restart immediately on exit, no delay or limit.
    #[default]
    Immediate,
    /// Restart with exponential backoff and rate limiting.
    Backoff(BackoffConfig),
}

/// Configuration for exponential backoff restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct BackoffConfig {
    /// Initial delay before the first restart.
    pub initial_delay: Duration,
    /// Multiplier applied to the delay after each restart, in tenths.
    /// For example, `20` means 2.0x, `15` means 1.5x, `10` means 1.0x.
    pub multiplier: u32,
    /// Maximum delay between restarts.
    pub max_delay: Duration,
    /// Maximum number of consecutive restarts before giving up.
    /// The counter resets to 0 when the process runs stably for at least
    /// `min_uptime` after a restart.
    pub max_restarts: u32,
    /// Minimum continuous uptime required before the restart counter resets.
    /// If a process crashes before reaching this uptime, the counter keeps
    /// incrementing. This prevents backoff from being defeated by the
    /// backoff sleep time itself consuming a sliding time window.
    pub min_uptime: Duration,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            multiplier: 20,
            max_delay: Duration::from_secs(60),
            max_restarts: 3,
            min_uptime: Duration::from_secs(10),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_config_default() {
        let config = BackoffConfig::default();
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.multiplier, 20);
        assert_eq!(config.max_delay, Duration::from_secs(60));
        assert_eq!(config.max_restarts, 3);
        assert_eq!(config.min_uptime, Duration::from_secs(10));
    }

    #[test]
    fn test_restart_policy_default() {
        assert_eq!(RestartPolicy::default(), RestartPolicy::Immediate);
    }

    #[test]
    fn test_restart_policy_immediate_serde_roundtrip() {
        let policy = RestartPolicy::Immediate;
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: RestartPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, deserialized);
    }

    #[test]
    fn test_restart_policy_backoff_serde_roundtrip() {
        let policy = RestartPolicy::Backoff(BackoffConfig::default());
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: RestartPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, deserialized);
    }

    #[test]
    fn test_backoff_config_serde_roundtrip() {
        let config = BackoffConfig {
            initial_delay: Duration::from_millis(500),
            multiplier: 15,
            max_delay: Duration::from_secs(30),
            max_restarts: 5,
            min_uptime: Duration::from_secs(15),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackoffConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }
}
