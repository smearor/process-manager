use serde::Deserialize;
use serde::Serialize;

/// Unique identifier for a managed process.
///
/// Assigned by `ProcessManager` using an `AtomicU64` counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[must_use]
pub struct ProcessId(u64);

impl ProcessId {
    /// Create a new `ProcessId` from a raw `u64`.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the raw `u64` value.
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for ProcessId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_id_new() {
        let id = ProcessId::new(42);
        assert_eq!(id.raw(), 42);
    }

    #[test]
    fn test_process_id_display() {
        let id = ProcessId::new(7);
        assert_eq!(format!("{}", id), "7");
    }

    #[test]
    fn test_process_id_equality() {
        let a = ProcessId::new(1);
        let b = ProcessId::new(1);
        let c = ProcessId::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
