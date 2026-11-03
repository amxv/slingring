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
fn registered_command() -> Result<Option<String>> {
    let output = Command::new("reg.exe")
        .args(["query", RUN_KEY, "/v", LABEL])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let rows = String::from_utf8(output.stdout)?;
    Ok(rows.lines().find_map(|line| {
        line.split_once("REG_SZ")
            .map(|(_, value)| value.trim().to_owned())
    }))
}
pub fn installed() -> bool {
    registered_command()
        .ok()
        .flatten()
        .zip(run_command().ok())
        .is_some_and(|(registered, expected)| registered.eq_ignore_ascii_case(&expected))
}
pub fn loaded() -> bool {
    installed()
}
pub fn install(root: &Path) -> Result<()> {
    ensure!(
        std::env::var_os("SLINGRING_HOME").is_none(),
        "Windows login startup uses default APPDATA state; custom roots can run on demand only"
    );
    config::private_dir(root)?;
    let program = run_command()?;
    if let Some(existing) = registered_command()? {
        ensure!(
            existing.eq_ignore_ascii_case(&program),
            "a different Slingring startup registration exists; refusing to overwrite it"
        );
    }
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
    if let Some(existing) = registered_command()? {
        ensure!(
            existing.eq_ignore_ascii_case(&run_command()?),
            "startup registration belongs to another executable; refusing to delete it"
        );
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
