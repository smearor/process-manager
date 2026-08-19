# Simple Child Process

Spawn a child process with piped stdout/stderr for output capture.

## Overview

This is the most basic usage of `ProcessManager`: spawn a process, let it run, and stop it when done. The `Piped` stdio configuration causes `ProcessManager` to spawn reader threads that forward stdout to `tracing::debug!` and stderr to `tracing::error!`, preventing pipe buffer deadlocks.

## Example

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
// Reader threads capture stdout/stderr and forward to tracing

// Check if the process is still running
let process = manager.get(id).unwrap();
println!("Running: {}", process.is_running());
drop(process); // Release DashMap guard

// Stop when done
manager.stop(id)?;
```

## What Happens Internally

1. `ProcessManager::start()` resolves `echo` via `which` (since it's not an absolute path)
2. A `std::process::Command` is built with the piped stdio configuration
3. The child is spawned and stored in the `DashMap` under the label `"echo-app"`
4. Reader threads are spawned for stdout and stderr, forwarding lines to `tracing`
5. `manager.stop(id)` sends `SIGTERM`, waits up to 5 seconds, then escalates to `SIGKILL` if needed
