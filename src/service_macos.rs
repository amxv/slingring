use crate::{config, daemon};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub const LABEL: &str = "xyz.ashray.slingring";
fn domain() -> Result<String> {
    let id = Command::new("/usr/bin/id").arg("-u").output()?;
    ensure!(id.status.success(), "could not determine user ID");
    Ok(format!("gui/{}", String::from_utf8(id.stdout)?.trim()))
}
pub fn path() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME missing")?)
            .join(format!("Library/LaunchAgents/{LABEL}.plist")),
    )
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn plist(exe: &Path, root: &Path, search_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>daemon</string></array>
<key>EnvironmentVariables</key><dict><key>SLINGRING_HOME</key><string>{}</string><key>PATH</key><string>{}</string></dict>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>30</integer>
<key>ExitTimeOut</key><integer>25</integer>
<key>ProcessType</key><string>Background</string>
<key>StandardOutPath</key><string>/dev/null</string>
<key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(&exe.to_string_lossy()),
        xml(&root.to_string_lossy()),
        xml(search_path),
        xml(&root.join("service.log").to_string_lossy())
    )
}
fn launchctl(args: &[&str]) -> Result<()> {
    let result = Command::new("/bin/launchctl").args(args).output()?;
    ensure!(
        result.status.success(),
        "launchctl failed: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(())
}
pub fn installed() -> bool {
    path().is_ok_and(|p| p.exists())
}
pub fn loaded() -> bool {
    domain().is_ok_and(|d| {
        Command::new("/bin/launchctl")
            .args(["print", &format!("{d}/{LABEL}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}
pub fn install(root: &Path) -> Result<()> {
    config::private_dir(root)?;
    let log = root.join("service.log");
    if !log.exists() || log.metadata()?.len() > 1024 * 1024 {
        config::atomic(&log, b"")?;
    }
    let p = path()?;
    fs::create_dir_all(p.parent().context("missing LaunchAgents directory")?)?;
    let search_path =
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    let text = plist(&std::env::current_exe()?, root, &search_path);
    // Write only the owned plist, without changing permissions of LaunchAgents.
    let tmp = root.join("launch-agent.plist");
    config::atomic(&tmp, text.as_bytes())?;
    ensure!(
        !fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()),
        "plist cannot be a symlink"
    );
    if loaded() {
        launchctl(&["bootout", &format!("{}/{LABEL}", domain()?)])?;
    }
    fs::copy(&tmp, &p)?;
    fs::remove_file(tmp)?;
    launchctl(&["bootstrap", &domain()?, &p.to_string_lossy()])
}
pub fn uninstall() -> Result<()> {
    if loaded() {
        launchctl(&["bootout", &format!("{}/{LABEL}", domain()?)])?;
    }
    let p = path()?;
    if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}
pub async fn start(root: &Path, ephemeral: bool) -> Result<()> {
    if daemon::running(root).await {
        return Ok(());
    }
    config::private_dir(root)?;
    if !ephemeral && loaded() {
        launchctl(&["kickstart", &format!("{}/{LABEL}", domain()?)])?;
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
    anyhow::bail!("service did not start; run slingring daemon in the foreground to diagnose")
}
pub async fn stop(root: &Path) -> Result<()> {
    if !daemon::running(root).await {
        return Ok(());
    }
    daemon::call(root, &daemon::Request::Stop).await?;
    for _ in 0..500 {
        if !root.join("slingring.sock").exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("service is still shutting down; do not kill unrelated browser processes")
}
