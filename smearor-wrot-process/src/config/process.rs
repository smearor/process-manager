use crate::config::StdioConfig;
use crate::signal::KillSignal;
use serde::Deserialize;
use serde::Serialize;
use smearor_wrot_socket::Socket;
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
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[must_use]
pub struct ProcessConfig {
    /// The program name or absolute path to execute.
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
    /// prevent zombies — no thread-per-process blocking `wait()` needed.
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
