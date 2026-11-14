# Fresh Google artifact inspection

This implements only stage 1 of the [reviewed plan](FRESH_GOOGLE_PLAN.md). It is a validation tool, not an installer, supported browser selection, or release change. It has not acquired or assessed a real artifact. The workflow is local/unpublished and has never been dispatched.

## Admission and scope

`.github/workflows/fresh-google-inspection.yml` is manual/default-off and also callable from a future separately reviewed CI dispatch route. It requires `approve_fresh_google_inspection=true`. This authorizes **only** fresh consumer Stable/Beta retrieval and inspection on a disposable GitHub-hosted ARM64 Mac, subject to [Google Terms](https://policies.google.com/terms) and [Chrome additional terms](https://www.google.com/chrome/terms/). No browser-launch input or code path exists. A later native test needs separate review, exact reviewed hashes/versions and explicit launch authorization.

The Python entry point checks the real OS/architecture/username, repository, manual event, hosted runner and explicit opt-in before creating output or invoking any inspection commands. It refuses local mode. Never spoof hosted context on a personal computer.

The workflow pins checkout/upload action commits, grants only contents-read, persists no checkout credential and receives no secrets. Normal CI, the preinstalled-browser workflow, all production/helper pins and capability gates are unchanged. The new workflow is not registered on main; an authorized future publication needs a reviewed registered-workflow call path or registration before it can be dispatched. No registration or dispatch is part of this implementation.

## What it does

- Uses only the two literal consumer DMG URLs in the plan. No Homebrew, PKG installer, browser discovery or third-party mirror.
- Uses the preinstalled system curl with TLS verification, config disabled, HTTPS only, no redirects or retries, a 180-second total-transfer timeout and a 1 GiB streamed byte limit. Requires curl 8.4.0 or newer because older versions do not enforce the size limit for unknown-length transfers. This is based on [curl's documented limits](https://curl.se/docs/manpage.html#--max-filesize). A separate 185-second command observation limit cannot turn an uncertain transfer into success.
- Writes into exclusively created private files through an inherited descriptor. Requires completed HTTP 200 and bounded output before hashing. Reports the SHA as an observed-byte digest, not publisher authentication.
- Uses only system `hdiutil verify`, read-only/no-auto-open attach, structured mount receipts, and `statvfs` read-only evidence. A failed attach with a complete exact receipt still attempts scoped cleanup. Incomplete/ambiguous receipts remain unknown; it never guesses a device.
- Copies only the expected full app into the job's private directory using attribute-preserving `ditto`. Nothing is installed in `/Applications`. Outer-bundle/executable escapes and wrong consumer identities are rejected.
- Requires recursive strict signature integrity, a separate exact top-level Google Developer ID requirement, affirmative enabled assessment, normal `spctl` execution acceptance and ARM64 architecture. Probes are individually recorded even if another completed probe fails. No app is executed by these checks.
- Compares bounded quarantine metadata only in memory to check that copying preserved it. The only `xattr` operation is read-only `-px`; no metadata is changed. Reports only its presence, never values. It does not claim first-download Gatekeeper UI testing, even if metadata exists.
- Before detach, rechecks the exact image/device/mount against `hdiutil info`. Only its own exact receipt may be detached, without force. Timeout, unknown child outcome or cleanup uncertainty stops the run conservatively; no PID signals or device-name guessing are used. Disposable VM teardown is containment, not a successful cleanup assertion.

## Output and limits

The sole uploaded artifact is `fresh-google-inspection.json`. It contains fixed source labels/URLs, observed DMG hashes/byte counts, verified versions/IDs/expected team, allowlisted result categories/statuses, image/workflow version and explicit untested scope. It contains no raw diagnostics, private paths, profiles, screenshots, cookies, arbitrary environment or browser files. Reports are retained for seven days.

All command output is capped at 64 KiB in memory; truncation, missing EOF, timeout and unknown status fail closed. System-tool and mounted-image behavior still needs an authorized hosted run. Unit mocks do not prove a real transfer, publisher signature, Gatekeeper verdict, mount, architecture, or native app identity.

Consumer DMG URLs roll. This stage does not approve any observed digest automatically. Later validation must independently review those hashes and versions and fail if a subsequent retrieval changes. Browser execution, helper integration, subtree cleanup and issue #4 closure are not implemented here.

## Local static checks only

```sh
python3 -W error tests/hosted/test_inspect_fresh_google.py
python3 -m py_compile tests/hosted/inspect_fresh_google.py
actionlint .github/workflows/fresh-google-inspection.yml
git diff --check
```

Fixtures use fake command/transfer adapters and disposable ordinary files, never actual downloads, signatures, mounts, installed services or browsers. They cover admission before side effects; fixed source and transfer limits; redirect/partial/timeout refusal; bounded capture; exact publisher requirements; signature/policy failures; distinct consumer Beta; ambiguous/foreign mount cleanup; pending-command races; preserved quarantine; and metadata-only delivery.
