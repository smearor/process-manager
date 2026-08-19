# smearor-wrot-process-manager

[![Rust Edition](https://img.shields.io/badge/rust-2024-f5b700.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-89fc00.svg)](LICENSE.md)
[![Book](https://img.shields.io/badge/book-main-00a1e4.svg)](https://github.com/smearor/smearor-wrot-process-manager/tree/main/book)

Shared socket and process management crates for `smearor-wrot` and `smearor-swipe-launcher`.

## Overview

`smearor-wrot-process-manager` is a Rust workspace providing two reusable crates for managing Wayland sockets and child process lifecycles. Both `smearor-wrot` (a Wayland compositor) and `smearor-swipe-launcher` (a desktop launcher) need to spawn and track child processes — compositor clients, terminal commands, desktop applications. This workspace consolidates that logic into two framework-agnostic crates with no dependency on GTK, Smithay, or any plugin API.

## Workspace Structure

- **[`smearor-wrot-socket`](smearor-wrot-socket/)** — Wayland socket path management (`Socket`, `SocketBuilder`, `SocketManager`)
- **[`smearor-wrot-process`](smearor-wrot-process/)** — Child process lifecycle management (`ProcessConfig`, `ProcessManager`, reaper thread)

## Features

### smearor-wrot-socket

- **`Socket` newtype** — `PathBuf` wrapper with `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, `AsRef<str>` implementations
- **`SocketBuilder`** — Builds socket paths in `XDG_RUNTIME_DIR`, validates existing names or generates unique ones
- **`SocketManager`** — Multi-socket manager using `DashMap`, shareable via `Arc` across threads
- **Error types** — `SocketBuilderError` and `SocketManagerError` via `thiserror`

### smearor-wrot-process

- **`ProcessConfig`** — Unified configuration via `TypedBuilder` (command, args, env, working_dir, shell, forked, terminate_on_exit, kill_signal, restart_on_exit, stdio, socket)
- **`ProcessManager`** — Concurrent process tracking via `DashMap`, label-based grouping, optional reaper thread
- **Label-based operations** — Start/stop multiple processes under a shared label
- **Forked/detached processes** — `setsid()` via `pre_exec` for terminal detachment
- **Reaper thread** — Non-blocking `try_wait()` polling with `ProcessExitEvent` channel for zombie prevention and exit notifications
- **Signal escalation** — `SIGTERM` with configurable timeout, automatic `SIGKILL` escalation
- **Wayland socket binding** — Sets `WAYLAND_DISPLAY` in child environment from `Socket`
- **`StdioConfig`** — Inherit/Null/Piped enum with reader threads for output capture
- **`KillSignal`** — `Sigterm`/`Sigkill` enum with serde support
- **Executable resolution** — `which` integration for PATH lookup
- **Graceful shutdown** — `terminate_on_exit` flag kills processes on `ProcessManager` drop

## Quick Start

### Spawn a child process

```rust
use smearor_wrot_process::{ProcessConfig, ProcessManager, StdioConfig};

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
use smearor_wrot_socket::{SocketBuilder, SocketManager};

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

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dashmap` | Concurrent process/socket tracking |
| `nix` | Signal handling (`SIGTERM`, `SIGKILL`) |
| `libc` | `setsid()` for forked processes |
| `typed-builder` | `ProcessConfig` builder pattern |
| `which` | Executable path resolution |
| `thiserror` | Error types |
| `tracing` | Logging |

## Consumers

- **`smearor-wrot`** — Uses `SocketManager` for multi-output Wayland sockets and `ProcessManager` for spawning compositor clients
- **`smearor-swipe-launcher`** — Uses `ProcessManager` in `terminal_command` and `app-launcher` services

## License

MIT