# Error Types

## SocketBuilderError

Errors that can occur when building a socket path via `SocketBuilder::build()`.

| Variant | When | Description |
|---------|------|-------------|
| `XdgRuntimeDirNotSet` | `build()` | `XDG_RUNTIME_DIR` environment variable is not set — the user session may not be properly initialized |
| `SocketAlreadyExists` | `build()` | A socket file with the given name already exists in `XDG_RUNTIME_DIR` — only returned when a specific name is requested |

## SocketManagerError

Errors that can occur when managing sockets in `SocketManager`.

| Variant | When | Description |
|---------|------|-------------|
| `AlreadyRegistered` | `register()` | A socket with the given name has already been registered in this `SocketManager` instance |

## Usage

```rust
use process_manager_socket::{SocketBuilder, SocketBuilderError};

match SocketBuilder::build(&Some("wayland-0".to_string())) {
    Ok(socket) => println!("Socket: {}", socket),
    Err(SocketBuilderError::XdgRuntimeDirNotSet) => {
        eprintln!("XDG_RUNTIME_DIR is not set");
    }
    Err(SocketBuilderError::SocketAlreadyExists) => {
        eprintln!("Socket wayland-0 already exists");
    }
    Err(e) => eprintln!("Error: {}", e),
}
```
