//! Bounded, private, payload-free task lifecycle events. Never log prompts,
//! responses, URLs, browser output, auth headers or raw external errors.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

const MAX_LOG_BYTES: u64 = 256 * 1024;
fn code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'))
}

/// Best-effort diagnostics must never change submission behavior.
pub fn event(root: &Path, job_id: &str, stage: &str, status: &str, detail: &str, elapsed_ms: u64) {
    if ![job_id, stage, status, detail].iter().all(|v| code(v)) {
        return;
    }
    let path = root.join("events.log");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return;
        }
        if meta.len() >= MAX_LOG_BYTES {
            // Keep the most recent previous segment for troubleshooting.
            if fs::rename(&path, root.join("events.previous.log")).is_err() {
                return;
            }
        }
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(
        file,
        "at_ms={} job={} stage={} status={} detail={} elapsed_ms={}",
        crate::queue::now(),
        job_id,
        stage,
        status,
        detail,
        elapsed_ms
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn event_log_is_private_bounded_and_drops_unsafe_fields() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        event(
            dir.path(),
            "safe_id",
            "browser_launch",
            "in_progress",
            "none",
            123,
        );
        event(
            dir.path(),
            "unsafe id",
            "private prompt",
            "in_progress",
            "none",
            321,
        );
        let log = dir.path().join("events.log");
        let content = fs::read_to_string(&log)?;
        assert!(content.contains(
            "job=safe_id stage=browser_launch status=in_progress detail=none elapsed_ms=123"
        ));
        assert!(!content.contains("unsafe id"));
        assert!(!content.contains("private prompt"));
        assert_eq!(fs::metadata(&log)?.permissions().mode() & 0o777, 0o600);
        fs::rename(&log, dir.path().join("original.log"))?;
        symlink(dir.path().join("original.log"), &log)?;
        event(
            dir.path(),
            "safe_id",
            "browser_retry",
            "in_progress",
            "none",
            456,
        );
        assert!(!fs::read_to_string(dir.path().join("original.log"))?.contains("browser_retry"));
        Ok(())
    }

    #[test]
    fn events_rotate_without_unbounded_growth() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("events.log"),
            vec![b'x'; MAX_LOG_BYTES as usize],
        )?;
        event(
            dir.path(),
            "safe",
            "browser_launch",
            "in_progress",
            "none",
            7,
        );
        assert_eq!(
            fs::metadata(dir.path().join("events.previous.log"))?.len(),
            MAX_LOG_BYTES
        );
        assert!(fs::metadata(dir.path().join("events.log"))?.len() < 1024);
        Ok(())
    }
}
