# Restart

Restart a process while preserving its config and label.

## Overview

`ProcessManager::restart(id)` stops the process (with escalation) and starts a new one with the same config and label. It returns the same `ProcessId` - the process is updated in-place. `restart_label(label)` does the same for all processes under a label.

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef input fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef stop fill: #dc0073, stroke: #333333, stroke-width: 2px, color: #ffffff
    classDef start fill: #89fc00, stroke: #333333, stroke-width: 2px, color: #000
    classDef done fill: #04e762, stroke: #333333, stroke-width: 1px, color: #000

    A["restart(id)"] --> B{"Process in\nRestarting state?"}
    B -- Yes --> C["Cancel backoff\nSpawn immediately"]
    B -- No --> D["stop(id)"]
    D --> E["Old process\nremoved"]
    C --> F["Spawn in-place\n<small>same config + label</small>"]
    E --> F
    F --> G["Same ProcessId\npreserved"]
    G --> H["Manager still\nhas process"]

    class A input
    class B stop
    class C start
    class D stop
    class E stop
    class F start
    class G start
    class H done
```

## Example

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("worker", &config)?;
    println!("Started worker (PID {})", manager.get_pid(id).unwrap());

    // Restart - stops the old process, starts a new one with same config+label
    // ProcessId is preserved (updated in-place)
    let new_id = manager.restart(id)?;
    println!("Restarted worker (new PID {})", manager.get_pid(new_id).unwrap());
    assert_eq!(id, new_id);

    // Clean up
    manager.stop(new_id)?;
    println!("Manager empty: {}", manager.is_empty());

    Ok(())
}
```

## Label-based Restart

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("sleep".to_string())
    .args(vec!["10".to_string()])
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

// Start a pool of 3 workers
for _ in 0..3 {
    manager.start("worker-pool", &config)?;
}

// Restart all workers in the pool
manager.restart_label("worker-pool")?;
```

## Running the Example

```sh
cargo run --example restart
```
