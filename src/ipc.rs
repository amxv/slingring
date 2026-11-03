//! Private IPC. Unix uses 0600 Unix-domain sockets; Windows uses loopback
//! TCP with a random one-use endpoint secret in the restricted state folder.
use crate::config;
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy)]
pub enum Kind {
    Daemon,
    Browser,
}
impl Kind {
    fn stem(self) -> &'static str {
        match self {
            Self::Daemon => "slingring",
            Self::Browser => "browser",
        }
    }
}

#[cfg(unix)]
pub type Stream = tokio::net::UnixStream;
#[cfg(windows)]
pub type Stream = tokio::net::TcpStream;
#[cfg(unix)]
pub type Listener = tokio::net::UnixListener;
#[cfg(windows)]
pub type Listener = tokio::net::TcpListener;
#[cfg(unix)]
pub type NativeStream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub type NativeStream = std::net::TcpStream;

pub fn endpoint(root: &Path, kind: Kind) -> PathBuf {
    #[cfg(unix)]
    return root.join(format!("{}.sock", kind.stem()));
    #[cfg(windows)]
    return root.join(format!("{}.endpoint.json", kind.stem()));
}

pub struct Cleanup(pub PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
#[derive(serde::Deserialize, serde::Serialize)]
struct Endpoint {
    port: u16,
    secret: String,
}
#[cfg(windows)]
fn read_endpoint(root: &Path, kind: Kind) -> Result<Endpoint> {
    let bytes = config::read_private(&endpoint(root, kind), 4096)?;
    let value: Endpoint = serde_json::from_slice(&bytes)?;
    ensure!(
        value.port > 0 && uuid::Uuid::parse_str(&value.secret).is_ok(),
        "invalid private endpoint"
    );
    Ok(value)
}

pub async fn bind(root: &Path, kind: Kind) -> Result<Listener> {
    config::private_dir(root)?;
    let path = endpoint(root, kind);
    if path.exists() {
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            !meta.file_type().is_symlink(),
            "IPC endpoint cannot be a symlink"
        );
        std::fs::remove_file(&path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let listener = Listener::bind(&path).context("could not bind private Unix socket")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }
    #[cfg(windows)]
    {
        let listener = Listener::bind(("127.0.0.1", 0)).await?;
        let value = Endpoint {
            port: listener.local_addr()?.port(),
            secret: uuid::Uuid::new_v4().to_string(),
        };
        config::atomic(&path, &serde_json::to_vec(&value)?)?;
        Ok(listener)
    }
}

pub async fn connect(root: &Path, kind: Kind) -> Result<Stream> {
    #[cfg(unix)]
    {
        Stream::connect(endpoint(root, kind))
            .await
            .context("Slingring service is not running")
    }
    #[cfg(windows)]
    {
        let value = read_endpoint(root, kind).context("Slingring service is not running")?;
        let mut stream = Stream::connect(("127.0.0.1", value.port)).await?;
        stream
            .write_all(format!("{}\n", value.secret).as_bytes())
            .await?;
        Ok(stream)
    }
}

pub async fn authenticate(stream: &mut Stream, root: &Path, kind: Kind) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = (stream, root, kind);
        Ok(())
    }
    #[cfg(windows)]
    {
        let expected = read_endpoint(root, kind)?;
        let mut provided = [0u8; 37];
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            stream.read_exact(&mut provided),
        )
        .await??;
        ensure!(
            provided[36] == b'\n' && provided[..36] == expected.secret.as_bytes()[..],
            "local IPC authentication failed"
        );
        Ok(())
    }
}

pub fn native_connect(root: &Path) -> Result<NativeStream> {
    #[cfg(unix)]
    {
        NativeStream::connect(endpoint(root, Kind::Browser)).context("no active Slingring task")
    }
    #[cfg(windows)]
    {
        use std::io::Write;
        let value = read_endpoint(root, Kind::Browser)?;
        let mut stream = NativeStream::connect(("127.0.0.1", value.port))?;
        stream.write_all(format!("{}\n", value.secret).as_bytes())?;
        Ok(stream)
    }
}
