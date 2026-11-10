use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// User-declared access, not billing or account verification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Subscription {
    #[default]
    Plus,
    Pro,
}
impl Subscription {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "plus" => Ok(Self::Plus),
            "pro" => Ok(Self::Pro),
            _ => anyhow::bail!("subscription must be plus or pro"),
        }
    }
    pub fn allows(self, effort: &str) -> bool {
        self == Self::Pro || matches!(effort, "Instant" | "Medium" | "High")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub browser_binary: String,
    /// Explicitly show the isolated automation browser for debugging.
    pub browser_headed: bool,
    pub profile: String,
    pub default_effort: Option<String>,
    /// Sidecar keeps config.json readable by installed older releases.
    #[serde(skip)]
    pub subscription: Subscription,
    pub start_timeout_secs: u64,
    pub relay_url: Option<String>,
    pub device_token: Option<String>,
    pub mcp_url: Option<String>,
    pub remote_enabled: bool,
    pub heartbeat_secs: u64,
    /// Per-job browser lease, not a user setting. Queue waiting may shorten the
    /// request deadline but must not change fork flags between queued jobs.
    #[serde(skip)]
    pub browser_idle_secs: Option<u64>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            browser_binary: "agent-browser-browse".into(),
            browser_headed: false,
            profile: "Default".into(),
            default_effort: Some("High".into()),
            subscription: Subscription::Plus,
            start_timeout_secs: 180,
            relay_url: None,
            device_token: None,
            mcp_url: None,
            remote_enabled: false,
            heartbeat_secs: 60,
            browser_idle_secs: None,
        }
    }
}
pub fn root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SLINGRING_HOME") {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(std::env::var_os("HOME").context("HOME missing")?).join(".config/slingring"))
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "state directory cannot be a symlink"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("missing parent")?;
    private_dir(parent)?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "refusing nonregular state file"
        );
    }
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
/// Read bounded private state without following symlinks.
pub fn read_private(path: &Path, max: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "refusing nonregular state file"
    );
    ensure!(meta.len() <= max, "state file exceeds size limit");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(fs::read(path)?)
}
pub fn secure_url(value: &str, websocket: bool) -> Result<String> {
    let mut url = url::Url::parse(value).context("invalid HTTPS/WSS URL")?;
    ensure!(
        matches!(url.scheme(), "https" | "wss") && url.host_str().is_some(),
        "URL must use HTTPS or WSS"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "URL cannot contain userinfo or fragments"
    );
    if websocket {
        ensure!(
            url.query().is_none(),
            "relay URL cannot contain a query credential"
        );
        url.set_scheme("wss")
            .map_err(|_| anyhow::anyhow!("invalid relay URL"))?;
        url.set_path("/device");
    }
    Ok(url.into())
}
impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.json");
        let mut c: Self = if path.exists() {
            let mut value: serde_json::Value =
                serde_json::from_slice(&read_private(&path, crate::model::MAX_JSON as u64)?)?;
            // Existing files that omitted this field used the old implicit default.
            // Only a genuinely fresh installation adopts High.
            if let Some(object) = value.as_object_mut() {
                object
                    .entry("default_effort")
                    .or_insert(serde_json::json!("Extra High"));
            }
            serde_json::from_value(value)?
        } else {
            Self::default()
        };
        let subscription_path = root.join("subscription.json");
        c.subscription = if subscription_path.exists() {
            serde_json::from_slice(&read_private(&subscription_path, 1024)?)?
        } else if c
            .default_effort
            .as_deref()
            .is_some_and(|e| !Subscription::Plus.allows(e))
        {
            // Preserve advanced defaults saved by releases without a plan setting.
            Subscription::Pro
        } else {
            Subscription::Plus
        };
        c.validate()?;
        Ok(c)
    }
    pub fn check_effort(&self, effort: &str) -> Result<()> {
        ensure!(
            self.subscription.allows(effort),
            "Extra High and Pro require a Pro subscription; use Instant, Medium or High, or set subscription pro"
        );
        Ok(())
    }
    pub fn set_subscription(&mut self, root: &Path, subscription: Subscription) -> Result<()> {
        ensure!(
            self.default_effort
                .as_deref()
                .is_none_or(|e| subscription.allows(e)),
            "saved default requires Pro; first set default-effort high before selecting Plus"
        );
        atomic(
            &root.join("subscription.json"),
            &serde_json::to_vec(&subscription)?,
        )?;
        self.subscription = subscription;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(effort) = &self.default_effort {
            ensure!(
                crate::model::effort(effort)? == *effort,
                "default_effort must be canonical"
            );
        }
        ensure!(
            (1..=3600).contains(&self.start_timeout_secs),
            "start_timeout_secs must be 1..3600"
        );
        ensure!(
            (30..=300).contains(&self.heartbeat_secs),
            "heartbeat_secs must be 30..300"
        );
        ensure!(
            !self.browser_binary.trim().is_empty() && !self.profile.trim().is_empty(),
            "empty browser configuration"
        );
        if let Some(url) = &self.relay_url {
            secure_url(url, true)?;
        }
        if let Some(url) = &self.mcp_url {
            secure_url(url, false)?;
        }
        if let Some(token) = &self.device_token {
            ensure!(
                (32..=4096).contains(&token.len()) && token.bytes().all(|c| c.is_ascii_graphic()),
                "device token must be 32..4096 printable ASCII characters"
            );
        }
        ensure!(
            !self.remote_enabled || (self.relay_url.is_some() && self.device_token.is_some()),
            "configure remote URL and device token first"
        );
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        atomic(&root.join("config.json"), &serde_json::to_vec(self)?)
    }
    pub fn redacted(&self) -> serde_json::Value {
        serde_json::json!({"browser_binary": self.browser_binary, "browser_headed": self.browser_headed, "profile": self.profile,
            "default_effort": self.default_effort, "subscription": self.subscription, "start_timeout_secs": self.start_timeout_secs,
            "remote_enabled": self.remote_enabled, "remote_configured": self.relay_url.is_some() && self.device_token.is_some(),
            "mcp_url_configured": self.mcp_url.is_some(), "heartbeat_secs": self.heartbeat_secs})
    }
}
