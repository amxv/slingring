use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub browser_binary: String,
    pub profile: String,
    pub default_effort: Option<String>,
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
            profile: "Default".into(),
            default_effort: Some("Extra High".into()),
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
    #[cfg(windows)]
    return Ok(
        PathBuf::from(std::env::var_os("APPDATA").context("APPDATA missing")?).join("slingring"),
    );
    #[cfg(unix)]
    Ok(PathBuf::from(std::env::var_os("HOME").context("HOME missing")?).join(".config/slingring"))
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "state directory cannot be a symlink"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    secure_windows_acl(path)?;
    Ok(())
}
#[cfg(windows)]
fn secure_windows_acl(path: &Path) -> Result<()> {
    use std::process::Command;
    // A per-user ACL is required: local loopback IPC endpoint tokens live here.
    let who = Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()?;
    ensure!(
        who.status.success(),
        "cannot determine user SID for private state"
    );
    let row = String::from_utf8(who.stdout)?;
    let sid = row
        .trim()
        .rsplit(',')
        .next()
        .unwrap_or("")
        .trim_matches('"');
    ensure!(
        sid.starts_with("S-1-5-") && sid.len() < 128,
        "invalid local user SID"
    );
    let owner = format!("*{sid}:(OI)(CI)F");
    let system = "*S-1-5-18:(OI)(CI)F";
    let grant = Command::new("icacls.exe")
        .arg(path)
        .args(["/grant:r", &owner, system])
        .output()?;
    ensure!(
        grant.status.success(),
        "cannot restrict Slingring state ACL"
    );
    let inherit = Command::new("icacls.exe")
        .arg(path)
        .arg("/inheritance:r")
        .output()?;
    ensure!(
        inherit.status.success(),
        "cannot remove inherited state permissions"
    );
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
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temp, path)?;
        #[cfg(unix)]
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
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
        let c: Self = if path.exists() {
            serde_json::from_slice(&read_private(&path, crate::model::MAX_JSON as u64)?)?
        } else {
            Self::default()
        };
        c.validate()?;
        Ok(c)
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
        serde_json::json!({"browser_binary": self.browser_binary, "profile": self.profile,
            "default_effort": self.default_effort, "start_timeout_secs": self.start_timeout_secs,
            "remote_enabled": self.remote_enabled, "remote_configured": self.relay_url.is_some() && self.device_token.is_some(),
            "mcp_url_configured": self.mcp_url.is_some(), "heartbeat_secs": self.heartbeat_secs})
    }
}
