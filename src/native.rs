//! One-shot Chrome Native Messaging host. It exists only during an active task.
use crate::{config, ipc};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const HOST: &str = "xyz.ashray.slingring";
pub const EXTENSION_ID: &str = "fbackmlfhpjaindjdpiamojfpdneobpe";
pub const MAX_FRAME: usize = 512 * 1024;
const FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../extension/manifest.json")),
    ("bridge.html", include_str!("../extension/bridge.html")),
    ("bridge.js", include_str!("../extension/bridge.js")),
    ("dom.js", include_str!("../extension/dom.js")),
    ("content.js", include_str!("../extension/content.js")),
];
pub fn assets(root: &Path) -> Result<PathBuf> {
    let directory = root.join("extension");
    config::private_dir(&directory)?;
    for (name, bytes) in FILES {
        let path = directory.join(name);
        if fs::read(&path).ok().as_deref() != Some(bytes.as_bytes()) {
            config::atomic(&path, bytes.as_bytes())?;
        }
    }
    Ok(directory)
}
#[cfg(unix)]
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Owns the temporary per-user Windows native host registration. On Unix the
/// registration lives only in the copied Chrome profile and needs no cleanup.
pub struct Registration {
    #[cfg(windows)]
    manifest: PathBuf,
}
#[cfg(windows)]
const REGISTRY_KEY: &str = r"HKCU\Software\Google\Chrome\NativeMessagingHosts\xyz.ashray.slingring";

#[cfg(windows)]
fn registered_manifest() -> Result<Option<PathBuf>> {
    let output = std::process::Command::new("reg.exe")
        .args(["query", REGISTRY_KEY, "/ve"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let body = String::from_utf8(output.stdout)?;
    let value = body
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, path)| path.trim()));
    Ok(value.map(PathBuf::from))
}
#[cfg(windows)]
impl Drop for Registration {
    fn drop(&mut self) {
        if registered_manifest().ok().flatten().as_deref() == Some(self.manifest.as_path()) {
            let _ = std::process::Command::new("reg.exe")
                .args(["delete", REGISTRY_KEY, "/f"])
                .status();
        }
    }
}

pub fn register(root: &Path, browser_data: &Path) -> Result<Registration> {
    ensure!(root.is_absolute(), "state directory must be absolute");
    config::private_dir(root)?;
    assets(root)?;
    #[cfg(unix)]
    let wrapper = root.join("native-host");
    #[cfg(unix)]
    {
        let text = format!(
            "#!/bin/sh\nexec {} native-host --home {} \"$@\"\n",
            quote(&std::env::current_exe()?.to_string_lossy()),
            quote(&root.to_string_lossy())
        );
        config::atomic(&wrapper, text.as_bytes())?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))?;
    }
    // A copied profile uses its own NativeMessagingHosts directory. Register
    // ONLY there, never alter the user's everyday Chrome installation.
    let data = browser_data.canonicalize()?;
    ensure!(
        data.starts_with(std::env::temp_dir().canonicalize()?)
            && data
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.starts_with("agent-browser-profile-")),
        "native host registration requires an isolated agent-browser profile copy"
    );
    #[cfg(unix)]
    let hosts = data.join("NativeMessagingHosts");
    #[cfg(windows)]
    let hosts = root.to_path_buf();
    config::private_dir(&hosts)?;
    #[cfg(unix)]
    let binary = wrapper;
    #[cfg(windows)]
    let binary = std::env::current_exe()?;
    let manifest = json!({"name":HOST,"description":"Slingring task bridge","path":binary,
        "type":"stdio","allowed_origins":[format!("chrome-extension://{EXTENSION_ID}/")]});
    #[cfg(unix)]
    let path = hosts.join(format!("{HOST}.json"));
    #[cfg(windows)]
    let path = hosts.join("native-host.json");
    ensure!(
        !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
        "native registration cannot be a symlink"
    );
    let temporary = hosts.join(format!(".slingring-{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&temporary)?;
    f.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    f.sync_all()?;
    fs::rename(temporary, path)?;
    #[cfg(windows)]
    {
        if let Some(existing) = registered_manifest()? {
            ensure!(
                existing == hosts.join("native-host.json"),
                "another native host is registered; refusing to change a different installation"
            );
        }
        let manifest_path = hosts.join("native-host.json");
        let status = std::process::Command::new("reg.exe")
            .args(["add", REGISTRY_KEY, "/ve", "/t", "REG_SZ", "/d"])
            .arg(&manifest_path)
            .arg("/f")
            .status()?;
        ensure!(status.success(), "Windows Chrome host registration failed");
        Ok(Registration {
            manifest: manifest_path,
        })
    }
    #[cfg(unix)]
    Ok(Registration {})
}

#[cfg(windows)]
pub fn serve_registered(browser_args: &[String]) -> Result<()> {
    let manifest = registered_manifest()?.context("native host is not registered")?;
    ensure!(
        manifest
            .file_name()
            .is_some_and(|name| name == "native-host.json"),
        "unexpected native host manifest"
    );
    let root = manifest
        .parent()
        .context("native host manifest has no directory")?;
    let registered: serde_json::Value =
        serde_json::from_slice(&config::read_private(&manifest, 4096)?)?;
    ensure!(
        registered["path"] == json!(std::env::current_exe()?) && registered["name"] == HOST,
        "native host manifest does not match this executable"
    );
    serve(root, browser_args)
}
fn forward_frame(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header) as usize;
    ensure!((1..=MAX_FRAME).contains(&size), "invalid native frame size");
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    writer.write_all(&header)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
/// No async runtime: on socket EOF the main thread exits even if Chrome's stdin
/// remains open. The only helper thread and process are both short-lived.
pub fn serve(root: &Path, browser_args: &[String]) -> Result<()> {
    let expected = format!("chrome-extension://{EXTENSION_ID}/");
    ensure!(
        browser_args.first() == Some(&expected),
        "native sender not authorized"
    );
    let mut socket = ipc::native_connect(root).context("no active Slingring task")?;
    let mut outgoing = socket.try_clone()?;
    std::thread::spawn(move || {
        let input = std::io::stdin();
        let mut input = input.lock();
        while forward_frame(&mut input, &mut outgoing).is_ok() {}
        let _ = outgoing.shutdown(std::net::Shutdown::Both);
    });
    let output = std::io::stdout();
    let mut output = output.lock();
    while forward_frame(&mut socket, &mut output).is_ok() {}
    Ok(())
}
pub async fn read<R: tokio::io::AsyncRead + Unpin>(stream: &mut R) -> Result<serde_json::Value> {
    use tokio::io::AsyncReadExt;
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    let size = u32::from_ne_bytes(header) as usize;
    ensure!((1..=MAX_FRAME).contains(&size), "invalid native frame size");
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn write<W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    value: &serde_json::Value,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_FRAME, "native frame too large");
    stream
        .write_all(&(bytes.len() as u32).to_ne_bytes())
        .await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}
