#!/usr/bin/env python3
"""Linux installer fixtures: isolated homes, fake downloads, no browser/systemd."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
CLI_PREFIX = "slingring-x86_64-unknown-linux-gnu"
CLI = CLI_PREFIX + ".tar.xz"

# A shell stub supplies CLI output; the fixture compiler supplies genuine ELF
# so architecture checks exercise real bytes. It only executes the stub.
EXEC_STUB = r"""
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef FIXTURE_KEY
#define FIXTURE_KEY "CLI_FIXTURE"
#endif
int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "fixture-daemon") == 0) {
        const char *stop = getenv("FIXTURE_STOP");
        if (stop == NULL) return 98;
        for (int i = 0; i < 3000 && access(stop, F_OK) != 0; ++i) usleep(10000);
        usleep(350000);
        return 0;
    }
    argv[0] = getenv(FIXTURE_KEY);
    if (argv[0] != NULL) execv(argv[0], argv);
    return 99;
}
"""

OLD_CLI = """#!/bin/sh
set -eu
case "$1 ${2:-}" in
  "--version ") echo 'slingring 0.6.7';;
  "status --json") cat "$FIXTURES/status";;
  *) echo unexpected-old-cli-call >> "$EVENT_LOG"; exit 1;;
esac
"""

NEW_CLI = """#!/bin/sh
set -eu
case "$1 ${2:-}" in
  "--version ") echo 'slingring 0.7.0';;
  "browser install") echo browser-install >> "$EVENT_LOG";;
  *) echo unexpected-new-cli-call >> "$EVENT_LOG"; exit 1;;
esac
"""

IDLE = '{"active_tasks":0,"browser_busy":false,"queued_tasks":0,"running":false,"starting_tasks":0,"status_available":true}'


@unittest.skipUnless(os.uname().sysname == "Linux" and os.uname().machine == "x86_64", "Linux x86_64 fixtures")
class LinuxInstallers(unittest.TestCase):
    def setUp(self):
        self.base = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="slingring-linux-install-")))
        self.fixtures = self.base / "fixtures"
        self.tools = self.base / "tools"
        self.home = self.base / "home with spaces"
        for directory in (self.fixtures, self.tools, self.home):
            directory.mkdir()
        self.install = self.home / "bin"
        self.state = self.home / "state"
        self.log = self.base / "events"
        self.env = {
            "HOME": str(self.home), "TMPDIR": str(self.base), "FIXTURES": str(self.fixtures),
            "PATH": str(self.tools) + ":/usr/bin:/bin", "EVENT_LOG": str(self.log),
            "SLINGRING_HOME": str(self.state), "CLI_INSTALL_DIR": str(self.install),
            "CLI_VERSION": "0.7.0",
        }
        self.stub("getconf", '[ "$1" = GNU_LIBC_VERSION ]; echo "glibc 2.39"\n')
        self.stub("gh", 'exit 1\n')  # Deliberately exercise the anonymous path first.
        self.stub("curl", """
url= out=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift 2 ;;
    https://*) url=$1; shift ;;
    *) shift ;;
  esac
done
printf '%s\\n' "$url" >> "$EVENT_LOG"
test -n "$out"
cp "$FIXTURES/${url##*/}" "$out"
""")
        self.stub("systemctl", 'echo systemctl-must-not-run >> "$EVENT_LOG"; exit 73\n')

    def stub(self, name, text):
        self.write_executable(self.tools / name, "#!/bin/sh\nset -eu\n" + text)

    @staticmethod
    def write_executable(path, text):
        path.write_text(text)
        path.chmod(0o755)

    def elf(self, key="CLI_FIXTURE"):
        exe = self.base / key
        if not exe.exists():
            compiler = shutil.which("cc")
            self.assertIsNotNone(compiler, "Linux installer fixtures require cc")
            subprocess.run([compiler, "-Wall", "-Werror", f'-DFIXTURE_KEY="{key}"', "-x", "c", "-", "-o", str(exe)],
                           input=EXEC_STUB, text=True, check=True)
        return exe.read_bytes()

    @staticmethod
    def archive(path, mode, entries):
        with tarfile.open(path, mode) as stream:
            for name, data in entries:
                member = tarfile.TarInfo(name)
                if isinstance(data, str):
                    member.type = tarfile.SYMTYPE
                    member.linkname = data
                    stream.addfile(member)
                else:
                    member.mode = 0o755
                    member.size = len(data)
                    stream.addfile(member, io.BytesIO(data))
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def cli_fixture(self, entries=None, good_hash=True):
        cli_fixture = self.base / "new-cli"
        self.write_executable(cli_fixture, NEW_CLI)
        self.env["CLI_FIXTURE"] = str(cli_fixture)
        if entries is None:
            entries = [(CLI_PREFIX + "/slingring", self.elf()), (CLI_PREFIX + "/LICENSE", b"fixture")]
        digest = self.archive(self.fixtures / CLI, "w:xz", entries)
        (self.fixtures / (CLI + ".sha256")).write_text((digest if good_hash else "0" * 64) + "\n")

    def run_script(self, script, arg):
        return subprocess.run(["/bin/sh", str(script), arg], env=self.env, text=True,
                              capture_output=True, timeout=30)

    def run_cli(self, action="install"):
        return self.run_script(ROOT / "docs/public/install.sh", action)

    def old_cli(self, status=IDLE):
        self.install.mkdir()
        binary = self.install / "slingring"
        self.write_executable(binary, OLD_CLI)
        (self.fixtures / "status").write_text(status)
        return binary

    def test_linux_installers_reject_older_or_unproven_glibc_before_any_action(self):
        for libc in ("glibc 2.9", "glibc 2.38", "musl 1.2", "glibc unavailable", ""):
            with self.subTest(libc=libc):
                self.stub("getconf", f"printf '%s\\n' '{libc}'\n")
                for result in (
                    self.run_cli(),
                    self.run_cli("--check"),
                ):
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("requires glibc 2.39 or newer", result.stderr)
                self.assertFalse(self.log.exists())
                self.assertFalse(self.install.exists())
                self.assertFalse(self.state.exists())

    def test_linux_cli_nested_release_and_no_registration(self):
        self.cli_fixture()
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.install / "slingring").read_bytes(), self.elf())
        self.assertFalse((self.install / "Slingring.app").exists())
        self.assertEqual(self.log.read_text().splitlines()[-1], "browser-install")
        self.assertNotIn("systemctl", self.log.read_text())

    def test_linux_cli_flat_private_release(self):
        self.cli_fixture([("slingring", self.elf()), ("README.md", b"readme")])
        self.stub("curl", "exit 77\n")
        self.stub("gh", """
case "$1 $2" in
  "auth status") exit 0;;
  "release download")
    shift 2
    [ "$1" = v0.7.0 ]; shift
    while [ "$#" -gt 0 ]; do
      case "$1" in --pattern) name=$2;; --output) output=$2;; esac
      shift 2
    done
    cp "$FIXTURES/$name" "$output";;
  *) exit 82;;
esac
""")
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_linux_cli_bad_hash_preserves_install(self):
        old = self.old_cli()
        self.cli_fixture(good_hash=False)
        result = self.run_cli("upgrade")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checksum mismatch", result.stderr)
        self.assertEqual(old.read_text(), OLD_CLI)

    def test_linux_cli_rejects_non_elf(self):
        self.cli_fixture([("slingring", b"#!/bin/sh\nexit 0\n")])
        result = self.run_cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not x86_64 ELF", result.stderr)
        self.assertFalse((self.install / "slingring").exists())

    def test_linux_cli_rejects_unsafe_layouts(self):
        for entries in (
            [("slingring", self.elf()), ("../escape", b"bad")],
            [("slingring", self.elf()), ("slingring", self.elf())],
            [("slingring", "/bin/true")],
            [("slingring", self.elf()), (CLI_PREFIX + "/README.md", b"bad")],
            [("slingring", self.elf()), ("unexpected README.md", b"bad")],
        ):
            with self.subTest(names=[name for name, _ in entries]):
                self.cli_fixture(entries)
                result = self.run_cli()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.install / "slingring").exists())

    def test_linux_cli_idle_upgrade(self):
        old = self.old_cli()
        self.cli_fixture()
        result = self.run_cli("upgrade")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(old.read_bytes(), self.elf())
        self.assertNotIn("unexpected-old-cli-call", self.log.read_text())

    def test_linux_cli_failed_postinstall_verification_restores_previous(self):
        old = self.old_cli()
        self.cli_fixture()
        self.write_executable(Path(self.env["CLI_FIXTURE"]), NEW_CLI.replace(
            "echo 'slingring 0.7.0';;",
            """if [ -e "$FIXTURES/version-seen" ]; then exit 1; fi
    touch "$FIXTURES/version-seen"; echo 'slingring 0.7.0';;"""))
        result = self.run_cli("upgrade")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("restoring previous installation", result.stderr)
        self.assertEqual(old.read_text(), OLD_CLI)

    def test_linux_cli_waits_for_exact_fixture_daemon_and_restores_service(self):
        old = self.old_cli()
        old.write_bytes(self.elf("CLI_OLD_FIXTURE"))
        old_stub = self.base / "old-cli"
        self.write_executable(old_stub, OLD_CLI.replace(
            '  *) echo unexpected-old-cli-call',
            """  "service stop") echo fixture-stop >> "$EVENT_LOG";
    cp "$FIXTURES/stopped" "$FIXTURES/status"; touch "$FIXTURE_STOP";;
  *) echo unexpected-old-cli-call"""))
        self.env["CLI_OLD_FIXTURE"] = str(old_stub)
        self.env["FIXTURE_STOP"] = str(self.base / "stop-fixture")
        (self.fixtures / "stopped").write_text(IDLE)
        self.cli_fixture()
        self.write_executable(Path(self.env["CLI_FIXTURE"]), NEW_CLI.replace(
            "  *) echo unexpected-new-cli-call",
            """  "service start") echo fixture-start >> "$EVENT_LOG";;
  *) echo unexpected-new-cli-call"""))
        with subprocess.Popen([str(old), "fixture-daemon"], env=self.env) as fixture:
            try:
                (self.fixtures / "status").write_text(IDLE.replace(
                    '"running":false', f'"running":true,"pid":{fixture.pid}'))
                result = self.run_cli("upgrade")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(fixture.poll(), 0, "must wait for the owned fixture executable to exit")
            finally:
                Path(self.env["FIXTURE_STOP"]).touch(exist_ok=True)
        self.assertEqual(self.log.read_text().splitlines()[-3:],
                         ["fixture-stop", "browser-install", "fixture-start"])

    def test_linux_cli_indeterminate_or_busy_upgrade_preserves_cli_and_helper(self):
        old = self.old_cli()
        self.cli_fixture()
        for status in (
            IDLE.replace('"status_available":true', '"status_available":false'),
            IDLE.replace('"active_tasks":0', '"active_tasks":1'),
            IDLE.replace('"queued_tasks":0', '"queued_tasks":false'),
            IDLE.replace('"browser_busy":false', '"browser_busy":true'),
            IDLE.replace('"running":false', '"running":true,"pid":1'),
            '{"running":false}',
            IDLE.replace('"running":false', '"running":true,"running":false'),
        ):
            with self.subTest(status=status):
                (self.fixtures / "status").write_text(status)
                result = self.run_cli("upgrade")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(old.read_text(), OLD_CLI)
                self.assertNotIn("browser-install", self.log.read_text())
                self.assertNotIn("unexpected-old-cli-call", self.log.read_text())

    def test_standalone_patched_helper_installer_cannot_install_on_linux(self):
        result = self.run_script(ROOT / "scripts/install-browser.sh", str(self.state))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Apple Silicon Macs only", result.stderr)
        self.assertFalse(self.log.exists())
        self.assertFalse(self.state.exists())


if __name__ == "__main__":
    unittest.main()
