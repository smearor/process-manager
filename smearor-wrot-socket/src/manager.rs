use crate::SocketBuilder;
use crate::error::SocketManagerError;
use crate::socket::Socket;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;

/// Manages multiple Wayland sockets, keyed by a string name.
///
/// Uses `DashMap` internally so all methods take `&self`, allowing the
/// manager to be shared via `Arc<SocketManager>` across threads without
/// external locks.
#[derive(Debug)]
pub struct SocketManager {
    sockets: DashMap<String, Socket>,
}

impl SocketManager {
    /// Create a new empty `SocketManager`.
    pub fn new() -> Self {
        Self { sockets: DashMap::new() }
    }

    /// Register a socket by name.
    ///
    /// Returns an error if the name is already registered.
    pub fn register(&self, name: &str, socket: Socket) -> Result<(), SocketManagerError> {
        match self.sockets.entry(name.to_string()) {
            Entry::Occupied(_) => Err(SocketManagerError::AlreadyRegistered(name.to_string())),
            Entry::Vacant(entry) => {
                entry.insert(socket);
                Ok(())
            }
        }
    }

    /// Build and register a socket from an optional name hint.
    ///
    /// Uses `SocketBuilder::build` internally. Returns the registered `Socket`.
    pub fn create(&self, name: &str, socket_name_hint: &Option<String>) -> Result<Socket, SocketManagerError> {
        let socket = SocketBuilder::build(socket_name_hint).map_err(SocketManagerError::BuilderError)?;
        self.register(name, socket.clone())?;
        Ok(socket)
    }

    /// Retrieve a registered socket by name.
    pub fn get(&self, name: &str) -> Option<Socket> {
        self.sockets.get(name).map(|entry| entry.value().clone())
    }

    /// Remove a registered socket by name.
    ///
    /// Returns the removed socket if it existed.
    pub fn remove(&self, name: &str) -> Option<Socket> {
        self.sockets.remove(name).map(|(_, socket)| socket)
    }

    /// List all registered socket names.
    pub fn names(&self) -> Vec<String> {
        self.sockets.iter().map(|entry| entry.key().clone()).collect()
    }

    /// List all registered sockets.
    pub fn sockets(&self) -> Vec<Socket> {
        self.sockets.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Number of registered sockets.
    pub fn len(&self) -> usize {
        self.sockets.len()
    }

    /// Whether no sockets are registered.
    pub fn is_empty(&self) -> bool {
        self.sockets.is_empty()
    }
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_socket_manager_new_is_empty() {
        let manager = SocketManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_socket_manager_register_and_get() {
        let manager = SocketManager::new();
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        manager.register("default", socket.clone()).unwrap();
        assert_eq!(manager.len(), 1);
        assert!(!manager.is_empty());
        let retrieved = manager.get("default").unwrap();
        assert_eq!(retrieved.path(), socket.path());
    }

    #[test]
    fn test_socket_manager_register_duplicate() {
        let manager = SocketManager::new();
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        manager.register("default", socket.clone()).unwrap();
        let result = manager.register("default", socket);
        assert!(matches!(result, Err(SocketManagerError::AlreadyRegistered(_))));
    }

    #[test]
    fn test_socket_manager_remove() {
        let manager = SocketManager::new();
        let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
        manager.register("default", socket.clone()).unwrap();
        let removed = manager.remove("default").unwrap();
        assert_eq!(removed.path(), socket.path());
        assert!(manager.is_empty());
    }

    #[test]
    fn test_socket_manager_remove_nonexistent() {
        let manager = SocketManager::new();
        assert!(manager.remove("nonexistent").is_none());
    }

    #[test]
    fn test_socket_manager_names() {
        let manager = SocketManager::new();
        manager.register("a", Socket::from(PathBuf::from("/tmp/a"))).unwrap();
        manager.register("b", Socket::from(PathBuf::from("/tmp/b"))).unwrap();
        let mut names = manager.names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_socket_manager_sockets() {
        let manager = SocketManager::new();
        manager.register("a", Socket::from(PathBuf::from("/tmp/a"))).unwrap();
        manager.register("b", Socket::from(PathBuf::from("/tmp/b"))).unwrap();
        assert_eq!(manager.sockets().len(), 2);
    }

    #[test]
    fn test_socket_manager_get_nonexistent() {
        let manager = SocketManager::new();
        assert!(manager.get("nonexistent").is_none());
    }
}
