---
title: Privacy and security
description: What is sent, what is saved, and who can start a chat.
order: 24
category: Reference
---

## Data flow

Slingring sends the requested prompt and any local CLI attachments to ChatGPT using your chosen Chrome login. Remote prompts also pass through your relay. It detects whether assistant activity is present but does not return, log, or save generated answer text.

Chat URLs, timestamps, status, and request fingerprints are stored in a bounded local journal. Prompts and image contents are not persisted in that journal. The relay forwards requests in memory; it does not keep a durable prompt queue.

## Local security

State directories use 0700 permissions and state/configuration files use 0600. These permissions do not encrypt data at rest. Protect your Mac login, backups, and Chrome profile.

The browser runs with an isolated temporary profile copy. Native Messaging registration is scoped to that owned browser data directory. Slingring does not take control of ordinary Chrome windows or replace the normal agent-browser CLI.

## Remote access

Anyone with the client token or private MCP URL can send prompts through your account. Use separate high-entropy client and device credentials. Never publish a connection URL, commit credentials, or include them in support logs. Rotate exposed credentials in Cloudflare and reconfigure the Mac.

MCP accepts text and reasoning effort only. Remote callers cannot select image paths, browser profiles, destination URLs, or shell commands. The annotation hints do not change the fact that calling `new_task` creates a chat; they are not a security boundary.

## Website compatibility

Browser automation can be detected and may stop working when the site changes. Challenge-test results are observations, not a guarantee of undetectability or account safety. Follow the terms and policies that apply to your account. When a login or challenge requires human attention, resolve it in your browser instead of blindly repeating submissions.
