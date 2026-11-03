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
