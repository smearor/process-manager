use serde::Deserialize;
use serde::Serialize;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fmt::Formatter;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

/// A Wayland socket path, represented as a newtype around `PathBuf`.
///
/// Provides convenient trait implementations for dereferencing to `Path`,
/// displaying as a string, and converting from `PathBuf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub struct Socket(PathBuf);

impl Socket {
    /// Returns a reference to the socket path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Deref for Socket {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.0.as_path()
    }
}

impl AsRef<OsStr> for Socket {
    fn as_ref(&self) -> &OsStr {
        self.0.as_os_str()
    }
}

impl AsRef<str> for Socket {
    fn as_ref(&self) -> &str {
        self.0.as_os_str().to_str().unwrap_or("")
    }
}

impl Display for Socket {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl From<PathBuf> for Socket {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_from_pathbuf() {
        let path = PathBuf::from("/tmp/wayland-0");
        let socket = Socket::from(path);
        assert_eq!(socket.path(), Path::new("/tmp/wayland-0"));
    }

    #[test]
    fn test_socket_deref() {
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        let path: &Path = &*socket;
        assert_eq!(path, Path::new("/tmp/wayland-0"));
    }

    #[test]
    fn test_socket_as_ref_osstr() {
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        let os_str: &OsStr = socket.as_ref();
        assert_eq!(os_str, OsStr::new("/tmp/wayland-0"));
    }

    #[test]
    fn test_socket_as_ref_str() {
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        let s: &str = socket.as_ref();
        assert_eq!(s, "/tmp/wayland-0");
    }

    #[test]
    fn test_socket_display() {
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        assert_eq!(format!("{}", socket), "/tmp/wayland-0");
    }
}
