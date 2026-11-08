use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start one ChatGPT chat, return its URL once real activity begins.
    #[command(alias = "new_task")]
    NewTask {
        prompt: Option<String>,
        #[arg(long, conflicts_with_all = ["prompt", "stdin"])]
        prompt_file: Option<String>,
        #[arg(long, conflicts_with = "prompt")]
        stdin: bool,
        #[arg(long)]
        reasoning_effort: Option<String>,
        /// Attach a local image. Repeat for multiple images. CLI only.
        #[arg(long = "image")]
        images: Vec<std::path::PathBuf>,
        /// Reuse this ID only to recover the SAME request, never another task.
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Follow up in an existing ChatGPT Chat conversation, optionally attaching local images.
    #[command(alias = "send_message")]
    SendMessage {
        /// Exact https://chatgpt.com/c/ID conversation URL.
        chatgpt_url: String,
        message: Option<String>,
        #[arg(long, conflicts_with_all = ["message", "stdin"])]
        message_file: Option<String>,
        #[arg(long, conflicts_with = "message")]
        stdin: bool,
        /// Optional reasoning level. Omit to keep this chat's current setting.
        #[arg(long)]
        reasoning_effort: Option<String>,
        /// Attach a local image. Repeat for multiple images. CLI only.
        #[arg(long = "image")]
        images: Vec<std::path::PathBuf>,
        /// Reuse this ID only to recover the SAME message and attachments.
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Configure a Chrome profile and install the bundled extension assets.
    Setup {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        default_effort: Option<String>,
    },
    /// Choose the signed-in Chrome profile used for new tasks.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Install or inspect Slingring's managed browser helper.
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },
    /// Inspect or persist settings, including browser-mode headed|headless.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Read status without starting a daemon or browser.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Read the bounded task journal, without prompts or assistant output.
    #[command(alias = "queue")]
    Jobs {
        #[arg(long)]
        json: bool,
        /// Inspect a single recorded request without submitting anything.
        #[arg(long)]
        request_id: Option<String>,
    },
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Manage the menu bar and service login settings independently.
    Login {
        #[command(subcommand)]
        action: LoginAction,
    },
    /// Run the event-driven local service in the foreground.
    Daemon {
        #[arg(long)]
        ephemeral: bool,
    },
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },
    /// Optional local stdio MCP adapter (images remain CLI-only).
    Mcp,
    /// Internal Chrome Native Messaging endpoint. Never call from an agent.
    #[command(hide = true)]
    NativeHost {
        #[arg(long)]
        home: std::path::PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        browser_args: Vec<String>,
    },
    /// Enable, disable, or inspect independently scheduled daily upgrades.
    AutoUpdate {
        #[command(subcommand)]
        action: AutoUpdateAction,
    },
    /// Install the latest release, or check without installing.
    Upgrade {
        #[arg(long)]
        check: bool,
        #[arg(long, hide = true, conflicts_with = "check")]
        automatic: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AutoUpdateAction {
    On,
    Off,
    Status,
}
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    #[command(alias = "get")]
    Show {
        #[arg(long)]
        json: bool,
    },
    Set {
        /// Setting name, such as browser-mode or default-effort.
        key: String,
        value: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Install,
    Uninstall,
    Status,
}
#[derive(Debug, Subcommand)]
pub enum LoginAction {
    /// Register the installed Slingring.app with macOS Login Items.
    MenuBar {
        #[command(subcommand)]
        action: LoginToggleAction,
    },
    /// Control future launchd service startup without stopping the running daemon.
    Service {
        #[command(subcommand)]
        action: LoginToggleAction,
    },
    /// Inspect both settings and their actual macOS registration status.
    Status,
    /// Initialize fresh installs or migrate legacy installations; used by the installer.
    #[command(hide = true)]
    Setup {
        #[arg(long)]
        upgrade: bool,
    },
}
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum LoginToggleAction {
    On,
    Off,
    Status,
}
#[derive(Debug, Subcommand)]
pub enum ProfileAction {
    /// List Chrome profiles without opening Chrome.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Select a profile by its directory or unique display name.
    Use { profile: String },
}
#[derive(Debug, Subcommand)]
pub enum BrowserAction {
    /// Install the tested, checksummed fork. No Homebrew or Node required.
    Install,
    /// Print helper paths without launching Chrome.
    Status,
}
#[derive(Debug, Subcommand)]
pub enum RemoteAction {
    Configure {
        #[arg(long)]
        url: String,
        #[arg(long, alias = "device-token-stdin", required = true)]
        token_stdin: bool,
        #[arg(long)]
        mcp_url: Option<String>,
    },
    On,
    #[command(alias = "disable")]
    Off,
    Status,
    /// Print the secret-bearing MCP connection URL. Do not share publicly.
    Url,
}
