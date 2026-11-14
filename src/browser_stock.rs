//! Isolated stock Chrome backend for Linux. No task path is wired in this candidate.
mod group;
mod guardian;
mod receipt;

use anyhow::{Context, Result, ensure};
use group::GroupLease;
use guardian::GuardedChild;
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

pub(crate) const PROFILE: &str = "Slingring";
pub(crate) const PROVIDER: &str = "stock-chrome-stable";
const MAX_FRAME: usize = 4 * 1024 * 1024;
#[cfg(feature = "stock-browser-fixture")]
const STOCK_SMOKE: &str = "--private-slingring-stock-chrome-smoke";

pub(super) fn require_private(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || path.canonicalize()? != path
        || !meta.is_dir()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "directory not canonical, private and owned",
        ));
    }
    Ok(())
}

pub(super) fn chrome_executable() -> io::Result<PathBuf> {
    let path = Path::new(crate::browser_linux::CHROME).canonicalize()?;
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
    {
        return Err(io::Error::other("system-owned Chrome Stable is required"));
    }
    Ok(path)
}

pub(crate) fn validate_registration(root: &Path, profile: &Path) -> Result<PathBuf> {
    require_private(root)?;
    require_private(profile)?;
    ensure!(
        profile == root.join("stock-chrome-profile"),
        "native host target is not the exact dedicated Slingring profile"
    );
    Ok(profile.to_path_buf())
}

fn create_private(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    require_private(path)
}

pub(crate) fn maybe_run_guardian() -> Option<Result<()>> {
    let mut args = std::env::args_os().skip(1);
    match args.next()?.to_str()? {
        guardian::MODE => Some(guardian::child_mode(&args.collect::<Vec<_>>()).map_err(Into::into)),
        #[cfg(feature = "stock-browser-fixture")]
        STOCK_SMOKE => Some((|| {
            let root = PathBuf::from(args.next().context("missing dedicated fixture state")?);
            ensure!(args.next().is_none(), "unexpected fixture argument");
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(smoke(&root))
        })()),
        _ => None,
    }
}

/// Chromium's NUL-delimited browser pipe. An abandoned/partial call poisons the
/// connection; a response from a previous turn can never authorize a new close.
pub(crate) struct Pipe {
    channel: BufReader<UnixStream>,
    next_id: u64,
    usable: bool,
}
impl Pipe {
    pub(crate) async fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value> {
        ensure!(self.usable, "private browser channel unverified");
        self.usable = false;
        self.next_id = self.next_id.checked_add(1).context("CDP ID exhausted")?;
        let id = self.next_id;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let mut bytes = serde_json::to_vec(&request)?;
        ensure!(bytes.len() < MAX_FRAME, "CDP command size exceeds limit");
        bytes.push(0);
        let result: Value = tokio::time::timeout(Duration::from_secs(8), async {
            self.channel.write_all(&bytes).await?;
            self.channel.flush().await?;
            loop {
                let mut bytes = Vec::new();
                let count = (&mut self.channel)
                    .take(MAX_FRAME as u64)
                    .read_until(0, &mut bytes)
                    .await?;
                ensure!(
                    count > 0 && bytes.pop() == Some(0),
                    "incomplete CDP response"
                );
                let reply: Value = serde_json::from_slice(&bytes)?;
                if reply.get("id").is_none() {
                    continue;
                }
                ensure!(reply["id"].as_u64() == Some(id), "CDP response ID mismatch");
                return Ok::<Value, anyhow::Error>(reply);
            }
        })
        .await??;
        ensure!(
            result.get("result").is_some() != result.get("error").is_some(),
            "invalid browser result"
        );
        self.usable = true;
        ensure!(
            result.get("error").is_none(),
            "stock Chrome rejected {method}"
        );
        result.get("result").cloned().context("missing CDP result")
    }
}

#[derive(Clone)]
pub(crate) struct Channel {
    pipe: Arc<Mutex<Pipe>>,
    release: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<bool>>>>,
}
impl Channel {
    pub(crate) async fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value> {
        self.pipe.lock().await.call(method, params, session).await
    }

    /// A proved task result returns immediately, independent of cleanup.
    /// Only the original private native socket is touched here, never CDP.
    /// A missing release demands an orderly browser generation retirement
    /// before queue reuse, without resending the submitted task.
    pub(crate) fn acknowledge(&self, mut socket: UnixStream, job: String) {
        let child = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(1), async {
                crate::native::write(&mut socket, &json!({"type":"ack"})).await?;
                let release = crate::native::read(&mut socket).await?;
                Ok::<_, anyhow::Error>(release["type"] == "released" && release["job"] == job)
            })
            .await
            .is_ok_and(|ret| ret.is_ok_and(|released| released))
        });
        // Queue serialization permits exactly one result handoff before
        // the next begin/close consumes the previous cleanup handle.
        let mut guard = self.release.lock().expect("native release lock");
        if guard.is_some() {
            child.abort();
            if let Some(prior) = guard.as_ref() {
                prior.abort(); // Unexpected overlap forces retirement.
            }
        } else {
            *guard = Some(child);
        }
    }

    async fn released_or_absent(&self) -> bool {
        let found = self.release.lock().expect("native release lock").take();
        match found {
            Some(child) => child.await.unwrap_or(false),
            None => true,
        }
    }
}

/// A bound local handoff path exists only until launch has been authenticated.
/// Chrome never binds any port; receives anonymous inherited 3/4.
struct Socket(PathBuf);
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct Browser {
    channel: Channel,
    guardian: GuardedChild,
    evidence: receipt::Generation,
    profile: PathBuf,
    headed: bool,
}
impl Browser {
    async fn open(root: &Path, headed: bool) -> Result<Self> {
        crate::browser_linux::preflight()?;
        require_private(root)?;
        ensure!(
            !receipt::fenced(&root.join("stock-chrome-generation"))?,
            "prior stock Chrome shutdown unverified; no browser launched",
        );
        let runtime = crate::browser_linux::runtime_root(root)?;
        match create_private(&runtime) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                require_private(&runtime)?
            }
            result => result?,
        }
        let attempt = runtime.join(format!("a-{}", uuid::Uuid::new_v4().simple()));
        create_private(&attempt)?;
        let profile = root.join("stock-chrome-profile");
        match create_private(&profile) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                require_private(&profile)?
            }
            result => result?,
        }
        ensure!(
            fs::symlink_metadata(profile.join("SingletonLock"))
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound),
            "dedicated Slingring browser profile already in use or unverified",
        );
        let home = root.join("stock-chrome-home");
        match create_private(&home) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => require_private(&home)?,
            result => result?,
        }
        let socket = Socket(attempt.join("cdp"));
        ensure!(
            socket.0.as_os_str().as_encoded_bytes().len() <= 103,
            "private handoff path too long"
        );
        let server = UnixListener::bind(&socket.0)?;
        let evidence = receipt::begin(&root.join("stock-chrome-generation"), &profile)?;
        let nonce = evidence.nonce();
        let current = std::env::current_exe()?;
        let paths = [
            root.to_owned(),
            socket.0.clone(),
            profile.clone(),
            home,
            PathBuf::from(if headed { "headed" } else { "headless" }),
        ];
        let guard = tokio::task::spawn_blocking(move || {
            let argv = paths.each_ref().map(PathBuf::as_path);
            let mut child = GuardedChild::spawn(&current, &argv, nonce)?;
            child.check_live()?;
            Ok::<_, io::Error>(child)
        });
        let (mut channel, _) =
            tokio::time::timeout(Duration::from_secs(8), server.accept()).await??;
        let guardian = guard.await??;
        let cred = channel.peer_cred()?;
        ensure!(
            cred.uid() == rustix::process::geteuid().as_raw()
                && cred.pid().and_then(|id| u32::try_from(id).ok())
                    == Some(guardian.guardian_pid()),
            "private browser handoff peer did not match exact guardian"
        );
        let mut hello = [0; 16];
        tokio::time::timeout(Duration::from_secs(3), channel.read_exact(&mut hello)).await??;
        ensure!(
            hello == nonce,
            "private browser handoff generation mismatch"
        );
        drop(server);
        drop(socket);
        let mut this = Self {
            channel: Channel {
                pipe: Arc::new(Mutex::new(Pipe {
                    channel: BufReader::new(channel),
                    next_id: 0,
                    usable: true,
                })),
                release: Arc::new(std::sync::Mutex::new(None)),
            },
            guardian,
            evidence,
            profile,
            headed,
        };
        this.verify_root().await?;
        Ok(this)
    }

    async fn verify_root(&mut self) -> Result<()> {
        let info = self
            .channel
            .call("SystemInfo.getProcessInfo", json!({}), None)
            .await?;
        let roots: Vec<_> = info["processInfo"]
            .as_array()
            .context("stock Chrome process information unavailable")?
            .iter()
            .filter(|entry| entry["type"] == "browser")
            .collect();
        ensure!(
            roots.len() == 1 && roots[0]["id"].as_u64() == Some(u64::from(self.guardian.root())),
            "private endpoint does not identify our exact browser"
        );
        crate::browser_linux::verify_stock(self.guardian.root(), self.headed, &self.profile)
    }

    async fn close(mut self) -> Result<()> {
        let _ = self.channel.released_or_absent().await;
        self.verify_root().await?;
        let mut guard = self.guardian;
        let guardian = tokio::task::spawn_blocking(move || guard.arm().map(|()| guard)).await??;
        self.channel.call("Browser.close", json!({}), None).await?;
        self.guardian = guardian;
        let Self {
            mut guardian,
            evidence,
            ..
        } = self;
        // Delay no other tasks after final command; the same live owner
        // authenticates direct reap / ECHILD / EOF before durable terminal.
        let terminal = tokio::task::spawn_blocking(move || guardian.finish()).await??;
        evidence.complete(terminal)?;
        Ok(())
    }
}

static ACTIVE: Mutex<Option<Browser>> = Mutex::const_new(None);
pub(crate) fn profile_prepared(root: &Path) -> bool {
    require_private(root).is_ok()
        && crate::config::read_private(&root.join("stock-chrome-configured"), 32)
            .is_ok_and(|marker| marker == b"dedicated-stock-v1\n")
}
pub(crate) async fn open_task(root: &Path, headed: bool) -> Result<(Channel, PathBuf)> {
    require_private(root)?;
    let mut active = ACTIVE.lock().await;
    if let Some(current) = active.as_mut() {
        ensure!(
            current.headed == headed && current.profile == root.join("stock-chrome-profile"),
            "existing browser ownership or visibility differs"
        );
        if current.channel.released_or_absent().await {
            current.verify_root().await?;
            return Ok((current.channel.clone(), current.profile.clone()));
        }
        // An old controller may still own a tab. Don't touch it through a
        // detached CDP call; instead retire the exact browser, prove cleanup,
        // and admit the next queued job only on a fresh owned generation.
        active
            .take()
            .context("active browser disappeared")?
            .close()
            .await?;
    }
    // No implicit change of backend, reuse of personal data, or anonymous
    // task submission without the owner's explicit initial profile setup.
    ensure!(
        profile_prepared(root),
        "run slingring profile open on this Linux desktop to prepare the dedicated profile"
    );
    let browser = Browser::open(root, headed).await?;
    let result = (browser.channel.clone(), browser.profile.clone());
    *active = Some(browser);
    Ok(result)
}
pub(crate) async fn close_task() -> Result<()> {
    let mut active = ACTIVE.lock().await;
    if let Some(browser) = active.take() {
        browser.close().await?;
    }
    Ok(())
}
pub(crate) async fn manual_profile(root: &Path) -> Result<()> {
    use std::io::IsTerminal;
    ensure!(
        io::stdin().is_terminal(),
        "profile open needs an interactive Linux terminal"
    );
    ensure!(
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        "profile open needs an interactive Linux desktop"
    );
    ensure!(
        !crate::daemon::running(root).await,
        "stop the local Slingring service before manual profile setup"
    );
    let browser = Browser::open(root, true).await?;
    eprintln!(
        "A dedicated Chrome window is open on this Linux desktop. Sign in manually there. When done, leave Chrome open and press Enter here to close it cleanly."
    );
    let confirmation = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        io::stdin().read_line(&mut line)
    })
    .await??;
    ensure!(confirmation > 0, "manual profile setup was interrupted");
    browser.close().await?;
    crate::config::atomic(
        &root.join("stock-chrome-configured"),
        b"dedicated-stock-v1\n",
    )
}

#[cfg(feature = "stock-browser-fixture")]
async fn smoke(root: &Path) -> Result<()> {
    // Exact candidate process proof only. No task, website, account, profile
    // import, service changes, helper download, or third-party interaction.
    require_private(root)?;
    ensure!(
        root.starts_with("/tmp")
            && root
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.starts_with("sl-stock-rust-"))
            && fs::read_dir(root)?.next().is_none(),
        "smoke requires a newly created empty private /tmp/sl-stock-rust-* path"
    );
    ensure!(
        open_task(root, false).await.is_err(),
        "unprovisioned profile unexpectedly usable"
    );
    crate::config::atomic(
        &root.join("stock-chrome-configured"),
        b"dedicated-stock-v1\n",
    )?;
    ensure!(profile_prepared(root), "fixture setup marker unreadable");
    for _ in 0..2 {
        let (pipe, profile) = open_task(root, false).await?;
        let extension = crate::native::assets(root)?;
        crate::native::register(root, &profile)?;
        let loaded = pipe
            .call("Extensions.loadUnpacked", json!({"path":extension}), None)
            .await?;
        ensure!(
            loaded["id"] == crate::native::EXTENSION_ID,
            "stock Chrome cannot load the exact embedded extension"
        );

        let state_socket = Socket(root.join("browser.sock"));
        let native = UnixListener::bind(&state_socket.0)?;
        let nonce = uuid::Uuid::new_v4().to_string();
        let job = "offline-native-hello";
        let url = format!(
            "chrome-extension://{}/bridge.html#token={nonce}&job={job}",
            crate::native::EXTENSION_ID
        );
        pipe.call("Target.createTarget", json!({"url":url}), None)
            .await?;
        let (mut conn, _) = tokio::time::timeout(Duration::from_secs(6), native.accept()).await??;
        ensure!(
            conn.peer_cred()?.uid() == rustix::process::geteuid().as_raw(),
            "unexpected native peer"
        );
        let hello =
            tokio::time::timeout(Duration::from_secs(2), crate::native::read(&mut conn)).await??;
        ensure!(
            hello["type"] == "hello" && hello["token"] == nonce && hello["job"] == job,
            "native identity mismatch"
        );
        // Deliberately send no task, no files, no account credentials and no
        // authorization. Chrome never navigates a page outside our fixture.
        drop(conn);
        drop(native);
        drop(state_socket);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = listener.local_addr()?;
        let local_server = tokio::spawn(async move {
            let (mut stream, address) =
                tokio::time::timeout(Duration::from_secs(6), listener.accept()).await??;
            ensure!(
                address.ip().is_loopback(),
                "unexpected non-loopback fixture request"
            );
            let mut request = [0; 2048];
            let read =
                tokio::time::timeout(Duration::from_secs(2), stream.read(&mut request)).await??;
            ensure!(
                read > 0 && request[..read].starts_with(b"GET /fixture HTTP/"),
                "unexpected fixture request"
            );
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: text/html\r\nContent-Length: 29\r\n\r\n<title>stock-fixture</title>\n").await?;
            Ok::<_, anyhow::Error>(())
        });
        pipe.call(
            "Target.createTarget",
            json!({"url":format!("http://{endpoint}/fixture")}),
            None,
        )
        .await?;
        local_server.await??;

        let info = pipe
            .call("SystemInfo.getProcessInfo", json!({}), None)
            .await?;
        ensure!(
            info["processInfo"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|entry| entry["type"] == "renderer")
                .any(|entry| entry["id"]
                    .as_u64()
                    .and_then(|pid| fs::read_to_string(format!("/proc/{pid}/status")).ok())
                    .is_some_and(|status| status.lines().any(|line| line
                        .strip_prefix("Seccomp:")
                        .is_some_and(|mode| mode.trim() == "2")))),
            "no sandboxed renderer observed"
        );
        close_task().await?;
    }
    // Exercise the same Linux result-ack state used by the task driver.
    // A healthy native release preserves the still-owned CDP generation.
    let (first, _) = open_task(root, false).await?;
    let (owner, mut peer) = UnixStream::pair()?;
    first.acknowledge(owner, "account-free-result".into());
    ensure!(
        crate::native::read(&mut peer).await?["type"] == "ack",
        "wrong native result ack"
    );
    crate::native::write(
        &mut peer,
        &json!({"type":"released","job":"account-free-result"}),
    )
    .await?;
    let (reused, _) = open_task(root, false).await?;
    ensure!(
        Arc::ptr_eq(&first.pipe, &reused.pipe),
        "affirmative release did not preserve the generation"
    );

    // Retain the original peer but never send released. Retire the old
    // browser before admitting a new generation, without poisoning CDP.
    let (owner, mut stalled_peer) = UnixStream::pair()?;
    reused.acknowledge(owner, "account-free-stalled-result".into());
    ensure!(
        crate::native::read(&mut stalled_peer).await?["type"] == "ack",
        "stalled peer did not receive ack"
    );
    let since = tokio::time::Instant::now();
    let (fresh, _) = open_task(root, false).await?;
    ensure!(
        !Arc::ptr_eq(&first.pipe, &fresh.pipe) && since.elapsed() >= Duration::from_millis(900),
        "unreleased task did not retire its exact browser"
    );
    close_task().await?;
    println!(
        "healthy native result release reuses exact browser; stalled release retires it and admits a fresh, verified generation"
    );
    println!(
        "stock Chrome: two owned sandbox-default launches, exact embedded extension ID and native HELLO, sandboxed loopback renderer, acknowledged Browser.close, two root-reap/ECHILD/EOF proofs, durable generation rotation"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn pair() -> (Pipe, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        (
            Pipe {
                channel: BufReader::new(ours),
                next_id: 0,
                usable: true,
            },
            theirs,
        )
    }

    #[tokio::test]
    async fn unrelated_response_poison_cannot_send_a_second_command() {
        let (mut pipe, mut peer) = pair();
        let task = tokio::spawn(async move {
            let mut request = [0_u8; 256];
            assert!(peer.read(&mut request).await.unwrap() > 0);
            peer.write_all(b"{\"id\":980,\"result\":{}}\0")
                .await
                .unwrap();
            let outcome =
                tokio::time::timeout(Duration::from_millis(30), peer.read(&mut request)).await;
            assert!(outcome.is_err());
        });
        assert!(
            pipe.call("Browser.getVersion", json!({}), None)
                .await
                .is_err()
        );
        assert!(pipe.call("Browser.close", json!({}), None).await.is_err());
        task.await.unwrap();
    }

    #[tokio::test]
    async fn matched_capability_refusal_can_be_followed_by_verified_cleanup() {
        let (mut pipe, peer) = pair();
        let task = tokio::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut frame = Vec::new();
            peer.read_until(0, &mut frame).await.unwrap();
            assert!(frame.starts_with(b"{\"id\":1,"));
            peer.write_all(b"{\"id\":1,\"error\":{\"code\":-32601,\"message\":\"unavailable\"}}\0")
                .await
                .unwrap();
            frame.clear();
            peer.read_until(0, &mut frame).await.unwrap();
            assert!(frame.starts_with(b"{\"id\":2,"));
            peer.write_all(b"{\"id\":2,\"result\":{}}\0").await.unwrap();
        });
        assert!(
            pipe.call("Extensions.loadUnpacked", json!({}), None)
                .await
                .is_err()
        );
        assert_eq!(
            pipe.call("Browser.close", json!({}), None).await.unwrap(),
            json!({})
        );
        task.await.unwrap();
    }

    #[test]
    fn registration_rejects_aliases_and_only_accepts_dedicated_profile() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let unrelated = root.join("regular-profile");
        create_private(&unrelated).unwrap();
        let dedicated = root.join("stock-chrome-profile");
        symlink(&unrelated, &dedicated).unwrap();
        assert!(validate_registration(&root, &dedicated).is_err());
        assert!(validate_registration(&root, &unrelated).is_err());
        fs::remove_file(&dedicated).unwrap();
        create_private(&dedicated).unwrap();
        assert_eq!(validate_registration(&root, &dedicated).unwrap(), dedicated);
        let ownership = root.join("stock-chrome-generation");
        assert!(!receipt::fenced(&ownership).unwrap());
        let generation = receipt::begin(&ownership, &dedicated).unwrap();
        receipt::verify_pending_profile(&ownership, generation.nonce(), &dedicated).unwrap();
        assert!(
            receipt::verify_pending_profile(&ownership, generation.nonce(), &unrelated).is_err()
        );
        assert!(receipt::fenced(&ownership).unwrap());
        assert!(receipt::begin(&ownership, &dedicated).is_err());
    }
}
