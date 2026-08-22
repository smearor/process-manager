# SocketBuilder

`SocketBuilder` constructs socket paths in `XDG_RUNTIME_DIR`.

## Behavior

`SocketBuilder::build()` takes an optional name and returns a `Socket`:

- If a name is provided (`Some("wayland-0")`), it validates that no socket file with that name already exists in `XDG_RUNTIME_DIR`.
- If no name is provided (`None`), it generates a unique name by incrementing a counter (`wayland-0`, `wayland-1`, `wayland-2`, ...) until an unused name is found.
- The socket path is constructed as `XDG_RUNTIME_DIR/{name}`.

## Build Flow

```mermaid
graph TD
    classDef default fill: #1e1e1e, stroke: #333333, stroke-width: 1px, color: #ffffff
    classDef input fill: #00a1e4, stroke: #ffffff, stroke-width: 2px, color: #ffffff
    classDef check fill: #f5b700, stroke: #333333, stroke-width: 2px, color: #000000
    classDef output fill: #89fc00, stroke: #333333, stroke-width: 1px, color: #000
    classDef error fill: #dc0073, stroke: #333333, stroke-width: 1px, color: #ffffff

    A["SocketBuilder::build(&name)"] --> B{"name provided?"}
    B -->|Some name| C{"Socket exists<br/>in XDG_RUNTIME_DIR?"}
    B -->|None| D["Generate unique name<br/>wayland-0, wayland-1, ..."]
    C -->|No| E["Construct path<br/>XDG_RUNTIME_DIR/name"]
    C -->|Yes| F["SocketBuilderError<br/>::SocketAlreadyExists"]
    D --> E
    E --> G["Return Socket"]

    class A input
    class B check
    class C check
    class D check
    class E output
    class F error
    class G output
```

## Usage

```rust
use process_manager_socket::SocketBuilder;

// Auto-generate unique name (recommended)
let socket = SocketBuilder::build(&None)?;
// e.g. /run/user/1000/wayland-0

// Use a specific name
let socket = SocketBuilder::build(&Some("wayland-1".to_string()))?;
// /run/user/1000/wayland-1
```

## Errors

| Error | When |
|-------|------|
| `SocketBuilderError::XdgRuntimeDirNotSet` | `XDG_RUNTIME_DIR` environment variable is not set |
| `SocketBuilderError::SocketAlreadyExists` | A socket file with the given name already exists in `XDG_RUNTIME_DIR` |
