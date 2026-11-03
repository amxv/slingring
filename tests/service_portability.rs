//! Host-native IPC smoke. Intentionally never launches a browser or logs into
//! ChatGPT: exercises the daemon/CLI boundary on the current OS.
#![cfg(any(target_os = "linux", target_os = "windows"))]
use std::{
    fs::File,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct StopOnDrop {
    executable: PathBuf,
    root: PathBuf,
}
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let Ok(mut child) = Command::new(&self.executable)
            .args(["service", "stop"])
            .env("SLINGRING_HOME", &self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let start = Instant::now();
        loop {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn daemon_starts_authenticates_status_and_cleans_up_endpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("isolated-state");
    std::fs::create_dir_all(&root).unwrap();
    // Local-only mode intentionally stops after two idle seconds. Keep this
    // isolated test daemon alive with a *loopback-only* unreachable relay,
    // so slow Windows process/ACL checks cannot race its idle shutdown.
    // No external network, browser, credentials, or ChatGPT task is involved.
    std::fs::write(
        root.join("config.json"),
        serde_json::to_vec(&serde_json::json!({
            "remote_enabled": true,
            "relay_url": "wss://127.0.0.1:9",
            "device_token": "00000000000000000000000000000000"
        }))
        .unwrap(),
    )
    .unwrap();
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_slingring"));
    let _stop = StopOnDrop {
        executable: executable.clone(),
        root: root.clone(),
    };
    let invoke = |args: &[&str]| -> String {
        let stdout = tmp.path().join("cli.out");
        let stderr = tmp.path().join("cli.err");
        let mut child = Command::new(&executable)
            .args(args)
            .env("SLINGRING_HOME", &root)
            .stdout(Stdio::from(File::create(&stdout).unwrap()))
            .stderr(Stdio::from(File::create(&stderr).unwrap()))
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(15) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("CLI invocation '{}' exceeded 15 seconds", args.join(" "));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let out = std::fs::read_to_string(&stdout).unwrap();
        let err = std::fs::read_to_string(&stderr).unwrap();
        assert!(
            status.success(),
            "{} failed: stdout={} stderr={}",
            args.join(" "),
            out,
            err
        );
        out
    };
    let before: serde_json::Value = serde_json::from_str(&invoke(&["status", "--json"])).unwrap();
    assert_eq!(before["running"], false);
    invoke(&["service", "start"]);
    let during: serde_json::Value = serde_json::from_str(&invoke(&["status", "--json"])).unwrap();
    assert_eq!(during["running"], true);
    assert_eq!(during["active_tasks"], 0);
    invoke(&["service", "stop"]);
    let after: serde_json::Value = serde_json::from_str(&invoke(&["status", "--json"])).unwrap();
    assert_eq!(after["running"], false);
    #[cfg(unix)]
    assert!(!root.join("slingring.sock").exists());
    #[cfg(windows)]
    assert!(!root.join("slingring.endpoint.json").exists());
}
