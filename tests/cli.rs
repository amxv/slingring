use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

fn cli(root: &tempfile::TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("slingring"));
    cmd.env("SLINGRING_HOME", root.path());
    cmd
}
#[test]
fn shows_the_product_not_the_template() {
    let root = tempfile::tempdir().unwrap();
    cli(&root)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("new-task"))
        .stdout(predicate::str::contains("hello").not());
    cli(&root)
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("slingring "));
}
#[test]
fn read_only_status_does_not_start_a_service_or_browser() {
    let root = tempfile::tempdir().unwrap();
    let output = cli(&root).args(["status", "--json"]).output().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["running"], false);
    assert!(!root.path().join("slingring.sock").exists());
    assert!(!root.path().join("config.json").exists());
}
#[test]
fn settings_are_canonical_and_secrets_are_not_printed() {
    let root = tempfile::tempdir().unwrap();
    cli(&root)
        .args(["config", "set", "default-effort", "xhigh"])
        .assert()
        .success();
    cli(&root)
        .args(["config", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Extra High"));
    cli(&root)
        .args(["config", "set", "default-effort", "site"])
        .assert()
        .success();
    let mut config = slingring::config::Config::load(root.path()).unwrap();
    assert!(config.default_effort.is_none());
    config.device_token = Some("a".repeat(48));
    config.mcp_url = Some(format!("https://relay.example/mcp/{}", "b".repeat(48)));
    config.save(root.path()).unwrap();
    cli(&root)
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a".repeat(48)).not())
        .stdout(predicate::str::contains("b".repeat(48)).not());
}
#[test]
fn invalid_local_inputs_fail_before_a_browser_is_launched() {
    let root = tempfile::tempdir().unwrap();
    cli(&root)
        .args(["new-task", "hello", "--reasoning-effort", "unknown"])
        .assert()
        .failure();
    cli(&root)
        .args(["new-task", "hello", "--image", "/no/such/image.png"])
        .assert()
        .failure();
    cli(&root)
        .args(["new-task", "hello", "--stdin"])
        .assert()
        .failure();
    assert!(!root.path().join("slingring.sock").exists());
}
#[test]
fn stdio_metadata_is_usable_without_any_daemon() {
    let root = tempfile::tempdir().unwrap();
    let input=[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})]
        .iter().map(Value::to_string).collect::<Vec<_>>().join("\n")+"\n";
    let output = cli(&root).arg("mcp").write_stdin(input).output().unwrap();
    assert!(output.status.success());
    let lines = String::from_utf8(output.stdout).unwrap();
    let replies = lines
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[1]["result"]["tools"][0]["name"], "new_task");
    assert!(!root.path().join("slingring.sock").exists());
}
