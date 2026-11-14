//! Synthetic protocol coverage. These tests never open a browser or ChatGPT.
use super::*;
use std::sync::Mutex;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::accept_async;

const BOOTSTRAP: &str = "about:blank#slingring-private-test-nonce";
const CHAT: &str = "https://chatgpt.com/c/followup-test";

#[derive(Clone, Copy, Default)]
enum Scenario {
    #[default]
    Good,
    DuplicateBootstrap,
    WrongTarget,
    WrongDocument,
    MovedAfterQuery,
    AmbiguousInput,
}

type Calls = Arc<Mutex<Vec<Value>>>;

async fn fixture(scenario: Scenario, url: &str) -> (Cdp, Calls, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}/devtools/browser/test",
        listener.local_addr().unwrap()
    );
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let saved = calls.clone();
    let url = url.to_string();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let mut queried = false;
        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else { break };
            let request: Value = serde_json::from_str(&text).unwrap();
            saved.lock().unwrap().push(request.clone());
            let result = match request["method"].as_str().unwrap() {
                "Target.getTargets" => {
                    let mut targets = vec![
                        json!({"targetId":"home","type":"page","url":"https://chatgpt.com/"}),
                        json!({"targetId":"same-url-other-tab","type":"page","url":url}),
                        json!({"targetId":"bound-tab","type":"page","url":BOOTSTRAP}),
                    ];
                    if matches!(scenario, Scenario::DuplicateBootstrap) {
                        targets.push(json!({"targetId":"duplicate","type":"page","url":BOOTSTRAP}));
                    }
                    json!({"targetInfos":targets})
                }
                "Target.getTargetInfo" => {
                    assert_eq!(request["params"]["targetId"], "bound-tab");
                    let moved = matches!(scenario, Scenario::MovedAfterQuery) && queried;
                    let id = if matches!(scenario, Scenario::WrongTarget) {
                        "other"
                    } else {
                        "bound-tab"
                    };
                    json!({"targetInfo":{"targetId":id,"type":"page","url":if moved { "https://chatgpt.com/c/other" } else { &url }}})
                }
                "Target.attachToTarget" => {
                    assert_eq!(request["params"]["targetId"], "bound-tab");
                    json!({"sessionId":"bound-session"})
                }
                "DOM.getDocument" => {
                    assert_eq!(request["sessionId"], "bound-session");
                    json!({"root":{"nodeId":1,"documentURL":if matches!(scenario, Scenario::WrongDocument) { "https://chatgpt.com/" } else { &url }}})
                }
                "DOM.querySelectorAll" => {
                    assert_eq!(request["sessionId"], "bound-session");
                    queried = true;
                    json!({"nodeIds":if matches!(scenario, Scenario::AmbiguousInput) { vec![7,8] } else { vec![7] }})
                }
                "DOM.setFileInputFiles" => {
                    assert_eq!(request["sessionId"], "bound-session");
                    assert_eq!(request["params"]["nodeId"], 7);
                    assert_eq!(request["params"]["files"], json!(["/synthetic/image.png"]));
                    json!({})
                }
                "Target.detachFromTarget" => json!({}),
                other => panic!("unexpected CDP method: {other}"),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        }
    });
    (Cdp::connect(&endpoint).await.unwrap(), calls, server)
}

#[tokio::test]
async fn uploads_follow_the_native_bound_tab_even_with_other_same_url_tabs() {
    for url in [CHAT, "https://chatgpt.com/"] {
        let (mut cdp, calls, server) = fixture(Scenario::Good, url).await;
        let target = cdp.bind_tab(BOOTSTRAP, url).await.unwrap();
        cdp.upload(
            &target,
            "input[type=\"file\"][multiple]",
            &["/synthetic/image.png".into()],
        )
        .await
        .unwrap();
        let methods: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c["method"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            methods.iter().filter(|m| *m == "Target.getTargets").count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|m| *m == "DOM.setFileInputFiles")
                .count(),
            1
        );
        assert!(!methods.iter().any(|m| m.starts_with("Runtime.")));
        drop(cdp);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn missing_or_ambiguous_native_tab_binding_cannot_select_a_url_match() {
    let (mut cdp, calls, server) = fixture(Scenario::DuplicateBootstrap, CHAT).await;
    assert!(cdp.bind_tab(BOOTSTRAP, CHAT).await.is_err());
    assert!(
        cdp.bind_tab("about:blank#another-nonce", CHAT)
            .await
            .is_err()
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
    drop(cdp);
    server.await.unwrap();
}

#[tokio::test]
async fn changed_target_document_and_ambiguous_inputs_never_receive_files() {
    for scenario in [
        Scenario::WrongTarget,
        Scenario::WrongDocument,
        Scenario::MovedAfterQuery,
        Scenario::AmbiguousInput,
    ] {
        let (mut cdp, calls, server) = fixture(scenario, CHAT).await;
        let target = cdp.bind_tab(BOOTSTRAP, CHAT).await.unwrap();
        assert!(
            cdp.upload(
                &target,
                "input[type=\"file\"]",
                &["/synthetic/image.png".into()]
            )
            .await
            .is_err()
        );
        assert!(
            !calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c["method"] == "DOM.setFileInputFiles")
        );
        drop(cdp);
        server.await.unwrap();
    }
}

#[test]
fn both_browser_modes_require_matching_visibility_and_an_isolated_profile() {
    let directory = tempfile::Builder::new()
        .prefix("agent-browser-profile-slingring-test-")
        .tempdir()
        .unwrap();
    let base = format!(
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir={} --remote-debugging-port=0",
        directory.path().display()
    );
    assert_eq!(
        owned_profile_from_command(&base, true).unwrap(),
        directory.path().canonicalize().unwrap()
    );
    assert!(owned_profile_from_command(&base, false).is_err());
    let headless = format!("{base} --headless=new");
    assert!(owned_profile_from_command(&headless, false).is_ok());
    assert!(owned_profile_from_command(&headless, true).is_err());
    assert!(owned_profile_from_command("Chrome --headless=new --user-data-dir=/Users/ashray/Library/Application Support/Google/Chrome", false).is_err());
    let unowned = tempfile::tempdir().unwrap();
    assert!(
        owned_profile_from_command(
            &format!("Chrome --user-data-dir={}", unowned.path().display()),
            true
        )
        .is_err()
    );
}

#[tokio::test]
async fn launcher_passes_persisted_headed_setting_and_fixed_background_flags() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("synthetic-browser");
    std::fs::write(&binary, r#"#!/bin/sh
case "$1" in --headed) mode=headed;; *) mode=headless;; esac
if [ "${AGENT_BROWSER_HEADED+x}" = x ]; then inherited=present; else inherited=absent; fi
if [ "${AGENT_BROWSER_ARGS+x}" = x ]; then inherited=present; fi
if [ "${SLINGRING_PRIVATE_LAUNCH_GUARD+x}" = x ]; then inherited=present; fi
if [ "${SLINGRING_PRIVATE_LAUNCH_SOCKET+x}" = x ]; then inherited=present; fi
if [ "${SLINGRING_PRIVATE_LAUNCH_GENERATION+x}" = x ]; then inherited=present; fi
if [ "${SLINGRING_PRIVATE_LAUNCH_SESSION+x}" = x ]; then inherited=present; fi
pinned=false
webmcp=false
features=false
for argument do
  case "$argument" in
    --pin-tab) pinned=true;;
    --no-webmcp) webmcp=true;;
    --disable-features=MediaRouter,MacAppCodeSignClone,--disable-default-apps,--disable-updater-scheduler) features=true;;
    --user-agent|--executable-path) exit 9;;
  esac
done
printf '{"success":true,"data":{"mode":"%s","headed_env":"%s","pinned":%s,"webmcp":%s,"features":%s}}' "$mode" "$inherited" "$pinned" "$webmcp" "$features"
"#).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    for headed in [false, true] {
        let config = Config {
            browser_binary: binary.to_string_lossy().into_owned(),
            browser_headed: headed,
            ..Config::default()
        };
        let result = agent(&config, &["open", "about:blank"]).await.unwrap();
        assert_eq!(result["mode"], if headed { "headed" } else { "headless" });
        assert_eq!(result["headed_env"], "absent");
        assert_eq!(result["pinned"], true);
        assert_eq!(result["webmcp"], true);
        assert_eq!(result["features"], true);
    }
}

/// Opt-in real-browser check, with an empty profile and no website navigation.
/// The separate home ensures this cannot close the user's Slingring session.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires the installed pinned browser helper and an isolated SLINGRING_HOME"]
async fn real_browser_disables_app_cloning_and_updater_startup() {
    let root = config::root().unwrap().canonicalize().unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("slingring-browser-smoke")
    );
    let profile = tempfile::tempdir().unwrap();
    // Optional, explicit metadata-only integration check. Never print updater
    // history; compare only new process events attributed to our browser PID.
    let updater_history = std::env::var_os("SLINGRING_TEST_UPDATER_HISTORY")
        .map(PathBuf::from)
        .map(|path| {
            let previous = std::fs::read_to_string(&path).unwrap();
            (path, previous.lines().count())
        });
    let config = Config {
        browser_binary: std::env::var("SLINGRING_TEST_BROWSER").unwrap(),
        profile: profile.path().to_string_lossy().into_owned(),
        ..Config::default()
    };
    let result: Result<()> = async {
        agent(&config, &["open", "about:blank"]).await?;
        let endpoint = agent(&config, &["get", "cdp-url"]).await?;
        let mut cdp =
            Cdp::connect(endpoint["cdpUrl"].as_str().context("endpoint missing")?).await?;
        // Chrome 156's updater scheduler first runs 19 seconds after startup.
        // Cover that delayed path, not just immediate app cloning. Do not
        // inspect page content or enable Runtime.
        tokio::time::sleep(Duration::from_secs(30)).await;
        let histograms = cdp
            .call("Browser.getHistograms", json!({"query":"Mac.App"}), None)
            .await?;
        let values = histograms["histograms"]
            .as_array()
            .context("browser histogram response missing")?;
        ensure!(
            !values.iter().any(|value| matches!(
                value["name"].as_str(),
                Some("Mac.AppClonefileError" | "Mac.AppHardLinkError")
            )),
            "Chrome attempted an app clone or executable hard link"
        );
        let info = cdp
            .call("SystemInfo.getProcessInfo", json!({}), None)
            .await?;
        let pid = info["processInfo"]
            .as_array()
            .context("process metadata missing")?
            .iter()
            .find(|entry| entry["type"] == "browser")
            .and_then(|entry| entry["id"].as_u64())
            .context("browser PID missing")?;
        let command = tokio::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .await?;
        let command = String::from_utf8(command.stdout)?;
        ensure!(
            command.contains("--disable-features=MediaRouter,MacAppCodeSignClone")
                && command
                    .split_whitespace()
                    .any(|arg| arg == "--disable-updater-scheduler")
                && command.contains("--headless=new")
                && command.contains(&format!("--user-data-dir={}", profile.path().display())),
            "isolated Chrome did not receive the required launch settings"
        );
        if let Some((path, previous_lines)) = &updater_history {
            let history = std::fs::read_to_string(path)?;
            ensure!(
                history.lines().count() >= *previous_lines,
                "updater history rotated during test"
            );
            for line in history.lines().skip(*previous_lines) {
                let event: Value = serde_json::from_str(line)?;
                ensure!(
                    !(event["eventType"] == "UPDATER_PROCESS"
                        && event["bound"] == "START"
                        && event["parentPid"].as_u64() == Some(pid)),
                    "owned Chrome started an updater process"
                );
            }
        }
        Ok(())
    }
    .await;
    let closed = close(&config).await;
    result.unwrap();
    closed.unwrap();
}

#[tokio::test]
async fn validated_success_is_immediate_even_when_native_and_cdp_cleanup_stall() {
    for disconnected in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/test",
            listener.local_addr().unwrap()
        );
        let (close_seen, closed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "Target.closeTarget");
            assert_eq!(request["params"]["targetId"], "old-owned-controller");
            close_seen.send(()).unwrap();
            // Do not answer CDP. Bounded cleanup must drop this connection.
            let _ = socket.next().await;
        });
        let cdp = Cdp::connect(&endpoint).await.unwrap();
        let (socket, peer) = UnixStream::pair().unwrap();
        let mut peer = if disconnected {
            drop(peer);
            None
        } else {
            Some(peer)
        };
        let expected = TaskResult {
            pause_status: None,
            requested_effort: None,
            verified_effort: None,
            effort_verification: Default::default(),
            job_id: "proved".into(),
            url: CHAT.into(),
            reasoning_effort: Some("Medium".into()),
            evidence: "thinking".into(),
        };
        let before = tokio::time::Instant::now();
        let result = handoff_started(expected.clone(), socket, cdp, "old-owned-controller".into());
        assert_eq!(result, expected);
        assert!(before.elapsed() < Duration::from_millis(50));
        if let Some(peer) = peer.as_mut() {
            let ack = tokio::time::timeout(Duration::from_millis(500), native::read(peer))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(ack["type"], "ack");
            // Never send released. It must not delay the already-returned URL.
        }
        tokio::time::timeout(Duration::from_secs(2), closed)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.evidence, "thinking");
    }
}
