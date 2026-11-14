//! No browser or live submission. Exercise the durable pre-send barrier.
use anyhow::Result;
use slingring::{
    config::{self, Config},
    model::{NewTask, StartError, Task, TaskResult},
    queue::{Backend, BrowserFuture, Queue, SubmissionGate},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Notify, watch};
struct GateBackend {
    root: PathBuf,
    reached: Notify,
    proceed: Notify,
    after: AtomicBool,
    fail_save: bool,
}
impl Backend for GateBackend {
    fn run<'a>(&'a self, _: &'a Config, _: &'a Task, _: &'a str) -> BrowserFuture<'a, TaskResult> {
        unreachable!()
    }
    fn run_authorized<'a>(
        &'a self,
        _: &'a Config,
        _: &'a Task,
        _: &'a str,
        _: Arc<dyn Fn(&'static str) + Send + Sync>,
        gate: SubmissionGate,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(async move {
            self.reached.notify_one();
            self.proceed.notified().await;
            if self.fail_save {
                std::fs::rename(
                    self.root.join("journal.json"),
                    self.root.join("old-journal"),
                )?;
                std::fs::create_dir(self.root.join("journal.json"))?;
            }
            let authorized = gate();
            if self.fail_save {
                std::fs::remove_dir(self.root.join("journal.json"))?;
                std::fs::rename(
                    self.root.join("old-journal"),
                    self.root.join("journal.json"),
                )?;
            }
            authorized.map_err(|_| StartError {
                code: "submission_intent_not_persisted".into(),
                may_have_submitted: false,
                url: None,
            })?;
            // Assert disk persistence before the stand-in for sending is reached.
            let rows: serde_json::Value =
                serde_json::from_slice(&std::fs::read(self.root.join("journal.json"))?)?;
            assert_eq!(rows[0]["submission_intent"], true);
            self.after.store(true, Ordering::SeqCst);
            std::future::pending().await
        })
    }
    fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn task() -> Task {
    NewTask {
        prompt: "fixture".into(),
        reasoning_effort: None,
        image_paths: vec![],
    }
    .into()
}
#[tokio::test]
async fn stop_before_and_after_authorization_remain_distinct_and_never_replay() -> Result<()> {
    for authorize in [false, true] {
        let dir = tempfile::tempdir()?;
        config::private_dir(dir.path())?;
        let backend = Arc::new(GateBackend {
            root: dir.path().into(),
            reached: Notify::new(),
            proceed: Notify::new(),
            after: AtomicBool::new(false),
            fail_save: false,
        });
        let (stop, rx) = watch::channel(false);
        let (q, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
        let receipt = q.accept_before("fixture".into(), task(), Config::default(), None)?;
        backend.reached.notified().await;
        if authorize {
            backend.proceed.notify_one();
            while !backend.after.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        }
        stop.send(true)?;
        worker.await??;
        assert!(receipt.await?.is_err());
        assert_eq!(
            q.recovery_status("fixture")["outcome"],
            if authorize {
                "uncertain"
            } else {
                "not_submitted"
            }
        );
        // A duplicate ID only returns the recorded result, even with a fresh waiter.
        assert!(
            q.submit("fixture".into(), task(), Config::default())
                .await
                .is_err()
        );
    }
    Ok(())
}
#[tokio::test]
async fn failed_intent_save_never_reaches_send() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let backend = Arc::new(GateBackend {
        root: dir.path().into(),
        reached: Notify::new(),
        proceed: Notify::new(),
        after: AtomicBool::new(false),
        fail_save: true,
    });
    let (stop, rx) = watch::channel(false);
    let (q, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
    let receipt = q.accept_before("fixture".into(), task(), Config::default(), None)?;
    backend.reached.notified().await;
    backend.proceed.notify_one();
    assert!(receipt.await?.is_err());
    assert!(!backend.after.load(Ordering::SeqCst));
    assert_eq!(q.recovery_status("fixture")["outcome"], "not_submitted");
    stop.send(true)?;
    worker.await??;
    Ok(())
}

#[tokio::test]
async fn restart_requires_explicit_durable_false_to_prove_interrupted_start_was_unsent()
-> Result<()> {
    for intent in [None, Some(false), Some(true)] {
        let dir = tempfile::tempdir()?;
        let mut row = serde_json::json!({"id":"crashed","status":"starting","created_at":1,
            "finished_at":null,"result":null,"error":null,"url":null,"phase":"composer_wait"});
        if let Some(intent) = intent {
            row["submission_intent"] = intent.into();
        }
        config::atomic(
            &dir.path().join("journal.json"),
            &serde_json::to_vec(&vec![row])?,
        )?;
        let backend = Arc::new(GateBackend {
            root: dir.path().into(),
            reached: Notify::new(),
            proceed: Notify::new(),
            after: AtomicBool::new(false),
            fail_save: false,
        });
        let (stop, rx) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), backend.clone(), rx)?;
        assert_eq!(
            queue.recovery_status("crashed")["outcome"],
            if intent == Some(false) {
                "not_submitted"
            } else {
                "uncertain"
            }
        );
        assert!(!backend.after.load(Ordering::SeqCst));
        stop.send(true)?;
        worker.await??;
    }
    Ok(())
}
