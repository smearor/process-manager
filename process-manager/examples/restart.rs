use process_manager::Label;
use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;

fn main() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["10".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("worker", &config).expect("failed to start process");
    println!("Started process with ID {}", id);

    let new_id = manager.restart(id).expect("failed to restart process");
    println!("Restarted: old ID={}, new ID={}", id, new_id);
    assert_ne!(id, new_id);
    assert_eq!(manager.get_label(new_id), Some(Label::new("worker")));

    manager.stop(new_id).expect("failed to stop process");
    println!("Process stopped, manager empty: {}", manager.is_empty());
}
