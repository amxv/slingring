//! Platform-specific registration around the same private Unix daemon.
#[cfg(target_os = "linux")]
#[path = "service_linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "service_macos.rs"]
mod platform;

pub use platform::*;
