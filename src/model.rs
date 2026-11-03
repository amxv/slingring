use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf};

pub const MAX_JSON: usize = 256 * 1024;
pub const MAX_PROMPT: usize = 128 * 1024;
pub const QUEUE_CAPACITY: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTask {
    pub prompt: String,
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub image_paths: Vec<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendMessage {
    pub chatgpt_url: String,
    pub message: String,
    /// Opt-in: omitted reasoning effort leaves the existing chat untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_paths: Vec<PathBuf>,
}

/// Both operations share the same queue, browser owner and submission guard.
/// A follow-up may request an effort explicitly; otherwise it leaves the
/// existing conversation's setting alone. Local CLI callers may attach images;
/// remote MCP callers cannot provide file paths.
#[derive(Clone)]
pub enum Task {
    New(NewTask),
    Message(SendMessage),
}

impl From<NewTask> for Task {
    fn from(task: NewTask) -> Self {
        Self::New(task)
    }
}
impl From<SendMessage> for Task {
    fn from(task: SendMessage) -> Self {
        Self::Message(task)
    }
}

impl SendMessage {
    pub fn resolve(mut self, _config: &crate::config::Config) -> Result<Self> {
        ensure!(
            canonical_url(&self.chatgpt_url),
            "chatgpt_url must be an exact https://chatgpt.com/c/<id> conversation URL without a query or fragment"
        );
        ensure!(!self.message.trim().is_empty(), "message cannot be empty");
        ensure!(self.message.len() <= MAX_PROMPT, "message exceeds 128 KiB");
        self.reasoning_effort = match self.reasoning_effort.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => Some(effort(value)?),
            _ => None,
        };
        validate_images(&mut self.image_paths)?;
        Ok(self)
    }
}

impl Task {
    pub fn resolve(self, config: &crate::config::Config) -> Result<Self> {
        match self {
            Self::New(task) => task.resolve(config).map(Self::New),
            Self::Message(task) => task.resolve(config).map(Self::Message),
        }
    }
    pub fn prompt(&self) -> &str {
        match self {
            Self::New(task) => &task.prompt,
            Self::Message(task) => &task.message,
        }
    }
    pub fn conversation_url(&self) -> Option<&str> {
        match self {
            Self::New(_) => None,
            Self::Message(task) => Some(&task.chatgpt_url),
        }
    }
    pub fn reasoning_effort(&self) -> Option<&str> {
        match self {
            Self::New(task) => task.reasoning_effort.as_deref(),
            Self::Message(task) => task.reasoning_effort.as_deref(),
        }
    }
    pub fn image_paths(&self) -> &[PathBuf] {
        match self {
            Self::New(task) => &task.image_paths,
            Self::Message(task) => &task.image_paths,
        }
    }
    pub fn fingerprint(&self, profile: &str) -> Result<String> {
        match self {
            // Preserve existing new_task receipts across an upgrade.
            Self::New(task) => task.fingerprint(profile),
            Self::Message(task) => {
                let mut hash = Sha256::new();
                hash.update(b"send_message\0");
                hash.update(serde_json::to_vec(task)?);
                hash.update(profile.as_bytes());
                hash_image_contents(&task.image_paths, &mut hash)?;
                Ok(format!("{:x}", hash.finalize()))
            }
        }
    }
    pub fn validate_result(&self, result: &TaskResult) -> Result<()> {
        result.validate()?;
        if let Self::New(_) = self {
            ensure!(
                self.reasoning_effort().is_none()
                    || self.reasoning_effort() == result.reasoning_effort.as_deref(),
                "requested effort was not verified; inspect chat before retrying"
            );
        }
        ensure!(
            self.conversation_url().is_none_or(|url| url == result.url),
            "follow-up conversation changed; inspect chat before retrying"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskResult {
    #[serde(default)]
    pub job_id: String,
    pub url: String,
    pub reasoning_effort: Option<String>,
    pub evidence: String,
}

/// Only curated browser errors cross the IPC boundary, never page HTML or text.
#[derive(Debug)]
pub struct StartError {
    pub code: String,
    pub may_have_submitted: bool,
    pub url: Option<String>,
}
impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            self.code,
            if self.may_have_submitted {
                "; submission may have started; inspect the task before retrying"
            } else {
                "; nothing was submitted"
            }
        )
    }
}
impl std::error::Error for StartError {}

pub fn effort(value: &str) -> Result<String> {
    Ok(match value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "")
        .as_str()
    {
        "instant" => "Instant",
        "med" | "medium" => "Medium",
        "high" => "High",
        "xhigh" | "extrahigh" => "Extra High",
        "pro" => "Pro",
        _ => bail!("unsupported reasoning effort; use Instant, Medium, High, Extra High or Pro"),
    }
    .into())
}

impl NewTask {
    pub fn resolve(mut self, config: &crate::config::Config) -> Result<Self> {
        ensure!(!self.prompt.trim().is_empty(), "prompt cannot be empty");
        ensure!(self.prompt.len() <= MAX_PROMPT, "prompt exceeds 128 KiB");
        self.reasoning_effort = match self.reasoning_effort.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => Some(effort(value)?),
            _ => config.default_effort.clone(),
        };
        validate_images(&mut self.image_paths)?;
        Ok(self)
    }
    pub fn fingerprint(&self, profile: &str) -> Result<String> {
        let mut hash = Sha256::new();
        hash.update(serde_json::to_vec(self)?);
        hash.update(profile.as_bytes());
        hash_image_contents(&self.image_paths, &mut hash)?;
        Ok(format!("{:x}", hash.finalize()))
    }
}

fn hash_image_contents(paths: &[PathBuf], hash: &mut Sha256) -> Result<()> {
    for path in paths {
        let mut f = std::fs::File::open(path)?;
        let mut bytes = [0_u8; 65536];
        loop {
            let n = f.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            hash.update(&bytes[..n]);
        }
    }
    Ok(())
}

fn validate_images(paths: &mut Vec<PathBuf>) -> Result<()> {
    ensure!(paths.len() <= 16, "at most 16 images are allowed");
    let mut total = 0;
    for path in paths {
        *path = path
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("image file is unavailable"))?;
        let meta = path.metadata()?;
        ensure!(
            meta.is_file() && meta.len() > 0 && meta.len() <= 20 * 1024 * 1024,
            "image must be a nonempty regular file up to 20 MiB"
        );
        total += meta.len();
        ensure!(total <= 100 * 1024 * 1024, "combined images exceed 100 MiB");
        ensure!(
            matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("png" | "jpg" | "jpeg" | "webp" | "gif")
            ),
            "unsupported image extension"
        );
        let mut header = [0_u8; 12];
        let len = std::fs::File::open(path)?.read(&mut header)?;
        let image = header.starts_with(b"\x89PNG\r\n\x1a\n")
            || header.starts_with(b"\xff\xd8\xff")
            || header.starts_with(b"GIF87a")
            || header.starts_with(b"GIF89a")
            || (len >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WEBP");
        ensure!(image, "file does not have a supported image signature");
    }
    Ok(())
}

pub fn canonical_url(url: &str) -> bool {
    let id = url.strip_prefix("https://chatgpt.com/c/").unwrap_or("");
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
impl TaskResult {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            canonical_url(&self.url),
            "browser returned a noncanonical conversation URL"
        );
        ensure!(
            matches!(self.evidence.as_str(), "thinking" | "working" | "streaming"),
            "browser returned invalid start evidence"
        );
        if let Some(e) = &self.reasoning_effort {
            ensure!(effort(e)? == *e, "invalid verified effort");
        }
        Ok(())
    }
}
