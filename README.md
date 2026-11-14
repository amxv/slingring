# Slingring

**Delegate ChatGPT tasks from any agent.**

Slingring lets local and remote agents start tasks in your ChatGPT account and follow up in the same conversation. It opens an isolated, headless browser on your Mac or Linux computer, submits the prompt, and returns a ChatGPT link once the task starts. Your agent can use that link to steer the task later.

No OpenAI API key, answer scraping, or separate agent workspace. Slingring submits the task; ChatGPT does the work.

## Two ways to delegate

**CLI for local agents.** Claude Code and other local agents can use the `slingring` command:

```bash
slingring new-task "Draft a deployment plan" --reasoning-effort High
slingring send-message https://chatgpt.com/c/YOUR-CHAT-ID "Add a rollback checklist"
```

The CLI also supports prompt files, images, and JSON output. Use `slingring pause-task CHATGPT_URL` to stop a response without sending a message. Recover pending or uncertain operations with `slingring jobs --request-id ID --json`, never by repeating a write.

**MCP for remote agents.** Connect an always-on assistant or cloud agent to your private MCP URL. It can call `new_task`, `send_message`, `pause_task`, and `get_job_status`. Remote requests pass through a relay in your own Cloudflare account to your Slingring service.

Both interfaces use the same task queue and browser. Headless is the default; visible browser mode is available for debugging. Slingring returns a conversation URL, **not the generated answer**, and never automatically retries an uncertain submission.

## Install

Requires **Apple Silicon macOS** or **x86_64 Linux with glibc 2.39+** (for example Ubuntu 24.04), Google Chrome, and a ChatGPT account signed in to Chrome. Linux runs as a regular user with Chrome's sandbox enabled, not as root or in an unsupported container/CI environment.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The installer sets up the CLI, browser helper, and extension. macOS also includes a menu bar app: select **Configure → Chrome Profile** and **Configure → Subscription**. On Linux use `slingring profile list`, `slingring profile use "PROFILE"`, and `slingring config set subscription plus|pro`. Fresh installs use Plus with High reasoning; saved settings are preserved. Linux has no tray app, makes no login/service changes during installation, and supports an optional explicit systemd user service. No separate browser automation tools are required.

Local CLI use does not need a relay. To delegate from remote agents, follow the one-time [remote setup guide](https://slingring.ashray.xyz/docs/remote).

## Documentation

**[Website & full docs](https://slingring.ashray.xyz)** · [Quickstart](https://slingring.ashray.xyz/docs/quickstart) · [CLI & MCP reference](https://slingring.ashray.xyz/docs/command-reference) · [Remote setup](https://slingring.ashray.xyz/docs/remote) · [Prompts & images](https://slingring.ashray.xyz/docs/local-input)

[Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Apache-2.0 license](LICENSE)

Slingring is an independent, unofficial project. Not affiliated with or endorsed by OpenAI or Google.
