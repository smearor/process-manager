use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;
use process_manager_socket::SocketBuilder;
use process_manager_socket::SocketManager;

fn main() {
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        // SAFETY: single-threaded example
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/tmp") };
    }

    let socket_manager = SocketManager::new();
    let socket = SocketBuilder::build(&None).expect("failed to build socket");
    socket_manager.register("default", socket.clone()).expect("failed to register socket");
    println!("Created Wayland socket: {}", socket);

    let process_manager = ProcessManager::new();
    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .socket(Some(socket.clone()))
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = process_manager.start("wayland-client", &config).expect("failed to start process");
    println!("Started process with ID {} bound to socket", id);

    let info = process_manager.get_info(id).expect("process not found");
    println!("Process info: pid={}, label={}, program={}, state={}", info.pid, info.label, info.program_name, info.state);
    assert_eq!(info.config.socket, Some(socket));

    process_manager.stop(id).expect("failed to stop process");
    println!("Process stopped");
}
