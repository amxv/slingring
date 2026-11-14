//! Synthetic peers only. No Chrome, helper daemon, account, or OS signaling.
#![cfg(target_os = "macos")]
use anyhow::Result;
use slingring::launch_coordinator::{
    self as coordinator, GuardConfig, PendingLaunch, State, VERSION,
};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
struct Fixture {
    root: tempfile::TempDir,
    profile: tempfile::TempDir,
    config: GuardConfig,
}
impl Fixture {
    fn new() -> Result<Self> {
        let profile = tempfile::Builder::new()
            .prefix("agent-browser-profile-coordinator-")
            .tempdir()?;
        fs::set_permissions(profile.path(), fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            root: tempfile::tempdir()?,
            profile,
            config: GuardConfig {
                guard_executable: std::env::current_exe()?.canonicalize()?,
                browser_executable: PathBuf::from("/fixture/never-executed-browser"),
            },
        })
    }
    fn pending(&self) -> Result<PendingLaunch> {
        PendingLaunch::prepare(self.root.path(), self.config.clone())
    }
    fn fields(&self, pending: &PendingLaunch) -> Vec<Vec<u8>> {
        vec![
            VERSION.as_bytes().to_vec(),
            pending.session().as_bytes().to_vec(),
            pending.generation().as_bytes().to_vec(),
            std::process::id().to_be_bytes().to_vec(),
            self.config
                .browser_executable
                .as_os_str()
                .as_encoded_bytes()
                .to_vec(),
            self.profile.path().as_os_str().as_encoded_bytes().to_vec(),
        ]
    }
}
async fn peer(path: PathBuf, fields: Vec<Vec<u8>>) -> Result<UnixStream> {
    let mut socket = UnixStream::connect(path).await?;
    for field in fields {
        socket.write_u32(field.len() as u32).await?;
        socket.write_all(&field).await?;
    }
    Ok(socket)
}
#[tokio::test]
async fn approval_is_durable_and_held_until_explicit_lease_close() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let path = pending.socket_path();
    assert!(path.as_os_str().as_encoded_bytes().len() < 104);
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(path.parent().unwrap())?.permissions().mode() & 0o777,
        0o700
    );
    let fields = fixture.fields(&pending);
    let root = fixture.root.path().to_owned();
    let client = tokio::spawn(async move {
        let mut socket = peer(path, fields).await?;
        assert_eq!(socket.read_u8().await?, 1);
        let receipt = coordinator::read_receipt(&root)?.unwrap();
        assert_eq!(receipt.state, State::Authorized);
        assert_eq!(receipt.guardian.pid, std::process::id());
        assert_eq!(
            fs::metadata(root.join("launch-receipt.json"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let mut byte = [0];
        assert_eq!(socket.read(&mut byte).await?, 0);
        socket.write_all(&[2]).await?;
        Ok::<_, anyhow::Error>(())
    });
    let lease = pending.authorize().await?;
    assert!(
        !client.is_finished(),
        "lease must not close at authorization return"
    );
    assert!(
        fixture.pending().is_err(),
        "generation lock must remain held"
    );
    let receipt = lease.close().await?;
    assert_eq!(receipt.state, State::RootReapedTreeUnverified);
    client.await??;
    assert!(
        fixture.pending().is_err(),
        "root exit cannot prove tree exit"
    );
    let log = fs::read_to_string(fixture.root.path().join("events.log"))?;
    assert!(log.contains("tree_unverified"));
    assert!(!log.contains("fixture/"));
    assert!(!log.contains("agent-browser-profile"));
    Ok(())
}
#[tokio::test]
async fn each_untrusted_protocol_claim_is_checked_before_approval() -> Result<()> {
    for index in 0..6 {
        let fixture = Fixture::new()?;
        let pending = fixture.pending()?;
        let mut fields = fixture.fields(&pending);
        fields[index] = if index == 3 {
            1_u32.to_be_bytes().to_vec()
        } else {
            b"untrusted".to_vec()
        };
        let client = tokio::spawn(peer(pending.socket_path(), fields));
        assert!(pending.authorize().await.is_err(), "field {index}");
        if let Ok(Ok(mut socket)) = client.await {
            assert!(
                socket.read_u8().await.is_err(),
                "invalid peer must not receive approval"
            );
        }
        assert!(coordinator::read_receipt(fixture.root.path())?.is_none());
    }
    Ok(())
}
#[tokio::test]
async fn exact_peer_executable_is_required_not_only_a_claimed_pid() -> Result<()> {
    let mut fixture = Fixture::new()?;
    fixture.config.guard_executable = PathBuf::from("/bin/sh");
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    assert!(pending.authorize().await.is_err());
    let _ = client.await;
    assert!(coordinator::read_receipt(fixture.root.path())?.is_none());
    Ok(())
}
#[tokio::test]
async fn non_private_profile_and_oversized_frame_are_rejected() -> Result<()> {
    let fixture = Fixture::new()?;
    fs::set_permissions(fixture.profile.path(), fs::Permissions::from_mode(0o755))?;
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    assert!(pending.authorize().await.is_err());
    let _ = client.await;
    fs::set_permissions(fixture.profile.path(), fs::Permissions::from_mode(0o700))?;
    let pending = fixture.pending()?;
    let path = pending.socket_path();
    let client = tokio::spawn(async move {
        let mut socket = UnixStream::connect(path).await?;
        socket.write_u32(u32::MAX).await?;
        socket.read_u8().await
    });
    assert!(pending.authorize().await.is_err());
    assert!(client.await?.is_err());
    assert!(coordinator::read_receipt(fixture.root.path())?.is_none());
    Ok(())
}
#[tokio::test]
async fn durable_receipt_failure_never_authorizes_launch() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    fs::create_dir(fixture.root.path().join("launch-receipt.json"))?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    assert!(pending.authorize().await.is_err());
    if let Ok(Ok(mut socket)) = client.await {
        assert!(socket.read_u8().await.is_err());
    }
    assert!(fixture.root.path().join("launch-receipt.json").is_dir());
    Ok(())
}
#[tokio::test]
async fn unstarted_helper_or_cancelled_handshake_does_not_leave_a_launch_fence() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let socket = pending.socket_path();
    drop(pending); // Definite helper exec failure: never an authorized guardian.
    assert!(!socket.exists());
    assert!(coordinator::read_receipt(fixture.root.path())?.is_none());
    let pending = fixture.pending()?;
    let task = tokio::spawn(pending.authorize());
    tokio::task::yield_now().await;
    task.abort();
    let _ = task.await;
    assert!(fixture.pending().is_ok());
    Ok(())
}
#[tokio::test]
async fn owner_loss_keeps_uncertainty_and_does_not_reuse_a_generation() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    let lease = pending.authorize().await?;
    let mut socket = client.await??;
    assert_eq!(socket.read_u8().await?, 1);
    let generation = lease.receipt().generation.clone();
    drop(lease); // Models worker cancellation after the durable approval.
    assert_eq!(socket.read(&mut [0]).await?, 0);
    let receipt = coordinator::read_receipt(fixture.root.path())?.unwrap();
    assert_eq!(receipt.generation, generation);
    assert_eq!(receipt.state, State::Authorized);
    assert!(fixture.pending().is_err());
    Ok(())
}
#[tokio::test]
async fn invalid_cleanup_report_never_upgrades_the_durable_outcome() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    let lease = pending.authorize().await?;
    let mut socket = client.await??;
    assert_eq!(socket.read_u8().await?, 1);
    socket.write_all(&[99]).await?;
    assert!(lease.close().await.is_err());
    assert_eq!(
        coordinator::read_receipt(fixture.root.path())?
            .unwrap()
            .state,
        State::Authorized
    );
    assert!(fixture.pending().is_err());
    Ok(())
}

#[tokio::test]
async fn stalled_peer_is_bounded_without_authorization_or_a_persistent_fence() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let path = pending.socket_path();
    let client = tokio::spawn(async move {
        let mut stream = UnixStream::connect(path).await?;
        stream.write_u32(32).await?; // Never supply the promised field bytes.
        stream.read_u8().await
    });
    assert!(pending.authorize().await.is_err());
    assert!(client.await?.is_err());
    assert!(coordinator::read_receipt(fixture.root.path())?.is_none());
    assert!(fixture.pending().is_ok());
    Ok(())
}

#[tokio::test]
async fn root_exit_report_permanently_ends_reuse_and_preserves_its_limited_scope() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    let mut lease = pending.authorize().await?;
    let mut socket = client.await??;
    assert_eq!(socket.read_u8().await?, 1);
    socket.write_all(&[2]).await?;
    // Wait for readiness without treating a scheduler delay as lifecycle proof.
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while lease.check_live().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(
        lease.check_live().is_err(),
        "consumed report must not restore reuse"
    );
    assert_eq!(lease.close().await?.state, State::RootReapedTreeUnverified);
    assert!(slingring::browser_session::ensure_reconciled(fixture.root.path()).is_err());
    Ok(())
}

#[tokio::test]
async fn cleanup_never_overwrites_a_different_durable_generation() -> Result<()> {
    let fixture = Fixture::new()?;
    let pending = fixture.pending()?;
    let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
    let lease = pending.authorize().await?;
    let mut socket = client.await??;
    assert_eq!(socket.read_u8().await?, 1);
    let mut replacement = lease.receipt().clone();
    replacement.generation = uuid::Uuid::new_v4().to_string();
    slingring::config::atomic(
        &fixture.root.path().join("launch-receipt.json"),
        &serde_json::to_vec(&replacement)?,
    )?;
    socket.write_all(&[2]).await?;
    assert!(lease.close().await.is_err());
    let current = coordinator::read_receipt(fixture.root.path())?.unwrap();
    assert_eq!(current.generation, replacement.generation);
    assert_eq!(current.state, State::Authorized);
    Ok(())
}

#[tokio::test]
async fn invalid_live_evidence_stays_terminal_even_if_a_later_report_looks_valid() -> Result<()> {
    for invalid in [Some(99), None] {
        let fixture = Fixture::new()?;
        let pending = fixture.pending()?;
        let client = tokio::spawn(peer(pending.socket_path(), fixture.fields(&pending)));
        let mut lease = pending.authorize().await?;
        let mut socket = client.await??;
        assert_eq!(socket.read_u8().await?, 1);
        if let Some(byte) = invalid {
            socket.write_all(&[byte]).await?;
        } else {
            socket.shutdown().await?;
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while lease.check_live().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        if invalid.is_some() {
            socket.write_all(&[2]).await?;
        }
        assert!(lease.check_live().is_err());
        assert!(lease.close().await.is_err());
        assert_eq!(
            coordinator::read_receipt(fixture.root.path())?
                .unwrap()
                .state,
            State::Authorized
        );
    }
    Ok(())
}
