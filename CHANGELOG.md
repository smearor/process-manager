# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- `Label` newtype wrapping `String` with `#[serde(transparent)]` - type-safe process labels for grouped operations and dependency references, consistent with `ProcessId`
- `DependencyRef::label()` and `DependencyRef::id()` convenience constructors accepting `impl Into<Label>` / `impl Into<ProcessId>`
- `From<u64> for ProcessId` - enables `DependencyRef::id(42u64)`
- `ProcessId`, `ProcessInfo`, `ProcessExitEvent`, `ExitedProcess` now use `Label` instead of `String` for label fields
- `ProcessManager` methods accept `impl Into<Label>` for label parameters (`start`, `stop_label`, `restart_label`, `get_by_label`, `pids_by_label`, `send_signal_label`, `start_with_deps`, `group_members`)
- `ProcessManager::get_label()` returns `Option<Label>` instead of `Option<String>`
- `ProcessManager::labels()` returns `Vec<Label>` instead of `Vec<String>`
- `ProcessManagerError::DependencyTimeout.dependency` changed to `Option<DependencyRef>` (was `DependencyRef`)
- Book page: `process/label.md`
- `SupervisorStrategy` enum (`OneForOne`, `OneForAll`, `RestForOne`) - controls which processes are restarted when one process in a group crashes, following the Erlang OTP supervisor model
- `DependencyRef` enum (`Label(String)`, `Id(ProcessId)`) - declares start-order dependencies by label or process ID
- `ProcessState::Waiting` variant - processes queued but waiting for dependencies to become `Running`; `is_alive()` returns `true`
- `ProcessConfig.supervisor_strategy`, `ProcessConfig.depends_on`, `ProcessConfig.dependency_timeout_ms`, `ProcessConfig.cascade_stop` fields with builder support
- `ProcessManagerError::DependencyTimeout`, `DependencyNotFound`, `DependencyCycle` error variants
- `ProcessManager::start_with_deps()` - blocking API that waits for dependencies to become `Running` before returning
- `ProcessManager::dependents(id)` - returns process IDs that depend on the given process
- `ProcessManager::group_members(label)` - returns all process IDs with the given label
- DFS-based cycle detection in `start()` using a local `HashMap` snapshot
- Reaper loop Phase 5: checks `Waiting` processes, resolves label dependencies, spawns when all deps are `Running`, fail-fast on terminal dependencies, timeout if deps not ready
- Reaper loop Phase 6: monitors `Running` processes for terminal dependencies, fail-fast when a resolved dependency is removed or enters a terminal state
- Supervisor strategy cascade kill in reaper loop: `OneForAll` stops all group members, `RestForOne` stops crashed process and all with higher `spawn_sequence`
- `cascade_flag` on `Process` to prevent recursive cascade triggers
- Dependency-ordered restart: `perform_restart` transitions to `Waiting` if dependencies are not `Running`, ensuring ordered restart in `OneForAll`/`RestForOne`
- `waiting_since: Option<Instant>` on `Process` for dependency timeout tracking
- `spawn_sequence: u64` on `Process` for deterministic `RestForOne` ordering
- `resolved_deps: Vec<ProcessId>` on `Process` for persistent label binding
- Label binding semantics: resolved once at `start()` time, persists for dependent's lifetime, does not re-resolve to new processes with same label
- Book pages: `process/supervisor_strategies.md`, `process/dependencies.md`, `examples/supervisor_strategies.md`, `examples/dependencies.md`
- Integration tests for supervisor strategies, dependencies, cycle detection, fail-fast, label binding, cascade stop, and dependency-ordered restart (31 tests total)
- `ProcessState` enum with lifecycle states: `Starting`, `Running`, `Stopping`, `Stopped`, `Crashed`, `Restarting`, `Failed`
  - `ProcessState::is_alive()` - `true` for `Starting`, `Running`, `Waiting`, `Stopping`, `Restarting`
  - `ProcessState::is_terminated()` - `true` for `Stopped`, `Crashed`, `Failed`
  - `Default` (defaults to `Starting`), `Display` (lowercase string), `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`
- `RestartTrigger` enum (`CrashOnly`, `Always`) - controls when automatic restart is triggered
- `RestartPolicy` enum (`Immediate`, `Backoff(BackoffConfig)`) - controls restart strategy
- `BackoffConfig` struct (`initial_delay`, `multiplier`, `max_delay`, `max_restarts`, `min_uptime`) - exponential backoff configuration
- `RestartState` struct - internal state tracking for restart count, backoff timer, and stable uptime resets
- `ProcessConfig.restart_trigger` and `ProcessConfig.restart_policy` fields with builder support
- `ProcessManagerError::ProcessInRestartingState` error variant - returned by `send_signal()` when process is in `Restarting` state
- Automatic restart with exponential backoff and rate limiting in the reaper thread
- `ProcessId` preservation across restarts - `restart()` updates the process in-place instead of removing and re-inserting
- Spawn failure during automatic restart sets state to `Failed`, emits single `ProcessExitEvent`, and removes process
- `stop()` during `Restarting` state cancels backoff silently (no event, no signal)
- `restart()` during `Restarting` state cancels backoff and spawns immediately
- `spawn_internal()` helper method extracted from `start()` for reuse in `restart()`
- Book page for Restart Policy at `book/src/process/restart_policy.md`
- Integration tests for restart with backoff, rate limiting, `Restarting` state interactions, and `ProcessId` preservation
- Unit tests for `BackoffConfig`, `RestartState`, `RestartTrigger`, `RestartPolicy`
- `Process.state` field and `Process::state()` method - lazily updates state via non-blocking `try_wait()`
- `ProcessInfo.state` field - lifecycle state snapshot in `ProcessInfo`
- `ProcessExitEvent.state` field - lifecycle state at exit (`Stopped` or `Crashed`), derived from exit status by the reaper
- `ProcessExitEvent.exit_status` field - the `ExitStatus` of the exited process
- `ProcessManager::state(id)` - returns the current `ProcessState` for a process
- `ExitedProcess` internal struct for the reaper loop (replaces tuple-based collection)
- Book page for `ProcessState` at `book/src/process/state.md`
- Integration tests for `ProcessState` transitions: `Stopped` after normal exit, `Crashed` after failure, state in `ProcessInfo`, state in reaper exit events
- Unit tests for `ProcessState`: `Display`, `is_alive()`, `is_terminated()`, `Default`, equality, `Clone`/`Copy`
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

- `DependencyRef::Label` variant now holds `Label` instead of `String`
- `DepGraph` internal type uses `HashMap<Label, ...>` instead of `HashMap<String, ...>`
- All dependency resolution functions use `&Label` instead of `&str`
- Crates renamed from `smearor-wrot-process` / `smearor-wrot-socket` to `process-manager` / `process-manager-socket`
- Repository URL changed from `smearor/smearor-wrot-process-manager` to `smearor/process-manager`
- `Process::is_running()` now delegates to `state().is_alive()` instead of directly calling `try_wait()`
- `ProcessManager::stop()` and `stop_many()` now set `ProcessState::Stopping` before sending signals
- `ProcessManager::stop_many()` now sets `ProcessState::Failed` when re-inserting a process after `force_kill()` failure
- `ProcessManager::restart()` and `restart_label()` now set `ProcessState::Restarting` before stopping the old process
- `ProcessManager::restart()` now preserves `ProcessId` - updates the process in-place instead of removing and re-inserting
- `ProcessManager::stop()` during `Restarting` state cancels backoff silently (no signal, no event)
- `ProcessManager::send_signal()` returns `ProcessInRestartingState` error when process is in `Restarting` state
- `ProcessExitEvent.state` can now be `Failed` (in addition to `Stopped` and `Crashed`) when rate limit is exceeded or spawn fails during restart
- Reaper loop now handles automatic restart with exponential backoff, rate limiting, and stable uptime resets
- `ExitedProcess` struct updated to carry `restart_config` for in-place restart
- `ProcessManager::get_info()` now calls `state()` on the process before snapshotting to ensure the state is up-to-date
- `Process::state()` short-circuits for `Stopping`, `Restarting`, and terminated states without calling `try_wait()`
- Reaper loop now derives `ProcessState::Stopped` or `ProcessState::Crashed` from `exit_status.success()` and includes it in `ProcessExitEvent`
- `ProcessInfo` and `ProcessConfig` now derive `PartialEq` and `Eq`
- Reader threads are now joined early when the process exits (detected via `try_wait()` in reaper or during `stop()`), not only during `stop()`
- Book documentation updated: `process.md`, `exit_event.md`, `manager.md`, `reaper.md`, `index.md`, `architecture.md`, `introduction.md`, and all example pages now reference `ProcessState`
- Examples updated: `basic_spawn`, `reaper`, `send_signal`, `wayland_socket` now demonstrate `state()` and `event.state`
- Em dashes (`—`) replaced with hyphens (`-`) across book and changelog for consistency
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

### Removed

- `impl From<Label> for String` - removed to preserve newtype encapsulation; use `Label::as_str()`, `Label::into_inner()`, or `Display` instead

### Fixed

- Potential endless blocking when a process is in an unkillable state (e.g. state D / uninterruptible sleep) - `stop()` now handles this gracefully
- `Process::send_signal()` now uses `nix::Error::from_raw_os_error` consistently (was `Error::other` in some paths, losing the OS error code)
- Removed leftover no-ops in process management code
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
