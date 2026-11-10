use assert_cmd::Command;
use predicates::prelude::*;

fn cli(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slingring"));
    command.env("SLINGRING_HOME", home);
    command
}
#[test]
fn help_and_version_are_product_specific() {
    let home = tempfile::tempdir().unwrap();
    cli(home.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("new-task"))
        .stdout(predicate::str::contains("hello").not());
    cli(home.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("slingring "));
}
#[test]
fn status_does_not_start_a_service_or_browser() {
    let home = tempfile::tempdir().unwrap();
    cli(home.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"running\":false"));
    assert!(!home.path().join("slingring.sock").exists());
    assert!(!home.path().join("browser.sock").exists());
    let status = cli(home.path())
        .args(["jobs", "--request-id", "never-sent", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&status).unwrap();
    assert_eq!(value["known"], false);
    assert_eq!(value["outcome"], "unknown");
    assert!(!home.path().join("slingring.sock").exists());
}
#[test]
fn login_commands_are_separate_and_status_is_read_only() {
    let home = tempfile::tempdir().unwrap();
    cli(home.path())
        .args(["login", "menu-bar", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("on"))
        .stdout(predicate::str::contains("off"))
        .stdout(predicate::str::contains("status"));
    cli(home.path())
        .args(["login", "service", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("on"))
        .stdout(predicate::str::contains("off"))
        .stdout(predicate::str::contains("status"));
    let result = cli(home.path())
        .args(["login", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert!(status["menu_bar"].is_object());
    assert!(status["service"].is_object());
    assert!(!home.path().join("login-preferences.json").exists());
    assert!(!home.path().join("slingring.sock").exists());
}
#[test]
fn service_start_stop_restart_do_not_kickstart_other_state_directories() {
    // This process has its own HOME and SLINGRING_HOME. On developer machines
    // the regular Slingring LaunchAgent may already be loaded, but its plist
    // points at a different state directory and must never be kickstarted.
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let root = home.join("state");
    let command = |args: &[&str]| {
        let mut cli = Command::new(env!("CARGO_BIN_EXE_slingring"));
        cli.env("HOME", &home)
            .env("SLINGRING_HOME", &root)
            .args(args);
        cli
    };
    let bytes = command(&["service", "start"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status["running"], true);
    command(&["service", "start"]).assert().success();
    command(&["service", "restart"]).assert().success();
    command(&["service", "stop"]).assert().success();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &command(&["status", "--json"])
                .assert()
                .success()
                .get_output()
                .stdout
        )
        .unwrap()["running"],
        false
    );
}
#[test]
fn config_round_trip_and_remote_errors_do_not_launch_a_browser() {
    let home = tempfile::tempdir().unwrap();
    cli(home.path())
        .args(["config", "set", "default-effort", "med"])
        .assert()
        .success();
    cli(home.path())
        .args(["config", "get", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"default_effort\":\"Medium\""));
    cli(home.path()).args(["remote", "on"]).assert().failure();
    cli(home.path())
        .args(["new-task", "", "--json"])
        .assert()
        .failure();
    cli(home.path())
        .args(["new-task", "task", "--image", "/does/not/exist.png"])
        .assert()
        .failure();
    cli(home.path())
        .args([
            "send-message",
            "https://chatgpt.com/c/example",
            "follow-up",
            "--image",
            "/does/not/exist.png",
        ])
        .assert()
        .failure();
    cli(home.path())
        .args(["send-message", "https://example.com/c/invalid", "follow-up"])
        .assert()
        .failure();
    assert!(!home.path().join("slingring.sock").exists());
}
#[test]
fn stdio_metadata_is_browser_free_and_files_are_never_exposed() {
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command as ProcessCommand, Stdio},
    };
    let home = tempfile::tempdir().unwrap();
    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_slingring"))
        .arg("mcp")
        .env("SLINGRING_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut receive = || {
        let mut line = String::new();
        assert!(
            output.read_line(&mut line).unwrap() > 0,
            "unexpected MCP EOF"
        );
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize", "params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}})).unwrap();
    input.flush().unwrap();
    // The official SDK requires a completed initialize response before the
    // initialized notification; batch-writing all lines violates the handshake.
    let init = receive();
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
    )
    .unwrap();
    input.flush().unwrap();
    let listing = receive();
    writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"send_message","arguments":{"chatgpt_url":"https://chatgpt.com/c/example","message":"test","image_paths":["/private/file"]}}})).unwrap();
    input.flush().unwrap();
    let rejection = receive();
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
        "name":"get_job_status","arguments":{"request_id":"not-ever-submitted"}}})
    )
    .unwrap();
    input.flush().unwrap();
    let status = receive();
    drop(input);
    assert!(child.wait().unwrap().success());
    let messages = [init, listing, rejection, status];
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[0]["result"]["protocolVersion"], "2025-11-25");
    let value = &messages[1];
    assert_eq!(value["id"], 2);
    assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 4);
    assert_eq!(value["result"]["tools"][0]["name"], "new_task");
    assert_eq!(value["result"]["tools"][1]["name"], "send_message");
    assert_eq!(value["result"]["tools"][2]["name"], "get_job_status");
    assert_eq!(value["result"]["tools"][3]["name"], "pause_task");
    assert_eq!(
        value["result"]["tools"][2]["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["request_id"]
    );
    assert_eq!(
        value["result"]["tools"][3]["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["chatgpt_url"]
    );
    assert_eq!(
        value["result"]["tools"][0]["inputSchema"]["properties"]["reasoning_effort"]["enum"],
        serde_json::json!(["", "Instant", "Medium", "High"])
    );
    assert_eq!(
        value["result"]["tools"][0]["annotations"]["readOnlyHint"],
        true
    );
    assert_eq!(
        value["result"]["tools"][0]["annotations"]["openWorldHint"],
        false
    );
    assert!(!messages[1].to_string().contains("image_paths"));
    assert_eq!(messages[2]["id"], 3);
    assert!(messages[2]["error"].is_object());
    assert_eq!(messages[3]["id"], 4);
    assert_eq!(
        messages[3]["result"]["structuredContent"]["outcome"],
        "unknown"
    );
    assert!(!home.path().join("slingring.sock").exists());
}

#[test]
fn invalid_write_request_ids_are_rejected_before_service_start() {
    let home = tempfile::tempdir().unwrap();
    for id in [
        "unsafe\nline".to_string(),
        "a".repeat(129),
        "../path".into(),
    ] {
        cli(home.path())
            .args(["new-task", "fixture", "--request-id", &id])
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("invalid request ID"))
            .stderr(predicate::str::contains("Request ID:").not());
        cli(home.path())
            .args([
                "send-message",
                "https://chatgpt.com/c/fixture",
                "fixture",
                "--request-id",
                &id,
            ])
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("invalid request ID"));
    }
    assert!(!home.path().join("slingring.sock").exists());
    assert!(!home.path().join("journal.json").exists());
}

#[test]
fn cli_status_accepts_colon_and_128_character_recovery_keys() {
    let home = tempfile::tempdir().unwrap();
    let id = format!("cli:{}", "a".repeat(124));
    let output = cli(home.path())
        .args(["jobs", "--request-id", &id, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["request_id"], id);
    assert_eq!(value["outcome"], "unknown");
    assert!(!home.path().join("slingring.sock").exists());
}
