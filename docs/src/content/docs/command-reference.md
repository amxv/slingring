---
title: CLI and MCP reference
description: Complete command syntax, tool arguments, responses, limits, and recovery behavior.
order: 20
category: Reference
---

Slingring has two ways to submit messages to ChatGPT: a macOS/Linux command-line interface and four MCP tools for local or remote agents. Both use the same configured Chrome account and serial task queue.

**A successful submission means ChatGPT has started thinking, working, or streaming a response.** Slingring returns the conversation URL, not the answer, and does not wait for generation to finish.

## CLI command map

```text
slingring setup [--profile NAME] [--default-effort LEVEL]
slingring profile list [--json]
slingring profile use NAME
slingring browser install|status

slingring new-task [PROMPT] [--prompt-file FILE | --stdin]
  [--reasoning-effort LEVEL] [--image PATH]... [--request-id ID] [--json]
slingring send-message CHATGPT_URL [MESSAGE] [--message-file FILE | --stdin]
  [--reasoning-effort LEVEL] [--image PATH]... [--request-id ID] [--json]

slingring pause-task CHATGPT_URL [--json]

slingring config show [--json]
slingring config set KEY VALUE
slingring status [--json]
slingring jobs [--request-id ID] [--json]

slingring service start|stop|restart|install|uninstall|status
slingring login menu-bar on|off|status
slingring login service on|off|status
slingring login status
slingring remote configure --url WSS_URL --token-stdin [--mcp-url URL]
slingring remote on|off|status|url
slingring mcp
slingring daemon [--ephemeral]

slingring upgrade [--check]
slingring auto-update on|off|status
```

Run `slingring --help` or `slingring <command> --help` for the installed command's usage. Supported aliases are `new_task` for `new-task`, `send_message` for `send-message`, `queue` for `jobs`, and `config get` for `config show`.

## Submit messages from the CLI

### Start a new conversation

Pass a prompt as an argument, read one from a file, or pipe it through standard input.

```bash
# Short prompt
slingring new-task "Review this architecture and identify the main risks"

# Longer brief with an explicit reasoning level
slingring new-task --prompt-file ./architecture.md \
  --reasoning-effort High \
  --request-id architecture-review-001 \
  --json

# Piped input
cat architecture.md | slingring new-task --stdin
```

Use `--prompt-file -` to read standard input as an alternative to `--stdin`. Only one prompt source can be supplied.

To attach images, repeat `--image` with a local path:

```bash
slingring new-task "Compare these two designs" \
  --image ./design-a.png \
  --image ./design-b.png
```

New conversations are created in **Chat**, not Work. When `--reasoning-effort` is omitted or blank, Slingring uses the configured default, initially `High` on a fresh Plus configuration. Plus allows `Instant`, `Medium`, and `High`; Pro also allows `Extra High` and `Pro`. Saved defaults are preserved. Set your subscription tier before requesting a Pro-only level.

### Follow up in an existing conversation

`send-message` targets an existing conversation directly. The URL must be exactly `https://chatgpt.com/c/ID`, where `ID` contains 1 to 128 ASCII letters, digits, or hyphens. Other hosts, trailing slashes, query strings, fragments, and Work, share, project, or custom GPT routes are not accepted.

```bash
CHAT_URL="https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef"

# Plain-text follow-up
slingring send-message "$CHAT_URL" "Compare the two strongest options"

# File plus a local attachment
slingring send-message "$CHAT_URL" \
  --message-file ./follow-up.md \
  --image ./diagram.png \
  --request-id architecture-followup-001 \
  --json

# Piped follow-up
cat follow-up.md | slingring send-message "$CHAT_URL" --stdin
```

`--message-file -` also reads standard input. As with new tasks, provide exactly one message source.

Omitting `--reasoning-effort` on a follow-up **leaves the existing chat's reasoning setting alone**. An explicit level outside the configured subscription tier is rejected. A tier-allowed explicit level requests a best-effort change; if the model picker is unavailable, Slingring may still send the message without verifying that change.

Before sending, Slingring waits for the conversation to become ready (voice control visible, no active stop control), protects existing drafts, and checks that the target URL has not changed. It does not interrupt a response or automatically retry an uncertain send.

### Submission options

| Option | Applicable commands | Behavior |
| --- | --- | --- |
| `--prompt-file FILE` / `--message-file FILE` | `new-task` / `send-message` | Read UTF-8 text from a file; `-` means stdin. |
| `--stdin` | Both | Read UTF-8 text from stdin instead of a positional argument. |
| `--reasoning-effort LEVEL` | Both | Override the new chat's default or request a best-effort follow-up change. |
| `--image PATH` | Both | Attach one local image; repeat for multiple images. Not available through MCP. |
| `--request-id ID` | Both | Assign a stable identifier to this logical submission for later status recovery. |
| `--json` | Both | Print the confirmed result as JSON instead of only the conversation URL. |

CLI submission commands print a recovery request ID to stderr early and wait for a confirmed start or an error; they do not return the MCP's five-second pending receipt. By default, a successful plain-text command prints only the conversation URL.

A representative `--json` result:

```json
{
  "job_id": "architecture-review-001",
  "url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef",
  "reasoning_effort": "High",
  "requested_effort": "High",
  "verified_effort": "High",
  "effort_verification": "verified",
  "evidence": "thinking"
}
```

`evidence` is one of `thinking`, `working`, or `streaming`. It confirms new response activity, **not** completion. For a follow-up the URL is the existing conversation's URL, and `reasoning_effort` may be `null`.

The structured `requested_effort`, `verified_effort`, and `effort_verification` fields distinguish requested settings from observed selections. Do not treat a requested value as proof of verification. Legacy records without verification metadata remain unverified.

### Pause an existing response

```bash
slingring pause-task https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef --json
```

Pause uses the same exact conversation URL rule and serial queue. It never sends a new message. A confirmed result reports `pause_status: stopped` if Slingring clicked Stop, or `pause_status: already_idle` if it observed an idle turn. Evidence must remain stable: either latest-turn completion controls (`evidence: completion_controls`), or the latest turn’s dedicated exact “Stopped thinking” UI status with adjacent Dictate/Start Voice controls and no Stop/active signals (`evidence: stopped_thinking`). Missing Stop or voice controls alone cannot prove completion. Stopping during thinking without either positive indicator remains uncertain, even when no response or footer appears. No generated answer is retrieved.

The CLI waits for the result and prints its recovery ID to stderr early. Recover through `jobs --request-id ID --json`; do not repeat `pause-task` to poll. The pause command does not accept `--request-id`.

## Configure and operate Slingring

### Select a Chrome profile

```bash
slingring profile list
slingring profile list --json

slingring setup --profile "Profile 6" --default-effort High
slingring profile use "Profile 6"
```

`profile use` accepts an exact Chrome profile directory or an unambiguous display name. It refuses to switch accounts while tasks are active. `setup` configures the selected account and installs the companion extension assets; if no profile is specified for a fresh setup, automatic selection requires exactly one available profile.

The browser helper is a separately managed, pinned and verified installation. Inspect or reinstall it without touching a separately installed browser CLI:

```bash
slingring browser status
slingring browser install
```

### Settings

```bash
slingring config show --json
slingring config set subscription plus
slingring config set default-effort High
slingring config set browser-mode headed
slingring config set browser-mode headless
slingring config set timeout-secs 180
```

| `config set` key | Values and meaning |
| --- | --- |
| `subscription` | `plus` (fresh default) or `pro`; match your account. |
| `default-effort` | Plus: `Instant`, `Medium`, `High`. Pro also allows `Extra High`, `Pro`. `site` or `default` clears the override. |
| `profile` | Selected Chrome profile directory. Prefer `profile use` for validated selection. |
| `browser-binary` | Browser helper executable; an explicit absolute override takes precedence over the managed helper. |
| `browser-mode` | `headless` (default) or `headed` (visible browser for debugging). |
| `browser-headed` | Equivalent boolean setting: `true` or `false`. |
| `timeout-secs` | Task-start budget, 1 to 3,600 seconds; default 180. `start-timeout-secs` is an alias. |
| `heartbeat-secs` | Remote connection heartbeat, 30 to 300 seconds; default 60. |

The menu wraps these settings through **Configure → Subscription** and tier-filtered **Default Reasoning** choices. Saved defaults are preserved; legacy Extra High/Pro defaults infer Pro when no tier was saved. The private `subscription.json` sidecar preserves compatibility with older installed services, which do not gain new controls until upgraded.

`default-reasoning-effort` and `chrome-profile` are also accepted key aliases; underscores may be used in place of hyphens.

Headed and headless mode use the same isolated browser profile and submission safeguards. A mode change affects future queue admissions, not an active task's mode. A visible browser is useful for diagnosis but does not guarantee that site challenges will disappear.

Slingring keeps its private state under `~/.config/slingring`. Set `SLINGRING_HOME` to an **absolute** directory to use a different state location. `config show` redacts remote credentials.

### Status, jobs, and service lifecycle

```bash
slingring status --json
slingring jobs --json
slingring jobs --request-id architecture-review-001 --json

slingring service install
slingring service start
slingring service status
slingring service restart
slingring service stop
slingring service uninstall
```

`status`, `jobs`, and `config show` always emit JSON. Their `--json` flags are retained as compatibility flags and do not change the format.

`status` distinguishes active/queued work from historical `latest_finished_job` and `latest_started_job` records. A historical `started` status confirms that ChatGPT began responding; it does not mean Slingring is still running that task. If another process owns the queue but cannot answer, `status_available: false` warns that saved history is not a live queue snapshot.

`jobs` reads the bounded, prompt-free journal. Use `--request-id` to get a single request's recovery outcome without opening a browser or submitting anything. `service install`/`uninstall` are low-level LaunchAgent lifecycle commands; `start`, `stop`, and `restart` control the daemon. Advanced use: `slingring daemon` runs the service in the foreground, and `--ephemeral` permits idle exit.

### Startup at login

On a fresh installation, **Open Menu Bar at Login** and **Start Slingring Service at Login** are both enabled. Configure shows two independent checkboxes, each backed by the following CLI operations:

```bash
slingring login status                 # JSON with the actual macOS states of both options
slingring login menu-bar status       # only the visible Slingring.app
slingring login service status        # only the background launchd service
slingring login menu-bar off          # do not open the menu bar after login
slingring login service off           # do not start the service at the next login
slingring login menu-bar on
slingring login service on
```

On Linux, `service install` explicitly registers a systemd user unit without enabling or starting it. `login service on|off` changes enablement without starting or stopping active work. A user manager is required for those controls; ordinary direct daemons and local MCP do not require systemd. Only the default state directory is eligible for a registered unit. Menu-bar status is `unsupported`, and menu-bar on/off refuse without changes. Linux has no supported automatic update timer; use `upgrade` manually.

On macOS, the menu bar command uses `SMAppService` to register the installed `Slingring.app` (which must be next to the CLI). The service command uses the existing `xyz.ashray.slingring` LaunchAgent and macOS launchd's persistent enabled/disabled override. **Turning service autostart off does not stop a running daemon, remove the plist, or interrupt current tasks.** Use `slingring service stop` separately for that. Repeating on/off is safe. Status reflects macOS state rather than a saved checkbox value; a `requires_approval` menu bar state needs approval in System Settings → General → Login Items & Extensions.

If launchd has just restarted or Slingring has just upgraded, `service start` may wait for macOS's 30-second restart throttle before declaring failure. The CLI reports a service startup problem only after a bounded readiness check and includes limited launchd diagnostics when available. This waiting does not submit a new ChatGPT task or restart a running service.

The installer initializes both settings for fresh installs. Upgrades retain explicit on/off choices, including old installations where the user removed the service LaunchAgent before these commands existed. The private `login-preferences.json` saves opt-outs to protect them from later upgrades, while macOS remains the source of truth for actual enablement. If you previously added Slingring manually in macOS Login Items, remove the obsolete manual entry if macOS lists two entries for Slingring.

Local-only use does not need an always-running daemon. The owned browser closes when the queue drains; remote mode maintains an outbound connection while enabled.

### Remote access and updates

Remote MCP requires a relay in your own Cloudflare account. After following the [relay setup guide](/docs/reference/relay), the configuration and lifecycle commands are:

```bash
# DEVICE_TOKEN and PRIVATE_MCP_URL come from your private relay setup.
printf '%s' "$DEVICE_TOKEN" | slingring remote configure \
  --url wss://relay.example.com/device \
  --token-stdin \
  --mcp-url "$PRIVATE_MCP_URL"

slingring remote on
slingring remote status
slingring remote url | pbcopy
slingring remote off
```

`remote configure` reads the token from stdin (or accepts a JSON credential object there). `remote url` prints a **secret-bearing** MCP URL; do not publish it or include it in logs. Your Mac must be awake, online, and logged into the selected ChatGPT profile for remote requests to work. See [Connect a remote assistant](/docs/remote) for client setup.

```bash
slingring upgrade --check
slingring upgrade

slingring auto-update status
slingring auto-update on
slingring auto-update off
```

`upgrade --check` checks without installing. `upgrade` installs the latest available release, while `auto-update` manages the independently scheduled daily update setting.

## MCP tools

Start the local **stdio** MCP server with `slingring mcp`, or connect a remote MCP client to the private URL from `slingring remote url`. The local server uses the official Rust MCP SDK and requires a standard initialization handshake. Both surfaces expose the same four text-only tools.

| Tool | Required arguments | Optional arguments | Effect |
| --- | --- | --- | --- |
| `new_task` | `prompt` | `reasoning_effort`, `request_id` | Submit a new ChatGPT Chat conversation. |
| `send_message` | `chatgpt_url`, `message` | `reasoning_effort`, `request_id` | Submit a follow-up to an existing conversation. |
| `pause_task` | `chatgpt_url` | None | Stop an existing response without sending a message. |
| `get_job_status` | `request_id` | None | Read recorded status; never send or retry. |

All tools reject unrecognized arguments. MCP tools do not accept image paths, image bytes, or local file attachments. Use the CLI for image inputs.

### `new_task`

```json
{
  "name": "new_task",
  "arguments": {
    "prompt": "Review the design and identify the three biggest risks.",
    "reasoning_effort": "High",
    "request_id": "design-review-001"
  }
}
```

`prompt` must contain non-whitespace text (up to 128 KiB). Live `reasoning_effort` enums reflect the configured subscription: Plus exposes `Instant`, `Medium`, and `High`; Pro additionally exposes `Extra High` and `Pro`. An empty string is also accepted. Explicit tier-unavailable values are rejected. Omitted or empty effort uses the Mac's configured default.

### `send_message`

```json
{
  "name": "send_message",
  "arguments": {
    "chatgpt_url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef",
    "message": "Revisit the recommendation using the revised constraints.",
    "request_id": "design-review-followup-001"
  }
}
```

`chatgpt_url` follows the exact conversation URL rule described above. `message` is nonblank and limited to 128 KiB. Omitting `reasoning_effort` leaves the chat's current setting untouched; supplying it asks for a best-effort change. This tool opens the exact conversation directly without navigating through the homepage.

### `pause_task`

```json
{
  "name": "pause_task",
  "arguments": {
    "chatgpt_url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef"
  }
}
```

Exactly one argument is accepted: `chatgpt_url`. Do not add `request_id`, message text, or other fields. The URL must satisfy the same canonical conversation rule. The tool shares the serial queue and does not submit text or retrieve generated content. Confirmed pauses use `pause_status: stopped` after clicking Stop or `already_idle` when observing an idle turn, with stable `completion_controls` or `stopped_thinking` evidence as described above; ambiguous UI returns uncertainty.

If still pending after the five-second post-admission result window, the receipt supplies a generated `request_id`. Query `get_job_status` with that ID. Never repeat the pause call to poll, even if a caller timed out before receiving its recovery ID.

### `get_job_status`

```json
{
  "name": "get_job_status",
  "arguments": {
    "request_id": "design-review-001"
  }
}
```

This lookup reads only locally recorded metadata. It does not start a browser, resubmit the message, or extend the job's lifetime. The `request_id` is required and must be 1 to 128 ASCII characters from `A-Z`, `a-z`, `0-9`, `_`, `-`, or `:`. The same rule applies to CLI IDs, submission-tool IDs, and remote idempotency headers.

## Results and safe recovery

### Confirmed result or pending receipt

For MCP write calls, Slingring waits **up to five seconds after queue admission**. Pause confirmation is described above. For new-task and follow-up submissions, if new ChatGPT activity has been confirmed, the result includes a URL and structured fields such as `job_id`, `request_id`, `accepted: true`, `outcome: submitted`, `reasoning_effort`, and `evidence`.

If the job is still starting, the call instead returns a non-error **accepted receipt**. Representative fields (abbreviated):

```json
{
  "request_id": "design-review-001",
  "job_id": "design-review-001",
  "accepted": true,
  "outcome": "in_progress",
  "recovery_tool": "get_job_status",
  "job": {
    "status": "starting",
    "phase": "browser_launch"
  }
}
```

**Accepted does not mean submitted.** The task keeps running in memory after the MCP call returns or the caller disconnects. Inspect it with `get_job_status` using the original request ID.

The status lookup returns an `outcome` and, when recorded, job metadata, a phase, elapsed time, and diagnostic guidance. For message submissions, these are the possible recovery outcomes. For pauses, inspect the operation and `pause_status`/`evidence` metadata; a pause never sends a message:

| Outcome | Meaning | What to do |
| --- | --- | --- |
| `in_progress` | Queued or starting. | Query the same ID later. |
| `submitted` | New ChatGPT activity was confirmed. | Open the recorded URL. |
| `not_submitted` | Definitively failed before sending. | Diagnose before deliberately creating a new request. |
| `uncertain` | Submission may have occurred. | Inspect the chat and journal. Never automatically retry. |
| `unknown` | No usable record, including a pruned one. | Treat as unverified, **not** as proof nothing was sent. |

A shortened example of a successful status lookup:

```json
{
  "request_id": "design-review-001",
  "known": true,
  "outcome": "submitted",
  "job": {
    "id": "design-review-001",
    "status": "started",
    "url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef"
  }
}
```

### Request IDs and idempotency

**Choose a unique `request_id` before each logical new-task or follow-up write and retain it.** Otherwise, a client timeout may leave you without a reliable lookup key. Pause has a one-argument MCP contract and generates its own recovery ID instead. IDs are bound to the operation, selected profile, and submitted message/attachment inputs. Reusing an ID with different inputs is rejected.

For raw remote HTTP clients, `Idempotency-Key` is the equivalent request header. If the header and the tool's `request_id` are both present, they must match. The relay may echo `X-Slingring-Request-Id` in a response, but a caller that disconnects before a response cannot depend on that header.

If a call times out or its result is missing, **query the same ID; never send the prompt again merely to check whether it worked**:

```bash
slingring jobs --request-id design-review-001 --json
```

Queued jobs left by a crash are classified as not submitted; interrupted starts are classified conservatively as uncertain. Slingring never replays either automatically. A failed or evicted journal lookup does not authorize another submission.

### Protocol and permissions

The local stdio server uses the official `rmcp` SDK; the remote relay supports MCP initialization, ping, tool discovery, and tool calls. Supported protocol versions are `2025-03-26`, `2025-06-18`, and `2025-11-25`. Remote Streamable HTTP returns JSON to POST requests; GET and DELETE are not implemented.

Tool metadata advertises `readOnlyHint: true` and `openWorldHint: false`. These are client-facing hints, **not permissions**. `new_task` and `send_message` still send messages using your ChatGPT account, and `pause_task` can stop a response. Treat MCP access as permission to create and modify conversations; `get_job_status` is the only read-only tool.

## Limits and operational guarantees

| Resource | Limit or behavior |
| --- | --- |
| Prompt / follow-up text | Nonblank UTF-8, up to **128 KiB** per message. |
| Remote JSON request | Up to **256 KiB**. |
| Concurrent submissions | One serial queue with **32** active or queued tasks total, across CLI and MCP. |
| CLI image attachments | Up to **16** images, **20 MiB** each, **100 MiB** combined; PNG, JPEG, WebP, and GIF. Files must have a matching supported signature. |
| Default browser-start budget | **180 seconds**, including follow-up readiness. Local queue waiting is excluded. |
| MCP receipt window | **5 seconds** after admission, then an accepted receipt if still in progress. |
| Remote deadlines | **230-second** Mac-side job deadline (including queue wait) and **240-second** relay HTTP safety limit; the browser budget may be shorter. |
| Conversation output | URL and submission or pause evidence only, **never generated answer text**. |

Jobs are serialized, but Slingring cannot lock a ChatGPT conversation against a human or another client editing it. A follow-up waiting for a busy conversation can delay later queued tasks. A client-side or network timeout does not cancel a job already accepted by MCP.

For diagnosis, inspect the payload-free, private lifecycle log at `~/.config/slingring/events.log` (and its optional rotated segment), alongside `slingring status --json` and `slingring jobs --json`. The log records bounded stages and timing categories, not prompts, answers, credentials, or raw browser output.

For related workflows, see [Send prompts and images](/docs/local-input), [Chrome profiles](/docs/profiles), [Remote assistants](/docs/remote), and [Troubleshooting](/docs/troubleshooting).
