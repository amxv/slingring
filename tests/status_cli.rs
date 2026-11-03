use assert_cmd::Command;
use serde_json::{Value, json};
use slingring::config::atomic;

fn output(home: &std::path::Path, arguments: &[&str]) -> Value {
    let bytes = Command::new(env!("CARGO_BIN_EXE_slingring"))
        .env("SLINGRING_HOME", home)
        .args(arguments)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&bytes).unwrap()
}

fn journal(home: &std::path::Path) {
    // Older/missing ownership sidecars cannot prove that a queued record was
    // never submitted. Model a known stopped daemon with both free lock files.
    for lock in ["daemon.lock", "queue.lock"] {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.join(lock))
            .unwrap();
    }
    let entries = json!([
        {"id":"confirmed","status":"started","created_at":1,"finished_at":2,
         "result":{"job_id":"confirmed","url":"https://chatgpt.com/c/confirmed","reasoning_effort":"Pro","evidence":"thinking"},
         "error":null,"url":"https://chatgpt.com/c/confirmed"},
        {"id":"starting","status":"starting","created_at":3,"finished_at":null,
         "phase":"submission_authorized","result":null,"error":null,"url":null},
        {"id":"queued","status":"queued","created_at":4,"finished_at":null,
         "phase":"queued","result":null,"error":null,"url":null}
    ]);
    atomic(
        &home.join("journal.json"),
        &serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
}

#[test]
fn missing_ownership_evidence_never_claims_a_queued_task_was_unsent() {
    let home = tempfile::tempdir().unwrap();
    journal(home.path());
    std::fs::remove_file(home.path().join("queue.lock")).unwrap();
    let status = output(home.path(), &["jobs", "--request-id", "queued", "--json"]);
    assert_eq!(status["outcome"], "uncertain");
    assert_eq!(status["job"]["finished_at"], Value::Null);
}

#[test]
fn offline_status_recovers_crash_history_without_claiming_work_is_in_progress() {
    let home = tempfile::tempdir().unwrap();
    journal(home.path());
    let status = output(home.path(), &["status", "--json"]);
    assert_eq!(status["running"], false);
    assert_eq!(status["status_available"], true);
    assert_eq!(status["active_tasks"], 0);
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["latest_started_job"]["id"], "confirmed");
    assert_eq!(status["latest_finished_job"]["id"], "queued");
    for (id, expected) in [
        ("queued", "not_submitted"),
        ("starting", "uncertain"),
        ("confirmed", "submitted"),
    ] {
        assert_eq!(
            output(home.path(), &["jobs", "--request-id", id, "--json"])["outcome"],
            expected
        );
    }
    assert!(!home.path().join("slingring.sock").exists());
}

#[test]
fn unreachable_service_is_not_reported_as_stopped_or_idle() {
    let home = tempfile::tempdir().unwrap();
    journal(home.path());
    let owner = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.path().join("queue.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&owner).unwrap();
    let status = output(home.path(), &["status", "--json"]);
    assert_eq!(status["running"], true);
    assert_eq!(status["status_available"], false);
    assert_eq!(status["status_source"], "journal");
    assert_eq!(status["browser_busy"], Value::Null);
    assert_eq!(status["active_tasks"], 2);
    assert_eq!(status["starting_tasks"], 1);
    assert_eq!(status["queued_tasks"], 1);
    assert_eq!(status["active_job"]["id"], "starting");
    assert!(!home.path().join("slingring.sock").exists());
}
