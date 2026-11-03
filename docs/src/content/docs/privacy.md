---
title: Privacy and security
description: Know what crosses each boundary.
order: 6
category: Reference
summary: Know what crosses each boundary.
---

## Data flow

Prompts and local attachments go to ChatGPT through the authenticated browser. Remote prompts also transit your relay. Generated assistant text is not extracted or returned. Chat URLs and job metadata can still reveal sensitive context; protect them.

The browser uses the tested fork and isolated authenticated Profile 6. Keep that session private. Browser automation is observable and may fail as interfaces change. Slingring makes no promise of undetectability, account safety, or compliance with a service's terms. Review the terms and policies that apply to your account and use.

## Credentials and local storage

Configuration, state, and ledger directories use private permissions (0700 directories, 0600 files). Configuration stores the relay device token locally; `config show` and status must redact it. Protect the Mac account, backups, browser profile, and relay secrets. Local file permissions do not encrypt data at rest.

Use separate device and client bearer tokens. The relay authenticates both sides, bounds messages and concurrent requests, and must not log request bodies or token-bearing URLs. Rotate compromised tokens in the deployment and reconfigure the Mac. HTTPS and WSS protect transport, not compromised endpoints.

## Remote boundaries

Remote callers can submit prompt text and effort only. They cannot select local image paths. This is a write operation that creates a chat, not a read-only search tool. Grant client credentials only to agents you trust to submit work. No automatic retry should follow an uncertain submission.
