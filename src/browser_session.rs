//! Queue-scoped coordinator lifetime. Never grants authority to signal a PID.
//! The candidate production admission gate is closed until an exact packaged
//! helper/guardian capability and recovery policy have been reviewed.
use crate::{
    config,
    launch_coordinator::{self, GuardConfig, LaunchLease, PendingLaunch},
};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
use tokio::time::Instant;

enum Phase {
    Pending(PendingLaunch),
    Authorizing,
    Active(LaunchLease),
    ClosingUnverified,
    Closed,
}
pub struct Session {
    root: PathBuf,
    name: String,
    environment: Vec<(&'static str, String)>,
    phase: Mutex<Phase>,
    cancelled: watch::Sender<bool>,
}
impl Session {
    /// Integration seam for an explicitly approved artifact pair. The CLI does
    /// not call this constructor until its release capability gate is opened.
    pub fn prepare(root: &Path, guard: GuardConfig) -> Result<Arc<Self>> {
        let pending = PendingLaunch::prepare(root, guard)?;
        let name = pending.session().to_owned();
        let helper_directory = pending
            .socket_path()
            .parent()
            .context("launch socket missing")?
            .join("h");
        config::private_dir(&helper_directory)?;
        let mut environment = pending.environment();
        environment.push((
            "AGENT_BROWSER_SOCKET_DIR",
            helper_directory.to_string_lossy().into_owned(),
        ));
        // Both socket and restore-state namespaces are generation-specific.
        // A short session avoids Darwin's 104-byte Unix socket limit.
        environment.push(("AGENT_BROWSER_NAMESPACE", name.clone()));
        let helper_socket = helper_directory
            .join("namespaces")
            .join(&name)
            .join("run")
            .join(format!("{name}.sock"));
        ensure!(
            helper_socket.as_os_str().as_encoded_bytes().len() < 104,
            "helper socket path exceeds macOS limit"
        );
        Ok(Arc::new(Self {
            root: root.into(),
            name,
            environment,
            phase: Mutex::new(Phase::Pending(pending)),
            cancelled: watch::channel(false).0,
        }))
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn environment(&self) -> Vec<(&'static str, String)> {
        self.environment.clone()
    }
    pub fn is_active(&self) -> bool {
        matches!(*self.phase.lock().expect("session lock"), Phase::Active(_))
    }
    pub fn check_command(&self, opening: bool) -> Result<()> {
        match &mut *self.phase.lock().expect("session lock") {
            Phase::Pending(_) | Phase::Authorizing if opening => Ok(()),
            Phase::Active(lease) => lease.check_live(),
            _ => anyhow::bail!("guarded browser session is not authorized"),
        }
    }
    pub async fn authorize(&self, deadline: Instant) -> Result<()> {
        let pending = {
            let mut phase = self.phase.lock().expect("session lock");
            if let Phase::Active(lease) = &mut *phase {
                return lease.check_live();
            }
            match std::mem::replace(&mut *phase, Phase::Authorizing) {
                Phase::Pending(pending) => pending,
                other => {
                    *phase = other;
                    anyhow::bail!("guarded browser launch already attempted")
                }
            }
        };
        let mut cancelled = self.cancelled.subscribe();
        let lease = tokio::select! {
            biased;
            _ = cancelled.wait_for(|value| *value) => anyhow::bail!("browser authorization cancelled"),
            lease = pending.authorize_before(deadline) => lease?,
        };
        let mut phase = self.phase.lock().expect("session lock");
        ensure!(
            matches!(*phase, Phase::Authorizing) && !*cancelled.borrow(),
            "browser authorization closed before activation"
        );
        *phase = Phase::Active(lease);
        Ok(())
    }
    pub fn verify_profile(&self, profile: &Path) -> Result<()> {
        let mut phase = self.phase.lock().expect("session lock");
        let Phase::Active(lease) = &mut *phase else {
            anyhow::bail!("guarded browser not authorized")
        };
        lease.check_live()?;
        ensure!(
            lease.receipt().profile == profile.canonicalize()?,
            "CDP profile differs from authorized launch"
        );
        Ok(())
    }
    pub async fn close(&self) -> Result<()> {
        self.cancelled.send_replace(true);
        let phase = std::mem::replace(
            &mut *self.phase.lock().expect("session lock"),
            Phase::Closed,
        );
        match phase {
            Phase::Active(lease) => {
                let _root_only = lease.close().await?;
                anyhow::bail!("owned root reaped; complete browser tree remains unverified")
            }
            Phase::Pending(pending) => {
                drop(pending);
                Ok(())
            }
            // A concurrent authorization may already have crossed its durable
            // approval barrier. Cancel it, never restore Active, and do not
            // assert no launch while that future can still be unwinding.
            Phase::Authorizing | Phase::ClosingUnverified => {
                *self.phase.lock().expect("session lock") = Phase::ClosingUnverified;
                anyhow::bail!("browser authorization cancellation remains unverified")
            }
            Phase::Closed => {
                ensure!(
                    launch_coordinator::read_receipt(&self.root)?.is_none(),
                    "prior launch cleanup remains unverified"
                );
                Ok(())
            }
        }
    }
}
/// A root/group-only receipt is not sufficient to resume after a previous owner.
/// Apply this even in a default build so toggling the candidate off cannot
/// bypass a recorded uncertainty. Read-only job recovery remains available.
pub fn ensure_reconciled(root: &Path) -> Result<()> {
    if launch_coordinator::read_receipt(root)?.is_some() {
        return Err(crate::model::StartError {
            code: "browser_cleanup_unverified".into(),
            may_have_submitted: false,
            url: None,
        }
        .into());
    }
    Ok(())
}
/// Do not infer capability from a filename, env support, or upstream version.
/// There is no published, pinned artifact pair implementing this contract yet.
#[cfg(feature = "owned-launch-candidate")]
pub fn configure_supported_release(_root: &Path, _config: &mut config::Config) -> Result<()> {
    Err(crate::model::StartError {
        code: "guarded_helper_capability_unavailable".into(),
        may_have_submitted: false,
        url: None,
    }
    .into())
}
