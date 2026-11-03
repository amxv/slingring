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
    assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(value["result"]["tools"][0]["name"], "new_task");
    assert_eq!(value["result"]["tools"][1]["name"], "send_message");
    assert_eq!(value["result"]["tools"][2]["name"], "get_job_status");
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
