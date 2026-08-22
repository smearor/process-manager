use smearor_wrot_process::ProcessConfig;
use smearor_wrot_process::ProcessManager;
use smearor_wrot_process::Signal;
use smearor_wrot_process::StdioConfig;

fn main() {
    let manager = ProcessManager::new();

    let config = ProcessConfig::builder()
        .command("sleep".to_string())
        .args(vec!["30".to_string()])
        .stdout(StdioConfig::Null)
        .stderr(StdioConfig::Null)
        .build();

    let id = manager.start("target", &config).expect("failed to start process");
    println!("Started process with ID {} (PID {})", id, manager.get_pid(id).unwrap());
    assert_eq!(manager.is_running(id), Some(true));

    // Send SIGWINCH — sleep ignores it, process keeps running
    manager.send_signal(id, Signal::Sigwinch).expect("failed to send SIGWINCH");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(manager.is_running(id), Some(true));
    println!("After SIGWINCH: still running");

    // Send SIGTERM — sleep terminates
    manager.send_signal(id, Signal::Sigterm).expect("failed to send SIGTERM");
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(manager.is_running(id), Some(false));
    println!("After SIGTERM: process exited");

    manager.stop(id).expect("failed to clean up");
    println!("Manager empty: {}", manager.is_empty());
}
