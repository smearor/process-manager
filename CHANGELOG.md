# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## [0.0.0-alpha-1] - 2026-xx-xx

### Added

- `ProcessManager` for concurrent child process lifecycle management via `DashMap`
- `ProcessConfig` with `TypedBuilder` (command, args, env, working_dir, shell, forked, terminate_on_exit, kill_signal, restart_on_exit, stdio, socket)
- `Process` / `ProcessId` / `ProcessInfo` types for process handles and snapshots
- `ProcessExitEvent` emitted by the optional reaper thread on process exit
- `StdioConfig` enum (Inherit/Null/Piped) with reader threads for output capture
- `KillSignal` enum (Sigterm/Sigkill) with serde support for termination config
- `Signal` enum with 11 common Unix signals (SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGKILL, SIGUSR1, SIGUSR2, SIGWINCH, SIGSTOP, SIGCONT, SIGALRM) for general process control
- `ProcessManager::start(label, &config)` - Spawn a child process with label and config
- `ProcessManager::stop(id)` - Stop a process with configured kill signal and SIGKILL escalation
- `ProcessManager::stop_label(label)` - Stop all processes under a label
- `ProcessManager::stop_all()` - Stop all tracked processes
- `ProcessManager::send_signal(id, signal)` - Send an arbitrary signal to a process by ID without removing it from the manager
- `ProcessManager::send_signal_label(label, signal)` - Send a signal to all processes with a given label
- `ProcessManager::restart(id)` / `ProcessManager::restart_label(label)` - Restart processes preserving config and label
- `ProcessManager::with_reaper(poll_interval, sender)` - Construct with a reaper thread for exit notifications and zombie prevention
- Convenience getter methods: `is_forked`, `get_label`, `get_terminate_on_exit`, `is_running`, `get_pid`, `get_program_name`, `get_config`, `get_info`
- `From<KillSignal> for Signal` conversion
- `Socket`, `SocketBuilder`, `SocketManager` for Wayland socket management
- `Socket::path()` accessor (field made private)
- Serde support for `Socket`, `StdioConfig`, `ProcessConfig`, and `KillSignal`
- Crate-level rustdoc for both `process-manager` and `process-manager-socket`
- 7 examples: `basic_spawn`, `forked`, `reaper`, `restart`, `stop_escalation`, `wayland_socket`, `send_signal`
- `socket_basic` example for `process-manager-socket`
- Integration tests for process lifecycle, socket management, and TOCTOU concurrency safety
- Tests for `stop_label` / `stop_all` escalation timing and correctness
- mdBook documentation with architecture diagrams, type references, and usage examples
- CI workflows: build, test, clippy, audit, docs, book, MSRV, release, label management
- Run configurations for build, clippy, fmt, test, and update

### Changed

- `Socket` field made private - use `path()`, `Deref`, or `AsRef` instead of accessing `.0` directly
- `SocketManager::register()` now uses `DashMap` entry API for atomic check-and-insert (TOCTOU fix)
- `ProcessManager::get()` is now `pub(crate)` - use `get_info()` for a `ProcessInfo` snapshot or the convenience getter methods instead
- `ProcessConfig` is no longer cloned per process - stored as `Arc<ProcessConfig>`
- Reader threads are now joined on process stop instead of being fire-and-forget
- `stop_many` stops multiple processes concurrently instead of sequentially
- `#[must_use]` applied to all public structs and enums
- Reorganized `kill_signal.rs` and `signal.rs` into a `signal/` directory module with `signal/mod.rs`, `signal/kill_signal.rs`, and `signal/signal.rs`
- `Process::send_signal()` now preserves the raw OS error code (uses `from_raw_os_error` instead of `Error::other`) so callers can detect ESRCH
- `ProcessManager::stop()` now handles ESRCH gracefully - if the process has already exited, it joins readers and returns `Ok(())` instead of erroring
- `KillSignal::to_signal()` renamed to `KillSignal::to_nix_signal()` for consistency with `Signal::to_nix_signal()`

### Fixed

- TOCTOU race condition in `SocketManager::register()` (contains_key + insert was not atomic)
- Deadlock risk from exposing `ProcessManager::get()` - replaced with `get_info()` snapshot and dedicated getters
- Defensive fix if setting process group leader fails
- Error handling if reaper thread could not be created
- Removed dead error variant
- Removed unnecessary clone in `SocketBuilder::build_socket_path()`
- Removed unused dependency

### Distribution

### Infrastructure

- GitHub Actions CI with build, test, clippy, audit, docs, book, MSRV, release, and label management workflows
- Dependabot configuration with auto-approve-and-merge
- PR auto-assign and labeler configuration
- `.run/` configurations for IDE build, clippy, fmt, test, and update tasks
