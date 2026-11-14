# Opt-in disposable hosted macOS identity check

This is an **opt-in validation fixture, not a released feature**. The initial hosted run reached preflight but did not launch a browser. It is independent of release workflows, helper installation, production capability gates and the owner's Mac. Never run browser mode on a personal machine or a self-hosted runner.

## Scope and safety

The manual workflow uses a fresh `macos-15` GitHub-hosted ARM64 runner for each of headed/headless modes. GitHub's [image inventory](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md) lists both regular Chrome and Chrome for Testing; its [installer source](https://github.com/actions/runner-images/blob/main/images/macos/scripts/build/install-chrome.sh) puts both bundles in `/Applications`. The fixture does not download or install either. Missing bundles, an unavailable GUI/Launch Services session, signature/policy rejection or missing native evidence produce a nonzero **unproven** result, never an Info.plist-only pass.

Browser mode is guarded by the hosted-runner environment, ARM64/macOS, expected repository, manual event, runner username and an explicit authorization input. No job receives secrets. Checkout credentials are not persisted. Only bounded metadata JSON is uploaded; no profiles, browser logs, screenshots, page text, cookies, tokens or arbitrary environment dumps are uploaded.

Every browser gets a fresh private profile and sanitized HOME/TMPDIR. The sole page is `about:blank`; CDP connections use loopback and a validated endpoint from that profile. Browser-owned background networking may still occur under normal browser defaults. No user-agent/fingerprint tuning, protection flags, Gatekeeper changes, quarantine removal, bundle rewriting, app installation, default-browser changes or process-name/PID signals are used.

Code signatures and `spctl` execution policy are checked without modification. This establishes integrity and system policy, **not an independently pinned Google publisher identity**; the report says so. Trust in the preinstalled artifacts is scoped to the documented GitHub image. Exact browser version/image and signature provenance should be pinned before using this to approve a production browser replacement.

## Read-only preflight diagnostics

Every available bundle is checked independently with both `codesign --verify --deep --strict` and `spctl --assess --type execute`, even when an earlier probe fails. All four checks must succeed before browser launch. A separate read-only `spctl --status` probe must also return affirmative `assessments enabled`; an empty or unavailable status is unproven. Metadata reports contain only the fixed browser/probe labels, numeric exit status, bounded elapsed time and an allowlisted category: signature missing/invalid/unclassified, policy rejected, assessment unavailable/unclassified, tool-start failure or timeout. Raw tool output is capped at 8 KiB in memory and is never printed, persisted or uploaded. Incomplete or truncated capture is unproven even when a tool exits zero. An unclassified nonzero result is not automatically called a corrupt signature.

A separately obtained official artifact would need an independently reviewed exact version/source and digest, then the same unmodified signature and system-policy checks in a new disposable runner. That is not part of this workflow, and merely redownloading is not evidence of a fix. Do not remove quarantine, re-sign a browser, disable assessment or skip a failing check. An unavailable assessment environment calls for a supported environment, not a weaker pass condition.

## What a pass means

1. Normal Chrome opens through Launch Services with a fresh profile, an ordinary foreground window and matching native bundle ID, executable URL, bundle URL and launch date. It is closed through its own CDP connection.
2. CfT starts from its exact preinstalled executable with another fresh profile. Headed mode additionally requires an ordinary window. Both modes require actual `NSRunningApplication` binding; a headless process without such an object remains unproven.
3. While that same CfT process and native object are still alive, normal Chrome opens again through Launch Services with a separate foreground window and distinct application object. The CfT binding is rechecked after the Chrome window exists, preventing a temporal false positive if CfT exited early.
4. Both roots close; normal Chrome opens successfully again after CfT close, then closes.

The [Apple identity guidance](https://developer.apple.com/documentation/appkit/nsrunningapplication/processidentifier) is followed: compare native objects using `isEqual`, not only PIDs. Dynamic AppKit properties are observed while pumping the main run loop. Cleanup first binds the same CDP connection's browser PID to the retained launch, then requests `Browser.close`. Changed identity, missing endpoints or timeout remain unproven. There is no raw signal fallback. If bounded close fails, the job fails and disposal of that hosted VM provides the containment boundary; it is **not** counted as a verified browser-tree cleanup result.

## Local static checks only

```sh
swiftc -swift-version 5 -warnings-as-errors -typecheck tests/hosted/browser_identity.swift
swiftc -swift-version 5 -warnings-as-errors tests/hosted/browser_identity.swift -o /tmp/slingring-identity-self-test
/tmp/slingring-identity-self-test --self-test
actionlint .github/workflows/owned-browser-identity.yml
```

`--self-test` only tests pure context/evidence predicates. It performs no browser launch or running-application lookup. Do not fake hosted environment variables to run browser mode locally.

## Safe trigger after review and authorization

The validation branch was reviewed, published and explicitly dispatched for the initial run described below. Review the exact branch commit and authorization before any further dispatch. [GitHub requires a dispatchable workflow file on the default branch](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow), so the new dedicated workflow cannot yet be dispatched by name.

The validation branch therefore adds a default-off input to the **already registered CI workflow**. With that explicit input, it skips ordinary CI jobs and calls the local reusable identity workflow from the same reviewed commit. No experimental runtime or workflow needs to be merged into main just to use this route. Publication and each deliberately scoped validation run require authorization. The initial run verified this branch-only dispatch route without a main-branch change.

After that review/authorization, use the registered workflow with the reviewed validation ref:

```sh
gh workflow run ci.yml --repo amxv/slingring \
  --ref validation/owned-browser-hosted \
  -f approve_disposable_browser_test=true
```

The direct `owned-browser-identity.yml` dispatch route becomes available only if that manual workflow is separately registered on the default branch. The reusable call path was exercised successfully by the initial run; that is workflow wiring evidence, not a browser identity pass.

Check that GitHub resolves the intended SHA before trusting the run. There are two bounded hosted jobs, so normal repository Actions usage charges may apply. Read both metadata artifacts and treat either `unproven`, missing report, failed step or unavailable runner as incomplete validation. Do not retry by relaxing protection or identity checks.

## Recorded hosted evidence

The [initial run 43535946917](https://github.com/amxv/slingring/actions/runs/43535946917) used reviewed commit `74ed2e5c3172e989399e66ed5542bcd024181f61` on GitHub-hosted image `macos-15-arm64` version `20261104.0514.1`. Both headed and headless jobs passed hosted-context, compile and pure guard checks, then failed closed at the original combined signature/policy preflight. No fixture browser/profile was launched, and the uploaded reports had empty native and cleanup evidence. The old combined code did not identify which browser/probe returned nonzero; the independent diagnostics above address that evidence gap without changing the pass condition or system policy.

## Still not issue #4 closure or production validation

This is an independently testable **app-identity assessment**. It does not run Slingring's helper/guardian, validate the supported fork's behavior, prove all descendants exited, handle guardian death, reconcile durable receipts, or authorize automatic replacement. A subsequent test must integrate the exact reviewed/pinned helper package and actual Session coordinator with the approved preinstalled browser, preserving browser/protection behavior, then cover timeout, abort, daemon/guardian crash, restart and relevant-child cleanup. The current candidate production gate remains closed. Issue #4 stays open until its remaining lifecycle and real macOS acceptance criteria pass.
