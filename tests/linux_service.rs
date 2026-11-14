#![cfg(target_os = "linux")]

use anyhow::{Result, ensure};
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn cli(home: &Path, root: &Path) -> Command {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slingring"));
    child
        .env_clear()
        .env("HOME", home)
        .env("SLINGRING_HOME", root)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("runtime"))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/no-systemd-bus", home.display()),
        )
        .env("PATH", "/usr/bin:/bin");
    child
}

fn run_json(command: &mut Command) -> Result<Value> {
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn alternate_state_service_lifecycle_is_private_and_does_not_register() -> Result<()> {
    let home = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let root = home.path().join("state");
    let status = run_json(cli(home.path(), &root).args(["service", "status"]))?;
    assert_eq!(status["running"], false);
    assert!(!root.exists());

    // Uses an isolated direct daemon. No configuration, relay, browser,
    // external websites, or real systemd manager are involved.
    let status = run_json(cli(home.path(), &root).args(["service", "start"]))?;
    let stopped = cli(home.path(), &root).args(["service", "stop"]).output()?;
    assert_eq!(status["running"], true);
    assert_eq!(status["active_tasks"], 0);
    assert_eq!(status["installed"], false);
    assert!(stopped.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&stopped.stdout)?["running"],
        false
    );
    assert!(!root.join("slingring.sock").exists());

    let login = run_json(cli(home.path(), &root).args(["login", "status"]))?;
    assert_eq!(login["menu_bar"]["state"], "unsupported");
    assert_eq!(login["service"]["state"], "unmanaged");
    let output = cli(home.path(), &root)
        .args(["login", "service", "on"])
        .output()?;
    assert!(!output.status.success());
    assert!(
        !home
            .path()
            .join("config/systemd/user/slingring.service")
            .exists()
    );
    Ok(())
}
