# Supervisor Strategies

The `ProcessManager` supports supervisor strategies that control which processes are restarted when one process in a group crashes. This follows the Erlang OTP supervisor model.

## Strategies

| Strategy | Description |
|----------|-------------|
| `OneForOne` (default) | Restart only the crashed process |
| `OneForAll` | Restart all processes in the same label group |
| `RestForOne` | Restart the crashed process and all processes started after it |

## How it works

When the reaper detects a crash and determines that a restart should occur (based on `restart_on_exit`, `restart_trigger`, and rate limiting), it reads the `supervisor_strategy` from the crashed process's config.

- **OneForOne** - Only the crashed process is restarted. Other processes in the group are unaffected.
- **OneForAll** - All processes in the same label group are stopped and restarted. Each process gets its own backoff timer.
- **RestForOne** - The crashed process and all processes with a higher `spawn_sequence` in the same group are stopped and restarted.

## Interaction with dependencies

When using `OneForAll` or `RestForOne` with dependencies, restart order follows the dependency chain. The compositor restarts first, then dependent processes start once the compositor is `Running`.

## Cascade flag

When a process is cascade-killed (stopped because another process in its group crashed), it is marked with `cascade_flag = true`. The reaper skips supervisor strategy logic for cascade-killed processes and emits a `Stopped` event instead of triggering another restart cycle. This prevents recursive cascades.

## Example

```rust
use process_manager::{ProcessConfig, ProcessManager, SupervisorStrategy, StdioConfig};

let manager = ProcessManager::new();

let config = ProcessConfig::builder()
    .command("my-service".to_string())
    .restart_on_exit(true)
    .supervisor_strategy(SupervisorStrategy::OneForAll)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id = manager.start("group-a", &config)?;
```
