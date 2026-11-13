---
title: Owned launch coordinator experiment
description: Private pre-execution authorization protocol and its explicit lifecycle limits.
order: 15
category: Reference
---

This is an **unreleased, unwired experiment** for [issue #4](https://github.com/amxv/slingring/issues/4). The installed helper, browser flags, Chrome profiles, application identity, and normal submission path are unchanged. It is not a claim of complete browser-process cleanup.

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

Remaining work before production integration:

- Prove bounded ownership and cleanup of browser descendants, including detached processes, without a PID-reuse signal race.
- Establish durable terminal evidence that remains trustworthy after coordinator or guardian death.
- Integrate the exact generation lease into queue cancellation, timeout and service startup while preserving confirmed submission outcomes.
- Package, sign, version and checksum the guardian and patched helper; release those dependencies before changing Slingring's pin. Never silently replace the current helper with an experimental binary.
- Evaluate a distinct macOS application identity separately. A temporary profile or guardian does not establish a separate Launch Services identity.

## Fixture verification

`cargo test --locked --test launch_coordinator` uses synthetic Unix peers and temporary private state, not Chrome or ChatGPT. It covers peer authentication, bounded framing, durable approval, cancellation, generation exclusion and conservative cleanup reports. A separate opt-in cross-repository fixture is needed to validate the exact patched-helper launch path with a disposable fake browser. Neither test authorizes a real account submission or a production service restart.

### Joint pinned-helper fixture

Build the separately reviewed guardian and the pinned helper's test binary. Then run from this checkout, substituting those two exact artifact paths:

```sh
SLINGRING_TEST_HELPER_FIXTURE=/absolute/path/to/agent_browser-test-binary \
SLINGRING_TEST_LAUNCH_GUARD=/absolute/path/to/owned-launch-guard \
cargo test --locked --test helper_launch_coordinator -- --ignored --nocapture
```

The test invokes only `native::cdp::chrome::tests::test_private_launch_guard_path_with_external_coordinator` in that helper test binary. It creates a private fake browser/profile, passes the coordinator's four lease environment values, authenticates the real guardian peer, and waits for the helper's `fake-browser-ready` marker. The coordinator then half-closes its lease, accepts only the root-reaped report, and confirms the helper test independently observed guardian/root exit. The root-only receipt still blocks another launch. The fake browser is a temporary shell script that writes a fake DevTools endpoint and becomes `sleep`; it never opens a port, website, Chrome profile or account.

The helper source under review is pinned to upstream `v0.41.1` (`5868f7c`), with a separate launch-only guardian patch. This fixture does not validate helper daemon discovery, installed packaging, crash recovery of a complete browser tree, or any macOS application identity.

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
