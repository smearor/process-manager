# StdioConfig

`StdioConfig` controls how standard streams (stdin, stdout, stderr) are configured for child processes.

## Overview

When spawning a child process, each standard stream can be configured independently:

- **`Inherit`** — The child inherits the parent's stream. Output appears in the parent's terminal. Useful for debugging.
- **`Null`** — The child's stream is connected to `/dev/null`. All output is discarded. Suitable for background processes.
- **`Piped`** — The child's stream is piped. `ProcessManager::start()` spawns reader threads that forward output to `tracing`. This prevents pipe buffer deadlocks when a child produces significant output.

## Default

`StdioConfig::Null` — all streams are null by default. This is the safest default for background processes and services that should not pollute the parent's output.

## Serde

`StdioConfig` implements `Serialize` and `Deserialize` with `#[serde(rename_all = "lowercase")]`, so it serializes as `"inherit"`, `"null"`, and `"piped"`.

## Why Reader Threads for Piped?

When `StdioConfig::Piped` is used, the OS creates a pipe with a fixed buffer size (typically 64KB). If the child writes more than the buffer can hold and nobody reads the pipe, the child blocks — causing a deadlock. `ProcessManager::start()` spawns dedicated reader threads that continuously read from the pipe and forward lines to `tracing::debug!` / `tracing::error!`, preventing this deadlock.

## Usage

```rust
use smearor_wrot_process::{ProcessConfig, StdioConfig};

// Background process — discard all output
let config = ProcessConfig::builder()
    .command("daemon".to_string())
    .stdin(StdioConfig::Null)
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

// Debug mode — inherit parent's streams
let config = ProcessConfig::builder()
    .command("my-app".to_string())
    .stdin(StdioConfig::Inherit)
    .stdout(StdioConfig::Inherit)
    .stderr(StdioConfig::Inherit)
    .build();

// Capture output via tracing
let config = ProcessConfig::builder()
    .command("my-app".to_string())
    .stdout(StdioConfig::Piped)
    .stderr(StdioConfig::Piped)
    .build();
// Reader threads will forward stdout to tracing::debug!
// and stderr to tracing::error!
```
