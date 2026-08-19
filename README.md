# smearor-wrot-process-manager

Shared socket and process management crates for `smearor-wrot` and `smearor-swipe-launcher`.

## Workspace Structure

- **`smearor-wrot-socket`** — Wayland socket path management (`Socket`, `SocketBuilder`, `SocketManager`)
- **`smearor-wrot-process`** — Child process lifecycle management (`ProcessConfig`, `ProcessManager`, reaper thread)

## License

MIT