use anyhow::Result;
use serde_json::{Value, json};
use slingring::{
    config::{Config, Subscription},
    model::{EffortVerification, PauseTask, StartError, Task, TaskResult},
    queue::{Backend, BrowserFuture, Queue},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::watch;

const CHAT: &str = "https://chatgpt.com/c/pause-test";
fn request(args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"pause_task","arguments":args}})
}
fn paused(status: &str) -> TaskResult {
    TaskResult {
        job_id: "pause-test".into(),
        url: CHAT.into(),
        pause_status: Some(status.into()),
        reasoning_effort: None,
        requested_effort: None,
        verified_effort: None,
        effort_verification: EffortVerification::NotRequested,
        evidence: "completion_controls".into(),
    }
}

#[test]
fn pause_accepts_only_one_exact_conversation_url() {
    let tool = slingring::mcp::pause_tool();
    assert_eq!(
        tool["inputSchema"]["properties"].as_object().unwrap().len(),
        1
    );
    assert_eq!(tool["inputSchema"]["required"], json!(["chatgpt_url"]));
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    let task = slingring::mcp::arguments(&request(json!({"chatgpt_url":CHAT}))).unwrap();
    assert!(task.is_pause());
    assert_eq!(task.operation(), "pause");
    assert_eq!(task.prompt(), "");
    assert!(task.image_paths().is_empty());
    assert_eq!(task.reasoning_effort(), None);
    for field in [
        "prompt",
        "message",
        "request_id",
        "reasoning_effort",
        "image_paths",
        "profile",
    ] {
        assert!(
            slingring::mcp::arguments(&request(json!({"chatgpt_url":CHAT,field:"extra"}))).is_err()
        );
    }
    for url in [
        "https://chatgpt.com/",
        "https://chatgpt.com/c/a?x=1",
        "https://chatgpt.com/c/a#fragment",
        "https://chatgpt.com/c/a/",
        "http://chatgpt.com/c/a",
        "https://chatgpt.com.evil.example/c/a",
        "https://chatgpt.com/c/a\n",
    ] {
        assert!(
            PauseTask {
                chatgpt_url: url.into()
            }
            .resolve()
            .is_err()
        );
    }
    let mut notification = request(json!({"chatgpt_url":CHAT}));
    notification.as_object_mut().unwrap().remove("id");
    assert_eq!(slingring::mcp::metadata(&notification), Some(None));
}

#[test]
fn pause_results_require_positive_evidence_and_exact_target() -> Result<()> {
    let task = Task::from(PauseTask {
        chatgpt_url: CHAT.into(),
    });
    for status in ["stopped", "already_idle"] {
        task.validate_result(&paused(status))?;
    }
    let mut early = paused("stopped");
    early.evidence = "stopped_thinking".into();
    task.validate_result(&early)?;
    for status in ["unknown", "uncertain", "stopping", ""] {
        assert!(task.validate_result(&paused(status)).is_err());
    }
    let mut result = paused("stopped");
    result.evidence = "voice".into();
    assert!(task.validate_result(&result).is_err());
    result.evidence = "completion_controls".into();
    result.url = "https://chatgpt.com/c/other".into();
    assert!(task.validate_result(&result).is_err());
    result.url = CHAT.into();
    result.pause_status = None;
    result.evidence = "streaming".into();
    assert!(task.validate_result(&result).is_err());
    Ok(())
}

#[test]
fn live_tool_lists_filter_the_configured_subscription() -> Result<()> {
    for (subscription, expected) in [
        (Subscription::Plus, json!(["", "Instant", "Medium", "High"])),
        (
            Subscription::Pro,
            json!(["", "Instant", "Medium", "High", "Extra High", "Pro"]),
        ),
    ] {
        let home = tempfile::tempdir()?;
        let mut config = Config::default();
        config.set_subscription(home.path(), subscription)?;
        let listing = slingring::mcp::immediate_response(
            home.path(),
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(listing["result"]["tools"].as_array().unwrap().len(), 4);
        for tool in &listing["result"]["tools"].as_array().unwrap()[..2] {
            assert_eq!(
                tool["inputSchema"]["properties"]["reasoning_effort"]["enum"],
                expected
            );
        }
        let tools = listing["result"]["tools"].as_array().unwrap();
        for (name, fields) in [
            ("new_task", vec!["prompt", "reasoning_effort", "request_id"]),
            (
                "send_message",
                vec!["chatgpt_url", "message", "reasoning_effort", "request_id"],
            ),
            ("get_job_status", vec!["request_id"]),
            ("pause_task", vec!["chatgpt_url"]),
        ] {
            let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
            let properties = tool["inputSchema"]["properties"].as_object().unwrap();
            assert_eq!(
                properties.keys().map(String::as_str).collect::<Vec<_>>(),
                fields,
                "{subscription:?} {name} must not acquire phantom fields"
            );
            assert!(properties.values().all(|schema| schema["type"].is_string()));
        }
        assert!(!home.path().join("slingring.sock").exists());
    }
    Ok(())
}

struct PauseBackend {
    fail: bool,
    runs: AtomicUsize,
}
impl Backend for PauseBackend {
    fn run<'a>(
        &'a self,
        _config: &'a Config,
        task: &'a Task,
        _id: &'a str,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            assert!(task.is_pause());
            self.runs.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(StartError {
                    code: "pause_stop_unavailable".into(),
                    may_have_submitted: false,
                    url: Some(CHAT.into()),
                }
                .into());
            }
            Ok(paused("stopped"))
        })
    }
    fn close<'a>(&'a self, _config: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pause_dispatch_and_recovery_never_report_submission_or_false_success() -> Result<()> {
    for fail in [false, true] {
        let home = tempfile::tempdir()?;
        let (stop, stopped) = watch::channel(false);
        let backend = Arc::new(PauseBackend {
            fail,
            runs: AtomicUsize::new(0),
        });
        let (queue, worker) = Queue::open(home.path().into(), backend.clone(), stopped)?;
        let reply = slingring::mcp::dispatch(
            home.path(),
            &queue,
            request(json!({"chatgpt_url":CHAT})),
            "pause:internal-1".into(),
            None,
        )
        .await
        .unwrap();
        assert!(reply.get("error").is_none());
        assert_eq!(reply["result"]["isError"], fail);
        let status = queue.recovery_status("pause:internal-1");
        assert_eq!(status["operation"], "pause");
        assert_eq!(status["outcome"], if fail { "uncertain" } else { "paused" });
        assert_eq!(backend.runs.load(Ordering::SeqCst), 1);
        if fail {
            assert!(
                status["guidance"]
                    .as_str()
                    .unwrap()
                    .contains("may still be generating")
            );
            assert!(status["job"]["result"].is_null());
        } else {
            assert_eq!(reply["result"]["structuredContent"]["outcome"], "paused");
            assert_eq!(
                reply["result"]["structuredContent"]["pause_status"],
                "stopped"
            );
            assert_eq!(
                status["job"]["result"]["effort_verification"],
                "not_requested"
            );
        }
        stop.send(true)?;
        worker.await??;
    }
    Ok(())
}

#[test]
fn pause_cli_rejects_invalid_targets_without_starting_a_service() -> Result<()> {
    let home = tempfile::tempdir()?;
    assert_cmd::Command::new(env!("CARGO_BIN_EXE_slingring"))
        .env("SLINGRING_HOME", home.path())
        .args(["pause-task", "https://chatgpt.com/c/a?invalid=1", "--json"])
        .assert()
        .failure();
    assert!(!home.path().join("slingring.sock").exists());
    Ok(())
}
