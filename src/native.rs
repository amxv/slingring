//! One-shot Chrome Native Messaging host. It exists only during an active task.
use crate::config;
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::UnixStream,
    },
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
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn register(root: &Path, browser_data: &Path) -> Result<()> {
    ensure!(root.is_absolute(), "state directory must be absolute");
    config::private_dir(root)?;
    assets(root)?;
    let wrapper = root.join("native-host");
    let text = format!(
        "#!/bin/sh\nexec {} native-host --home {} \"$@\"\n",
        quote(&std::env::current_exe()?.to_string_lossy()),
        quote(&root.to_string_lossy())
    );
    config::atomic(&wrapper, text.as_bytes())?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))?;
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
    let hosts = data.join("NativeMessagingHosts");
    config::private_dir(&hosts)?;
    let manifest = json!({"name":HOST,"description":"Slingring task bridge","path":wrapper,
        "type":"stdio","allowed_origins":[format!("chrome-extension://{EXTENSION_ID}/")]});
    let path = hosts.join(format!("{HOST}.json"));
    ensure!(
        !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
        "native registration cannot be a symlink"
    );
    let temporary = hosts.join(format!(".slingring-{}", uuid::Uuid::new_v4()));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    f.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    f.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
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
    let mut socket =
        UnixStream::connect(root.join("browser.sock")).context("no active Slingring task")?;
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
