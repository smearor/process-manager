use process_manager::BackoffConfig;
use process_manager::DependencyRef;
use process_manager::ProcessConfig;
use process_manager::ProcessExitEvent;
use process_manager::ProcessManager;
use process_manager::ProcessState;
use process_manager::RestartPolicy;
use process_manager::RestartTrigger;
use process_manager::StdioConfig;
use process_manager::SupervisorStrategy;
use std::sync::mpsc;
use std::time::Duration;

/// Helper: a backoff config that allows 1 restart then gives up.
/// Uses a long initial_delay so the process stays in Restarting for 60s,
/// preventing rapid crash-restart loops that can exhaust system resources.
fn one_restart_backoff() -> BackoffConfig {
    BackoffConfig {
        initial_delay: Duration::from_secs(60),
        multiplier: 20,
        max_delay: Duration::from_secs(300),
        max_restarts: 1,
        min_uptime: Duration::from_secs(10),
    }
}

/// Helper: a backoff config with a short initial_delay for tests that need
/// a process to restart quickly and then fail (max_restarts: 1).
/// Total cycle: crash → 100ms backoff → restart → crash → Failed.
fn fast_one_restart_backoff() -> BackoffConfig {
    BackoffConfig {
        initial_delay: Duration::from_millis(100),
        multiplier: 10,
        max_delay: Duration::from_secs(1),
        max_restarts: 1,
        min_uptime: Duration::from_secs(10),
    }
}

/// Helper: a backoff config with a 5s initial_delay for tests that need
/// to observe a process in Restarting state for a deterministic window.
fn slow_one_restart_backoff() -> BackoffConfig {
    BackoffConfig {
        initial_delay: Duration::from_secs(5),
        multiplier: 10,
        max_delay: Duration::from_secs(30),
        max_restarts: 1,
        min_uptime: Duration::from_secs(10),
    }
}

/// Helper: create a config for a long-running process.
fn sleep_config() -> ProcessConfig {
    ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build()
}

// ============================================================
// Cycle detection tests
// ============================================================

#[test]
fn start_rejects_circular_dependency() {
    let manager = ProcessManager::new();

    // Start process A that depends on label "b"
    let config_a = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("b")])
        .build();

    let id_a = manager.start("a", &config_a).unwrap();
    // A is in Waiting state since "b" is not running
    let info_a = manager.get_info(id_a).unwrap();
    assert_eq!(info_a.state, ProcessState::Waiting);

    // Try to start B that depends on label "a" - should fail with DependencyCycle
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("a")])
        .build();

    let result = manager.start("b", &config_b);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Circular dependency"), "Expected circular dependency error, got: {err}");
}

#[test]
fn start_rejects_self_dependency() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("self")])
        .build();

    let result = manager.start("self", &config);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("Circular dependency"),
        "Expected circular dependency error for self-dependency"
    );
}

#[test]
fn start_allows_diamond_dependency() {
    let manager = ProcessManager::new();

    // Start D (no deps) - runs immediately
    let _id_d = manager.start("d", &sleep_config()).unwrap();
    // Wait for D to be Running
    std::thread::sleep(Duration::from_millis(100));

    // Start B (depends on D)
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("d")])
        .build();
    let id_b = manager.start("b", &config_b).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_b).unwrap().state, ProcessState::Running);

    // Start C (depends on D) - D is already Running, should start
    let config_c = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("d")])
        .build();
    let id_c = manager.start("c", &config_c).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_c).unwrap().state, ProcessState::Running);

    // Start A (depends on B and C) - both Running, should start
    let config_a = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("b"), DependencyRef::label("c")])
        .build();
    let id_a = manager.start("a", &config_a).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_a).unwrap().state, ProcessState::Running);

    // Cleanup
    manager.stop_all();
}

// ============================================================
// Waiting state tests
// ============================================================

#[test]
fn start_with_unsatisfied_deps_enters_waiting() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("missing")])
        .dependency_timeout_ms(5000)
        .build();

    let id = manager.start("dependent", &config).unwrap();
    let info = manager.get_info(id).unwrap();
    assert_eq!(info.state, ProcessState::Waiting);
    assert_eq!(info.pid, 0);

    manager.stop(id).unwrap();
    assert!(manager.is_empty());
}

#[test]
fn start_with_satisfied_deps_spawns_immediately() {
    let manager = ProcessManager::new();

    // Start dependency first
    let id_dep = manager.start("dep", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_dep).unwrap().state, ProcessState::Running);

    // Start dependent - should spawn immediately
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("dep")])
        .build();

    let id = manager.start("dependent", &config).unwrap();
    let info = manager.get_info(id).unwrap();
    assert_ne!(info.state, ProcessState::Waiting);
    assert_ne!(info.pid, 0);

    manager.stop_all();
}

// ============================================================
// start_with_deps blocking API tests
// ============================================================

#[test]
fn start_with_deps_blocks_until_dependency_running() {
    let manager = ProcessManager::new();

    // Start dependency first (takes a moment to reach Running)
    let _id_dep = manager.start("dep", &sleep_config()).unwrap();

    // Start dependent with blocking API - should block until dep is Running
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("dep")])
        .dependency_timeout_ms(5000)
        .build();

    let id = manager.start_with_deps("dependent", &config).unwrap();
    let info = manager.get_info(id).unwrap();
    assert_ne!(info.state, ProcessState::Waiting);

    manager.stop_all();
}

#[test]
fn start_with_deps_times_out_when_dependency_never_starts() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("nonexistent")])
        .dependency_timeout_ms(200)
        .build();

    let result = manager.start_with_deps("dependent", &config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("timeout") || err.to_string().contains("Timeout"),
        "Expected timeout error, got: {err}"
    );
}

// ============================================================
// Supervisor strategy tests (with reaper)
// ============================================================

#[test]
fn one_for_one_restarts_only_crashed_process() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start two processes in the same label group with OneForOne strategy.
    // Use BackoffConfig with max_restarts:1 to prevent infinite crash loops.
    let config = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForOne)
        .build();

    let id1 = manager.start("group", &config).unwrap();
    let id2 = manager.start("group", &sleep_config()).unwrap();

    std::thread::sleep(Duration::from_millis(100));

    // id1 (false) should crash and emit a Crashed event, id2 (sleep) should be unaffected
    let mut found_crashed = false;
    let timeout = Duration::from_secs(3);
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100))
            && event.id == id1
        {
            found_crashed = true;
        }
        if found_crashed {
            break;
        }
    }
    assert!(found_crashed, "Expected crashed event for id1");

    // id2 should still be alive
    let info2 = manager.get_info(id2);
    assert!(info2.is_some(), "id2 should still be tracked");

    manager.stop_all();
}

#[test]
fn one_for_all_cascades_kill_to_all_group_members() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start a crashing process with OneForAll strategy.
    // Use BackoffConfig with max_restarts:1 and 60s initial_delay so the
    // crash-cascade happens exactly once, then all processes sit in
    // Restarting state until the test cleans up.
    let crash_config = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .build();

    let sleep_config_cascade = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .build();

    // Start sleep processes first so they exist when the crash process crashes.
    let _id2 = manager.start("group", &sleep_config_cascade).unwrap();
    let _id3 = manager.start("group", &sleep_config_cascade).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Start the crashing process last - it will crash immediately, and the
    // reaper should cascade-kill id2 and id3 (OneForAll strategy).
    let _id1 = manager.start("group", &crash_config).unwrap();

    // Wait for id1 to crash and cascade-kill id2 and id3
    let mut got_crashed = false;
    let mut got_stopped = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            if event.state == ProcessState::Crashed {
                got_crashed = true;
            }
            if event.state == ProcessState::Stopped {
                got_stopped = true;
            }
        }
        if got_crashed && got_stopped {
            break;
        }
    }

    assert!(got_crashed, "Expected at least one Crashed event");
    assert!(got_stopped, "Expected at least one Stopped event from cascade");

    manager.stop_all();
}

#[test]
fn cascade_flag_prevents_recursive_cascade() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Use BackoffConfig with max_restarts:1 to prevent infinite crash loops.
    let crash_config = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .build();

    let sleep_config_cascade = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .build();

    let _id1 = manager.start("group", &crash_config).unwrap();
    let _id2 = manager.start("group", &sleep_config_cascade).unwrap();
    let _id3 = manager.start("group", &sleep_config_cascade).unwrap();

    // Wait and count events - should not get infinite cascade
    let mut crash_count = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100))
            && event.state == ProcessState::Crashed
        {
            crash_count += 1;
        }
    }

    // Should have exactly 1 Crashed event (the original crash).
    // Cascade-killed processes emit Stopped, not Crashed.
    assert_eq!(crash_count, 1, "Expected exactly 1 Crashed event, got {crash_count}");

    manager.stop_all();
}

// ============================================================
// cascade_stop tests
// ============================================================

#[test]
fn cascade_stop_stops_dependents() {
    let manager = ProcessManager::new();

    // Start dependency with cascade_stop = true
    let dep_config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .cascade_stop(true)
        .build();
    let id_dep = manager.start("dep", &dep_config).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // Start dependent
    let dependent_config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_dep)])
        .build();
    let _id_dependent = manager.start("dependent", &dependent_config).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(manager.len(), 2);

    // Stop the dependency - should cascade-stop the dependent
    manager.stop(id_dep).unwrap();
    assert!(manager.is_empty(), "Expected manager to be empty after cascade stop");
}

// ============================================================
// dependents() and group_members() tests
// ============================================================

#[test]
fn dependents_returns_correct_process_ids() {
    let manager = ProcessManager::new();

    let id_a = manager.start("a", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_a)])
        .build();
    let id_b = manager.start("b", &config_b).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let dependents = manager.dependents(id_a);
    assert_eq!(dependents.len(), 1);
    assert!(dependents.contains(&id_b));

    let no_dependents = manager.dependents(id_b);
    assert!(no_dependents.is_empty());

    manager.stop_all();
}

#[test]
fn group_members_returns_all_with_same_label() {
    let manager = ProcessManager::new();

    let id1 = manager.start("group-a", &sleep_config()).unwrap();
    let id2 = manager.start("group-a", &sleep_config()).unwrap();
    let id3 = manager.start("group-b", &sleep_config()).unwrap();

    let group_a = manager.group_members("group-a");
    assert_eq!(group_a.len(), 2);
    assert!(group_a.contains(&id1));
    assert!(group_a.contains(&id2));

    let group_b = manager.group_members("group-b");
    assert_eq!(group_b.len(), 1);
    assert!(group_b.contains(&id3));

    let group_c = manager.group_members("group-c");
    assert!(group_c.is_empty());

    manager.stop_all();
}

// ============================================================
// Waiting state stop with event emission
// ============================================================

#[test]
fn stop_waiting_process_emits_stopped_event() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("nonexistent")])
        .dependency_timeout_ms(60000)
        .build();

    let id = manager.start("waiting", &config).unwrap();
    assert_eq!(manager.get_info(id).unwrap().state, ProcessState::Waiting);

    // Stop the waiting process
    manager.stop(id).unwrap();
    assert!(manager.is_empty());

    // Should receive a Stopped event
    let event = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(event.id, id);
    assert_eq!(event.state, ProcessState::Stopped);
}

// ============================================================
// spawn_sequence tests
// ============================================================

#[test]
fn spawn_sequence_increases_monotonically() {
    let manager = ProcessManager::new();

    let id1 = manager.start("a", &sleep_config()).unwrap();
    let id2 = manager.start("b", &sleep_config()).unwrap();
    let id3 = manager.start("c", &sleep_config()).unwrap();

    let seq1 = manager.get_info(id1).unwrap().spawn_sequence;
    let seq2 = manager.get_info(id2).unwrap().spawn_sequence;
    let seq3 = manager.get_info(id3).unwrap().spawn_sequence;

    assert!(seq1 < seq2, "seq1 ({seq1}) should be < seq2 ({seq2})");
    assert!(seq2 < seq3, "seq2 ({seq2}) should be < seq3 ({seq3})");

    manager.stop_all();
}

// ============================================================
// Indirect cycle detection
// ============================================================

#[test]
fn start_rejects_indirect_circular_dependency() {
    let manager = ProcessManager::new();

    // A depends on "b", B depends on "c", C depends on "a" → cycle
    let config_a = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("b")])
        .build();
    let _id_a = manager.start("a", &config_a).unwrap();

    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("c")])
        .build();
    let _id_b = manager.start("b", &config_b).unwrap();

    // Starting C (depends on "a") should detect the cycle A→B→C→A
    let config_c = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("a")])
        .build();

    let result = manager.start("c", &config_c);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("Circular dependency"),
        "Expected circular dependency error for indirect cycle"
    );

    manager.stop_all();
}

// ============================================================
// Cascade stop disabled
// ============================================================

#[test]
fn cascade_stop_disabled_does_not_stop_dependents() {
    let manager = ProcessManager::new();

    // Start dependency with cascade_stop = false (default)
    let id_dep = manager.start("dep", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // Start dependent
    let dependent_config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_dep)])
        .build();
    let id_dependent = manager.start("dependent", &dependent_config).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(manager.len(), 2);

    // Stop the dependency - should NOT cascade-stop the dependent
    manager.stop(id_dep).unwrap();
    assert_eq!(manager.len(), 1, "Dependent should still be running after non-cascade stop");

    // Clean up
    manager.stop(id_dependent).unwrap();
}

// ============================================================
// Waiting state reported correctly
// ============================================================

#[test]
fn waiting_state_reported_correctly() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("nonexistent")])
        .dependency_timeout_ms(60000)
        .build();

    let id = manager.start("waiting", &config).unwrap();

    // state() should return Waiting
    assert_eq!(manager.state(id).unwrap(), ProcessState::Waiting);

    // get_info() should also return Waiting
    let info = manager.get_info(id).unwrap();
    assert_eq!(info.state, ProcessState::Waiting);

    manager.stop(id).unwrap();
}

// ============================================================
// Dependency on ProcessId
// ============================================================

#[test]
fn dependency_on_process_id_starts_after_dependency() {
    let manager = ProcessManager::new();

    // Start dependency
    let id_dep = manager.start("dep", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_dep).unwrap().state, ProcessState::Running);

    // Start dependent with Id dependency
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_dep)])
        .build();

    let id = manager.start("dependent", &config).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id).unwrap().state, ProcessState::Running);

    manager.stop_all();
}

// ============================================================
// Multiple dependencies
// ============================================================

#[test]
fn multiple_dependencies_start_after_all_ready() {
    let manager = ProcessManager::new();

    // Start two dependencies
    let id_a = manager.start("a", &sleep_config()).unwrap();
    let id_b = manager.start("b", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // Start process C that depends on both A and B
    let config_c = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_a), DependencyRef::Id(id_b)])
        .build();

    let id_c = manager.start("c", &config_c).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(manager.get_info(id_c).unwrap().state, ProcessState::Running);

    manager.stop_all();
}

// ============================================================
// Fail-fast on dependency Stopped without restart
// ============================================================

#[test]
fn fail_fast_on_dependency_stopped_without_restart() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start A (short sleep, no restart) - will exit cleanly → Stopped → removed
    let config_a = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["0.2".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();
    let id_a = manager.start("dep", &config_a).unwrap();

    // Wait for A to exit and be removed
    std::thread::sleep(Duration::from_millis(400));
    assert!(manager.get_info(id_a).is_none(), "A should have exited and been removed");

    // Start B (depends on Id(a_id)) - A is gone → B enters Waiting → fail-fast
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_a)])
        .dependency_timeout_ms(30000)
        .build();
    let id_b = manager.start("dependent", &config_b).unwrap();

    // B should fail-fast because dependency A is gone (terminal)
    let mut got_failed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100))
            && event.id == id_b
            && event.state == ProcessState::Failed
        {
            got_failed = true;
            break;
        }
    }
    assert!(got_failed, "Expected B to fail-fast when dependency A is stopped without restart");

    manager.stop_all();
}

// ============================================================
// Fail-fast on dependency Failed
// ============================================================

#[test]
fn fail_fast_on_dependency_failed() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start A (crashes, restarts once, crashes again → Failed → removed)
    let config_a = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(fast_one_restart_backoff()))
        .build();
    let id_a = manager.start("dep", &config_a).unwrap();

    // Wait for A to crash, restart, crash again, and be removed (~300ms)
    let mut a_removed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if manager.get_info(id_a).is_none() {
            a_removed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(a_removed, "A should have crashed, restarted, crashed again, and been removed");

    // Start B (depends on Id(a_id)) - A is gone → B enters Waiting → fail-fast
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_a)])
        .dependency_timeout_ms(30000)
        .build();
    let id_b = manager.start("dependent", &config_b).unwrap();

    // B should fail-fast because dependency A is gone (Failed → removed)
    let mut got_failed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100))
            && event.id == id_b
            && event.state == ProcessState::Failed
        {
            got_failed = true;
            break;
        }
    }
    assert!(got_failed, "Expected B to fail-fast when dependency A has Failed");

    manager.stop_all();
}

// ============================================================
// No fail-fast during dependency Restarting
// ============================================================

#[test]
fn no_fail_fast_during_dependency_restarting() {
    let (tx, _rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start A (crashes, enters Restarting with 5s delay)
    let config_a = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(slow_one_restart_backoff()))
        .build();
    let id_a = manager.start("dep", &config_a).unwrap();

    // Wait for A to crash and enter Restarting (~100ms)
    std::thread::sleep(Duration::from_millis(200));

    // Start B (depends on Id(a_id)) - A is Restarting → B enters Waiting
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::Id(id_a)])
        .dependency_timeout_ms(30000)
        .build();
    let id_b = manager.start("dependent", &config_b).unwrap();

    // Wait a bit - B should still be Waiting (not Failed) because A is Restarting
    std::thread::sleep(Duration::from_millis(200));

    let info_b = manager.get_info(id_b);
    assert!(info_b.is_some(), "B should still be tracked (not fail-fast during dependency Restarting)");
    assert_eq!(
        info_b.unwrap().state,
        ProcessState::Waiting,
        "B should be Waiting, not Failed, while dependency A is Restarting"
    );

    manager.stop_all();
}

// ============================================================
// RestForOne strategy tests
// ============================================================

#[test]
fn rest_for_one_cascades_kill_to_later_processes() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let crash_config = ProcessConfig::builder()
        .command("sleep 0.3 && false")
        .shell(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    let sleep_config_rfo = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    // Start 3 processes in order: A (sleep), B (delayed crash), C (sleep)
    // B crashes after 300ms, giving C time to start and reach Running.
    // When B crashes, RestForOne should cascade-kill C (started after B)
    // but NOT A (started before B).
    let _id_a = manager.start("group", &sleep_config_rfo).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let id_b = manager.start("group", &crash_config).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let id_c = manager.start("group", &sleep_config_rfo).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Wait for B to crash (after ~300ms) and cascade-kill C
    let mut got_crashed = false;
    let mut got_stopped = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            if event.id == id_b && event.state == ProcessState::Crashed {
                got_crashed = true;
            }
            if event.id == id_c && event.state == ProcessState::Stopped {
                got_stopped = true;
            }
        }
        if got_crashed && got_stopped {
            break;
        }
    }

    assert!(got_crashed, "Expected Crashed event for B");
    assert!(got_stopped, "Expected Stopped event for C (cascade-killed by RestForOne)");

    // A should still be tracked (started before B, not cascade-killed)
    let info_a = manager.get_info(_id_a);
    assert!(info_a.is_some(), "A should still be tracked (started before crashed process B)");

    manager.stop_all();
}

#[test]
fn rest_for_one_first_process_crashes_restarts_all() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let crash_config = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    let sleep_config_rfo = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    // Start 3 processes: A (crash), B (sleep), C (sleep)
    // When A (first) crashes, RestForOne should cascade-kill B and C
    // (all started after A).
    let id_a = manager.start("group", &crash_config).unwrap();
    let _id_b = manager.start("group", &sleep_config_rfo).unwrap();
    let _id_c = manager.start("group", &sleep_config_rfo).unwrap();

    // Wait for A to crash and cascade-kill B and C
    let mut got_crashed = false;
    let mut stopped_count = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            if event.id == id_a && event.state == ProcessState::Crashed {
                got_crashed = true;
            }
            if event.state == ProcessState::Stopped {
                stopped_count += 1;
            }
        }
        if got_crashed && stopped_count >= 2 {
            break;
        }
    }

    assert!(got_crashed, "Expected Crashed event for A");
    assert!(stopped_count >= 2, "Expected at least 2 Stopped events from cascade (B and C)");

    manager.stop_all();
}

#[test]
fn no_recursive_cascade_on_rest_for_one() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    let crash_config = ProcessConfig::builder()
        .command("false".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    let sleep_config_rfo = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::RestForOne)
        .build();

    // Start 3 processes: A (crash), B (sleep), C (sleep)
    let _id_a = manager.start("group", &crash_config).unwrap();
    let _id_b = manager.start("group", &sleep_config_rfo).unwrap();
    let _id_c = manager.start("group", &sleep_config_rfo).unwrap();

    // Wait and count events - should not get recursive cascade
    let mut crash_count = 0;
    let mut stopped_count = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            if event.state == ProcessState::Crashed {
                crash_count += 1;
            }
            if event.state == ProcessState::Stopped {
                stopped_count += 1;
            }
        }
    }

    // Should have exactly 1 Crashed event (the original crash).
    // Cascade-killed processes emit Stopped, not Crashed.
    assert_eq!(crash_count, 1, "Expected exactly 1 Crashed event, got {crash_count}");
    // Should have 2 Stopped events (B and C cascade-killed), not more.
    assert_eq!(stopped_count, 2, "Expected exactly 2 Stopped events, got {stopped_count}");

    manager.stop_all();
}

// ============================================================
// Reaper dependency timeout for Waiting processes
// ============================================================

#[test]
fn reaper_times_out_waiting_process_without_start_with_deps() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start a process with an unsatisfiable label dependency and a short timeout.
    // Using start() (not start_with_deps()) - the reaper should enforce the timeout.
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("nonexistent")])
        .dependency_timeout_ms(500)
        .build();

    let id = manager.start("waiting", &config).unwrap();
    assert_eq!(manager.get_info(id).unwrap().state, ProcessState::Waiting);

    // The reaper should timeout the Waiting process after ~500ms
    let mut got_failed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100))
            && event.id == id
            && event.state == ProcessState::Failed
        {
            got_failed = true;
            break;
        }
    }
    assert!(got_failed, "Expected Waiting process to be timed out by reaper and emit Failed event");

    // Process should be removed after timeout
    assert!(manager.get_info(id).is_none(), "Timed-out Waiting process should be removed");

    manager.stop_all();
}

// ============================================================
// Dependency-ordered restart in OneForAll
// ============================================================

#[test]
fn one_for_all_with_dependencies_restarts_in_order() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // A (compositor) - crashes, restarts with backoff
    let config_a = ProcessConfig::builder()
        .command("sleep 0.3 && false")
        .shell(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(fast_one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .build();

    // Start A first
    let id_a = manager.start("group", &config_a).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // B (panel) - depends on A by Id, same label group, OneForAll
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(fast_one_restart_backoff()))
        .supervisor_strategy(SupervisorStrategy::OneForAll)
        .depends_on(vec![DependencyRef::Id(id_a)])
        .dependency_timeout_ms(30000)
        .build();

    let id_b = manager.start("group", &config_b).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Both should be Running/Starting
    assert_eq!(manager.get_info(id_a).unwrap().state, ProcessState::Running);
    assert_eq!(manager.get_info(id_b).unwrap().state, ProcessState::Running);

    // Wait for A to crash (after ~300ms) → OneForAll cascade-kills B
    // → A restarts first → B enters Waiting → B spawns after A is Running
    let mut a_crashed = false;
    let mut b_stopped = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50)) {
            if event.id == id_a && event.state == ProcessState::Crashed {
                a_crashed = true;
            }
            if event.id == id_b && event.state == ProcessState::Stopped {
                b_stopped = true;
            }
        }
        if a_crashed && b_stopped {
            break;
        }
    }
    assert!(a_crashed, "Expected A to crash");
    assert!(b_stopped, "Expected B to be cascade-stopped");

    // After A restarts, B should transition from Waiting to Running.
    // Wait for B to reach Running.
    let mut b_running = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Some(info) = manager.get_info(id_b)
            && info.state == ProcessState::Running
        {
            b_running = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(b_running, "Expected B to reach Running after A restarted");

    // A should also be Running
    let info_a = manager.get_info(id_a);
    assert!(info_a.is_some(), "A should still be tracked");
    assert_eq!(info_a.unwrap().state, ProcessState::Running, "A should be Running after restart");

    manager.stop_all();
}

// ============================================================
// Label-based dependency resolution
// ============================================================

#[test]
fn label_based_dependency_resolution_transitions_to_running() {
    let (tx, _rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start B first with unsatisfied label dependency → Waiting
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("compositor")])
        .dependency_timeout_ms(30000)
        .build();

    let id_b = manager.start("panel", &config_b).unwrap();
    assert_eq!(manager.get_info(id_b).unwrap().state, ProcessState::Waiting);

    // Start A with label "compositor"
    let id_a = manager.start("compositor", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // B should transition from Waiting to Running after A starts
    assert_eq!(manager.get_info(id_a).unwrap().state, ProcessState::Running, "A should be Running");
    assert_eq!(
        manager.get_info(id_b).unwrap().state,
        ProcessState::Running,
        "B should transition to Running after A (compositor) starts"
    );

    manager.stop_all();
}

// ============================================================
// Label binding persists across dependency restart
// ============================================================

#[test]
fn label_binding_persists_across_dependency_restart() {
    let (tx, _rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start A (label "compositor") - crashes and restarts
    let config_a = ProcessConfig::builder()
        .command("sleep 0.5 && false")
        .shell(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .restart_on_exit(true)
        .restart_trigger(RestartTrigger::Always)
        .restart_policy(RestartPolicy::Backoff(fast_one_restart_backoff()))
        .build();

    let id_a = manager.start("compositor", &config_a).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Start B (depends on Label("compositor")) - should resolve to id_a
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("compositor")])
        .dependency_timeout_ms(30000)
        .build();

    let id_b = manager.start("panel", &config_b).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Both should be Running
    assert_eq!(manager.get_info(id_a).unwrap().state, ProcessState::Running);
    assert_eq!(manager.get_info(id_b).unwrap().state, ProcessState::Running);

    // Wait for A to crash (~500ms) and restart (~100ms backoff = ~600ms)
    // Check at 700ms - A should be Running after restart.
    // A's second crash would be at ~1100ms (500ms after restart), so we're safe.
    std::thread::sleep(Duration::from_millis(700));

    // A should have restarted (same ProcessId)
    let info_a = manager.get_info(id_a);
    assert!(info_a.is_some(), "A should still be tracked after restart");
    assert_eq!(info_a.unwrap().state, ProcessState::Running, "A should be Running after restart");

    // B should still be Running - it's bound to A's ProcessId which persists
    let info_b = manager.get_info(id_b);
    assert!(info_b.is_some(), "B should still be tracked");
    assert_eq!(
        info_b.unwrap().state,
        ProcessState::Running,
        "B should remain Running across A's restart (label binding persists)"
    );

    manager.stop_all();
}

// ============================================================
// Label binding does not switch to new process
// ============================================================

#[test]
fn label_binding_does_not_switch_to_new_process() {
    let (tx, rx) = mpsc::channel::<ProcessExitEvent>();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).unwrap();

    // Start A (label "session") - no restart, will exit cleanly
    let config_a = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["0.2".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id_a = manager.start("session", &config_a).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Start B (depends on Label("session")) - resolves to id_a
    let config_b = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .depends_on(vec![DependencyRef::label("session")])
        .dependency_timeout_ms(30000)
        .build();

    let id_b = manager.start("worker", &config_b).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // B should be Running (A is Running)
    assert_eq!(manager.get_info(id_b).unwrap().state, ProcessState::Running);

    // Wait for A to exit and be removed (no restart)
    std::thread::sleep(Duration::from_millis(300));
    assert!(manager.get_info(id_a).is_none(), "A should have exited and been removed");

    // Start C with label "session" - B should NOT switch to C
    let id_c = manager.start("session", &sleep_config()).unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // C should be Running
    assert_eq!(manager.get_info(id_c).unwrap().state, ProcessState::Running);

    // B should fail-fast because its bound dependency (id_a) is gone
    let mut got_failed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
            && event.id == id_b
            && event.state == ProcessState::Failed
        {
            got_failed = true;
            break;
        }
    }
    assert!(got_failed, "B should fail-fast (bound to id_a which is gone), not switch to new process C");

    manager.stop_all();
}
