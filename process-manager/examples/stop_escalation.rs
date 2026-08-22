use process_manager::KillSignal;
use process_manager::ProcessConfig;
use process_manager::ProcessManager;
use process_manager::StdioConfig;

fn main() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .kill_signal(KillSignal::Sigterm)
        .terminate_timeout_ms(100)
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("stubborn", &config).expect("failed to start process");
    println!("Started stubborn process with ID {}", id);

    manager.stop(id).expect("failed to stop process");
    println!("Process stopped (SIGTERM -> SIGKILL escalation after 100ms)");
    assert!(manager.is_empty());
}
