# Slingring

**Give your agents a way into your ChatGPT.**

Send a prompt from a local or remote agent, get a ChatGPT conversation link back, and continue there. Useful for a second opinion on code, a deeper review of a plan, or handing over a brief without copying it between apps.

Slingring starts a new **Chat** or sends a follow-up to an existing Chat, never Work. It confirms a conversation URL only after ChatGPT begins thinking or responding to your message, and closes its temporary browser when the queue is empty. MCP calls can first return a pending receipt while the Mac prepares the task. It does not retrieve the answer.

> Browser-mode controls, CLI `send-message`, and MCP receipt/status recovery are available in Slingring v0.4.0 and later. Earlier versions, including v0.3.0, do not have these features.

## Get started

You need an Apple Silicon Mac, Google Chrome, and a ChatGPT login in Chrome. While this repository is private, release downloads also require an authenticated `gh` with repository access.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The installer includes the CLI, menu bar app, companion extension, and verified browser helper. **No separate agent-browser, Node, or Homebrew installation.**

Choose your signed-in account in **Slingring → Configure → Chrome Profile**, or use:

```bash
slingring profile list
slingring profile use "Profile 6"  # choose a directory or name from YOUR list
slingring new-task "Review this plan and identify its biggest risks" --reasoning-effort High
```

Open the returned link to read or continue the conversation. The CLI is installed at `~/.local/bin/slingring` if that directory is not already in PATH.

## Use it with local agents

Claude Code or another local agent can call the CLI directly. For a longer brief or images:

```bash
slingring new-task --prompt-file ./brief.md --reasoning-effort "Extra High" --json
slingring new-task "Compare these designs" --image first.png --image second.png
slingring send-message https://chatgpt.com/c/YOUR-CHAT-ID "Compare the revised image" --image revised.png
slingring send-message https://chatgpt.com/c/YOUR-CHAT-ID --message-file ./follow-up.md --image revised.png
```

Choose Instant, Medium, High, Extra High, or Pro when your account exposes it. Omitted or blank effort uses the configured default. Change it in Configure → Default Reasoning or with `slingring config set default-effort High`.

The automated browser is headless by default. To watch a task while debugging, choose **Configure → Browser Mode → Headed (Visible Browser)**, or run `slingring config set browser-mode headed`. The choice persists and applies to newly submitted local and remote tasks. Switch back with `slingring config set browser-mode headless`.

## Use it with remote agents

After the one-time [remote setup](https://slingring.ashray.xyz/docs/remote), copy your private MCP URL from Configure → Copy Remote MCP URL, or:

```bash
slingring remote url | pbcopy
```

Add it to your assistant as a remote MCP server. URL-key connections use **No authentication** in the client because the private URL itself is the credential. Keep it secret. Three text-only tools are available:

- `new_task(prompt, reasoning_effort?, request_id?)` starts a conversation.
- `send_message(chatgpt_url, message, request_id?)` follows up in an existing chat and preserves its reasoning effort.
- `get_job_status(request_id)` reads the Mac's journal without submitting anything, including when an earlier MCP caller timed out.

After accepting an MCP request, Slingring waits up to five seconds for a result. If the task is still queued or starting, it returns an `accepted: true`, `outcome: in_progress` receipt with the request ID. That receipt does not claim that a message was sent. Query `get_job_status` with the same ID to obtain the confirmed conversation link or failure.

**Choose and keep a unique `request_id` before calling**, so you can also recover if the client times out before receiving its receipt. Query status rather than sending again. `submitted` means a confirmed start, `not_submitted` means a definite pre-submit failure, and `uncertain` or `unknown` must never be blindly retried. See private `~/.config/slingring/events.log` for safe startup stage/timing diagnostics.

Follow-ups wait for the voice button and no stop control before filling the composer. They never interrupt a response. Use an exact conversation URL without a query or fragment; other routes are unsupported. All three tools are also available through local `slingring mcp`, implemented with the official Rust MCP SDK. Images are available for local `new-task` and `send-message` CLI commands only; remote and stdio MCP tool arguments remain text-only.

Remote access needs an awake, online Mac and your own Cloudflare relay. Local use needs neither a cloud service nor a background app. See [relay provisioning](https://slingring.ashray.xyz/docs/reference/relay) for that optional setup.

## Stays out of the way

Up to 32 requests share one serial queue. A follow-up waiting for a busy chat uses the existing startup timeout and can delay later tasks. The queue does not prevent a person or another client from writing to the same chat. The browser closes when the queue drains. Local-only mode leaves no Slingring process between tasks. Remote mode keeps one small push connection. The optional menu bar is a CLI wrapper, not another browser.

Open the menu to refresh the current startup phase, queued count, and last submission outcome. A confirmed submission is separate from a task still starting. Slingring stops tracking once new ChatGPT activity begins; it does not track whether the answer is finished.

If a submission is **uncertain**, inspect ChatGPT and `slingring jobs --request-id ID --json` before trying again. Slingring never automatically retries a possibly submitted prompt.

## Documentation

[Get started](https://slingring.ashray.xyz/docs/quickstart) · [Choose your account](https://slingring.ashray.xyz/docs/profiles) · [Prompts and images](https://slingring.ashray.xyz/docs/local-input) · [Remote assistants](https://slingring.ashray.xyz/docs/remote)

Technical details live in Reference: [commands and MCP](https://slingring.ashray.xyz/docs/command-reference), [architecture](https://slingring.ashray.xyz/docs/customize-cli), [installation and updates](https://slingring.ashray.xyz/docs/release-and-npm), [privacy](https://slingring.ashray.xyz/docs/privacy), and [development](https://slingring.ashray.xyz/docs/docs-site).

The site uses Astro/ZueDocs. Apache-2.0.
