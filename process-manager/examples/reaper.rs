use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    let (tx, rx) = mpsc::channel();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx).expect("failed to create reaper manager");

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("short-lived", &config).expect("failed to start process");
    println!("Started process with ID {} (reaper active)", id);

    let event = rx.recv_timeout(Duration::from_secs(5)).expect("no exit event received");
    println!("Reaper detected exit: id={}, label={}, pid={}, state={}", event.id, event.label, event.pid, event.state);
    assert_eq!(event.id, id);

    manager.stop_all();
    println!("All processes stopped");
}
