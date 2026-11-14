//! CLI-owned login preferences. macOS is the source of truth for each status;
//! the small preferences file exists only to prevent installers from resetting
//! an explicit opt-out on future upgrades.

use crate::{
    cli::{LoginAction, LoginToggleAction},
    config, service,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Preferences {
    menu_bar: Option<bool>,
    service: Option<bool>,
}

impl Preferences {
    fn load(root: &Path) -> Result<Self> {
        let path = root.join("login-preferences.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&config::read_private(&path, 4096)?)
            .context("invalid login preferences; refusing to overwrite explicit choices")
    }

    fn save(&self, root: &Path) -> Result<()> {
        config::atomic(
            &root.join("login-preferences.json"),
            &serde_json::to_vec(self)?,
        )
    }
}

fn menu_bar(command: &str) -> Result<Value> {
    // The companion CLI lives beside Slingring.app. Running the executable
    // *inside the bundle* lets SMAppService.mainApp register the actual app,
    // rather than creating a brittle AppleScript/System Events login item.
    let cli = std::env::current_exe()?.canonicalize()?;
    let exe = cli
        .parent()
        .context("cannot find Slingring's installation directory")?
        .join("Slingring.app/Contents/MacOS/slingring-menubar");
    ensure!(
        exe.is_file(),
        "Slingring.app is missing next to the CLI at {}",
        exe.display()
    );
    let output = Command::new(&exe)
        .arg(format!("--login-item-{command}"))
        .output()
        .with_context(|| {
            format!(
                "unable to inspect the installed Slingring.app at {}",
                exe.display()
            )
        })?;
    ensure!(
        output.status.success(),
        "menu bar login item {command} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    serde_json::from_slice(&output.stdout).context("invalid menu bar login item response")
}

fn service_status() -> Result<Value> {
    let installed = service::installed();
    let disabled = service::login_disabled()?;
    Ok(json!({
        "enabled": installed && !disabled,
        "state": if !installed { "not_installed" } else if disabled { "disabled" } else { "enabled" },
        "registered": installed,
        "loaded": service::loaded(),
    }))
}

fn menu_toggle(root: &Path, action: LoginToggleAction) -> Result<Value> {
    let command = match action {
        LoginToggleAction::On => "on",
        LoginToggleAction::Off => "off",
        LoginToggleAction::Status => "status",
    };
    let mut preferences = if matches!(action, LoginToggleAction::Status) {
        None
    } else {
        Some(Preferences::load(root)?)
    };
    let value = menu_bar(command)?;
    if let Some(ref mut preferences) = preferences {
        preferences.menu_bar = Some(matches!(action, LoginToggleAction::On));
        preferences.save(root)?;
    }
    Ok(value)
}

fn service_toggle(root: &Path, action: LoginToggleAction) -> Result<Value> {
    let mut preferences = if matches!(action, LoginToggleAction::Status) {
        None
    } else {
        Some(Preferences::load(root)?)
    };
    match action {
        LoginToggleAction::On => service::login_enable(root)?,
        LoginToggleAction::Off => service::login_disable()?,
        LoginToggleAction::Status => {}
    }
    let value = service_status()?;
    if let Some(ref mut preferences) = preferences {
        preferences.service = Some(matches!(action, LoginToggleAction::On));
        preferences.save(root)?;
    }
    Ok(value)
}

fn status_pair() -> Value {
    fn state(result: Result<Value>) -> Value {
        match result {
            Ok(value) => value,
            Err(error) => json!({"enabled": null, "state": "error", "error": error.to_string()}),
        }
    }
    json!({"menu_bar": state(menu_bar("status")), "service": state(service_status())})
}

/// Never infer a legacy service opt-out from an absent *new* preference alone:
/// before the two toggles existed, users explicitly uninstalled the LaunchAgent.
/// A missing legacy plist on upgrade is therefore preserved as off. The old
/// menu bar had no such toggle, so its fresh default is on unless macOS already
/// requires explicit user approval (evidence of a system-level denial).
fn desired(saved: Option<bool>, upgrade: bool, legacy: bool) -> Option<bool> {
    if saved.is_some() {
        None // already managed: upgrades must not change the user's choices
    } else {
        Some(!upgrade || legacy)
    }
}

fn setup(root: &Path, upgrade: bool) -> Result<Value> {
    let mut prefs = Preferences::load(root)?;
    let mut failures = Vec::new();
    if prefs.menu_bar.is_none() {
        let existing = menu_bar("status");
        match existing {
            Ok(status) => {
                let legacy = status["state"].as_str() != Some("requires_approval");
                if let Some(enabled) = desired(prefs.menu_bar, upgrade, legacy) {
                    let outcome = if enabled { menu_bar("on") } else { Ok(status) };
                    match outcome {
                        Ok(_) => {
                            prefs.menu_bar = Some(enabled);
                            prefs.save(root)?;
                        }
                        Err(error) => failures.push(format!("menu bar: {error:#}")),
                    }
                }
            }
            Err(error) => failures.push(format!("menu bar: {error:#}")),
        }
    }
    if prefs.service.is_none() {
        let legacy = if upgrade && service::installed() {
            service::login_disabled().map(|disabled| !disabled)
        } else {
            Ok(false)
        };
        if let Err(error) = &legacy {
            failures.push(format!("service: {error:#}"));
        }
        if let Ok(Some(enabled)) = legacy.map(|legacy| desired(prefs.service, upgrade, legacy)) {
            let outcome = if enabled {
                service::login_enable(root)
            } else {
                Ok(())
            };
            match outcome {
                Ok(()) => {
                    // Off during migration means preserving the missing or
                    // disabled legacy service, without booting it out.
                    prefs.service = Some(enabled);
                    prefs.save(root)?;
                }
                Err(error) => failures.push(format!("service: {error:#}")),
            }
        }
    }
    if !failures.is_empty() {
        bail!(
            "login setup incomplete (other setting may have succeeded): {}",
            failures.join("; ")
        );
    }
    Ok(
        json!({"configured":true,"preferences":{"menu_bar":prefs.menu_bar,"service":prefs.service},
        "system":status_pair()}),
    )
}

pub fn run(root: &Path, action: LoginAction) -> Result<Value> {
    match action {
        LoginAction::MenuBar { action } => menu_toggle(root, action),
        LoginAction::Service { action } => service_toggle(root, action),
        LoginAction::Status => Ok(status_pair()),
        LoginAction::Setup { upgrade } => setup(root, upgrade),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_installs_enable_both_but_upgrades_honor_legacy_absence() {
        assert_eq!(desired(None, false, false), Some(true));
        assert_eq!(desired(None, true, true), Some(true));
        assert_eq!(desired(None, true, false), Some(false));
        assert_eq!(desired(Some(false), false, true), None);
        assert_eq!(desired(Some(false), true, true), None);
        assert_eq!(desired(Some(true), true, false), None);
    }

    #[test]
    fn preferences_preserve_opt_outs_and_reject_corrupt_files() {
        let home = tempfile::tempdir().unwrap();
        let pref = Preferences {
            menu_bar: Some(false),
            service: Some(true),
        };
        pref.save(home.path()).unwrap();
        let saved = Preferences::load(home.path()).unwrap();
        assert_eq!(saved.menu_bar, Some(false));
        assert_eq!(saved.service, Some(true));
        std::fs::write(home.path().join("login-preferences.json"), b"{invalid").unwrap();
        assert!(Preferences::load(home.path()).is_err());
    }
}
