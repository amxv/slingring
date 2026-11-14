#!/usr/bin/env python3
"""Offline-only archive verifier/stager. Never downloads, executes or installs.

The pin must come from independent source/artifact review, not the archive's
self-generated checksum sidecars. No pin for a supported release exists yet.
All staged files remain non-executable; the production admission gate is closed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tarfile

MANIFEST = 'SLINGRING_LAUNCH_CAPABILITIES.json'
BINARIES = {'agent-browser-browse-native', 'slingring-owned-launch-guard'}
MEMBERS = BINARIES | {MANIFEST, 'LICENSE.upstream', 'UPSTREAM_VERSION', 'UPSTREAM_COMMIT', 'PATCH_SHA256'}
SOURCES = {'upstream_version', 'upstream_commit', 'fork_baseline_commit',
           'lifecycle_patch_sha256', 'guardian_cargo_lock_sha256'}
CONTRACT = {'schema': 1, 'protocol': 'slingring-launch-lease-v1',
            'platform': 'darwin-arm64', 'artifact_scope': 'offline_fixture_candidate',
            'cleanup_evidence': 'root_reaped_tree_unverified'}
PIN_KEYS = SOURCES | {'schema', 'archive_sha256', 'manifest_sha256'}
MAX_ARCHIVE = 256 * 1024 * 1024
MAX_BINARY = 256 * 1024 * 1024
MAX_METADATA = 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024


def require(condition, code):
    if not condition:
        raise ValueError(code)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate_json_field')
        result[key] = value
    return result


def parse(data):
    value = json.loads(data, object_pairs_hook=unique_object)
    require(isinstance(value, dict), 'invalid_json_object')
    return value


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':')) + '\n').encode()


def bounded_regular(path, limit):
    # Open once without following a final symlink; verify the opened descriptor.
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, 'rb') as source:
        meta = os.fstat(source.fileno())
        require(stat.S_ISREG(meta.st_mode) and meta.st_size <= limit, 'invalid_input_file')
        data = source.read(limit + 1)
        require(len(data) <= limit, 'input_too_large')
        return data


def verify(archive, pin_path):
    pin = parse(bounded_regular(pin_path, MAX_METADATA))
    require(set(pin) == PIN_KEYS and type(pin['schema']) is int and pin['schema'] == 1,
            'invalid_pin_schema')
    for key in ['archive_sha256', 'manifest_sha256', 'lifecycle_patch_sha256', 'guardian_cargo_lock_sha256']:
        require(isinstance(pin[key], str) and re.fullmatch('[0-9a-f]{64}', pin[key]), 'invalid_pin_digest')
    for key in ['upstream_commit', 'fork_baseline_commit']:
        require(isinstance(pin[key], str) and re.fullmatch('[0-9a-f]{40}', pin[key]), 'invalid_pin_commit')
    require(isinstance(pin['upstream_version'], str) and
            re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', pin['upstream_version']), 'invalid_pin_version')
    data = bounded_regular(archive, MAX_ARCHIVE)
    require(digest(data) == pin['archive_sha256'], 'archive_checksum_mismatch')
    # Parse the same bytes that were hashed, not a reopened mutable pathname.
    import io
    contents = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as bundle:
        for member in bundle:
            require(member.name in MEMBERS and member.name not in contents, 'unexpected_or_duplicate_member')
            require(member.isfile() and not member.pax_headers, 'unsafe_archive_member')
            limit = MAX_BINARY if member.name in BINARIES else MAX_METADATA
            require(0 <= member.size <= limit, 'archive_member_too_large')
            total += member.size
            require(total <= MAX_TOTAL, 'archive_too_large')
            source = bundle.extractfile(member)
            require(source is not None, 'missing_archive_data')
            value = source.read(limit + 1)
            require(len(value) == member.size, 'invalid_archive_data')
            contents[member.name] = value
    require(set(contents) == MEMBERS, 'missing_archive_member')
    raw = contents[MANIFEST]
    require(digest(raw) == pin['manifest_sha256'], 'manifest_checksum_mismatch')
    manifest = parse(raw)
    require(set(manifest) == set(CONTRACT) | SOURCES | {'binaries'}, 'invalid_manifest_schema')
    require(canonical(manifest) == raw, 'noncanonical_manifest')
    require(type(manifest['schema']) is int, 'invalid_manifest_schema')
    require(all(manifest[k] == v for k, v in CONTRACT.items()), 'unsupported_capability')
    require(all(manifest[k] == pin[k] for k in SOURCES), 'source_pin_mismatch')
    require(isinstance(manifest['binaries'], dict) and set(manifest['binaries']) == BINARIES,
            'invalid_binary_manifest')
    for name in BINARIES:
        require(digest(contents[name]) == manifest['binaries'][name], 'binary_checksum_mismatch')
    for name, key in [('UPSTREAM_VERSION', 'upstream_version'), ('UPSTREAM_COMMIT', 'upstream_commit'),
                      ('PATCH_SHA256', 'lifecycle_patch_sha256')]:
        require(contents[name] == (manifest[key] + '\n').encode(), 'source_sidecar_mismatch')
    return contents


def stage(archive, pin, destination):
    contents = verify(archive, pin)
    destination = Path(destination)
    require(destination.is_absolute(), 'destination_must_be_absolute')
    parent = destination.parent.resolve(strict=True)
    require(parent == destination.parent and destination.name not in ('', '.', '..'), 'unclean_destination')
    meta = parent.stat()
    require(meta.st_uid == os.getuid() and meta.st_mode & 0o077 == 0, 'destination_parent_not_private')
    # Exclusive creation: never replace a prior installation, directory or link.
    destination.mkdir(mode=0o700)
    created = []
    try:
        for name, content in sorted(contents.items()):
            target = destination / name
            fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            created.append(target)
            with os.fdopen(fd, 'wb') as output:
                output.write(content)
                output.flush()
                os.fsync(output.fileno())
        fd = os.open(destination, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    except BaseException:
        for target in created:
            target.unlink()
        destination.rmdir()
        raise
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--pin', required=True, type=Path)
    parser.add_argument('--destination', required=True, type=Path)
    args = parser.parse_args()
    try:
        stage(args.archive, args.pin, args.destination)
    except (ValueError, OSError, tarfile.TarError) as error:
        # Do not print raw archive members or attacker-controlled parser errors.
        code = str(error) if type(error) is ValueError else type(error).__name__
        if not re.fullmatch('[A-Za-z_]+', code):
            code = 'invalid_package'
        parser.exit(1, f'offline candidate rejected: {code}\n')
    print('Offline fixture candidate staged non-executable; not installed or approved for production.')


if __name__ == '__main__':
    main()
