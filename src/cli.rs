use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"), after_help = "Examples:\n  slingring new-task 'Research this topic' --reasoning-effort high\n  slingring send-message https://chatgpt.com/c/ID 'Explore the second option'\n  slingring jobs --request-id REQUEST_ID --json\n\nSuccessful submissions return a conversation URL after activity begins, not a finished answer.\nReference: https://slingring.ashray.xyz/docs/command-reference")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start one ChatGPT chat, return its URL once real activity begins.
    #[command(
        alias = "new_task",
        after_help = "Examples:\n  slingring new-task 'Research this topic'\n  slingring new-task --prompt-file task.txt --reasoning-effort high\n\nOmitted effort uses the saved default (High on fresh installs). Plus supports Instant, Medium and High; Pro also supports Extra High and Pro."
    )]
    NewTask {
        /// Prompt text; use --prompt-file or --stdin for longer input.
        prompt: Option<String>,
        /// Read the prompt from FILE; use - for stdin. Choose exactly one input source.
        #[arg(long, conflicts_with_all = ["prompt", "stdin"])]
        prompt_file: Option<String>,
        /// Read the prompt from stdin instead of a positional prompt or file.
        #[arg(long, conflicts_with = "prompt")]
        stdin: bool,
        /// Instant, Medium, High, "Extra High" (xhigh), or Pro. Omit for the configured default.
        #[arg(long)]
        reasoning_effort: Option<String>,
        /// Attach a local image. Repeat for multiple images. CLI only.
        #[arg(long = "image")]
        images: Vec<std::path::PathBuf>,
        /// Fresh stable key (1..128 ASCII letters/digits/_/-/:). Recover with jobs --request-id ID; never resend to poll.
        #[arg(long)]
        request_id: Option<String>,
        /// Print structured submission metadata instead of URL-only stdout; recovery ID is on stderr.
        #[arg(long)]
        json: bool,
    },
    /// Follow up in an existing ChatGPT Chat conversation, optionally attaching local images.
    #[command(
        alias = "send_message",
        after_help = "Example:\n  slingring send-message https://chatgpt.com/c/ID 'Explore the second option'\n\nOmitting --reasoning-effort preserves the chat setting, regardless of your saved default. An explicit effort is best-effort; an unavailable picker does not block a ready follow-up."
    )]
    SendMessage {
        /// Exact https://chatgpt.com/c/ID conversation URL.
        chatgpt_url: String,
        /// Follow-up text. Choose exactly one positional, file, or stdin input.
        message: Option<String>,
        /// Read the message from FILE; use - for stdin.
        #[arg(long, conflicts_with_all = ["message", "stdin"])]
        message_file: Option<String>,
        /// Read the message from stdin instead of a positional message or file.
        #[arg(long, conflicts_with = "message")]
        stdin: bool,
        /// Instant, Medium, High, "Extra High" (xhigh), or Pro. Omit to preserve this chat; explicit changes are best-effort.
        #[arg(long)]
        reasoning_effort: Option<String>,
        /// Attach a local image. Repeat for multiple images. CLI only.
        #[arg(long = "image")]
        images: Vec<std::path::PathBuf>,
        /// Fresh stable key (1..128 ASCII letters/digits/_/-/:). Recover with jobs --request-id ID; never resend to poll.
        #[arg(long)]
        request_id: Option<String>,
        /// Print submission and effort-verification metadata; does not wait for a finished answer.
        #[arg(long)]
        json: bool,
    },
    /// Stop a running response in an exact ChatGPT conversation and verify the UI stopped.
    #[command(
        alias = "pause_task",
        after_help = "Example:\n  slingring pause-task https://chatgpt.com/c/ID\n\nUses only the isolated browser and never extracts generated answers. If the outcome is uncertain, inspect the conversation before retrying."
    )]
    PauseTask {
        /// Exact https://chatgpt.com/c/ID conversation URL.
        chatgpt_url: String,
        /// Print a JSON result.
        #[arg(long)]
        json: bool,
    },
    /// Configure a Chrome profile and install the bundled extension assets.
    Setup {
        /// Chrome profile directory or unique display name; inspect with profile list.
        #[arg(long)]
        profile: Option<String>,
        /// New-chat default: Instant, Medium, High, "Extra High" (xhigh), or Pro. Omit to preserve existing settings.
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
    /// Inspect or persist subscription, new-chat defaults and browser settings.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Read status as JSON without starting a daemon or browser.
    Status {
        /// Compatibility flag: output is always JSON.
        #[arg(long)]
        json: bool,
    },
    /// Read the bounded task journal as JSON, without prompts or assistant output.
    #[command(
        alias = "queue",
        after_help = "Example:\n  slingring jobs --request-id REQUEST_ID\n\nRead-only recovery: inspect uncertain submissions in ChatGPT before retrying. Unknown or evicted IDs do not prove nothing was sent. Never automatically replay a request."
    )]
    Jobs {
        /// Compatibility flag: output is always JSON.
        #[arg(long)]
        json: bool,
        /// Inspect a single recorded request without submitting anything.
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Control the local service process, independently of login settings.
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
        /// Exit after local work drains instead of keeping the service available.
        #[arg(long)]
        ephemeral: bool,
    },
    /// Configure or toggle the outbound remote MCP connection.
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
        /// Check release availability without installing or changing the service.
        #[arg(long)]
        check: bool,
        #[arg(long, hide = true, conflicts_with = "check")]
        automatic: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AutoUpdateAction {
    /// Enable the independently scheduled daily upgrade check.
    On,
    /// Disable the independently scheduled daily upgrade check.
    Off,
    /// Inspect the daily upgrade schedule and current registration.
    Status,
}
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print redacted configuration as JSON (also the default without --json).
    #[command(alias = "get")]
    Show {
        /// Compatibility flag: output is always JSON.
        #[arg(long)]
        json: bool,
    },
    /// Persist a setting. Subscription is your declared plan, not a billing change.
    #[command(
        after_help = "Examples:\n  slingring config set subscription pro\n  slingring config set default-effort high\n  slingring config set browser-mode headless\n\nPlus supports Instant, Medium and High. Pro also supports Extra High and Pro. Switching to Plus requires a compatible saved default first. Default effort applies only to new chats; site preserves ChatGPT's setting."
    )]
    Set {
        /// subscription, default-effort, browser-mode, profile, browser-binary, start-timeout-secs or heartbeat-secs.
        key: String,
        /// subscription: plus|pro; default-effort: a mode or site/default reset; browser-mode: headed|headless; timeouts: seconds.
        value: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum ServiceAction {
    /// Start the local process now; does not change future login startup preferences.
    Start,
    /// Stop the local process, not already-running ChatGPT responses. Use pause-task for those.
    Stop,
    /// Stop and start the local process now; does not replay interrupted jobs.
    Restart,
    /// Register the service (macOS LaunchAgent; optional Linux user unit without starting it).
    Install,
    /// Stop and remove this user's Slingring service registration.
    Uninstall,
    /// Print process/remote status as JSON without starting a browser.
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
        /// Print structured profile rows instead of the readable list.
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
    /// Save the relay endpoint and credentials without enabling remote access.
    Configure {
        /// HTTPS/WSS relay origin; Slingring uses its /device WebSocket endpoint.
        #[arg(long)]
        url: String,
        /// Read a raw device token or JSON {device_token,mcp_url} from stdin; never place tokens in arguments.
        #[arg(long, alias = "device-token-stdin", required = true)]
        token_stdin: bool,
        /// Secret-bearing MCP connection URL, used with raw-token stdin; JSON credentials carry their own mcp_url.
        #[arg(long)]
        mcp_url: Option<String>,
    },
    /// Enable outbound remote MCP and start the local service.
    On,
    /// Disable outbound remote MCP; the idle service exits when work drains.
    #[command(alias = "disable")]
    Off,
    /// Print remote/process status as JSON without launching a browser.
    Status,
    /// Print the secret-bearing MCP connection URL. Do not share publicly.
    Url,
}
