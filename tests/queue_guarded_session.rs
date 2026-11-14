//! Real queue/session lifetime with a synthetic guardian. No browser or signals.
#![cfg(target_os = "macos")]
use anyhow::{Context, Result};
use slingring::{
    browser_session::Session,
    config::Config,
    launch_coordinator::{self, GuardConfig, State},
    model::{NewTask, Task, TaskResult},
    queue::{Backend, BrowserFuture, Queue, SubmissionGate},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::{Notify, watch},
    task::JoinHandle,
};
struct Synthetic {
    profile: PathBuf,
    configured: AtomicUsize,
    runs: AtomicUsize,
    hold: Option<bool>,
    reached: Notify,
    leases: Mutex<Vec<Weak<Session>>>,
    peers: Mutex<Vec<JoinHandle<Result<()>>>>,
}
impl Synthetic {
    fn new(profile: &Path, hold: Option<bool>) -> Self {
        Self {
            profile: profile.into(),
            configured: 0.into(),
            runs: 0.into(),
            hold,
            reached: Notify::new(),
            leases: Mutex::new(vec![]),
            peers: Mutex::new(vec![]),
        }
    }
    async fn drain(&self) -> Result<()> {
        let peers = std::mem::take(&mut *self.peers.lock().unwrap());
        for peer in peers {
            tokio::time::timeout(Duration::from_secs(2), peer).await???;
        }
        Ok(())
    }
}
impl Backend for Synthetic {
    fn configure_session(&self, root: &Path, config: &mut Config) -> Result<()> {
        let session = Session::prepare(
            root,
            GuardConfig {
                guard_executable: std::env::current_exe()?,
                browser_executable: "/fixture/no-browser".into(),
            },
        )?;
        let env = session.environment();
        let get = |name| env.iter().find(|(key, _)| *key == name).unwrap().1.clone();
        assert_eq!(get("AGENT_BROWSER_NAMESPACE"), session.name());
        assert_eq!(get("SLINGRING_PRIVATE_LAUNCH_SESSION"), session.name());
        assert!(session.check_command(false).is_err());
        let socket = get("SLINGRING_PRIVATE_LAUNCH_SOCKET");
        let fields = vec![
            launch_coordinator::VERSION.as_bytes().to_vec(),
            session.name().as_bytes().to_vec(),
            get("SLINGRING_PRIVATE_LAUNCH_GENERATION").into_bytes(),
            std::process::id().to_be_bytes().to_vec(),
            b"/fixture/no-browser".to_vec(),
            self.profile.as_os_str().as_encoded_bytes().to_vec(),
        ];
        self.peers.lock().unwrap().push(tokio::spawn(async move {
            let mut peer = UnixStream::connect(socket).await?;
            for field in fields {
                peer.write_u32(field.len() as u32).await?;
                peer.write_all(&field).await?;
            }
            if peer.read_u8().await.is_err() {
                return Ok(());
            }
            let mut byte = [0];
            let _ = peer.read(&mut byte).await;
            let _ = peer.write_all(&[2]).await; // May fail after owner death; never guessed as success.
            Ok(())
        }));
        self.leases.lock().unwrap().push(Arc::downgrade(&session));
        config.browser_session = Some(session);
        self.configured.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn run<'a>(&'a self, _: &'a Config, _: &'a Task, _: &'a str) -> BrowserFuture<'a, TaskResult> {
        unreachable!()
    }
    fn run_authorized<'a>(
        &'a self,
        c: &'a Config,
        t: &'a Task,
        _: &'a str,
        _: Arc<dyn Fn(&'static str) + Send + Sync>,
        gate: SubmissionGate,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let lease = c.browser_session.as_ref().context("lease missing")?;
            lease
                .authorize(tokio::time::Instant::now() + Duration::from_secs(2))
                .await?;
            lease.verify_profile(&self.profile)?;
            if self.hold == Some(false) {
                self.reached.notify_one();
                std::future::pending::<()>().await;
            }
            gate()?;
            if self.hold == Some(true) {
                self.reached.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(serde_json::from_value(
                serde_json::json!({"job_id":"fixture","url":"https://chatgpt.com/c/fixture",
                "reasoning_effort":t.reasoning_effort(),"evidence":"thinking"}),
            )?)
        })
    }
    fn close<'a>(&'a self, c: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            c.browser_session
                .as_ref()
                .context("lease missing")?
                .close()
                .await
        })
    }
}
fn task() -> Task {
    NewTask {
        prompt: "fixture only".into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
    .into()
}
fn profile() -> Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("agent-browser-profile-queue-")
        .tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}

#[tokio::test]
async fn queued_batch_reuses_exact_lease_then_root_only_cleanup_stops_worker() -> Result<()> {
    let root = tempfile::tempdir()?;
    let profile = profile()?;
    let backend = Arc::new(Synthetic::new(profile.path(), None));
    let (_stop, rx) = watch::channel(false);
    let (q, worker) = Queue::open(root.path().into(), backend.clone(), rx)?;
    let first = q.accept_before("first".into(), task(), Config::default(), None)?;
    let second = q.accept_before("second".into(), task(), Config::default(), None)?;
    first.await??;
    assert!(second.await?.is_ok());
    assert!(worker.await?.is_err());
    assert_eq!(backend.configured.load(Ordering::SeqCst), 1);
    assert_eq!(backend.runs.load(Ordering::SeqCst), 2);
    assert_eq!(q.recovery_status("first")["outcome"], "submitted");
    assert_eq!(q.recovery_status("second")["outcome"], "submitted");
    assert_eq!(
        launch_coordinator::read_receipt(root.path())?
            .unwrap()
            .state,
        State::RootReapedTreeUnverified
    );
    assert!(
        backend
            .leases
            .lock()
            .unwrap()
            .iter()
            .all(|lease| lease.upgrade().is_none())
    );
    backend.drain().await?;
    Ok(())
}
#[tokio::test]
async fn uncertain_cleanup_blocks_configuration_change_without_overwriting_confirmed_result()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let profile = profile()?;
    let backend = Arc::new(Synthetic::new(profile.path(), None));
    let (_stop, rx) = watch::channel(false);
    let (q, worker) = Queue::open(root.path().into(), backend.clone(), rx)?;
    let first = q.accept_before("first".into(), task(), Config::default(), None)?;
    let next = Config {
        profile: "different-fixture".into(),
        ..Config::default()
    };
    let second = q.accept_before("second".into(), task(), next, None)?;
    first.await??;
    assert!(worker.await?.is_err());
    assert!(second.await?.is_err());
    assert_eq!(backend.runs.load(Ordering::SeqCst), 1);
    assert_eq!(backend.configured.load(Ordering::SeqCst), 1);
    assert_eq!(q.recovery_status("first")["outcome"], "submitted");
    assert_eq!(q.recovery_status("second")["outcome"], "not_submitted");
    assert!(
        fs::read_to_string(root.path().join("events.log"))?.contains("browser_cleanup_unverified")
    );
    backend.drain().await?;
    Ok(())
}
#[tokio::test]
async fn worker_abort_drops_lease_even_when_queue_and_backend_handles_outlive_it() -> Result<()> {
    for sent in [false, true] {
        let root = tempfile::tempdir()?;
        let profile = profile()?;
        let backend = Arc::new(Synthetic::new(profile.path(), Some(sent)));
        let (_stop, rx) = watch::channel(false);
        let (q, worker) = Queue::open(root.path().into(), backend.clone(), rx)?;
        let _receipt = q.accept_before("cancelled".into(), task(), Config::default(), None)?;
        tokio::time::timeout(Duration::from_secs(4), backend.reached.notified()).await?;
        worker.abort();
        assert!(worker.await.is_err());
        assert!(
            backend
                .leases
                .lock()
                .unwrap()
                .iter()
                .all(|lease| lease.upgrade().is_none())
        );
        backend.drain().await?;
        let rows: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join("journal.json"))?)?;
        assert_eq!(rows[0]["submission_intent"], sent);
        assert_eq!(
            launch_coordinator::read_receipt(root.path())?
                .unwrap()
                .state,
            State::Authorized
        );
        assert!(
            Session::prepare(
                root.path(),
                GuardConfig {
                    guard_executable: std::env::current_exe()?,
                    browser_executable: "/fixture/no-browser".into()
                }
            )
            .is_err()
        );
    }
    Ok(())
}
#[test]
fn lease_is_runtime_only_and_cannot_be_set_via_config_json() -> Result<()> {
    let value = serde_json::to_value(Config::default())?;
    assert!(value.get("browser_session").is_none());
    let mut malicious = value;
    malicious["browser_session"] = serde_json::json!({"generation":"untrusted"});
    assert!(serde_json::from_value::<Config>(malicious).is_err());
    Ok(())
}
#[cfg(feature = "owned-launch-candidate")]
#[tokio::test]
async fn unavailable_release_capability_refuses_before_any_browser_execution() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_stop, rx) = watch::channel(false);
    let (q, worker) = Queue::open(root.path().into(), Arc::new(slingring::queue::Browser), rx)?;
    let config = Config {
        browser_binary: "/fixture/must-not-run".into(),
        ..Config::default()
    };
    let result = q.submit("unsupported".into(), task(), config).await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("capability_unavailable")
    );
    assert!(worker.await?.is_err());
    assert_eq!(q.recovery_status("unsupported")["outcome"], "not_submitted");
    assert!(launch_coordinator::read_receipt(root.path())?.is_none());
    Ok(())
}

#[tokio::test]
async fn stop_and_deadline_keep_submission_outcome_separate_from_limited_cleanup() -> Result<()> {
    for deadline in [false, true] {
        for sent in [false, true] {
            let root = tempfile::tempdir()?;
            let profile = profile()?;
            let backend = Arc::new(Synthetic::new(profile.path(), Some(sent)));
            let (stop, rx) = watch::channel(false);
            let (q, worker) = Queue::open(root.path().into(), backend.clone(), rx)?;
            let config = Config {
                start_timeout_secs: 1,
                ..Config::default()
            };
            let receipt = q.accept_before("interrupted".into(), task(), config, None)?;
            tokio::time::timeout(Duration::from_secs(3), backend.reached.notified()).await?;
            if !deadline {
                stop.send(true)?;
            }
            assert!(
                tokio::time::timeout(Duration::from_secs(3), worker)
                    .await??
                    .is_err()
            );
            assert!(receipt.await?.is_err());
            assert_eq!(
                q.recovery_status("interrupted")["outcome"],
                if sent { "uncertain" } else { "not_submitted" }
            );
            assert_eq!(
                launch_coordinator::read_receipt(root.path())?
                    .unwrap()
                    .state,
                State::RootReapedTreeUnverified
            );
            assert!(
                q.submit("interrupted".into(), task(), Config::default())
                    .await
                    .is_err()
            );
            assert_eq!(backend.runs.load(Ordering::SeqCst), 1);
            backend.drain().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn closing_during_authorization_cannot_restore_active_or_approve_a_waiting_peer() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let session = Session::prepare(
        root.path(),
        GuardConfig {
            guard_executable: std::env::current_exe()?,
            browser_executable: "/fixture/no-browser".into(),
        },
    )?;
    let env = session.environment();
    let socket = &env
        .iter()
        .find(|(k, _)| *k == "SLINGRING_PRIVATE_LAUNCH_SOCKET")
        .unwrap()
        .1;
    let mut peer = UnixStream::connect(socket).await?;
    peer.write_u32(32).await?; // Hold the handshake before any approval barrier.
    let authorizing = session.clone();
    let task = tokio::spawn(async move {
        authorizing
            .authorize(tokio::time::Instant::now() + Duration::from_secs(2))
            .await
    });
    tokio::task::yield_now().await;
    assert!(session.close().await.is_err());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), task)
            .await??
            .is_err()
    );
    assert!(!session.is_active());
    assert!(session.close().await.is_err());
    assert!(session.check_command(true).is_err());
    assert!(peer.read_u8().await.is_err());
    assert!(launch_coordinator::read_receipt(root.path())?.is_none());
    Ok(())
}
