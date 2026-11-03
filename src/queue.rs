//! One bounded FIFO owns the browser. Only metadata is persisted; prompts and
//! image paths live in memory and interrupted jobs are never automatically replayed.
use crate::{
    config::{self, Config},
    model::{QUEUE_CAPACITY, StartError, Task, TaskResult},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

pub type BrowserFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub trait Backend: Send + Sync + 'static {
    fn run<'a>(
        &'a self,
        config: &'a Config,
        task: &'a Task,
        id: &'a str,
    ) -> BrowserFuture<'a, TaskResult>;
    /// Optional metadata-only progress for caller-visible diagnostics. Test
    /// backends and existing integrations need not implement this hook.
    fn run_with_progress<'a>(
        &'a self,
        config: &'a Config,
        task: &'a Task,
        id: &'a str,
        _progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> BrowserFuture<'a, TaskResult> {
        self.run(config, task, id)
    }
    fn close<'a>(&'a self, config: &'a Config) -> BrowserFuture<'a, ()>;
}
pub struct Browser;
impl Backend for Browser {
    fn run<'a>(&'a self, c: &'a Config, t: &'a Task, id: &'a str) -> BrowserFuture<'a, TaskResult> {
        Box::pin(crate::browser::run(c, t, id))
    }
    fn run_with_progress<'a>(
        &'a self,
        c: &'a Config,
        t: &'a Task,
        id: &'a str,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> BrowserFuture<'a, TaskResult> {
        Box::pin(crate::browser::run_with_progress(c, t, id, progress))
    }
    fn close<'a>(&'a self, c: &'a Config) -> BrowserFuture<'a, ()> {
        Box::pin(crate::browser::close(c))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub status: String,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub result: Option<TaskResult>,
    pub error: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub phase_at: Option<u64>,
    #[serde(default)]
    fingerprint: String,
}
impl Job {
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "starting")
    }
    pub fn public(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"status":self.status,"created_at":self.created_at,
            "finished_at":self.finished_at,"result":self.result,"error":self.error,"url":self.url,
            "phase":self.phase,"phase_at":self.phase_at,
            "elapsed_ms":self.finished_at.unwrap_or_else(now).saturating_sub(self.created_at)})
    }
    fn outcome(&self) -> Result<TaskResult> {
        self.result.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                self.error
                    .as_deref()
                    .unwrap_or("outcome uncertain; inspect queue before retrying")
            )
        })
    }
}
pub fn recovery_status(job: Option<&Job>, request_id: &str) -> serde_json::Value {
    let Some(job) = job else {
        return serde_json::json!({"request_id":request_id,"known":false,"outcome":"unknown",
            "guidance":"Record not found or pruned. Absence is NOT proof that nothing was submitted. Inspect the chat and Mac journal before deciding whether to submit a new request."});
    };
    let (outcome, guidance) = match job.status.as_str() {
        "queued" | "starting" => (
            "in_progress",
            "Job is still pending. Query this same request_id later; do not submit it again.",
        ),
        "started" => (
            "submitted",
            "Message submission was confirmed. Open the returned conversation URL; do not send it again.",
        ),
        "failed" => (
            "not_submitted",
            "The journal confirms nothing was submitted. Diagnose the failure before consciously creating a new request.",
        ),
        _ => (
            "uncertain",
            "Submission may have occurred. Inspect the conversation; never automatically retry or treat a missing URL as evidence of failure.",
        ),
    };
    let diagnosis = match job
        .error
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
    {
        "chat_mode_unverified" => {
            "Chat mode could not be positively verified before submission. Confirm the configured Chrome account opens Chat (not Work) and check session/profile readiness. Inspect events.log for the stage and elapsed time."
        }
        "browser_start_failed" => {
            "The isolated browser failed to start twice. Inspect events.log for safe launch/retry categories (timeout, spawn, exit, invalid JSON or unsuccessful response); verify the pinned browser helper with slingring browser status and agent-browser-browse doctor."
        }
        "browser_endpoint_unavailable"
        | "isolated_profile_unverified"
        | "native_registration_failed"
        | "extension_installation_failed"
        | "native_connection_failed" => {
            "Browser or native bridge setup failed before submission. Inspect events.log, verify the pinned browser helper and selected Chrome profile, then diagnose before a new request."
        }
        _ => {
            "Use the recorded phase and outcome; inspect the Mac journal and conversation before choosing any further action."
        }
    };
    serde_json::json!({"request_id":request_id,"known":true,"outcome":outcome,
        "guidance":guidance,"diagnosis":diagnosis,"job":job.public()})
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn valid_job_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':'))
}
pub fn records(root: &Path) -> Result<VecDeque<Job>> {
    let path = root.join("journal.json");
    if !path.exists() {
        return Ok(VecDeque::new());
    }
    let jobs: VecDeque<Job> =
        serde_json::from_slice(&config::read_private(&path, 2 * 1024 * 1024)?)?;
    ensure!(jobs.len() <= 256, "journal contains too many jobs");
    Ok(jobs)
}
type Reply = oneshot::Sender<Result<TaskResult>>;
struct State {
    jobs: VecDeque<Job>,
    waiters: HashMap<String, Vec<Reply>>,
    root: PathBuf,
}
impl State {
    fn save(&mut self) -> Result<()> {
        while self.jobs.len() > 256 {
            let i = self
                .jobs
                .iter()
                .position(|j| !j.active())
                .context("journal full")?;
            self.jobs.remove(i);
        }
        config::atomic(
            &self.root.join("journal.json"),
            &serde_json::to_vec(&self.jobs)?,
        )
    }
    fn finish(&mut self, id: &str, result: Result<TaskResult>, status: &str) -> Result<()> {
        let safe_code = match &result {
            Ok(_) => "activity_confirmed",
            Err(error) => error
                .downcast_ref::<StartError>()
                .map(|e| e.code.as_str())
                .filter(|c| {
                    c.len() <= 100 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                })
                .unwrap_or("operation_error"),
        }
        .to_owned();
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .context("job missing")?;
        job.status = status.into();
        job.finished_at = Some(now());
        match result {
            Ok(r) => {
                job.url = Some(r.url.clone());
                job.result = Some(r);
            }
            Err(e) => {
                if let Some(e) = e.downcast_ref::<StartError>().filter(|e| e.url.is_some()) {
                    job.url = e.url.clone();
                }
                job.error = Some(e.to_string());
            }
        }
        let saved = self.save();
        if saved.is_ok() {
            let job = self
                .jobs
                .iter()
                .find(|job| job.id == id)
                .context("job missing")?;
            crate::diagnostics::event(
                &self.root,
                id,
                "finished",
                status,
                &safe_code,
                job.finished_at
                    .unwrap_or_else(now)
                    .saturating_sub(job.created_at),
            );
        }
        let job = self
            .jobs
            .iter()
            .find(|j| j.id == id)
            .context("job missing")?;
        for waiter in self.waiters.remove(id).unwrap_or_default() {
            let response = if saved.is_ok() {
                job.outcome()
            } else {
                Err(anyhow::anyhow!(
                    "submission may have started; completion could not be persisted; do not blindly retry"
                ))
            };
            let _ = waiter.send(response);
        }
        saved
    }
}
struct Item {
    id: String,
    task: Task,
    config: Config,
    deadline: Option<Instant>,
}
#[derive(Clone)]
pub struct Queue {
    state: Arc<Mutex<State>>,
    tx: mpsc::Sender<Item>,
    busy: Arc<AtomicBool>,
}
impl Queue {
    pub fn open(
        root: PathBuf,
        backend: Arc<dyn Backend>,
        stop: watch::Receiver<bool>,
    ) -> Result<(Self, tokio::task::JoinHandle<Result<()>>)> {
        config::private_dir(&root)?;
        let mut jobs = records(&root)?;
        for job in &mut jobs {
            if job.active() {
                let pending = job.status == "queued";
                job.status = if pending { "failed" } else { "uncertain" }.into();
                job.finished_at = Some(now());
                job.error = Some(
                    if pending {
                        "service restarted before launch; not replayed"
                    } else {
                        "service restarted during submission; inspect the chat before retrying"
                    }
                    .into(),
                );
            }
        }
        let mut state = State {
            jobs,
            waiters: HashMap::new(),
            root,
        };
        state.save()?;
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let queue = Self {
            state: Arc::new(Mutex::new(state)),
            tx,
            busy: Arc::new(AtomicBool::new(false)),
        };
        let worker = tokio::spawn(work(queue.clone(), rx, backend, stop));
        Ok((queue, worker))
    }
    pub fn snapshot(&self) -> Vec<serde_json::Value> {
        self.state
            .lock()
            .expect("queue lock")
            .jobs
            .iter()
            .map(Job::public)
            .collect()
    }
    pub fn active(&self) -> usize {
        self.state
            .lock()
            .expect("queue lock")
            .jobs
            .iter()
            .filter(|j| j.active())
            .count()
    }
    pub fn quiet(&self) -> bool {
        self.active() == 0 && !self.busy.load(Ordering::Acquire)
    }
    pub async fn submit(
        &self,
        id: String,
        task: impl Into<Task>,
        config: Config,
    ) -> Result<TaskResult> {
        self.submit_before(id, task, config, None).await
    }
    /// A remote caller may impose an end-to-end deadline. Local queue waiting
    /// does not consume the individual browser startup budget.
    pub async fn submit_before(
        &self,
        id: String,
        task: impl Into<Task>,
        config: Config,
        deadline: Option<Instant>,
    ) -> Result<TaskResult> {
        ensure!(valid_job_id(&id), "invalid request ID");
        let task = task.into().resolve(&config)?;
        let fingerprint = task.fingerprint(&config.profile)?;
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().expect("queue lock");
            if let Some(job) = state.jobs.iter().find(|j| j.id == id) {
                ensure!(
                    job.fingerprint == fingerprint,
                    "request ID was already used with different input"
                );
                if !job.active() {
                    return job.outcome();
                }
                let waiters = state.waiters.entry(id).or_default();
                ensure!(
                    waiters.len() < 8,
                    "too many callers waiting for this request"
                );
                waiters.push(tx);
            } else {
                ensure!(
                    state.jobs.iter().filter(|j| j.active()).count() < QUEUE_CAPACITY,
                    "queue is full (32 tasks); nothing was submitted"
                );
                let permit = self.tx.try_reserve().context("queue unavailable")?;
                state.jobs.push_back(Job {
                    id: id.clone(),
                    status: "queued".into(),
                    created_at: now(),
                    finished_at: None,
                    result: None,
                    error: None,
                    url: task.conversation_url().map(str::to_string),
                    phase: Some("queued".into()),
                    phase_at: Some(now()),
                    fingerprint,
                });
                if let Err(e) = state.save() {
                    state.jobs.pop_back();
                    return Err(e).context("cannot persist task metadata");
                }
                state.waiters.insert(id.clone(), vec![tx]);
                crate::diagnostics::event(&state.root, &id, "queued", "in_progress", "none", 0);
                permit.send(Item {
                    id,
                    task,
                    config,
                    deadline,
                });
            }
        }
        rx.await
            .context("service stopped; inspect queue before retrying")?
    }
}
async fn work(
    queue: Queue,
    mut rx: mpsc::Receiver<Item>,
    backend: Arc<dyn Backend>,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let mut open_config: Option<Config> = None;
    let outcome = async {
        loop {
            let item = tokio::select! { biased; _ = stop.changed() => None, item = rx.recv() => item };
            let Some(Item { id, task, mut config, deadline }) = item else { break; };
            if *stop.borrow() { break; }
            let skipped = {
                let mut state = queue.state.lock().expect("queue lock");
                let abandoned = state.waiters.get(&id).is_none_or(|w| w.iter().all(oneshot::Sender::is_closed));
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) || abandoned {
                    state.finish(&id, Err(anyhow::anyhow!("request expired or caller disconnected before launch; nothing was submitted")), "failed")?;
                    true
                } else {
                    let job = state.jobs.iter_mut().find(|j| j.id == id).context("job missing")?;
                    job.status = "starting".into();
                    job.phase = Some("starting".into());
                    job.phase_at = Some(now());
                    let elapsed = now().saturating_sub(job.created_at);
                    state.save()?;
                    crate::diagnostics::event(&state.root, &id, "starting", "in_progress", "none", elapsed);
                    false
                }
            };
            if skipped {
                if rx.is_empty() {
                    if let Some(config) = open_config.take() { close_bounded(backend.as_ref(), &config).await; }
                    queue.busy.store(false, Ordering::Release);
                }
                continue;
            }
            queue.busy.store(true, Ordering::Release);
            config.browser_idle_secs = Some(config.start_timeout_secs.saturating_add(60));
            let own_deadline = Instant::now() + Duration::from_secs(config.start_timeout_secs);
            let deadline = deadline.map_or(own_deadline, |external| external.min(own_deadline));
            config.start_timeout_secs = deadline.saturating_duration_since(Instant::now()).as_secs().max(1);
            open_config = Some(config.clone());
            let state = queue.state.clone();
            let progress_id = id.clone();
            let progress: Arc<dyn Fn(&'static str) + Send + Sync> = Arc::new(move |stage| {
                let mut state = state.lock().expect("queue lock");
                let mut elapsed = None;
                if let Some(job) = state.jobs.iter_mut().find(|j| j.id == progress_id && j.active()) {
                    job.phase = Some(stage.into());
                    job.phase_at = Some(now());
                    elapsed = Some(now().saturating_sub(job.created_at));
                }
                if let Some(elapsed) = elapsed && state.save().is_ok() {
                    crate::diagnostics::event(&state.root, &progress_id, stage, "in_progress", "none", elapsed);
                }
            });
            let result = tokio::select! {
                biased;
                _ = stop.changed() => Err(anyhow::anyhow!("service stopped during submission; inspect queue before retrying")),
                r = tokio::time::timeout_at(deadline, backend.run_with_progress(&config, &task, &id, progress)) => match r {
                    Ok(Ok(mut result)) => {
                        result.job_id = id.clone();
                        task.validate_result(&result).map(|()| result)
                    }
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(anyhow::anyhow!("submission deadline reached; outcome uncertain; inspect chat before retrying")),
                }
            };
            let success = result.is_ok();
            let status = if success { "started" } else if result.as_ref().err()
                .and_then(|e| e.downcast_ref::<StartError>()).is_some_and(|e| !e.may_have_submitted) { "failed" } else { "uncertain" };
            if !success || rx.is_empty() || *stop.borrow() {
                close_bounded(backend.as_ref(), &config).await;
                open_config = None;
                queue.busy.store(false, Ordering::Release);
            }
            queue.state.lock().expect("queue lock").finish(&id, result, status)?;
            if *stop.borrow() { break; }
        }
        Ok(())
    }.await;
    rx.close();
    if let Some(config) = open_config {
        close_bounded(backend.as_ref(), &config).await;
    }
    queue.busy.store(false, Ordering::Release);
    let mut state = queue.state.lock().expect("queue lock");
    let ids: Vec<_> = state
        .jobs
        .iter()
        .filter(|j| j.active())
        .map(|j| (j.id.clone(), j.status.clone()))
        .collect();
    for (id, status) in ids {
        let _ = state.finish(
            &id,
            Err(anyhow::anyhow!("service stopped; request was not replayed")),
            if status == "queued" {
                "failed"
            } else {
                "uncertain"
            },
        );
    }
    outcome
}
async fn close_bounded(backend: &dyn Backend, config: &Config) {
    let _ = tokio::time::timeout(Duration::from_secs(20), backend.close(config)).await;
}
