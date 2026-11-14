---
title: Owned launch coordinator experiment
description: Private pre-execution authorization protocol and its explicit lifecycle limits.
order: 15
category: Reference
---

This is an **unreleased, production-gated candidate** for [issue #4](https://github.com/amxv/slingring/issues/4). Queue/session integration is covered by fixtures, but the released helper has no approved packaged capability for this protocol. Enabling the `owned-launch-candidate` build feature therefore refuses browser execution with `guarded_helper_capability_unavailable`. Default builds do not opt into the experimental launch path. No installed helper, browser flags, Chrome profile, or application identity is changed. This is not a claim of complete browser-process cleanup.

## Authorization contract

The coordinator creates a new private socket directory and a unique session and generation. The helper must start a fresh daemon for that generation and use its exact session for every open, get, and close operation. Reusing a daemon that did not inherit the launch lease is not supported.

The guardian first receives the helper's approval through its inherited control pipe, then connects to the coordinator. Before Chrome execution, it sends six fields, each prefixed by a four-byte big-endian length and bounded to 4,096 bytes:

1. Protocol version: `slingring-launch-lease-v1`.
2. Exact session.
3. Exact generation.
4. Guardian PID as four big-endian bytes.
5. Expected browser executable path.
6. Isolated temporary profile path.

The PID is a claim, not proof of identity. On macOS the coordinator verifies the socket's kernel UID/PID, the guardian's executable and microsecond process start, and the expected session, generation and browser path. The profile must be an owner-private directory under the system temporary directory, with the helper's isolated-profile prefix. The separately configured guardian executable must match the actual peer; the browser path alone does not authenticate a guardian.

A private, fsynced receipt is written **before** sending approval byte `0x01`. Persistence failure sends no approval. A helper that never executes or a handshake cancelled before authorization leaves no launch receipt and does not permanently fence later work. A crash after the durable receipt is written remains conservatively uncertain, even if approval was never delivered.

The authenticated socket stays open for the browser session, not merely the launch future. Closing its write half requests cleanup while retaining the read half for the result. Worker cancellation or owner death closes the stream instead. Slingring never sends a signal to an adopted PID.

## Cleanup scope and recovery

The experimental guardian owns an unreaped direct child. Report byte `0x02` means that **direct browser root was reaped**, not that every descendant exited. EOF, an invalid report, a timeout or a missing durable result never becomes a successful cleanup claim.

The coordinator retains a `root_reaped_tree_unverified` receipt after this report. Both that receipt and an authorized-but-unconfirmed receipt block automatic replacement. There is intentionally no automatic deletion, PID/name kill, broad profile scan or ambiguous restart recovery.

## Queue integration and admission

The queue creates a runtime-only session after admission, shares that exact lease across a queued batch, and drops it with the worker even when queue/backend handles remain alive. Config serialization cannot persist or inject a lease. The helper socket directory and restore-state namespace are generation-specific; all helper commands receive the same namespace, session and four coordinator environment values.

The guarded path joins initial helper launch with coordinator authorization. It verifies the CDP-reported profile against the durable receipt and checks the lease before commands and before the durable Send barrier. A failed guarded launch is not retried. A consumed root-exit report permanently ends reuse.

Cleanup is separate from submission outcome: confirmed results are persisted and delivered before cleanup, while cancellation before versus after durable Send intent remains `not_submitted` versus `uncertain`. Any backend cleanup error or timeout stops the serial worker rather than launching another job. A legacy helper's successful close acknowledgment is logged as `backend_ack`, not verified process exit. Default builds also refuse a previously recorded unresolved launch receipt, so disabling the candidate cannot bypass its fence. Read-only job recovery remains available.

The v1 session deliberately reports cleanup as incomplete even after root reaping. There is no reviewed policy allowing another generation after that limited report. The current gate is not a usable opt-in production feature: approved artifacts and a complete recovery policy are prerequisites, not checks inferred from helper filenames or a matching upstream version.

Remaining work before enabling the production path:

- Prove bounded ownership and cleanup of browser descendants, including detached processes, without a PID-reuse signal race.
- Establish durable terminal evidence that remains trustworthy after coordinator or guardian death.
- Validate the integrated lease through the actual packaged CLI/helper daemon discovery path, not only synthetic queue backends and the patched helper launch fixture. Define how an unresolved receipt is safely reconciled before any new generation, including after service restart.
- Package, sign, version and checksum the guardian and patched helper; release those dependencies before changing Slingring's pin. Never silently replace the current helper with an experimental binary.
- Evaluate a distinct macOS application identity separately. A temporary profile or guardian does not establish a separate Launch Services identity.

## Fixture verification

`cargo test --locked --test launch_coordinator` uses synthetic Unix peers and temporary private state, not Chrome or ChatGPT. It covers peer authentication, bounded framing, durable approval, cancellation, generation exclusion and conservative cleanup reports. The queue suite (`cargo test --locked --test queue_guarded_session`) covers reuse, configuration boundaries, worker abort, stop/deadline on both sides of the Send barrier, immutable submission outcomes and runtime-only lease configuration. The same suite with `--features owned-launch-candidate` additionally verifies refusal before unsupported helper execution. The opt-in cross-repository fixture validates the exact patched-helper launch path and the runtime session wrapper with a disposable fake browser. Neither test authorizes a real account submission or a production service restart.

### Joint pinned-helper fixture

Build the separately reviewed guardian and the pinned helper's test binary. Then run from this checkout, substituting those two exact artifact paths:

```sh
SLINGRING_TEST_HELPER_FIXTURE=/absolute/path/to/agent_browser-test-binary \
SLINGRING_TEST_LAUNCH_GUARD=/absolute/path/to/owned-launch-guard \
cargo test --locked --test helper_launch_coordinator -- --ignored --nocapture
```

The test invokes only `native::cdp::chrome::tests::test_private_launch_guard_path_with_external_coordinator` in that helper test binary. It creates a private fake browser/profile, passes the session's four coordinator lease values and isolated helper namespace/socket directory, authenticates the real guardian peer, and waits for the helper's `fake-browser-ready` marker. The runtime session wrapper then half-closes its lease, persists only the root-reaped report, refuses to call that complete tree cleanup, and confirms the helper test independently observed guardian/root exit. The root-only receipt still blocks another launch. The fake browser is a temporary shell script that writes a fake DevTools endpoint and becomes `sleep`; it never opens a port, website, Chrome profile or account.

The helper source under review is pinned to upstream `v0.41.1` (`5868f7c`), with a separate launch-only guardian patch. The latest guardian additionally uses a private session/group and a retained, unreaped direct-child anchor for bounded same-group cleanup, but v1 still communicates only root reaping. An escaped process, dead guardian, or ambiguous snapshot is not proven absent. This fixture does not validate helper daemon discovery, installed packaging, crash recovery of a complete browser tree, or any macOS application identity. It does not justify closing issue #4 or releasing a helper substitution.

## Offline package validation

`scripts/stage_owned_helper_candidate.py` is a separate **offline fixture stager**, not an installer or a supported-helper pin change. It never downloads or executes a binary and does not create a wrapper. It requires an independently reviewed pin containing exact archive and manifest SHA-256 digests plus expected source version, commits and patch/lock digests. Do not construct a trusted release pin from an archive's self-generated sidecars.

The fixed archive has seven regular root members: the native helper, guardian, `SLINGRING_LAUNCH_CAPABILITIES.json`, upstream license, version, commit and patch digest. The canonical manifest binds schema/protocol/platform, both executable hashes, source provenance, `artifact_scope: offline_fixture_candidate` and `cleanup_evidence: root_reaped_tree_unverified`. Unknown, duplicate, missing, linked, oversized or traversal members are rejected. Every source sidecar and binary hash must match. The stager accepts only a new destination under a canonical owner-private directory and writes **all files mode 0600**, including the two binaries, so staging does not make them executable.

```sh
python3 -W error scripts/test_owned_helper_candidate.py
python3 scripts/stage_owned_helper_candidate.py \
  --archive /absolute/path/to/reviewed-fixture.tar.gz \
  --pin /absolute/path/to/independently-reviewed-fixture-pin.json \
  --destination /canonical/private/temp/directory/new-stage
```

The package tests cover independently pinned checksums, source/version mismatch, missing or altered capability, tampering, malformed JSON, archive attacks, permission bounds and preserving an existing destination. Runtime tests separately reject missing/mismatched guardian claims, untrusted config injection and inherited lease environment. Only the queue-created generation supplies lease and namespace values; subsequent commands require the authenticated lease. Unknown or ended lease evidence cannot become live again.

The current legacy installer remains unchanged. Its wrapper deliberately sets `AGENT_BROWSER_NAMESPACE=slingring`, and its archive allowlist has no guardian/manifest entries. It must not be used as-is for the candidate. Enabling the candidate will require a verified native artifact path or a revised, explicitly authenticated wrapper that preserves the exact generation namespace. An environment variable alone is not authentication.

The helper's offline archive demonstrates the packaging contract only. It was built from exact upstream plus the lifecycle patch and has **not** established equivalence with all settings of the supported browse fork. Its signed-file hashes are useful fixture evidence, not approval to replace the released helper. The stager does not itself establish Mach-O architecture, signature trust, Gatekeeper acceptance, live daemon discovery, full descendant cleanup or application identity. Those remain separate checks before a production pin, capability gate or release can change.

### Remaining delivery order

1. Review the lifecycle patch against the exact supported fork without altering its browser/protection behavior. Freeze source and lockfile provenance and validate both role-specific no-browser capability probes.
2. Settle the bounded cleanup/reconciliation policy for root-only or ambiguous receipts. No current v1 result authorizes automatic replacement.
3. Prepare an immutable signed/checksummed helper-plus-guardian release and independently approved manifest pin, then extend installer validation and generation namespace handling. Preserve the legacy path and reject incomplete or mismatched upgrades. Do not republish or overwrite the existing helper tag.
4. With explicit approval, validate the exact packaged native path in a disposable macOS environment using a fresh profile and trusted local fixture. Check cancellation/crash ownership and native application identity separately. A temporary directory in the interactive account does not guarantee isolation from user-level Launch Services effects.
5. Only after those checks and coordinated dependency publication, change Slingring's supported pin and open the production capability gate. Rerun the full suite, release the patch, then verify the exact installed Mac version. Linux remains a separate later release.

## Read-only application-identity assessment

Chrome for Testing is a candidate **for separate application identity only**, not an already validated replacement. Chromium's [Mac flavor documentation](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/mac/flavors_of_chrome.md) gives it bundle ID `com.google.chrome.for.testing`, distinct from normal Chrome's `com.google.Chrome`. However, the same document lists its default user-data directory as normal Chrome's location. An explicit, private throwaway profile is therefore mandatory; never launch it with an implicit profile.

The official [Chrome for Testing documentation](https://developer.chrome.com/docs/automation-and-testing/chrome-for-testing) describes versioned automation builds without automatic updates and limits their intended use to trustworthy content. [Official downloads](https://googlechromelabs.github.io/chrome-for-testing/) include macOS ARM64. A maintained version pin and security-update process would be necessary before adoption. No browser has been downloaded, installed or launched for this assessment.

Chromium is another distinctly branded candidate (`org.chromium.Chromium` appears in the [upstream macOS bundle handling](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/build/apple/tweak_info_plist.py)), but its [best-effort builds do not reliably map to Chrome releases](https://www.chromium.org/getting-involved/download-chromium/). Official side-by-side Chrome Beta/Dev or Canary are additional normally supported alternatives; verify their actual bundle IDs, since historical non-side-by-side Beta/Dev installations share normal Chrome's identity. A normally supported headed session could be used as an explicit isolated identity test. Headed mode alone does not create a separate bundle identity.

There is also a compatibility boundary: the pinned browse fork's existing headless user-agent handling is conditioned on the exact regular-Chrome executable path. Choosing another browser is not automatically behavior-neutral. Do not replicate or retune that behavior, add anti-detection changes, or modify browser protections as part of an identity experiment.

### Native validation still required

These are proposed checks, **not tests already performed**:

1. In an isolated test account or machine, obtain an approved official artifact. Verify its unmodified bundle identifier, executable architecture, version, code signature and Gatekeeper assessment. Do not rewrite Info.plist, ad-hoc re-sign it, remove quarantine or disable protections.
2. Launch only that explicit test executable with a new private profile and a local trusted fixture or `about:blank`. Use a normally supported browser mode; test headed and headless separately without changing the owner's saved default.
3. Bind its owned CDP browser PID to native [NSRunningApplication](https://developer.apple.com/documentation/appkit/nsrunningapplication) metadata: bundle ID, bundle URL, executable URL, launch date, activation policy and object identity. Apple warns that a bare [processIdentifier is not sufficient to compare applications](https://developer.apple.com/documentation/appkit/nsrunningapplication/processidentifier). If a headless process has no native application object, record the binding as unproven rather than inferring it from a filename.
4. Read normal Chrome's existing native application objects and application lookup before/after without launching, activating, quitting or accessing its profile. Verify those bindings remain unchanged and that the test browser has its own identity. Do not force Launch Services registration or change the default browser.
5. Exercise graceful close, cancellation, timeout and owner/helper failures against the isolated fixture, checking exact owned root/descendants and application disappearance. Never use normal-Chrome process termination as cleanup. Any test that requires normal Chrome activation belongs in a separately authorized clean test account, not the owner's interactive session.
6. Audit extension/native-host support and lifecycle behavior separately. Distinct application identity does not solve orphaned descendants or establish crash-safe process ownership.
