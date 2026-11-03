use anyhow::Result;
use serde_json::{Value, json};
use slingring::{
    config::Config,
    model::{MAX_PROMPT, NewTask, SendMessage, StartError, Task, TaskResult},
    queue::{Backend, BrowserFuture, Queue},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Notify, Semaphore, watch},
    time::Instant,
};

const CHAT: &str = "https://chatgpt.com/c/existing-chat";

fn message(text: &str) -> SendMessage {
    SendMessage {
        chatgpt_url: CHAT.into(),
        message: text.into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
}

fn new_task(text: &str) -> NewTask {
    NewTask {
        prompt: text.into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
}

fn request(arguments: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"send_message","arguments":arguments}})
}

#[test]
fn followup_tool_keeps_strict_fields_plus_explicit_recovery_id() {
    let tool = slingring::mcp::message_tool();
    assert_eq!(tool["name"], "send_message");
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    let fields = tool["inputSchema"]["properties"].as_object().unwrap();
    assert_eq!(fields.len(), 4);
    assert!(
        fields.contains_key("chatgpt_url")
            && fields.contains_key("message")
            && fields.contains_key("request_id")
            && fields.contains_key("reasoning_effort")
    );
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["annotations"]["openWorldHint"], false);
    assert_eq!(tool["annotations"]["idempotentHint"], false);

    let listing = slingring::mcp::metadata(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .unwrap()
        .unwrap();
    let names: Vec<_> = listing["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["new_task", "send_message", "get_job_status"]);

    for field in [
        "prompt",
        "runtime_flags",
        "image_paths",
        "images",
        "profile",
        "model",
    ] {
        let input = request(json!({"chatgpt_url":CHAT,"message":"hello",field:"unexpected"}));
        assert!(slingring::mcp::arguments(&input).is_err(), "{field}");
    }
    for input in [
        json!(null),
        json!([]),
        json!({"chatgpt_url":CHAT}),
        json!({"message":"hello"}),
        json!({"chatgpt_url":null,"message":"hello"}),
        json!({"chatgpt_url":123,"message":"hello"}),
        json!({"chatgpt_url":CHAT,"message":["hello"]}),
        json!({"chatgpt_url":CHAT,"message":null}),
    ] {
        assert!(slingring::mcp::arguments(&request(input)).is_err());
    }
    let parsed = slingring::mcp::arguments(&request(json!({
        "chatgpt_url":CHAT,"message":"hello"
    })))
    .unwrap();
    assert!(matches!(parsed, Task::Message(_)));
    let mut notification = request(json!({"chatgpt_url":CHAT,"message":"hello"}));
    notification.as_object_mut().unwrap().remove("id");
    assert_eq!(slingring::mcp::metadata(&notification), Some(None));
}

#[test]
fn followups_require_an_exact_canonical_private_conversation_url() {
    for target in [
        "",
        "https://chatgpt.com/",
        "http://chatgpt.com/c/id",
        "https://chat.openai.com/c/id",
        "https://chatgpt.com.evil.example/c/id",
        "https://chatgpt.com@evil.example/c/id",
        "https://user@chatgpt.com/c/id",
        "https://chatgpt.com:443/c/id",
        "https://chatgpt.com/c/",
        "https://chatgpt.com/c/id/",
        "https://chatgpt.com/c/id?model=other",
        "https://chatgpt.com/c/id#fragment",
        "https://chatgpt.com/c/id\n",
        " https://chatgpt.com/c/id",
        "https://chatgpt.com/c/%69d",
        "https://chatgpt.com/c/id/../other",
        "https://chatgpt.com/c/id\\other",
        "https://chatgpt.com/c/id_other",
        "https://chatgpt.com/c/你好",
        "https://chatgpt.com/share/id",
        "https://chatgpt.com/work/id",
        "https://chatgpt.com/g/g-project/c/id",
        "javascript:alert(1)",
        "file:///tmp/chat",
    ] {
        let mut followup = message("hello");
        followup.chatgpt_url = target.into();
        assert!(
            followup.resolve(&Config::default()).is_err(),
            "accepted {target:?}"
        );
    }
    for id in ["a".into(), "Ab09-cD".into(), "a".repeat(128)] {
        let mut followup = message("hello");
        followup.chatgpt_url = format!("https://chatgpt.com/c/{id}");
        assert!(followup.resolve(&Config::default()).is_ok());
    }
    let mut too_long = message("hello");
    too_long.chatgpt_url = format!("https://chatgpt.com/c/{}", "a".repeat(129));
    assert!(too_long.resolve(&Config::default()).is_err());
}

#[test]
fn message_limits_and_omitted_effort_leave_existing_chat_unchanged() -> Result<()> {
    for text in ["", " \t\r\n"] {
        assert!(message(text).resolve(&Config::default()).is_err());
    }
    assert!(
        message(&"x".repeat(MAX_PROMPT))
            .resolve(&Config::default())
            .is_ok()
    );
    assert!(
        message(&"x".repeat(MAX_PROMPT + 1))
            .resolve(&Config::default())
            .is_err()
    );
    assert!(
        message(&"é".repeat(MAX_PROMPT / 2))
            .resolve(&Config::default())
            .is_ok()
    );
    assert!(
        message(&"é".repeat(MAX_PROMPT / 2 + 1))
            .resolve(&Config::default())
            .is_err()
    );
    let config = Config {
        default_effort: Some("Pro".into()),
        ..Config::default()
    };
    let task = Task::from(message("  Keep literal whitespace.\n")).resolve(&config)?;
    assert_eq!(task.prompt(), "  Keep literal whitespace.\n");
    assert_eq!(task.reasoning_effort(), None);
    assert_eq!(task.conversation_url(), Some(CHAT));
    assert!(task.image_paths().is_empty());
    Ok(())
}

#[test]
fn explicit_followup_effort_is_opt_in_and_part_of_idempotency() -> Result<()> {
    let config = Config {
        default_effort: Some("Pro".into()),
        ..Config::default()
    };
    let plain = Task::from(message("one follow-up")).resolve(&config)?;
    assert_eq!(plain.reasoning_effort(), None);
    let mut explicit = message("one follow-up");
    explicit.reasoning_effort = Some("xhigh".into());
    let chosen = Task::from(explicit.clone()).resolve(&config)?;
    assert_eq!(chosen.reasoning_effort(), Some("Extra High"));
    assert_ne!(
        plain.fingerprint("Profile 6")?,
        chosen.fingerprint("Profile 6")?
    );
    let mut invalid = message("one follow-up");
    invalid.reasoning_effort = Some("Ultra".into());
    assert!(Task::from(invalid).resolve(&config).is_err());
    // Follow-up effort is best-effort. A missing picker must not invalidate a
    // confirmed URL and fresh activity *after* the message was submitted.
    chosen.validate_result(&TaskResult {
        job_id: "chosen".into(),
        url: CHAT.into(),
        reasoning_effort: None,
        evidence: "thinking".into(),
    })?;
    let parsed = slingring::mcp::arguments(&request(json!({
        "chatgpt_url":CHAT,"message":"one follow-up","reasoning_effort":"High"
    })))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(parsed.resolve(&config)?.reasoning_effort(), Some("High"));
    Ok(())
}

#[test]
fn local_followup_images_are_validated_and_bound_to_request_fingerprints() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let image = dir.path().join("sample.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\nfirst")?;
    let mut with_image = message("Describe this attachment");
    with_image.image_paths.push(image.clone());
    let task = Task::from(with_image.clone()).resolve(&Config::default())?;
    assert_eq!(task.image_paths(), &[image.canonicalize()?]);
    assert_eq!(task.conversation_url(), Some(CHAT));
    assert_eq!(task.reasoning_effort(), None);
    assert_ne!(
        task.fingerprint("Profile 6")?,
        Task::from(message("Describe this attachment")).fingerprint("Profile 6")?
    );
    let original = task.fingerprint("Profile 6")?;
    std::fs::write(&image, b"\x89PNG\r\n\x1a\nsecond")?;
    assert_ne!(original, task.fingerprint("Profile 6")?);
    let mut too_many = with_image.clone();
    too_many.image_paths = vec![image.clone(); 17];
    assert!(too_many.resolve(&Config::default()).is_err());
    let mut malformed = with_image;
    std::fs::write(&image, b"not actually an image")?;
    malformed.image_paths = vec![image];
    assert!(malformed.resolve(&Config::default()).is_err());
    Ok(())
}

#[test]
fn text_only_followup_serialization_remains_backward_compatible() -> Result<()> {
    let serialized = serde_json::to_value(message("same text"))?;
    assert_eq!(
        serialized,
        json!({"chatgpt_url":CHAT,"message":"same text"})
    );
    let decoded: SendMessage = serde_json::from_value(serialized)?;
    assert!(decoded.image_paths.is_empty());
    Ok(())
}

#[test]
fn result_must_belong_to_requested_chat_and_fingerprints_bind_every_input() -> Result<()> {
    let task = Task::from(message("same text")).resolve(&Config::default())?;
    let mut result = TaskResult {
        job_id: "followup".into(),
        url: CHAT.into(),
        reasoning_effort: Some("Extra High".into()),
        evidence: "thinking".into(),
    };
    task.validate_result(&result)?;
    result.url = "https://chatgpt.com/c/different-chat".into();
    assert!(task.validate_result(&result).is_err());
    result.url = CHAT.into();
    result.evidence = "stop_button".into();
    assert!(task.validate_result(&result).is_err());

    let fingerprint = task.fingerprint("Profile 6")?;
    assert_eq!(fingerprint, task.fingerprint("Profile 6")?);
    assert_ne!(fingerprint, task.fingerprint("Profile 5")?);
    assert_ne!(
        fingerprint,
        Task::from(message("different text")).fingerprint("Profile 6")?
    );
    let mut other_chat = message("same text");
    other_chat.chatgpt_url = "https://chatgpt.com/c/another-chat".into();
    assert_ne!(
        fingerprint,
        Task::from(other_chat).fingerprint("Profile 6")?
    );
    let creation = new_task("same text").resolve(&Config::default())?;
    assert_ne!(
        fingerprint,
        Task::from(creation.clone()).fingerprint("Profile 6")?
    );
    // Existing new_task request IDs still recognize their pre-followup journal entries.
    assert_eq!(
        creation.fingerprint("Profile 6")?,
        Task::from(creation).fingerprint("Profile 6")?
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Finish {
    Started,
    BeforeSubmission,
    Uncertain,
    WrongConversation,
}

#[derive(Debug, PartialEq, Eq)]
struct Seen {
    id: String,
    target: Option<String>,
    requested_effort: Option<String>,
}

struct Mock {
    calls: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    closes: AtomicUsize,
    seen: Mutex<Vec<Seen>>,
    started: Notify,
    gate: Semaphore,
    finish: Finish,
}

impl Mock {
    fn new(permits: usize, finish: Finish) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
            seen: Mutex::new(vec![]),
            started: Notify::new(),
            gate: Semaphore::new(permits),
            finish,
        }
    }
}

impl Backend for Mock {
    fn run<'a>(
        &'a self,
        _: &'a Config,
        task: &'a Task,
        id: &'a str,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.seen.lock().unwrap().push(Seen {
                id: id.into(),
                target: task.conversation_url().map(str::to_string),
                requested_effort: task.reasoning_effort().map(str::to_string),
            });
            self.started.notify_one();
            self.gate.acquire().await.unwrap().forget();
            self.active.fetch_sub(1, Ordering::SeqCst);
            match self.finish {
                Finish::BeforeSubmission | Finish::Uncertain => Err(StartError {
                    code: "conversation_not_ready".into(),
                    may_have_submitted: matches!(self.finish, Finish::Uncertain),
                    url: task.conversation_url().map(str::to_string),
                }
                .into()),
                Finish::Started | Finish::WrongConversation => Ok(TaskResult {
                    job_id: id.into(),
                    url: if matches!(self.finish, Finish::WrongConversation) {
                        "https://chatgpt.com/c/wrong-chat".into()
                    } else {
                        task.conversation_url()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("https://chatgpt.com/c/{id}"))
                    },
                    reasoning_effort: task
                        .reasoning_effort()
                        .or(Some("Medium"))
                        .map(str::to_string),
                    evidence: "thinking".into(),
                }),
            }
        })
    }

    fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

async fn until(predicate: impl Fn() -> bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn creation_and_followups_share_serial_queue_idempotency_and_private_metadata() -> Result<()>
{
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(0, Finish::Started));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let q = queue.clone();
    let first = tokio::spawn(async move {
        q.submit(
            "creation".into(),
            new_task("private first prompt"),
            Config::default(),
        )
        .await
    });
    backend.started.notified().await;
    let q = queue.clone();
    let second = tokio::spawn(async move {
        q.submit(
            "followup".into(),
            message("private followup message"),
            Config::default(),
        )
        .await
    });
    until(|| queue.active() == 2).await?;
    let q = queue.clone();
    let duplicate = tokio::spawn(async move {
        q.submit(
            "followup".into(),
            message("private followup message"),
            Config::default(),
        )
        .await
    });
    assert!(
        queue
            .submit(
                "followup".into(),
                message("changed message"),
                Config::default()
            )
            .await
            .is_err()
    );
    let mut other = message("private followup message");
    other.chatgpt_url = "https://chatgpt.com/c/other".into();
    assert!(
        queue
            .submit("followup".into(), other, Config::default())
            .await
            .is_err()
    );
    assert!(
        queue
            .submit(
                "followup".into(),
                new_task("private followup message"),
                Config::default()
            )
            .await
            .is_err()
    );
    backend.gate.add_permits(2);
    first.await??;
    let result = second.await??;
    assert_eq!(result, duplicate.await??);
    assert_eq!(result.url, CHAT);
    assert_eq!(result.reasoning_effort.as_deref(), Some("Medium"));
    until(|| queue.quiet()).await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(backend.peak.load(Ordering::SeqCst), 1);
    assert_eq!(backend.closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        *backend.seen.lock().unwrap(),
        vec![
            Seen {
                id: "creation".into(),
                target: None,
                requested_effort: Some("Extra High".into())
            },
            Seen {
                id: "followup".into(),
                target: Some(CHAT.into()),
                requested_effort: None
            },
        ]
    );
    let journal = std::fs::read_to_string(dir.path().join("journal.json"))?;
    assert!(!journal.contains("private first prompt"));
    assert!(!journal.contains("private followup message"));
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn invalid_followup_is_rejected_before_backend_or_journal_admission() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(1, Finish::Started));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let mut invalid = message("hello");
    invalid.chatgpt_url = "https://evil.example/c/id".into();
    assert!(
        queue
            .submit("invalid".into(), invalid, Config::default())
            .await
            .is_err()
    );
    let response = slingring::mcp::dispatch(
        dir.path(),
        &queue,
        request(json!({"chatgpt_url":"https://chatgpt.com/work/id","message":"hello"})),
        "invalid-mcp".into(),
        None,
    )
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
async fn expired_queued_followup_never_launches_after_previous_chat_handoff() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(Mock::new(0, Finish::Started));
    let (stop, receiver) = watch::channel(false);
    let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
    let q = queue.clone();
    let first = tokio::spawn(async move {
        q.submit("first".into(), new_task("first"), Config::default())
            .await
    });
    backend.started.notified().await;
    let q = queue.clone();
    let expired = tokio::spawn(async move {
        q.submit_before(
            "expired".into(),
            message("must not send"),
            Config::default(),
            Some(Instant::now() - Duration::from_secs(1)),
        )
        .await
    });
    until(|| queue.active() == 2).await?;
    backend.gate.add_permits(1);
    first.await??;
    assert!(expired.await?.is_err());
    until(|| queue.quiet()).await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert_eq!(queue.snapshot()[1]["status"], "failed");
    assert_eq!(backend.closes.load(Ordering::SeqCst), 1);
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn followup_failures_are_classified_cached_and_never_replayed_after_restart() -> Result<()> {
    for (finish, expected) in [
        (Finish::BeforeSubmission, "failed"),
        (Finish::Uncertain, "uncertain"),
        (Finish::WrongConversation, "uncertain"),
    ] {
        let dir = tempfile::tempdir()?;
        let backend = Arc::new(Mock::new(1, finish));
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), receiver)?;
        for _ in 0..2 {
            assert!(
                queue
                    .submit(
                        "followup".into(),
                        message("private retry input"),
                        Config::default()
                    )
                    .await
                    .is_err()
            );
        }
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
        assert_eq!(backend.closes.load(Ordering::SeqCst), 1);
        assert_eq!(queue.snapshot()[0]["status"], expected);
        assert!(
            !std::fs::read_to_string(dir.path().join("journal.json"))?
                .contains("private retry input")
        );
        stop.send(true)?;
        worker.await??;
        let restarted = Arc::new(Mock::new(1, Finish::Started));
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), restarted.clone(), receiver)?;
        assert!(
            queue
                .submit(
                    "followup".into(),
                    message("private retry input"),
                    Config::default()
                )
                .await
                .is_err()
        );
        assert_eq!(restarted.calls.load(Ordering::SeqCst), 0);
        assert_eq!(queue.snapshot()[0]["status"], expected);
        stop.send(true)?;
        worker.await??;
    }
    Ok(())
}
