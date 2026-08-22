use smearor_wrot_process::ProcessConfig;
use smearor_wrot_process::ProcessManager;
use smearor_wrot_process::StdioConfig;
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    let (tx, rx) = mpsc::channel();
    let manager = ProcessManager::with_reaper(Duration::from_millis(50), tx)
        .expect("failed to create reaper manager");

    let config = ProcessConfig::builder()
        .command("true".to_string())
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("short-lived", &config).expect("failed to start process");
    println!("Started process with ID {} (reaper active)", id);

    let event = rx.recv_timeout(Duration::from_secs(5)).expect("no exit event received");
    println!("Reaper detected exit: id={}, label={}, pid={}", event.id, event.label, event.pid);
    assert_eq!(event.id, id);

    manager.stop_all();
    println!("All processes stopped");
}
