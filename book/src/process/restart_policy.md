# Restart Policy

`RestartTrigger` and `RestartPolicy` control automatic restart behavior when `restart_on_exit` is enabled.

## Overview

When `restart_on_exit(true)` is set on a `ProcessConfig` and the `ProcessManager` is constructed with `with_reaper()`, the reaper thread automatically restarts processes that exit. The restart behavior is controlled by two config fields:

- `restart_trigger` - Determines **when** to restart
- `restart_policy` - Determines **how** to restart (immediate or with backoff)

## RestartTrigger

| Variant | Description |
|---------|-------------|
| `CrashOnly` | Restart only on crashes (non-zero exit code or signal). Clean exits (`exit(0)`) are not restarted. Default. |
| `Always` | Restart on any exit, including clean exits. Useful for processes that should always be running. |

## RestartPolicy

| Variant | Description |
|---------|-------------|
| `Immediate` | Restart immediately on exit. No delay, no rate limiting. Default. |
| `Backoff(BackoffConfig)` | Restart with exponential backoff and rate limiting. |

## BackoffConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `initial_delay` | `Duration` | `1s` | Initial delay before first restart |
| `multiplier` | `u32` | `20` | Multiplier applied to delay after each restart, in tenths (20 = 2.0x, 15 = 1.5x, 10 = 1.0x) |
| `max_delay` | `Duration` | `60s` | Maximum delay cap (prevents unbounded growth) |
| `max_restarts` | `u32` | `5` | Maximum consecutive restarts before giving up |
| `min_uptime` | `Duration` | `10s` | Uptime required to reset the restart counter |

### Backoff Delay Calculation

The delay for restart N (1-indexed) is:

```text
delay = min(initial_delay * (multiplier / 10)^(N-1), max_delay)
```

Example with `initial_delay=1s`, `multiplier=20` (2.0x), `max_delay=60s`:

| Restart # | Delay |
|-----------|-------|
| 1 | 1s |
| 2 | 2s |
| 3 | 4s |
| 4 | 8s |
| 5 | 16s |
| 6 | 32s |
| 7+ | 60s (capped) |

### Rate Limiting

After `max_restarts` consecutive restarts, the process transitions to `Failed` state and is removed from the manager. A `ProcessExitEvent` with `state=Failed` is emitted.

### Stable Uptime Reset

If a process runs for >= `min_uptime` without crashing, the restart counter resets to 0. This prevents a process that crashes once and then runs stably from being rate-limited.

## RestartState (Internal)

Each process with `restart_on_exit=true` has an internal `RestartState` that tracks:

- `restart_count` - Consecutive restarts since last stable uptime
- `last_started_at` - When the process was last spawned
- `next_eligible_restart` - Earliest time the process can be restarted (backoff timer)

This state is managed by the reaper thread and is not directly accessible by consumers.

## Interaction with Manual Operations

During the `Restarting` state (backoff wait), manual operations behave as follows:

| Operation | Behavior |
|-----------|----------|
| `stop(id)` | Cancels backoff, removes process silently (no event, no signal) |
| `restart(id)` | Cancels backoff, spawns immediately (preserves `ProcessId`) |
| `send_signal(id, ...)` | Returns `ProcessInRestartingState` error |

## Usage

### Immediate restart on crash

```rust
use process_manager::{ProcessConfig, ProcessManager, RestartPolicy, RestartTrigger, StdioConfig};
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender)?;

let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Immediate)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id = manager.start("service", &config)?;
```

### Backoff with rate limiting

```rust
use process_manager::{BackoffConfig, ProcessConfig, ProcessManager, RestartPolicy, RestartTrigger, StdioConfig};
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_millis(100), sender)?;

let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Backoff(BackoffConfig {
        initial_delay: Duration::from_secs(2),
        multiplier: 20,
        max_delay: Duration::from_secs(120),
        max_restarts: 10,
        min_uptime: Duration::from_secs(30),
    }))
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id = manager.start("service", &config)?;
```

### Always restart (even on clean exit)

```rust
let config = ProcessConfig::builder()
    .command("watcher".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::Always)
    .restart_policy(RestartPolicy::Immediate)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
```
