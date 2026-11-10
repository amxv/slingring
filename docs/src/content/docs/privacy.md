---
title: Privacy and security
description: What is sent, what is saved, and who can send messages.
order: 24
category: Reference
---

## Data flow

Slingring sends the requested prompt or follow-up message and any local new-task CLI attachments to ChatGPT using your chosen Chrome login. Remote requests, including follow-up target URLs and messages, also pass through your relay. It detects whether fresh assistant activity is present but does not return, log, or save generated answer text.

Chat URLs, including follow-up targets, timestamps, status, phase, elapsed time, and request fingerprints are stored in a bounded local journal. Prompts, message text, and image contents are not persisted in that journal. A separate private, rotated `events.log` stores only validated request IDs, static phase/status codes and timings; it never includes raw stdout/stderr, tokens, URLs, prompts, or generated answers. The relay forwards requests in memory; it does not keep a durable message queue. Messages sent to ChatGPT remain subject to ChatGPT's conversation storage settings.

## Local security

State directories use 0700 permissions and state/configuration files use 0600. These permissions do not encrypt data at rest. Protect your Mac login, backups, and Chrome profile.

The browser runs with an isolated temporary profile copy. Native Messaging registration is scoped to that owned browser data directory. Slingring does not take control of ordinary Chrome windows or replace the normal agent-browser CLI.

Task launches disable two unnecessary sources of app modification: Chrome's macOS app-bundle cloning feature, which hard-links its installed executable, and the isolated process's delayed GoogleUpdater scheduler, which can wake the global updater and try to update installed Chrome under Slingring's responsibility. Slingring's short-lived browser needs neither operation. These process-local flags do not change normal Chrome updates, updater launchd jobs, update policy or macOS signature checks. Installing or upgrading Slingring itself can still modify its own CLI and menu bar app.

Headless mode is the default. Opting into headed mode makes that same isolated automation browser visible for debugging, including the conversation and prompt on screen. It retains draft checks, profile ownership checks and compatibility flags. Changing the setting does not interrupt the current task.

## Remote access

Anyone with the client token or private MCP URL can start chats and send follow-ups to accessible chats when they know the conversation URL. Use separate high-entropy client and device credentials. Never publish a connection URL, commit credentials, or include them in support logs. Rotate exposed credentials in Cloudflare and reconfigure the Mac.

MCP `new_task` accepts prompt text, optional reasoning effort, and optional unique `request_id`. `send_message` accepts message text, an exact `https://chatgpt.com/c/ID` target, optional reasoning effort, and optional `request_id`; chat IDs are limited to 1..128 ASCII letters, digits, or hyphens, without queries or fragments. A third read-only `get_job_status(request_id)` returns metadata and possibly a conversation URL from the Mac's journal, but cannot start a browser or send any message. These IDs are not credentials; never embed a secret in them. Other destinations and routes are rejected. Remote callers cannot select image paths, browser profiles, or shell commands. Follow-ups leave existing effort untouched unless explicitly requested and never interrupt a response. The queue cannot prevent concurrent writes by a person or another client.

All tools advertise `readOnlyHint: true` and `openWorldHint: false` as requested metadata. The two write tools still send messages and create or modify conversations; the status tool does not. Hints are not authorization controls or a security boundary.

## Website compatibility

Browser automation can be detected and may stop working when the site changes. Challenge-test results are observations, not a guarantee of undetectability or account safety. Follow the terms and policies that apply to your account. When a login or challenge requires human attention, resolve it in your browser instead of blindly repeating submissions.
