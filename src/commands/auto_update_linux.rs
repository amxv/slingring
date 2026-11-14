//! The initial Linux release supports explicit in-place upgrades, not an
//! automatically installed scheduler. It never touches a user's systemd timers.
use crate::cli::AutoUpdateAction;
use anyhow::{Result, bail};

pub fn enabled() -> Result<bool> {
    Ok(false)
}
pub fn claim() -> Result<bool> {
    Ok(false)
}
pub fn run(action: AutoUpdateAction) -> Result<()> {
    match action {
        AutoUpdateAction::Status => {
            println!(
                "Automatic updates: unsupported on Linux; use slingring upgrade --check and slingring upgrade."
            );
            Ok(())
        }
        AutoUpdateAction::On | AutoUpdateAction::Off => {
            bail!(
                "Linux automatic update scheduling is not supported; no settings changed. Use slingring upgrade."
            )
        }
    }
}
