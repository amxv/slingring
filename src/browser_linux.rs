//! Linux-only, read-only process/profile checks. This is not a process reaper:
//! a PID observation never grants authority to signal a process (see issue #4).
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    os::unix::{
        ffi::OsStringExt,
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub(crate) const CHROME: &str = "/opt/google/chrome/chrome";

fn sandbox_host(uid: u32, ci: bool, container_file: bool, cgroups: &str) -> Result<()> {
    ensure!(
        uid != 0
            && !ci
            && !container_file
            && !["docker", "kubepods", "lxc"]
                .iter()
                .any(|word| cgroups.contains(word)),
        "Linux browser requires a regular sandbox-capable user session, not root, CI or a container"
    );
    Ok(())
}

/// Refuse the exact environments in which the pinned helper automatically adds
/// --no-sandbox. Never change policy or override that safety check with flags.
pub(crate) fn preflight() -> Result<()> {
    let cgroups = String::from_utf8(bounded(Path::new("/proc/1/cgroup"))?)?;
    sandbox_host(
        uid()?,
        std::env::var("CI").is_ok(),
        Path::new("/.dockerenv").try_exists()? || Path::new("/run/.containerenv").try_exists()?,
        &cgroups,
    )?;
    expected_executable()?;
    Ok(())
}

fn expected_executable() -> Result<PathBuf> {
    let exe = Path::new(CHROME)
        .canonicalize()
        .context("Linux requires Google Chrome Stable at /opt/google/chrome/chrome")?;
    let metadata = fs::metadata(&exe)?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o111 != 0,
        "Linux requires a system-owned, non-writable Google Chrome executable"
    );
    Ok(exe)
}

fn uid() -> Result<u32> {
    Ok(fs::metadata("/proc/self")?.uid())
}

fn private_directory(path: &Path, create: bool) -> Result<()> {
    if create {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == uid()?
            && metadata.mode() & 0o077 == 0,
        "Linux browser directory must be a private, owned, nonsymlink directory"
    );
    Ok(())
}

/// A short path also keeps the pinned helper's Unix socket below its 103-byte
/// limit. The root hash separates independent Slingring state directories.
pub(crate) fn runtime_root(root: &Path) -> Result<PathBuf> {
    static GENERATION: OnceLock<String> = OnceLock::new();
    let generation =
        GENERATION.get_or_init(|| uuid::Uuid::new_v4().simple().to_string()[..16].to_owned());
    let root = root.canonicalize()?;
    let digest = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
    Ok(std::env::temp_dir().canonicalize()?.join(format!(
        "sr-{}-{}-{generation}",
        uid()?,
        &digest[..8]
    )))
}

pub(crate) fn prepare(root: &Path, command: &mut tokio::process::Command) -> Result<()> {
    let directory = runtime_root(root)?;
    private_directory(&directory, true)?;
    let sockets = directory.join("sockets");
    let socket = sockets
        .join("namespaces/slingring/run")
        .join(format!("{}.sock", crate::browser::session(root)));
    ensure!(
        socket.as_os_str().as_encoded_bytes().len() <= 103,
        "Linux helper socket path exceeds its limit; use a shorter TMPDIR"
    );
    private_directory(&sockets, true)?;
    command
        .env("TMPDIR", &directory)
        .env("AGENT_BROWSER_SOCKET_DIR", &sockets);
    Ok(())
}

pub(crate) fn validate_profile(root: &Path, profile: &Path) -> Result<PathBuf> {
    let runtime = runtime_root(root)?;
    private_directory(&runtime, false)?;
    private_directory(profile, false)?;
    let profile = profile.canonicalize()?;
    ensure!(
        profile.parent() == Some(runtime.as_path())
            && profile
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("agent-browser-profile-")),
        "Linux native registration requires this session's private profile copy"
    );
    Ok(profile)
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    uid: u32,
    start: u64,
    executable: PathBuf,
}

fn bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 65_536,
        "Linux process metadata exceeds limit"
    );
    Ok(bytes)
}

fn identity(directory: &Path) -> Result<Identity> {
    let stat = String::from_utf8(bounded(&directory.join("stat"))?)?;
    // comm is parenthesized and may itself contain spaces/parentheses.
    let fields = stat
        .rsplit_once(") ")
        .context("invalid Linux process stat")?
        .1;
    let start = fields
        .split_whitespace()
        .nth(19)
        .context("Linux process start time missing")?
        .parse()?;
    let executable = fs::read_link(directory.join("exe"))?.canonicalize()?;
    ensure!(
        executable.is_file(),
        "Linux browser executable is unavailable"
    );
    Ok(Identity {
        uid: fs::metadata(directory)?.uid(),
        start,
        executable,
    })
}

fn profile_from_arguments(bytes: &[u8], expected: &Identity, headed: bool) -> Result<PathBuf> {
    ensure!(
        bytes.last() == Some(&0),
        "Linux process argv is not complete"
    );
    let args: Vec<_> = bytes[..bytes.len() - 1].split(|byte| *byte == 0).collect();
    ensure!(
        !args.is_empty() && args.len() <= 256,
        "invalid Linux process argv"
    );
    let executable = PathBuf::from(OsString::from_vec(args[0].to_vec())).canonicalize()?;
    ensure!(
        executable == expected.executable,
        "Linux browser executable identity mismatch"
    );
    ensure!(
        matches!(
            executable.file_name().and_then(|name| name.to_str()),
            Some("chrome" | "chromium" | "chromium-browser" | "google-chrome")
        ),
        "unexpected Linux browser executable"
    );
    for arg in &args[1..] {
        ensure!(
            ![
                b"--no-sandbox".as_slice(),
                b"--disable-setuid-sandbox",
                b"--disable-namespace-sandbox",
                b"--disable-seccomp-filter-sandbox"
            ]
            .iter()
            .any(|flag| *arg == *flag
                || arg
                    .strip_prefix(*flag)
                    .is_some_and(|tail| tail.starts_with(b"="))),
            "Linux browser sandbox must remain enabled"
        );
        ensure!(
            *arg != b"--type" && !arg.starts_with(b"--type=") && *arg != b"--user-data-dir",
            "CDP process is not the browser root"
        );
    }
    let headless = args
        .iter()
        .filter(|arg| **arg == b"--headless" || arg.starts_with(b"--headless="))
        .count();
    ensure!(
        headless <= 1 && (headless == 1) != headed,
        "Linux browser visibility mismatch"
    );
    let profiles: Vec<_> = args
        .iter()
        .filter_map(|arg| arg.strip_prefix(b"--user-data-dir="))
        .collect();
    ensure!(
        profiles.len() == 1 && !profiles[0].is_empty(),
        "Linux browser profile missing or ambiguous"
    );
    Ok(PathBuf::from(OsString::from_vec(profiles[0].to_vec())))
}

pub(crate) fn owned_profile(pid: u32, headed: bool, root: &Path) -> Result<PathBuf> {
    ensure!(pid > 0, "invalid browser PID");
    inspect(
        &Path::new("/proc").join(pid.to_string()),
        headed,
        root,
        &expected_executable()?,
    )
}

fn inspect(directory: &Path, headed: bool, root: &Path, executable: &Path) -> Result<PathBuf> {
    let before = identity(directory)?;
    ensure!(
        before.uid == uid()?,
        "Linux browser belongs to another user"
    );
    ensure!(
        before.executable == executable,
        "Linux browser is not the selected Google Chrome executable"
    );
    let args = bounded(&directory.join("cmdline"))?;
    let profile = profile_from_arguments(&args, &before, headed)?;
    let profile = validate_profile(root, &profile)?;
    ensure!(
        identity(directory)? == before,
        "Linux browser identity changed during inspection"
    );
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn automatic_sandbox_downgrade_environments_are_refused_before_launch() {
        assert!(sandbox_host(1000, false, false, "0::/user.slice").is_ok());
        assert!(sandbox_host(0, false, false, "").is_err());
        assert!(sandbox_host(1000, true, false, "").is_err());
        assert!(sandbox_host(1000, false, true, "").is_err());
        for name in ["docker", "kubepods", "lxc"] {
            assert!(sandbox_host(1000, false, false, name).is_err());
        }
    }

    #[test]
    fn nul_delimited_arguments_preserve_spaces_and_reject_ambiguous_or_unsafe_state() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("chrome");
        fs::write(&exe, b"fixture").unwrap();
        let id = Identity {
            uid: uid().unwrap(),
            start: 1,
            executable: exe.clone(),
        };
        let profile = temp.path().join("profile with spaces --not-an-option");
        let argv = |extras: &[&str]| {
            let mut args = vec![
                exe.to_string_lossy().into_owned(),
                format!("--user-data-dir={}", profile.display()),
            ];
            args.extend(extras.iter().map(|s| s.to_string()));
            format!("{}\0", args.join("\0")).into_bytes()
        };
        assert_eq!(
            profile_from_arguments(&argv(&["--headless=new"]), &id, false).unwrap(),
            profile
        );
        assert!(profile_from_arguments(&argv(&[]), &id, false).is_err());
        assert!(profile_from_arguments(&argv(&["--headless=new"]), &id, true).is_err());
        for flag in [
            "--no-sandbox",
            "--no-sandbox=true",
            "--disable-setuid-sandbox",
            "--type=renderer",
            "--type",
            "--user-data-dir",
            "--user-data-dir=/other",
            "--headless",
            "--headless=new",
        ] {
            assert!(
                profile_from_arguments(&argv(&["--headless=new", flag]), &id, false).is_err(),
                "{flag}"
            );
        }
        let mut incomplete = argv(&["--headless=new"]);
        incomplete.pop();
        assert!(profile_from_arguments(&incomplete, &id, false).is_err());
    }

    #[test]
    fn private_runtime_is_per_root_and_registration_rejects_aliases_or_public_profiles() {
        let root = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("/bin/true");
        prepare(root.path(), &mut command).unwrap();
        let runtime = runtime_root(root.path()).unwrap();
        let profile = runtime.join("agent-browser-profile-fixture");
        private_directory(&profile, true).unwrap();
        assert_eq!(validate_profile(root.path(), &profile).unwrap(), profile);
        let other = tempfile::tempdir().unwrap();
        assert_ne!(runtime_root(other.path()).unwrap(), runtime);
        assert!(validate_profile(other.path(), &profile).is_err());
        let alias = runtime.join("agent-browser-profile-alias");
        symlink(&profile, &alias).unwrap();
        assert!(validate_profile(root.path(), &alias).is_err());
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(validate_profile(root.path(), &profile).is_err());
        // Only fixture-created paths are removed; runtime is unique to this temp root.
        fs::remove_file(alias).unwrap();
        fs::remove_dir(profile).unwrap();
        fs::remove_dir(runtime.join("sockets")).unwrap();
        fs::remove_dir(runtime).unwrap();
    }

    #[test]
    fn process_identity_reads_kernel_uid_executable_and_start_without_signaling() {
        let id = identity(Path::new("/proc/self")).unwrap();
        assert_eq!(id.uid, uid().unwrap());
        assert!(id.start > 0);
        assert_eq!(
            id.executable,
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        let metadata = tempfile::tempdir().unwrap();
        fs::write(
            metadata.path().join("stat"),
            b"123 (name with ) parentheses) S 1 1",
        )
        .unwrap();
        assert!(identity(metadata.path()).is_err());
    }
}
