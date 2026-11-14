//! The patched browser owns Chrome launch and its fingerprint settings. Browser
//! CDP is limited to extension installation, owned tabs and local file upload;
//! it never enables Runtime events or reads generated answers.
use crate::{
    config::{self, Config},
    model::{StartError, Task, TaskResult, canonical_url},
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
    sync::Arc,
    time::Duration,
};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

pub fn install_extension(root: &Path) -> Result<PathBuf> {
    native::assets(root)
}
pub fn session(root: &Path) -> String {
    let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    format!("slingring-{}", &digest[..12])
}
#[derive(Debug)]
struct RunnerProblem(&'static str);
impl std::fmt::Display for RunnerProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for RunnerProblem {}
fn problem(kind: &'static str) -> anyhow::Error {
    RunnerProblem(kind).into()
}
fn launch_stage(error: &anyhow::Error, retry: bool) -> &'static str {
    match (retry, error.downcast_ref::<RunnerProblem>().map(|e| e.0)) {
        (false, Some("timeout")) => "browser_launch_timeout",
        (false, Some("spawn")) => "browser_launch_spawn_error",
        (false, Some("exit")) => "browser_launch_nonzero_exit",
        (false, Some("json")) => "browser_launch_invalid_json",
        (false, Some("response")) => "browser_launch_unsuccessful",
        (false, _) => "browser_launch_other_error",
        (true, Some("timeout")) => "browser_retry_timeout",
        (true, Some("spawn")) => "browser_retry_spawn_error",
        (true, Some("exit")) => "browser_retry_nonzero_exit",
        (true, Some("json")) => "browser_retry_invalid_json",
        (true, Some("response")) => "browser_retry_unsuccessful",
        (true, _) => "browser_retry_other_error",
    }
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
    // The persisted application setting owns visibility, not a daemon's
    // inherited shell environment. Fork defaults retain fingerprint settings.
    command.env_remove("AGENT_BROWSER_HEADED");
    command.env_remove("AGENT_BROWSER_ARGS");
    command.env_remove("AGENT_BROWSER_NO_WEBMCP");
    if config.browser_headed {
        command.arg("--headed");
    }
    command
        .args([
            "--session",
            &name,
            "--profile",
            &config.profile,
            "--idle-timeout",
            &idle,
            // Keep the helper anchored to its initial blank tab. Otherwise its
            // discovery of our extension-created tabs steals the active target
            // and a later open could race owned controller cleanup.
            "--pin-tab",
            // Slingring never uses the fork's experimental WebMCP discovery.
            // Keeping it enabled changes Chrome feature flags and adds work
            // to an already heavy ChatGPT renderer.
            "--no-webmcp",
            // Do not discover LAN casting targets or install Chrome's default
            // apps inside our isolated throwaway profile. Chrome's macOS
            // update-survival clone also hard-links its installed executable,
            // triggering App Management consent attributed to Slingring. Our
            // short-lived browser does not need that bundle-writing feature.
            // Also skip this process's delayed GoogleUpdater scheduler: it
            // wakes the global updater, which can modify installed Chrome and
            // inherit Slingring's responsibility. Normal Chrome's updater,
            // launchd jobs, update policy and signature checks stay unchanged.
            "--args",
            "--disable-features=MediaRouter,MacAppCodeSignClone,--disable-default-apps,--disable-updater-scheduler",
            "--json",
        ])
        .args(args)
        .env("AGENT_BROWSER_DEFAULT_TIMEOUT", "60000")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(Duration::from_secs(65), command.output()).await {
        Err(_) => return Err(problem("timeout")),
        Ok(Err(_)) => return Err(problem("spawn")),
        Ok(Ok(output)) => output,
    };
    ensure!(
        output.stdout.len() <= 2 * 1024 * 1024,
        "browser response too large"
    );
    if !output.status.success() {
        return Err(problem("exit"));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| problem("json"))?;
    if value["success"] != true {
        return Err(problem("response"));
    }
    Ok(value["data"].clone())
}

struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: u64,
}
#[derive(Clone, Debug)]
struct ChatTarget {
    id: String,
    url: String,
}
fn owned_profile_from_command(command: &str, headed: bool) -> Result<PathBuf> {
    let headless = command
        .split_whitespace()
        .any(|word| word == "--headless" || word.starts_with("--headless="));
    ensure!(
        headless != headed,
        "owned browser visibility does not match configuration"
    );
    let directory = command
        .split_once("--user-data-dir=")
        .map(|(_, rest)| rest.split(" --").next().unwrap_or("").trim())
        .filter(|value| !value.is_empty())
        .context("owned browser profile directory missing")?;
    let directory = PathBuf::from(directory).canonicalize()?;
    ensure!(
        directory.starts_with(std::env::temp_dir().canonicalize()?)
            && directory
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.starts_with("agent-browser-profile-")),
        "refusing to register outside an isolated browser profile"
    );
    Ok(directory)
}
impl Cdp {
    async fn owned_profile(&mut self, headed: bool) -> Result<PathBuf> {
        let info = self
            .call("SystemInfo.getProcessInfo", json!({}), None)
            .await?;
        let pid = info["processInfo"]
            .as_array()
            .context("browser process metadata missing")?
            .iter()
            .find(|entry| entry["type"] == "browser")
            .and_then(|entry| entry["id"].as_u64())
            .filter(|pid| *pid > 0 && *pid <= u32::MAX as u64)
            .context("browser PID missing")?;
        trace(&format!("browser_pid={pid}"));
        let output = tokio::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .stdin(Stdio::null())
            .output()
            .await?;
        ensure!(
            output.status.success(),
            "could not inspect owned browser process"
        );
        let command = String::from_utf8(output.stdout)?;
        owned_profile_from_command(&command, headed)
    }
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
    async fn bind_tab(&mut self, bootstrap: &str, url: &str) -> Result<ChatTarget> {
        let targets = self.call("Target.getTargets", json!({}), None).await?;
        let candidates: Vec<_> = targets["targetInfos"]
            .as_array()
            .context("missing browser targets")?
            .iter()
            .filter(|t| t["type"] == "page" && t["url"] == bootstrap)
            .collect();
        ensure!(
            candidates.len() == 1,
            "could not bind exactly one owned task tab"
        );
        let id = candidates[0]["targetId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("missing chat target")?
            .to_string();
        Ok(ChatTarget {
            id,
            url: url.into(),
        })
    }
    async fn assert_target(&mut self, target: &ChatTarget) -> Result<()> {
        let info = self
            .call("Target.getTargetInfo", json!({"targetId":target.id}), None)
            .await?;
        ensure!(
            info["targetInfo"]["targetId"] == target.id
                && info["targetInfo"]["type"] == "page"
                && info["targetInfo"]["url"] == target.url,
            "owned task tab changed"
        );
        Ok(())
    }
    async fn upload(
        &mut self,
        target: &ChatTarget,
        selector: &str,
        files: &[PathBuf],
    ) -> Result<()> {
        ensure!(
            selector.len() <= 1024 && selector.starts_with("input[type=\"file\"]"),
            "unexpected image selector"
        );
        self.assert_target(target).await?;
        let attached = self
            .call(
                "Target.attachToTarget",
                json!({"targetId":target.id,"flatten":true}),
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
            ensure!(
                doc["root"]["documentURL"] == target.url,
                "image document changed"
            );
            let input = self
                .call(
                    "DOM.querySelectorAll",
                    json!({"nodeId":doc["root"]["nodeId"],"selector":selector}),
                    Some(session),
                )
                .await?;
            let nodes = input["nodeIds"]
                .as_array()
                .context("image input not found")?;
            ensure!(
                nodes.len() == 1 && nodes[0].as_i64().is_some_and(|n| n > 0),
                "image input is missing or ambiguous"
            );
            // Recheck after DOM discovery. Navigations invalidate the captured
            // document node, so setFileInputFiles cannot follow a new document.
            self.assert_target(target).await?;
            self.call(
                "DOM.setFileInputFiles",
                json!({"nodeId":nodes[0],"files":files}),
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
fn trace(stage: &str) {
    if std::env::var_os("SLINGRING_TRACE").is_some() {
        eprintln!("slingring browser: {stage}");
    }
}
fn effort_code(value: Option<&str>) -> Option<&'static str> {
    match value {
        Some("Instant") => Some("instant"),
        Some("Medium") => Some("medium"),
        Some("High") => Some("high"),
        Some("Extra High") => Some("extra_high"),
        Some("Pro") => Some("pro"),
        None => Some("missing"),
        _ => None,
    }
}
/// Browser payloads are untrusted. Emit only static vocabulary, never model
/// picker text, page data, user messages, URLs, or arbitrary error strings.
fn safe_effort_observation(event: &Value) -> Option<(&'static str, String)> {
    let phase = match event["phase"].as_str()? {
        "prepare" => "prepare",
        "before_authorization" => "before_authorization",
        "after_authorization" => "after_authorization",
        _ => return None,
    };
    let expected = effort_code(event["expected"].as_str())?;
    if expected == "missing" {
        return None;
    }
    let observed = effort_code(event["observed"].as_str())?;
    let kind = match event["kind"].as_str()? {
        "known" => "known",
        "missing" => "missing",
        "ambiguous" => "ambiguous",
        "unrecognized" => "unrecognized",
        _ => return None,
    };
    if (observed == "missing" && kind == "known") || (observed != "missing" && kind != "known") {
        return None;
    }
    Some((
        phase,
        format!("expected_{expected}_observed_{observed}_{kind}"),
    ))
}

fn handoff_started(
    result: TaskResult,
    mut socket: UnixStream,
    mut cdp: Cdp,
    controller_id: String,
) -> TaskResult {
    // Activity is already validated. Cleanup must not turn proven submission
    // into uncertainty if the native channel, CDP, or outer deadline expires.
    // This task owns only the old accepted stream/connection/controller ID. It
    // never removes the shared socket path or closes the named browser session.
    let _cleanup = tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            native::write(&mut socket, &json!({"type":"ack"})).await?;
            native::read(&mut socket).await
        })
        .await;
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            cdp.call(
                "Target.closeTarget",
                json!({"targetId":controller_id}),
                None,
            ),
        )
        .await;
    });
    result
}

pub async fn run_authorized(
    config: &Config,
    task: &Task,
    task_id: &str,
    progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    gate: crate::queue::SubmissionGate,
) -> Result<TaskResult> {
    trace("launch");
    progress("browser_launch");
    let started_at = tokio::time::Instant::now();
    let deadline =
        started_at + Duration::from_secs(config.start_timeout_secs.saturating_sub(5).max(1));
    let root = config::root()?;
    let extension = native::assets(&root)?;
    if let Err(error) = agent(config, &["open", "about:blank"]).await {
        // This is strictly before extension installation and before ChatGPT is
        // opened. A stale runner/socket may be recreated once without any risk
        // of replaying a prompt. No submission or UI action is retried here.
        progress(launch_stage(&error, false));
        trace("retry_blank_browser_launch");
        progress("browser_retry_cleanup");
        let _ = tokio::time::timeout(Duration::from_secs(20), close(config)).await;
        progress("browser_retry");
        if let Err(error) = agent(config, &["open", "about:blank"]).await {
            progress(launch_stage(&error, true));
            return Err(failure("browser_start_failed", false, None));
        }
    }
    progress("browser_endpoint");
    let endpoint = agent(config, &["get", "cdp-url"])
        .await
        .map_err(|_| failure("browser_endpoint_unavailable", false, None))?;
    let mut cdp = Cdp::connect(
        endpoint["cdpUrl"]
            .as_str()
            .context("browser endpoint missing")?,
    )
    .await?;
    progress("browser_profile_validation");
    if std::env::var_os("SLINGRING_TRACE").is_some() {
        config::atomic(
            &root.join("debug-browser.json"),
            &serde_json::to_vec(&endpoint)?,
        )?;
    }
    let browser_data = cdp
        .owned_profile(config.browser_headed)
        .await
        .map_err(|_| failure("isolated_profile_unverified", false, None))?;
    progress("native_registration");
    native::register(&root, &browser_data)
        .map_err(|_| failure("native_registration_failed", false, None))?;
    progress("extension_loading");
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
    progress("native_handshake");
    let controller_id = controller["targetId"]
        .as_str()
        .context("extension controller missing")?
        .to_string();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
        .await
        .map_err(|_| failure("native_connection_failed", false, None))??;
    let hello = tokio::time::timeout(Duration::from_secs(5), native::read(&mut socket)).await??;
    trace("native_connected");
    ensure!(
        hello["type"] == "hello" && hello["token"] == token && hello["job"] == task_id,
        "native session authentication failed"
    );
    let bootstrap = format!("about:blank#slingring-{token}");
    native::write(&mut socket,&json!({"type":"task","job":task_id,"operation":task.operation(),"prompt":task.prompt(),"tab_url":bootstrap,
        "chatgpt_url":task.conversation_url(),"effort":task.reasoning_effort(),"image_count":task.image_paths().len(),"timeout_ms":config.start_timeout_secs*1000,
        "deadline_ms":crate::queue::now()+deadline.saturating_duration_since(tokio::time::Instant::now()).as_millis() as u64})).await?;
    progress(if task.conversation_url().is_some() {
        "conversation_loading"
    } else {
        "chat_mode_verification"
    });
    let mut submitted = false;
    let mut target = None;
    let mut conversation_url = task.conversation_url().map(str::to_string);
    loop {
        let event = tokio::time::timeout_at(deadline, native::read(&mut socket))
            .await
            .map_err(|_| {
                failure(
                    "generation_not_started",
                    submitted,
                    conversation_url.clone(),
                )
            })?
            .map_err(|_| {
                failure(
                    "native_connection_lost",
                    submitted,
                    conversation_url.clone(),
                )
            })?;
        ensure!(event["job"] == task_id, "unexpected native job");
        if let Some(kind) = event["type"].as_str().filter(|value| {
            matches!(
                *value,
                "upload" | "will_submit" | "started" | "paused" | "failed"
            )
        }) {
            trace(kind);
        }
        match event["type"].as_str() {
            Some("composer_state") => {
                if let Some(state) = event["state"].as_str().filter(|state| {
                    matches!(
                        *state,
                        "missing" | "hidden" | "ambiguous" | "disabled" | "ready"
                    )
                }) {
                    crate::diagnostics::event(
                        &root,
                        task_id,
                        "composer_state",
                        "not_ready",
                        state,
                        started_at.elapsed().as_millis() as u64,
                    );
                }
            }
            Some("effort_state") => {
                if let Some((phase, detail)) = safe_effort_observation(&event) {
                    crate::diagnostics::event(
                        &root,
                        task_id,
                        "effort_readback",
                        phase,
                        &detail,
                        started_at.elapsed().as_millis() as u64,
                    );
                }
            }
            Some("bind_tab") if !submitted && target.is_none() => {
                target = Some(
                    cdp.bind_tab(
                        &bootstrap,
                        task.conversation_url().unwrap_or("https://chatgpt.com/"),
                    )
                    .await
                    .map_err(|_| failure("tab_binding_failed", false, conversation_url.clone()))?,
                );
                native::write(&mut socket, &json!({"type":"tab_bound","ok":true})).await?;
            }
            Some("progress") => {
                let stage = match event["stage"].as_str() {
                    Some("chat_loading") => "chat_loading",
                    Some("chat_mode_verification") => "chat_mode_verification",
                    Some("conversation_loading") => "conversation_loading",
                    Some("composer_ready") => "composer_ready",
                    Some("composer_wait") => "composer_wait",
                    Some("effort_selection") => "effort_selection",
                    Some("prompt_filling") => "prompt_filling",
                    Some("prompt_verification") => "prompt_verification",
                    Some("attachment_wait") => "attachment_wait",
                    Some("send_ready") => "send_ready",
                    Some("activity_wait") => "activity_wait",
                    Some("submission_recovery") => "submission_recovery",
                    Some("pause_controls") => "pause_controls",
                    Some("pause_verification") => "pause_verification",
                    _ => return Err(failure("invalid_native_event", submitted, conversation_url)),
                };
                progress(stage);
            }
            Some("conversation") if submitted => {
                let url = event["url"]
                    .as_str()
                    .filter(|url| canonical_url(url))
                    .context("invalid conversation URL")?;
                if task.conversation_url().is_some_and(|target| target != url) {
                    return Err(failure("conversation_changed", submitted, conversation_url));
                }
                conversation_url = Some(url.into());
                trace("conversation_identified");
            }
            Some("upload") if !submitted && !task.image_paths().is_empty() => {
                progress("image_upload");
                let selector = event["selector"]
                    .as_str()
                    .context("image input selector missing")?;
                let owned = target.as_ref().ok_or_else(|| {
                    failure("tab_binding_failed", false, conversation_url.clone())
                })?;
                cdp.upload(owned, selector, task.image_paths())
                    .await
                    .map_err(|_| failure("image_upload_failed", false, None))?;
                native::write(&mut socket, &json!({"type":"uploaded","ok":true})).await?;
            }
            Some("will_submit") if !submitted && !task.is_pause() => {
                let owned = target.as_ref().ok_or_else(|| {
                    failure("tab_binding_failed", false, conversation_url.clone())
                })?;
                cdp.assert_target(owned).await.map_err(|_| {
                    failure("conversation_changed", false, conversation_url.clone())
                })?;
                // A fallible durable barrier must precede the native ack. A
                // progress event alone cannot authorize a potentially real send.
                gate().map_err(|_| {
                    failure(
                        "submission_intent_not_persisted",
                        false,
                        conversation_url.clone(),
                    )
                })?;
                submitted = true;
                progress("submission_authorized");
                native::write(&mut socket, &json!({"type":"continue"})).await?;
            }
            Some("started") if submitted => {
                let mut result = TaskResult {
                    pause_status: None,
                    requested_effort: None,
                    verified_effort: None,
                    effort_verification: Default::default(),
                    job_id: task_id.into(),
                    url: event["url"].as_str().context("task URL missing")?.into(),
                    reasoning_effort: event["effort"].as_str().map(str::to_string),
                    evidence: event["evidence"]
                        .as_str()
                        .context("activity evidence missing")?
                        .into(),
                };
                result.record_effort(task.reasoning_effort());
                task.validate_result(&result)
                    .map_err(|_| failure("invalid_start_result", true, conversation_url.clone()))?;
                progress("activity_confirmed");
                // The listener and its path guard drop synchronously here,
                // before the serial worker can begin another job.
                return Ok(handoff_started(result, socket, cdp, controller_id));
            }
            Some("paused") if task.is_pause() && !submitted => {
                let owned = target.as_ref().ok_or_else(|| {
                    failure("tab_binding_failed", false, conversation_url.clone())
                })?;
                cdp.assert_target(owned).await.map_err(|_| {
                    failure("conversation_changed", false, conversation_url.clone())
                })?;
                let result = TaskResult {
                    job_id: task_id.into(),
                    url: event["url"].as_str().context("pause URL missing")?.into(),
                    reasoning_effort: None,
                    requested_effort: None,
                    verified_effort: None,
                    effort_verification: crate::model::EffortVerification::NotRequested,
                    pause_status: event["status"].as_str().map(str::to_string),
                    evidence: event["evidence"]
                        .as_str()
                        .context("pause evidence missing")?
                        .into(),
                };
                task.validate_result(&result)
                    .map_err(|_| failure("pause_unverified", false, conversation_url.clone()))?;
                progress("pause_confirmed");
                return Ok(handoff_started(result, socket, cdp, controller_id));
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
                    event["url"]
                        .as_str()
                        .filter(|url| task.conversation_url().is_none_or(|target| target == *url))
                        .map(str::to_string)
                        .or(conversation_url),
                ));
            }
            _ => return Err(failure("invalid_native_event", submitted, conversation_url)),
        }
    }
}
pub async fn close(config: &Config) -> Result<()> {
    trace("close");
    if let Ok(root) = config::root() {
        let _ = std::fs::remove_file(root.join("debug-browser.json"));
    }
    agent(config, &["close"]).await.map(|_| ())
}

#[cfg(test)]
mod diagnostic_tests {
    use super::{launch_stage, problem, safe_effort_observation};

    #[test]
    fn effort_diagnostics_accept_only_static_safe_metadata() {
        let event = serde_json::json!({"phase":"prepare","expected":"Extra High",
            "observed":"High","kind":"known"});
        assert_eq!(
            safe_effort_observation(&event),
            Some(("prepare", "expected_extra_high_observed_high_known".into()))
        );
        for field in ["expected", "observed", "kind", "phase"] {
            let mut corrupt = event.clone();
            corrupt[field] = serde_json::json!("PRIVATE DOM or prompt text");
            assert!(safe_effort_observation(&corrupt).is_none(), "{field}");
        }
        let missing = serde_json::json!({"phase":"prepare","expected":"Extra High",
            "observed":null,"kind":"missing"});
        assert_eq!(
            safe_effort_observation(&missing),
            Some((
                "prepare",
                "expected_extra_high_observed_missing_missing".into()
            ))
        );
    }

    #[test]
    fn runner_categories_are_static_and_identify_first_and_second_attempts() {
        for (kind, first, second) in [
            ("timeout", "browser_launch_timeout", "browser_retry_timeout"),
            (
                "spawn",
                "browser_launch_spawn_error",
                "browser_retry_spawn_error",
            ),
            (
                "exit",
                "browser_launch_nonzero_exit",
                "browser_retry_nonzero_exit",
            ),
            (
                "json",
                "browser_launch_invalid_json",
                "browser_retry_invalid_json",
            ),
            (
                "response",
                "browser_launch_unsuccessful",
                "browser_retry_unsuccessful",
            ),
        ] {
            let err = problem(kind);
            assert_eq!(launch_stage(&err, false), first);
            assert_eq!(launch_stage(&err, true), second);
        }
        let unknown = anyhow::anyhow!("SENSITIVE unexpected error");
        assert_eq!(launch_stage(&unknown, false), "browser_launch_other_error");
        assert_eq!(launch_stage(&unknown, true), "browser_retry_other_error");
    }
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod boundary_tests;
