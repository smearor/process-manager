# smearor-wrot-process

Child process lifecycle management with Wayland socket binding.

## Types

- **`ProcessConfig`** — Unified configuration via `TypedBuilder` (command, args, env, working_dir, shell, forked, terminate_on_exit, kill_signal, restart_on_exit, stdio, socket)
- **`ProcessManager`** — Concurrent process tracking via `DashMap`, label-based grouping, optional reaper thread
- **`Process`** / **`ProcessId`** — Process handle and unique identifier
- **`ProcessExitEvent`** — Reaper exit notification
- **`StdioConfig`** — Inherit/Null/Piped enum for standard streams
- **`KillSignal`** — Sigterm/Sigkill enum
- **`ProcessManagerError`** / **`ProcessConfigError`** — Error types

## Usage

### Simple child process

```rust
use smearor_wrot_process::{ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("echo")
    .args(vec!["hello".to_string()])
    .stdout(StdioConfig::Piped)
    .stderr(StdioConfig::Piped)
    .build();
let id = manager.start("my-app", &config)?;
manager.stop(id)?;
```

### Forked / detached process

```rust
let config = ProcessConfig::builder()
    .command("my-daemon")
    .forked(true)
    .terminate_on_exit(true)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let id = manager.start("daemon", &config)?;
```

### Reaper monitoring

```rust
use std::time::Duration;

let (sender, receiver) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender);

// Receive exit events in your loop
if let Ok(event) = receiver.try_recv() {
    if event.restart_on_exit {
        // Restart the process
    }
}
```

### Label-based grouping

```rust
manager.start("group-a", &config1)?;
manager.start("group-a", &config2)?;
manager.stop_label("group-a")?; // Stops both
```

### Wayland socket binding

```rust
use smearor_wrot_socket::Socket;
use std::path::PathBuf;

let socket = Socket::from(PathBuf::from("/run/user/1000/wayland-0"));
let config = ProcessConfig::builder()
    .command("gtk4-app")
    .socket(Some(socket))
    .build();
let id = manager.start("wayland-app", &config)?;
```

## License

MIT
