use crate::{
    config::{self, Config},
    model::{MAX_JSON, NewTask},
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
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, watch},
    task::JoinSet,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Submit { id: String, task: NewTask },
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
}
async fn client(stream: UnixStream, ctx: Arc<ContextState>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let bytes = tokio::time::timeout(Duration::from_secs(5), line(&mut reader, MAX_JSON)).await??;
    let request = serde_json::from_slice::<Request>(&bytes);
    let mut stop_after_write = false;
    let operation = async {
        Ok(match request {
            Ok(Request::Submit { id, task }) => {
                let config = Config::load(&ctx.root)?;
                serde_json::to_value(ctx.queue.submit(id, task, config).await?)?
            }
            Ok(Request::Mcp { body, key }) => {
                json!({"response":crate::mcp::dispatch(&ctx.root,&ctx.queue,body,key,None).await})
            }
            Ok(Request::Status) => {
                let config = Config::load(&ctx.root)?;
                let jobs = ctx.queue.snapshot();
                json!({"running":true,"pid":std::process::id(),"remote_enabled":config.remote_enabled,
                    "remote_state":ctx.remote.borrow().clone(),"active_tasks":ctx.queue.active(),
                    "browser_busy":!ctx.queue.quiet(),"latest_job":jobs.last(),"config":config.redacted()})
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
    reader.get_mut().write_all(&bytes).await?;
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
                    clients.spawn(async move { let _permit = permit; let _ = client(stream,ctx).await; });
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
