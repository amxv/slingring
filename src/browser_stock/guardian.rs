//! Single-generation Linux guardian. Hard deadlines are not terminal evidence.
use super::{GroupLease, receipt};
use rustix::{
    io::Errno,
    process::{Pid, WaitId, WaitIdOptions, child_subreaper, set_child_subreaper, setsid, waitid},
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

pub const MODE: &str = "--private-slingring-stock-chrome-guardian";
pub const WAIT: Duration = Duration::from_secs(7);

const START: u8 = 1;
const POLL: u8 = 2;
const ARM: u8 = 3;
const FINISH: u8 = 4;
const STARTED: u8 = 11;
const LIVE: u8 = 12;
const ARMED: u8 = 13;
const TERMINAL: u8 = 14;
const FRAME: usize = 21;

enum Reply {
    Frame([u8; FRAME]),
    Eof,
    Invalid,
}

pub struct Terminal([u8; 16]);
impl Terminal {
    pub(super) fn matches(&self, nonce: [u8; 16]) -> bool {
        self.0 == nonce
    }
}

fn frame(kind: u8, generation: [u8; 16], root: u32) -> [u8; FRAME] {
    let mut v = [0; FRAME];
    v[0] = kind;
    v[1..17].copy_from_slice(&generation);
    v[17..].copy_from_slice(&root.to_be_bytes());
    v
}

fn no_descendants(budget: Duration) -> io::Result<()> {
    let opts = WaitIdOptions::EXITED
        | WaitIdOptions::NOHANG
        | WaitIdOptions::from_bits_retain(linux_raw_sys::general::__WALL);
    let deadline = Instant::now() + budget;
    loop {
        match waitid(WaitId::All, opts) {
            Err(Errno::CHILD) => return Ok(()),
            Ok(Some(_)) | Err(Errno::INTR) => {}
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("live or unknown descendant"));
        }
    }
}

pub(super) fn child_mode(arguments: &[OsString]) -> io::Result<()> {
    let [root, socket, profile] = arguments else {
        return Err(io::Error::other("invalid private guardian bootstrap"));
    };
    let root = std::path::Path::new(root);
    let socket = std::path::Path::new(socket);
    let profile = std::path::Path::new(profile);
    // This is a dedicated process. No other children or competing waits.
    setsid()?;
    set_child_subreaper(Pid::from_raw(1))?;
    if child_subreaper()?.is_none() {
        return Err(io::Error::other("subreaper readback failed"));
    }
    no_descendants(Duration::ZERO)?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut initial = [0; 17];
    input.read_exact(&mut initial)?;
    if initial[0] != START || initial[1..] == [0; 16] {
        return Err(io::Error::other("invalid start message"));
    }
    let generation = initial[1..].try_into().unwrap();
    super::require_private(root)?;
    super::require_private(profile)?;
    let ownership = root.join("stock-chrome-generation");
    receipt::verify_pending_profile(&ownership, generation, profile)?;
    let channel = std::os::unix::net::UnixStream::connect(socket)?;
    channel.set_write_timeout(Some(Duration::from_secs(3)))?;
    (&channel).write_all(&generation)?;
    channel.set_write_timeout(None)?;
    // The guardian's 0/1 are private control pipes. Explicitly remap the
    // separate owner channel to Chromium's 3/4, then exec the exact browser.
    // No command text, shell interpolation or executable path from callers.
    let cdp_in = channel.try_clone()?;
    let cdp_out = channel;
    let cwd = profile
        .parent()
        .ok_or_else(|| io::Error::other("invalid profile"))?;
    let chrome = super::chrome_executable()?;
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "exec 3<&0 4>&1 0</dev/null 1>/dev/null; exec \"$@\"",
            "slingring-chrome",
        ])
        .arg(chrome)
        .args([
            "--headless",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--remote-debugging-pipe",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", cwd.join("home"))
        .env("XDG_CONFIG_HOME", cwd.join("home/.config"))
        .env("XDG_CACHE_HOME", cwd.join("home/.cache"))
        .env("XDG_DATA_HOME", cwd.join("home/.local/share"))
        .current_dir(cwd)
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(cdp_in)))
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(cdp_out)))
        .stderr(Stdio::null());
    let mut lease = GroupLease::spawn(&mut command)?;
    let root = lease.id();
    send(&mut output, STARTED, generation, root)?;
    let mut armed = false;
    loop {
        let mut command = [0; 1];
        input.read_exact(&mut command)?;
        match command[0] {
            POLL if !armed && lease.observe()?.is_none() => {
                send(&mut output, LIVE, generation, root)?
            }
            ARM if !armed && lease.observe()?.is_none() => {
                send(&mut output, ARMED, generation, root)?;
                armed = true;
            }
            FINISH if armed => {
                // A normal path has no forced signals before or after reap.
                if !lease.reap_without_signal(Duration::from_secs(4))?.success() {
                    return Err(io::Error::other("browser root exit was not successful"));
                }
                no_descendants(Duration::from_secs(2))?;
                send(&mut output, TERMINAL, generation, root)?;
                return Ok(());
            }
            _ => return Err(io::Error::other("guardian state unverified")),
        }
    }
}
fn send(output: &mut impl Write, kind: u8, generation: [u8; 16], root: u32) -> io::Result<()> {
    output.write_all(&frame(kind, generation, root))?;
    output.flush()
}

/// Only the live direct owner of this Child and its unshared anonymous pipes
/// may obtain an ordinary terminal proof; the guardian never writes one.
pub struct GuardedChild {
    guard: Child,
    input: Option<ChildStdin>,
    replies: Mutex<Receiver<Reply>>,
    generation: [u8; 16],
    root: u32,
    armed: bool,
    poisoned: bool,
}

impl GuardedChild {
    pub fn spawn(
        guardian_executable: &std::path::Path,
        argv: &[&std::path::Path],
        generation: [u8; 16],
    ) -> io::Result<Self> {
        let mut command = Command::new(guardian_executable);
        command.arg(MODE).args(argv);
        let mut guard = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = guard
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing guardian stdin"))?;
        let mut output = guard
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing guardian stdout"))?;
        let (tx, replies) = mpsc::sync_channel(2);
        thread::Builder::new()
            .name("guardian-reply".into())
            .spawn(move || {
                loop {
                    let mut frame = [0; FRAME];
                    let first = loop {
                        match output.read(&mut frame[0..1]) {
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            result => break result,
                        }
                    };
                    let reply = match first {
                        Ok(0) => Reply::Eof,
                        Ok(1) if output.read_exact(&mut frame[1..]).is_ok() => Reply::Frame(frame),
                        _ => Reply::Invalid,
                    };
                    let done = !matches!(reply, Reply::Frame(_));
                    if tx.send(reply).is_err() || done {
                        break;
                    }
                }
            })?;
        let mut this = Self {
            guard,
            input: Some(input),
            replies: Mutex::new(replies),
            generation,
            root: 0,
            armed: false,
            poisoned: false,
        };
        let sent = this.input.as_mut().unwrap();
        sent.write_all(&[START])?;
        sent.write_all(&generation)?;
        sent.flush()?;
        let msg = this.receive()?;
        this.root = u32::from_be_bytes(msg[17..].try_into().unwrap());
        if this.root < 2 || msg != frame(STARTED, generation, this.root) {
            return Err(io::Error::other("invalid guardian startup record"));
        }
        Ok(this)
    }

    fn receive(&self) -> io::Result<[u8; FRAME]> {
        match self
            .replies
            .lock()
            .map_err(|_| io::Error::other("guardian reply lock poisoned"))?
            .recv_timeout(WAIT)
        {
            Ok(Reply::Frame(reply)) => Ok(reply),
            _ => Err(io::Error::other("guardian outcome unverified")),
        }
    }

    fn exchange(&mut self, request: u8, response: u8) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other("guardian lease irreversibly unverified"));
        }
        let result = (|| {
            let pipe = self
                .input
                .as_mut()
                .ok_or_else(|| io::Error::other("guardian pipe closed"))?;
            pipe.write_all(&[request])?;
            pipe.flush()?;
            if self.receive()? != frame(response, self.generation, self.root) {
                return Err(io::Error::other("guardian generation mismatch"));
            }
            Ok(())
        })();
        if result.is_err() {
            self.poisoned = true;
            self.input.take();
        }
        result
    }
    pub fn guardian_pid(&self) -> u32 {
        self.guard.id()
    }
    pub fn root(&self) -> u32 {
        self.root
    }
    pub fn check_live(&mut self) -> io::Result<()> {
        self.exchange(POLL, LIVE)
    }
    pub fn arm(&mut self) -> io::Result<()> {
        self.exchange(ARM, ARMED)?;
        self.armed = true;
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<Terminal> {
        if !self.armed || self.poisoned {
            return Err(io::Error::other("ordinary close not authorized"));
        }
        self.armed = false;
        self.exchange(FINISH, TERMINAL)?;
        self.input.take();
        if !matches!(
            self.replies
                .lock()
                .map_err(|_| io::Error::other("guardian reply lock poisoned"))?
                .recv_timeout(Duration::from_secs(2)),
            Ok(Reply::Eof)
        ) {
            return Err(io::Error::other("guardian protocol did not end cleanly"));
        }
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if let Some(status) = self.guard.try_wait()? {
                if status.success() {
                    return Ok(Terminal(self.generation));
                }
                return Err(io::Error::other(
                    "guardian did not exit with bounded complete proof",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(io::Error::other("guardian shutdown unverified"))
    }
}
impl Drop for GuardedChild {
    fn drop(&mut self) {
        // Abandonment only closes the private pipe. A tardy or partial report
        // is never accepted, and we never signal a recovered numeric PID.
        self.input.take();
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if self.guard.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
