---
title: Changelog
description: Release status and user-visible changes.
order: 99
category: Reference
summary: Release status and user-visible changes.
---

## 0.7.0 · 14 November 2026

Add x86_64 Linux CLI, local/remote MCP, private Unix IPC, and explicit systemd user-service controls. Linux requires glibc 2.39 or newer and system-installed Google Chrome Stable with its sandbox enabled. Root, CI and container browser execution is refused instead of weakening browser protections. Linux has no menu/tray app or automatic update scheduler; installations do not change service or login settings.

Add a checksummed Linux installer and conservative in-place upgrades. Linux uses system Google Chrome Stable with an explicitly initialized dedicated profile, a private browser control pipe, and shutdown verification for the exact owned browser generation. It does not import regular Chrome profiles or use the browser fork. Existing macOS app, service behavior, signing support, and 0.41.1 helper pin are retained. macOS browser lifecycle issue #4 remains open.

Preserve 0.6.7's durable pre-Send authorization, positive Chat/effort readiness, private diagnostics, and no-replay uncertainty semantics on both platforms. No Windows or Linux ARM64 distribution is included.

## 0.6.7 · 14 November 2026

Retry transient composer hydration failures during new-chat preparation within a bounded pre-send budget, while preserving positive Chat-mode and reasoning-effort checks and existing-draft protection. Readiness diagnostics distinguish missing, hidden, disabled, ambiguous, and ready editors without recording their contents.

Persist submission intent before authorizing Send, and refuse to send if that durable save fails. Recovery distinguishes requests interrupted before authorization from uncertain submissions after authorization; interrupted legacy starting records remain conservative. No uncertain request is automatically replayed, and post-send recovery remains one read-only refresh of the exact conversation. Browser-process lifecycle and helper behavior are unchanged.

## 0.6.6 · 10 November 2026

Fix subscription-aware MCP tool descriptions adding an invalid, unused reasoning-effort field to `pause_task` and `get_job_status`. These tools now advertise exactly their supported conversation URL or request ID argument. Reasoning choices remain filtered by subscription only on `new_task` and `send_message`. Runtime argument validation was already strict; no messages or pause actions are replayed by this correction.

## 0.6.5 · 10 November 2026

Add Plus/Pro subscription selection in the CLI and menu bar, with tier-aware reasoning choices in local and remote MCP tools. Fresh installations default to Plus and High; existing defaults, including Extra High, are preserved. Subscription settings live separately so older installed services cannot overwrite them.

Add `slingring pause-task URL` (also `pause_task`) and the one-argument MCP `pause_task(chatgpt_url)` tool. Pauses use the existing serial queue and exact conversation URL, click Stop at most once, and confirm the outcome only from stable completion controls or the dedicated stopped-thinking indicator. Ambiguous outcomes remain uncertain. Pausing never sends a message or extracts generated answers; queued MCP pauses return a recovery ID promptly.

Unify request IDs across CLI, local MCP and remote calls: 1–128 ASCII letters, digits, underscores, hyphens or colons. The CLI prints recovery IDs to stderr before waiting, and help explains status lookup without repeating a write. Record requested and verified reasoning effort separately, with an explicit warning when a follow-up's requested effort cannot be verified. Omitted follow-up effort continues to leave the picker untouched.

## 0.6.4 · 10 November 2026

Stop Slingring's isolated Chrome process from starting its delayed GoogleUpdater scheduler. The scheduler could wake the global updater and attempt to replace installed Chrome through `rsync`, producing another macOS App Management notification attributed to Slingring. This is separate from the app-cloning operation disabled in 0.6.2.

Use Chromium's process-local `--disable-updater-scheduler` switch in both headless and headed task browsers. Normal Chrome launches, updater launchd jobs, update policy and code-signature checks remain unchanged. The blank-browser smoke test now observes startup for 30 seconds, beyond Chrome 156's 19-second scheduler delay, and can verify that no updater process was started by its owned Chrome PID.

## 0.6.3 · 10 November 2026

Fix public-certificate extraction when preserving a locally signed installation during upgrades. macOS requires the certificate output prefix to use the `--extract-certificates=PREFIX` form; the installer now uses that syntax instead of failing before replacement. Regression tests reject the invalid form, and an isolated macOS upgrade verified that changed CLI and menu bar executables retain the same signing certificate and designated requirements.

This maintenance release includes the installer correction already deployed on the website. It retains 0.6.2's browser app-cloning fix and does not change browser behavior or macOS trust and privacy settings.

## 0.6.2 · 10 November 2026

Stop the isolated task browser from cloning Chrome's app bundle and hard-linking its installed executable. This unnecessary Chrome update-survival feature caused macOS to report that **Slingring was blocked from modifying apps**. Task submissions no longer need App Management access for that operation. The change applies only to Slingring's short-lived browser; normal Chrome updates, code-signature validation, browser fingerprint defaults, and existing headless/headed preferences are preserved.

Add regression coverage for both browser modes and an opt-in, empty-profile browser smoke test that verifies the real launch flags and absence of app-clone/hard-link operations. Ad-hoc signing still cannot guarantee permission retention across binary changes; this patch removes the unnecessary app modification rather than resetting or bypassing macOS privacy consent.

Add explicit opt-in local certificate signing during installation. Later upgrades preserve an existing certificate-backed CLI and menu bar identity, and stop before replacement if the signer or designated requirement cannot be preserved. Private keys stay in the local Keychain; public release artifacts remain ad-hoc signed. No trust, key-access, or privacy settings are changed automatically.

## 0.6.1 · 8 November 2026

Fix an intermittent **service did not start** alert after an upgrade or macOS launchd restart. Service startup now allows for launchd's 30-second throttling window instead of falsely failing after five seconds, detects early failures of directly spawned daemons, and reports bounded, actionable launchd diagnostics. CLI service commands do not accidentally kickstart the normal user's LaunchAgent when using a separate `SLINGRING_HOME`, and a manually started service works independently of the login autostart preference.

Upgrades now wait for the old daemon process to **fully exit**, not just close its socket, before replacing the signed CLI. This avoids code-signing termination races during installation. Add regression coverage for isolated service lifecycles and delayed daemon shutdown during upgrades. The existing login toggles, menu bar design, browser isolation, and task submission behavior are unchanged.

## 0.6.0 · 8 November 2026

Add two independent, CLI-backed Configure controls: **Open Menu Bar at Login** and **Start Slingring Service at Login**. Both default to on for fresh installs. `slingring login menu-bar on|off|status`, `slingring login service on|off|status`, and `slingring login status` expose the same actual macOS states without opening the UI. The visible app uses macOS ServiceManagement login items, with registration/approval errors surfaced rather than silently pretending a toggle succeeded.

Disabling service autostart uses launchd's persistent override rather than unloading the running daemon, so current tasks are not interrupted. Installation and upgrades migrate older service registrations conservatively, preserve explicit opt-outs, and never silently reset startup choices during automatic updates. Add offline installer, Rust CLI, Swift menu-state, and isolated macOS startup registration coverage.

## 0.5.1 · 3 November 2026

Recover an exact “Could not load this ChatGPT conversation” error with at most two clicks of the **page-load** Try again control, stopping with a clear non-submission failure if it never exposes a composer. A red **message-send** Retry is never clicked and a submitted prompt is never replayed: when a send-error is seen, refresh the already-owned exact conversation once and check its visible new turn and response-start activity read-only. If evidence remains incomplete, return **uncertain**, not “not submitted.” Protect drafts, images, voice readiness, exact targets and existing Chat/Extra High defaults.

Throttle response-start DOM checks to reduce renderer jank, preserve the correct activity URL, and return once activity is verified without awaiting generation or cleanup. Disable unused experimental WebMCP, local device casting discovery and default app installation in the temporary browser (no LAN device permission is needed for Slingring). Assign the CLI a stable signing identifier. Apple Silicon macOS is now the **only** release target; Intel bundles and checks are removed. Browser permissions are still managed by macOS; these changes do not bypass Apple privacy protections or guarantee that the OS will retain an ad-hoc-signed app's consent after updates.

## 0.5.0 · 3 November 2026

Follow-ups now open the exact existing conversation URL directly rather than navigating through the ChatGPT homepage or switching Chat/Work mode first. Omitted follow-up `reasoning_effort` skips all model-picker reads, changes and blocks. An optional explicit level on CLI and MCP follow-ups attempts to select the desired reasoning effort before filling; an unavailable or rehydrating picker does not prevent submission. Follow-ups retain strict voice/stop readiness, target-URL binding, user-draft protection, one-shot submission and fresh-activity confirmation. Record only allowlisted requested/observed effort metadata when an explicit level was supplied, never model output or picker text. No new-chat selection behavior changes.

## 0.4.0 · 3 November 2026

Add read-only recovery with `request_id` and `get_job_status`, bounded private startup-stage/timing diagnostics, and accepted MCP receipts after a five-second result wait. Accepted jobs continue in memory after caller disconnects. Reserve status capacity when submissions are saturated, classify interrupted jobs conservatively before restart, and preserve confirmed outcomes without waiting for browser cleanup. A timeout or missing record never proves that a message was unsent, and interrupted work is never replayed.

Use the official `rmcp` Rust SDK for local stdio MCP protocol handling and transport, preserving the existing remote relay integration and strict text-only MCP boundary. Add `slingring send-message` with local image attachments, file/stdin message input, and request-ID recovery. Follow-up images use the existing isolated browser upload path, with readiness and unchanged-conversation checks before submission. No generated answers are extracted.

Preserve rich-text line breaks during prompt verification, reject existing or changed drafts, bind uploads to the exact owned browser target, and wait for uploads without confusing their progress with an active response. Require identifiable, stable attachment cards before sending; a layout with only Remove buttons is rejected rather than risking a changed sibling preview. Keep submission evidence separate from cleanup and report missing composers clearly.

Add persistent headless/headed selection through `config set browser-mode` and Configure → Browser Mode. Headless remains the default. Show current preparation phases and queued jobs separately from the last confirmed, failed, or uncertain attempt. Protect read-only crash recovery by holding the daemon and queue ownership locks across its journal snapshot; never claim a queued job was unsent when ownership cannot be verified.

## 0.3.0 · 3 November 2026

Add the MCP-only `send_message(chatgpt_url, message)` tool for continuing an existing ChatGPT Chat conversation using the same isolated browser, Chrome profile, and serial queue as `new_task`. A follow-up waits for the voice control to signal readiness, never interrupts an ongoing response, preserves the current reasoning effort, and returns the same conversation URL once new thinking, working, or response activity begins. Generated answers are not extracted.

Protect follow-ups from invalid or redirected URLs, existing drafts and attachments, Work mode, busy conversations, and misleading historical activity. A failed or uncertain submission is not automatically retried; follow-up messages remain limited to 128 KiB. Add regression coverage for queueing, duplicate protection, readiness, and real start detection.

## 0.2.0 · 3 November 2026

Install the verified browser helper automatically with Slingring. Add named Chrome profile selection in the CLI and menu bar. Move settings into Configure, retaining service control, latest chat, and quit at the top level. Advertise the requested MCP hint values while retaining explicit tool behavior.

Keep the browser's crash backstop longer than the configured task deadline without changing launch options as queued requests age. Retry a model-menu click that races page hydration, while retaining the one-shot submission guard and strict effort verification.

## 0.1.1 · 3 November 2026

Harden browser startup and reasoning-picker hydration, anchor activity detection to the active transcript, and reject loading-only states. Preserve a full per-task startup budget for locally queued requests, close a warm browser when abandoned queued work drains, and use smaller idle WebSocket buffers. Add regression fixtures, private-release installation support, and automatic relay-code deployment.

## 0.1.0 · 3 November 2026

Initial private macOS release. Local CLI and remote Streamable HTTP MCP share one serial queue and a patched-browser runner. The CLI supports local image attachments; the single remote tool accepts only prompt and reasoning effort.

The browser adapter verifies Chat mode, supports all five reasoning levels when the account exposes them, and returns the conversation URL only after real assistant activity. It handles the current retained home view, user-bubble, and assistant Markdown structure. Stop controls, pulsing dots, and screen-reader loading text do not count as a confirmed task start.

Remote mode uses a single outbound Rust WebSocket, a hibernating Cloudflare relay, sparse connection heartbeats, and no Redis or idle browser. Local-only mode exits when idle. The optional signed Swift menu bar wraps CLI commands and has no status polling timer.

The initial private installer supported authenticated GitHub downloads, paired CLI/app archives, checksum verification, and explicit update preferences. At the time, private release artifacts required repository access; no npm package was published.
