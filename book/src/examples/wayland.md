# Wayland Socket Binding

Bind a child process to a Wayland socket by setting `WAYLAND_DISPLAY` in its environment.

## Overview

When spawning a Wayland client (e.g. a GTK4 app, a panel, a wallpaper daemon), the client needs to know which Wayland display to connect to. This is done via the `WAYLAND_DISPLAY` environment variable.

`ProcessConfig::socket(Some(socket))` tells `ProcessManager` to extract the socket name (the last path component, e.g. `wayland-0`) from the `Socket` and set it as `WAYLAND_DISPLAY` in the child's environment.

## Example

```rust
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};
use process_manager_socket::{SocketBuilder, SocketManager};

// Create and register a socket
let socket = SocketBuilder::build(&None)?;
let socket_manager = SocketManager::new();
socket_manager.register("default", socket)?;
let socket = socket_manager.get("default").unwrap().clone();

// Spawn a process bound to the socket
let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("gtk4-app".to_string())
    .socket(Some(socket))
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();

let id = manager.start("wayland-client", &config)?;
```

## Multi-Output Example

In a multi-output compositor, each output may have its own socket. Clients spawned for a specific output should be bound to that output's socket:

```rust
use process_manager_socket::{SocketBuilder, SocketManager};
use process_manager::{ProcessConfig, ProcessManager, StdioConfig};

let socket_manager = SocketManager::new();

// Register sockets for each output
socket_manager.register("eDP-1", SocketBuilder::build(&Some("wayland-0".to_string()))?)?;
socket_manager.register("HDMI-1", SocketBuilder::build(&Some("wayland-1".to_string()))?)?;

let manager = ProcessManager::new();

// Spawn a panel on eDP-1
let edp_socket = socket_manager.get("eDP-1").unwrap().clone();
let panel_config = ProcessConfig::builder()
    .command("waybar".to_string())
    .socket(Some(edp_socket))
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
manager.start("panel-eDP-1", &panel_config)?;

// Spawn a wallpaper on HDMI-1
let hdmi_socket = socket_manager.get("HDMI-1").unwrap().clone();
let wallpaper_config = ProcessConfig::builder()
    .command("swaybg".to_string())
    .args(vec!["--image".to_string(), "/path/to/wallpaper.jpg".to_string()])
    .socket(Some(hdmi_socket))
    .stdout(StdioConfig::Null)
    .stderr(StdioConfig::Null)
    .build();
manager.start("wallpaper-HDMI-1", &wallpaper_config)?;
```

## How It Works

When `config.socket` is `Some(socket)`:

1. `ProcessManager::start()` extracts the socket name from the `Socket` path (e.g. `wayland-0` from `/run/user/1000/wayland-0`)
2. The socket name is inserted into the child's environment as `WAYLAND_DISPLAY=wayland-0`
3. The child process connects to the Wayland display using this environment variable

If `config.socket` is `None`, `WAYLAND_DISPLAY` is not set - the child inherits the parent's `WAYLAND_DISPLAY` if present.
