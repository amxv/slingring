# Slingring implementation contract

The user authorized building a private Rust-first macOS project in this repo from the Rust CLI template. Bootstrap completed: Rust 1.99.1, direct distribution, Swift menu bar scaffold. Browser must remain agent-browser-browse (the tested fork) and authenticated Profile 6, isolated from everyday Chrome. No homebox, Redis, or model-output extraction.

## Shared Rust types (core agent owns src/config.rs and src/model.rs)
Config: Clone + Serialize/Deserialize with public fields browser_binary:String (default agent-browser-browse), chrome_profile:String (default Profile 6), default_reasoning_effort:String (default Extra High), timeout_secs:u64 (default 180), relay_url:Option<String>, relay_token:Option<String>. Additional fields allowed as needed.
TaskRequest: Clone + Serialize/Deserialize with public fields prompt:String, reasoning_effort:Option<String>, image_paths:Vec<PathBuf> (serde default). No request id inside this type.
TaskResult: Clone + Serialize/Deserialize with public fields job_id:String, status:String (started/failed/uncertain), url:Option<String>, evidence:Option<String>, error:Option<String>. All result content is status metadata, never assistant text.
Normalize efforts to exactly Instant, Medium, High, Extra High, Pro. Blank request effort uses configured default. Accept conventional CLI aliases including med, xhigh, extra-high; never silently degrade an unavailable account tier.

## Browser module (lead owns all src/browser*.rs and extension/)
pub struct BrowserRunner;
impl BrowserRunner { pub fn new(config:Config, state_dir:PathBuf)->anyhow::Result<Self>; pub async fn run(&mut self, job_id:&str, request:&TaskRequest)->anyhow::Result<TaskResult>; pub async fn close(&mut self)->anyhow::Result<()>; }
Core will add mod browser but must NOT create or edit browser implementation. Lead is creating it concurrently. run starts a new chat, configures effort, uploads local-only images, verifies Chat (never Work), sends once, waits for real thinking/working/streaming evidence AND canonical /c/... URL, returns metadata. Stop button or loading dot alone NEVER means success. Browser kept warm only while queue has more tasks, then closed. A launch error can return Err; submitted ambiguity returns TaskResult status uncertain. All script/extension assets embedded in binary, extracted privately for loading. Prototype may use direct CDP only for extension installation in the owned headless session; fork's normal page handling and Runtime.disable remain unchanged.

## CLI contract
slingring new-task [PROMPT] [--prompt-file PATH|-] [--reasoning-effort VALUE] [--image PATH repeated] [--request-id ID] [--json]
slingring config show [--json]
slingring config set KEY VALUE (at least default_reasoning_effort, chrome_profile, browser_binary, timeout_secs)
slingring status [--json] (no daemon spawn just for status)
slingring jobs [--json]
slingring service start|stop|restart|install|uninstall
slingring daemon [--ephemeral] (internal run mode; foreground possible for debugging)
slingring remote configure --url wss://.../bridge --token-stdin (secret read from stdin, never argv)
slingring remote disable
retain template upgrade and auto-update commands.
Data root: SLINGRING_HOME override, else ~/Library/Application Support/Slingring. Config/root/ledger private (0700 dirs,0600 files). Local UDS <root>/slingring.sock. Config stores secrets privately and config/status output MUST redact relay_token. CLI new-task connects to daemon if running; otherwise starts ephemeral daemon which exits after queue drains and browser closes (small startup/idle grace okay). Remote-enabled installed launchd service keeps tiny Rust daemon with one outbound WebSocket; no Chrome/node runtime idle. One serial bounded queue for both transports, durable atomic per-job records, no automatic resubmission after submitted/uncertain or daemon crash. Duplicate request-id is idempotent (different payload is rejected); successful ledger record need not retain prompt or image paths.

## Relay protocol (core and relay agents share)
Cloudflare Worker/DO is ONLY authenticated transport, not MCP policy or browser logic. Rust owns MCP semantics.
Mac connects outbound wss://<relay>/bridge, Authorization: Bearer <DEVICE_TOKEN>.
Worker POST /mcp, Authorization: Bearer <CLIENT_TOKEN> (also ?key=<CLIENT_TOKEN> for clients without header configuration). Validate content type, body size, Origin, Accept. GET /mcp returns405; notifications return202 after delivery; normal POST returns application/json JSON-RPC reply. Cache-Control:no-store. Reject requests when Mac offline with503. Never log URLs containing tokens or bodies. One authenticated Mac socket only; DO hibernation API and optional automatic ping/pong, no poll loop or permanent interval. Worker cap 32 active pending RPCs, per-call deadline 240s, reject overload and unknown messages. Disconnection fails pending callers with explicit uncertain error; never blindly retry new_task.
Worker->Mac {"type":"rpc","id":"uuid","request":<JSON-RPC object>,"idempotency_key":<optional header value>}
Mac->Worker {"type":"rpc_result","id":"same uuid","response":<JSON-RPC object or null for notification>}
Optional app heartbeat ping/pong strings; daemon one low-frequency health heartbeat (60s) allowed, not task polling. JSON max512KiB; tools/new_task max128KiB prompt. Tasks validate paths only through local CLI. Reject ANY remote arguments other than prompt/reasoning_effort, with additionalProperties:false in tools/list. One tool new_task. MCP 2025-11-25 initialize, tools/list, tools/call, ping, notifications/initialized supported; don't claim capabilities not implemented. Tool write semantics truthful, readOnlyHint:false, destructiveHint:false. No return of generated text.

## Menubar (docs/app agent owns apps/menubar only)
Native AppKit, no browser/WebView, no timer/status polling. Refresh by invoking CLI status/config when menu opens and on explicit actions. All config, remote state, startup/update actions are CLI calls (never duplicate state or directly edit config). All five reasoning default choices. Show service/remote and latest job metadata. Quit menu bar doesn't implicitly stop daemon. Retain update/autoupdate, launch at login. CLI is source of truth.

## Ownership
Core agent: src except browser*.rs; Cargo.toml/Cargo.lock; tests Rust. Relay agent: relay/ only. Docs/app agent: docs/ contents, README.md, CONTRIBUTORS.md, apps/menubar/, scripts/packaging if required (not bootstrap). Lead: extension/, src/browser*.rs, deployment, integration, fixes. Do not reset/revert others' changes. Do not commit/push/tag/deploy/install services. No agent may visit ChatGPT; lead owns browser sessions and all live submissions. Keep every source file <1000 lines. Write final handoff to tmp/gg/<task>-result.md.
