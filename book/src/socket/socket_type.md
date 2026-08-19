# Socket Type

`Socket` is a `PathBuf` newtype representing a Wayland socket path.

It exists as a distinct type rather than using `PathBuf` directly for two reasons:

1. **Type safety** — Functions that expect a Wayland socket can take `&Socket` instead of `&Path`, preventing accidental misuse with arbitrary paths.
2. **Ergonomic trait implementations** — `AsRef<str>` and `Display` make it easy to extract the socket name (e.g. `wayland-0`) for setting `WAYLAND_DISPLAY` in child environments.

## Trait Implementations

| Trait | Purpose |
|-------|---------|
| `Deref<Target = Path>` | Access the inner path for filesystem operations |
| `Display` | Format the path as a string |
| `AsRef<OsStr>` | Interop with `std::ffi::OsStr` |
| `AsRef<str>` | String access for environment variable names |
| `From<PathBuf>` | Construct from a `PathBuf` |

## Usage

```rust
use smearor_wrot_socket::Socket;
use std::path::PathBuf;

// Construct from a path
let socket = Socket::from(PathBuf::from("/run/user/1000/wayland-0"));

// Display the full path
println!("{}", socket); // /run/user/1000/wayland-0

// Access as &Path via Deref
let path: &std::path::Path = &*socket;
assert!(path.exists());

// Access as &str via AsRef
let socket_str: &str = socket.as_ref();
assert_eq!(socket_str, "/run/user/1000/wayland-0");
```

## How It's Used by ProcessManager

When a `Socket` is passed to `ProcessConfig::socket(Some(socket))`, the `ProcessManager::start()` method extracts the socket name (the last path component) and sets it as the `WAYLAND_DISPLAY` environment variable in the child process. This allows the child to connect to the correct Wayland display.

```rust
use smearor_wrot_process::{ProcessConfig, ProcessManager, StdioConfig};
use smearor_wrot_socket::Socket;
use std::path::PathBuf;

let socket = Socket::from(PathBuf::from("/run/user/1000/wayland-1"));

let config = ProcessConfig::builder()
    .command("gtk4-app".to_string())
    .socket(Some(socket))
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

// The child process will have WAYLAND_DISPLAY=wayland-1
let id = manager.start("wayland-client", &config)?;
```
