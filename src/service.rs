//! OS-specific background service manager.
#[cfg(target_os = "linux")]
pub use crate::service_linux::*;
#[cfg(target_os = "macos")]
pub use crate::service_macos::*;
#[cfg(windows)]
pub use crate::service_windows::*;
