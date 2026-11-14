#!/usr/bin/env python3
"""Opt-in hosted inspection only. Never executes a downloaded app or installer."""
from __future__ import annotations

import dataclasses
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import pwd
import re
import selectors
import stat
import subprocess
import sys
import tempfile
import time

MAX_DOWNLOAD = 1024 * 1024 * 1024
MAX_CAPTURE = 65536
TEAM = "EQHXZ8M8AV"


@dataclasses.dataclass(frozen=True)
class Spec:
    name: str
    url: str
    app: str
    executable: str
    identifier: str


SPECS = (
    Spec("stable", "https://dl.google.com/chrome/mac/universal/stable/GGRO/googlechrome.dmg",
         "Google Chrome.app", "Google Chrome", "com.google.Chrome"),
    Spec("beta", "https://dl.google.com/chrome/mac/universal/beta/googlechromebeta.dmg",
         "Google Chrome Beta.app", "Google Chrome Beta", "com.google.Chrome.beta"),
)


class Unproven(Exception):
    """Only constant, allowlisted codes may be passed here."""


def require(value, code):
    if not value:
        raise Unproven(code)


def hosted(env, *, system=None, machine=None, user=None):
    return (
        env.get("GITHUB_ACTIONS") == "true"
        and env.get("RUNNER_ENVIRONMENT") == "github-hosted"
        and env.get("RUNNER_OS") == "macOS"
        and env.get("RUNNER_ARCH") == "ARM64"
        and env.get("GITHUB_REPOSITORY") == "amxv/slingring"
        and env.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
        and env.get("SLINGRING_FRESH_GOOGLE_INSPECTION") == "explicitly-authorized"
        and (system if system is not None else platform.system()) == "Darwin"
        and (machine if machine is not None else platform.machine()) == "arm64"
        and (user if user is not None else pwd.getpwuid(os.getuid()).pw_name) == "runner"
    )


def regular(path):
    require(stat.S_ISREG(path.lstat().st_mode) and path.resolve() == path,
            "nonregular_or_escaping_path")


def private_dir(path):
    path.mkdir(mode=0o700)
    require(path.resolve() == path and stat.S_ISDIR(path.lstat().st_mode), "private_directory_unproven")


def publisher_requirement(spec):
    require(spec in SPECS, "unknown_browser_spec")
    return (f'identifier "{spec.identifier}" and anchor apple generic '
            'and certificate 1[field.1.2.840.113635.100.6.2.6] exists '
            'and certificate leaf[field.1.2.840.113635.100.6.1.13] exists '
            f'and certificate leaf[subject.OU] = "{TEAM}"')


def download(spec, target, *, run=None, maximum=MAX_DOWNLOAD):
    require(spec in SPECS, "unknown_browser_spec")
    run = run or command
    version = run(["/usr/bin/curl", "-q", "--version"])
    if version.timed_out or version.status is None:
        raise Unproven("download_command_pending")
    match = re.match(rb"curl ([0-9]+)\.([0-9]+)\.([0-9]+) ", version.data)
    require(version.complete and version.status == 0 and match is not None,
            "download_client_unproven")
    # Since 8.4.0, max-filesize also aborts chunked/unknown-size transfers at the limit.
    require(tuple(int(x) for x in match.groups()) >= (8, 4, 0), "download_client_too_old")
    try:
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            # No config, redirects, retry, credentials, cookies, compression or TLS exceptions.
            # curl's total-transfer deadline includes drip-fed reads, unlike an inactivity timeout.
            result = run(["/usr/bin/curl", "-q", "--fail", "--silent", "--show-error", "--globoff",
                          "--proto", "=https", "--max-time", "180", "--connect-timeout", "15",
                          "--max-filesize", str(maximum), "--max-redirs", "0", "--retry", "0",
                          "--output", f"/dev/fd/{fd}", "--write-out", "%{http_code}", spec.url],
                         timeout=185, pass_fds=(fd,))
            if result.timed_out or result.status is None:
                raise Unproven("download_command_pending")
            require(result.complete and result.status == 0 and result.data.strip() == b"200",
                    "download_response_unproven")
            os.fsync(fd)
        finally:
            os.close(fd)
    except Unproven:
        raise
    except Exception:
        raise Unproven("download_transport_unproven") from None
    regular(target)
    size = target.stat().st_size
    require(0 < size <= maximum, "download_size_unproven")
    digest = hashlib.sha256()
    with target.open("rb") as source:
        while chunk := source.read(65536):
            digest.update(chunk)
    return {"bytes": size, "sha256": digest.hexdigest(), "digest_scope": "observed_bytes_not_publisher_proof"}


@dataclasses.dataclass
class Result:
    status: int | None
    data: bytes = b""
    complete: bool = True
    timed_out: bool = False


def command(args, timeout=45, pass_fds=()):
    """Bounded in-memory capture; no shell, raw logs, or process signal fallback."""
    try:
        child = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT, pass_fds=pass_fds,
                                 env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"})
    except OSError:
        return Result(None, complete=False)
    captured = bytearray()
    clipped, eof = False, False
    began = time.monotonic()
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while time.monotonic() - began < timeout:
                for key, _ in selector.select(0.1):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        eof = True
                    else:
                        remaining = MAX_CAPTURE - len(captured)
                        clipped |= len(chunk) > remaining
                        captured.extend(chunk[:remaining])
                if eof and child.poll() is not None:
                    child.stdout.close()
                    return Result(child.returncode, bytes(captured), not clipped)
    except (OSError, ValueError):
        child.stdout.close()
        child.poll()
        return Result(None, bytes(captured), False)
    # An owned helper may still be finishing. Do not guess a PID or proceed as if it exited.
    child.stdout.close()
    child.poll()
    return Result(child.returncode, bytes(captured), False, True)


def category(probe, result):
    if result.timed_out:
        return "timeout"
    if not result.complete:
        return "capture_or_tool_unproven"
    text = result.data.decode("utf-8", errors="replace").strip().lower()
    if probe == "assessment_enabled":
        return "passed" if result.status == 0 and text == "assessments enabled" else "assessment_status_unproven"
    if probe == "execution_policy" and any(x in text for x in (
            "assessments disabled", "assessment system is disabled", "not supported", "operation not permitted", "unable to obtain")):
        return "assessment_unavailable"
    return "passed" if result.status == 0 else "nonzero_unclassified"


def checked(report, probe, args, *, run=command):
    result = run(args)
    label = category(probe, result)
    item = {"probe": probe, "result": label}
    if result.status is not None:
        item["exit_status"] = result.status
    report["checks"].append(item)
    if result.timed_out or result.status is None:
        report["pending_tool"] = True
    require(label == "passed", "required_check_unproven")
    return result.data


@dataclasses.dataclass(frozen=True)
class Receipt:
    device: str
    mount: Path
    image: Path


def mount_receipt(data, mount, image):
    try:
        require(len(data) <= MAX_CAPTURE, "mount_receipt_unproven")
        entities = plistlib.loads(data)["system-entities"]
        require(isinstance(entities, list) and 0 < len(entities) < 16, "mount_receipt_unproven")
        devices = [x.get("dev-entry") for x in entities]
        require(all(isinstance(x, str) and re.fullmatch(r"/dev/disk[0-9]+(?:s[0-9]+)*", x) for x in devices), "mount_receipt_unproven")
        roots = {re.match(r"/dev/disk[0-9]+", x).group() for x in devices}
        mounts = [x["mount-point"] for x in entities if "mount-point" in x]
        require(len(roots) == 1 and mounts == [str(mount)], "mount_receipt_unproven")
        device = roots.pop()
        require(device in devices and mount.resolve() == mount, "mount_receipt_unproven")
        return Receipt(device, mount, image)
    except (KeyError, TypeError, ValueError, AttributeError, plistlib.InvalidFileException):
        raise Unproven("mount_receipt_unproven") from None


def matching_active_mount(data, receipt):
    try:
        images = plistlib.loads(data)["images"]
        require(isinstance(images, list) and len(images) < 128, "cleanup_ownership_unproven")
        matches = [x for x in images if x.get("image-path") == str(receipt.image)]
        require(len(matches) == 1, "cleanup_ownership_unproven")
        require(mount_receipt(plistlib.dumps(matches[0]), receipt.mount, receipt.image) == receipt,
                "cleanup_ownership_unproven")
        return True
    except Exception:
        return False


def detach(receipt, report, *, run=command):
    info = run(["/usr/bin/hdiutil", "info", "-plist"])
    if category("mount_info", info) != "passed" or not matching_active_mount(info.data, receipt):
        report["mount_cleanup"] = "ownership_unproven"
        return False
    result = run(["/usr/bin/hdiutil", "detach", receipt.device])
    report["mount_cleanup"] = "detached" if category("detach", result) == "passed" else "detach_unproven"
    return report["mount_cleanup"] == "detached"


def bundle_metadata(bundle, spec):
    require(bundle.name == spec.app and bundle.resolve() == bundle and stat.S_ISDIR(bundle.lstat().st_mode),
            "bundle_path_unproven")
    executable = bundle / "Contents" / "MacOS" / spec.executable
    regular(executable)
    info = bundle / "Contents" / "Info.plist"
    regular(info)
    require(info.stat().st_size <= 1024 * 1024, "bundle_info_oversize")
    try:
        value = plistlib.loads(info.read_bytes())
        require(value.get("CFBundleIdentifier") == spec.identifier
                and value.get("CFBundleExecutable") == spec.executable, "bundle_identity_unproven")
        version = value.get("CFBundleShortVersionString")
        require(isinstance(version, str) and re.fullmatch(r"[0-9]{1,6}(?:\.[0-9]{1,6}){3}", version),
                "bundle_version_unproven")
        return executable, version
    except (ValueError, TypeError, AttributeError, plistlib.InvalidFileException):
        raise Unproven("bundle_info_unproven") from None


def quarantine_marker(bundle, report, *, run=command):
    # Compare a bounded marker only in memory. Never persist, print, change or remove it.
    result = run(["/usr/bin/xattr", "-px", "com.apple.quarantine", str(bundle)])
    if result.timed_out or result.status is None:
        report["pending_tool"] = True
    require(result.complete and not result.timed_out, "quarantine_metadata_unproven")
    if result.status == 1 and b"No such xattr: com.apple.quarantine" in result.data:
        return None
    require(result.status == 0 and len(result.data) <= 16384, "quarantine_metadata_unproven")
    try:
        value = bytes.fromhex(result.data.decode("ascii"))
        require(len(value) <= 4096, "quarantine_metadata_unproven")
        return value
    except ValueError:
        raise Unproven("quarantine_metadata_unproven") from None


def inspect_one(spec, root, *, run=command, fetch=download, readonly=None):
    report = {"browser": spec.name, "source": spec.url, "status": "unproven", "checks": [],
              "mount_cleanup": "not_mounted", "browser_execution": "not_run"}
    receipt = None
    try:
        directory = root / spec.name
        private_dir(directory)
        image, mount = directory / "download.dmg", directory / "mount"
        private_dir(mount)
        report.update(fetch(spec, image))
        checked(report, "disk_image_integrity", ["/usr/bin/hdiutil", "verify", str(image)], run=run)
        report["mount_cleanup"] = "mount_outcome_unproven"
        result = run(["/usr/bin/hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen",
                      "-mountpoint", str(mount), "-plist", str(image)])
        # A failing attach may still supply an exact receipt. Recover it for scoped cleanup.
        if result.complete:
            receipt = mount_receipt(result.data, mount, image)
        require(receipt is not None and category("attach", result) == "passed", "mount_outcome_unproven")
        is_readonly = readonly(mount) if readonly else bool(os.statvfs(mount).f_flag & os.ST_RDONLY)
        require(is_readonly, "mount_not_proven_readonly")
        source = mount / spec.app
        bundle_metadata(source, spec)
        copy = directory / spec.app
        require(not copy.exists() and not copy.is_symlink(), "copy_destination_exists")
        source_quarantine = quarantine_marker(source, report, run=run)
        checked(report, "copy_intact_bundle", ["/usr/bin/ditto", "--rsrc", "--extattr", "--acl", str(source), str(copy)], run=run)
        executable, version = bundle_metadata(copy, spec)
        copied_quarantine = quarantine_marker(copy, report, run=run)
        require(source_quarantine == copied_quarantine, "quarantine_preservation_unproven")
        report["quarantine_present"] = copied_quarantine is not None
        report["first_download_gatekeeper_ui"] = "not_tested"
        # Scope this exact identifier requirement to the outer app, not its differently named helpers.
        probes = [
            ("codesign_integrity", ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(copy)]),
            ("codesign_google_publisher", ["/usr/bin/codesign", "--verify", "--strict", "-R", publisher_requirement(spec), str(copy)]),
            ("assessment_enabled", ["/usr/sbin/spctl", "--status"]),
            ("execution_policy", ["/usr/sbin/spctl", "--assess", "--type", "execute", str(copy)]),
            ("architecture", ["/usr/bin/lipo", "-archs", str(executable)]),
        ]
        evidence, verified = {}, True
        for name, args in probes:
            try:
                evidence[name] = checked(report, name, args, run=run)
            except Unproven:
                verified = False
                if report.get("pending_tool"):
                    raise  # Do not race a still-running command with more probes or cleanup.
        require(verified, "required_check_unproven")
        architectures = evidence["architecture"].decode("ascii").split()
        require("arm64" in architectures and set(architectures) <= {"arm64", "x86_64"}, "architecture_unproven")
        report.update(status="passed", code="artifact_inspected_without_execution", version=version,
                      bundle_id=spec.identifier, signing_id=spec.identifier, google_team=TEAM)
    except Unproven as error:
        report["code"] = str(error)
        if str(error) == "download_command_pending":
            report["pending_tool"] = True
    except Exception:
        report["code"] = "inspection_error_unproven"
    finally:
        if report.get("pending_tool"):
            report["mount_cleanup"] = "pending_tool_outcome_unproven"
            report.update(status="unproven", code="pending_tool_outcome_unproven")
        elif receipt is not None:
            if not detach(receipt, report, run=run):
                report.update(status="unproven", code="mount_cleanup_unproven")
        elif report["mount_cleanup"] == "mount_outcome_unproven":
            report.update(status="unproven", code="mount_cleanup_unproven")
    return report


def main(argv):
    if len(argv) != 3 or argv[1] != "--run-hosted":
        print("use --run-hosted REPORT; no local artifact inspection mode", file=sys.stderr)
        return 2
    env = os.environ
    if not hosted(env):
        print("not_disposable_hosted_context", file=sys.stderr)
        return 2
    try:
        temp = Path(env["RUNNER_TEMP"])
        require(temp.is_absolute() and temp.resolve() == temp and temp.is_relative_to("/Users/runner"), "runner_temp_unproven")
        report_path = Path(argv[2])
        require(report_path == temp / "fresh-google-inspection.json", "report_path_unproven")
        fd = os.open(report_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    except Exception:
        print("report_destination_unproven", file=sys.stderr)
        return 2
    report = {"schema": 1, "scope": "artifact_inspection_only", "status": "unproven", "artifacts": [],
              "browser_execution": "not_run", "first_download_gatekeeper_ui": "not_tested",
              "native_identity": "not_tested", "helper_integration": "not_run", "tree_cleanup": "unproven"}
    for key, field in (("GITHUB_SHA", "workflow_sha"), ("ImageVersion", "image_version")):
        value = env.get(key, "unknown")
        report[field] = value if re.fullmatch(r"[A-Za-z0-9._-]{1,64}", value) else "unproven"
    try:
        root = Path(tempfile.mkdtemp(prefix="slingring-fresh-google-", dir=temp))
        for spec in SPECS:
            item = inspect_one(spec, root)
            report["artifacts"].append(item)
            if item["mount_cleanup"] not in ("not_mounted", "detached"):
                break  # Do not compound an uncertain mount or incomplete tool operation.
        report["status"] = "passed" if len(report["artifacts"]) == 2 and all(x["status"] == "passed" for x in report["artifacts"]) else "unproven"
    except Exception:
        report["code"] = "inspection_error_unproven"
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(report, output, sort_keys=True, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
    except Exception:
        print("report_write_failed", file=sys.stderr)
        return 2
    print("fresh Google artifact inspection: " + report["status"])
    return 0 if report["status"] == "passed" else 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
