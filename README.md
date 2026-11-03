# Slingring

**Give your agents a way into your ChatGPT.**

Send a prompt from a local or remote agent, get a ChatGPT conversation link back, and continue there. Useful for a second opinion on code, a deeper review of a plan, or handing over a brief without copying it between apps.

Slingring starts a new **Chat**, never Work. It waits until ChatGPT actually begins thinking or responding, returns the URL, and closes its temporary browser when the queue is empty. It does not retrieve the answer.

## Get started

The macOS app targets **Apple Silicon**. This branch also adds **Linux x86_64 and Windows x64 CLI/MCP** builds, with OS-specific background startup. Google Chrome and a ChatGPT login in a Chrome profile are required to run tasks. The Windows and Linux builds depend on matching `agent-browser-browse` platform release assets; until those assets ship, installations cannot complete. Private Slingring releases require authenticated `gh` access.

```bash
# macOS ARM64 or Linux x86_64 (after matching release assets exist)
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

On Windows x64, run this in PowerShell (after matching release assets exist):

```powershell
irm https://slingring.ashray.xyz/install.ps1 | iex
```

`Slingring.app` is currently **macOS-only**; Linux and Windows provide the CLI/MCP and optional background service, without a tray UI.

The installer includes the CLI, companion extension, and verified browser helper (plus the menu bar app on macOS). **No separate agent-browser, Node, or Homebrew installation.**

Choose your signed-in account in **Slingring → Configure → Chrome Profile** (macOS), or use:

```bash
slingring profile list
slingring profile use "Profile 6"  # choose a directory or name from YOUR list
slingring new-task "Review this plan and identify its biggest risks" --reasoning-effort High
```

Open the returned link to read or continue the conversation. On macOS/Linux the CLI installs to `~/.local/bin/slingring`; on Windows it installs to `%LOCALAPPDATA%\Programs\Slingring\slingring.exe`.

## Use it with local agents

Claude Code or another local agent can call the CLI directly. For a longer brief or images:

```bash
slingring new-task --prompt-file ./brief.md --reasoning-effort "Extra High" --json
slingring new-task "Compare these designs" --image first.png --image second.png
```

Choose Instant, Medium, High, Extra High, or Pro when your account exposes it. Omitted or blank effort uses the configured default. Change it in Configure → Default Reasoning or with `slingring config set default-effort High`.

## Use it with remote agents

After the one-time [remote setup](https://slingring.ashray.xyz/docs/remote), copy your private MCP URL from Configure → Copy Remote MCP URL, or:

```bash
slingring remote url | pbcopy
```

Add it to your assistant as a remote MCP server. URL-key connections use **No authentication** in the client because the private URL itself is the credential. Keep it secret. The single tool is `new_task(prompt, reasoning_effort?)`; image paths stay local-only.

Remote access needs an awake, online host and your own Cloudflare relay. Local use needs neither a cloud service nor a persistent background app. See [relay provisioning](https://slingring.ashray.xyz/docs/reference/relay) for that optional setup.

## Stays out of the way

Several requests can queue up, but chats start one at a time. The browser closes when the queue drains. Local-only mode leaves no Slingring process between tasks. Remote mode keeps one small push connection. The optional menu bar is a CLI wrapper, not another browser.

If a submission is **uncertain**, inspect ChatGPT and `slingring jobs --json` before trying again. Slingring never automatically retries a possibly submitted prompt.

## Documentation

[Get started](https://slingring.ashray.xyz/docs/quickstart) · [Choose your account](https://slingring.ashray.xyz/docs/profiles) · [Prompts and images](https://slingring.ashray.xyz/docs/local-input) · [Remote assistants](https://slingring.ashray.xyz/docs/remote)

Technical details live in Reference: [commands and MCP](https://slingring.ashray.xyz/docs/command-reference), [architecture](https://slingring.ashray.xyz/docs/customize-cli), [installation and updates](https://slingring.ashray.xyz/docs/release-and-npm), [privacy](https://slingring.ashray.xyz/docs/privacy), and [development](https://slingring.ashray.xyz/docs/docs-site).

The site uses Astro/ZueDocs. Apache-2.0.

### Cross-platform validation status

The browser fork uses four platform-agnostic CDP `Runtime.disable` patches and OS-specific stable Chrome discovery / headless user-agent normalization. On Ubuntu x86_64 with Chrome 152, its loopback navigation/snapshot/eval smoke passed; SannySoft webdriver checks passed and Rebrowser's `runtimeEnableLeak` and `navigatorWebdriver` were green. **The ScrapingCourse Cloudflare challenge did not clear on Linux.** NowSecure loaded, while G2 showed no usable content in the observed run. These are single-session results, not guarantees.

Slingring's Rust unit/CLI tests are separate from real ChatGPT submissions. Cross-platform native extension registration and authenticated task submission require independent verification on each OS before claiming parity. In particular, Windows uses transient per-user Chrome native-messaging registry registration, with task-scoped cleanup and local IPC authentication; unlike macOS, it cannot be kept solely in a copied Chrome profile.
