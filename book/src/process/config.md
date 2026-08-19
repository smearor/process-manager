# ProcessConfig

`ProcessConfig` is the unified configuration for spawning a child process, built via `TypedBuilder`.

## Overview

`ProcessConfig` encapsulates everything `ProcessManager::start()` needs to spawn a child process:

- The command to run and its arguments
- Environment variables and working directory
- Process behavior flags (forked, shell, terminate_on_exit, restart_on_exit)
- Signal configuration (kill_signal, terminate_timeout)
- Standard I/O configuration (stdin, stdout, stderr)
- Optional Wayland socket binding

The `TypedBuilder` pattern enforces required fields at compile time — only `command` is required. All other fields have sensible defaults.

## Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `command` | `String` | (required) | Program name or path. Resolved via `which` if not absolute. |
| `args` | `Vec<String>` | `vec![]` | Command-line arguments passed to the program |
| `env` | `HashMap<String, String>` | `HashMap::new()` | Additional environment variables merged into the child's environment |
| `working_dir` | `Option<PathBuf>` | `None` | Working directory for the child process |
| `shell` | `bool` | `false` | Run command via `sh -c` instead of direct execution |
| `forked` | `bool` | `false` | Detach via `setsid()` in `pre_exec` — process gets its own session |
| `terminate_on_exit` | `bool` | `false` | Kill this process when `ProcessManager` is dropped |
| `kill_signal` | `KillSignal` | `Sigterm` | Signal to send on termination (`Sigterm` or `Sigkill`) |
| `terminate_timeout_ms` | `u64` | `5000` | Grace period (ms) before escalating from `SIGTERM` to `SIGKILL` |
| `restart_on_exit` | `bool` | `false` | Flag in `ProcessExitEvent` — consumer decides whether to restart |
| `stdin` | `StdioConfig` | `Null` | Standard input configuration |
| `stdout` | `StdioConfig` | `Null` | Standard output configuration |
| `stderr` | `StdioConfig` | `Null` | Standard error configuration |
| `socket` | `Option<Socket>` | `None` | Wayland socket — sets `WAYLAND_DISPLAY` in child environment |

## Builder Flow

```mermaid
graph LR
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef required fill: #dc0073, stroke: #333333, stroke-width: 2px, color: #ffffff
    classDef optional fill: #00a1e4, stroke: #ffffff, stroke-width: 1px, color: #ffffff
    classDef build fill: #89fc00, stroke: #333333, stroke-width: 2px, color: #000

    A[".command()"] --> B[".args()"]
    B --> C[".env()"]
    C --> D[".forked()"]
    D --> E[".kill_signal()"]
    E --> F[".stdout()"]
    F --> G[".socket()"]
    G --> H[".build()"]

    class A required
    class B optional
    class C optional
    class D optional
    class E optional
    class F optional
    class G optional
    class H build
```

## Usage

### Minimal

```rust
use smearor_wrot_process::ProcessConfig;

let config = ProcessConfig::builder()
    .command("echo".to_string())
    .build();
```

### Full

```rust
use smearor_wrot_process::{ProcessConfig, StdioConfig, KillSignal};
use smearor_wrot_socket::Socket;
use std::path::PathBuf;
use std::collections::HashMap;

let mut env = HashMap::new();
env.insert("MY_VAR".to_string(), "value".to_string());

let config = ProcessConfig::builder()
    .command("my-app".to_string())
    .args(vec!["--verbose".to_string(), "--port".to_string(), "8080".to_string()])
    .env(env)
    .working_dir(PathBuf::from("/tmp"))
    .shell(false)
    .forked(true)
    .terminate_on_exit(true)
    .kill_signal(KillSignal::Sigterm)
    .terminate_timeout_ms(3000)
    .restart_on_exit(true)
    .stdin(StdioConfig::Null)
    .stdout(StdioConfig::Piped)
    .stderr(StdioConfig::Piped)
    .socket(Some(Socket::from(PathBuf::from("/run/user/1000/wayland-0"))))
    .build();
```

## Defaults Explained

- **`shell = false`**: Direct execution is safer and faster. Use `shell = true` only when you need shell features (pipes, redirects, variable expansion).
- **`forked = false`**: By default, processes share the parent's controlling terminal. Use `forked = true` for daemons that should survive terminal close.
- **`terminate_on_exit = false`**: By default, processes are left running when the manager is dropped. Use `true` for processes that should be cleaned up with the manager.
- **`kill_signal = Sigterm`**: Graceful termination by default. The process can catch `SIGTERM` and clean up.
- **`terminate_timeout_ms = 5000`**: 5 seconds grace period before `SIGKILL`. Adjust for processes that need more cleanup time.
- **`stdio = Null`**: All streams are null by default, suitable for background processes. Use `Piped` for output capture or `Inherit` for debugging.
- **`socket = None`**: No Wayland binding by default. Set when spawning Wayland clients.
