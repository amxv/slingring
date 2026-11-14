# Fresh consumer Chrome validation plan

Status: **design only, not executable, not published, no artifacts obtained**.
This separate validation branch starts at `7b1884b209c4c7405c7c78a39cd604093868968b`.
The preinstalled-browser fixture and its failed hosted evidence remain intact.
No helper/runtime capability gate, supported browser, release, or installation changes are proposed here.

## Purpose and evidence boundary

Test freshly obtained, unmodified consumer Chrome Stable and side-by-side Chrome Beta on a disposable GitHub-hosted ARM64 Mac. The preinstalled Stable and CfT artifacts both failed strict signature verification in [run 43537212836](https://github.com/amxv/slingring/actions/runs/43537212836); the exact cause remains unclassified. A fresh artifact is a separate candidate, not permission to ignore that result.

The intended result is application-identity/coexistence evidence only. It will not prove helper compatibility, all-descendant cleanup, guardian crash recovery, or issue #4 closure. No first-download Gatekeeper UI validation is claimed merely from command-line assessment; direct HTTP acquisition does not necessarily apply quarantine metadata.

## Sources and identities

- Consumer [Stable](https://www.google.com/chrome/?platform=mac) and [Beta](https://www.google.com/chrome/beta/?platform=mac) are the source products. Downloads are governed by [Google Terms](https://policies.google.com/terms) and [Chrome additional terms](https://www.google.com/chrome/terms/). Reading this plan does not download or accept anything.
- Google's [flavor documentation](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/mac/flavors_of_chrome.md) distinguishes consumer SxS Beta from enterprise/non-SxS Beta. Only consumer SxS Beta is admissible. Never substitute the enterprise Beta package.
- Stable: `Google Chrome.app`, executable `Contents/MacOS/Google Chrome`, bundle and signing identifier `com.google.Chrome`.
- Beta: `Google Chrome Beta.app`, executable `Contents/MacOS/Google Chrome Beta`, bundle and signing identifier `com.google.Chrome.beta`.
- Both use Google Team ID `EQHXZ8M8AV`, independently corroborated by [Apple's browser signing registry](https://github.com/apple/password-manager-resources/blob/main/quirks/web-browser-extension-distribution-information.json). These are distinct app identities, not distinct publishers. The expected signing requirement must also require an Apple anchor and Developer ID Application certificate type, not just a string read from an unverified plist.
- Exact verification requirement: `identifier "EXPECTED_ID" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = "EQHXZ8M8AV"`, where `EXPECTED_ID` is replaced only by the two fixed identifiers above. Use it for verification (`codesign --verify --deep --strict -R`), never signing. The certificate-type conditions follow [Apple's Developer ID rule](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html).
- Candidate fixed download locations are `https://dl.google.com/chrome/mac/universal/stable/GGRO/googlechrome.dmg` and `https://dl.google.com/chrome/mac/universal/beta/googlechromebeta.dmg`. Google [documents the Stable DMG URL](https://support.google.com/chrome/a/answer/12818048?hl=en-GB); the [Homebrew Beta formula](https://github.com/Homebrew/homebrew-cask/blob/master/Casks/g/google-chrome%40beta.rb) corroborates the Beta URL. The rendered Google consumer Beta page did not expose its direct binary URL in this research. Therefore the exact Beta artifact remains unproven until a separately authorized hosted acquisition passes the independent publisher and SxS identity checks. No Homebrew install, third-party binary mirror, or formula script will be used.
- These are **rolling URLs**, not immutable version or checksum endpoints. Google's [VersionHistory API](https://developer.chrome.com/docs/web-platform/versionhistory/guide) supplies channel/version history, not an independent SHA-256 for these DMGs. Latest release metadata alone cannot prove what a rolling endpoint serves.

## Two explicit stages

Both stages must be manual, default off, reviewed at an exact workflow commit, GitHub-hosted only, with no secrets or persisted checkout credentials. No browser artifact is fetched or mounted on a personal Mac. No workflow is dispatched as part of preparing the fixture.

### 1. Inspect-only acquisition

1. Enforce the existing hosted ARM64 repository/manual-event/context guards before network or disk-image work. Use a new private directory under the runner's temporary directory.
2. Fetch only the two fixed HTTPS URLs with TLS verification, redirects disabled (or separately reviewed exact allowlisted redirects), a bounded duration and byte count, exclusive output files, and no cookies/credentials. Never execute retrieved scripts. Unexpected status, redirect, size, or transport error is unproven.
3. Record the actual DMG SHA-256. It is an **observed-byte digest**, not independent publisher authentication. Reject symlink/nonregular input and verify disk-image integrity with the system `hdiutil verify`.
4. Mount read-only with `hdiutil attach -readonly -nobrowse -noautoopen -plist` at an exact private mountpoint. Parse structured output. Reject ambiguous/multiple unexpected volumes, unexpected mountpoints, and unproven read-only state. Use no `sudo`, PKG installer, auto-open, repair, or policy-changing options. Standard macOS `hdiutil` supports this without installing an app; actual hosted mount support has not yet been exercised.
5. Admit only the exact expected .app directory. Reject symlink/escaping outer bundle or main executable and ambiguous candidates. Preserve legitimate internal signed-bundle layout. Copy only that complete app into a private job directory using an attribute-preserving copy; never copy the DMG's Applications shortcut or arbitrary neighboring content. Do not modify or write anything in `/Applications`.
6. On the **copied** app, independently require exact bundle/executable/signing identity, ARM64 support, strict/deep signature validity, the exact Google Developer ID requirement, affirmative enabled system assessment, and successful normal `spctl --assess --type execute`. Any failure remains unproven; no re-signing, quarantine removal/change, ignored checks, policy rules, security disabling, or alternate launch to get around it.
7. Record only bounded allowlisted metadata: workflow SHA, image/OS version, fixed source label, byte count, DMG SHA, verified bundle version and IDs, expected publisher match, assessment categories/status and quarantine-present boolean. Never upload bundles, DMGs, private paths, profiles, raw diagnostics, arbitrary extended attributes, or environment dumps.
8. Detach only the exact device returned for this invocation, without force. On timeout or uncertainty record incomplete cleanup and leave containment to disposal of the hosted VM. Do not guess device names or signal unrelated processes.

This stage does not launch Stable, Beta, any installer, or any helper. It produces evidence for review, not an approval to run a browser.

### 2. Pinned native identity validation

- Require separately reviewed exact Stable and Beta DMG hashes **and** versions from stage 1, in addition to explicit launch authorization. Repeat acquisition and all checks. If a rolling endpoint has changed, fail closed; do not silently update a pin, retry another version, or accept a new hash automatically.
- Keep the old CfT fixture behavior unchanged. Add a separate explicit fresh-pair variant using the exact admitted private bundle paths. Never fall back to the preinstalled Chrome, another channel, PATH discovery, or a process with the same name.
- Retain fresh private HOME/TMPDIR and separate explicit user-data directories for every launch, about:blank/loopback only, and the existing normal browser arguments. No detection/fingerprint or protection changes.
- Retain the exact native AppKit identity/launch-date/object checks and the temporal assertion that Beta is still alive while Stable has a foreground ordinary window. Exercise Stable baseline, concurrent Stable/Beta, and Stable after Beta closure, in both headed/headless Beta modes. Missing native registration or GUI support remains unproven.
- Keep CDP-owned-root cleanup and no raw PID signals. Report root evidence separately from unproven whole-tree cleanup. A signature/policy pass is not itself native identity evidence.
- Preserve any existing quarantine metadata unchanged. If acquisition did not create quarantine, explicitly report that the first-download Gatekeeper UI path was not tested. A full first-open/quarantine test would require a separately reviewed normal user-download/open route; it must not be simulated by removing metadata or disabling policy.

## Proposed local implementation and static checks

The next implementation commit should add a hosted-only acquisition/checker, pure tests, a separate default-off workflow, and an explicit fresh-pair entry point in the native fixture. The registered CI dispatch route can be extended only as a mutually exclusive opt-in choice; normal CI and the existing preinstalled fixture must remain unchanged. All publication/dispatch stays separately gated.

Pure fixtures should cover context rejection before side effects; exact URL and redirect allowlisting; invalid/missing/changed pins; malformed digest/version; oversized/partial input; nonregular and escaping paths; ambiguous mount receipts; wrong publisher/team/certificate type/app ID; each independent signature/policy failure; missing or truncated diagnostics; absent quarantine scope; foreign device cleanup refusal; and exact fresh-versus-preinstalled native identity mismatch. Fake command adapters must never mount or launch real applications locally.

Local checks: Python unit tests and syntax, Swift compile/typecheck and pure self-tests, actionlint, diff whitespace, outgoing privacy and secret scans. Inspect source for prohibited policy mutation, quarantine changes, unowned signals, secrets and production integration. Independent review must precede any push or one deliberately scoped hosted execution.

## What can be pinned now

Can pin before acquisition: source host/path policy, exact publisher requirement, bundle/signing/executable identities, fixture commit, workflow/action revisions, expected architecture, and report schema. A hosted image label is not an immutable image; record its actual version.

Cannot honestly claim yet: independent DMG content digest, actual downloaded version, successful normal policy assessment, readonly mount success on the hosted image, quarantine propagation, or native coexistence. Those require authorized hosted evidence. Observed hashes can later become reviewed reproducibility pins but never replace publisher verification.
