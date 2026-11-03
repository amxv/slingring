use anyhow::Result;
use serde_json::{Value, json};
use slingring::{
    config::Config,
    model::{NewTask, StartError, Task, TaskResult},
    queue::{Backend, BrowserFuture, Queue},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    sync::{Notify, Semaphore, watch},
    time::{Duration, Instant},
};

struct FakeBrowser {
    calls: AtomicUsize,
    began: Notify,
    permits: Semaphore,
    fail: bool,
}
impl FakeBrowser {
    fn new(fail: bool) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            began: Notify::new(),
            permits: Semaphore::new(0),
            fail,
        }
    }
}
impl Backend for FakeBrowser {
    fn run<'a>(
        &'a self,
        _: &'a Config,
        task: &'a Task,
        id: &'a str,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.began.notify_one();
            self.permits.acquire().await?.forget();
            if self.fail {
                return Err(StartError {
                    code: "chat_mode_unverified".into(),
                    may_have_submitted: false,
                    url: None,
                }
                .into());
            }
            Ok(TaskResult {
                job_id: id.into(),
                url: format!("https://chatgpt.com/c/{id}"),
                reasoning_effort: task.reasoning_effort().map(str::to_string),
                evidence: "thinking".into(),
            })
        })
    }
    fn run_with_progress<'a>(
        &'a self,
        config: &'a Config,
        task: &'a Task,
        id: &'a str,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            progress("browser_launch");
            progress("native_handshake");
            progress("chat_mode_verification");
            self.run(config, task, id).await
        })
    }
    fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn task(message: &str) -> NewTask {
    NewTask {
        prompt: message.into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
}
fn read_status(root: &std::path::Path, request_id: &str) -> Value {
    let req = json!({"jsonrpc":"2.0","id":17,"method":"tools/call",
        "params":{"name":"get_job_status","arguments":{"request_id":request_id}}});
    slingring::mcp::status_response(root, &req)
}

#[tokio::test]
async fn caller_timeout_then_late_success_is_readable_without_resubmission() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let id = "remote-first";
    let q = queue.clone();
    let caller = tokio::spawn(async move {
        q.submit_before(
            id.into(),
            task("PRIVATE first prompt"),
            Config::default(),
            None,
        )
        .await
    });
    backend.began.notified().await;
    let during = read_status(dir.path(), id);
    assert_eq!(
        during["result"]["structuredContent"]["outcome"],
        "in_progress"
    );
    assert_eq!(
        during["result"]["structuredContent"]["job"]["phase"],
        "chat_mode_verification"
    );
    // Simulates a caller-side HTTP/MCP timeout after the job started.
    caller.abort();
    backend.permits.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !queue.quiet() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let finished = read_status(dir.path(), id);
    assert_eq!(
        finished["result"]["structuredContent"]["outcome"],
        "submitted"
    );
    assert_eq!(
        finished["result"]["structuredContent"]["job"]["result"]["url"],
        format!("https://chatgpt.com/c/{id}")
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let result = queue
        .submit(id.into(), task("PRIVATE first prompt"), Config::default())
        .await?;
    assert_eq!(result.job_id, id);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(
        queue
            .submit(id.into(), task("DIFFERENT prompt"), Config::default())
            .await
            .is_err()
    );
    let mismatched = json!({"jsonrpc":"2.0","id":19,"method":"tools/call","params":{
        "name":"new_task","arguments":{"prompt":"DIFFERENT prompt","request_id":id}}});
    let rejected = slingring::mcp::dispatch(dir.path(), &queue, mismatched, id.to_owned(), None)
        .await
        .unwrap();
    assert_eq!(
        rejected["result"]["structuredContent"]["outcome"],
        "rejected"
    );
    assert!(rejected["result"]["structuredContent"]["job"].is_null());
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let journal = std::fs::read_to_string(dir.path().join("journal.json"))?;
    let events = std::fs::read_to_string(dir.path().join("events.log"))?;
    for output in [&journal, &events] {
        assert!(!output.contains("PRIVATE first prompt"));
        assert!(!output.contains("DIFFERENT prompt"));
    }
    assert!(events.contains("stage=browser_launch"));
    assert!(events.contains("stage=chat_mode_verification"));
    assert!(events.contains("stage=finished status=started"));
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn known_pre_submission_failure_and_unknown_history_have_distinct_recovery() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(true));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let id = "failed-job";
    let q = queue.clone();
    let request = tokio::spawn(async move {
        q.submit(id.into(), task("PRIVATE failed prompt"), Config::default())
            .await
    });
    backend.began.notified().await;
    backend.permits.add_permits(1);
    assert!(request.await?.is_err());
    let status = read_status(dir.path(), id)["result"]["structuredContent"].clone();
    assert_eq!(status["outcome"], "not_submitted");
    assert_eq!(status["job"]["status"], "failed");
    assert!(status["diagnosis"].as_str().unwrap().contains("Chat mode"));
    let unknown = read_status(dir.path(), "pruned-id")["result"]["structuredContent"].clone();
    assert_eq!(unknown["known"], false);
    assert_eq!(unknown["outcome"], "unknown");
    assert!(unknown["guidance"].as_str().unwrap().contains("NOT proof"));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let events = std::fs::read_to_string(dir.path().join("events.log"))?;
    assert!(events.contains("detail=chat_mode_unverified"));
    assert!(!events.contains("PRIVATE failed prompt"));
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[test]
fn status_is_strict_and_request_id_cannot_override_a_different_http_key() {
    let dir = tempfile::tempdir().unwrap();
    let malformed = json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{
        "name":"get_job_status", "arguments":{"request_id":"../invalid"}}});
    assert_eq!(
        slingring::mcp::status_response(dir.path(), &malformed)["error"]["code"],
        -32602
    );
    let extra = json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{
        "name":"get_job_status", "arguments":{"request_id":"ok","prompt":"unexpected"}}});
    assert_eq!(
        slingring::mcp::status_response(dir.path(), &extra)["error"]["code"],
        -32602
    );
    assert!(!dir.path().join("journal.json").exists());
}

#[test]
fn post_authorization_composer_errors_do_not_claim_an_uncertain_job_was_unsent() {
    for code in ["composer_unavailable", "prompt_lost"] {
        let job: slingring::queue::Job = serde_json::from_value(json!({
            "id":"after-ack","status":"uncertain","created_at":1,"finished_at":2,
            "result":null,"error":format!("{code}; submission may have started"),"url":null
        }))
        .unwrap();
        let status = slingring::queue::recovery_status(Some(&job), "after-ack");
        assert_eq!(status["outcome"], "uncertain");
        let diagnosis = status["diagnosis"].as_str().unwrap().to_ascii_lowercase();
        assert!(!diagnosis.contains("nothing was submitted"));
        assert!(!diagnosis.contains("no message was submitted"));
        assert!(diagnosis.contains("recorded outcome"));
    }
}

#[tokio::test]
async fn mismatched_idempotency_header_rejects_before_admission() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{
        "name":"new_task", "arguments":{"prompt":"PRIVATE mismatch", "request_id":"expected"}}});
    let response = slingring::mcp::dispatch(dir.path(), &queue, request, "different".into(), None)
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    assert!(queue.snapshot().is_empty());
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn expired_before_launch_is_known_safe_without_running_backend() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let request = queue
        .submit_before(
            "expired".into(),
            task("unsubmitted"),
            Config::default(),
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await;
    assert!(request.is_err());
    assert_eq!(
        read_status(dir.path(), "expired")["result"]["structuredContent"]["outcome"],
        "not_submitted"
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn crashed_owner_is_classified_without_restarting_or_mutating_history() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (_stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let starting = queue.accept_before(
        "interrupted-start".into(),
        task("PRIVATE running").into(),
        Config::default(),
        None,
    )?;
    backend.began.notified().await;
    let queued = queue.accept_before(
        "interrupted-queue".into(),
        task("PRIVATE queued").into(),
        Config::default(),
        None,
    )?;
    assert!(slingring::queue::owner_running(dir.path())?);
    assert_eq!(
        read_status(dir.path(), "interrupted-start")["result"]["structuredContent"]["outcome"],
        "in_progress"
    );
    // Abort the owner as a crash would, without the orderly shutdown handler.
    worker.abort();
    let _ = worker.await;
    drop(queue);
    drop(starting);
    drop(queued);
    assert!(!slingring::queue::owner_running(dir.path())?);
    let before = std::fs::read(dir.path().join("journal.json"))?;
    assert_eq!(
        read_status(dir.path(), "interrupted-start")["result"]["structuredContent"]["outcome"],
        "uncertain"
    );
    assert_eq!(
        read_status(dir.path(), "interrupted-queue")["result"]["structuredContent"]["outcome"],
        "not_submitted"
    );
    for request_id in ["interrupted-start", "interrupted-queue"] {
        let job = read_status(dir.path(), request_id)["result"]["structuredContent"]["job"].clone();
        assert!(job["finished_at"].is_null());
        assert!(job["elapsed_ms"].is_null());
        assert!(job["created_at"].as_u64().is_some());
    }
    assert_eq!(std::fs::read(dir.path().join("journal.json"))?, before);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(!dir.path().join("slingring.sock").exists());
    Ok(())
}

#[tokio::test]
async fn one_queue_owner_and_retained_mcp_admission_survive_lost_replies() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    assert!(Queue::open(dir.path().into(), backend.clone(), stop.subscribe()).is_err());
    let first = queue.accept_before(
        "first-accepted".into(),
        task("PRIVATE one").into(),
        Config::default(),
        None,
    )?;
    backend.began.notified().await;
    let next = queue.accept_before(
        "queued-accepted".into(),
        task("PRIVATE two").into(),
        Config::default(),
        None,
    )?;
    drop(first);
    drop(next);
    backend.permits.add_permits(2);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !queue.quiet() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        read_status(dir.path(), "queued-accepted")["result"]["structuredContent"]["outcome"],
        "submitted"
    );
    queue
        .submit(
            "queued-accepted".into(),
            task("PRIVATE two"),
            Config::default(),
        )
        .await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn slow_mcp_returns_a_bounded_receipt_then_status_reports_late_success() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let request = json!({"jsonrpc":"2.0","id":91,"method":"tools/call","params":{
        "name":"new_task","arguments":{"prompt":"PRIVATE slow MCP", "request_id":"bounded-receipt"}}});
    let response = tokio::time::timeout(
        slingring::mcp::RECEIPT_WAIT + Duration::from_secs(1),
        slingring::mcp::dispatch(dir.path(), &queue, request, "bounded-receipt".into(), None),
    )
    .await?
    .unwrap();
    let details = &response["result"]["structuredContent"];
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(details["outcome"], "in_progress");
    assert_eq!(details["accepted"], true);
    assert_eq!(details["request_id"], "bounded-receipt");
    assert_eq!(details["recovery_tool"], "get_job_status");
    assert!(details.get("url").is_none());
    backend.permits.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !queue.quiet() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(
        read_status(dir.path(), "bounded-receipt")["result"]["structuredContent"]["outcome"],
        "submitted"
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn saturated_queue_keeps_status_available_and_rejects_new_admission_honestly() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    for index in 0..32 {
        drop(queue.accept_before(
            format!("full-{index}"),
            task("PRIVATE fill queue").into(),
            Config::default(),
            None,
        )?);
    }
    backend.began.notified().await;
    assert_eq!(queue.active(), 32);
    let lookup = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"get_job_status","arguments":{"request_id":"full-0"}}});
    let response = tokio::time::timeout(
        Duration::from_millis(200),
        slingring::mcp::dispatch(dir.path(), &queue, lookup, "lookup-key".into(), None),
    )
    .await?
    .unwrap();
    assert_eq!(
        response["result"]["structuredContent"]["outcome"],
        "in_progress"
    );
    let extra = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"new_task","arguments":{"prompt":"PRIVATE overflow","request_id":"overflow"}}});
    let rejected = slingring::mcp::dispatch(dir.path(), &queue, extra, "overflow".into(), None)
        .await
        .unwrap();
    assert_eq!(
        rejected["result"]["structuredContent"]["outcome"],
        "not_submitted"
    );
    assert_eq!(rejected["result"]["structuredContent"]["accepted"], false);
    assert_eq!(queue.active(), 32);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn an_expired_duplicate_recovers_previous_success_instead_of_claiming_unsent() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(FakeBrowser::new(false));
    backend.permits.add_permits(1);
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    queue
        .submit("already-sent".into(), task("same input"), Config::default())
        .await?;
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"new_task","arguments":{"prompt":"same input","request_id":"already-sent"}}});
    let response =
        slingring::mcp::dispatch(dir.path(), &queue, request, "already-sent".into(), Some(1))
            .await
            .unwrap();
    assert_eq!(
        response["result"]["structuredContent"]["outcome"],
        "submitted"
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

struct SlowCleanup {
    began: tokio::sync::Notify,
    gate: tokio::sync::Semaphore,
    fail: bool,
}
impl Backend for SlowCleanup {
    fn run<'a>(
        &'a self,
        _: &'a Config,
        task: &'a Task,
        id: &'a str,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            if self.fail {
                anyhow::bail!("PRIVATE external page error must never reach journal");
            }
            Ok(TaskResult {
                job_id: id.into(),
                url: format!("https://chatgpt.com/c/{id}"),
                reasoning_effort: task.reasoning_effort().map(str::to_string),
                evidence: "thinking".into(),
            })
        })
    }
    fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.began.notify_one();
            self.gate.acquire().await.unwrap().forget();
            Ok(())
        })
    }
}

#[tokio::test]
async fn proven_outcomes_are_published_before_slow_cleanup_and_raw_errors_stay_private()
-> Result<()> {
    for fail in [false, true] {
        let dir = tempfile::tempdir()?;
        let backend = Arc::new(SlowCleanup {
            began: tokio::sync::Notify::new(),
            gate: tokio::sync::Semaphore::new(0),
            fail,
        });
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            queue.submit(
                "before-cleanup".into(),
                task("PRIVATE fixture input"),
                Config::default(),
            ),
        )
        .await?;
        assert_eq!(result.is_err(), fail);
        backend.began.notified().await;
        assert!(!queue.quiet());
        assert_eq!(queue.active(), 0);
        assert!(slingring::queue::owner_running(dir.path())?);
        let status =
            read_status(dir.path(), "before-cleanup")["result"]["structuredContent"].clone();
        assert_eq!(
            status["outcome"],
            if fail { "uncertain" } else { "submitted" }
        );
        let journal = std::fs::read_to_string(dir.path().join("journal.json"))?;
        assert!(!journal.contains("PRIVATE"));
        backend.gate.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !queue.quiet() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        stop.send(true)?;
        worker.await??;
        assert!(!slingring::queue::owner_running(dir.path())?);
    }
    Ok(())
}
