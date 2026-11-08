# Security policy

Slingring automates a signed-in ChatGPT browser session and can optionally
accept remote MCP requests. Treat remote MCP URLs, device credentials, browser
profiles and session data as sensitive.

## Report vulnerabilities privately

Please do **not** post exploit details, session data, credentials, remote MCP
URLs or personal conversation URLs in a public GitHub issue.

Use **Security → Advisories → Report a vulnerability** in this repository if
GitHub private vulnerability reporting is enabled. If that option is not
available, open an issue requesting a private contact channel **without**
including vulnerability details or any sensitive data.

Security fixes are prioritized for the latest supported release. Older
versions may not receive separate fixes; keep the CLI and companion app updated
together.

## Credential hygiene

- Never commit `.env` files, Cloudflare deployment credentials, browser
  profiles, ChatGPT cookies, local private configuration or remote capability
  URLs.
- If you accidentally expose a remote MCP URL or access token, revoke or
  rotate it and reconfigure the Mac. Deleting a committed secret is **not**
  sufficient because Git retains history.
- Relay production account, zone and hostname values must be provisioned
  through GitHub Actions secrets, not the public Wrangler template.

The `key` field in `extension/manifest.json` is a **public** Chrome extension
identification key, not a private API credential. It is intentionally embedded
to preserve the extension identity across installations.
