#[cfg(not(target_os = "linux"))]
pub mod auto_update;
#[cfg(target_os = "linux")]
#[path = "auto_update_linux.rs"]
pub mod auto_update;
pub mod upgrade;
