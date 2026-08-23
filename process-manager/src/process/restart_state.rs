use crate::config::BackoffConfig;
use std::time::Duration;
use std::time::Instant;

/// Per-process restart tracking state.
///
/// Maintained by the reaper thread to track consecutive restart count
/// and backoff timing. Stored alongside the `Process` in the `DashMap`.
///
/// The restart counter resets to 0 only when the process achieves stable
/// uptime >= `min_uptime` after a restart. This follows the same pattern as
/// Kubernetes and systemd: the backoff counter is tied to process health,
/// not wall-clock time. A sliding time window would be defeated by the
/// backoff sleep itself consuming the window.
#[derive(Debug, Clone)]
pub struct RestartState {
    /// Consecutive restart count (resets to 0 on stable uptime >= min_uptime).
    restart_count: u32,
    /// When the process was last started (for measuring uptime).
    /// `None` until the first start or after a reset.
    last_started_at: Option<Instant>,
    /// When the process is eligible for restart (backoff timer).
    /// `None` when no restart is pending.
    next_eligible_restart: Option<Instant>,
}

impl Default for RestartState {
    fn default() -> Self {
        Self::new()
    }
}

impl RestartState {
    /// Create a new `RestartState` with zero restarts and no pending backoff.
    pub fn new() -> Self {
        Self {
            restart_count: 0,
            last_started_at: None,
            next_eligible_restart: None,
        }
    }

    /// The current consecutive restart count.
    pub fn restart_count(&self) -> u32 {
        self.restart_count
    }

    /// The timestamp when the process was last started, if any.
    pub fn last_started_at(&self) -> Option<Instant> {
        self.last_started_at
    }

    /// The timestamp when the process is eligible for restart, if a backoff is pending.
    pub fn next_eligible_restart(&self) -> Option<Instant> {
        self.next_eligible_restart
    }

    /// Compute the current backoff delay based on the consecutive restart count.
    ///
    /// The delay is: `initial_delay * (multiplier / 10)^restart_count`
    /// capped at `max_delay`.
    ///
    /// When `restart_count == 0` (first crash or after stable-uptime reset),
    /// returns `initial_delay`.
    pub fn current_delay(&self, config: &BackoffConfig) -> Duration {
        let multiplier = config.multiplier as f64 / 10.0;
        let delay_secs = config.initial_delay.as_secs_f64() * multiplier.powi(self.restart_count as i32);
        let delay = Duration::from_secs_f64(delay_secs);
        delay.min(config.max_delay)
    }

    /// Check if the process should be rate-limited (give up restarting).
    ///
    /// Returns `true` when `restart_count >= max_restarts`.
    /// The counter only resets on stable uptime, so this is reliable
    /// regardless of how long the backoff sleep takes.
    pub fn is_rate_limited(&self, config: &BackoffConfig) -> bool {
        self.restart_count >= config.max_restarts
    }

    /// Check if the process has achieved stable uptime and reset the counter.
    ///
    /// Called by the reaper on each poll cycle for `Running` processes.
    /// If the process has been running for >= `min_uptime` since its last
    /// start, the restart counter resets to 0.
    pub fn check_stable_uptime(&mut self, config: &BackoffConfig, now: Instant) {
        if let Some(started_at) = self.last_started_at
            && now.duration_since(started_at) >= config.min_uptime
            && self.restart_count > 0
        {
            self.restart_count = 0;
        }
    }

    /// Check if the process is eligible for restart (backoff timer elapsed).
    ///
    /// Returns `true` when `next_eligible_restart` is `None` or
    /// `now >= next_eligible_restart`.
    pub fn is_eligible_for_restart(&self, now: Instant) -> bool {
        self.next_eligible_restart.is_none_or(|eligible| now >= eligible)
    }

    /// Record a restart: increment counter.
    ///
    /// The `last_started_at` timestamp is set separately via `mark_started()`
    /// when the new process is actually spawned.
    pub fn record_restart(&mut self) {
        self.restart_count += 1;
    }

    /// Mark the process as started - set `last_started_at` to now.
    ///
    /// Called when a new OS process is spawned (either initial start or restart).
    pub fn mark_started(&mut self, now: Instant) {
        self.last_started_at = Some(now);
    }

    /// Schedule the next eligible restart time (backoff timer).
    pub fn schedule_restart(&mut self, eligible_at: Instant) {
        self.next_eligible_restart = Some(eligible_at);
    }

    /// Cancel a pending backoff timer.
    pub fn cancel_pending_restart(&mut self) {
        self.next_eligible_restart = None;
    }

    /// Reset the restart state to initial values.
    pub fn reset(&mut self) {
        self.restart_count = 0;
        self.last_started_at = None;
        self.next_eligible_restart = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BackoffConfig {
        BackoffConfig {
            initial_delay: Duration::from_secs(2),
            multiplier: 20,
            max_delay: Duration::from_secs(60),
            max_restarts: 5,
            min_uptime: Duration::from_secs(10),
        }
    }

    #[test]
    fn test_restart_state_new() {
        let state = RestartState::new();
        assert_eq!(state.restart_count(), 0);
        assert!(state.last_started_at().is_none());
        assert!(state.next_eligible_restart().is_none());
    }

    #[test]
    fn test_record_restart_increments_count() {
        let mut state = RestartState::new();
        state.record_restart();
        assert_eq!(state.restart_count(), 1);
        state.record_restart();
        assert_eq!(state.restart_count(), 2);
    }

    #[test]
    fn test_current_delay_first_crash() {
        let state = RestartState::new();
        let config = test_config();
        assert_eq!(state.current_delay(&config), Duration::from_secs(2));
    }

    #[test]
    fn test_current_delay_exponential() {
        let mut state = RestartState::new();
        let config = test_config();
        state.record_restart();
        assert_eq!(state.current_delay(&config), Duration::from_secs(4));
        state.record_restart();
        assert_eq!(state.current_delay(&config), Duration::from_secs(8));
        state.record_restart();
        assert_eq!(state.current_delay(&config), Duration::from_secs(16));
        state.record_restart();
        assert_eq!(state.current_delay(&config), Duration::from_secs(32));
    }

    #[test]
    fn test_current_delay_capped() {
        let mut state = RestartState::new();
        let config = test_config();
        for _ in 0..10 {
            state.record_restart();
        }
        assert_eq!(state.current_delay(&config), Duration::from_secs(60));
    }

    #[test]
    fn test_is_rate_limited() {
        let mut state = RestartState::new();
        let config = test_config();
        assert!(!state.is_rate_limited(&config));
        for _ in 0..5 {
            state.record_restart();
        }
        assert!(state.is_rate_limited(&config));
    }

    #[test]
    fn test_check_stable_uptime_resets_counter() {
        let mut state = RestartState::new();
        let config = test_config();
        let now = Instant::now();
        state.mark_started(now);
        state.record_restart();
        state.record_restart();
        assert_eq!(state.restart_count(), 2);

        // Before min_uptime - no reset
        state.check_stable_uptime(&config, now + Duration::from_secs(5));
        assert_eq!(state.restart_count(), 2);

        // After min_uptime - reset
        state.check_stable_uptime(&config, now + Duration::from_secs(11));
        assert_eq!(state.restart_count(), 0);
    }

    #[test]
    fn test_check_stable_uptime_no_reset_when_count_zero() {
        let mut state = RestartState::new();
        let config = test_config();
        let now = Instant::now();
        state.mark_started(now);
        state.check_stable_uptime(&config, now + Duration::from_secs(20));
        assert_eq!(state.restart_count(), 0);
    }

    #[test]
    fn test_check_stable_uptime_no_reset_without_start_time() {
        let mut state = RestartState::new();
        let config = test_config();
        state.record_restart();
        state.check_stable_uptime(&config, Instant::now());
        assert_eq!(state.restart_count(), 1);
    }

    #[test]
    fn test_is_eligible_for_restart() {
        let mut state = RestartState::new();
        let now = Instant::now();
        assert!(state.is_eligible_for_restart(now));

        state.schedule_restart(now + Duration::from_secs(10));
        assert!(!state.is_eligible_for_restart(now));
        assert!(state.is_eligible_for_restart(now + Duration::from_secs(10)));
        assert!(state.is_eligible_for_restart(now + Duration::from_secs(15)));
    }

    #[test]
    fn test_cancel_pending_restart() {
        let mut state = RestartState::new();
        state.schedule_restart(Instant::now() + Duration::from_secs(10));
        assert!(state.next_eligible_restart().is_some());
        state.cancel_pending_restart();
        assert!(state.next_eligible_restart().is_none());
    }

    #[test]
    fn test_reset() {
        let mut state = RestartState::new();
        state.record_restart();
        state.record_restart();
        state.mark_started(Instant::now());
        state.schedule_restart(Instant::now() + Duration::from_secs(10));
        state.reset();
        assert_eq!(state.restart_count(), 0);
        assert!(state.last_started_at().is_none());
        assert!(state.next_eligible_restart().is_none());
    }

    #[test]
    fn test_stable_uptime_reset_makes_rate_limited_false() {
        let mut state = RestartState::new();
        let config = test_config();
        let now = Instant::now();
        state.mark_started(now);
        for _ in 0..5 {
            state.record_restart();
        }
        assert!(state.is_rate_limited(&config));
        state.check_stable_uptime(&config, now + Duration::from_secs(11));
        assert!(!state.is_rate_limited(&config));
        assert_eq!(state.current_delay(&config), Duration::from_secs(2));
    }
}
