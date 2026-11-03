use assert_cmd::Command;
use serde_json::{Value, json};
use slingring::config::{Config, atomic};

fn cli(home: &std::path::Path, arguments: &[&str]) -> assert_cmd::assert::Assert {
    Command::new(env!("CARGO_BIN_EXE_slingring"))
        .env("SLINGRING_HOME", home)
        .args(arguments)
        .assert()
}

#[test]
fn existing_config_stays_headless_and_mode_persists_without_losing_settings() {
    let home = tempfile::tempdir().unwrap();
    let old_config = json!({
        "profile": "Profile 6", "default_effort": "Pro", "start_timeout_secs": 240,
        "browser_binary": "/test/pinned-helper", "remote_enabled": false
    });
    atomic(
        &home.path().join("config.json"),
        &serde_json::to_vec(&old_config).unwrap(),
    )
    .unwrap();
    assert!(!Config::load(home.path()).unwrap().browser_headed);

    cli(home.path(), &["config", "set", "browser-mode", "headed"]).success();
    let config = Config::load(home.path()).unwrap();
    assert!(config.browser_headed);
    assert_eq!(config.profile, "Profile 6");
    assert_eq!(config.default_effort.as_deref(), Some("Pro"));
    assert_eq!(config.start_timeout_secs, 240);
    assert_eq!(config.browser_binary, "/test/pinned-helper");
    let output = cli(home.path(), &["config", "show", "--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let shown: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(shown["browser_headed"], true);

    cli(home.path(), &["config", "set", "browser-mode", "headless"]).success();
    assert!(!Config::load(home.path()).unwrap().browser_headed);
    cli(home.path(), &["config", "set", "browser_headed", "true"]).success();
    assert!(Config::load(home.path()).unwrap().browser_headed);
    assert!(!home.path().join("slingring.sock").exists());
    assert!(!home.path().join("browser.sock").exists());
}

#[test]
fn invalid_modes_do_not_change_saved_configuration_or_launch_service() {
    let home = tempfile::tempdir().unwrap();
    Config::default().save(home.path()).unwrap();
    let before = std::fs::read(home.path().join("config.json")).unwrap();
    for (key, value) in [
        ("browser-mode", "visible"),
        ("browser-mode", ""),
        ("browser-headed", "yes"),
    ] {
        cli(home.path(), &["config", "set", key, value]).failure();
        assert_eq!(
            before,
            std::fs::read(home.path().join("config.json")).unwrap()
        );
    }
    assert!(!home.path().join("slingring.sock").exists());
}
