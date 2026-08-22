//! Wayland socket management for desktop launchers.
//!
//! `process-manager-socket` provides utilities for creating, registering, and
//! managing Wayland socket paths. It is designed to work alongside
//! [`process-manager`] to bind spawned processes to specific sockets.
//!
//! # Key types
//!
//! - [`Socket`] — newtype wrapping a `PathBuf` representing a Wayland socket path
//! - [`SocketBuilder`] — creates sockets with automatic unique name generation
//! - [`SocketManager`] — concurrent registry of named sockets (`DashMap`-backed)
//! - [`SocketBuilderError`] — errors during socket creation
//! - [`SocketManagerError`] — errors during registration or lookup
//!
//! # Usage
//!
//! ```no_run
//! use process_manager_socket::{SocketBuilder, SocketManager};
//!
//! let manager = SocketManager::new();
//!
//! // Build a socket with an auto-generated unique name
//! let socket = SocketBuilder::build(&None).unwrap();
//! manager.register("default", socket.clone()).unwrap();
//!
//! // Retrieve by name
//! let retrieved = manager.get("default").unwrap();
//! assert_eq!(retrieved, socket);
//! ```
//!
//! [`process-manager`]: https://docs.rs/process-manager

pub mod builder;
pub mod error;
pub mod manager;
pub mod socket;

pub use builder::SocketBuilder;
pub use error::SocketBuilderError;
pub use error::SocketManagerError;
pub use manager::SocketManager;
pub use socket::Socket;
