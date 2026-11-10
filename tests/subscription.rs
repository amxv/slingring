use slingring::config::{Config, Subscription};
use slingring::model::{NewTask, SendMessage};

#[test]
fn fresh_and_legacy_defaults_are_safe() {
    let root = tempfile::tempdir().unwrap();
    let fresh = Config::load(root.path()).unwrap();
    assert_eq!(fresh.default_effort.as_deref(), Some("High"));
    assert_eq!(fresh.subscription, Subscription::Plus);
    std::fs::write(
        root.path().join("config.json"),
        br#"{"default_effort":"Extra High","profile":"Personal"}"#,
    )
    .unwrap();
    let mut legacy = Config::load(root.path()).unwrap();
    assert_eq!(legacy.subscription, Subscription::Pro);
    let before = std::fs::read(root.path().join("config.json")).unwrap();
    legacy
        .set_subscription(root.path(), Subscription::Pro)
        .unwrap();
    assert_eq!(
        before,
        std::fs::read(root.path().join("config.json")).unwrap()
    );
    assert_eq!(
        Config::load(root.path()).unwrap().default_effort.as_deref(),
        Some("Extra High")
    );
    assert!(
        legacy
            .set_subscription(root.path(), Subscription::Plus)
            .is_err()
    );
    assert_eq!(
        Config::load(root.path()).unwrap().subscription,
        Subscription::Pro
    );
    legacy.default_effort = Some("High".into());
    legacy.save(root.path()).unwrap();
    legacy
        .set_subscription(root.path(), Subscription::Plus)
        .unwrap();
    assert_eq!(
        Config::load(root.path()).unwrap().subscription,
        Subscription::Plus
    );
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("subscription")
            .is_none()
    );
}

#[test]
fn explicit_efforts_are_filtered_but_followup_omission_stays_untouched() {
    let config = Config::default();
    for effort in ["Instant", "Medium", "High", "Extra High", "Pro"] {
        let new = NewTask {
            prompt: "Task".into(),
            reasoning_effort: Some(effort.into()),
            image_paths: vec![],
        };
        assert_eq!(
            new.resolve(&config).is_ok(),
            Subscription::Plus.allows(effort)
        );
        let pro = Config {
            subscription: Subscription::Pro,
            ..Config::default()
        };
        assert!(
            NewTask {
                prompt: "Task".into(),
                reasoning_effort: Some(effort.into()),
                image_paths: vec![]
            }
            .resolve(&pro)
            .is_ok()
        );
    }
    let followup = SendMessage {
        chatgpt_url: "https://chatgpt.com/c/test".into(),
        message: "Next".into(),
        reasoning_effort: None,
        image_paths: vec![],
    };
    assert_eq!(followup.resolve(&config).unwrap().reasoning_effort, None);
}

#[test]
fn cli_documents_output_recovery_and_defaults() {
    use clap::CommandFactory;
    let mut cli = slingring::cli::Cli::command();
    for (name, expected) in [
        ("new-task", "saved default"),
        ("send-message", "preserves the chat setting"),
        ("jobs", "Unknown or evicted"),
        ("status", "always JSON"),
        ("pause-task", "uncertain"),
    ] {
        assert!(
            cli.find_subcommand_mut(name)
                .unwrap()
                .render_long_help()
                .to_string()
                .contains(expected)
        );
    }
    let config = cli.find_subcommand_mut("config").unwrap();
    assert!(
        config
            .find_subcommand_mut("set")
            .unwrap()
            .render_long_help()
            .to_string()
            .contains("subscription pro")
    );
}

#[test]
fn invalid_subscription_is_rejected() {
    assert!(Subscription::parse("enterprise").is_err());
    assert_eq!(Subscription::parse("Pro").unwrap(), Subscription::Pro);
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("subscription.json"), br#""invalid""#).unwrap();
    assert!(Config::load(root.path()).is_err());
}

#[test]
fn legacy_implicit_default_and_explicit_site_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("config.json"), b"{}").unwrap();
    let config = Config::load(root.path()).unwrap();
    assert_eq!(config.default_effort.as_deref(), Some("Extra High"));
    assert_eq!(config.subscription, Subscription::Pro);
    std::fs::write(
        root.path().join("config.json"),
        br#"{"default_effort":null}"#,
    )
    .unwrap();
    assert_eq!(Config::load(root.path()).unwrap().default_effort, None);
}

#[test]
fn cli_subscription_roundtrip_and_invalid_changes_are_safe() {
    let root = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_slingring"))
            .env("SLINGRING_HOME", root.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        run(&["config", "set", "subscription", "pro"])
            .status
            .success()
    );
    assert!(!root.path().join("config.json").exists());
    assert!(
        run(&["config", "set", "default-effort", "xhigh"])
            .status
            .success()
    );
    let before = std::fs::read(root.path().join("config.json")).unwrap();
    assert!(
        !run(&["config", "set", "subscription", "plus"])
            .status
            .success()
    );
    assert!(
        !run(&["config", "set", "subscription", "invalid"])
            .status
            .success()
    );
    assert_eq!(
        before,
        std::fs::read(root.path().join("config.json")).unwrap()
    );
    let output = run(&["config", "show"]);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["subscription"], "pro");
    assert_eq!(value["default_effort"], "Extra High");
    assert!(!root.path().join("slingring.sock").exists());
    assert!(
        run(&["config", "set", "default-effort", "high"])
            .status
            .success()
    );
    assert!(
        run(&["config", "set", "subscription", "plus"])
            .status
            .success()
    );
    assert!(
        !run(&["config", "set", "default-effort", "pro"])
            .status
            .success()
    );
}
