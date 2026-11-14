//! A private, version-pinned browser helper. Nothing runs between commands.
#[cfg(not(target_os = "linux"))]
use crate::config;
use anyhow::Result;
#[cfg(not(target_os = "linux"))]
use anyhow::{Context, ensure};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "linux"))]
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    process::{Command, Stdio},
};

#[cfg(target_os = "linux")]
pub const VERSION: &str = "system-chrome-stable";
#[cfg(not(target_os = "linux"))]
pub const VERSION: &str = "0.41.1";
#[cfg(not(target_os = "linux"))]
const INSTALLER: &str = include_str!("../scripts/install-browser.sh");

#[cfg(not(target_os = "linux"))]
pub fn managed(root: &Path) -> PathBuf {
    root.join("browser/agent-browser-browse")
}

/// Explicit absolute overrides remain authoritative; otherwise prefer the
/// managed helper so a new installation never needs a second CLI in PATH.
#[cfg(not(target_os = "linux"))]
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

#[cfg(not(target_os = "linux"))]
pub fn install(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "Slingring home must be absolute");
    config::private_dir(root)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("browser-install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("another browser installation is in progress")?;
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
    Ok(managed(root))
}

#[cfg(target_os = "linux")]
pub fn managed(_root: &Path) -> PathBuf {
    PathBuf::from(crate::browser_linux::CHROME)
}
#[cfg(target_os = "linux")]
pub fn install(_root: &Path) -> Result<PathBuf> {
    Ok(crate::browser_stock::chrome_executable()?)
}
