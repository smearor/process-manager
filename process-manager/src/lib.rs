//! Process management for Wayland compositors and desktop launchers.
//!
//! `process-manager` provides a [`ProcessManager`] that spawns, tracks,
//! and terminates child processes with support for:
//!
//! - **Labels** — group processes and operate on them collectively ([`stop_label`], [`restart_label`])
//! - **Forked mode** — detach processes via `setsid()` while keeping them tracked
//! - **Reaper thread** — detect process exits without a thread-per-process blocking `wait()`
//! - **Graceful shutdown** — `SIGTERM` → `SIGKILL` escalation with configurable timeout
//! - **Wayland sockets** — automatically set `WAYLAND_DISPLAY` when a [`Socket`] is configured
//! - **Terminate-on-exit** — kill processes when the manager is dropped
//!
//! # Key types
//!
//! - [`ProcessManager`] — central handle for spawning, stopping, and inspecting processes
//! - [`ProcessConfig`] — builder-based configuration for each spawned process
//! - [`ProcessId`] — opaque identifier assigned by the manager
//! - [`ProcessInfo`] — lightweight snapshot of a running process (no `Child` handle)
//! - [`ProcessExitEvent`] — emitted by the reaper when a process exits
//! - [`KillSignal`] — `SIGTERM` or `SIGKILL` (used for termination config)
//! - [`Signal`] — broader signal enum for general process control (`SIGHUP`, `SIGUSR1`, `SIGSTOP`, etc.)
//! - [`StdioConfig`] — `Inherit`, `Null`, or `Piped` for stdin/stdout/stderr
//!
//! # Usage
//!
//! ```no_run
//! use process_manager::{ProcessConfig, ProcessManager, StdioConfig};
//!
//! let manager = ProcessManager::new();
//!
//! let config = ProcessConfig::builder()
//!     .command("sleep".to_string())
//!     .args(vec!["10".to_string()])
//!     .stdout(StdioConfig::Null)
//!     .stderr(StdioConfig::Null)
//!     .build();
//!
//! let id = manager.start("worker", &config).unwrap();
//! assert_eq!(manager.is_running(id), Some(true));
//!
//! manager.stop(id).unwrap();
//! assert!(manager.is_empty());
//! ```
//!
//! [`stop_label`]: ProcessManager::stop_label
//! [`restart_label`]: ProcessManager::restart_label
//! [`Socket`]: process_manager_socket::Socket

pub mod config;
pub mod manager;
pub mod process;
pub mod reaper;
pub mod signal;

pub use config::ProcessConfig;
pub use config::ProcessConfigError;
pub use config::StdioConfig;
pub use manager::ProcessManager;
pub use manager::ProcessManagerError;
pub use manager::StopManyError;
pub use process::Process;
pub use process::ProcessExitEvent;
pub use process::ProcessId;
pub use process::ProcessInfo;
pub use process::ProcessState;
pub use signal::KillSignal;
pub use signal::Signal;
