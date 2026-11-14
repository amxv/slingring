//! Experimental pre-exec launch authorization. Released helper admission is gated.
//! No PID signaling: only the helper's retained child capability may terminate
//! its child. A root-exit report is deliberately NOT a process-tree guarantee.
use crate::{config, diagnostics};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

pub const VERSION: &str = "slingring-launch-lease-v1";
const RECEIPT: &str = "launch-receipt.json";
const MAX_FIELD: usize = 4096;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub uid: u32,
    pub start_seconds: u64,
    pub start_micros: u64,
    pub executable: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Persisted before approval. Launch may or may not have happened.
    Authorized,
    /// The guardian reports only its directly owned browser root was reaped.
    /// Uncaptured descendants and Launch Services identity remain unverified.
    RootReapedTreeUnverified,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: String,
    pub generation: String,
    pub session: String,
    pub owner: ProcessIdentity,
    pub guardian: ProcessIdentity,
    pub browser_executable: PathBuf,
    pub profile: PathBuf,
    pub state: State,
}
/// Expectations come from the approved helper installation, never the peer.
#[derive(Clone)]
pub struct GuardConfig {
    pub guard_executable: PathBuf,
    pub browser_executable: PathBuf,
}
struct SocketDirectory(PathBuf);
impl Drop for SocketDirectory {
    fn drop(&mut self) {
        let socket = self.0.join("s");
        if fs::symlink_metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = fs::remove_file(socket);
        }
        let _ = fs::remove_dir(self.0.join("h")); // Only our empty helper directory.
        let _ = fs::remove_dir(&self.0); // Never recursively delete peer paths.
    }
}
/// Hold this at browser-session scope, not only in the launch task future.
/// Dropping it before authorization cannot launch Chrome and leaves no fence.
pub struct PendingLaunch {
    listener: UnixListener,
    socket_directory: SocketDirectory,
    lock: File,
    root: PathBuf,
    config: GuardConfig,
    owner: ProcessIdentity,
    generation: String,
    session: String,
}
/// Dropping this closes the authenticated stream and asks the guardian to reap
/// its own child. The durable receipt remains uncertain without a report.
pub struct LaunchLease {
    stream: UnixStream,
    _socket_directory: SocketDirectory,
    _lock: File,
    root: PathBuf,
    receipt: Receipt,
    uncertain: bool,
}
fn absolute_clean(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}
#[cfg(target_os = "macos")]
fn inspect(pid: u32) -> Result<ProcessIdentity> {
    use libproc::{
        bsd_info::BSDInfo,
        proc_pid::{pidinfo, pidpath},
    };
    ensure!(pid > 1 && pid <= i32::MAX as u32, "invalid peer process");
    let a = pidinfo::<BSDInfo>(pid as i32, 0)
        .map_err(|_| anyhow::anyhow!("peer identity unavailable"))?;
    let executable =
        pidpath(pid as i32).map_err(|_| anyhow::anyhow!("peer executable unavailable"))?;
    let b =
        pidinfo::<BSDInfo>(pid as i32, 0).map_err(|_| anyhow::anyhow!("peer identity changed"))?;
    ensure!(
        (a.pbi_uid, a.pbi_start_tvsec, a.pbi_start_tvusec)
            == (b.pbi_uid, b.pbi_start_tvsec, b.pbi_start_tvusec)
            && a.pbi_status != 5
            && b.pbi_status != 5,
        "peer identity changed"
    );
    Ok(ProcessIdentity {
        pid,
        uid: a.pbi_uid,
        start_seconds: a.pbi_start_tvsec,
        start_micros: a.pbi_start_tvusec,
        executable: PathBuf::from(executable),
    })
}
#[cfg(not(target_os = "macos"))]
fn inspect(_pid: u32) -> Result<ProcessIdentity> {
    anyhow::bail!("launch coordinator requires macOS")
}
fn lock(root: &Path) -> Result<File> {
    let path = root.join("launch-coordinator.lock");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "invalid launch lock"
        );
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("launch coordinator already owned")?;
    Ok(file)
}
fn event(root: &Path, stage: &str, detail: &str) {
    diagnostics::event(root, "browser", stage, "cleanup", detail, 0);
}
pub fn read_receipt(root: &Path) -> Result<Option<Receipt>> {
    let path = root.join(RECEIPT);
    if !path.try_exists()? {
        return Ok(None);
    }
    let receipt: Receipt = serde_json::from_slice(&config::read_private(&path, 32 * 1024)?)?;
    ensure!(
        receipt.version == VERSION && uuid::Uuid::parse_str(&receipt.generation).is_ok(),
        "unsupported launch receipt"
    );
    Ok(Some(receipt))
}
async fn field(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let length = stream.read_u32().await? as usize;
    ensure!(
        (1..=MAX_FIELD).contains(&length),
        "invalid launch field size"
    );
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn text_field(stream: &mut UnixStream) -> Result<String> {
    String::from_utf8(field(stream).await?).context("invalid launch field")
}
impl PendingLaunch {
    pub fn prepare(root: &Path, config: GuardConfig) -> Result<Self> {
        ensure!(
            absolute_clean(root) && absolute_clean(&config.browser_executable),
            "launch paths must be absolute"
        );
        config::private_dir(root)?;
        let owner = inspect(std::process::id())?;
        ensure!(
            fs::metadata(root)?.uid() == owner.uid,
            "launch state owner mismatch"
        );
        let lock = lock(root)?;
        // Even root-reaped is not proof every browser descendant exited. This
        // experiment refuses automatic replacement until the tree is proven.
        ensure!(
            read_receipt(root)?.is_none(),
            "previous launch cleanup remains unverified"
        );
        let guard = config.guard_executable.canonicalize()?;
        ensure!(
            fs::metadata(&guard)?.is_file(),
            "guard executable unavailable"
        );
        let config = GuardConfig {
            guard_executable: guard,
            ..config
        };
        let generation = uuid::Uuid::new_v4().to_string();
        let session = format!("s-{}", &generation.replace('-', "")[..16]);
        // Root can be longer than macOS sun_path. Use a newly created private,
        // unpredictable short directory instead of unlinking a guessed socket.
        let directory = PathBuf::from("/tmp").join(format!("slg-{}", generation.replace('-', "")));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket_directory = SocketDirectory(directory);
        let listener = UnixListener::bind(socket_directory.0.join("s"))?;
        fs::set_permissions(
            socket_directory.0.join("s"),
            fs::Permissions::from_mode(0o600),
        )?;
        Ok(Self {
            listener,
            socket_directory,
            lock,
            root: root.into(),
            config,
            owner,
            generation,
            session,
        })
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn socket_path(&self) -> PathBuf {
        self.socket_directory.0.join("s")
    }
    /// Supply only to the dedicated helper generation's initial spawn. All
    /// open/get/close calls must use session(), never reuse an older daemon.
    pub fn environment(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "SLINGRING_PRIVATE_LAUNCH_GUARD",
                self.config.guard_executable.to_string_lossy().into_owned(),
            ),
            (
                "SLINGRING_PRIVATE_LAUNCH_SOCKET",
                self.socket_path().to_string_lossy().into_owned(),
            ),
            (
                "SLINGRING_PRIVATE_LAUNCH_GENERATION",
                self.generation.clone(),
            ),
            ("SLINGRING_PRIVATE_LAUNCH_SESSION", self.session.clone()),
        ]
    }
    pub async fn authorize(self) -> Result<LaunchLease> {
        self.authorize_before(tokio::time::Instant::now() + Duration::from_secs(65))
            .await
    }
    /// The caller's launch deadline includes profile preparation. Once a peer
    /// connects, a separate short handshake bound prevents a stalled peer.
    pub async fn authorize_before(self, deadline: tokio::time::Instant) -> Result<LaunchLease> {
        match tokio::time::timeout_at(deadline, self.handshake()).await {
            Ok(result) => result,
            Err(_) => anyhow::bail!("launch authorization timed out"),
        }
    }
    async fn handshake(self) -> Result<LaunchLease> {
        let (stream, _) = self.listener.accept().await?;
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, self.validate_peer(stream)).await {
            Ok(result) => result,
            Err(_) => anyhow::bail!("launch handshake timed out"),
        }
    }
    async fn validate_peer(self, mut stream: UnixStream) -> Result<LaunchLease> {
        let credentials = stream.peer_cred()?;
        let pid = credentials
            .pid()
            .filter(|pid| *pid > 1)
            .context("peer PID unavailable")? as u32;
        ensure!(
            credentials.uid() == self.owner.uid,
            "launch peer user mismatch"
        );
        let guardian = inspect(pid)?;
        ensure!(
            guardian.uid == self.owner.uid && guardian.executable == self.config.guard_executable,
            "launch peer executable mismatch"
        );
        ensure!(
            text_field(&mut stream).await? == VERSION,
            "launch version mismatch"
        );
        ensure!(
            text_field(&mut stream).await? == self.session,
            "launch session mismatch"
        );
        ensure!(
            text_field(&mut stream).await? == self.generation,
            "launch generation mismatch"
        );
        let claimed_pid: [u8; 4] = field(&mut stream)
            .await?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid launch PID field"))?;
        ensure!(
            u32::from_be_bytes(claimed_pid) == pid,
            "launch peer PID mismatch"
        );
        let browser = PathBuf::from(text_field(&mut stream).await?);
        ensure!(
            browser == self.config.browser_executable,
            "launch browser mismatch"
        );
        let profile = PathBuf::from(text_field(&mut stream).await?);
        ensure!(absolute_clean(&profile), "invalid launch profile");
        let meta = fs::symlink_metadata(&profile)?;
        ensure!(
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.uid() == self.owner.uid
                && meta.permissions().mode() & 0o077 == 0,
            "launch profile is not private"
        );
        let profile = profile.canonicalize()?;
        ensure!(
            profile.starts_with(std::env::temp_dir().canonicalize()?)
                && profile
                    .file_name()
                    .and_then(|v| v.to_str())
                    .is_some_and(|s| s.starts_with("agent-browser-profile-")),
            "launch profile is not isolated"
        );
        ensure!(
            inspect(pid)? == guardian,
            "launch peer changed before approval"
        );
        let receipt = Receipt {
            version: VERSION.into(),
            generation: self.generation,
            session: self.session,
            owner: self.owner,
            guardian,
            browser_executable: browser,
            profile,
            state: State::Authorized,
        };
        ensure!(
            read_receipt(&self.root)?.is_none(),
            "launch receipt already exists"
        );
        config::atomic(&self.root.join(RECEIPT), &serde_json::to_vec(&receipt)?)?;
        // This durable barrier precedes the only byte permitting Chrome spawn.
        stream.write_all(&[1]).await?;
        stream.flush().await?;
        event(&self.root, "launch_authorized", "durable_guard_receipt");
        Ok(LaunchLease {
            stream,
            _socket_directory: self.socket_directory,
            _lock: self.lock,
            root: self.root,
            receipt,
            uncertain: false,
        })
    }
}
impl LaunchLease {
    /// A guardian report or EOF ends reuse. This never signals or infers tree exit.
    pub fn check_live(&mut self) -> Result<()> {
        ensure!(!self.uncertain, "guardian lease evidence already invalid");
        ensure!(
            self.receipt.state == State::Authorized,
            "owned browser root already ended; tree remains unverified"
        );
        let mut byte = [0];
        match self.stream.try_read(&mut byte) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Ok(1) if byte == [2] => {
                if let Err(error) = self.persist_root_reaped() {
                    self.uncertain = true;
                    return Err(error);
                }
                anyhow::bail!("owned browser root ended; tree remains unverified")
            }
            _ => {
                self.uncertain = true;
                anyhow::bail!("guardian lease ended or reported invalid evidence")
            }
        }
    }
    fn persist_root_reaped(&mut self) -> Result<()> {
        let current = read_receipt(&self.root)?.context("launch receipt missing")?;
        ensure!(
            current.generation == self.receipt.generation
                && current.guardian == self.receipt.guardian,
            "launch ownership generation changed"
        );
        self.receipt.state = State::RootReapedTreeUnverified;
        config::atomic(
            &self.root.join(RECEIPT),
            &serde_json::to_vec(&self.receipt)?,
        )
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    /// Report scope is only the directly owned browser root. Retain the receipt
    /// and refuse automatic replacement: this is not a tree-exit assertion.
    pub async fn close(mut self) -> Result<Receipt> {
        // Drop closes the lease, but later bytes cannot rehabilitate invalid
        // evidence or upgrade the durable outcome.
        ensure!(!self.uncertain, "guardian cleanup evidence already invalid");
        if self.receipt.state == State::RootReapedTreeUnverified {
            self.persist_root_reaped()?;
            return Ok(self.receipt.clone());
        }
        event(&self.root, "browser_close_requested", "guardian_lease_eof");
        let result = tokio::time::timeout(CLEANUP_TIMEOUT, async {
            self.stream.shutdown().await?;
            ensure!(
                self.stream.read_u8().await? == 2,
                "unverified guardian cleanup report"
            );
            self.persist_root_reaped()?;
            Ok::<_, anyhow::Error>(self.receipt.clone())
        })
        .await;
        match result {
            Ok(Ok(receipt)) => {
                event(&self.root, "browser_root_reaped", "tree_unverified");
                Ok(receipt)
            }
            _ => {
                event(
                    &self.root,
                    "browser_cleanup_unverified",
                    "guard_report_missing",
                );
                anyhow::bail!("guardian cleanup unverified")
            }
        }
    }
}
