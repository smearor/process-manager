# Error Types

## ProcessManagerError

Errors returned by `ProcessManager` operations.

| Variant | When | Description |
|---------|------|-------------|
| `ExecutableNotFound(String)` | `start()` | The command could not be resolved via `which` and is not an absolute path |
| `SpawnFailed(String)` | `start()` | `std::process::Command::spawn()` returned an error |
| `NotFound(ProcessId)` | `stop()`, `get()` | No process with the given `ProcessId` exists in the `DashMap` |
| `NixError(nix::Error)` | `stop()` | A `nix` signal operation failed - the process may have already exited |

## ProcessConfigError

Errors returned during `ProcessConfig` construction. Currently minimal since `TypedBuilder` enforces required fields at compile time.

## Usage

```rust
use process_manager::{ProcessConfig, ProcessManager, ProcessManagerError};

let manager = ProcessManager::new();

let config = ProcessConfig::builder()
    .command("nonexistent-program".to_string())
    .build();

match manager.start("test", &config) {
    Ok(id) => println!("Started with ID {}", id),
    Err(ProcessManagerError::ExecutableNotFound(cmd)) => {
        eprintln!("Program '{}' not found in PATH", cmd);
    }
    Err(ProcessManagerError::SpawnFailed(msg)) => {
        eprintln!("Failed to spawn: {}", msg);
    }
    Err(e) => eprintln!("Error: {}", e),
}
```
