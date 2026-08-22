# Quick Start

Get started with `smearor-wrot-process-manager` in a few minutes.

## Installation

Add the crates to your `Cargo.toml`:

```toml
[dependencies]
process-manager = { git = "https://github.com/smearor/smearor-wrot-process-manager" }
# Optional: only if you need Wayland socket management
process-manager-socket = { git = "https://github.com/smearor/smearor-wrot-process-manager" }
```

## Minimal Example

Spawn a child process, let it run, and stop it:

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["5".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("sleeper", &config)?;
    println!("Started process with ID {}", id);

    // Check if it's running
    let process = manager.get(id).unwrap();
    println!("Running: {}", process.is_running());
    drop(process);

    // Stop it
    manager.stop(id)?;
    println!("Stopped");

    Ok(())
}
```

## With Reaper and Restart

Spawn a process that auto-restarts on exit:

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::Error::Error>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let manager = ProcessManager::with_reaper(Duration::from_secs(2), sender)?;

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .restart_on_exit(true)
        .stdout(StdioConfig::Null)
        .build();

    manager.start("service", &config)?;

    // Wait for exit and restart
    let event = receiver.recv_timeout(Duration::from_secs(10))?;
    println!("Process {} exited, restarting...", event.label);

    if event.restart_on_exit {
        manager.start(&event.label, &config)?;
    }

    Ok(())
}
```

## With Wayland Socket

Spawn a Wayland client bound to a specific socket:

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};
use process_manager_socket::SocketBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ProcessManager::new();
    let socket = SocketBuilder::build(&None)?;

    let config = ProcessConfig::builder()
        .command("gtk4-demo".to_string())
        .socket(Some(socket))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("wayland-client", &config)?;
    println!("Started Wayland client with ID {}", id);

    Ok(())
}
```

## Next Steps

- [Architecture](architecture.md) — Visual overview of crate internals and design decisions
- [ProcessConfig](process/config.md) — Full configuration reference
- [ProcessManager](process/manager.md) — Complete API documentation
- [Usage Examples](examples.md) — More practical examples
- [Migration Guide](migration.md) — Migrating from old patterns
