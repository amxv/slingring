#!/usr/bin/env python3
"""Local synthetic archives only. No browser, network, installation or execution."""
import copy
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import stage_owned_helper_candidate as candidate


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='slingring-offline-package-')
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        self.archive = self.root / 'candidate.tar.gz'
        self.pin_path = self.root / 'approved-fixture-pin.json'
        self.destination = self.root / 'staged'
        self.source = {'upstream_version': '0.41.1', 'upstream_commit': '1' * 40,
                       'fork_baseline_commit': '2' * 40, 'lifecycle_patch_sha256': '3' * 64,
                       'guardian_cargo_lock_sha256': '4' * 64}
        self.contents = {
            'agent-browser-browse-native': b'#!/bin/sh\necho must-not-execute\n',
            'slingring-owned-launch-guard': b'#!/bin/sh\necho must-not-execute\n',
            'LICENSE.upstream': b'fixture license\n', 'UPSTREAM_VERSION': b'0.41.1\n',
            'UPSTREAM_COMMIT': b'1' * 40 + b'\n', 'PATCH_SHA256': b'3' * 64 + b'\n'}
        self.manifest = {**candidate.CONTRACT, **self.source,
                         'binaries': {name: candidate.digest(self.contents[name]) for name in candidate.BINARIES}}
        self.contents[candidate.MANIFEST] = candidate.canonical(self.manifest)
        self.pin = {'schema': 1, **self.source}
        self.write()

    def tearDown(self):
        self.temp.cleanup()

    def write(self, entries=None, repin_manifest=True):
        if entries is None:
            entries = [(name, data, tarfile.REGTYPE) for name, data in sorted(self.contents.items())]
        with tarfile.open(self.archive, 'w:gz', format=tarfile.USTAR_FORMAT) as bundle:
            for name, data, kind in entries:
                info = tarfile.TarInfo(name)
                info.type = kind
                info.mode = 0o755
                info.size = len(data) if kind == tarfile.REGTYPE else 0
                if kind != tarfile.REGTYPE:
                    info.linkname = 'agent-browser-browse-native'
                bundle.addfile(info, io.BytesIO(data) if info.size else None)
        self.pin['archive_sha256'] = candidate.digest(self.archive.read_bytes())
        if repin_manifest:
            self.pin['manifest_sha256'] = candidate.digest(self.contents[candidate.MANIFEST])
        self.pin_path.write_bytes(candidate.canonical(self.pin))

    def verify(self):
        return candidate.verify(self.archive, self.pin_path)

    def test_stage_is_private_nonexecutable_and_does_not_install(self):
        candidate.stage(self.archive, self.pin_path, self.destination)
        self.assertEqual({p.name for p in self.destination.iterdir()}, candidate.MEMBERS)
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o700)
        for p in self.destination.iterdir():
            self.assertEqual(p.stat().st_mode & 0o777, 0o600)
            self.assertEqual(p.read_bytes(), self.contents[p.name])
        self.assertFalse((self.root / 'browser').exists())
        self.assertFalse((self.destination / 'agent-browser-browse').exists())

    def test_archive_checksum_is_independent_of_sidecar_claims(self):
        self.archive.write_bytes(self.archive.read_bytes() + b'tampered')
        with self.assertRaisesRegex(ValueError, 'archive_checksum_mismatch'):
            self.verify()

    def test_manifest_digest_is_required_even_when_archive_hash_matches(self):
        self.pin['manifest_sha256'] = '0' * 64
        self.pin_path.write_bytes(candidate.canonical(self.pin))
        with self.assertRaisesRegex(ValueError, 'manifest_checksum_mismatch'):
            self.verify()

    def test_each_binary_hash_is_checked_after_archive_verification(self):
        for name in candidate.BINARIES:
            with self.subTest(name=name):
                original = self.contents[name]
                self.contents[name] += b'tamper'
                self.write()
                with self.assertRaisesRegex(ValueError, 'binary_checksum_mismatch'):
                    self.verify()
                self.contents[name] = original

    def test_protocol_platform_scope_and_cleanup_cannot_be_self_attested(self):
        original = copy.deepcopy(self.manifest)
        for key, bad in [('protocol', 'v2'), ('platform', 'linux-arm64'),
                         ('artifact_scope', 'production'), ('cleanup_evidence', 'tree_gone'), ('schema', True)]:
            with self.subTest(key=key):
                self.manifest = {**original, key: bad}
                self.contents[candidate.MANIFEST] = candidate.canonical(self.manifest)
                self.write()
                with self.assertRaises(ValueError):
                    self.verify()

    def test_exact_source_and_version_pins_are_required(self):
        for key in candidate.SOURCES:
            with self.subTest(key=key):
                manifest = {**self.manifest, key: '9' if key == 'upstream_version' else '9' * len(self.source[key])}
                self.contents[candidate.MANIFEST] = candidate.canonical(manifest)
                self.write()
                with self.assertRaisesRegex(ValueError, 'source_pin_mismatch'):
                    self.verify()

    def test_sidecar_source_identity_must_match_manifest(self):
        for name in ['UPSTREAM_VERSION', 'UPSTREAM_COMMIT', 'PATCH_SHA256']:
            with self.subTest(name=name):
                original = self.contents[name]
                self.contents[name] = b'wrong\n'
                self.write()
                with self.assertRaisesRegex(ValueError, 'source_sidecar_mismatch'):
                    self.verify()
                self.contents[name] = original

    def test_missing_extra_duplicate_and_traversal_members_are_rejected(self):
        entries = [(name, data, tarfile.REGTYPE) for name, data in self.contents.items()]
        variants = [entries[:-1], entries + [entries[0]], entries + [('extra', b'x', tarfile.REGTYPE)],
                    entries + [('../escape', b'x', tarfile.REGTYPE)], entries + [('/escape', b'x', tarfile.REGTYPE)]]
        for altered in variants:
            with self.subTest(names=[entry[0] for entry in altered]):
                self.write(altered)
                with self.assertRaises(ValueError):
                    self.verify()

    def test_links_are_never_extracted(self):
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE]:
            entries = [(name, data, kind if name == 'slingring-owned-launch-guard' else tarfile.REGTYPE)
                       for name, data in self.contents.items()]
            self.write(entries)
            with self.assertRaisesRegex(ValueError, 'unsafe_archive_member'):
                self.verify()

    def test_unknown_missing_duplicate_or_noncanonical_json_is_rejected(self):
        raw = self.contents[candidate.MANIFEST]
        variants = [raw.replace(b'{', b'{"schema":1,', 1),
                    candidate.canonical({**self.manifest, 'unknown': True}),
                    candidate.canonical({k: v for k, v in self.manifest.items() if k != 'protocol'}),
                    json.dumps(self.manifest, indent=2).encode()]
        for altered in variants:
            self.contents[candidate.MANIFEST] = altered
            self.write()
            with self.assertRaises(ValueError):
                self.verify()

    def test_untrusted_pin_cannot_omit_expected_fields(self):
        self.pin.pop('manifest_sha256')
        self.pin_path.write_bytes(candidate.canonical(self.pin))
        with self.assertRaisesRegex(ValueError, 'invalid_pin_schema'):
            self.verify()

    def test_existing_destination_is_preserved(self):
        self.destination.mkdir()
        marker = self.destination / 'prior-installation'
        marker.write_text('untouched')
        with self.assertRaises(FileExistsError):
            candidate.stage(self.archive, self.pin_path, self.destination)
        self.assertEqual(marker.read_text(), 'untouched')
        self.assertEqual(list(self.destination.iterdir()), [marker])

    def test_symlink_inputs_and_destination_are_rejected(self):
        original = self.root / 'real-archive'
        self.archive.rename(original)
        self.archive.symlink_to(original)
        with self.assertRaises(OSError):
            self.verify()
        self.archive.unlink()
        original.rename(self.archive)
        self.destination.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            candidate.stage(self.archive, self.pin_path, self.destination)

    def test_nonprivate_parent_is_rejected_before_writing(self):
        self.root.chmod(0o755)
        with self.assertRaisesRegex(ValueError, 'destination_parent_not_private'):
            candidate.stage(self.archive, self.pin_path, self.destination)
        self.assertFalse(self.destination.exists())

    def test_size_limits_are_enforced_before_staging(self):
        with patch.object(candidate, 'MAX_BINARY', 4):
            with self.assertRaisesRegex(ValueError, 'archive_member_too_large'):
                candidate.stage(self.archive, self.pin_path, self.destination)
        self.assertFalse(self.destination.exists())

    def test_legacy_wrapper_contract_is_unchanged(self):
        source = (Path(__file__).parent / 'install-browser.sh').read_text()
        self.assertIn('version=0.41.1\n', source)
        self.assertIn('export AGENT_BROWSER_NAMESPACE=slingring\n', source)
        self.assertIn('exec "$base/0.41.1/agent-browser-browse-native" "$@"', source)
        self.assertNotIn(candidate.MANIFEST, source)


if __name__ == '__main__':
    unittest.main()
