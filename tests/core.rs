use anyhow::Result;
use serde_json::{Value, json};
use slingring::{
    config::Config,
    model::{NewTask, Task, TaskResult, effort},
    queue::{Backend, BrowserFuture, Queue},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Notify, Semaphore, watch};

fn task(prompt: &str) -> NewTask {
    NewTask {
        prompt: prompt.into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
}
struct Mock {
    calls: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    closes: AtomicUsize,
    started: Notify,
    closed: Notify,
    gate: Semaphore,
}
impl Mock {
    fn new(permits: usize) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
            started: Notify::new(),
            closed: Notify::new(),
            gate: Semaphore::new(permits),
        }
    }
}
impl Backend for Mock {
    fn run<'a>(&'a self, _: &'a Config, t: &'a Task, id: &'a str) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.started.notify_one();
            self.gate.acquire().await.unwrap().forget();
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(TaskResult {
                pause_status: None,
                requested_effort: None,
                verified_effort: None,
                effort_verification: Default::default(),
                job_id: id.into(),
                url: format!("https://chatgpt.com/c/{id}"),
                reasoning_effort: t.reasoning_effort().map(str::to_string),
                evidence: "thinking".into(),
            })
        })
    }
    fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.closed.notify_one();
            Ok(())
        })
    }
}
#[test]
fn effort_aliases_and_blank_default_are_explicit() {
    for (input, expected) in [
        ("instant", "Instant"),
        ("med", "Medium"),
        ("HIGH", "High"),
        ("xhigh", "Extra High"),
        ("extra-high", "Extra High"),
        ("pro", "Pro"),
    ] {
        assert_eq!(effort(input).unwrap(), expected);
    }
    assert!(effort("maximum").is_err());
    let mut t = task("prompt");
    t.reasoning_effort = Some(" ".into());
    assert_eq!(
        t.resolve(&Config::default())
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("High")
    );
    let c = Config {
        default_effort: None,
        ..Config::default()
    };
    assert!(
        task("prompt")
            .resolve(&c)
            .unwrap()
            .reasoning_effort
            .is_none()
    );
}

#[test]
fn outbound_tls_has_an_explicit_crypto_provider() {
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    assert!(!config.crypto_provider().cipher_suites.is_empty());
}

#[test]
fn runtime_browser_lease_is_not_persisted_as_user_configuration() {
    let c = Config {
        browser_idle_secs: Some(3660),
        ..Config::default()
    };
    assert!(
        serde_json::to_value(&c)
            .unwrap()
            .get("browser_idle_secs")
            .is_none()
    );
    assert!(
        serde_json::from_value::<Config>(serde_json::to_value(c).unwrap())
            .unwrap()
            .browser_idle_secs
            .is_none()
    );
}
#[test]
fn remote_schema_and_validation_do_not_accept_files_or_unknown_fields() {
    let tool = slingring::mcp::tool();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        tool["inputSchema"]["properties"].as_object().unwrap().len(),
        3
    );
    for key in ["image_paths", "images", "attachments", "profile", "model"] {
        let request =
            json!({"params":{"name":"new_task","arguments":{"prompt":"p",key:["/private/file"]}}});
        assert!(slingring::mcp::arguments(&request).is_err());
    }
    let notification = json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"new_task","arguments":{"prompt":"p"}}});
    assert_eq!(slingring::mcp::metadata(&notification), Some(None));
}
#[test]
fn mcp_negotiates_supported_versions_and_rejects_malformed_framing() {
    for version in slingring::mcp::VERSIONS {
        let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version}});
        let response = slingring::mcp::metadata(&request).unwrap().unwrap();
        assert_eq!(response["result"]["protocolVersion"], *version);
    }
    assert!(
        slingring::mcp::metadata(&json!([]))
            .unwrap()
            .unwrap()
            .get("error")
            .is_some()
    );
}
#[test]
fn results_require_real_evidence_and_a_canonical_chat_url() {
    let mut r = TaskResult {
        pause_status: None,
        requested_effort: None,
        verified_effort: None,
        effort_verification: Default::default(),
        job_id: "x".into(),
        url: "https://chatgpt.com/c/test-id".into(),
        reasoning_effort: Some("Extra High".into()),
        evidence: "thinking".into(),
    };
    assert!(r.validate().is_ok());
    for url in [
        "https://chatgpt.com/work/123",
        "https://evil.example/c/123",
        "https://chatgpt.com/c/123?q=secret",
    ] {
        r.url = url.into();
        assert!(r.validate().is_err());
    }
    r.url = "https://chatgpt.com/c/test".into();
    for evidence in ["stop_button", "loading", "", "assistant_placeholder"] {
        r.evidence = evidence.into();
        assert!(r.validate().is_err());
    }
}
#[tokio::test]
async fn starts_are_serial_browser_is_reused_and_idempotency_is_payload_bound() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(0));
    let (stop, rx) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
    let q = queue.clone();
    let first = tokio::spawn(async move {
        q.submit(
            "first".into(),
            task("secret first prompt"),
            Config::default(),
        )
        .await
    });
    backend.started.notified().await;
    let q = queue.clone();
    let second = tokio::spawn(async move {
        q.submit(
            "second".into(),
            task("secret second prompt"),
            Config::default(),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while queue.active() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    backend.gate.add_permits(2);
    first.await??;
    second.await??;
    backend.closed.notified().await;
    assert_eq!(backend.peak.load(Ordering::SeqCst), 1);
    assert_eq!(backend.closes.load(Ordering::SeqCst), 1);
    queue
        .submit(
            "first".into(),
            task("secret first prompt"),
            Config::default(),
        )
        .await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert!(
        queue
            .submit("first".into(), task("different"), Config::default())
            .await
            .is_err()
    );
    let journal = std::fs::read_to_string(dir.path().join("journal.json"))?;
    assert!(!journal.contains("secret first prompt") && !journal.contains("secret second prompt"));
    stop.send(true)?;
    worker.await??;
    Ok(())
}
#[tokio::test]
async fn abandoned_queued_calls_never_launch() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(0));
    let (stop, rx) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
    let q = queue.clone();
    let first = tokio::spawn(async move {
        q.submit("first".into(), task("first"), Config::default())
            .await
    });
    backend.started.notified().await;
    let q = queue.clone();
    let abandoned = tokio::spawn(async move {
        q.submit("abandoned".into(), task("never send"), Config::default())
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while queue.active() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    abandoned.abort();
    let _ = abandoned.await;
    backend.gate.add_permits(2);
    first.await??;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while queue.active() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !queue.quiet() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(backend.closes.load(Ordering::SeqCst), 1);
    stop.send(true)?;
    worker.await??;
    Ok(())
}
#[tokio::test]
async fn queued_browser_mode_or_profile_changes_close_only_between_jobs() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(0));
    let (stop, rx) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
    let first = queue.accept_before(
        "mode-first".into(),
        task("one").into(),
        Config::default(),
        None,
    )?;
    backend.started.notified().await;
    let headed = Config {
        browser_headed: true,
        ..Config::default()
    };
    let second = queue.accept_before(
        "mode-second".into(),
        task("two").into(),
        headed.clone(),
        None,
    )?;
    let other_profile = Config {
        profile: "Another Profile".into(),
        ..headed
    };
    let third = queue.accept_before(
        "mode-third".into(),
        task("three").into(),
        other_profile,
        None,
    )?;
    assert_eq!(backend.closes.load(Ordering::SeqCst), 0);
    backend.gate.add_permits(3);
    first.await??;
    second.await??;
    third.await??;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !queue.quiet() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(backend.peak.load(Ordering::SeqCst), 1);
    assert_eq!(backend.closes.load(Ordering::SeqCst), 3);
    stop.send(true)?;
    worker.await??;
    Ok(())
}
#[tokio::test]
async fn crash_recovery_does_not_replay_queued_or_starting_jobs() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let rows = vec![
        json!({"id":"queued","status":"queued","created_at":1,"finished_at":null,"result":null,"error":null,"url":null}),
        json!({"id":"starting","status":"starting","created_at":2,"finished_at":null,"result":null,"error":null,"url":null}),
    ];
    slingring::config::atomic(
        &dir.path().join("journal.json"),
        &serde_json::to_vec(&rows)?,
    )?;
    let backend = Arc::new(Mock::new(2));
    let (stop, rx) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
    let jobs = queue.snapshot();
    assert_eq!(jobs[0]["status"], "failed");
    assert_eq!(jobs[1]["status"], "uncertain");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    stop.send(true)?;
    worker.await??;
    Ok(())
}
#[test]
fn native_extension_identity_and_manifest_are_consistent() {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let manifest: Value = serde_json::from_str(include_str!("../extension/manifest.json")).unwrap();
    let key = base64::engine::general_purpose::STANDARD
        .decode(manifest["key"].as_str().unwrap())
        .unwrap();
    let hash = format!("{:x}", Sha256::digest(key));
    let id: String = hash[..32]
        .chars()
        .map(|c| char::from(b'a' + c.to_digit(16).unwrap() as u8))
        .collect();
    assert_eq!(id, slingring::native::EXTENSION_ID);
    assert!(manifest.get("background").is_none());
    assert_eq!(
        manifest["host_permissions"],
        json!(["https://chatgpt.com/*"])
    );
}

#[test]
fn request_ids_are_compatible_across_cli_and_mcp() {
    for id in ["a".to_string(), "cli:stable_1-2".into(), "a".repeat(128)] {
        assert!(slingring::queue::valid_job_id(&id));
        assert!(slingring::mcp::valid_request_id(&id));
        for tool in slingring::mcp::tools() {
            if tool["name"] == "pause_task" {
                continue;
            }
            assert_eq!(
                tool["inputSchema"]["properties"]["request_id"]["maxLength"],
                128
            );
        }
    }
    for id in [
        "".to_string(),
        "a".repeat(129),
        "é".into(),
        "x\ny".into(),
        "a/b".into(),
        "a b".into(),
    ] {
        assert!(!slingring::queue::valid_job_id(&id));
        assert!(!slingring::mcp::valid_request_id(&id));
    }
}

#[test]
fn effort_metadata_is_explicit_and_legacy_results_remain_readable() -> Result<()> {
    let mut legacy: TaskResult = serde_json::from_value(json!({
        "url":"https://chatgpt.com/c/legacy", "reasoning_effort":"High", "evidence":"thinking"
    }))?;
    assert_eq!(legacy.requested_effort, None);
    // Legacy data cannot prove which effort the caller requested.
    assert_eq!(
        serde_json::to_value(&legacy)?["effort_verification"],
        "unverified"
    );
    legacy.record_effort(Some("High"));
    let verified = serde_json::to_value(&legacy)?;
    assert_eq!(verified["requested_effort"], "High");
    assert_eq!(verified["verified_effort"], "High");
    assert_eq!(verified["reasoning_effort"], "High");
    assert_eq!(verified["effort_verification"], "verified");
    legacy.reasoning_effort = None;
    legacy.record_effort(Some("Pro"));
    let unverified = serde_json::to_value(&legacy)?;
    assert_eq!(unverified["requested_effort"], "Pro");
    assert_eq!(unverified["verified_effort"], Value::Null);
    assert_eq!(unverified["effort_verification"], "unverified");
    legacy.record_effort(None);
    assert_eq!(
        serde_json::to_value(&legacy)?["effort_verification"],
        "not_requested"
    );
    Ok(())
}
