# Supervisor Strategies

This example demonstrates `OneForAll` and `RestForOne` supervisor strategies.

## OneForAll - restart all group members

When any process in the group crashes, all processes in the same label group are restarted.

```rust
use process_manager::{
    BackoffConfig, ProcessConfig, ProcessManager, RestartPolicy,
    RestartTrigger, StdioConfig, SupervisorStrategy,
};
use std::time::Duration;

let (tx, rx) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_millis(100), tx)?;

let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Backoff(BackoffConfig::default()))
    .supervisor_strategy(SupervisorStrategy::OneForAll)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id_a = manager.start("group-a", &config)?;
let id_b = manager.start("group-a", &config)?;

// If either process crashes, both are restarted.
```

## RestForOne - restart crashed and later processes

When a process crashes, it and all processes with a higher `spawn_sequence` in the same group are restarted.

```rust
let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Backoff(BackoffConfig::default()))
    .supervisor_strategy(SupervisorStrategy::RestForOne)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id_a = manager.start("group-b", &config)?;
let id_b = manager.start("group-b", &config)?;
let id_c = manager.start("group-b", &config)?;

// If B crashes, B and C are restarted. A is unaffected.
```
