# Label-based Grouping

Start and stop multiple processes under a shared label.

## Overview

Labels provide a way to group related processes. Multiple processes can share the same label, and operations like `stop_label()` and `pids_by_label()` act on all processes with that label. This is useful for:

- **Worker pools** — Start N workers under a shared label, stop them all at once
- **Application groups** — Group auxiliary processes (e.g. a main app + its helpers)
- **Service management** — Start/stop services by name rather than tracking individual `ProcessId`s

Labels are **not** unique identifiers — multiple processes can share the same label. Use `ProcessId` for individual process operations and labels for grouped operations.

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef manager fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef label fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef process fill: #89fc00, stroke: #333333, stroke-width: 1px, color: #000

    M["ProcessManager<br/><small>DashMap</small>"]
    M --> L1["label: \"frontend\""]
    M --> L2["label: \"backend\""]
    L1 --> P1["Process #1<br/>PID 1234"]
    L1 --> P2["Process #2<br/>PID 1235"]
    L2 --> P3["Process #3<br/>PID 1236"]
    L2 --> P4["Process #4<br/>PID 1237"]
    L2 --> P5["Process #5<br/>PID 1238"]

    S["stop_label#40;&quot;backend&quot;#41;"] -.->|"stops all"| L2

    class M manager
    class L1 label
    class L2 label
    class P1 process
    class P2 process
    class P3 process
    class P4 process
    class P5 process
```

## Example

```rust
use smearor_wrot_process::{ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("worker".to_string())
    .stdout(StdioConfig::Null)
    .build();

// Start multiple workers under the same label
manager.start("worker-pool", &config)?;
manager.start("worker-pool", &config)?;
manager.start("worker-pool", &config)?;

// Check all PIDs for the label
let pids = manager.pids_by_label("worker-pool");
assert_eq!(pids.len(), 3);

// Stop all workers in the pool at once
manager.stop_label("worker-pool")?;
assert!(manager.is_empty());
```

## Querying by Label

| Method | Returns | Description |
|--------|---------|-------------|
| `pids_by_label(label)` | `Vec<u32>` | All PIDs for processes with this label |
| `get_by_label(label)` | `Vec<Process>` | All processes with this label |
| `labels()` | `Vec<String>` | All distinct labels currently registered |

## Mixed Labels

You can have multiple labels active at the same time:

```rust
manager.start("frontend", &frontend_config)?;
manager.start("frontend", &frontend_config)?;
manager.start("backend", &backend_config)?;
manager.start("backend", &backend_config)?;
manager.start("backend", &backend_config)?;

assert_eq!(manager.pids_by_label("frontend").len(), 2);
assert_eq!(manager.pids_by_label("backend").len(), 3);
assert_eq!(manager.labels().len(), 2);

// Stop only the backend
manager.stop_label("backend")?;
assert_eq!(manager.pids_by_label("frontend").len(), 2);
assert!(manager.pids_by_label("backend").is_empty());
```
