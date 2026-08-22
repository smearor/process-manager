use nix::sys::signal::Signal as NixSignal;
use serde::Deserialize;
use serde::Serialize;

/// Signal used for process termination.
///
/// Restricted to `SIGTERM` and `SIGKILL` - the only signals that make sense
/// for the `stop()` path. Use [`crate::Signal`] for general-purpose signaling
/// via `send_signal()`.
///
/// Made framework-agnostic - the conversion to `nix::sys::signal::Signal`
/// is internal. This allows the type to be used without importing `nix`
/// in consumer crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[must_use]
pub enum KillSignal {
    /// Send `SIGTERM` - allows the process to clean up gracefully.
    #[default]
    Sigterm,
    /// Send `SIGKILL` - immediate termination, no cleanup possible.
    Sigkill,
}

impl KillSignal {
    /// Convert `KillSignal` to `nix::sys::signal::Signal`.
    pub fn to_nix_signal(self) -> NixSignal {
        match self {
            KillSignal::Sigterm => NixSignal::SIGTERM,
            KillSignal::Sigkill => NixSignal::SIGKILL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_signal_default_is_sigterm() {
        assert_eq!(KillSignal::default(), KillSignal::Sigterm);
    }

    #[test]
    fn test_kill_signal_to_nix_signal_sigterm() {
        assert_eq!(KillSignal::Sigterm.to_nix_signal(), NixSignal::SIGTERM);
    }

    #[test]
    fn test_kill_signal_to_nix_signal_sigkill() {
        assert_eq!(KillSignal::Sigkill.to_nix_signal(), NixSignal::SIGKILL);
    }
}
