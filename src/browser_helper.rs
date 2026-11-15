//! Linux `agent-browser-browse` backend. The helper launches system Chrome
//! Stable with its tested fingerprint defaults on a private, per-session copy
//! of the dedicated Slingring profile. The dedicated profile is only read, and
//! normal Chrome profile data is never used.
use crate::config::Config;
use anyhow::{Context, Result, ensure};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub(crate) const HELPER: &str = "agent-browser-browse";
const COPY: &str = "agent-browser-profile-slingring";
const MAX_DEPTH: usize = 16;
/// Disposable caches and per-installation state that a signed-in session never
/// needs. Singleton locks are skipped separately; the native host manifest is
/// registered afresh in the copy for every browser generation.
const SKIP: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GraphiteDawnCache",
    "GrShaderCache",
    "ShaderCache",
    "Crash Reports",
    "component_crx_cache",
    "extensions_crx_cache",
    "NativeMessagingHosts",
];

/// True when this state directory uses the helper rather than stock Chrome.
pub(crate) fn selected(config: &Config) -> bool {
    config.browser_binary != crate::browser_stock::PROVIDER
}

/// Resolve the helper to an absolute path once, at configuration time, so a
/// service with a minimal PATH runs exactly the binary the owner selected.
pub(crate) fn resolve(configured: &str) -> Result<PathBuf> {
    let path = PathBuf::from(configured);
    let found = if path.is_absolute() {
        path
    } else {
        ensure!(
            configured == HELPER,
            "Linux accepts stock-chrome-stable, agent-browser-browse, or an absolute helper path"
        );
        let mut directories: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        if let Some(home) = std::env::var_os("HOME") {
            directories.push(PathBuf::from(home).join(".local/bin"));
        }
        directories
            .into_iter()
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(HELPER))
            .find(|candidate| candidate.is_file())
            .context("agent-browser-browse was not found in PATH or ~/.local/bin")?
    };
    ensure!(
        found.is_file(),
        "configured browser helper is missing or not a file"
    );
    ensure!(
        found
            .file_name()
            .is_some_and(|name| name != "agent-browser"),
        "upstream agent-browser is not an allowed fallback"
    );
    Ok(found)
}

/// The fixed copy location inside this daemon generation's private runtime
/// directory. Its name matches the isolated-profile convention checked by
/// native registration and browser ownership validation.
pub(crate) fn copy_path(root: &Path) -> Result<PathBuf> {
    Ok(crate::browser_linux::runtime_root(root)?.join(COPY))
}

/// Create the private profile copy if this generation has none yet. A live
/// helper session keeps using its existing copy until `remove_copy`.
pub(crate) fn ensure_copy(root: &Path) -> Result<PathBuf> {
    ensure!(
        crate::browser_stock::profile_prepared(root),
        "run slingring profile open on a Linux desktop to prepare the dedicated profile"
    );
    let source = root.join("stock-chrome-profile");
    crate::browser_stock::require_private(&source)?;
    let runtime = crate::browser_linux::runtime_root(root)?;
    crate::browser_linux::private_directory(&runtime, true)?;
    let copy = runtime.join(COPY);
    if fs::symlink_metadata(&copy).is_ok() {
        crate::browser_linux::private_directory(&copy, false)?;
        return Ok(copy);
    }
    let staging = runtime.join(format!(".{COPY}-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&staging)?;
    let copied = copy_tree(&source, &staging, 0).and_then(|()| fs::rename(&staging, &copy));
    if let Err(error) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(error.into());
    }
    Ok(copy)
}

/// Remove this generation's copy after the helper session closed. Missing is
/// fine; anything other than the exact private copy directory is refused.
pub(crate) fn remove_copy(root: &Path) -> Result<()> {
    let copy = copy_path(root)?;
    match fs::symlink_metadata(&copy) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => {
            crate::browser_linux::private_directory(&copy, false)?;
            fs::remove_dir_all(&copy)?;
            Ok(())
        }
    }
}

fn skipped(name: &str) -> bool {
    SKIP.contains(&name) || name.starts_with("Singleton") || name.starts_with(".org.chromium.")
}

/// Copy only regular files and directories with private modes. Symlinks and
/// special files are not followed or reproduced.
fn copy_tree(source: &Path, target: &Path, depth: usize) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(io::Error::other("profile tree is unexpectedly deep"));
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_str().is_none_or(skipped) {
            continue;
        }
        let kind = entry.file_type()?;
        let destination = target.join(&name);
        if kind.is_dir() {
            fs::DirBuilder::new().mode(0o700).create(&destination)?;
            copy_tree(&entry.path(), &destination, depth + 1)?;
        } else if kind.is_file() {
            let mut input = fs::File::open(entry.path())?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&destination)?;
            io::copy(&mut input, &mut output)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_skips_caches_locks_and_symlinks_and_keeps_private_modes() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(source.join("Default/Cache")).unwrap();
        fs::create_dir_all(source.join("Default/Network")).unwrap();
        fs::write(source.join("Local State"), b"{}").unwrap();
        fs::write(source.join("Default/Network/Cookies"), b"cookies").unwrap();
        fs::write(source.join("Default/Cache/data"), b"cache").unwrap();
        symlink("/etc/passwd", source.join("SingletonLock")).unwrap();
        symlink("/etc/passwd", source.join("Default/link")).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
        copy_tree(&source, &target, 0).unwrap();
        assert!(target.join("Local State").is_file());
        assert_eq!(
            fs::read(target.join("Default/Network/Cookies")).unwrap(),
            b"cookies"
        );
        assert!(!target.join("Default/Cache").exists());
        assert!(fs::symlink_metadata(target.join("SingletonLock")).is_err());
        assert!(fs::symlink_metadata(target.join("Default/link")).is_err());
        let mode = fs::metadata(target.join("Default/Network/Cookies"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let mode = fs::metadata(target.join("Default/Network"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn resolve_rejects_unknown_names_and_upstream_helper() {
        assert!(resolve("chromium").is_err());
        let temp = tempfile::tempdir().unwrap();
        let upstream = temp.path().join("agent-browser");
        fs::write(&upstream, b"#!/bin/sh\n").unwrap();
        assert!(resolve(upstream.to_str().unwrap()).is_err());
        let helper = temp.path().join("agent-browser-browse");
        fs::write(&helper, b"#!/bin/sh\n").unwrap();
        assert_eq!(resolve(helper.to_str().unwrap()).unwrap(), helper);
    }
}
