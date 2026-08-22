use nix::sys::signal::Signal as NixSignal;
use serde::Deserialize;
use serde::Serialize;

use crate::signal::KillSignal;

/// Unix signals that can be sent to a managed process.
///
/// Broader than [`crate::KillSignal`] (which is only `SIGTERM`/`SIGKILL`),
/// this enum covers common signals for general process control: reloading
/// configs, interrupting, pausing, resuming, user-defined signals, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[must_use]
pub enum Signal {
    /// `SIGHUP` — hang up, often used to reload configuration.
    Sighup,
    /// `SIGINT` — interrupt (Ctrl+C).
    Sigint,
    /// `SIGQUIT` — quit with core dump.
    Sigquit,
    /// `SIGTERM` — graceful termination request.
    Sigterm,
    /// `SIGKILL` — immediate forced termination, cannot be caught.
    Sigkill,
    /// `SIGUSR1` — user-defined signal 1.
    Sigusr1,
    /// `SIGUSR2` — user-defined signal 2.
    Sigusr2,
    /// `SIGWINCH` — window size change.
    Sigwinch,
    /// `SIGSTOP` — pause execution, cannot be caught.
    Sigstop,
    /// `SIGCONT` — resume execution after `SIGSTOP`.
    Sigcont,
    /// `SIGALRM` — timer alarm.
    Sigalrm,
}

impl Signal {
    /// Convert `Signal` to `nix::sys::signal::Signal`.
    pub fn to_nix_signal(self) -> NixSignal {
        match self {
            Signal::Sighup => NixSignal::SIGHUP,
            Signal::Sigint => NixSignal::SIGINT,
            Signal::Sigquit => NixSignal::SIGQUIT,
            Signal::Sigterm => NixSignal::SIGTERM,
            Signal::Sigkill => NixSignal::SIGKILL,
            Signal::Sigusr1 => NixSignal::SIGUSR1,
            Signal::Sigusr2 => NixSignal::SIGUSR2,
            Signal::Sigwinch => NixSignal::SIGWINCH,
            Signal::Sigstop => NixSignal::SIGSTOP,
            Signal::Sigcont => NixSignal::SIGCONT,
            Signal::Sigalrm => NixSignal::SIGALRM,
        }
    }
}

impl From<KillSignal> for Signal {
    fn from(kill: KillSignal) -> Self {
        match kill {
            KillSignal::Sigterm => Signal::Sigterm,
            KillSignal::Sigkill => Signal::Sigkill,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_to_nix_sighup() {
        assert_eq!(Signal::Sighup.to_nix_signal(), NixSignal::SIGHUP);
    }

    #[test]
    fn test_signal_to_nix_sigint() {
        assert_eq!(Signal::Sigint.to_nix_signal(), NixSignal::SIGINT);
    }

    #[test]
    fn test_signal_to_nix_sigterm() {
        assert_eq!(Signal::Sigterm.to_nix_signal(), NixSignal::SIGTERM);
    }

    #[test]
    fn test_signal_to_nix_sigkill() {
        assert_eq!(Signal::Sigkill.to_nix_signal(), NixSignal::SIGKILL);
    }

    #[test]
    fn test_signal_to_nix_sigusr1() {
        assert_eq!(Signal::Sigusr1.to_nix_signal(), NixSignal::SIGUSR1);
    }

    #[test]
    fn test_signal_to_nix_sigusr2() {
        assert_eq!(Signal::Sigusr2.to_nix_signal(), NixSignal::SIGUSR2);
    }

    #[test]
    fn test_signal_to_nix_sigwinch() {
        assert_eq!(Signal::Sigwinch.to_nix_signal(), NixSignal::SIGWINCH);
    }

    #[test]
    fn test_signal_to_nix_sigstop() {
        assert_eq!(Signal::Sigstop.to_nix_signal(), NixSignal::SIGSTOP);
    }

    #[test]
    fn test_signal_to_nix_sigcont() {
        assert_eq!(Signal::Sigcont.to_nix_signal(), NixSignal::SIGCONT);
    }

    #[test]
    fn test_signal_from_kill_signal_sigterm() {
        let signal: Signal = KillSignal::Sigterm.into();
        assert_eq!(signal, Signal::Sigterm);
    }

    #[test]
    fn test_signal_from_kill_signal_sigkill() {
        let signal: Signal = KillSignal::Sigkill.into();
        assert_eq!(signal, Signal::Sigkill);
    }

    #[test]
    fn test_signal_serde_roundtrip() {
        let json = serde_json::to_string(&Signal::Sigusr1).unwrap();
        assert_eq!(json, "\"SIGUSR1\"");
        let deserialized: Signal = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Signal::Sigusr1);
    }
}
