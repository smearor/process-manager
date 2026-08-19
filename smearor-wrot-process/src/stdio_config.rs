use std::process::Stdio;

/// Configuration for a child process's standard streams.
///
/// Replaces hardcoded `Stdio` choices with a serializable enum.
/// Each variant maps to a `std::process::Stdio` configuration when
/// spawning a child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StdioConfig {
    /// Inherit from the parent process.
    Inherit,
    /// Discard output (connect to `/dev/null`).
    #[default]
    Null,
    /// Pipe output for reading by the parent. When used with stdout/stderr,
    /// the `ProcessManager` spawns reader threads that log to `tracing`.
    Piped,
}

impl StdioConfig {
    /// Convert `StdioConfig` to `std::process::Stdio`.
    pub fn to_stdio(self) -> Stdio {
        match self {
            StdioConfig::Inherit => Stdio::inherit(),
            StdioConfig::Null => Stdio::null(),
            StdioConfig::Piped => Stdio::piped(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_config_default_is_null() {
        assert_eq!(StdioConfig::default(), StdioConfig::Null);
    }

    #[test]
    fn test_stdio_config_to_stdio_inherit() {
        let _ = StdioConfig::Inherit.to_stdio();
    }

    #[test]
    fn test_stdio_config_to_stdio_null() {
        let _ = StdioConfig::Null.to_stdio();
    }

    #[test]
    fn test_stdio_config_to_stdio_piped() {
        let stdio = StdioConfig::Piped.to_stdio();
        // Stdio::piped() values are not comparable via PartialEq in all Rust versions,
        // but we can verify it doesn't panic.
        let _ = stdio;
    }
}
