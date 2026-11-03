pub mod browser;
pub mod cli;
pub mod commands;
pub mod config;
pub mod core_cli;
pub mod daemon;
pub mod dependencies;
pub mod ipc;
pub mod mcp;
pub mod model;
pub mod native;
pub mod profiles;
pub mod queue;
pub mod relay;
pub mod service;
#[cfg(target_os = "linux")]
pub mod service_linux;
#[cfg(target_os = "macos")]
pub mod service_macos;
#[cfg(windows)]
pub mod service_windows;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

pub fn run() -> Result<()> {
    #[cfg(windows)]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg.to_string_lossy().starts_with("chrome-extension://"))
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        return native::serve_registered(&args);
    }
    run_with(Cli::parse())
}

pub fn run_with(cli: Cli) -> Result<()> {
    match cli.command {
        Command::NativeHost { home, browser_args } => native::serve(&home, &browser_args),
        Command::AutoUpdate { action } => commands::auto_update::run(action),
        Command::Upgrade { check, automatic } => commands::upgrade::run(check, automatic),
        command => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(core_cli::run(command)),
    }
}
