//! The patched browser owns Chrome launch and its fingerprint settings. Browser
//! CDP is limited to extension installation, owned tabs and local file upload;
//! it never enables Runtime events or reads generated answers.
use crate::{
    config::{self, Config},
    model::{NewTask, StartError, TaskResult, canonical_url},
    native,
};
use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::net::{TcpStream, UnixListener};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

pub fn install_extension(root: &Path) -> Result<PathBuf> {
    native::assets(root)
}
pub fn session(root: &Path) -> String {
    let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    format!("slingring-{}", &digest[..12])
}
async fn owned_profile(config: &Config, endpoint: &str) -> Result<PathBuf> {
    let info = agent(config, &["session", "info"]).await?;
    let parent = info["runtime"]["backgroundPid"]
        .as_u64()
        .or_else(|| info["pid"].as_u64())
        .context("browser owner process is unknown")?;
    let processes = tokio::process::Command::new("/bin/ps")
        .args(["-axo", "ppid=,command="])
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        processes.status.success() && processes.stdout.len() < 4 * 1024 * 1024,
        "cannot inspect owned browser process"
    );
    let endpoint = url::Url::parse(endpoint)?;
    let port = endpoint.port().context("missing browser port")?.to_string();
    for row in String::from_utf8(processes.stdout)?.lines() {
        let Some((ppid, command)) = row.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        if ppid.parse::<u64>().ok() != Some(parent)
            || !command.contains("Google Chrome.app/Contents/MacOS/Google Chrome ")
        {
            continue;
        }
        let Some((_, value)) = command.split_once("--user-data-dir=") else {
            continue;
        };
        let path = PathBuf::from(value.split(" --").next().unwrap_or("").trim());
        let Ok(active) = std::fs::read_to_string(path.join("DevToolsActivePort")) else {
            continue;
        };
        let mut lines = active.lines();
        if lines.next() == Some(port.as_str()) && lines.next() == Some(endpoint.path()) {
            ensure!(
                path.is_absolute() && !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
                "invalid owned browser profile"
            );
            return Ok(path);
        }
    }
    anyhow::bail!("could not locate the profile owned by Slingring's browser")
}

async fn agent(config: &Config, args: &[&str]) -> Result<Value> {
    ensure!(
        Path::new(&config.browser_binary)
            .file_name()
            .is_none_or(|name| name != "agent-browser"),
        "upstream agent-browser is not an allowed fallback"
    );
    let name = session(&config::root()?);
    let idle = format!(
        "{}s",
        config
            .browser_idle_secs
            .unwrap_or_else(|| config.start_timeout_secs.saturating_add(60))
    );
    let mut command = tokio::process::Command::new(&config.browser_binary);
    command
        .args([
            "--session",
            &name,
            "--profile",
            &config.profile,
            "--idle-timeout",
            &idle,
            "--json",
        ])
        .args(args)
        .env("AGENT_BROWSER_DEFAULT_TIMEOUT", "60000")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(65), command.output())
        .await
        .context("browser command timed out")??;
    ensure!(
        output.stdout.len() <= 2 * 1024 * 1024,
        "browser response too large"
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).context("browser command returned invalid JSON")?;
    ensure!(
        output.status.success() && value["success"] == true,
        "patched browser command failed; run agent-browser-browse doctor"
    );
    Ok(value["data"].clone())
}

struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: u64,
}
impl Cdp {
    async fn connect(endpoint: &str) -> Result<Self> {
        let url = url::Url::parse(endpoint)?;
        ensure!(
            url.scheme() == "ws"
                && url.host_str() == Some("127.0.0.1")
                && url.path().starts_with("/devtools/browser/"),
            "unexpected browser endpoint"
        );
        let (socket, _) = connect_async(endpoint).await?;
        Ok(Self { socket, id: 0 })
    }
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.id += 1;
        let mut request = json!({"id":self.id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(request.to_string().into()))
            .await?;
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(message) = self.socket.next().await {
                match message? {
                    Message::Text(text) => {
                        ensure!(
                            text.len() <= 4 * 1024 * 1024,
                            "browser protocol response too large"
                        );
                        let value: Value = serde_json::from_str(&text)?;
                        if value["id"] != self.id {
                            continue;
                        }
                        ensure!(
                            value.get("error").is_none(),
                            "browser operation {method} was rejected"
                        );
                        return Ok(value["result"].clone());
                    }
                    Message::Ping(p) => self.socket.send(Message::Pong(p)).await?,
                    _ => {}
                }
            }
            anyhow::bail!("browser protocol disconnected")
        })
        .await?
    }
    async fn upload(&mut self, selector: &str, files: &[PathBuf]) -> Result<()> {
        ensure!(
            selector.len() <= 1024 && selector.starts_with("input[type=\"file\"]"),
            "unexpected image selector"
        );
        let targets = self.call("Target.getTargets", json!({}), None).await?;
        let candidates: Vec<_> = targets["targetInfos"]
            .as_array()
            .context("missing browser targets")?
            .iter()
            .filter(|t| t["type"] == "page" && t["url"] == "https://chatgpt.com/")
            .collect();
        ensure!(
            candidates.len() == 1,
            "could not identify exactly one owned new chat"
        );
        let target = candidates[0]["targetId"]
            .as_str()
            .context("missing chat target")?;
        let attached = self
            .call(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await?;
        let session = attached["sessionId"]
            .as_str()
            .context("could not attach image input")?;
        let outcome = async {
            let doc = self
                .call("DOM.getDocument", json!({"depth":0}), Some(session))
                .await?;
            let input = self
                .call(
                    "DOM.querySelector",
                    json!({"nodeId":doc["root"]["nodeId"],"selector":selector}),
                    Some(session),
                )
                .await?;
            ensure!(
                input["nodeId"].as_i64().is_some_and(|n| n > 0),
                "image input not found"
            );
            self.call(
                "DOM.setFileInputFiles",
                json!({"nodeId":input["nodeId"],"files":files}),
                Some(session),
            )
            .await?;
            Ok(())
        }
        .await;
        let _ = self
            .call(
                "Target.detachFromTarget",
                json!({"sessionId":session}),
                None,
            )
            .await;
        outcome
    }
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn failure(code: &str, submitted: bool, url: Option<String>) -> anyhow::Error {
    StartError {
        code: code.into(),
        may_have_submitted: submitted,
        url: url.filter(|v| canonical_url(v)),
    }
    .into()
}

pub async fn run(config: &Config, task: &NewTask, task_id: &str) -> Result<TaskResult> {
    let began = tokio::time::Instant::now();
    let root = config::root()?;
    native::register(&root).map_err(|_| failure("native_registration_failed", false, None))?;
    let extension = native::assets(&root)?;
    agent(config, &["open", "about:blank"])
        .await
        .map_err(|_| failure("browser_start_failed", false, None))?;
    let endpoint = agent(config, &["get", "cdp-url"])
        .await
        .map_err(|_| failure("browser_endpoint_unavailable", false, None))?;
    let endpoint = endpoint["cdpUrl"]
        .as_str()
        .context("browser endpoint missing")?;
    let profile = owned_profile(config, endpoint)
        .await
        .map_err(|_| failure("owned_profile_unavailable", false, None))?;
    native::register_owned_profile(&root, &profile)?;
    let mut cdp = Cdp::connect(endpoint).await?;
    let loaded = cdp
        .call("Extensions.loadUnpacked", json!({"path":extension}), None)
        .await
        .map_err(|_| failure("extension_installation_failed", false, None))?;
    ensure!(
        loaded["id"] == native::EXTENSION_ID,
        "unexpected companion extension identity"
    );
    let socket = root.join("browser.sock");
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let _cleanup = Cleanup(socket);
    let token = uuid::Uuid::new_v4().to_string();
    let mut bridge = url::Url::parse(&format!(
        "chrome-extension://{}/bridge.html",
        native::EXTENSION_ID
    ))?;
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", &token)
        .append_pair("job", task_id)
        .finish();
    bridge.set_fragment(Some(&fragment));
    let controller = cdp
        .call(
            "Target.createTarget",
            json!({"url":bridge.as_str(),"background":false}),
            None,
        )
        .await?;
    let controller_id = controller["targetId"]
        .as_str()
        .context("extension controller missing")?
        .to_string();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
        .await
        .map_err(|_| failure("native_connection_failed", false, None))??;
    let hello = tokio::time::timeout(Duration::from_secs(5), native::read(&mut socket)).await??;
    ensure!(
        hello["type"] == "hello" && hello["token"] == token && hello["job"] == task_id,
        "native session authentication failed"
    );
    native::write(&mut socket,&json!({"type":"task","job":task_id,"prompt":task.prompt,
        "effort":task.reasoning_effort,"image_count":task.image_paths.len(),"timeout_ms":config.start_timeout_secs*1000,
        "deadline_ms":crate::queue::now()+Duration::from_secs(config.start_timeout_secs).saturating_sub(began.elapsed()).as_millis() as u64})).await?;
    let mut submitted = false;
    loop {
        let event = native::read(&mut socket)
            .await
            .map_err(|_| failure("native_connection_lost", submitted, None))?;
        ensure!(event["job"] == task_id, "unexpected native job");
        match event["type"].as_str() {
            Some("upload") if !submitted && !task.image_paths.is_empty() => {
                let selector = event["selector"]
                    .as_str()
                    .context("image input selector missing")?;
                cdp.upload(selector, &task.image_paths)
                    .await
                    .map_err(|_| failure("image_upload_failed", false, None))?;
                native::write(&mut socket, &json!({"type":"uploaded","ok":true})).await?;
            }
            Some("will_submit") if !submitted => {
                // The queue persisted submission intent before this method ran.
                submitted = true;
                native::write(&mut socket, &json!({"type":"continue"})).await?;
            }
            Some("started") if submitted => {
                let result = TaskResult {
                    job_id: task_id.into(),
                    url: event["url"].as_str().context("task URL missing")?.into(),
                    reasoning_effort: event["effort"].as_str().map(str::to_string),
                    evidence: event["evidence"]
                        .as_str()
                        .context("activity evidence missing")?
                        .into(),
                };
                result.validate()?;
                ensure!(
                    task.reasoning_effort.is_none()
                        || result.reasoning_effort == task.reasoning_effort,
                    "effort verification mismatch"
                );
                native::write(&mut socket, &json!({"type":"ack"})).await?;
                let _ =
                    tokio::time::timeout(Duration::from_secs(3), native::read(&mut socket)).await;
                let _ = cdp
                    .call(
                        "Target.closeTarget",
                        json!({"targetId":controller_id}),
                        None,
                    )
                    .await;
                return Ok(result);
            }
            Some("failed") => {
                let code = event["code"]
                    .as_str()
                    .filter(|s| {
                        s.len() <= 100 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    })
                    .unwrap_or("extension_failed");
                return Err(failure(
                    code,
                    submitted || event["submitted"] == true,
                    event["url"].as_str().map(str::to_string),
                ));
            }
            _ => return Err(failure("invalid_native_event", submitted, None)),
        }
    }
}
pub async fn close(config: &Config) -> Result<()> {
    agent(config, &["close"]).await.map(|_| ())
}
