use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;

fn main() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["5".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("example", &config).expect("failed to start process");
    println!("Started process with ID {}, PID {}", id, manager.get_pid(id).unwrap());

    assert_eq!(manager.get_label(id), Some("example".to_string()));
    assert_eq!(manager.is_running(id), Some(true));

    manager.stop(id).expect("failed to stop process");
    println!("Process stopped, manager empty: {}", manager.is_empty());
}
