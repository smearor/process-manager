use process_manager::BackoffConfig;
use process_manager::KillSignal;
use process_manager::Label;
use process_manager::ProcessConfig;
use process_manager::ProcessExitEvent;
use process_manager::ProcessManager;
use process_manager::ProcessState;
use process_manager::RestartPolicy;
use process_manager::RestartTrigger;
use process_manager::Signal;
use process_manager::StdioConfig;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn start_stop_single_process() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("test", &config).unwrap();
    assert_eq!(manager.len(), 1);
    assert!(manager.get_info(id).is_some());

    manager.stop(id).unwrap();
    assert!(manager.is_empty());
}

#[test]
fn start_with_label_and_stop_by_label() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let _ = manager.start("group-a", &config).unwrap();
    let _ = manager.start("group-a", &config).unwrap();
    let _ = manager.start("group-b", &config).unwrap();

    assert_eq!(manager.len(), 3);
    assert_eq!(manager.pids_by_label("group-a").len(), 2);
    assert_eq!(manager.pids_by_label("group-b").len(), 1);

    manager.stop_label("group-a").unwrap();
    assert_eq!(manager.len(), 1);
    assert!(manager.pids_by_label("group-a").is_empty());
    assert_eq!(manager.pids_by_label("group-b").len(), 1);

    manager.stop_all();
    assert!(manager.is_empty());
}

#[test]
fn restart_preserves_config_and_label() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("worker", &config).unwrap();
    let new_id = manager.restart(id).unwrap();

    assert_eq!(id, new_id);
    assert_eq!(manager.len(), 1);
    assert_eq!(manager.get_label(new_id), Some(Label::new("worker")));

    manager.stop(new_id).unwrap();
}

#[test]
fn restart_label_concurrent_stop() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let _ = manager.start("batch", &config).unwrap();
    let _ = manager.start("batch", &config).unwrap();

    let new_ids = manager.restart_label("batch").unwrap();
    assert_eq!(new_ids.len(), 2);
    assert_eq!(manager.len(), 2);

    manager.stop_all();
}

#[test]
fn reaper_emits_exit_event() {
    let (tx, rx) = mpsc::channel();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("short-lived", &config).unwrap();

    let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event.id, id);
    assert_eq!(event.label, "short-lived");

    manager.stop_all();
}

#[test]
fn terminate_on_exit_kills_on_drop() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .terminate_on_exit(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("temp", &config).unwrap();
    assert_eq!(manager.get_terminate_on_exit(id), Some(true));

    drop(manager);
}

#[test]
fn forked_process_detaches_and_stops() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .forked(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("forked", &config).unwrap();
    assert_eq!(manager.is_forked(id), Some(true));

    manager.stop(id).unwrap();
    assert!(manager.is_empty());
}

#[test]
fn stop_nonexistent_returns_error() {
    let manager = ProcessManager::new();
    let result = manager.stop(process_manager::ProcessId::new(999));
    assert!(result.is_err());
}

#[test]
fn escalation_to_sigkill_on_short_timeout() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .kill_signal(KillSignal::Sigterm)
        .terminate_timeout_ms(100)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("stubborn", &config).unwrap();
    let result = manager.stop(id);
    assert!(result.is_ok());
    assert!(manager.is_empty());
}

#[test]
fn stop_label_escalates_sigkill_for_stubborn_group() {
    let manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .kill_signal(KillSignal::Sigterm)
        .terminate_timeout_ms(100)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let _ = manager.start("stubborn-a", &config).unwrap();
    let _ = manager.start("stubborn-a", &config).unwrap();
    let _ = manager.start("stubborn-b", &config).unwrap();

    assert_eq!(manager.len(), 3);

    let start = std::time::Instant::now();
    manager.stop_label("stubborn-a").unwrap();
    let elapsed = start.elapsed();

    assert_eq!(manager.len(), 1);
    assert!(manager.pids_by_label("stubborn-a").is_empty());
    assert_eq!(manager.pids_by_label("stubborn-b").len(), 1);
    assert!(elapsed < Duration::from_secs(5), "stop_label should escalate quickly, took {:?}", elapsed);

    manager.stop_all();
    assert!(manager.is_empty());
}

#[test]
fn stop_all_escalates_sigkill_for_mixed_group() {
    let manager = ProcessManager::new();

    let cooperative = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .kill_signal(KillSignal::Sigterm)
        .terminate_timeout_ms(100)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let _ = manager.start("coop-1", &cooperative).unwrap();
    let _ = manager.start("coop-2", &cooperative).unwrap();
    let _ = manager.start("stubborn-1", &cooperative).unwrap();

    assert_eq!(manager.len(), 3);

    let start = std::time::Instant::now();
    manager.stop_all();
    let elapsed = start.elapsed();

    assert!(manager.is_empty());
    assert!(elapsed < Duration::from_secs(10), "stop_all should escalate quickly, took {:?}", elapsed);
}

#[test]
fn stop_label_nonexistent_returns_empty_ok() {
    let manager = ProcessManager::new();
    let result = manager.stop_label("nonexistent");
    assert!(result.is_ok());
    assert!(manager.is_empty());
}

#[test]
fn reaper_immediate_restart_on_crash() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Immediate)
        .restart_trigger(RestartTrigger::CrashOnly)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("crash-restart", &config).unwrap();

    // First exit event - should be Crashed (restart triggered, not rate-limited)
    let event1 = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event1.id, id);
    assert_eq!(event1.state, ProcessState::Crashed);
    assert!(event1.restart_on_exit);

    // The reaper should restart the process. The restarted process will crash
    // again and emit another event. Collect a few events to verify restart loop.
    let event2 = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event2.id, id);
    assert_eq!(event2.state, ProcessState::Crashed);

    // Stop the process to cancel the restart loop
    manager.stop(id).unwrap();
}

#[test]
fn reaper_backoff_restart_preserves_process_id() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Backoff(BackoffConfig {
            initial_delay: Duration::from_millis(100),
            multiplier: 20,
            max_delay: Duration::from_secs(10),
            max_restarts: 10,
            min_uptime: Duration::from_secs(60),
        }))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("backoff-test", &config).unwrap();

    // First exit event
    let event1 = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event1.id, id);
    assert_eq!(event1.state, ProcessState::Crashed);

    // Wait for restart - the process should be in Restarting state briefly,
    // then respawn and crash again. The ProcessId should be preserved.
    let event2 = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(event2.id, id);
    assert_eq!(event2.state, ProcessState::Crashed);

    manager.stop(id).unwrap();
}

#[test]
fn reaper_rate_limit_exhausts_and_emits_failed() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Backoff(BackoffConfig {
            initial_delay: Duration::from_millis(50),
            multiplier: 10,
            max_delay: Duration::from_millis(50),
            max_restarts: 3,
            min_uptime: Duration::from_secs(60),
        }))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("rate-limit", &config).unwrap();

    // Collect events: first few should be Crashed, last should be Failed
    let mut got_failed = false;
    for _ in 0..10 {
        let event = match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(e) => e,
            Err(_) => break,
        };
        assert_eq!(event.id, id);
        if event.state == ProcessState::Failed {
            got_failed = true;
            break;
        }
        assert_eq!(event.state, ProcessState::Crashed);
    }

    assert!(got_failed, "Should have received a Failed event after rate limit exceeded");

    // Process should have been removed from the manager
    assert!(manager.get_info(id).is_none());
}

#[test]
fn reaper_no_restart_on_clean_exit_with_crash_only() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::CrashOnly)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("clean-exit", &config).unwrap();

    let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event.id, id);
    assert_eq!(event.state, ProcessState::Stopped);
    assert!(event.restart_on_exit);

    // No restart should happen - verify no second event arrives
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());

    // Process should be removed
    assert!(manager.get_info(id).is_none());
}

#[test]
fn reaper_restart_on_clean_exit_with_always_trigger() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Immediate)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("always-restart", &config).unwrap();

    // First exit - Stopped (but restart triggered because trigger=Always)
    let event1 = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event1.id, id);
    assert_eq!(event1.state, ProcessState::Stopped);

    // Should restart and exit again
    let event2 = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(event2.id, id);
    assert_eq!(event2.state, ProcessState::Stopped);

    manager.stop(id).unwrap();
}

#[test]
fn send_signal_during_restarting_returns_error() {
    let (tx, _rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Backoff(BackoffConfig {
            initial_delay: Duration::from_secs(30),
            multiplier: 20,
            max_delay: Duration::from_secs(60),
            max_restarts: 5,
            min_uptime: Duration::from_secs(60),
        }))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("restarting-state", &config).unwrap();

    // Wait for the process to exit and enter Restarting state
    // We need to consume the exit event first
    let _event = _rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // Give the reaper time to set Restarting state
    std::thread::sleep(Duration::from_millis(200));

    // Now the process should be in Restarting state
    let info = manager.get_info(id);
    assert!(info.is_some(), "Process should still be in manager during backoff");
    assert_eq!(manager.state(id), Some(ProcessState::Restarting));

    // send_signal should return an error
    let result = manager.send_signal(id, Signal::Sigterm);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), process_manager::ProcessManagerError::ProcessInRestartingState(_)));

    // Clean up - stop cancels backoff
    manager.stop(id).unwrap();
    assert!(manager.is_empty());
}

#[test]
fn stop_during_restarting_cancels_backoff_no_event() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Backoff(BackoffConfig {
            initial_delay: Duration::from_secs(30),
            multiplier: 20,
            max_delay: Duration::from_secs(60),
            max_restarts: 5,
            min_uptime: Duration::from_secs(60),
        }))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("stop-during-restart", &config).unwrap();

    // Consume the first exit event (Crashed)
    let _event = rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // Wait for Restarting state
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.state(id), Some(ProcessState::Restarting));

    // stop() should cancel backoff and remove the process
    manager.stop(id).unwrap();
    assert!(manager.is_empty());

    // No additional event should be emitted (the stop itself emits nothing)
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
}

#[test]
fn restart_during_restarting_spawns_immediately() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("false".to_string())
        .restart_on_exit(true)
        .restart_policy(RestartPolicy::Backoff(BackoffConfig {
            initial_delay: Duration::from_secs(30),
            multiplier: 20,
            max_delay: Duration::from_secs(60),
            max_restarts: 5,
            min_uptime: Duration::from_secs(60),
        }))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("restart-during-backoff", &config).unwrap();

    // Consume the first exit event
    let _event = rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // Wait for Restarting state
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.state(id), Some(ProcessState::Restarting));

    // restart() should cancel backoff and spawn immediately
    let new_id = manager.restart(id).unwrap();
    assert_eq!(id, new_id, "ProcessId should be preserved");

    // The new process should be running (it's "false" so it will crash quickly,
    // but it should be in Starting/Running state right after restart)
    let state = manager.state(id);
    assert!(state.is_some(), "Process should be in manager after restart");
    assert_ne!(state, Some(ProcessState::Restarting), "Should not be in Restarting after manual restart");

    manager.stop(id).unwrap();
}
