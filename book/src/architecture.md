# Architecture

`smearor-wrot-process-manager` achieves its socket and process management through two crates working together with `DashMap`-based concurrent tracking and an optional reaper thread.

## Overview

The workspace is composed of two crates:

- **`smearor-wrot-socket`** — Wayland socket path management with `Socket`, `SocketBuilder`, and `SocketManager`
- **`smearor-wrot-process`** — Child process lifecycle management with `ProcessConfig`, `ProcessManager`, and reaper thread

## Crate Relationships

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef socket fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef process fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef consumer fill: #89fc00, stroke: #333333, stroke-width: 2px, color: #000000

    Socket["smearor-wrot-socket<br/><small>Socket, SocketBuilder, SocketManager</small>"]
    Process["smearor-wrot-process<br/><small>ProcessConfig, ProcessManager, Reaper</small>"]
    Wrot["smearor-wrot<br/><small>Compositor + clients</small>"]
    Launcher["smearor-swipe-launcher<br/><small>terminal_command + app-launcher</small>"]

    Process -->|depends on| Socket
    Wrot -->|uses| Socket
    Wrot -->|uses| Process
    Launcher -->|uses| Process

    class Socket socket
    class Process process
    class Wrot consumer
    class Launcher consumer
```

## Core Components

### 1. Socket Management (`smearor-wrot-socket`)

The socket crate provides three main types:

- **`Socket`** — A `PathBuf` newtype representing a Wayland socket path. Implements `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, and `AsRef<str>`.
- **`SocketBuilder`** — Constructs socket paths in `XDG_RUNTIME_DIR`. If a name is provided, it validates uniqueness. If no name is provided, it auto-generates a unique name like `wayland-{N}`.
- **`SocketManager`** — A concurrent multi-socket manager using `DashMap`. Sockets are registered by name and can be retrieved, removed, or listed. Shareable via `Arc` across threads.

### 2. Process Management (`smearor-wrot-process`)

The process crate provides the `ProcessManager` as its central component:

- **`ProcessConfig`** — Built via `TypedBuilder`. Contains all configuration for spawning a child: command, args, env, working_dir, shell mode, forked mode, terminate_on_exit, kill_signal, terminate_timeout, restart_on_exit, stdio config, and optional Wayland socket.
- **`ProcessManager`** — Tracks child processes in a `DashMap` keyed by `ProcessId`. Supports label-based grouping, concurrent start/stop operations, and an optional reaper thread.
- **`Process`** — A handle to a managed child process. Contains the `ProcessId`, PID, label, config, and the `std::process::Child` handle.
- **`ProcessExitEvent`** — Emitted by the reaper thread when a process exits. Contains `id`, `pid`, `label`, and `restart_on_exit` flag.

## Process Lifecycle

```mermaid
graph LR
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef config fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef manager fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef state fill: #89fc00, stroke: #333333, stroke-width: 1px, color: #000
    classDef terminal fill: #dc0073, stroke: #333333, stroke-width: 1px, color: #ffffff

    A["ProcessConfig::builder()"] --> B["manager.start(label, &config)"]
    B --> C["Process spawned<br/>tracked in DashMap"]
    C --> D{Process exits?}
    D -->|No| E["Running<br/>is_running() == true"]
    D -->|Yes, reaper active| F["ProcessExitEvent<br/>sent via mpsc channel"]
    D -->|Yes, no reaper| G["Zombie until<br/>stop() or drop()"]
    E --> H["manager.stop(id)"]
    H --> I["SIGTERM sent"]
    I --> J{Process exited<br/>within timeout?}
    J -->|Yes| K["Removed from DashMap"]
    J -->|No| L["SIGKILL escalation"]
    L --> K
    F --> K
    G --> K

    class A config
    class B manager
    class C state
    class E state
    class F state
    class G state
    class H manager
    class I terminal
    class L terminal
    class K state
```

## Reaper Thread Architecture

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef thread fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef map fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef event fill: #89fc00, stroke: #333333, stroke-width: 1px, color: #000
    classDef consumer fill: #04e762, stroke: #333333, stroke-width: 1px, color: #000

    A["Reaper Thread<br/><small>process-reaper</small>"] -->|poll every N ms| B["Iterate DashMap"]
    B --> C{"try_wait() on<br/>each process"}
    C -->|Still running| D["Skip"]
    C -->|Exited| E["Remove from DashMap"]
    E --> F["Send ProcessExitEvent<br/>via mpsc::Sender"]
    F --> G["Consumer receives<br/>via mpsc::Receiver"]
    G --> H{"Consumer type"}
    H -->|Sync| I["receiver.recv()<br/>or try_recv()"]
    H -->|GTK/Async| J["Forwarding thread<br/>std → tokio::mpsc"]
    J --> K["MainContext::spawn_local<br/>async event handling"]

    class A thread
    class B map
    class C map
    class D map
    class E map
    class F event
    class G consumer
    class H consumer
    class I consumer
    class J consumer
    class K consumer
```

## Termination Flow

```mermaid
sequenceDiagram
    participant Consumer
    participant Manager as ProcessManager
    participant Process as Child Process
    participant Reaper as Reaper Thread

    Consumer->>Manager: stop(id)
    Manager->>Process: send SIGTERM (or SIGKILL)
    
    alt KillSignal::Sigterm
        Manager->>Manager: wait terminate_timeout_ms
        alt Process exits within timeout
            Process-->>Manager: try_wait() returns exit status
            Manager->>Manager: remove from DashMap
        else Process still running
            Manager->>Process: send SIGKILL
            Process-->>Manager: process killed
            Manager->>Manager: remove from DashMap
        end
    else KillSignal::Sigkill
        Process-->>Manager: process killed immediately
        Manager->>Manager: remove from DashMap
    end

    opt Reaper thread active
        Reaper->>Reaper: try_wait() detects exit
        Reaper-->>Consumer: ProcessExitEvent
    end
```

## Drop Behavior

When `ProcessManager` is dropped, the following sequence occurs:

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef action fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef terminal fill: #dc0073, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef done fill: #89fc00, stroke: #333333, stroke-width: 1px, color: #000

    A["ProcessManager::drop()"] --> B{"Reaper thread<br/>active?"}
    B -->|Yes| C["Set stop_flag<br/>join thread"]
    B -->|No| D["Skip"]
    C --> E["Iterate all processes"]
    D --> E
    E --> F{"terminate_on_exit<br/>== true?"}
    F -->|Yes| G["Send SIGTERM<br/>wait timeout<br/>escalate to SIGKILL"]
    F -->|No| H["Leave running<br/>(detach)"]
    G --> I["Drop complete"]
    H --> I

    class A action
    class C action
    class E action
    class G terminal
    class I done
```

## Design Decisions

### Why split socket and process into separate crates?

Socket management is a lightweight concern (path manipulation, `DashMap` for multi-socket support). Process management is heavier (child spawning, signal handling, reaper threads). Separating them allows consumers to depend on only what they need — `smearor-swipe-launcher` uses only `smearor-wrot-process` without needing the socket crate directly.

### Why `DashMap` instead of `Mutex<HashMap>`?

Both `SocketManager` and `ProcessManager` are accessed concurrently from multiple threads. `DashMap` provides shard-level locking, avoiding the contention of a single `Mutex`. The reaper thread iterates processes while the main thread may start/stop others — `DashMap` handles this without blocking.

### Why `std::sync::mpsc` for reaper events?

The reaper thread is a plain `std::thread`, not async. `std::sync::mpsc::Sender` is `Send + Sync` and integrates cleanly with both sync and async consumers. In GTK applications, a forwarding thread bridges the blocking `recv()` to a non-blocking `tokio::sync::mpsc` channel for use in `MainContext::spawn_local`. This avoids blocking the GTK main loop while keeping the reaper thread simple.

### Why `typed-builder` for `ProcessConfig`?

`ProcessConfig` has many fields with sensible defaults. `typed-builder` enforces required fields (only `command`) at compile time while keeping optional fields ergonomic with `.field_name(value)` syntax. This prevents missing-field bugs without runtime validation.

### Why not `tokio::process::Child`?

The `ProcessManager` is deliberately synchronous. It works with GTK's `MainContext` and Smithay's event loop without requiring an async runtime. The reaper thread uses non-blocking `try_wait()` polling instead — simpler, no runtime dependency, and sufficient for the use case (exit detection with configurable latency).

### Why `setsid()` instead of double-fork?

`setsid()` detaches the process from the controlling terminal. A double-fork (grandchild) would lose the `Child` handle, preventing tracking and `try_wait()` reaping. With `setsid()`, the process is still a direct child — the `Child` handle is stored, `try_wait()` works, and `stop()` can send signals. This is the correct trade-off for a process manager that needs to track and terminate its children.
