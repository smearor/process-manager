use process_manager::KillSignal;
use process_manager::ProcessConfig;
use process_manager::ProcessManager;
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

    assert_ne!(id, new_id);
    assert_eq!(manager.len(), 1);
    assert_eq!(manager.get_label(new_id), Some("worker".to_string()));

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
