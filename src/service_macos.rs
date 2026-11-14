use crate::{config, daemon};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

pub const LABEL: &str = "xyz.ashray.slingring";
// launchd can throttle a restart for up to ThrottleInterval (30 seconds),
// particularly when an upgrade has just replaced the signed executable.
// The old five-second startup check falsely reported failures while launchd
// was still preparing a healthy daemon.
const LAUNCHD_START_TIMEOUT: Duration = Duration::from_secs(40);
const DIRECT_START_TIMEOUT: Duration = Duration::from_secs(10);
const START_POLL: Duration = Duration::from_millis(100);
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
/// launchctl's disabled override is independent of whether a job is running.
/// A disabled job can continue running until logout, which is what the login
/// preference needs (unlike bootout, which would terminate active work).
fn parse_disabled(output: &str) -> Result<bool> {
    for line in output.lines() {
        if let Some((label, value)) = line.trim().split_once(" => ")
            && label.trim_matches('"') == LABEL
        {
            return match value.trim() {
                "disabled" => Ok(true),
                "enabled" => Ok(false),
                other => bail!("unexpected launchctl override for {LABEL}: {other}"),
            };
        }
    }
    // A missing override means launchd's default (enabled).
    Ok(false)
}
pub fn login_disabled() -> Result<bool> {
    let result = Command::new("/bin/launchctl")
        .args(["print-disabled", &domain()?])
        .output()
        .context("could not inspect launchd login overrides")?;
    ensure!(
        result.status.success(),
        "could not inspect launchd login overrides: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    parse_disabled(&String::from_utf8(result.stdout)?)
}
pub fn login_enable(root: &Path) -> Result<()> {
    // Re-enable first: launchctl refuses to bootstrap disabled services.
    launchctl(&["enable", &format!("{}/{LABEL}", domain()?)])?;
    if !installed() {
        install(root)?;
    }
    // An existing plist is enabled for *future logins*. Do not bootstrap it
    // here: another daemon may already own the socket, and the requested
    // preference must not start a second service or disturb running work.
    Ok(())
}
pub fn login_disable() -> Result<()> {
    // Do not bootout/uninstall: both actions can kill an active daemon/task.
    launchctl(&["disable", &format!("{}/{LABEL}", domain()?)])
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
/// A loaded system job is not necessarily ours: SLINGRING_HOME can point to a
/// separate instance. Never kickstart the user's regular LaunchAgent while
/// attempting to start a daemon for another state directory.
fn manages_root(root: &Path) -> Result<bool> {
    let p = path()?;
    let Ok(meta) = fs::symlink_metadata(&p) else {
        return Ok(false);
    };
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "refusing a nonregular Slingring LaunchAgent plist"
    );
    let result = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :EnvironmentVariables:SLINGRING_HOME")
        .arg(&p)
        .output()
        .context("could not inspect Slingring's LaunchAgent configuration")?;
    ensure!(
        result.status.success(),
        "could not read SLINGRING_HOME from the installed LaunchAgent"
    );
    Ok(Path::new(String::from_utf8(result.stdout)?.trim()) == root)
}

/// Report only allowlisted launchd status, never its entire environment, which
/// can contain private data. The error is useful if the job fails repeatedly.
fn launchd_diagnosis() -> String {
    let Ok(domain) = domain() else {
        return "launchd state unavailable".into();
    };
    let Ok(result) = Command::new("/bin/launchctl")
        .args(["print", &format!("{domain}/{LABEL}")])
        .output()
    else {
        return "launchd state unavailable".into();
    };
    if !result.status.success() {
        return "LaunchAgent not loaded".into();
    }
    let output = String::from_utf8_lossy(&result.stdout);
    if output.contains("last exit reason = OS_REASON_CODESIGNING") {
        return "macOS previously rejected the daemon's code signature".into();
    }
    for line in output.lines() {
        if let Some(state) = line.trim().strip_prefix("state = ")
            && matches!(state, "running" | "waiting" | "not running")
        {
            return format!("launchd state: {state}");
        }
    }
    "launchd state unavailable".into()
}

async fn await_ready(
    root: &Path,
    mut child: Option<&mut Child>,
    launched_by_launchd: bool,
) -> Result<()> {
    let timeout = if launched_by_launchd {
        LAUNCHD_START_TIMEOUT
    } else {
        DIRECT_START_TIMEOUT
    };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if daemon::running(root).await {
            return Ok(());
        }
        if let Some(process) = child.as_mut()
            && let Some(status) = process.try_wait()?
        {
            bail!(
                "Slingring daemon exited before becoming ready ({status}); run slingring daemon in the foreground to diagnose"
            );
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(START_POLL).await;
    }
    if launched_by_launchd {
        bail!(
            "Slingring service did not become ready after {} seconds ({}); run slingring daemon in the foreground to diagnose",
            timeout.as_secs(),
            launchd_diagnosis()
        );
    }
    bail!(
        "Slingring service did not become ready after {} seconds; run slingring daemon in the foreground to diagnose",
        timeout.as_secs()
    )
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
    let launchd = !ephemeral && manages_root(root)? && loaded() && !login_disabled()?;
    let mut child = None;
    if launchd {
        launchctl(&["kickstart", &format!("{}/{LABEL}", domain()?)])?;
    } else {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("daemon");
        if ephemeral {
            command.arg("--ephemeral");
        }
        child = Some(
            command
                .env("SLINGRING_HOME", root)
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
    }
    await_ready(root, child.as_mut(), launchd).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchctl_overrides_are_interpreted_without_guessing() {
        assert!(!parse_disabled("disabled services = {\n}\n").unwrap());
        assert!(!parse_disabled("\"xyz.ashray.slingring\" => enabled\n").unwrap());
        assert!(parse_disabled("\"xyz.ashray.slingring\" => disabled\n").unwrap());
        assert!(parse_disabled("\"xyz.ashray.slingring\" => ???").is_err());
        assert!(!parse_disabled("\"xyz.ashray.slingring.bak\" => disabled").unwrap());
    }

    #[test]
    fn launchd_timeout_covers_throttle_interval() {
        assert!(LAUNCHD_START_TIMEOUT > Duration::from_secs(30));
        assert!(DIRECT_START_TIMEOUT < LAUNCHD_START_TIMEOUT);
    }
}
