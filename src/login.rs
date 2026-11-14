//! CLI-owned login controls. Linux has no menu bar companion.
#[cfg(target_os = "macos")]
#[path = "login_macos.rs"]
mod platform;
#[cfg(target_os = "macos")]
pub use platform::run;

#[cfg(target_os = "linux")]
pub fn run(
    root: &std::path::Path,
    action: crate::cli::LoginAction,
) -> anyhow::Result<serde_json::Value> {
    use crate::{
        cli::{LoginAction, LoginToggleAction},
        service,
    };
    use serde_json::json;
    let unavailable = || json!({"enabled":false,"state":"unsupported","registered":false});
    let pair = || {
        let state = service::login_status(root).unwrap_or_else(|_| {
            json!({"enabled":null,"state":"unavailable","error":"systemd user service state unavailable"})
        });
        json!({"menu_bar":unavailable(),"service":state})
    };
    match action {
        LoginAction::MenuBar {
            action: LoginToggleAction::Status,
        } => Ok(unavailable()),
        LoginAction::MenuBar { .. } => {
            anyhow::bail!("the menu bar companion is available only on macOS")
        }
        LoginAction::Service { action } => {
            match action {
                LoginToggleAction::On => service::login_enable(root)?,
                LoginToggleAction::Off => service::login_disable(root)?,
                LoginToggleAction::Status => {}
            }
            service::login_status(root)
        }
        LoginAction::Status | LoginAction::Setup { .. } => Ok(pair()),
    }
}
