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
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

const MAX_FRAME: usize = 4 * 1024 * 1024;
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

fn create_private(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    require_private(path)
}

pub(crate) fn maybe_run_guardian() -> Option<Result<()>> {
    let mut args = std::env::args_os().skip(1);
    match args.next()?.to_str()? {
        guardian::MODE => Some(guardian::child_mode(&args.collect::<Vec<_>>()).map_err(Into::into)),
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
struct Pipe {
    channel: BufReader<UnixStream>,
    next_id: u64,
    usable: bool,
}
impl Pipe {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        ensure!(self.usable, "private browser channel unverified");
        self.usable = false;
        self.next_id = self.next_id.checked_add(1).context("CDP ID exhausted")?;
        let id = self.next_id;
        let mut bytes = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))?;
        ensure!(bytes.len() < MAX_FRAME, "CDP command size exceeds limit");
        bytes.push(0);
        let result = tokio::time::timeout(Duration::from_secs(8), async {
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
                ensure!(
                    reply.get("error").is_none(),
                    "stock Chrome rejected {method}"
                );
                return reply.get("result").cloned().context("missing CDP result");
            }
        })
        .await??;
        self.usable = true;
        Ok(result)
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
    pipe: Pipe,
    guardian: GuardedChild,
    evidence: receipt::Generation,
    profile: PathBuf,
}
impl Browser {
    async fn open(root: &Path) -> Result<Self> {
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
        // Keep the permissions and exact profile name contract native expects.
        let profile = runtime.join(format!(
            "agent-browser-profile-{}",
            uuid::Uuid::new_v4().simple()
        ));
        create_private(&profile)?;
        // The home belongs to this runtime only; never inherit the user profile.
        let home = runtime.join("home");
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
        let paths = [root.to_owned(), socket.0.clone(), profile.clone()];
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
            pipe: Pipe {
                channel: BufReader::new(channel),
                next_id: 0,
                usable: true,
            },
            guardian,
            evidence,
            profile,
        };
        this.verify_root().await?;
        Ok(this)
    }

    async fn verify_root(&mut self) -> Result<()> {
        let info = self
            .pipe
            .call("SystemInfo.getProcessInfo", json!({}))
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
        let exe = fs::read_link(format!("/proc/{}/exe", self.guardian.root()))?;
        ensure!(
            exe == chrome_executable()?,
            "owned root did not exec stock Chrome"
        );
        Ok(())
    }

    async fn close(mut self) -> Result<()> {
        self.verify_root().await?;
        self.guardian.arm()?;
        self.pipe.call("Browser.close", json!({})).await?;
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
    for _ in 0..2 {
        let mut browser = Browser::open(root).await?;
        let extension = crate::native::assets(root)?;
        crate::native::register(root, &browser.profile)?;
        let loaded = browser
            .pipe
            .call("Extensions.loadUnpacked", json!({"path":extension}))
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
        browser
            .pipe
            .call("Target.createTarget", json!({"url":url}))
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
        browser
            .pipe
            .call(
                "Target.createTarget",
                json!({"url":format!("http://{endpoint}/fixture")}),
            )
            .await?;
        local_server.await??;

        let info = browser
            .pipe
            .call("SystemInfo.getProcessInfo", json!({}))
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
        browser.close().await?;
    }
    println!(
        "stock Chrome: two owned sandbox-default launches, exact embedded extension ID and native HELLO, sandboxed loopback renderer, acknowledged Browser.close, two root-reap/ECHILD/EOF proofs, durable generation rotation"
    );
    Ok(())
}
