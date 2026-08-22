# ProcessManager

`ProcessManager` manages child processes concurrently using `DashMap`.

## Overview

`ProcessManager` is the central component of the `process-manager` crate. It provides:

- **Concurrent tracking** — All processes are stored in a `DashMap` keyed by `ProcessId`, allowing concurrent access from multiple threads
- **Label-based grouping** — Multiple processes can share a label for grouped start/stop operations
- **Optional reaper thread** — A background thread that polls `try_wait()` to detect exits and emit `ProcessExitEvent`s
- **Signal-based termination** — `SIGTERM` with configurable timeout, automatic `SIGKILL` escalation
- **Graceful shutdown** — Processes with `terminate_on_exit = true` are killed when the manager is dropped

## Construction

### `new()` — Without reaper

```rust
use process_manager::ProcessManager;

let manager = ProcessManager::new();
```

Use this when you don't need exit notifications. Processes are still tracked and can be stopped manually. Without the reaper, exited processes remain as zombies until `stop()` or `drop()` is called.

### `with_reaper(poll_interval, sender)` — With reaper

```rust
use process_manager::ProcessManager;
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender)?;
```

The reaper thread polls all tracked processes every `poll_interval` and emits `ProcessExitEvent`s via the `sender`. This prevents zombies and enables exit notifications and restart logic.

## Methods

### Spawning

| Method | Returns | Description |
|--------|---------|-------------|
| `start(label, &config)` | `Result<ProcessId, ProcessManagerError>` | Spawn a child process with the given label and config |

`start()` performs the following:
1. Resolves the executable via `which` if not an absolute path
2. Constructs `std::process::Command` with env, working_dir, shell mode
3. Applies `setsid()` via `pre_exec` if `config.forked` is `true`
4. Sets `WAYLAND_DISPLAY` if `config.socket` is `Some`
5. Configures stdio (spawns reader threads for `Piped`)
6. Spawns the child and stores it in the `DashMap`

### Stopping

| Method | Returns | Description |
|--------|---------|-------------|
| `stop(id)` | `Result<(), ProcessManagerError>` | Stop a single process by `ProcessId` |
| `stop_label(label)` | `Result<(), ProcessManagerError>` | Stop all processes under a label |
| `stop_all()` | `()` | Stop all tracked processes |

`stop()` sends the configured `kill_signal`, waits up to `terminate_timeout_ms`, and escalates to `SIGKILL` if the process is still running. If the process has already exited (ESRCH), it joins readers and returns `Ok(())`.

### Stop Flow

```mermaid
sequenceDiagram
    participant Consumer
    participant Manager as ProcessManager
    participant Process
    participant OS

    Consumer->>Manager: stop(id)
    Manager->>Process: send_signal(kill_signal)
    alt ESRCH (already exited)
        Process-->>Manager: Ok (process gone)
        Manager->>Process: join_readers(500ms)
        Manager-->>Consumer: Ok
    else Signal sent
        Process->>OS: kill(pid, SIGTERM)
        Manager->>Process: wait(terminate_timeout_ms)
        alt Process exited in time
            Process-->>Manager: exited
            Manager->>Process: join_readers
            Manager-->>Consumer: Ok
        else Process still running
            Manager->>Process: force_kill (SIGKILL)
            Process->>OS: kill(pid, SIGKILL)
            Process-->>Manager: killed
            Manager-->>Consumer: Ok
        end
    end
```

### Restarting

| Method | Returns | Description |
|--------|---------|-------------|
| `restart(id)` | `Result<ProcessId, ProcessManagerError>` | Restart a single process, preserving config and label |
| `restart_label(label)` | `Result<(), StopManyError>` | Restart all processes under a label |

`restart()` stops the process (with escalation), then starts a new one with the same config and label. Returns the new `ProcessId`.

### Restart Flow

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef input fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef stop fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef start fill: #89fc00, stroke: #333333, stroke-width: 2px, color: #000
    classDef done fill: #04e762, stroke: #333333, stroke-width: 1px, color: #000

    A["restart(id)"] --> B["stop(id)<br/><small>SIGTERM → wait → SIGKILL</small>"]
    B --> C["Remove old Process<br/>from DashMap"]
    C --> D["start(label, &config)<br/><small>same config + label</small>"]
    D --> E["New Process<br/>in DashMap"]
    E --> F["Return new ProcessId"]

    class A input
    class B stop
    class C stop
    class D start
    class E start
    class F done
```

### Signaling

| Method | Returns | Description |
|--------|---------|-------------|
| `send_signal(id, signal)` | `Result<(), ProcessManagerError>` | Send a signal to a process by `ProcessId` (process stays in manager) |
| `send_signal_label(label, signal)` | `Result<(), StopManyError>` | Send a signal to all processes under a label |

`send_signal()` sends any [`Signal`](signal.md) (SIGHUP, SIGUSR1, SIGWINCH, etc.) without removing the process from the manager. This is useful for triggering config reloads, pausing/resuming, or other non-terminating signals.

### Querying

| Method | Returns | Description |
|--------|---------|-------------|
| `get_info(id)` | `Option<ProcessInfo>` | Get a process snapshot by `ProcessId` (no deadlock risk) |
| `get_by_label(label)` | `Vec<(ProcessId, ProcessInfo)>` | Get all process snapshots with a label |
| `pids_by_label(label)` | `Vec<u32>` | Get PIDs for a label |
| `labels()` | `Vec<String>` | List all distinct labels |
| `ids()` | `Vec<ProcessId>` | List all `ProcessId`s |
| `is_empty()` | `bool` | Check if manager has no processes |
| `len()` | `usize` | Number of tracked processes |

### Convenience Getters

| Method | Returns | Description |
|--------|---------|-------------|
| `is_running(id)` | `Option<bool>` | Check if a process is still running (delegates to `state().is_alive()`) |
| `state(id)` | `Option<ProcessState>` | Get the explicit lifecycle state of a process |
| `is_forked(id)` | `Option<bool>` | Check if a process was spawned with `setsid()` |
| `get_pid(id)` | `Option<u32>` | Get the OS PID for a process |
| `get_label(id)` | `Option<String>` | Get the label for a process |
| `get_program_name(id)` | `Option<String>` | Get the command/program name |
| `get_terminate_on_exit(id)` | `Option<bool>` | Check if process has `terminate_on_exit` set |
| `get_config(id)` | `Option<Arc<ProcessConfig>>` | Get the process config (shared via `Arc`) |

`get()` is `pub(crate)` to avoid deadlock risks from holding `DashMap` guards. Use `get_info()` for a lightweight `ProcessInfo` snapshot or the convenience getters for specific fields.

## Start Flow

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef input fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef resolve fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef spawn fill: #89fc00, stroke: #333333, stroke-width: 2px, color: #000
    classDef error fill: #dc0073, stroke: #333333, stroke-width: 1px, color: #ffffff

    A["start(label, &config)"] --> B{"command is<br/>absolute path?"}
    B -->|No| C["which::which(command)"]
    B -->|Yes| D["Use path directly"]
    C -->|Found| D
    C -->|Not found| E["ExecutableNotFound"]
    D --> F["Build Command<br/>+ env + working_dir"]
    F --> G{"forked?"}
    G -->|Yes| H["Add pre_exec hook<br/>with setsid()"]
    G -->|No| I["Skip"]
    H --> J{"socket set?"}
    I --> J
    J -->|Yes| K["Set WAYLAND_DISPLAY<br/>from socket name"]
    J -->|No| L["Skip"]
    K --> M["Configure stdio<br/>(spawn reader threads<br/>for Piped)"]
    L --> M
    M --> N["Command::spawn()"]
    N -->|Success| O["Store Process<br/>in DashMap"]
    N -->|Failure| P["SpawnFailed"]
    O --> Q["Return ProcessId"]

    class A input
    class B resolve
    class C resolve
    class D resolve
    class F spawn
    class G spawn
    class H spawn
    class J spawn
    class K spawn
    class M spawn
    class N spawn
    class O spawn
    class Q spawn
    class E error
    class P error
```

## Drop Behavior

When `ProcessManager` is dropped:

1. If the reaper thread is active, it is stopped (via `stop_flag` atomic) and joined
2. All processes with `terminate_on_exit = true` are terminated (SIGTERM → wait → SIGKILL)
3. Processes with `terminate_on_exit = false` are left running (detached)
4. The `DashMap` is dropped, freeing all resources

## Usage

### Without reaper

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("sleep".to_string())
    .args(vec!["10".to_string()])
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id = manager.start("task", &config)?;
// ... do work ...
manager.stop(id)?;
```

### With reaper and restart

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender)?;

let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .stdout(StdioConfig::Null)
    .build();

manager.start("service", &config)?;

// In your event loop:
if let Ok(event) = receiver.try_recv() {
    if event.restart_on_exit || event.state == ProcessState::Crashed {
        manager.start(&event.label, &config)?;
    }
}
```

### Label-based operations

```rust
let config = ProcessConfig::builder()
    .command("worker".to_string())
    .stdout(StdioConfig::Null)
    .build();

// Start a pool of workers
for _ in 0..4 {
    manager.start("worker-pool", &config)?;
}

// Check all PIDs
let pids = manager.pids_by_label("worker-pool");
assert_eq!(pids.len(), 4);

// Stop the entire pool
manager.stop_label("worker-pool")?;
assert!(manager.is_empty());
```
