//! Profile discovery uses the browser fork's metadata-only command; no browser
//! is launched and no cookies, passwords, or browsing history are read here.
use crate::{config::Config, dependencies};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, process::Stdio, time::Duration};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    pub directory: String,
    pub name: String,
    #[serde(default)]
    pub selected: bool,
}

pub fn parse(value: Value, selected: &str) -> Result<Vec<Profile>> {
    ensure!(value["success"] == true, "Chrome profile discovery failed");
    let mut profiles: Vec<Profile> = serde_json::from_value(value["data"].clone())
        .context("unexpected Chrome profile metadata")?;
    ensure!(profiles.len() <= 1000, "too many Chrome profiles");
    for profile in &mut profiles {
        ensure!(
            !profile.directory.is_empty()
                && profile.directory.len() <= 200
                && !profile.directory.contains(['/', '\\', '\n', '\r'])
                && profile.directory != "."
                && profile.directory != "..",
            "invalid Chrome profile directory"
        );
        profile.selected = profile.directory == selected;
    }
    profiles.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.directory.cmp(&b.directory))
    });
    Ok(profiles)
}

pub async fn list(root: &Path, config: &Config) -> Result<Vec<Profile>> {
    let binary = dependencies::resolve(root, &config.browser_binary)?;
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(binary)
            .args(["profiles", "--json"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("Chrome profile discovery timed out")??;
    ensure!(
        output.status.success() && output.stdout.len() <= 1024 * 1024,
        "could not list Chrome profiles; open Chrome once and try again"
    );
    parse(serde_json::from_slice(&output.stdout)?, &config.profile)
}

pub fn select<'a>(profiles: &'a [Profile], value: &str) -> Result<&'a Profile> {
    if let Some(profile) = profiles.iter().find(|p| p.directory == value) {
        return Ok(profile);
    }
    let matches: Vec<_> = profiles
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(value))
        .collect();
    ensure!(
        matches.len() <= 1,
        "multiple profiles have this name; choose a directory from slingring profile list"
    );
    matches
        .first()
        .copied()
        .context("Chrome profile not found; run slingring profile list")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn profile_discovery_is_bounded_and_selection_is_unambiguous() {
        let rows = parse(
            json!({"success":true,"data":[
            {"directory":"Profile 6","name":"Personal"},
            {"directory":"Profile 2","name":"Work"},
            {"directory":"Default","name":"Work"}]}),
            "Profile 6",
        )
        .unwrap();
        assert!(select(&rows, "personal").unwrap().selected);
        assert_eq!(select(&rows, "Profile 2").unwrap().name, "Work");
        assert!(select(&rows, "Work").is_err());
        assert!(select(&rows, "missing").is_err());
        assert!(
            parse(
                json!({"success":true,"data":[{"directory":"../other","name":"Bad"}]}),
                ""
            )
            .is_err()
        );
    }
}
