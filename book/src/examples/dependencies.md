# Dependencies

This example demonstrates dependency ordering and timeout.

## Label-based dependency

Process B depends on process A by label. B starts in `Waiting` state and transitions to `Running` once A is `Running`.

```rust
use process_manager::{
    DependencyRef, ProcessConfig, ProcessManager, StdioConfig,
};
use std::time::Duration;

let (tx, rx) = std::sync::mpsc::channel();
let manager = ProcessManager::with_reaper(Duration::from_millis(100), tx)?;

let compositor = ProcessConfig::builder()
    .command("hyprland".to_string())
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let comp_id = manager.start("compositor", &compositor)?;

let panel = ProcessConfig::builder()
    .command("smearor-swipe-launcher".to_string())
    .depends_on(vec![DependencyRef::Label("compositor".to_string())])
    .dependency_timeout_ms(10_000)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let panel_id = manager.start("panel", &panel)?;
// Panel starts in Waiting, transitions to Running once compositor is Running
```

## Multiple dependencies

Process C depends on both A and B. C starts only after both are `Running`.

```rust
let config_c = ProcessConfig::builder()
    .command("my-service".to_string())
    .depends_on(vec![
        DependencyRef::Label("a".to_string()),
        DependencyRef::Label("b".to_string()),
    ])
    .dependency_timeout_ms(30_000)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let id_c = manager.start("c", &config_c)?;
```

## Dependency timeout

If dependencies are not `Running` within `dependency_timeout_ms`, the process transitions to `Failed`.

```rust
let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .depends_on(vec![DependencyRef::Label("missing".to_string())])
    .dependency_timeout_ms(5_000) // Fail after 5 seconds
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let id = manager.start("dependent", &config)?;
// Process enters Waiting, then transitions to Failed after 5 seconds
```
