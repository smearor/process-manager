# Usage Examples

This section provides practical examples for common use cases of `smearor-wrot-process-manager`.

## Examples

- [Simple Child Process](simple.md) — Spawn a process with piped output capture via tracing
- [Forked / Detached Process](forked.md) — Spawn a daemon that detaches from the controlling terminal via `setsid()`
- [Reaper Monitoring](reaper.md) — Detect process exits with `ProcessExitEvent`, implement restart logic, and integrate with GTK
- [Label-based Grouping](labels.md) — Manage multiple processes under a shared label for grouped start/stop
- [Stop Escalation](stop_escalation.md) — Observe SIGTERM to SIGKILL escalation with a short timeout
- [Restart](restart.md) — Restart a process preserving its config and label
- [Wayland Socket Binding](wayland.md) — Bind a child process to a Wayland socket by setting `WAYLAND_DISPLAY`
- [Send Signal](send_signal.md) — Send arbitrary signals (SIGHUP, SIGUSR1, etc.) to running processes

## Common Patterns

All examples share these common patterns:

1. **Create a `ProcessManager`** — Either `new()` (no reaper) or `with_reaper()` (with exit notifications)
2. **Build a `ProcessConfig`** — Using the `TypedBuilder` pattern with `.command()` as the only required field
3. **Start the process** — `manager.start(label, &config)` returns a `ProcessId`
4. **Stop or monitor** — Call `manager.stop(id)` for explicit termination, or use the reaper for exit detection
