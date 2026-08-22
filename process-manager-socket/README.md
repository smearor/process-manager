# process-manager-socket

[![Rust Edition](https://img.shields.io/badge/rust-2024-f5b700.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-89fc00.svg)](LICENSE.md)

Wayland socket path management for `smearor-wrot` and `smearor-swipe-launcher`.

## Overview

`process-manager-socket` provides types for building, registering, and managing Wayland socket paths. Sockets are created in `XDG_RUNTIME_DIR` and can be shared across threads via `Arc<SocketManager>`.

In a multi-output Wayland compositor, each output may need its own socket. `SocketManager` allows registering multiple sockets by name and retrieving them when spawning compositor clients.

## Features

- **`Socket` newtype** - `PathBuf` wrapper with `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, `AsRef<str>`
- **`SocketBuilder`** - Builds socket paths in `XDG_RUNTIME_DIR`, validates existing names or generates unique ones
- **`SocketManager`** - Multi-socket manager using `DashMap`, shareable via `Arc` across threads
- **Error types** - `SocketBuilderError` and `SocketManagerError` via `thiserror`

## Quick Start

```rust
use process_manager_socket::{SocketBuilder, SocketManager};
use std::sync::Arc;

// Build a socket (auto-generates unique name if None)
let socket = SocketBuilder::build(&None)?;

// Register in manager
let manager = Arc::new(SocketManager::new());
manager.register("default", socket)?;

// Retrieve by name
let socket = manager.get("default");
assert!(socket.is_some());
```

## API

### `Socket`

A `PathBuf` newtype representing a Wayland socket path. Implements `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, and `AsRef<str>`.

### `SocketBuilder`

Constructs socket paths in `XDG_RUNTIME_DIR`. If a name is provided, validates uniqueness. If no name is provided, auto-generates a unique name (`wayland-0`, `wayland-1`, ...).

### `SocketManager`

Concurrent multi-socket manager using `DashMap`. Register, retrieve, remove, and list sockets by name. Shareable via `Arc` across threads.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dashmap` | Concurrent socket tracking |
| `thiserror` | Error types |

## License

MIT
