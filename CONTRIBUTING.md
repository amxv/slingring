# Contributing to Slingring

Slingring is an Apache-2.0 Rust CLI with a Swift menu bar companion, a Chrome extension, an optional Cloudflare relay, and Astro documentation. The current downloadable releases target **Apple Silicon macOS**. Do not assume other platform builds are available until they are released.

## Prerequisites

- The pinned Rust toolchain in `rust-toolchain.toml` (currently **1.99.1**).
- `just` for common development commands.
- Bun for the extension, relay and documentation workspaces (use their committed lockfiles).
- `dist` **0.32.0** only when inspecting or regenerating release configuration.

No GitHub admin privileges, cloud credentials, or ChatGPT login are needed for local fixture tests. While the repository is private, cloning it requires GitHub access; a public clone does not.

## Build and test

Run from the repository root:

```bash
just check-fast
just check
just build
./target/debug/slingring --help
python3 scripts/test_installers.py
bash apps/menubar/test.sh
```

Run the web-component tests using the committed Bun lockfiles:

```bash
(cd extension && bun install --frozen-lockfile && bun run check && bun run test)
(cd relay && bun install --frozen-lockfile && bun run check && bun run test && bun run dry-run)
```

Never run a live ChatGPT submission as part of routine tests; it requires explicit authorization. Preserve user browser profiles, credentials and existing installations.

Before publishing changes, run `gitleaks git --log-opts='--all' .` if Gitleaks is available. The narrow exception in `.gitleaks.toml` permits only the Chrome extension's public identification key, not deployment or user credentials.

## Documentation

```bash
just docs-install
just docs-test
just docs-check
just docs-dev
```

Documentation lives in `docs/`. GitHub Actions deploys docs on pushes to `main` affecting `docs/**` (or a manual workflow dispatch), not on routine Rust-only changes. Vercel's project root is `docs`, while the prebuilt deployment command runs at the repository root. The docs workflow reads `VERCEL_TOKEN` from GitHub Actions **secrets** and `VERCEL_ORG_ID`/`VERCEL_PROJECT_ID` from Actions **variables**. Vercel Git-triggered deployments are intentionally disabled in `docs/vercel.json`.

## Self-hosting the relay

The checked-in `relay/wrangler.jsonc` is deliberately account-neutral. New installations may use Wrangler with their own account. Existing custom-domain installations must preserve their Worker name, Cloudflare account, zone, hostname, Durable Object migration and secrets. See [relay/README.md](relay/README.md) for `bun run deploy:configured` and the required GitHub Actions **secrets** (`CLOUDFLARE_ACCOUNT_ID`, `RELAY_ZONE_ID`, `RELAY_HOSTNAME`). The existing Cloudflare deployment token also remains a GitHub Actions secret. The `DEPLOY_RELAY` enable/disable switch remains a GitHub Actions **variable**. A pull request never deploys the relay.

## Releases

Slingring distributes binaries and the optional menu bar companion through **GitHub Releases**. It is not published to npm or crates.io (`Cargo.toml` sets `publish = false`). `dist-workspace.toml` is the source of truth for cargo-dist; `.github/workflows/release.yml` is generated and must not be hand-edited. Use:

```bash
dist plan
dist generate --check
```

Maintainers follow [the release skill](.agents/skills/release/SKILL.md) when explicitly authorized to prepare and publish a release. Normal contributions should not create tags, trigger production deployment, alter signing configuration or rotate credentials.
