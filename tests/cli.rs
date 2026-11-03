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
    assert!(!home.path().join("slingring.sock").exists());
}
#[test]
fn stdio_metadata_is_browser_free_and_files_are_never_exposed() {
    let home = tempfile::tempdir().unwrap();
    let result = cli(home.path())
        .arg("mcp")
        .write_stdin("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"]["tools"][0]["name"], "new_task");
    assert_eq!(
        value["result"]["tools"][0]["annotations"]["readOnlyHint"],
        true
    );
    assert_eq!(
        value["result"]["tools"][0]["annotations"]["openWorldHint"],
        false
    );
    assert!(!String::from_utf8(result).unwrap().contains("image_paths"));
    assert!(!home.path().join("slingring.sock").exists());
}
