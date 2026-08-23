# process-manager

[![Rust Edition](https://img.shields.io/badge/rust-2024-f5b700.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-89fc00.svg)](LICENSE.md)

Child process lifecycle management with Wayland socket binding.

## Overview

`process-manager` provides a `ProcessManager` for spawning, tracking, and terminating child processes. It supports label-based grouping, forked/detached processes, and an optional reaper thread for zombie prevention and exit notifications.

Used by both `smearor-wrot` (to spawn compositor clients) and `smearor-swipe-launcher` (to launch terminal commands and desktop applications).

## Features

- **`ProcessConfig` with `TypedBuilder`** - Compile-time enforcement of required fields, ergonomic optional fields
- **`ProcessManager`** - Concurrent process tracking via `DashMap`, label-based grouping, optional reaper thread
- **`Label` newtype** - Type-safe process labels for grouped operations and dependency references. Replaces bare `String` labels with a dedicated `Label` type, consistent with `ProcessId`
- **Label-based operations** - Start/stop multiple processes under a shared label
- **Forked/detached processes** - `setsid()` via `pre_exec` for terminal detachment
- **Reaper thread** - Non-blocking `try_wait()` polling with `ProcessExitEvent` channel
- **Signal escalation** - `SIGTERM` with configurable timeout, automatic `SIGKILL` escalation
- **Wayland socket binding** - Sets `WAYLAND_DISPLAY` from `Socket`
- **`StdioConfig`** - Inherit/Null/Piped with reader threads for output capture
- **`KillSignal`** - `Sigterm`/`Sigkill` with serde support
- **Restart policies** - `RestartPolicy::Immediate` or `RestartPolicy::Backoff` with exponential backoff, rate limiting, and `RestartTrigger` (crash-only or always)
- **Supervisor strategies** - `OneForOne` (restart only crashed), `OneForAll` (restart all in group), `RestForOne` (restart crashed and all started after it). Follows the Erlang OTP supervisor model
- **Dependencies** - Declare `depends_on` with label or `ProcessId` references. Processes wait for dependencies to be `Running` before starting, with configurable timeout
- **Graceful shutdown** - `terminate_on_exit` flag kills processes on drop

## Quick Start

### Spawn a child process

```rust
use process_manager::{Label, ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("echo".to_string())
    .args(vec!["hello world".to_string()])
    .stdout(StdioConfig::Piped)
    .stderr(StdioConfig::Piped)
    .build();

let id = manager.start("echo-app", &config)?;
manager.stop(id)?;
```

### Spawn a forked/detached process

```rust
let config = ProcessConfig::builder()
    .command("my-daemon".to_string())
    .forked(true)
    .terminate_on_exit(true)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let id = manager.start("daemon", &config)?;
```

### Monitor process exits with the reaper

```rust
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender)?;

manager.start("task", &config)?;

let event = receiver.recv_timeout(Duration::from_secs(10))?;
println!("Process {} (PID {}) exited", event.label, event.pid);
```

### Label-based grouping

```rust
manager.start("worker-pool", &config)?;
manager.start("worker-pool", &config)?;
manager.start("worker-pool", &config)?;
manager.stop_label("worker-pool")?; // Stops all three
```

### Wayland socket binding

```rust
use process_manager_socket::SocketBuilder;

let socket = SocketBuilder::build(&None)?;
let config = ProcessConfig::builder()
    .command("gtk4-app".to_string())
    .socket(Some(socket))
    .build();
let id = manager.start("wayland-client", &config)?;
```

### Supervisor strategies with dependencies

```rust
use process_manager::{
    BackoffConfig, DependencyRef, ProcessConfig, ProcessManager,
    RestartPolicy, RestartTrigger, StdioConfig, SupervisorStrategy,
};
use std::time::Duration;

let (tx, rx) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_millis(100), tx)?;

// Compositor - started first, no dependencies
let compositor_config = ProcessConfig::builder()
    .command("hyprland".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Backoff(BackoffConfig::default()))
    .supervisor_strategy(SupervisorStrategy::OneForAll)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let compositor_id = manager.start("session", &compositor_config)?;

// Panel - depends on compositor, restarts with it (OneForAll)
let panel_config = ProcessConfig::builder()
    .command("smearor-swipe-launcher".to_string())
    .restart_on_exit(true)
    .restart_trigger(RestartTrigger::CrashOnly)
    .restart_policy(RestartPolicy::Backoff(BackoffConfig::default()))
    .supervisor_strategy(SupervisorStrategy::OneForAll)
    .depends_on(vec![DependencyRef::label("session")])
    .dependency_timeout_ms(10_000)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let panel_id = manager.start("session", &panel_config)?;
// Panel starts in Waiting state, then transitions to Running
// once the compositor is Running.

// If the compositor crashes, both compositor and panel are restarted.
```

## API

### `ProcessConfig`

Configuration built via `TypedBuilder`. Required field: `command`. All other fields have defaults: `args`, `env`, `working_dir`, `shell`, `forked`, `terminate_on_exit`, `kill_signal`, `terminate_timeout_ms`, `restart_on_exit`, `restart_trigger`, `restart_policy`, `stdin`, `stdout`, `stderr`, `socket`.

### `ProcessManager`

The main process manager. Tracks child processes in a `DashMap`, supports label-based grouping, and optionally runs a reaper thread. Construction via `new()` (no reaper) or `with_reaper(poll_interval, sender)` (with exit notifications).

### `ProcessExitEvent`

Emitted by the reaper thread when a process exits. Contains `id`, `pid`, `label`, `restart_on_exit` flag, `exit_status`, and `state` (`Stopped`, `Crashed`, or `Failed`).

### `RestartPolicy` / `BackoffConfig` / `RestartTrigger`

Controls automatic restart behavior when `restart_on_exit` is `true`:
- `RestartTrigger::CrashOnly` - restart only on crashes (non-zero exit), not on clean exits
- `RestartTrigger::Always` - restart on any exit (including clean)
- `RestartPolicy::Immediate` - restart immediately, no delay or rate limiting
- `RestartPolicy::Backoff(BackoffConfig)` - exponential backoff with `initial_delay`, `multiplier`, `max_delay`, `max_restarts`, and `min_uptime` for counter reset

### `SupervisorStrategy`

Controls which processes are restarted when one process in a group crashes. `OneForOne` (default) restarts only the crashed process. `OneForAll` restarts all processes in the same label group. `RestForOne` restarts the crashed process and all processes started after it in the same group. Only active when `restart_on_exit = true`.

### `Label`

A newtype wrapping `String` for type-safe process labeling. Used in `start()`, `stop_label()`, `restart_label()`, `get_by_label()`, `pids_by_label()`, and `DependencyRef::Label`. All manager methods accept `impl Into<Label>`, so callers can pass `&str` directly (`manager.start("compositor", &config)`) or construct a `Label` explicitly (`Label::new("compositor")`). Implements `Display`, `AsRef<str>`, `From<&str>`, `From<String>`, and serde traits.

### `DependencyRef`

References a dependency by label (`DependencyRef::label("compositor")` or `DependencyRef::Label(Label::new("compositor"))`) or by `ProcessId` (`DependencyRef::id(id)` or `DependencyRef::Id(id)`). Used in `ProcessConfig::depends_on` to declare start-order dependencies. Label bindings are resolved once and persist for the dependent's lifetime.

### `ProcessState::Waiting`

New state for processes that are queued but waiting for dependencies to become `Running`. `is_alive()` returns `true` for `Waiting`. The process transitions to `Starting` once all dependencies are `Running`, or to `Failed` if the dependency timeout is exceeded.

### `StdioConfig`

Controls standard streams: `Inherit` (parent's streams), `Null` (`/dev/null`), `Piped` (reader threads forward to `tracing`).

### `KillSignal`

`Sigterm` (graceful, default) or `Sigkill` (immediate). Serialized as `"SIGTERM"` / `"SIGKILL"`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dashmap` | Concurrent process tracking |
| `nix` | Signal handling (`SIGTERM`, `SIGKILL`) |
| `libc` | `setsid()` for forked processes |
| `typed-builder` | `ProcessConfig` builder pattern |
| `which` | Executable path resolution |
| `process-manager-socket` | Wayland socket binding |
| `thiserror` | Error types |
| `tracing` | Logging |

## License

MIT
