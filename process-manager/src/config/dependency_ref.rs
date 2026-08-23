use crate::config::Label;
use crate::process::ProcessId;
use serde::Deserialize;
use serde::Serialize;

/// Reference to a dependency process, either by `ProcessId` or by label.
///
/// When using `Label`, the dependency is resolved at `start()` time by
/// looking up the first `Running` process with that label. Once resolved,
/// the `ProcessId` of the target is **bound** - the dependent process
/// tracks that specific `ProcessId`, not the label string. This means:
///
/// - If the bound dependency crashes and restarts (preserving its
///   `ProcessId`), the dependent continues to wait for that same
///   `ProcessId` to return to `Running`.
/// - If the bound dependency is stopped and removed (losing its
///   `ProcessId`), the dependent fails via fail-fast.
/// - A different process started later with the same label does **not**
///   satisfy the dependency - the binding is to the original `ProcessId`.
///
/// This binding eliminates ambiguity when multiple processes share the
/// same label. For typical setups with unique role labels (e.g.
/// `"compositor"`), there is only one `Running` process per label and
/// the binding is transparent.
///
/// When using `Id`, the dependency is a specific `ProcessId` that must
/// be `Running` before the dependent process can start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum DependencyRef {
    /// Dependency by label - resolved and bound to a `ProcessId` at
    /// `start()` time. The first `Running` process with this label is
    /// selected. The binding persists for the lifetime of the dependent
    /// process.
    Label(Label),

    /// Dependency by `ProcessId` - the process with this ID must be `Running`.
    Id(ProcessId),
}

impl DependencyRef {
    pub fn label<L: Into<Label>>(label: L) -> Self {
        Self::Label(label.into())
    }

    pub fn id<PID: Into<ProcessId>>(id: PID) -> Self {
        Self::Id(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_ref_label_serde_roundtrip() {
        let dep = DependencyRef::label("compositor");
        let json = serde_json::to_string(&dep).unwrap();
        let deserialized: DependencyRef = serde_json::from_str(&json).unwrap();
        assert_eq!(dep, deserialized);
    }

    #[test]
    fn test_dependency_ref_id_serde_roundtrip() {
        let dep = DependencyRef::Id(ProcessId::new(42));
        let json = serde_json::to_string(&dep).unwrap();
        let deserialized: DependencyRef = serde_json::from_str(&json).unwrap();
        assert_eq!(dep, deserialized);
    }

    #[test]
    fn test_dependency_ref_label_equality() {
        assert_eq!(DependencyRef::label("foo"), DependencyRef::label("foo"));
        assert_ne!(DependencyRef::label("foo"), DependencyRef::label("bar"));
    }

    #[test]
    fn test_dependency_ref_id_equality() {
        assert_eq!(DependencyRef::Id(ProcessId::new(1)), DependencyRef::Id(ProcessId::new(1)));
        assert_ne!(DependencyRef::Id(ProcessId::new(1)), DependencyRef::Id(ProcessId::new(2)));
    }
}
