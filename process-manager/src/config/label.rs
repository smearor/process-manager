use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// A label for grouping processes and referencing dependencies.
///
/// Labels are used for grouped operations (`stop_label`, `restart_label`,
/// `pids_by_label`) and as dependency references (`DependencyRef::Label`).
/// Multiple processes can share the same label.
///
/// Serialized as a bare string in all formats via `#[serde(transparent)]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct Label(String);

impl Label {
    /// Create a new `Label` from any string-like input.
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    /// Return the label as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the `Label` and return the inner `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for Label {
    fn from(label: String) -> Self {
        Self(label)
    }
}

impl From<&str> for Label {
    fn from(label: &str) -> Self {
        Self(label.to_string())
    }
}

impl From<&String> for Label {
    fn from(label: &String) -> Self {
        Self(label.clone())
    }
}

impl AsRef<str> for Label {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<str> for Label {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Label {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for Label {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_new_from_string() {
        let label = Label::new("compositor".to_string());
        assert_eq!(label.as_str(), "compositor");
    }

    #[test]
    fn test_label_new_from_str() {
        let label = Label::new("compositor");
        assert_eq!(label.as_str(), "compositor");
    }

    #[test]
    fn test_label_from_string() {
        let label: Label = "compositor".to_string().into();
        assert_eq!(label.as_str(), "compositor");
    }

    #[test]
    fn test_label_from_str() {
        let label: Label = "compositor".into();
        assert_eq!(label.as_str(), "compositor");
    }

    #[test]
    fn test_label_from_ref_string() {
        let s = "compositor".to_string();
        let label: Label = (&s).into();
        assert_eq!(label.as_str(), "compositor");
    }

    #[test]
    fn test_label_as_ref() {
        let label = Label::new("compositor");
        let s: &str = label.as_ref();
        assert_eq!(s, "compositor");
    }

    #[test]
    fn test_label_display() {
        let label = Label::new("compositor");
        assert_eq!(format!("{}", label), "compositor");
    }

    #[test]
    fn test_label_equality() {
        let a = Label::new("foo");
        let b = Label::new("foo");
        let c = Label::new("bar");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_label_equality_with_str() {
        let label = Label::new("foo");
        assert_eq!(label, "foo");
        assert_ne!(label, "bar");
    }

    #[test]
    fn test_label_equality_with_string() {
        let label = Label::new("foo");
        assert_eq!(label, "foo".to_string());
        assert_ne!(label, "bar".to_string());
    }

    #[test]
    fn test_label_serde_roundtrip() {
        let label = Label::new("compositor");
        let json = serde_json::to_string(&label).unwrap();
        assert_eq!(json, "\"compositor\"");
        let deserialized: Label = serde_json::from_str(&json).unwrap();
        assert_eq!(label, deserialized);
    }

    #[test]
    fn test_label_serde_transparent() {
        let json = "\"my-label\"";
        let label: Label = serde_json::from_str(json).unwrap();
        assert_eq!(label.as_str(), "my-label");
        let serialized = serde_json::to_string(&label).unwrap();
        assert_eq!(serialized, json);
    }

    #[test]
    fn test_label_into_label() {
        let label = Label::new("compositor");
        let label2: Label = label.into();
        assert_eq!(label2.as_str(), "compositor");
    }

    #[test]
    fn test_label_ordering() {
        let mut labels = vec![Label::new("c"), Label::new("a"), Label::new("b")];
        labels.sort();
        assert_eq!(labels[0].as_str(), "a");
        assert_eq!(labels[1].as_str(), "b");
        assert_eq!(labels[2].as_str(), "c");
    }

    #[test]
    fn test_label_into_inner() {
        let label = Label::new("compositor");
        assert_eq!(label.into_inner(), "compositor");
    }
}
