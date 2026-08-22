use process_manager_socket::Socket;
use process_manager_socket::SocketBuilder;
use process_manager_socket::SocketManager;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

#[test]
fn register_get_remove_socket() {
    let manager = SocketManager::new();
    let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));

    manager.register("display", socket.clone()).unwrap();
    assert_eq!(manager.len(), 1);

    let retrieved = manager.get("display").unwrap();
    assert_eq!(retrieved, socket);

    let _ = manager.remove("display").unwrap();
    assert_eq!(manager.len(), 0);
    assert!(manager.get("display").is_none());
}

#[test]
fn register_duplicate_fails() {
    let manager = SocketManager::new();
    let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));

    manager.register("display", socket.clone()).unwrap();
    let result = manager.register("display", socket);
    assert!(result.is_err());
}

#[test]
fn remove_nonexistent_fails() {
    let manager = SocketManager::new();
    let result = manager.remove("nonexistent");
    assert!(result.is_none());
}

#[test]
fn get_nonexistent_returns_none() {
    let manager = SocketManager::new();
    assert!(manager.get("nonexistent").is_none());
}

#[test]
fn names_returns_all_registered() {
    let manager = SocketManager::new();
    manager.register("a", Socket::from(PathBuf::from("/tmp/a"))).unwrap();
    manager.register("b", Socket::from(PathBuf::from("/tmp/b"))).unwrap();

    let mut names = manager.names();
    names.sort();
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn sockets_returns_all_values() {
    let manager = SocketManager::new();
    let socket_a = Socket::from(PathBuf::from("/tmp/a"));
    let socket_b = Socket::from(PathBuf::from("/tmp/b"));
    manager.register("a", socket_a.clone()).unwrap();
    manager.register("b", socket_b.clone()).unwrap();

    let sockets = manager.sockets();
    assert_eq!(sockets.len(), 2);
    assert!(sockets.contains(&socket_a));
    assert!(sockets.contains(&socket_b));
}

#[test]
fn socket_display_formats_path() {
    let socket = Socket::from(PathBuf::from("/tmp/wayland-1"));
    assert_eq!(format!("{}", socket), "/tmp/wayland-1");
}

#[test]
fn socket_deref_to_path() {
    let socket = Socket::from(PathBuf::from("/tmp/wayland-0"));
    let path = &*socket;
    assert_eq!(path, std::path::Path::new("/tmp/wayland-0"));
}

#[test]
fn socket_builder_generates_unique_name() {
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        // SAFETY: tests run single-threaded
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/tmp") };
    }

    let socket = SocketBuilder::build(&None).unwrap();
    assert!(socket.path().to_string_lossy().contains("smearor-wrot"));
}

#[test]
fn socket_builder_with_explicit_name() {
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        // SAFETY: tests run single-threaded
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/tmp") };
    }

    let name = "integration-test-socket".to_string();
    let socket = SocketBuilder::build(&Some(name)).unwrap();
    assert!(socket.path().to_string_lossy().ends_with("integration-test-socket"));
}

#[test]
fn register_concurrent_same_name_only_one_succeeds() {
    let manager = Arc::new(SocketManager::new());
    let barrier = Arc::new(Barrier::new(8));
    let socket = Socket::from(PathBuf::from("/tmp/concurrent-test"));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let manager = Arc::clone(&manager);
        let barrier = Arc::clone(&barrier);
        let socket = socket.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            manager.register("contested", socket)
        }));
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let successes = results.iter().filter(|r| r.is_ok()).count();
    let failures = results.iter().filter(|r| r.is_err()).count();

    assert_eq!(successes, 1, "exactly one register should succeed");
    assert_eq!(failures, 7, "exactly seven registers should fail");
    assert_eq!(manager.len(), 1);
}

#[test]
fn register_concurrent_different_names_all_succeed() {
    let manager = Arc::new(SocketManager::new());
    let barrier = Arc::new(Barrier::new(8));

    let mut handles = Vec::new();
    for i in 0..8 {
        let manager = Arc::clone(&manager);
        let barrier = Arc::clone(&barrier);
        let socket = Socket::from(PathBuf::from(format!("/tmp/concurrent-{i}")));
        handles.push(thread::spawn(move || {
            barrier.wait();
            manager.register(&format!("socket-{i}"), socket)
        }));
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let successes = results.iter().filter(|r| r.is_ok()).count();

    assert_eq!(successes, 8, "all registers should succeed");
    assert_eq!(manager.len(), 8);
}
