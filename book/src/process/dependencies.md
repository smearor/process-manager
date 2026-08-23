# Dependencies

Processes can declare dependencies on other processes. A process with `depends_on` will not start until all its dependencies are `Running`.

## DependencyRef

Dependencies are declared via `DependencyRef`:

- `DependencyRef::label("compositor")` - resolved and bound to a `ProcessId` at `start()` time; the first `Running` process with that label is selected and the binding persists for the dependent's lifetime
- `DependencyRef::id(process_id)` - the process with the given `ProcessId` must be `Running`

## Start flow

When `start()` is called with non-empty `depends_on`:

1. The process is inserted into the `DashMap` with `ProcessState::Waiting`.
2. A `ProcessId` is assigned immediately.
3. The reaper loop checks `Waiting` processes on each poll cycle.
4. Once all dependencies are `Running`, the process is spawned and transitions to `Starting`.
5. If dependencies are not `Running` within `dependency_timeout_ms`, the process transitions to `Failed`.

## ProcessState::Waiting

| Phase | `ProcessState` |
|-------|---------------|
| Queued, waiting for deps | `Waiting` |
| Dependencies ready, spawning | `Starting` |
| Process running | `Running` |
| Dependency timeout | `Failed` |

`manager.state(id)` returns `Waiting` while the process is waiting for dependencies. `is_alive()` returns `true` for `Waiting`.

## Fail-fast behavior

If a dependency enters a terminal state (`Failed` or permanently `Stopped` without `restart_on_exit`), the dependent process is immediately failed. This applies to both `Waiting` and `Running` processes:

- **Waiting processes** - The reaper checks resolved dependencies on each poll cycle. If a dependency is terminal or removed, the Waiting process transitions to `Failed`.
- **Running processes** - The reaper monitors resolved dependencies of Running processes. If a dependency is removed or enters a terminal state, the Running process is killed and transitions to `Failed`.

A dependency in `Restarting` state (backoff wait) is **not** terminal. The dependent stays in `Waiting` until the dependency recovers or exhausts its restart limit.

## Label binding semantics

Label bindings are resolved once and persist for the dependent's lifetime:

- When a `Label` dependency is resolved, the resulting `ProcessId` is stored in `resolved_deps`.
- The binding does **not** re-resolve if the dependency process is removed. A new process with the same label will not satisfy the binding.
- This ensures predictable behavior: if process A (label `"compositor"`) is stopped and process C is started with the same label, dependents of A do not switch to C.

## Cycle detection

At `start()` time, the manager performs a DFS-based cycle check. If a dependency cycle is detected (e.g. A depends on B, B depends on A), `start()` returns `ProcessManagerError::DependencyCycle`.

## Example

```rust
use process_manager::{DependencyRef, Label, ProcessConfig, ProcessManager, StdioConfig};
use std::time::Duration;

let manager = ProcessManager::new();

let compositor = ProcessConfig::builder()
    .command("hyprland".to_string())
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let comp_id = manager.start("compositor", &compositor)?;

let panel = ProcessConfig::builder()
    .command("smearor-swipe-launcher".to_string())
    .depends_on(vec![DependencyRef::label("compositor")])
    .dependency_timeout_ms(10_000)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
let panel_id = manager.start("panel", &panel)?;
// Panel starts in Waiting state, transitions to Running once compositor is Running
```
