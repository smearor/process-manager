use crate::config::DependencyRef;
use crate::config::RestartPolicy;
use crate::config::RestartTrigger;
use crate::config::StdioConfig;
use crate::config::SupervisorStrategy;
use crate::signal::KillSignal;
use process_manager_socket::Socket;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use typed_builder::TypedBuilder;

/// Configuration for a child process.
///
/// Covers all three consumers' needs: `smearor-wrot-wrapper`,
/// `smearor-swipe-launcher/services/terminal_command`, and
/// `smearor-swipe-launcher/services/app_launcher`.
///
/// Built using `TypedBuilder` for ergonomic construction with defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TypedBuilder)]
#[must_use]
pub struct ProcessConfig {
    /// The program name or absolute path to execute.
    #[builder(setter(into))]
    pub command: String,

    /// Arguments passed to the command.
    #[builder(default)]
    pub args: Vec<String>,

    /// Additional environment variables merged into the spawned process environment.
    #[builder(default)]
    pub env: HashMap<String, String>,

    /// Optional working directory for the spawned process.
    #[builder(default)]
    pub working_dir: Option<PathBuf>,

    /// Whether to run the command via `sh -c`.
    #[builder(default)]
    pub shell: bool,

    /// Start the process in a new session via `setsid()` so it detaches from
    /// the controlling terminal. The process is still tracked by the
    /// `ProcessManager` (stored in the `DashMap` with its `Child` handle).
    /// The reaper thread uses non-blocking `try_wait()` to detect exit and
    /// prevent zombies - no thread-per-process blocking `wait()` needed.
    #[builder(default)]
    pub forked: bool,

    /// Whether to terminate the process when the `ProcessManager` is dropped.
    #[builder(default)]
    pub terminate_on_exit: bool,

    /// Signal used for termination.
    #[builder(default)]
    pub kill_signal: KillSignal,

    /// Grace period in milliseconds before escalating to `SIGKILL`.
    #[builder(default = 2000)]
    pub terminate_timeout_ms: u64,

    /// Whether to restart the process if it exits unexpectedly.
    /// The `ProcessManager` emits an exit event; the consumer decides whether to restart.
    #[builder(default)]
    pub restart_on_exit: bool,

    /// Which exit states trigger an automatic restart.
    /// Only applies when `restart_on_exit` is `true`.
    /// Defaults to `CrashOnly` - restart only on `Crashed`, not `Stopped`.
    #[builder(default)]
    pub restart_trigger: RestartTrigger,

    /// Restart policy: `Immediate` or `Backoff(BackoffConfig)`.
    /// Only applies when `restart_on_exit` is `true`.
    /// Defaults to `Immediate`.
    #[builder(default)]
    pub restart_policy: RestartPolicy,

    /// Standard input configuration.
    #[builder(default = StdioConfig::Inherit)]
    pub stdin: StdioConfig,

    /// Standard output configuration.
    #[builder(default = StdioConfig::Piped)]
    pub stdout: StdioConfig,

    /// Standard error configuration.
    #[builder(default = StdioConfig::Piped)]
    pub stderr: StdioConfig,

    /// Optional Wayland socket. When set, `WAYLAND_DISPLAY` is set in the child environment.
    #[builder(default)]
    pub socket: Option<Socket>,

    /// Supervisor strategy for this process's group.
    ///
    /// Determines which processes are restarted when one process in the
    /// group crashes. Only applies when `restart_on_exit` is `true`.
    /// Defaults to `OneForOne`.
    #[builder(default)]
    pub supervisor_strategy: SupervisorStrategy,

    /// Processes that must be `Running` before this process starts.
    ///
    /// The `start()` call will wait (up to `dependency_timeout_ms`) for all
    /// dependencies to reach `Running` state. If a dependency is not
    /// running within the timeout, `start()` returns an error.
    ///
    /// Dependencies are resolved by `DependencyRef` (label or `ProcessId`).
    /// Empty by default - no dependencies, start immediately.
    #[builder(default)]
    pub depends_on: Vec<DependencyRef>,

    /// Maximum time in milliseconds to wait for dependencies to become `Running`.
    ///
    /// Defaults to 30 seconds. If a dependency does not reach `Running`
    /// within this timeout, `start()` fails with
    /// `ProcessManagerError::DependencyTimeout`.
    #[builder(default = 30_000)]
    pub dependency_timeout_ms: u64,

    /// Whether stopping this process should also stop processes that depend on it.
    ///
    /// Defaults to `false` - stopping a dependency does not affect dependents.
    /// Set to `true` for strict dependency chains where dependents cannot
    /// survive without this process.
    #[builder(default)]
    pub cascade_stop: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_config_builder_defaults() {
        let config = ProcessConfig::builder().command("echo".to_string()).build();
        assert_eq!(config.command, "echo");
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert!(config.working_dir.is_none());
        assert!(!config.shell);
        assert!(!config.forked);
        assert!(!config.terminate_on_exit);
        assert_eq!(config.kill_signal, KillSignal::Sigterm);
        assert_eq!(config.terminate_timeout_ms, 2000);
        assert!(!config.restart_on_exit);
        assert_eq!(config.stdin, StdioConfig::Inherit);
        assert_eq!(config.stdout, StdioConfig::Piped);
        assert_eq!(config.stderr, StdioConfig::Piped);
        assert!(config.socket.is_none());
    }

    #[test]
    fn test_process_config_builder_with_args() {
        let config = ProcessConfig::builder()
            .command("sleep".to_string())
            .args(vec!["10".to_string()])
            .forked(true)
            .terminate_on_exit(true)
            .kill_signal(KillSignal::Sigkill)
            .terminate_timeout_ms(5000)
            .restart_on_exit(true)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Null)
            .build();
        assert_eq!(config.command, "sleep");
        assert_eq!(config.args, vec!["10"]);
        assert!(config.forked);
        assert!(config.terminate_on_exit);
        assert_eq!(config.kill_signal, KillSignal::Sigkill);
        assert_eq!(config.terminate_timeout_ms, 5000);
        assert!(config.restart_on_exit);
        assert_eq!(config.stdout, StdioConfig::Null);
        assert_eq!(config.stderr, StdioConfig::Null);
    }

    #[test]
    fn test_process_config_builder_with_env() {
        let mut env = HashMap::new();
        env.insert("GDK_BACKEND".to_string(), "wayland".to_string());
        env.insert("WAYLAND_DEBUG".to_string(), "1".to_string());
        let config = ProcessConfig::builder().command("test".to_string()).env(env).build();
        assert_eq!(config.env.get("GDK_BACKEND"), Some(&"wayland".to_string()));
        assert_eq!(config.env.get("WAYLAND_DEBUG"), Some(&"1".to_string()));
    }

    #[test]
    fn test_process_config_serde_roundtrip() {
        let mut env = HashMap::new();
        env.insert("GDK_BACKEND".to_string(), "wayland".to_string());
        let config = ProcessConfig::builder()
            .command("test".to_string())
            .args(vec!["--foo".to_string(), "bar".to_string()])
            .env(env)
            .working_dir(Some(PathBuf::from("/tmp")))
            .shell(true)
            .forked(true)
            .terminate_on_exit(true)
            .kill_signal(KillSignal::Sigkill)
            .terminate_timeout_ms(5000)
            .restart_on_exit(true)
            .stdin(StdioConfig::Null)
            .stdout(StdioConfig::Null)
            .stderr(StdioConfig::Inherit)
            .socket(Some(Socket::from(PathBuf::from("/tmp/wayland-0"))))
            .build();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ProcessConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.command, deserialized.command);
        assert_eq!(config.args, deserialized.args);
        assert_eq!(config.env, deserialized.env);
        assert_eq!(config.working_dir, deserialized.working_dir);
        assert_eq!(config.shell, deserialized.shell);
        assert_eq!(config.forked, deserialized.forked);
        assert_eq!(config.terminate_on_exit, deserialized.terminate_on_exit);
        assert_eq!(config.kill_signal, deserialized.kill_signal);
        assert_eq!(config.terminate_timeout_ms, deserialized.terminate_timeout_ms);
        assert_eq!(config.restart_on_exit, deserialized.restart_on_exit);
        assert_eq!(config.stdin, deserialized.stdin);
        assert_eq!(config.stdout, deserialized.stdout);
        assert_eq!(config.stderr, deserialized.stderr);
        assert_eq!(config.socket, deserialized.socket);
    }
}
