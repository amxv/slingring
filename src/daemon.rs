use crate::{
    config::{self, Config},
    model::{MAX_JSON, NewTask, SendMessage},
    queue::{Browser, Queue},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader,
    },
    net::{UnixListener, UnixStream},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::JoinSet,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Submit { id: String, task: NewTask },
    SubmitFollowup { id: String, task: SendMessage },
    Mcp { body: Value, key: String },
    Status,
    Queue,
    Reload,
    Stop,
}
pub async fn line<R: AsyncBufRead + Unpin>(stream: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let bytes = stream.fill_buf().await?;
        ensure!(!bytes.is_empty(), "connection closed");
        let end = bytes.iter().position(|b| *b == b'\n');
        let n = end.map_or(bytes.len(), |n| n + 1);
        ensure!(out.len() + n <= max, "input exceeds size limit");
        out.extend_from_slice(&bytes[..n]);
        stream.consume(n);
        if end.is_some() {
            return Ok(out);
        }
    }
}
pub async fn call(root: &Path, request: &Request) -> Result<Value> {
    let mut stream = UnixStream::connect(root.join("slingring.sock"))
        .await
        .context("Slingring service is not running")?;
    let mut bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() < MAX_JSON, "request too large");
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    let mut reader = BufReader::new(stream);
    let bytes =
        tokio::time::timeout(Duration::from_secs(3610), line(&mut reader, MAX_JSON)).await??;
    let v: Value = serde_json::from_slice(&bytes)?;
    if let Some(error) = v.get("error").and_then(Value::as_str) {
        anyhow::bail!("{error}");
    }
    Ok(v)
}
pub async fn running(root: &Path) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_millis(600), call(root, &Request::Status)).await,
        Ok(Ok(_))
    )
}

struct ContextState {
    root: PathBuf,
    queue: Queue,
    stop: watch::Sender<bool>,
    config: watch::Sender<Config>,
    remote: watch::Receiver<String>,
    submissions: Semaphore,
    controls: Semaphore,
}
fn is_submission(request: &Request) -> bool {
    match request {
        Request::Submit { .. } | Request::SubmitFollowup { .. } => true,
        Request::Mcp { body, .. } => {
            crate::mcp::metadata(body).is_none() && !crate::mcp::status_request(body)
        }
        _ => false,
    }
}
async fn client<S>(stream: S, ctx: Arc<ContextState>, reading: OwnedSemaphorePermit) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stream);
    let bytes = tokio::time::timeout(Duration::from_secs(5), line(&mut reader, MAX_JSON)).await??;
    let request = serde_json::from_slice::<Request>(&bytes);
    // Slow/partial readers are bounded independently from accepted submissions.
    // A full write wait pool must never prevent status/reload/stop from parsing.
    let submitting = request.as_ref().is_ok_and(is_submission);
    let lane = if submitting {
        ctx.submissions.try_acquire()
    } else {
        ctx.controls.try_acquire()
    };
    // A client without a reply-lane permit retains its bounded reader slot
    // until the short error response is written or times out.
    let _reading = if lane.is_ok() {
        drop(reading);
        None
    } else {
        Some(reading)
    };
    let mut stop_after_write = false;
    let operation = async {
        if lane.is_err() {
            return Ok(json!({"error":if submitting {
                "Local submission wait capacity exceeded; this connection was not admitted. An earlier request with the same ID may exist; query its status."
            } else { "Local status/reply capacity exceeded; retry the read-only lookup later." }}));
        }
        Ok(match request {
            Ok(Request::Submit { id, task }) => {
                let config = Config::load(&ctx.root)?;
                serde_json::to_value(ctx.queue.submit(id, task, config).await?)?
            }
            Ok(Request::SubmitFollowup { id, task }) => {
                let config = Config::load(&ctx.root)?;
                serde_json::to_value(ctx.queue.submit(id, task, config).await?)?
            }
            Ok(Request::Mcp { body, key }) => {
                json!({"response":crate::mcp::dispatch(&ctx.root,&ctx.queue,body,key,None).await})
            }
            Ok(Request::Status) => {
                let config = Config::load(&ctx.root)?;
                let jobs = ctx.queue.snapshot();
                let active_job = jobs
                    .iter()
                    .find(|job| job["status"] == "starting")
                    .or_else(|| jobs.iter().find(|job| job["status"] == "queued"));
                let queued = jobs.iter().filter(|job| job["status"] == "queued").count();
                let starting = jobs
                    .iter()
                    .filter(|job| job["status"] == "starting")
                    .count();
                let latest_finished = jobs
                    .iter()
                    .rev()
                    .find(|job| !matches!(job["status"].as_str(), Some("queued" | "starting")));
                let latest_started = jobs.iter().rev().find(|job| job["status"] == "started");
                json!({"running":true,"pid":std::process::id(),"remote_enabled":config.remote_enabled,
                    "remote_state":ctx.remote.borrow().clone(),"active_tasks":ctx.queue.active(),
                    "browser_busy":!ctx.queue.quiet(),"latest_job":jobs.last(),"config":config.redacted(),
                    "status_available":true,"active_job":active_job,"queued_tasks":queued,"starting_tasks":starting,
                    "latest_finished_job":latest_finished,"latest_started_job":latest_started})
            }
            Ok(Request::Queue) => json!(ctx.queue.snapshot()),
            Ok(Request::Reload) => {
                ctx.config.send_replace(Config::load(&ctx.root)?);
                json!({"reloaded":true})
            }
            Ok(Request::Stop) => {
                stop_after_write = true;
                json!({"stopping":true})
            }
            Err(_) => json!({"error":"invalid local request"}),
        })
    };
    // A caller that vanishes while queued must not cause a surprise later launch.
    let mut eof = [0_u8; 1];
    let response: Result<Value> = tokio::select! {
        response = operation => response,
        _ = reader.get_mut().read(&mut eof) => return Ok(()),
    };
    let value = response.unwrap_or_else(|e| json!({"error":e.to_string()}));
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(5), reader.get_mut().write_all(&bytes)).await??;
    if stop_after_write {
        let _ = ctx.stop.send(true);
    }
    Ok(())
}
struct SocketCleanup(PathBuf);
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn run(root: PathBuf, ephemeral: bool) -> Result<()> {
    config::private_dir(&root)?;
    let lock_path = root.join("daemon.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&lock_path) {
        ensure!(
            !meta.file_type().is_symlink() && meta.is_file(),
            "invalid daemon lock"
        );
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("another Slingring service already owns this state directory")?;
    let socket = root.join("slingring.sock");
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)
        .context("could not bind private socket (state path may be too long)")?;
    let _cleanup = SocketCleanup(socket.clone());
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let initial = Config::load(&root)?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let (config_tx, config_rx) = watch::channel(initial);
    let (remote_tx, remote_rx) = watch::channel(String::from("disabled"));
    let (queue, mut worker) = Queue::open(root.clone(), Arc::new(Browser), stop_rx.clone())?;
    let remote = tokio::spawn(crate::relay::run(
        root.clone(),
        queue.clone(),
        config_rx,
        stop_rx.clone(),
        remote_tx,
    ));
    let ctx = Arc::new(ContextState {
        root,
        queue,
        stop: stop_tx.clone(),
        config: config_tx,
        remote: remote_rx,
        submissions: Semaphore::new(64),
        controls: Semaphore::new(16),
    });
    let permits = Arc::new(Semaphore::new(64));
    let mut clients = JoinSet::new();
    let mut stopping = stop_rx;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let grace = Duration::from_secs(if ephemeral { 1 } else { 2 });
    let idle = tokio::time::sleep(grace);
    tokio::pin!(idle);
    let mut worker_finished = false;
    let result = loop {
        tokio::select! {
            r = &mut worker => { worker_finished = true; break r.context("queue worker stopped")?; }
            _ = tokio::signal::ctrl_c() => break Ok(()),
            _ = terminate.recv() => break Ok(()),
            _ = stopping.changed() => break Ok(()),
            _ = &mut idle, if clients.is_empty() && !ctx.config.borrow().remote_enabled => {
                if ctx.queue.quiet() { break Ok(()); }
                idle.as_mut().reset(tokio::time::Instant::now() + grace);
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {
                idle.as_mut().reset(tokio::time::Instant::now() + grace);
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                if let Ok(permit) = permits.clone().try_acquire_owned() {
                    let ctx = ctx.clone();
                    clients.spawn(async move { let _ = client(stream,ctx,permit).await; });
                }
            }
        }
    };
    let _ = stop_tx.send(true);
    remote.abort();
    if !worker_finished
        && tokio::time::timeout(Duration::from_secs(22), &mut worker)
            .await
            .is_err()
    {
        worker.abort();
    }
    clients.abort_all();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Task, TaskResult},
        queue::{Backend, BrowserFuture},
    };

    struct Blocked;
    impl Backend for Blocked {
        fn run<'a>(
            &'a self,
            _: &'a Config,
            _: &'a Task,
            _: &'a str,
        ) -> BrowserFuture<'a, TaskResult> {
            Box::pin(std::future::pending())
        }
        fn close<'a>(&'a self, _: &'a Config) -> BrowserFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
    }
    async fn connect(
        ctx: Arc<ContextState>,
        readers: Arc<Semaphore>,
        request: Request,
    ) -> Result<(UnixStream, tokio::task::JoinHandle<Result<()>>)> {
        let (server, mut caller) = UnixStream::pair()?;
        let permit = readers.acquire_owned().await?;
        let handler = tokio::spawn(client(server, ctx, permit));
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        caller.write_all(&bytes).await?;
        Ok((caller, handler))
    }
    #[tokio::test]
    async fn status_and_queue_bypass_a_saturated_local_submission_pool() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), Arc::new(Blocked), receiver)?;
        let (config, _) = watch::channel(Config::default());
        let (_, remote) = watch::channel("disabled".to_string());
        let ctx = Arc::new(ContextState {
            root: dir.path().into(),
            queue,
            stop: stop.clone(),
            config,
            remote,
            submissions: Semaphore::new(1),
            controls: Semaphore::new(1),
        });
        let readers = Arc::new(Semaphore::new(1));
        let task = NewTask {
            prompt: "PRIVATE fixture".into(),
            reasoning_effort: None,
            image_paths: vec![],
        };
        let (_write, write_handler) = connect(
            ctx.clone(),
            readers.clone(),
            Request::Submit {
                id: "busy".into(),
                task,
            },
        )
        .await?;
        tokio::time::timeout(Duration::from_secs(1), async {
            while ctx.queue.active() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert_eq!(ctx.submissions.available_permits(), 0);
        assert_eq!(readers.available_permits(), 1);
        let (status, status_handler) =
            connect(ctx.clone(), readers.clone(), Request::Status).await?;
        let bytes = tokio::time::timeout(
            Duration::from_millis(500),
            line(&mut BufReader::new(status), MAX_JSON),
        )
        .await??;
        let status: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(status["running"], true);
        assert_eq!(status["active_tasks"], 1);
        assert_eq!(status["starting_tasks"], 1);
        assert_eq!(status["active_job"]["id"], "busy");
        assert!(status["latest_started_job"].is_null());
        status_handler.await??;
        let (history, history_handler) = connect(ctx.clone(), readers, Request::Queue).await?;
        let bytes = tokio::time::timeout(
            Duration::from_millis(500),
            line(&mut BufReader::new(history), MAX_JSON),
        )
        .await??;
        assert_eq!(serde_json::from_slice::<Value>(&bytes)?[0]["id"], "busy");
        history_handler.await??;
        stop.send(true)?;
        worker.await??;
        write_handler.await??;
        Ok(())
    }

    #[tokio::test]
    async fn a_nonreading_status_client_has_a_bounded_reply_lane_and_write_deadline() -> Result<()>
    {
        let dir = tempfile::tempdir()?;
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), Arc::new(Blocked), receiver)?;
        let (config, _) = watch::channel(Config::default());
        let (_, remote) = watch::channel("disabled".to_string());
        let ctx = Arc::new(ContextState {
            root: dir.path().into(),
            queue,
            stop: stop.clone(),
            config,
            remote,
            submissions: Semaphore::new(1),
            controls: Semaphore::new(1),
        });
        let readers = Arc::new(Semaphore::new(1));
        let (server, mut caller) = tokio::io::duplex(1);
        let permit = readers.clone().acquire_owned().await?;
        let handler = tokio::spawn(client(server, ctx.clone(), permit));
        caller.write_all(b"{\"type\":\"status\"}\n").await?;
        tokio::task::yield_now().await;
        assert_eq!(ctx.controls.available_permits(), 0);
        assert_eq!(readers.available_permits(), 1);
        // A second control response does not acquire unbounded waiting state.
        let (second, second_handler) =
            connect(ctx.clone(), readers.clone(), Request::Queue).await?;
        let bytes = tokio::time::timeout(
            Duration::from_millis(500),
            line(&mut BufReader::new(second), MAX_JSON),
        )
        .await??;
        assert!(
            serde_json::from_slice::<Value>(&bytes)?["error"]
                .as_str()
                .unwrap()
                .contains("capacity")
        );
        second_handler.await??;
        assert!(
            tokio::time::timeout(Duration::from_secs(6), handler)
                .await??
                .is_err()
        );
        assert_eq!(ctx.controls.available_permits(), 1);
        assert_eq!(readers.available_permits(), 1);
        assert!(ctx.queue.snapshot().is_empty());
        stop.send(true)?;
        worker.await??;
        Ok(())
    }
}
