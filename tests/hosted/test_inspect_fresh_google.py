"""Pure/disposable-file fixtures only: no network, mounts, signatures or browsers."""
import contextlib
import dataclasses
import io
import json
import os
from pathlib import Path
import plistlib
import shutil
import tempfile
import unittest
from unittest import mock

import inspect_fresh_google as subject


def make_bundle(directory, spec):
    bundle = directory / spec.app
    (bundle / "Contents" / "MacOS").mkdir(parents=True)
    (bundle / "Contents" / "MacOS" / spec.executable).write_bytes(b"fixture data, not an executable")
    (bundle / "Contents" / "Info.plist").write_bytes(plistlib.dumps({
        "CFBundleIdentifier": spec.identifier, "CFBundleExecutable": spec.executable,
        "CFBundleShortVersionString": "123.0.4567.89"}))
    return bundle


def receipt_data(mount, *, device="/dev/disk99"):
    return plistlib.dumps({"system-entities": [{"dev-entry": device},
                         {"dev-entry": device + "s1", "mount-point": str(mount)}]})


class FakeCurl:
    def __init__(self, data=b"fixture", result=None, version=b"curl 8.7.1 fixture"):
        self.data, self.result, self.version, self.calls = data, result or subject.Result(0, b"200"), version, []

    def run(self, args, **kwargs):
        self.calls.append((args, kwargs))
        if args[-1] == "--version":
            return subject.Result(0, self.version)
        os.write(kwargs["pass_fds"][0], self.data)
        return self.result


class FakeCommands:
    def __init__(self, spec):
        self.spec, self.calls, self.image, self.mount = spec, [], None, None
        self.fail, self.timeout, self.attach_status = None, None, 0
        self.wrong_info, self.wrong_bundle = False, False

    def fetch(self, spec, target):
        target.write_bytes(b"fixture disk image, not mountable")
        return {"bytes": 32, "sha256": "a" * 64, "digest_scope": "observed_bytes_not_publisher_proof"}

    def run(self, args):
        self.calls.append(args)
        if args[0] == "/usr/bin/hdiutil":
            if args[1] == "verify":
                return subject.Result(0)
            if args[1] == "attach":
                self.mount, self.image = Path(args[args.index("-mountpoint") + 1]), Path(args[-1])
                source = make_bundle(self.mount, self.spec)
                if self.wrong_bundle:
                    info = source / "Contents" / "Info.plist"
                    value = plistlib.loads(info.read_bytes())
                    value["CFBundleIdentifier"] = "com.google.Chrome"  # Wrong for SxS Beta.
                    info.write_bytes(plistlib.dumps(value))
                return subject.Result(self.attach_status, receipt_data(self.mount))
            if args[1] == "info":
                data = plistlib.loads(receipt_data(self.mount))
                data["image-path"] = "/not/our/image" if self.wrong_info else str(self.image)
                return subject.Result(0, plistlib.dumps({"images": [data]}))
            if args[1] == "detach":
                return subject.Result(1 if self.fail == "detach" else 0)
        if args[0] == "/usr/bin/ditto":
            if self.timeout == "copy":
                return subject.Result(None, complete=False, timed_out=True)
            shutil.copytree(args[-2], args[-1])
            return subject.Result(0)
        if args[0] == "/usr/bin/codesign":
            role = "publisher" if "-R" in args else "integrity"
            if self.fail == role:
                return subject.Result(1, b"private path and diagnostic must never be exported")
            return subject.Result(0)
        if args[0] == "/usr/sbin/spctl":
            if args[1] == "--status":
                return subject.Result(0, b"assessments disabled\n" if self.fail == "disabled" else b"assessments enabled\n")
            return subject.Result(1 if self.fail == "policy" else 0)
        if args[0] == "/usr/bin/lipo":
            return subject.Result(0, b"x86_64\n" if self.fail == "arch" else b"x86_64 arm64\n")
        if args[0] == "/usr/bin/xattr":
            assert args[1:3] == ["-px", "com.apple.quarantine"]
            return subject.Result(1, b"No such xattr: com.apple.quarantine")
        raise AssertionError("unexpected command")


class InspectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()

    def tearDown(self):
        self.temp.cleanup()

    def inspect(self, *, spec=None, configure=None, readonly=True):
        spec = spec or subject.SPECS[0]
        fake = FakeCommands(spec)
        if configure:
            configure(fake)
        result = subject.inspect_one(spec, self.root, run=fake.run, fetch=fake.fetch,
                                     readonly=lambda _: readonly)
        return result, fake

    def test_context_is_fail_closed(self):
        env = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_OS": "macOS",
               "RUNNER_ARCH": "ARM64", "GITHUB_REPOSITORY": "amxv/slingring", "GITHUB_EVENT_NAME": "workflow_dispatch",
               "SLINGRING_FRESH_GOOGLE_INSPECTION": "explicitly-authorized"}
        kwargs = dict(system="Darwin", machine="arm64", user="runner")
        self.assertTrue(subject.hosted(env, **kwargs))
        for key in env:
            bad = dict(env)
            bad[key] = "wrong"
            self.assertFalse(subject.hosted(bad, **kwargs), key)
        self.assertFalse(subject.hosted(env, system="Darwin", machine="arm64", user="owner"))

    def test_local_cli_rejects_before_any_side_effect(self):
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(subject, "download") as download, \
                mock.patch.object(subject, "command") as command, mock.patch.object(subject.tempfile, "mkdtemp") as mkdir, \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(subject.main(["fixture", "--run-hosted", "/not/used"]), 2)
            download.assert_not_called()
            command.assert_not_called()
            mkdir.assert_not_called()

    def test_no_launch_cli(self):
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(subject.main(["fixture", "--launch", "/not/used"]), 2)

    def test_literal_sources_and_exact_publisher(self):
        for spec in subject.SPECS:
            self.assertTrue(spec.url.startswith("https://dl.google.com/chrome/mac/universal/"))
            requirement = subject.publisher_requirement(spec)
            for token in (spec.identifier, "anchor apple generic", subject.TEAM, "1.2.840.113635.100.6.1.13", "1.2.840.113635.100.6.2.6"):
                self.assertIn(token, requirement)
        with self.assertRaises(subject.Unproven):
            subject.publisher_requirement(dataclasses.replace(subject.SPECS[0], identifier="anything"))

    def test_bounded_download_observes_digest_not_authenticity(self):
        fake = FakeCurl()
        result = subject.download(subject.SPECS[0], self.root / "file", run=fake.run)
        self.assertEqual(result["bytes"], 7)
        self.assertEqual(result["digest_scope"], "observed_bytes_not_publisher_proof")
        self.assertEqual(len(result["sha256"]), 64)
        self.assertEqual((self.root / "file").stat().st_mode & 0o777, 0o600)

    def test_redirect_and_failure_statuses_rejected(self):
        for status in (b"301", b"302", b"403", b"200\nextra private data"):
            fake = FakeCurl(result=subject.Result(0, status))
            with self.assertRaisesRegex(subject.Unproven, "download_response_unproven"):
                subject.download(subject.SPECS[0], self.root / status[:3].decode(), run=fake.run)

    def test_no_redirect_retry_config_or_tls_exception(self):
        fake = FakeCurl()
        subject.download(subject.SPECS[0], self.root / "file", run=fake.run)
        args, kwargs = fake.calls[-1]
        self.assertEqual(args[:2], ["/usr/bin/curl", "-q"])
        self.assertEqual(args[-1], subject.SPECS[0].url)
        for flag, value in (("--max-time", "180"), ("--max-filesize", str(subject.MAX_DOWNLOAD)),
                            ("--retry", "0"), ("--max-redirs", "0"), ("--proto", "=https")):
            self.assertEqual(args[args.index(flag) + 1], value)
        for forbidden in ("--location", "--follow", "--insecure", "--compressed", "--cookie", "--config"):
            self.assertNotIn(forbidden, args)
        self.assertEqual(kwargs["timeout"], 185)

    def test_download_wrong_spec_refused_before_network(self):
        run = mock.Mock()
        with self.assertRaises(subject.Unproven):
            subject.download(dataclasses.replace(subject.SPECS[0], url="https://example.org/fake"), self.root / "file", run=run)
        run.assert_not_called()

    def test_download_size_and_partial(self):
        for name, fake, maximum in (("size", FakeCurl(), 5), ("partial", FakeCurl(result=subject.Result(18)), 100)):
            with self.assertRaises(subject.Unproven):
                subject.download(subject.SPECS[0], self.root / name, run=fake.run, maximum=maximum)

    def test_download_existing_target_unchanged(self):
        target = self.root / "existing"
        target.write_bytes(b"preserve")
        fake = FakeCurl()
        with self.assertRaises(subject.Unproven):
            subject.download(subject.SPECS[0], target, run=fake.run)
        self.assertEqual(target.read_bytes(), b"preserve")
        self.assertEqual(len(fake.calls), 1)  # Version query only, no transfer.

    def test_download_drip_timeout_has_no_retry(self):
        fake = FakeCurl(result=subject.Result(28, b"timeout"))
        with self.assertRaisesRegex(subject.Unproven, "download_response_unproven"):
            subject.download(subject.SPECS[0], self.root / "file", run=fake.run)
        self.assertEqual(len(fake.calls), 2)

    def test_download_outer_timeout_stays_pending(self):
        fake = FakeCurl(result=subject.Result(None, complete=False, timed_out=True))
        with self.assertRaisesRegex(subject.Unproven, "download_command_pending"):
            subject.download(subject.SPECS[0], self.root / "file", run=fake.run)

    def test_download_client_requires_streamed_size_limit(self):
        fake = FakeCurl(version=b"curl 8.3.0 fixture")
        with self.assertRaisesRegex(subject.Unproven, "download_client_too_old"):
            subject.download(subject.SPECS[0], self.root / "file", run=fake.run)
        self.assertEqual(len(fake.calls), 1)

    def test_partial_or_truncated_capture_never_passes(self):
        for result in (subject.Result(0, complete=False), subject.Result(None, timed_out=True)):
            self.assertNotEqual(subject.category("codesign_integrity", result), "passed")

    def capture(self, chunks, clock=None):
        child = mock.Mock(returncode=0)
        child.poll.return_value = 0
        selector = mock.MagicMock()
        selector.__enter__.return_value = selector
        selector.select.return_value = [(mock.Mock(fd=10), 1)]
        with mock.patch.object(subject.subprocess, "Popen", return_value=child), \
                mock.patch.object(subject.selectors, "DefaultSelector", return_value=selector), \
                mock.patch.object(subject.os, "read", side_effect=chunks), \
                mock.patch.object(subject.time, "monotonic", side_effect=clock or [0] * 20):
            return subject.command(["/only/a/mock"])

    def test_capture_completes_only_with_eof(self):
        result = self.capture([b"ready", b""])
        self.assertTrue(result.complete)
        self.assertEqual(result.data, b"ready")

    def test_capture_limit_is_latched(self):
        result = self.capture([b"x" * subject.MAX_CAPTURE, b"hidden data", b""])
        self.assertFalse(result.complete)
        self.assertEqual(len(result.data), subject.MAX_CAPTURE)

    def test_capture_timeout_without_eof(self):
        result = self.capture([b"ready"], clock=[0, 0, 46])
        self.assertFalse(result.complete)
        self.assertTrue(result.timed_out)

    def test_capture_io_error_is_uncertain(self):
        result = self.capture([OSError("private details")])
        self.assertIsNone(result.status)
        self.assertFalse(result.complete)
        self.assertNotIn(b"private details", result.data)

    def test_assessment_status_requires_affirmative(self):
        for data in (b"", b"assessments disabled", b"assessments enabled\nprivate extra"):
            self.assertNotEqual(subject.category("assessment_enabled", subject.Result(0, data)), "passed")
        self.assertEqual(subject.category("assessment_enabled", subject.Result(0, b"assessments enabled\n")), "passed")

    def test_disabled_assessment_cannot_pass_on_zero(self):
        self.assertNotEqual(subject.category("execution_policy", subject.Result(0, b"assessment system is disabled")), "passed")
        self.assertNotEqual(subject.category("execution_policy", subject.Result(0, b"unable to obtain assessment")), "passed")

    def test_exact_mount_receipt(self):
        mount, image = self.root / "mount", self.root / "image"
        self.assertEqual(subject.mount_receipt(receipt_data(mount), mount, image).device, "/dev/disk99")

    def test_mount_ambiguity_and_escape(self):
        mount, image = self.root / "mount", self.root / "image"
        for value in (b"not plist", receipt_data(self.root / "other"),
                      plistlib.dumps({"system-entities": [{"dev-entry": "/dev/disk1"}, {"dev-entry": "/dev/disk2", "mount-point": str(mount)}]})):
            with self.assertRaises(subject.Unproven):
                subject.mount_receipt(value, mount, image)

    def test_duplicate_mounts_and_no_root_device_refused(self):
        mount, image = self.root / "mount", self.root / "image"
        value = plistlib.loads(receipt_data(mount))
        value["system-entities"].append({"dev-entry": "/dev/disk99s2", "mount-point": str(mount)})
        with self.assertRaises(subject.Unproven):
            subject.mount_receipt(plistlib.dumps(value), mount, image)
        with self.assertRaises(subject.Unproven):
            subject.mount_receipt(plistlib.dumps({"system-entities": [{"dev-entry": "/dev/disk99s1", "mount-point": str(mount)}]}), mount, image)

    def test_success_never_executes_downloaded_bytes(self):
        result, fake = self.inspect()
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["mount_cleanup"], "detached")
        self.assertEqual(result["browser_execution"], "not_run")
        self.assertEqual(result["first_download_gatekeeper_ui"], "not_tested")
        self.assertEqual({call[0] for call in fake.calls}, {"/usr/bin/hdiutil", "/usr/bin/ditto", "/usr/bin/codesign", "/usr/sbin/spctl", "/usr/bin/lipo", "/usr/bin/xattr"})
        self.assertNotIn(str(self.root), json.dumps(result))
        checks = [call for call in fake.calls if call[0] == "/usr/bin/codesign"]
        self.assertIn("--deep", checks[0])
        self.assertNotIn("--deep", checks[1])
        self.assertIn("-R", checks[1])

    def test_beta_is_distinct(self):
        result, _ = self.inspect(spec=subject.SPECS[1])
        self.assertEqual(result["bundle_id"], "com.google.Chrome.beta")

    def test_non_side_by_side_beta_rejected(self):
        result, _ = self.inspect(spec=subject.SPECS[1], configure=lambda f: setattr(f, "wrong_bundle", True))
        self.assertEqual(result["status"], "unproven")
        self.assertEqual(result["code"], "bundle_identity_unproven")

    def test_readwrite_mount_rejected(self):
        result, _ = self.inspect(readonly=False)
        self.assertEqual(result["code"], "mount_not_proven_readonly")
        self.assertEqual(result["mount_cleanup"], "detached")

    def test_failed_attach_with_exact_receipt_is_cleaned(self):
        result, fake = self.inspect(configure=lambda f: setattr(f, "attach_status", 1))
        self.assertEqual(result["status"], "unproven")
        self.assertEqual(result["mount_cleanup"], "detached")
        self.assertFalse(any(call[0] == "/usr/bin/ditto" for call in fake.calls))

    def test_foreign_device_never_detached(self):
        result, fake = self.inspect(configure=lambda f: setattr(f, "wrong_info", True))
        self.assertEqual(result["code"], "mount_cleanup_unproven")
        self.assertFalse(any(call[:2] == ["/usr/bin/hdiutil", "detach"] for call in fake.calls))

    def test_each_required_verification_fails_closed(self):
        for failure in ("integrity", "publisher", "disabled", "policy", "arch", "detach"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temp:
                original = self.root
                self.root = Path(temp).resolve()
                result, _ = self.inspect(configure=lambda f: setattr(f, "fail", failure))
                self.root = original
                self.assertEqual(result["status"], "unproven")
                self.assertNotIn("private path", json.dumps(result))

    def test_pending_copy_stops_without_racing_detach(self):
        result, fake = self.inspect(configure=lambda f: setattr(f, "timeout", "copy"))
        self.assertEqual(result["code"], "pending_tool_outcome_unproven")
        self.assertEqual(fake.calls[-1][0], "/usr/bin/ditto")

    def test_failed_signature_does_not_hide_other_read_only_probes(self):
        result, _ = self.inspect(configure=lambda f: setattr(f, "fail", "integrity"))
        labels = {item["probe"] for item in result["checks"]}
        self.assertTrue({"codesign_integrity", "codesign_google_publisher", "assessment_enabled", "execution_policy", "architecture"} <= labels)
        self.assertEqual(result["status"], "unproven")

    def test_escaping_main_executable_rejected(self):
        spec = subject.SPECS[0]
        bundle = make_bundle(self.root, spec)
        executable = bundle / "Contents" / "MacOS" / spec.executable
        executable.unlink()
        outside = self.root / "outside"
        outside.write_text("not an executable")
        executable.symlink_to(outside)
        with self.assertRaisesRegex(subject.Unproven, "nonregular_or_escaping_path"):
            subject.bundle_metadata(bundle, spec)

    def test_quarantine_marker_must_be_preserved(self):
        with mock.patch.object(subject, "quarantine_marker", side_effect=[b"original", b"changed"]):
            result, _ = self.inspect()
        self.assertEqual(result["code"], "quarantine_preservation_unproven")
        self.assertNotIn("original", json.dumps(result))

    def test_quarantine_read_only_and_bounded(self):
        report = {}
        run = mock.Mock(return_value=subject.Result(0, b"61 62 63\n"))
        self.assertEqual(subject.quarantine_marker(self.root, report, run=run), b"abc")
        self.assertEqual(run.call_args.args[0][:3], ["/usr/bin/xattr", "-px", "com.apple.quarantine"])
        for result in (subject.Result(0, b"xx"), subject.Result(1, b"permission denied"),
                       subject.Result(0, b"61" * 4097), subject.Result(0, b"", complete=False)):
            with self.assertRaises(subject.Unproven):
                subject.quarantine_marker(self.root, {}, run=lambda _: result)

    def test_quarantine_timeout_marks_pending_tool(self):
        report = {}
        with self.assertRaises(subject.Unproven):
            subject.quarantine_marker(self.root, report, run=lambda _: subject.Result(None, timed_out=True, complete=False))
        self.assertTrue(report["pending_tool"])

    def test_workflow_is_inspection_only_and_metadata_only(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/fresh-google-inspection.yml").read_text()
        self.assertIn("default: false", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("path: ${{ runner.temp }}/fresh-google-inspection.json", workflow)
        self.assertNotIn("secrets:", workflow)
        self.assertNotIn("--run-fresh-hosted", workflow)
        self.assertNotIn("browser_identity.swift", workflow)

    def test_ci_routes_are_default_off_and_mutually_exclusive(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/ci.yml").read_text()
        self.assertIn("approve_fresh_google_inspection:\n", workflow)
        self.assertIn("uses: ./.github/workflows/fresh-google-inspection.yml", workflow)
        self.assertIn("inputs.approve_fresh_google_inspection == true && inputs.approve_disposable_browser_test != true", workflow)
        self.assertIn("inputs.approve_disposable_browser_test == true && inputs.approve_fresh_google_inspection != true", workflow)
        self.assertIn("reject-conflicting-validation:", workflow)
        self.assertEqual(workflow.count("inputs.approve_disposable_browser_test != true && inputs.approve_fresh_google_inspection != true"), 2)


if __name__ == "__main__":
    unittest.main()
