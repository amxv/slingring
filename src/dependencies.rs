//! A private, version-pinned browser helper. Nothing runs between commands.
use crate::config;
use anyhow::{Context, Result, ensure};
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    process::Command,
};
#[cfg(unix)]
use std::{io::Write, process::Stdio};

pub const VERSION: &str = "0.41.1";
#[cfg(unix)]
const INSTALLER: &str = include_str!("../scripts/install-browser.sh");
#[cfg(windows)]
const WINDOWS_INSTALLER: &str = include_str!("../scripts/install-browser.ps1");

pub fn managed(root: &Path) -> PathBuf {
    #[cfg(windows)]
    return root.join("browser/0.41.1/agent-browser-browse-native.exe");
    #[cfg(unix)]
    root.join("browser/agent-browser-browse")
}

/// Explicit absolute overrides remain authoritative; otherwise prefer the
/// managed helper so a new installation never needs a second CLI in PATH.
pub fn resolve(root: &Path, configured: &str) -> Result<PathBuf> {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        ensure!(
            path.is_file(),
            "configured browser helper is missing; run slingring browser install and slingring setup"
        );
        return Ok(path);
    }
    let own = managed(root);
    if configured == "agent-browser-browse" && own.is_file() {
        return Ok(own);
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(configured))
        .find(|path| path.is_file())
        .context("browser helper missing; run slingring browser install")
}

pub fn install(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "Slingring home must be absolute");
    config::private_dir(root)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(root.join("browser-install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("another browser installation is in progress")?;
    #[cfg(unix)]
    {
        let mut child = Command::new("/bin/sh")
            .args(["-s", "--"])
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("could not start browser installer")?;
        child
            .stdin
            .take()
            .context("installer stdin missing")?
            .write_all(INSTALLER.as_bytes())?;
        ensure!(
            child.wait()?.success(),
            "browser helper installation failed; existing installation was preserved"
        );
    }
    #[cfg(windows)]
    {
        let script = root.join("browser-install.ps1");
        config::atomic(&script, WINDOWS_INSTALLER.as_bytes())?;
        let result = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script)
            .arg("-Root")
            .arg(root)
            .status();
        let _ = std::fs::remove_file(script);
        ensure!(
            result?.success(),
            "Windows browser helper installation failed"
        );
    }
    Ok(managed(root))
}
