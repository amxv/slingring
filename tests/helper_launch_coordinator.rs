//! Opt-in cross-repository integration. Runs only a disposable shell stub via
//! the reviewed pinned-helper test binary. Never launches Chrome or a website.
#![cfg(target_os = "macos")]
use anyhow::{Context, Result, ensure};
use slingring::launch_coordinator::{GuardConfig, PendingLaunch, State};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::{Instant, timeout},
};

#[tokio::test]
#[ignore = "requires separately built pinned-helper and launch-guardian fixture binaries"]
async fn exact_pinned_helper_uses_authenticated_coordinator_and_root_only_report() -> Result<()> {
    let helper = PathBuf::from(
        std::env::var_os("SLINGRING_TEST_HELPER_FIXTURE").context("set fixture test binary")?,
    )
    .canonicalize()?;
    let guardian = PathBuf::from(
        std::env::var_os("SLINGRING_TEST_LAUNCH_GUARD").context("set fixture guardian binary")?,
    )
    .canonicalize()?;
    let root = tempfile::tempdir()?;
    let profile = tempfile::Builder::new()
        .prefix("agent-browser-profile-joint-")
        .tempdir()?;
    fs::set_permissions(profile.path(), fs::Permissions::from_mode(0o700))?;
    let programs = tempfile::tempdir()?;
    let fake = programs.path().join("fake-chrome");
    fs::write(
        &fake,
        r#"#!/bin/sh
set -eu
for arg in "$@"; do
  case "$arg" in
    --user-data-dir=*) dir=${arg#--user-data-dir=};;
  esac
done
printf '%s\n' "$$" > "$dir/fake-browser.pid"
printf '12345\n/devtools/browser/fixture\n' > "$dir/DevToolsActivePort"
exec /bin/sleep 60
"#,
    )?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700))?;
    let pending = PendingLaunch::prepare(
        root.path(),
        GuardConfig {
            guard_executable: guardian.clone(),
            browser_executable: fake.clone(),
        },
    )?;
    let expected_session = pending.session().to_owned();
    let mut child = Command::new(helper)
        .args([
            "native::cdp::chrome::tests::test_private_launch_guard_path_with_external_coordinator",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .envs(pending.environment())
        .env("OWNED_LAUNCH_FAKE_BROWSER", &fake)
        .env("OWNED_LAUNCH_FAKE_PROFILE", profile.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let lease = pending
        .authorize_before(Instant::now() + Duration::from_secs(10))
        .await?;
    ensure!(
        lease.receipt().guardian.executable == guardian,
        "wrong guardian authenticated"
    );
    ensure!(
        lease.receipt().session == expected_session,
        "wrong generation session"
    );
    ensure!(
        lease.receipt().profile == profile.path().canonicalize()?,
        "wrong fixture profile"
    );
    let stdout = child.stdout.take().context("fixture stdout missing")?;
    let mut reader = BufReader::new(stdout);
    timeout(Duration::from_secs(5), async {
        loop {
            let mut line = String::new();
            let count = reader.read_line(&mut line).await?;
            ensure!(count > 0 && count < 4096, "fixture readiness missing");
            if line.contains("fake-browser-ready") {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(
        profile.path().join("fake-browser.pid").is_file(),
        "fixture never launched"
    );
    let receipt = lease.close().await?;
    ensure!(
        receipt.state == State::RootReapedTreeUnverified,
        "overstated cleanup scope"
    );
    // The pinned-helper test itself verifies its guardian and exact fake root
    // have exited, while the helper's original stdin lease was still held.
    let status = timeout(Duration::from_secs(5), child.wait()).await??;
    ensure!(status.success(), "pinned helper rejected joint fixture");
    ensure!(
        PendingLaunch::prepare(
            root.path(),
            GuardConfig {
                guard_executable: guardian,
                browser_executable: fake
            }
        )
        .is_err(),
        "root-only report must not unblock unproven tree cleanup"
    );
    Ok(())
}
