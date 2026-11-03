//! Per-user Windows startup registration; no administrator service installed.
use crate::{config, daemon};
use anyhow::{Result, ensure};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

pub const LABEL: &str = "Slingring";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn run_command() -> Result<String> {
    let exe = std::env::current_exe()?;
    let string = exe.to_string_lossy();
    ensure!(
        !string.contains(['\n', '\r', '"']),
        "invalid executable path"
    );
    Ok(format!("\"{string}\" daemon"))
}
pub fn installed() -> bool {
    Command::new("reg.exe")
        .args(["query", RUN_KEY, "/v", LABEL])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
pub fn loaded() -> bool {
    installed()
}
pub fn install(root: &Path) -> Result<()> {
    config::private_dir(root)?;
    let program = run_command()?;
    let status = Command::new("reg.exe")
        .args([
            "add", RUN_KEY, "/v", LABEL, "/t", "REG_SZ", "/d", &program, "/f",
        ])
        .status()?;
    ensure!(
        status.success(),
        "could not configure per-user startup registration"
    );
    Ok(())
}
pub fn uninstall() -> Result<()> {
    if installed() {
        let status = Command::new("reg.exe")
            .args(["delete", RUN_KEY, "/v", LABEL, "/f"])
            .status()?;
        ensure!(status.success(), "could not remove startup registration");
    }
    Ok(())
}
pub async fn start(root: &Path, ephemeral: bool) -> Result<()> {
    if daemon::running(root).await {
        return Ok(());
    }
    config::private_dir(root)?;
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("daemon");
    if ephemeral {
        command.arg("--ephemeral");
    }
    command
        .env("SLINGRING_HOME", root)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    for _ in 0..100 {
        if daemon::running(root).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("service did not start; run slingring daemon in foreground to diagnose")
}
pub async fn stop(root: &Path) -> Result<()> {
    if !daemon::running(root).await {
        return Ok(());
    }
    daemon::call(root, &daemon::Request::Stop).await?;
    for _ in 0..500 {
        if !daemon::running(root).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("service is still shutting down")
}
