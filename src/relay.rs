//! A single outbound TLS WebSocket. No job polling, HTTP listener, or idle browser.
use crate::{config::Config, model::MAX_JSON, queue::Queue};
use anyhow::{Result, ensure};
use futures_util::{SinkExt, StreamExt, stream::FuturesUnordered};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::{
    sync::watch,
    time::{Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    WebSocketStream, connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

async fn connection(
    root: PathBuf,
    config: Config,
    queue: Queue,
    status: watch::Sender<String>,
) -> Result<()> {
    let mut request = config
        .relay_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("relay disabled"))?
        .into_client_request()?;
    let auth = format!(
        "Bearer {}",
        config
            .device_token
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("device token missing"))?
    );
    request.headers_mut().insert("Authorization", auth.parse()?);
    let mut limits = WebSocketConfig::default();
    limits.read_buffer_size = 4096;
    limits.write_buffer_size = 4096;
    limits.max_message_size = Some(MAX_JSON + 4096);
    limits.max_frame_size = Some(MAX_JSON + 4096);
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(20),
        connect_async_with_config(request, Some(limits), false),
    )
    .await??;
    status.send_replace("connected".into());
    serve_socket(root, config, queue, socket).await
}

async fn serve_socket<S>(
    root: PathBuf,
    config: Config,
    queue: Queue,
    socket: WebSocketStream<S>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = socket.split();
    let mut calls = FuturesUnordered::new();
    let period = Duration::from_secs(config.heartbeat_secs);
    let mut heartbeat = tokio::time::interval_at(Instant::now() + period, period);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let pong_deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(pong_deadline);
    let mut awaiting_pong = false;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                sink.send(Message::Text("ping".into())).await?;
                awaiting_pong = true;
                pong_deadline.as_mut().reset(Instant::now() + Duration::from_secs(30));
            }
            _ = &mut pong_deadline, if awaiting_pong => anyhow::bail!("heartbeat expired"),
            Some(result) = calls.next(), if !calls.is_empty() => {
                sink.send(Message::Text(serde_json::to_string(&result)?.into())).await?;
            }
            frame = stream.next() => {
                match frame.transpose()? {
                    Some(Message::Text(text)) if text == "pong" => awaiting_pong = false,
                    Some(Message::Text(text)) if text == "ping" => sink.send(Message::Text("pong".into())).await?,
                    Some(Message::Text(text)) => {
                        let value: Value = serde_json::from_str(&text)?;
                        ensure!(value["type"] == "rpc" && value["id"].as_str().is_some_and(|v| !v.is_empty() && v.len() <= 128)
                            && value["request"].is_object(), "invalid relay envelope");
                        let declared = crate::mcp::declared_request_id(&value["request"]).ok().flatten();
                        let key = match value.get("idempotency_key") {
                            None | Some(Value::Null) => declared.unwrap_or_else(||value["id"].as_str().unwrap_or_default()).to_string(),
                            Some(Value::String(k)) if !k.is_empty() && k.len() <= 100 && k.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_')) => k.clone(),
                            _ => anyhow::bail!("invalid idempotency key"),
                        };
                        let deadline = value["expires_at"].as_u64().filter(|d| *d <= crate::queue::now() + 300_000)
                            .ok_or_else(|| anyhow::anyhow!("invalid request deadline"))?;
                        if let Some(response) = crate::mcp::immediate_response(&root, &value["request"]) {
                            sink.send(Message::Text(json!({"type":"rpc_result","id":value["id"],"response":response}).to_string().into())).await?;
                            continue;
                        }
                        if calls.len() >= 32 {
                            let body = crate::mcp::error(value["request"]["id"].clone(), -32000, "Mac submission wait capacity exceeded; this call was not admitted. An earlier request with the same ID may exist; use get_job_status.");
                            sink.send(Message::Text(json!({"type":"rpc_result","id":value["id"],"response":body}).to_string().into())).await?;
                            continue;
                        }
                        let queue = queue.clone(); let root = root.clone();
                        calls.push(async move {
                            let response = crate::mcp::dispatch(&root,&queue,value["request"].clone(),key,Some(deadline)).await;
                            json!({"type":"rpc_result","id":value["id"],"response":response})
                        });
                    }
                    Some(Message::Ping(v)) => sink.send(Message::Pong(v)).await?,
                    Some(Message::Pong(_)) => {},
                    Some(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            // Record only the numeric code, never URLs, credentials,
                            // prompts, or a server-supplied close reason.
                            eprintln!("relay closed (code {}); reconnecting",u16::from(frame.code));
                        }
                        break;
                    }
                    None => break,
                    _ => anyhow::bail!("unsupported relay message"),
                }
            }
        }
    }
    Ok(())
}

pub async fn run(
    root: PathBuf,
    queue: Queue,
    mut configs: watch::Receiver<Config>,
    mut stop: watch::Receiver<bool>,
    status: watch::Sender<String>,
) {
    let mut backoff: u64 = 1;
    loop {
        if *stop.borrow() {
            break;
        }
        let config = configs.borrow().clone();
        if !config.remote_enabled {
            status.send_replace("disabled".into());
            tokio::select! { _ = stop.changed() => break, _ = configs.changed() => { backoff = 1; continue; } }
        }
        status.send_replace("connecting".into());
        let started = Instant::now();
        tokio::select! {
            _ = stop.changed() => break,
            _ = configs.changed() => { backoff = 1; continue; },
            _ = connection(root.clone(),config,queue.clone(),status.clone()) => {},
        }
        status.send_replace("disconnected".into());
        if started.elapsed() > Duration::from_secs(60) {
            backoff = 1;
        }
        let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]) * 4;
        tokio::select! {
            _ = stop.changed() => break,
            _ = configs.changed() => backoff = 1,
            _ = tokio::time::sleep(Duration::from_millis(backoff * 1000 + jitter)) => backoff = (backoff * 2).min(60),
        }
    }
    status.send_replace("disabled".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Task, TaskResult},
        queue::{Backend, BrowserFuture},
    };
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::protocol::Role;

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

    #[tokio::test]
    async fn websocket_status_bypasses_32_pending_submission_calls() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let (stop, receiver) = watch::channel(false);
        let (queue, worker) = Queue::open(dir.path().into(), Arc::new(Blocked), receiver)?;
        let (device, relay) = tokio::io::duplex(1024 * 1024);
        let device = WebSocketStream::from_raw_socket(device, Role::Client, None).await;
        let mut relay = WebSocketStream::from_raw_socket(relay, Role::Server, None).await;
        let runner = tokio::spawn(serve_socket(
            dir.path().into(),
            Config::default(),
            queue.clone(),
            device,
        ));
        for index in 0..32 {
            let key = format!("pending-{index}");
            let request = json!({"jsonrpc":"2.0","id":index,"method":"tools/call","params":{
                "name":"new_task","arguments":{"prompt":"PRIVATE saturation fixture", "request_id":key}}});
            relay
                .send(Message::Text(
                    json!({"type":"rpc","id":key,"request":request,
                "expires_at":crate::queue::now()+230_000})
                    .to_string()
                    .into(),
                ))
                .await?;
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while queue.active() != 32 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let lookup = json!({"jsonrpc":"2.0","id":99,"method":"tools/call","params":{
            "name":"get_job_status","arguments":{"request_id":"pending-0"}}});
        relay
            .send(Message::Text(
                json!({"type":"rpc","id":"lookup","request":lookup,
            "expires_at":crate::queue::now()+230_000})
                .to_string()
                .into(),
            ))
            .await?;
        let frame = tokio::time::timeout(Duration::from_millis(500), relay.next())
            .await?
            .unwrap()?;
        let frame: Value = serde_json::from_str(frame.to_text()?)?;
        assert_eq!(frame["id"], "lookup");
        assert_eq!(
            frame["response"]["result"]["structuredContent"]["outcome"],
            "in_progress"
        );
        assert_eq!(queue.active(), 32);
        runner.abort();
        let _ = runner.await;
        stop.send(true)?;
        worker.await??;
        Ok(())
    }
}
