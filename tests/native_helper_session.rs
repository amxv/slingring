//! Explicit opt-in native daemon fixture. Never runs Chrome, a site or an account.
#![cfg(target_os = "macos")]
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use slingring::{
    browser_session::Session,
    launch_coordinator::{self, GuardConfig, State},
};
use std::{
    collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Stdio,
    time::Duration,
};
use tokio::{
    process::Command,
    time::{Instant, timeout},
};
const URL: &str = "about:blank#fake-lease-fixture";
fn required(name: &str) -> Result<PathBuf> {
    PathBuf::from(std::env::var_os(name).with_context(|| format!("set {name}"))?)
        .canonicalize()
        .map_err(Into::into)
}
fn digest(path: &PathBuf) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
struct Native {
    helper: PathBuf,
    fake: PathBuf,
    profile: PathBuf,
    root: PathBuf,
    env: BTreeMap<String, String>,
    session: std::sync::Arc<Session>,
}
impl Native {
    async fn cli(&self, args: &[&str]) -> Result<Value> {
        self.session.check_command(args.first() == Some(&"open"))?;
        let mut command = Command::new(&self.helper);
        command
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.root)
            .args([
                "--json",
                "--session",
                self.session.name(),
                "--executable-path",
            ])
            .arg(&self.fake)
            .arg("--profile")
            .arg(&self.profile)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = timeout(Duration::from_secs(16), command.output()).await??;
        ensure!(
            output.status.success(),
            "fixture helper command failed (output suppressed)"
        );
        let value: Value = serde_json::from_slice(&output.stdout)?;
        ensure!(
            value["success"] == true,
            "fixture helper did not acknowledge command"
        );
        Ok(value)
    }
}
#[tokio::test]
#[ignore = "requires explicitly pinned native helper/guardian fixture artifacts; no browser"]
async fn native_daemon_uses_actual_session_lease_and_unique_namespace() -> Result<()> {
    let helper = required("SLINGRING_TEST_NATIVE_HELPER")?;
    let guardian = required("SLINGRING_TEST_LAUNCH_GUARD")?;
    let manifest_path = required("SLINGRING_TEST_CAPABILITY_MANIFEST")?;
    let expected = std::env::var("SLINGRING_TEST_CAPABILITY_SHA256")?;
    ensure!(
        expected.len() == 64 && digest(&manifest_path)? == expected,
        "fixture manifest pin mismatch"
    );
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    ensure!(
        manifest["artifact_scope"] == "offline_fixture_candidate"
            && manifest["protocol"] == launch_coordinator::VERSION
            && manifest["cleanup_evidence"] == "root_reaped_tree_unverified",
        "wrong fixture capability"
    );
    ensure!(
        manifest["binaries"]["agent-browser-browse-native"] == digest(&helper)?
            && manifest["binaries"]["slingring-owned-launch-guard"] == digest(&guardian)?,
        "fixture binary pin mismatch"
    );
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let home = root.join("home");
    let profile = root.join("agent-browser-profile-native-session");
    for path in [&home, &profile] {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let python = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .await?;
    ensure!(python.status.success(), "fixture Python unavailable");
    let python = PathBuf::from(std::str::from_utf8(&python.stdout)?.trim()).canonicalize()?;
    let fake = root.join("fake-chrome");
    let script = include_str!("fixtures/fake_chrome.py")
        .split_once('\n')
        .context("fixture header")?
        .1;
    fs::write(&fake, format!("#!{}\n{script}", python.display()))?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700))?;
    let state = root.join("state");
    let session = Session::prepare(
        &state,
        GuardConfig {
            guard_executable: guardian,
            browser_executable: fake.clone(),
        },
    )?;
    let mut env: BTreeMap<String, String> = session
        .environment()
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
    let config = root.join("fixture-config.json");
    fs::write(&config, "{}\n")?;
    for (key, value) in [
        ("HOME", home.to_string_lossy().into_owned()),
        ("TMPDIR", root.to_string_lossy().into_owned()),
        ("PATH", "/usr/bin:/bin".into()),
        (
            "AGENT_BROWSER_CONFIG",
            config.to_string_lossy().into_owned(),
        ),
        ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "20000".into()),
    ] {
        env.insert(key.into(), value);
    }
    let daemon = PathBuf::from(&env["AGENT_BROWSER_SOCKET_DIR"])
        .join("namespaces")
        .join(session.name())
        .join("run");
    let pid_file = daemon.join(format!("{}.pid", session.name()));
    let socket = daemon.join(format!("{}.sock", session.name()));
    ensure!(
        !pid_file.exists() && !socket.exists(),
        "fixture namespace already exists"
    );
    let native = Native {
        helper,
        fake,
        profile: profile.clone(),
        root,
        env,
        session: session.clone(),
    };
    let outcome = async {
        let (_, opened) = tokio::try_join!(
            session.authorize(Instant::now() + Duration::from_secs(10)),
            native.cli(&["open", URL])
        )?;
        ensure!(opened.to_string().contains(URL), "fake navigation mismatch");
        session.verify_profile(&profile)?;
        let receipt = launch_coordinator::read_receipt(&state)?.context("receipt missing")?;
        ensure!(
            receipt.state == State::Authorized && receipt.session == session.name(),
            "wrong durable session"
        );
        let daemon_pid = fs::read(&pid_file)?;
        let fake_pid = fs::read(profile.join("fake.pid"))?;
        ensure!(socket.exists(), "isolated daemon socket missing");
        ensure!(
            native.cli(&["get", "url"]).await?.to_string().contains(URL),
            "fake URL mismatch"
        );
        ensure!(
            fs::read(&pid_file)? == daemon_pid && fs::read(profile.join("fake.pid"))? == fake_pid,
            "helper silently replaced owned generation"
        );
        native.cli(&["close"]).await?;
        ensure!(
            session.close().await.is_err(),
            "root-only report must not claim full cleanup"
        );
        ensure!(
            launch_coordinator::read_receipt(&state)?
                .context("receipt lost")?
                .state
                == State::RootReapedTreeUnverified,
            "missing scoped root report"
        );
        timeout(Duration::from_secs(5), async {
            while pid_file.exists() || socket.exists() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await?;
        ensure!(
            slingring::browser_session::ensure_reconciled(&state).is_err(),
            "limited cleanup must keep fence"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if outcome.is_err() {
        // No PID signaling. Only our exact isolated session can receive close;
        // dropping its coordinator stream also ends the owned guardian lease.
        if socket.exists() {
            let _ = native.cli(&["close"]).await;
        }
        let _ = session.close().await;
    }
    outcome
}
