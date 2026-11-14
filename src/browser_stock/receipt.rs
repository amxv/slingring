//! Stable, append-only generation history. Unrecognized and incomplete states fence.
use rustix::fs::{FlockOperation, flock};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const TERM: &str = "normal-root-exit-descendants-empty-v2";

pub struct Generation {
    path: PathBuf,
    nonce: [u8; 16],
}
impl Generation {
    pub fn nonce(&self) -> [u8; 16] {
        self.nonce
    }

    /// Called only by the direct guardian owner, with an unforgeable
    /// in-process proof tied to this same generation.
    pub fn complete(&self, proof: super::guardian::Terminal) -> io::Result<()> {
        if !proof.matches(self.nonce) {
            return Err(io::Error::other("guardian proof generation mismatch"));
        }
        let lock = lock(&self.path)?;
        if read_nonce(&self.path)? != self.nonce {
            return Err(io::Error::other("ownership generation changed"));
        }
        write(
            &self.path,
            "terminal.new",
            format!("{TERM} {}\n", nonce_hex(self.nonce)).as_bytes(),
        )?;
        fs::rename(self.path.join("terminal.new"), self.path.join("terminal"))?;
        File::open(&self.path)?.sync_all()?;
        File::open(self.path.parent().unwrap())?.sync_all()?;
        drop(lock);
        Ok(())
    }
}

fn nonce_hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn private(meta: &fs::Metadata) -> bool {
    meta.uid() == rustix::process::geteuid().as_raw() && meta.mode() & 0o077 == 0
}
fn lock(path: &Path) -> io::Result<File> {
    let path = path.with_extension("ownership-lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(linux_raw_sys::general::O_NOFOLLOW as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || !private(&meta) {
        return Err(io::Error::other("stable ownership lock invalid"));
    }
    flock(&file, FlockOperation::NonBlockingLockExclusive)?;
    Ok(file)
}
fn read(path: &Path) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(
            (linux_raw_sys::general::O_NOFOLLOW | linux_raw_sys::general::O_NONBLOCK) as i32,
        )
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || !private(&meta) {
        return Err(io::Error::other("invalid stable evidence file"));
    }
    let mut bytes = Vec::new();
    file.take(257).read_to_end(&mut bytes)?;
    if bytes.len() > 256 {
        return Err(io::Error::other("oversize stable evidence"));
    }
    Ok(bytes)
}
fn read_nonce(dir: &Path) -> io::Result<[u8; 16]> {
    let path = dir.join("generation");
    let data = read(&path)?;
    let raw = std::str::from_utf8(&data).map_err(io::Error::other)?;
    if raw.len() != 32
        || !raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(io::Error::other("invalid ownership generation"));
    }
    let mut result = [0; 16];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(io::Error::other)?;
    }
    if result == [0; 16] {
        return Err(io::Error::other("zero ownership generation"));
    }
    Ok(result)
}
fn clean_marker(path: &Path) -> io::Result<Option<[u8; 16]>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_dir() && private(&meta) => {}
        _ => return Err(io::Error::other("ownership path unverified")),
    }
    let nonce = read_nonce(path)?;
    let profile = read(&path.join("profile"))?;
    if profile.first() != Some(&b'/') || profile.contains(&0) {
        return Err(io::Error::other("ownership profile identity unavailable"));
    }
    if read(&path.join("pending"))? != b"linux-guardian-v2\n"
        || read(&path.join("terminal"))? != format!("{TERM} {}\n", nonce_hex(nonce)).as_bytes()
    {
        return Err(io::Error::other("ownership terminal unverified"));
    }
    File::open(path)?.sync_all()?;
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(Some(nonce))
}
pub(crate) fn fenced(path: &Path) -> io::Result<bool> {
    let _lock = lock(path)?;
    Ok(clean_marker(path).is_err())
}
pub(super) fn verify_pending_profile(
    path: &Path,
    nonce: [u8; 16],
    profile: &Path,
) -> io::Result<()> {
    let _lock = lock(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || !private(&metadata)
        || read_nonce(path)? != nonce
        || read(&path.join("profile"))? != profile.as_os_str().as_bytes()
        || read(&path.join("pending"))? != b"linux-guardian-v2\n"
        || !matches!(fs::symlink_metadata(path.join("terminal")), Err(e) if e.kind()==io::ErrorKind::NotFound)
    {
        return Err(io::Error::other("pre-spawn ownership record unverified"));
    }
    File::open(path)?.sync_all()?;
    File::open(path.parent().unwrap())?.sync_all()
}

pub(crate) fn begin(path: &Path, profile: &Path) -> io::Result<Generation> {
    let _lock = lock(path)?;
    if let Some(prior) = clean_marker(path)? {
        let history = path.with_extension(format!("history-{}", nonce_hex(prior)));
        if !matches!(fs::symlink_metadata(&history), Err(e) if e.kind() == io::ErrorKind::NotFound)
        {
            return Err(io::Error::other("ownership history destination exists"));
        }
        fs::rename(path, &history)?;
        File::open(path.parent().unwrap())?.sync_all()?;
    }
    fs::DirBuilder::new().mode(0o700).create(path)?;
    File::open(path.parent().unwrap())?.sync_all()?;
    let mut nonce = [0; 16];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    if nonce == [0; 16] {
        return Err(io::Error::other("zero ownership generation"));
    }
    write(path, "generation", nonce_hex(nonce).as_bytes())?;
    let profile = profile.as_os_str().as_bytes();
    if profile.first() != Some(&b'/') || profile.len() > 256 || profile.contains(&0) {
        return Err(io::Error::other("ownership profile path is out of scope"));
    }
    write(path, "profile", profile)?;
    write(path, "pending", b"linux-guardian-v2\n")?;
    Ok(Generation {
        path: path.to_owned(),
        nonce,
    })
}
fn write(path: &Path, name: &str, data: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join(name))?;
    file.write_all(data)?;
    file.sync_all()?;
    File::open(path)?.sync_all()
}
