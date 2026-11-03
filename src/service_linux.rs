//! Optional systemd --user unit plus an ephemeral, event-driven CLI service.
use crate::{config, daemon};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub const LABEL: &str = "slingring";
fn systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "systemctl --user {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
pub fn path() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME missing")?);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Ok(config.join("systemd/user/slingring.service"))
}
pub fn installed() -> bool {
    path().is_ok_and(|path| path.is_file())
}
pub fn loaded() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "slingring.service"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
fn systemd_quote(value: &str) -> Result<String> {
    ensure!(
        !value.contains(['\n', '\r', '\0']) && !value.contains('%'),
        "service paths cannot contain newlines or systemd expansion tokens"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
pub fn install(root: &Path) -> Result<()> {
    config::private_dir(root)?;
    let executable = std::env::current_exe()?;
    let unit = format!(
        "[Unit]\nDescription=Slingring user agent\n\n[Service]\nType=simple\nExecStart={} daemon\nEnvironment=SLINGRING_HOME={}\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(&executable.to_string_lossy())?,
        systemd_quote(&root.to_string_lossy())?,
    );
    let target = path()?;
    fs::create_dir_all(target.parent().context("unit has no parent")?)?;
    ensure!(
        !fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()),
        "systemd unit cannot be a symlink"
    );
    config::atomic(&target, unit.as_bytes())?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "slingring.service"])?;
    Ok(())
}
pub fn uninstall() -> Result<()> {
    if installed() {
        systemctl(&["disable", "--now", "slingring.service"])?;
        fs::remove_file(path()?)?;
        systemctl(&["daemon-reload"])?;
    }
    Ok(())
}
pub async fn start(root: &Path, ephemeral: bool) -> Result<()> {
    if daemon::running(root).await {
        return Ok(());
    }
    config::private_dir(root)?;
    if !ephemeral && installed() {
        systemctl(&["start", "slingring.service"])?;
    } else {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("daemon");
        if ephemeral {
            command.arg("--ephemeral");
        }
        command
            .env("SLINGRING_HOME", root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
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
