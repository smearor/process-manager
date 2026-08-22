# process-manager

[![Rust Edition](https://img.shields.io/badge/rust-2024-f5b700.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-89fc00.svg)](LICENSE.md)
[![Book](https://img.shields.io/badge/book-main-00a1e4.svg)](https://smearor.github.io/process-manager/book/)
[![Docs](https://img.shields.io/badge/docs-main-00a1e4.svg)](https://smearor.github.io/process-manager/docs/)

Reusable Rust crates for managing Wayland sockets and child process lifecycles.

## Overview

`process-manager` is a Rust workspace providing two framework-agnostic crates for managing Wayland sockets and child process lifecycles. Any application that needs to spawn, track, and gracefully terminate child processes - compositors, launchers, daemons, service managers - can use these crates. They have no dependency on GTK, Smithay, or any plugin API.

## Workspace Structure

- **[`process-manager-socket`](process-manager-socket/)** - Wayland socket path management (`Socket`, `SocketBuilder`, `SocketManager`)
- **[`process-manager`](process-manager/)** - Child process lifecycle management (`ProcessConfig`, `ProcessManager`, reaper thread)

## Features

### process-manager-socket

- **`Socket` newtype** - `PathBuf` wrapper with `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, `AsRef<str>` implementations
- **`SocketBuilder`** - Builds socket paths in `XDG_RUNTIME_DIR`, validates existing names or generates unique ones
- **`SocketManager`** - Multi-socket manager using `DashMap`, shareable via `Arc` across threads
- **Error types** - `SocketBuilderError` and `SocketManagerError` via `thiserror`

### process-manager

- **`ProcessConfig`** - Unified configuration via `TypedBuilder` (command, args, env, working_dir, shell, forked, terminate_on_exit, kill_signal, restart_on_exit, stdio, socket)
- **`ProcessManager`** - Concurrent process tracking via `DashMap`, label-based grouping, optional reaper thread
- **Label-based operations** - Start/stop multiple processes under a shared label
- **Forked/detached processes** - `setsid()` via `pre_exec` for terminal detachment
- **Reaper thread** - Non-blocking `try_wait()` polling with `ProcessExitEvent` channel for zombie prevention and exit notifications
- **Signal escalation** - `SIGTERM` with configurable timeout, automatic `SIGKILL` escalation
- **Wayland socket binding** - Sets `WAYLAND_DISPLAY` in child environment from `Socket`
- **`StdioConfig`** - Inherit/Null/Piped enum with reader threads for output capture
- **`KillSignal`** - `Sigterm`/`Sigkill` enum with serde support
- **`Signal`** - Broader signal enum (`SIGHUP`, `SIGUSR1`, `SIGWINCH`, `SIGSTOP`, etc.) for general process control
- **`send_signal` / `send_signal_label`** - Send arbitrary signals to processes without removing them from the manager
- **`restart` / `restart_label`** - Restart processes preserving config and label
- **`ProcessInfo`** - Lightweight process snapshot for inspection (no `Child` handle)
- **Serde support** - `ProcessConfig`, `KillSignal`, `Signal`, and `Socket` implement `Serialize`/`Deserialize`
- **Executable resolution** - `which` integration for PATH lookup
- **Graceful shutdown** - `terminate_on_exit` flag kills processes on `ProcessManager` drop

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
use process_manager_socket::{SocketBuilder, SocketManager};

let socket = SocketBuilder::build(&None)?;
let config = ProcessConfig::builder()
    .command("gtk4-app".to_string())
    .socket(Some(socket))
    .build();
let id = manager.start("wayland-client", &config)?;
```

## API

### `ProcessManager`

The main process manager. Tracks child processes in a `DashMap`, supports label-based grouping, and optionally runs a reaper thread.

### `ProcessConfig`

Configuration built via `TypedBuilder`. Required field: `command`. All other fields have defaults.

### `SocketManager`

Multi-socket manager using `DashMap`. Register, retrieve, and remove sockets by name.

### `ProcessExitEvent`

Emitted by the reaper thread when a process exits. Contains `id`, `pid`, `label`, and `restart_on_exit` flag.

### `Signal`

Broader signal enum for general process control. Supports `SIGHUP`, `SIGUSR1`, `SIGUSR2`, `SIGWINCH`, `SIGSTOP`, `SIGCONT`, `SIGALRM`, and more. Use `send_signal()` / `send_signal_label()` to deliver signals without stopping the process.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dashmap` | Concurrent process/socket tracking |
| `nix` | Signal handling (`SIGTERM`, `SIGKILL`) |
| `libc` | `setsid()` for forked processes |
| `serde` | Serialization for configs, signals, and sockets |
| `typed-builder` | `ProcessConfig` builder pattern |
| `which` | Executable path resolution |
| `thiserror` | Error types |
| `tracing` | Logging |

## License

MIT