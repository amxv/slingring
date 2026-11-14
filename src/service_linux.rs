//! An optional user systemd unit. Local CLI execution still works without
//! systemd and alternate state roots never operate the default user's unit.
use crate::{config, daemon, queue};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const UNIT: &str = "slingring.service";
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(25);

fn default_root() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME missing")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home.join(".config/slingring"))
}

fn systemd_quote(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "systemd service paths must be nonempty UTF-8 without control characters"
    );
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    Ok(format!("\"{escaped}\""))
}

fn unit_text(exe: &Path, root: &Path) -> Result<String> {
    ensure!(
        exe.is_absolute() && root.is_absolute(),
        "service paths must be absolute"
    );
    // The colon ExecStart prefix disables systemd's environment substitution.
    // Environment= does not expand $, but both directives expand % specifiers.
    let executable = systemd_quote(exe.to_str().context("non-UTF-8 executable path")?)?;
    let environment = systemd_quote(&format!(
        "SLINGRING_HOME={}",
        root.to_str().context("non-UTF-8 state path")?
    ))?;
    Ok(format!(
        "# Managed by Slingring\n[Unit]\nDescription=Slingring user service\n\
         [Service]\nType=exec\nExecStart=:{executable} daemon\nEnvironment={environment}\n\
         UMask=0077\nRestart=on-failure\nRestartSec=3s\nKillMode=mixed\nTimeoutStopSec=25s\n\
         StandardOutput=null\nStandardError=null\n[Install]\nWantedBy=default.target\n"
    ))
}

fn user_id() -> Result<u32> {
    Ok(fs::metadata("/proc/self")?.uid())
}

fn secure_directory(path: &Path, create: bool) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "service directory path must be absolute"
    );
    // Do not traverse symlinks, including in the configurable XDG path.
    let uid = user_id()?;
    // A private ancestor already prevents traversal by other users. This
    // permits conventional group-write modes created by umask 002 deeper
    // inside an otherwise private home/config directory.
    let mut private_ancestor = false;
    let mut current = PathBuf::new();
    for component in path.components() {
        ensure!(
            !matches!(component, std::path::Component::ParentDir),
            "unsafe parent traversal in service path"
        );
        current.push(component);
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match fs::DirBuilder::new().mode(0o700).create(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                fs::symlink_metadata(&current)?
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            meta.is_dir()
                && (meta.uid() == 0 || meta.uid() == uid)
                && (private_ancestor || meta.mode() & 0o022 == 0 || meta.mode() & 0o1000 != 0),
            "service directory contains a symlink, foreign owner, or unsafe writable ancestor"
        );
        private_ancestor |= meta.uid() == uid && meta.mode() & 0o077 == 0;
    }
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.uid() == uid && (private_ancestor || meta.mode() & 0o022 == 0),
        "service directory must be owned by the current user and not writable by others"
    );
    Ok(())
}

struct Manager {
    path: PathBuf,
    exe: PathBuf,
    command: PathBuf,
}

impl Manager {
    fn current() -> Result<Self> {
        let home = default_root()?;
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.with_file_name(""));
        let command = ["/usr/bin/systemctl", "/bin/systemctl"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("/usr/bin/systemctl"));
        Ok(Self {
            path: base.join("systemd/user").join(UNIT),
            exe: std::env::current_exe()?.canonicalize()?,
            command,
        })
    }

    fn invoke(&self, args: &[&str]) -> Result<(std::process::ExitStatus, String)> {
        let mut child = Command::new(&self.command)
            .args(["--user", "--no-pager", "--no-ask-password"])
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("systemctl is unavailable; use the direct Slingring daemon instead")?;
        // Only bounded property replies are expected. A blocked or overlong
        // response never counts as proof; only this owned command is killed.
        let deadline = Instant::now() + CONTROL_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("systemd user manager did not answer in time");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("missing systemctl reply")?
            .take(8193)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 8192,
            "systemd user manager reply was too large"
        );
        Ok((status, String::from_utf8(bytes)?))
    }

    fn control(&self, args: &[&str]) -> Result<()> {
        let (status, _) = self.invoke(args)?;
        ensure!(
            status.success(),
            "systemd user service command failed; check that a user manager is available"
        );
        Ok(())
    }

    fn show(&self) -> Result<UnitState> {
        let (status, stdout) = self.invoke(&[
            "show",
            "--all",
            "--property=LoadState,ActiveState,FragmentPath,DropInPaths,UnitFileState,NeedDaemonReload",
            UNIT,
        ])?;
        let state = UnitState::parse(&stdout)?;
        ensure!(
            status.success() || (matches!(status.code(), Some(1 | 4)) && state.absent()),
            "systemd user manager could not inspect the Slingring unit"
        );
        Ok(state)
    }

    fn owns_file(&self, root: &Path) -> Result<bool> {
        let meta = match fs::symlink_metadata(&self.path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        secure_directory(self.path.parent().context("missing unit directory")?, false)?;
        let expected = unit_text(&self.exe, root)?;
        ensure!(
            meta.is_file()
                && meta.uid() == user_id()?
                && meta.mode() & 0o022 == 0
                && meta.len() == expected.len() as u64
                && fs::read(&self.path)? == expected.as_bytes(),
            "refusing a foreign or modified Slingring systemd unit"
        );
        Ok(true)
    }

    fn owned_state(&self) -> Result<UnitState> {
        let state = self.show()?;
        ensure!(
            state.load == "loaded"
                && state.fragment == self.path.to_str().context("non-UTF-8 unit path")?
                && state.drop_ins.is_empty()
                && state.reload == "no",
            "systemd did not resolve the exact Slingring user unit without overrides"
        );
        Ok(state)
    }

    fn install(&self, root: &Path) -> Result<()> {
        if self.owns_file(root)? {
            secure_directory(root, false)?;
            self.owned_state()?;
            return Ok(());
        }
        ensure!(
            self.show()?.absent(),
            "another Slingring systemd unit is already present"
        );
        secure_directory(root, true)?;
        config::private_dir(root)?;
        let parent = self.path.parent().context("missing unit directory")?;
        secure_directory(parent, true)?;
        let body = unit_text(&self.exe, root)?;
        let temp = parent.join(format!(".slingring-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        let written = (|| -> Result<()> {
            file.write_all(body.as_bytes())?;
            file.sync_all()?;
            // Publish without replacing a competing or pre-existing unit.
            fs::hard_link(&temp, &self.path)?;
            Ok(())
        })();
        let _ = fs::remove_file(temp);
        written?;
        self.control(&["daemon-reload"])?;
        self.owned_state()?;
        Ok(())
    }

    fn toggle_login(&self, root: &Path, enable: bool) -> Result<()> {
        if enable {
            self.install(root)?;
        } else if !self.owns_file(root)? {
            return Ok(());
        }
        self.owned_state()?;
        // Never --now: future-login preferences must not interrupt work.
        self.control(&[if enable { "enable" } else { "disable" }, UNIT])
    }

    fn uninstall(&self, root: &Path) -> Result<()> {
        if !self.owns_file(root)? {
            return Ok(());
        }
        let state = self.owned_state()?;
        ensure!(
            state.inactive(),
            "stop the Slingring service before uninstalling its unit"
        );
        self.control(&["disable", UNIT])?;
        ensure!(
            self.owns_file(root)?,
            "Slingring unit changed during uninstall"
        );
        fs::remove_file(&self.path)?;
        self.control(&["daemon-reload"])
    }
}

#[derive(Default)]
struct UnitState {
    load: String,
    active: String,
    fragment: String,
    drop_ins: String,
    file_state: String,
    reload: String,
}

impl UnitState {
    fn parse(reply: &str) -> Result<Self> {
        let mut state = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for line in reply.lines() {
            let (key, value) = line
                .split_once('=')
                .context("invalid systemctl property reply")?;
            ensure!(seen.insert(key), "duplicate systemctl property");
            match key {
                "LoadState" => state.load = value.to_owned(),
                "ActiveState" => state.active = value.to_owned(),
                "FragmentPath" => state.fragment = value.to_owned(),
                "DropInPaths" => state.drop_ins = value.to_owned(),
                "UnitFileState" => state.file_state = value.to_owned(),
                "NeedDaemonReload" => state.reload = value.to_owned(),
                _ => bail!("unexpected systemctl property"),
            }
        }
        ensure!(
            seen.len() == 6
                && !state.load.is_empty()
                && !state.active.is_empty()
                && matches!(state.reload.as_str(), "yes" | "no"),
            "systemd user manager state is unavailable"
        );
        Ok(state)
    }

    fn absent(&self) -> bool {
        self.load == "not-found"
            && self.active == "inactive"
            && self.fragment.is_empty()
            && self.drop_ins.is_empty()
            && self.file_state.is_empty()
    }

    fn inactive(&self) -> bool {
        matches!(self.active.as_str(), "inactive" | "failed")
    }

    fn login(&self) -> Value {
        let (enabled, state) = match self.file_state.as_str() {
            "enabled" => (Some(true), "enabled"),
            "disabled" => (Some(false), "disabled"),
            "enabled-runtime" => (Some(false), "enabled_runtime"),
            _ => (None, "unavailable"),
        };
        json!({"enabled":enabled,"state":state,"registered":true,"loaded":true})
    }
}

fn require_default(root: &Path) -> Result<()> {
    ensure!(
        root == default_root()?,
        "systemd registration is supported only for the default Slingring state directory"
    );
    Ok(())
}

pub fn installed() -> bool {
    let Ok(root) = config::root() else {
        return false;
    };
    require_default(&root)
        .and_then(|()| Manager::current()?.owns_file(&root))
        .unwrap_or(false)
}

pub fn login_status(root: &Path) -> Result<Value> {
    if root != default_root()? {
        return Ok(json!({"enabled":false,"state":"unmanaged","registered":false,"loaded":false}));
    }
    let manager = Manager::current()?;
    if !manager.owns_file(root)? {
        return Ok(
            json!({"enabled":false,"state":"not_installed","registered":false,"loaded":false}),
        );
    }
    Ok(manager.owned_state()?.login())
}

pub fn install(root: &Path) -> Result<()> {
    require_default(root)?;
    Manager::current()?.install(root)
}

pub fn uninstall() -> Result<()> {
    let root = config::root()?;
    require_default(&root)?;
    Manager::current()?.uninstall(&root)
}

pub fn login_enable(root: &Path) -> Result<()> {
    require_default(root)?;
    Manager::current()?.toggle_login(root, true)
}

pub fn login_disable(root: &Path) -> Result<()> {
    require_default(root)?;
    Manager::current()?.toggle_login(root, false)
}

async fn await_ready(root: &Path, mut child: Option<&mut Child>) -> Result<()> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if daemon::running(root).await {
            return Ok(());
        }
        if let Some(child) = child.as_mut()
            && let Some(status) = child.try_wait()?
        {
            bail!(
                "Slingring daemon exited before becoming ready ({status}); run slingring daemon in the foreground to diagnose"
            );
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Slingring service did not become ready; run slingring daemon in the foreground to diagnose"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn direct_state_exists(root: &Path) -> Result<bool> {
    match fs::symlink_metadata(root) {
        Ok(meta) => {
            ensure!(
                meta.is_dir() && meta.uid() == user_id()?,
                "state directory must be user-owned and not a symlink"
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub async fn start(root: &Path, ephemeral: bool) -> Result<()> {
    // A caller-chosen state directory is a standalone Unix daemon, not a
    // systemd unit. Preserve the ordinary private-state rules without requiring
    // the root to live in the default unit's directory layout.
    let manager = if !ephemeral && default_root().is_ok_and(|default| root == default) {
        let manager = Manager::current()?;
        if manager.owns_file(root)? {
            secure_directory(root, true)?;
            Some(manager)
        } else {
            None
        }
    } else {
        None
    };
    direct_state_exists(root)?;
    config::private_dir(root)?;
    ensure!(
        direct_state_exists(root)?,
        "Slingring state directory is missing"
    );
    if daemon::running(root).await {
        return Ok(());
    }
    if queue::owner_running(root)? {
        // Another process can briefly own the lock before accepting IPC.
        return await_ready(root, None).await;
    }
    if let Some(manager) = manager {
        tokio::task::spawn_blocking(move || {
            manager.owned_state()?;
            manager.control(&["--no-block", "start", UNIT])
        })
        .await??;
        return await_ready(root, None).await;
    }
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("daemon");
    if ephemeral {
        command.arg("--ephemeral");
    }
    let mut child = command
        .env("SLINGRING_HOME", root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    await_ready(root, Some(&mut child)).await
}

pub async fn stop(root: &Path) -> Result<()> {
    if !direct_state_exists(root)? {
        return Ok(());
    }
    // Use only the same private daemon IPC as on macOS; never signal a PID
    // discovered from the process table or stop an unrelated manager unit.
    if daemon::running(root).await {
        tokio::time::timeout(
            Duration::from_secs(2),
            daemon::call(root, &daemon::Request::Stop),
        )
        .await
        .context("Slingring service did not acknowledge shutdown")??;
    } else {
        ensure!(
            !queue::owner_running(root)?,
            "Slingring owns this state directory but is not responding"
        );
        return Ok(());
    }
    let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
    while queue::owner_running(root)? {
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Slingring service is still shutting down; no processes were signalled"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        directory: tempfile::TempDir,
        manager: Manager,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Result<Self> {
            let directory = tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()?;
            let base = directory.path();
            let command = base.join("systemctl-stub");
            fs::write(
                &command,
                r#"#!/bin/sh
set -eu
base=${0%/*}
unit="$base/unit dir/slingring.service"
printf '%s\n' "$*" >> "$base/calls"
[ "$1" = --user ] && [ "$2" = --no-pager ] && [ "$3" = --no-ask-password ]
shift 3
[ ! -e "$base/unavailable" ] || exit 1
case "$1" in
  show)
    [ "$2" = --all ] && [ "$4" = slingring.service ]
    if [ -e "$unit" ]; then
      printf 'LoadState=loaded\nActiveState=%s\nFragmentPath=%s\nDropInPaths=%s\nUnitFileState=%s\nNeedDaemonReload=%s\n' \
        "$(cat "$base/active" 2>/dev/null || echo inactive)" \
        "$(cat "$base/shadow" 2>/dev/null || echo "$unit")" \
        "$(cat "$base/drop-in" 2>/dev/null || true)" \
        "$(cat "$base/enabled" 2>/dev/null || echo disabled)" \
        "$(cat "$base/reload" 2>/dev/null || echo no)"
    else
      printf 'LoadState=not-found\nActiveState=inactive\nFragmentPath=\nDropInPaths=\nUnitFileState=\nNeedDaemonReload=no\n'
      exit 4
    fi
    ;;
  daemon-reload) [ "$#" = 1 ];;
  enable|disable)
    [ "$#" = 2 ] && [ "$2" = slingring.service ]
    if [ "$1" = enable ]; then echo enabled; else echo disabled; fi > "$base/enabled"
    ;;
  *) exit 10;;
esac
"#,
            )?;
            fs::set_permissions(&command, fs::Permissions::from_mode(0o700))?;
            let manager = Manager {
                path: base.join("unit dir").join(UNIT),
                exe: base.join("my sling%ring"),
                command,
            };
            let root = base.join("private state");
            Ok(Self {
                directory,
                manager,
                root,
            })
        }

        fn set(&self, name: &str, value: &str) -> Result<()> {
            fs::write(self.directory.path().join(name), value)?;
            Ok(())
        }
    }

    #[test]
    fn unit_escapes_paths_and_rejects_line_injection() -> Result<()> {
        let unit = unit_text(
            Path::new("/tmp/a b%h/sling\"ring"),
            Path::new("/tmp/$HOME/another\\path"),
        )?;
        assert!(unit.contains("ExecStart=:\"/tmp/a b%%h/sling\\\"ring\" daemon\n"));
        assert!(unit.contains("Environment=\"SLINGRING_HOME=/tmp/$HOME/another\\\\path\"\n"));
        assert!(unit_text(Path::new("/tmp/evil\nExecStart=/bin/sh"), Path::new("/tmp")).is_err());
        assert!(unit_text(Path::new("relative"), Path::new("/tmp")).is_err());
        Ok(())
    }

    #[test]
    fn registration_login_and_uninstall_use_only_the_owned_unit() -> Result<()> {
        let f = Fixture::new()?;
        fs::create_dir(f.manager.path.parent().unwrap())?;
        fs::set_permissions(
            f.manager.path.parent().unwrap(),
            fs::Permissions::from_mode(0o775),
        )?;
        f.manager.install(&f.root)?;
        assert_eq!(f.manager.owned_state()?.login()["enabled"], false);
        assert_eq!(fs::metadata(&f.root)?.mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&f.manager.path)?.mode() & 0o777, 0o600);
        f.set("active", "active")?;
        f.manager.toggle_login(&f.root, true)?;
        assert_eq!(f.manager.owned_state()?.login()["enabled"], true);
        assert_eq!(f.manager.owned_state()?.active, "active");
        f.manager.toggle_login(&f.root, false)?;
        assert_eq!(f.manager.owned_state()?.login()["enabled"], false);
        assert!(f.manager.uninstall(&f.root).is_err());
        assert!(f.manager.path.is_file());
        f.set("active", "inactive")?;
        f.manager.uninstall(&f.root)?;
        assert!(!f.manager.path.exists());
        let calls = fs::read_to_string(f.directory.path().join("calls"))?;
        assert!(calls.contains(" enable slingring.service\n"));
        assert!(!calls.contains("--now"));
        assert!(!calls.contains(" start "));
        assert!(!calls.contains(" stop "));
        Ok(())
    }

    #[test]
    fn missing_manager_and_foreign_units_fail_closed() -> Result<()> {
        let f = Fixture::new()?;
        f.set("unavailable", "")?;
        assert!(f.manager.install(&f.root).is_err());
        assert!(!f.root.exists());
        assert!(!f.manager.path.exists());
        fs::remove_file(f.directory.path().join("unavailable"))?;
        f.manager.install(&f.root)?;
        f.set("shadow", "/usr/lib/systemd/user/slingring.service")?;
        assert!(f.manager.toggle_login(&f.root, true).is_err());
        fs::remove_file(f.directory.path().join("shadow"))?;
        f.set("drop-in", "/tmp/foreign.conf")?;
        assert!(f.manager.toggle_login(&f.root, false).is_err());
        fs::remove_file(f.directory.path().join("drop-in"))?;
        f.set("reload", "yes")?;
        assert!(f.manager.owned_state().is_err());
        fs::remove_file(f.directory.path().join("reload"))?;
        fs::write(&f.manager.path, "[Service]\nExecStart=/bin/false\n")?;
        assert!(f.manager.uninstall(&f.root).is_err());
        assert!(f.manager.path.is_file());
        Ok(())
    }

    #[test]
    fn directory_and_unit_symlinks_are_rejected() -> Result<()> {
        let f = Fixture::new()?;
        let elsewhere = f.directory.path().join("elsewhere");
        fs::create_dir(&elsewhere)?;
        std::os::unix::fs::symlink(&elsewhere, &f.root)?;
        assert!(f.manager.install(&f.root).is_err());
        assert!(!f.manager.path.exists());
        fs::remove_file(&f.root)?;
        f.manager.install(&f.root)?;
        let expected = fs::read(&f.manager.path)?;
        fs::remove_file(&f.manager.path)?;
        let other = elsewhere.join("another.service");
        fs::write(&other, &expected)?;
        std::os::unix::fs::symlink(&other, &f.manager.path)?;
        assert!(f.manager.toggle_login(&f.root, true).is_err());
        assert_eq!(fs::read(other)?, expected);
        Ok(())
    }
}
