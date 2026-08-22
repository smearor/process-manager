use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;

fn main() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .forked(true)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("daemon", &config).expect("failed to start forked process");
    println!("Started forked process with ID {}", id);
    assert_eq!(manager.is_forked(id), Some(true));

    manager.stop(id).expect("failed to stop forked process");
    println!("Forked process stopped, manager empty: {}", manager.is_empty());
}
