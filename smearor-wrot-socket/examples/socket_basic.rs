use smearor_wrot_socket::SocketBuilder;
use smearor_wrot_socket::SocketManager;

fn main() {
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        // SAFETY: single-threaded example
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/tmp") };
    }

    let manager = SocketManager::new();

    let socket = SocketBuilder::build(&None).expect("failed to build socket");
    println!("Generated socket: {}", socket);

    manager.register("default", socket.clone()).expect("failed to register socket");
    println!("Registered as 'default', manager len: {}", manager.len());

    let retrieved = manager.get("default").expect("failed to get socket");
    assert_eq!(retrieved, socket);
    println!("Retrieved socket matches: {}", retrieved);

    let explicit = SocketBuilder::build(&Some("my-wayland-0".to_string())).expect("failed to build named socket");
    manager.register("custom", explicit).expect("failed to register custom socket");

    println!("All socket names: {:?}", manager.names());
    println!("All sockets: {:?}", manager.sockets());

    let _ = manager.remove("default");
    println!("After removing 'default': {:?}", manager.names());
}
