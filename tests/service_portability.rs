//! Host-native IPC smoke. Intentionally never launches a browser or logs into
//! ChatGPT: exercises the daemon/CLI boundary on the current OS.
#![cfg(any(target_os = "linux", target_os = "windows"))]
use std::{path::PathBuf, process::Command};

struct StopOnDrop {
    executable: PathBuf,
    root: PathBuf,
}
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = Command::new(&self.executable)
            .args(["service", "stop"])
            .env("SLINGRING_HOME", &self.root)
            .output();
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
        let output = Command::new(&executable)
            .args(args)
            .env("SLINGRING_HOME", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} failed: stdout={} stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
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
