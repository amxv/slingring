pub mod browser;
pub mod browser_session;
pub mod cli;
pub mod commands;
pub mod config;
pub mod core_cli;
pub mod daemon;
pub mod dependencies;
pub mod diagnostics;
pub mod launch_coordinator;
pub mod login;
pub mod mcp;
pub mod mcp_sdk;
pub mod model;
pub mod native;
pub mod profiles;
pub mod queue;
pub mod relay;
pub mod service;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

pub fn run() -> Result<()> {
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
