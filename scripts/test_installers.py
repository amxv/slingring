#!/usr/bin/env python3
"""Offline installer regression tests. All writes go to temporary homes."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ASSET = 'agent-browser-browse-darwin-arm64.tar.gz'

class Installers(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='slingring-install-test-')
        self.base = Path(self.temp.name)
        self.bin = self.base / 'bin'; self.bin.mkdir()
        self.fixtures = self.base / 'fixtures'; self.fixtures.mkdir()
        self.home = self.base / 'home'; self.home.mkdir()
        self.state = self.home / 'state'
        self.env = {**os.environ, 'HOME':str(self.home), 'SLINGRING_HOME':str(self.state),
            'PATH':str(self.bin)+':/usr/bin:/bin:/usr/sbin:/sbin', 'FIXTURES':str(self.fixtures),
            'CLI_NO_LAUNCH':'1', 'CLI_INSTALL_DIR':str(self.home/'install'), 'EVENT_LOG':str(self.base/'events')}
        self.stub('uname', 'case "$1" in -s) echo Darwin;; -m) echo arm64;; esac')
        self.stub('codesign', 'exit 0')
        self.stub('lipo', 'echo arm64')
        self.stub('curl', '''url=; output=
while [ "$#" -gt 0 ]; do
 case "$1" in -o|--output) output=$2; shift 2;; https://*) url=$1; shift;; *) shift;; esac
done
[ -n "$output" ] && cp "$FIXTURES/${url##*/}" "$output"''')
    def tearDown(self): self.temp.cleanup()
    def stub(self, name, body):
        path=self.bin/name;path.write_text('#!/bin/sh\nset -eu\n'+body+'\n');path.chmod(0o755)
    def archive(self, path, entries):
        with tarfile.open(path,'w:gz') as tar:
            for name,body,mode in entries:
                value=body.encode();info=tarfile.TarInfo(name);info.size=len(value);info.mode=mode
                tar.addfile(info,io.BytesIO(value))
    def browser_fixture(self, bad_hash=False, malicious=False):
        path=self.fixtures/ASSET
        entries=[('./agent-browser-browse-native','#!/bin/sh\necho "agent-browser 0.41.1"\n',0o755),
                 ('./UPSTREAM_VERSION','0.41.1\n',0o644),('./LICENSE.upstream','test fixture',0o644)]
        if malicious: entries.append(('../escaped','bad',0o644))
        self.archive(path,entries)
        digest='0'*64 if bad_hash else hashlib.sha256(path.read_bytes()).hexdigest()
        (self.fixtures/'SHA256SUMS').write_text(f'{digest}  {ASSET}\n')
    def run_browser(self):
        return subprocess.run(['/bin/sh',str(ROOT/'scripts/install-browser.sh'),str(self.state)],env=self.env,capture_output=True,text=True,timeout=15)
    def test_browser_install_is_self_contained_and_repeatable(self):
        self.browser_fixture()
        result=self.run_browser();self.assertEqual(result.returncode,0,result.stderr)
        wrapper=self.state/'browser/agent-browser-browse'
        self.assertTrue(wrapper.is_file())
        self.assertIn('AGENT_BROWSER_NAMESPACE=slingring',wrapper.read_text())
        first=wrapper.stat().st_mtime_ns
        result=self.run_browser();self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual((self.state/'browser/0.41.1/UPSTREAM_VERSION').read_text().strip(),'0.41.1')
        self.assertGreaterEqual(wrapper.stat().st_mtime_ns,first)
    def test_bad_browser_checksum_preserves_existing_wrapper(self):
        self.browser_fixture(bad_hash=True)
        wrapper=self.state/'browser/agent-browser-browse';wrapper.parent.mkdir(parents=True);wrapper.write_text('original')
        result=self.run_browser();self.assertNotEqual(result.returncode,0)
        self.assertIn('checksum mismatch',result.stderr)
        self.assertEqual(wrapper.read_text(),'original')
        self.assertFalse((self.state/'browser/0.41.1').exists())
    def test_archive_traversal_is_rejected_before_extraction(self):
        self.browser_fixture(malicious=True)
        result=self.run_browser();self.assertNotEqual(result.returncode,0)
        self.assertIn('Unexpected browser archive member',result.stderr)
        self.assertFalse(any(self.base.rglob('escaped')))
    def test_intel_is_rejected_without_partial_install(self):
        self.stub('uname','case "$1" in -s) echo Darwin;; -m) echo x86_64;; esac')
        result=self.run_browser();self.assertNotEqual(result.returncode,0)
        self.assertIn('Apple Silicon',result.stderr)
        self.assertFalse((self.state/'browser').exists())
    def test_main_installer_installs_browser_before_cli_replacement(self):
        cli='''#!/bin/sh
set -eu
case "$1" in
 --version) echo 'slingring 0.2.0';;
 browser) echo browser-installed >> "$EVENT_LOG";;
 *) exit 1;;
esac
'''
        asset='slingring_darwin_arm64.tar.gz'
        self.archive(self.fixtures/asset,[('slingring',cli,0o755),('Slingring.app/Contents/MacOS/slingring-menubar','#!/bin/sh\nexit 0\n',0o755),('LICENSE','fixture',0o644)])
        (self.fixtures/(asset+'.sha256')).write_text(hashlib.sha256((self.fixtures/asset).read_bytes()).hexdigest()+'\n')
        self.stub('gh','''case "$1 $2" in
 "auth status") exit 0;;
 "api repos/amxv/slingring/releases/latest") echo v0.2.0; exit 0;;
 "release download")
 name=; output=; while [ "$#" -gt 0 ]; do case "$1" in --pattern) name=$2;shift 2;; --output) output=$2;shift 2;; *) shift;; esac;done
 cp "$FIXTURES/$name" "$output";;
 *) exit 1;; esac''')
        result=subprocess.run(['/bin/sh',str(ROOT/'docs/public/install.sh')],env=self.env,capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue((self.home/'install/slingring').is_file())
        self.assertEqual((self.base/'events').read_text().strip(),'browser-installed')
        self.assertIn('Chrome Profile',result.stdout)

if __name__=='__main__':unittest.main()
