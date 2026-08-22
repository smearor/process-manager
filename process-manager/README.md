# process-manager

[![Rust Edition](https://img.shields.io/badge/rust-2024-f5b700.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-89fc00.svg)](LICENSE.md)

Child process lifecycle management with Wayland socket binding.

## Overview

`process-manager` provides a `ProcessManager` for spawning, tracking, and terminating child processes. It supports label-based grouping, forked/detached processes, and an optional reaper thread for zombie prevention and exit notifications.

Used by both `smearor-wrot` (to spawn compositor clients) and `smearor-swipe-launcher` (to launch terminal commands and desktop applications).

## Features

- **`ProcessConfig` with `TypedBuilder`** — Compile-time enforcement of required fields, ergonomic optional fields
- **`ProcessManager`** — Concurrent process tracking via `DashMap`, label-based grouping, optional reaper thread
- **Label-based operations** — Start/stop multiple processes under a shared label
- **Forked/detached processes** — `setsid()` via `pre_exec` for terminal detachment
- **Reaper thread** — Non-blocking `try_wait()` polling with `ProcessExitEvent` channel
- **Signal escalation** — `SIGTERM` with configurable timeout, automatic `SIGKILL` escalation
- **Wayland socket binding** — Sets `WAYLAND_DISPLAY` from `Socket`
- **`StdioConfig`** — Inherit/Null/Piped with reader threads for output capture
- **`KillSignal`** — `Sigterm`/`Sigkill` with serde support
- **Graceful shutdown** — `terminate_on_exit` flag kills processes on drop

## Quick Start

### Spawn a child process

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

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
let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender);

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

## API

### `ProcessConfig`

Configuration built via `TypedBuilder`. Required field: `command`. All other fields have defaults: `args`, `env`, `working_dir`, `shell`, `forked`, `terminate_on_exit`, `kill_signal`, `terminate_timeout_ms`, `restart_on_exit`, `stdin`, `stdout`, `stderr`, `socket`.

### `ProcessManager`

The main process manager. Tracks child processes in a `DashMap`, supports label-based grouping, and optionally runs a reaper thread. Construction via `new()` (no reaper) or `with_reaper(poll_interval, sender)` (with exit notifications).

### `ProcessExitEvent`

Emitted by the reaper thread when a process exits. Contains `id`, `pid`, `label`, and `restart_on_exit` flag.

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
