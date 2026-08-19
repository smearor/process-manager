# smearor-wrot-socket

Wayland socket path management for `smearor-wrot` and `smearor-swipe-launcher`.

## Types

- **`Socket`** — `PathBuf` newtype with `Deref<Target = Path>`, `Display`, `AsRef<OsStr>`, `AsRef<str>` implementations
- **`SocketBuilder`** — Builds socket paths in `XDG_RUNTIME_DIR`, validates existing names or generates unique ones
- **`SocketManager`** — Multi-socket manager using `DashMap`, shareable via `Arc` across threads
- **`SocketBuilderError`** / **`SocketManagerError`** — Error types

## Usage

```rust
use smearor_wrot_socket::{SocketBuilder, SocketManager};

// Build a socket (auto-generates unique name if None)
let socket = SocketBuilder::build(&None)?;

// Register in manager
let manager = SocketManager::new();
manager.register("default", socket)?;

// Retrieve by name
let socket = manager.get("default");
```

## License

MIT
