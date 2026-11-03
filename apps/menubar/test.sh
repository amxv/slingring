#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/slingring-menubar-tests.XXXXXX")"
trap 'rm -rf "$test_dir"' EXIT
/usr/bin/swiftc "$root/apps/menubar/Sources/TaskPresentation.swift" \
  "$root/apps/menubar/Tests/main.swift" -o "$test_dir/status-tests"
"$test_dir/status-tests"
