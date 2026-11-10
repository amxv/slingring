#!/usr/bin/env python3
"""Offline installer regression tests. All writes go to temporary homes."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
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
        self.stub('codesign', '''case "$1" in
 --display) printf 'Identifier=com.amxv.slingring.fixture\\nSignature=adhoc\\n';;
 *) exit 0;;
esac''')
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
 login) echo "login-$2-${3:-}" >> "$EVENT_LOG";;
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
        self.assertEqual((self.base/'events').read_text().splitlines(),['browser-installed','login-setup-'])
        self.assertIn('Chrome Profile',result.stdout)

    def test_upgrade_calls_migration_without_resetting_saved_opt_outs(self):
        old_cli='''#!/bin/sh
set -eu
case "$1" in
 status) echo '{"running":false,"active_tasks":0}';;
 --version) echo 'slingring 0.5.1';;
 *) exit 1;;
esac
'''
        new_cli='''#!/bin/sh
set -eu
case "$1" in
 --version) echo 'slingring 0.6.0';;
 browser) echo browser-installed >> "$EVENT_LOG";;
 login)
   [ "$2" = setup ] && [ "${3:-}" = --upgrade ] || exit 1
   echo "login-setup-upgrade" >> "$EVENT_LOG"
   # A real CLI performs the migration. This fixture asserts the installer
   # cannot remove or overwrite existing preference data during replacement.
   [ -f "$SLINGRING_HOME/login-preferences.json" ] || exit 1;;
 *) exit 1;;
esac
'''
        target=self.home/'install/slingring';target.parent.mkdir()
        target.write_text(old_cli);target.chmod(0o755)
        app=target.parent/'Slingring.app/Contents/MacOS'
        app.mkdir(parents=True)
        (app/'slingring-menubar').write_text('#!/bin/sh\nexit 0\n')
        (app/'slingring-menubar').chmod(0o755)
        self.state.mkdir()
        saved='{"menu_bar":false,"service":false}'
        (self.state/'login-preferences.json').write_text(saved)
        asset='slingring_darwin_arm64.tar.gz'
        self.archive(self.fixtures/asset,[('slingring',new_cli,0o755),
            ('Slingring.app/Contents/MacOS/slingring-menubar','#!/bin/sh\nexit 0\n',0o755),
            ('LICENSE','fixture',0o644)])
        (self.fixtures/(asset+'.sha256')).write_text(hashlib.sha256((self.fixtures/asset).read_bytes()).hexdigest()+'\n')
        self.stub('gh','''case "$1 $2" in
 "auth status") exit 0;;
 "release download")
 name=; output=; while [ "$#" -gt 0 ]; do case "$1" in --pattern) name=$2;shift 2;; --output) output=$2;shift 2;; *) shift;; esac;done
 cp "$FIXTURES/$name" "$output";;
 *) exit 1;; esac''')
        env={**self.env,'CLI_VERSION':'0.6.0'}
        result=subprocess.run(['/bin/sh',str(ROOT/'docs/public/install.sh'),'upgrade'],env=env,capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('slingring 0.6.0',subprocess.run([str(target),'--version'],capture_output=True,text=True).stdout)
        self.assertEqual((self.state/'login-preferences.json').read_text(),saved)
        self.assertEqual((self.base/'events').read_text().splitlines(),['browser-installed','login-setup-upgrade'])

    def test_upgrade_waits_for_old_daemon_to_exit_before_replacing_its_executable(self):
        compiler=shutil.which('cc')
        if compiler is None: self.skipTest('Apple command-line tools are required for this macOS lifecycle test')
        target=self.home/'install/slingring';target.parent.mkdir()
        source=self.base/'daemon-fixture.c'
        source.write_text(r'''
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
  if (argc < 2) return 2;
  const char *root = getenv("FIXTURE_DAEMON_DIR");
  if (!root) return 2;
  char started[2048], stop[2048], finished[2048];
  snprintf(started, sizeof started, "%s/started", root);
  snprintf(stop, sizeof stop, "%s/stop", root);
  snprintf(finished, sizeof finished, "%s/finished", root);
  if (!strcmp(argv[1], "daemon")) {
    FILE *f = fopen(started, "w"); if (!f) return 2; fclose(f);
    while (access(stop, F_OK)) usleep(20000);
    usleep(800000); // Simulate shutdown continuing after the socket is gone.
    f = fopen(finished, "w"); if (!f) return 2; fclose(f);
    return 0;
  }
  if (!strcmp(argv[1], "status")) {
    puts("{\"running\":true,\"active_tasks\":0}");
    return 0;
  }
  if (!strcmp(argv[1], "service") && argc > 2 && !strcmp(argv[2], "stop")) {
    FILE *f = fopen(stop, "w"); if (!f) return 2; fclose(f);
    return 0;
  }
  if (!strcmp(argv[1], "--version")) { puts("slingring 0.6.0"); return 0; }
  return 2;
}
''')
        subprocess.run([compiler,'-Wall','-Werror',str(source),'-o',str(target)],check=True,capture_output=True)
        old_app=target.parent/'Slingring.app/Contents/MacOS'
        old_app.mkdir(parents=True)
        (old_app/'slingring-menubar').write_text('#!/bin/sh\nexit 0\n')
        (old_app/'slingring-menubar').chmod(0o755)
        next_cli='''#!/bin/sh
set -eu
case "$1" in
 --version) echo 'slingring 0.6.1';;
 browser) echo browser-installed >> "$EVENT_LOG";;
 login) echo login-setup >> "$EVENT_LOG";;
 *) exit 1;;
esac
'''
        asset='slingring_darwin_arm64.tar.gz'
        self.archive(self.fixtures/asset,[('slingring',next_cli,0o755),
            ('Slingring.app/Contents/MacOS/slingring-menubar','#!/bin/sh\nexit 0\n',0o755),
            ('LICENSE','fixture',0o644)])
        (self.fixtures/(asset+'.sha256')).write_text(hashlib.sha256((self.fixtures/asset).read_bytes()).hexdigest()+'\n')
        self.stub('gh','''case "$1 $2" in
 "auth status") exit 0;;
 "release download")
 name=; output=; while [ "$#" -gt 0 ]; do case "$1" in --pattern) name=$2;shift 2;; --output) output=$2;shift 2;; *) shift;; esac;done
 cp "$FIXTURES/$name" "$output";;
 *) exit 1;; esac''')
        env={**self.env,'CLI_VERSION':'0.6.1','FIXTURE_DAEMON_DIR':str(self.base)}
        daemon=subprocess.Popen([str(target),'daemon'],env=env)
        try:
            for _ in range(100):
                if (self.base/'started').exists(): break
                time.sleep(0.01)
            self.assertTrue((self.base/'started').exists())
            begin=time.monotonic()
            result=subprocess.run(['/bin/sh',str(ROOT/'docs/public/install.sh'),'upgrade'],
                env=env,capture_output=True,text=True,timeout=20)
            elapsed=time.monotonic()-begin
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertTrue((self.base/'finished').exists(),'installer replaced signed CLI before old daemon fully exited')
            self.assertGreater(elapsed,0.7)
            self.assertEqual((self.base/'events').read_text().splitlines(),['browser-installed','login-setup'])
        finally:
            if daemon.poll() is None: daemon.kill()
            daemon.wait(timeout=5)

    def signing_fixture(self, installed=True, identity=None, app_identity=None):
        self.env['CLI_VERSION']='0.6.2'
        self.env['MOCK_SIGNING_IDENTITY']='A'*40
        self.stub('security', '''[ "$1 $2 $3" = 'find-identity -p codesigning' ] || exit 1
[ "${MOCK_IDENTITY_MISSING:-0}" != 1 ] || exit 0
printf '  1) %s "Slingring Local Development"\\n' "$MOCK_SIGNING_IDENTITY"''')
        self.stub('openssl', '''input=
while [ "$#" -gt 0 ]; do case "$1" in -in) input=$2; shift 2;; *) shift;; esac; done
printf 'sha1 Fingerprint=%s\\n' "$(cat "$input")"''')
        signer=self.bin/'codesign'
        signer.write_text(r'''#!/usr/bin/env python3
import os
from pathlib import Path
import re
import sys
args=sys.argv[1:]
target=Path(args[-1])
is_app=target.is_dir()
payload=target/'Contents/MacOS/slingring-menubar' if is_app else target
text=payload.read_bytes().decode('utf-8',errors='replace')
kind='app' if is_app else 'cli'
def marker(name,default=''):
    values=re.findall(r'^# mock-'+name+r'=(.*)$',text,re.M)
    return values[-1] if values else default
identity=marker('identity')
identifier=marker('identifier','com.amxv.slingring.'+('menubar' if is_app else 'cli'))
requirement=marker('requirement','identifier "'+identifier+'" and anchor H"'+identity+'"')
if '--display' in args:
    if '--extract-certificates' in args:
        if not identity: sys.exit(1)
        Path(args[args.index('--extract-certificates')+1]+'0').write_text(identity)
    elif '--requirements' in args: print(('# ' if not marker('requirement') else '')+'designated => '+requirement)
    else:
        print('Identifier='+identifier)
        print('Authority=Slingring Local Development' if identity else 'Signature=adhoc')
    sys.exit(0)
if '--sign' in args:
    if os.environ.get('MOCK_SIGN_FAIL')==kind: sys.exit(1)
    identity=args[args.index('--sign')+1]
    identifier=args[args.index('--identifier')+1]
    requirement='identifier "'+identifier+'" and anchor H"'+identity+'"'
    if '--requirements' in args:
        requirement=Path(args[args.index('--requirements')+1]).read_text().strip().removeprefix('designated => ')
    if os.environ.get('MOCK_CHANGE_REQUIREMENT')==kind: requirement+=' and never'
    payload.write_text(text+'\n# mock-identity='+identity+'\n# mock-identifier='+identifier+'\n# mock-requirement='+requirement+'\n')
    with open(os.environ['EVENT_LOG'],'a') as out: out.write('sign-'+kind+'\n')
    sys.exit(0)
if '--verify' in args:
    if os.environ.get('MOCK_VERIFY_FAIL')==kind and '.slingring-stage.' in str(target): sys.exit(1)
    if '--test-requirement' in args:
        if args[args.index('--test-requirement')+1] != '='+requirement: sys.exit(1)
    sys.exit(0)
sys.exit(2)
''')
        signer.chmod(0o755)
        new_cli='''#!/bin/sh
set -eu
case "$1" in
 --version) echo 'slingring 0.6.2';;
 browser) echo browser-installed >> "$EVENT_LOG";;
 login) echo login-setup >> "$EVENT_LOG";;
 service) echo service-start >> "$EVENT_LOG";;
 *) exit 1;;
esac
'''
        asset='slingring_darwin_arm64.tar.gz'
        self.archive(self.fixtures/asset,[('slingring',new_cli,0o755),
            ('Slingring.app/Contents/MacOS/slingring-menubar','#!/bin/sh\nexit 0\n',0o755)])
        (self.fixtures/(asset+'.sha256')).write_text(hashlib.sha256((self.fixtures/asset).read_bytes()).hexdigest()+'\n')
        self.stub('gh','exit 1')
        if installed:
            target=self.home/'install/slingring';target.parent.mkdir()
            target.write_text('''#!/bin/sh
set -eu
case "$1" in
 status) echo '{"running":true,"active_tasks":0}';;
 service) echo service-stop >> "$EVENT_LOG";;
 --version) echo 'slingring 0.6.1';;
 *) exit 1;;
esac
'''+ ('\n# mock-identity='+identity+'\n' if identity else ''))
            target.chmod(0o755)
            app=target.parent/'Slingring.app/Contents/MacOS/slingring-menubar'
            app.parent.mkdir(parents=True)
            app.write_text('#!/bin/sh\nexit 0\n'+ ('# mock-identity='+app_identity+'\n' if app_identity else ''))
            app.chmod(0o755)
            self.old_cli=target.read_bytes(); self.old_app=app.read_bytes()

    def run_main(self, action='upgrade'):
        return subprocess.run(['/bin/sh',str(ROOT/'docs/public/install.sh'),action],
            env=self.env,capture_output=True,text=True,timeout=20)

    def assert_signing_failure_preserves_installation(self, result):
        self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
        self.assertEqual((self.home/'install/slingring').read_bytes(),self.old_cli)
        self.assertEqual((self.home/'install/Slingring.app/Contents/MacOS/slingring-menubar').read_bytes(),self.old_app)
        events=(self.base/'events').read_text() if (self.base/'events').exists() else ''
        self.assertNotIn('service-stop',events)
        self.assertNotIn('browser-installed',events)
        self.assertNotIn('login-setup',events)

    def test_explicit_identity_signs_both_files_before_initial_install(self):
        self.signing_fixture(installed=False)
        self.env['CLI_CODESIGN_IDENTITY']='a'*40
        result=self.run_main('install'); self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('# mock-identity='+'A'*40,(self.home/'install/slingring').read_text())
        self.assertIn('# mock-identity='+'A'*40,(self.home/'install/Slingring.app/Contents/MacOS/slingring-menubar').read_text())
        self.assertEqual((self.base/'events').read_text().splitlines(),['sign-cli','sign-app','browser-installed','login-setup'])

    def test_upgrade_preserves_existing_signer_and_both_designated_requirements(self):
        self.signing_fixture(identity='A'*40,app_identity='A'*40)
        target=self.home/'install/slingring'
        with target.open('a') as out: out.write('# mock-identifier=local.custom.slingring\n# mock-requirement=identifier "local.custom.slingring" and anchor H"'+'A'*40+'"\n')
        result=self.run_main(); self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('# mock-identity='+'A'*40,target.read_text())
        self.assertIn('# mock-identifier=local.custom.slingring',target.read_text())
        self.assertIn('# mock-requirement=identifier "local.custom.slingring" and anchor H"'+'A'*40+'"',target.read_text())
        self.assertEqual((self.base/'events').read_text().splitlines(),['sign-cli','sign-app','browser-installed','service-stop','login-setup','service-start'])

    def test_explicit_identity_migrates_adhoc_upgrade_without_changing_identifiers(self):
        self.signing_fixture()
        self.env['CLI_CODESIGN_IDENTITY']='A'*40
        result=self.run_main(); self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('# mock-identifier=com.amxv.slingring.cli',(self.home/'install/slingring').read_text())
        self.assertIn('# mock-identifier=com.amxv.slingring.menubar',(self.home/'install/Slingring.app/Contents/MacOS/slingring-menubar').read_text())

    def test_missing_existing_identity_cannot_downgrade_to_adhoc(self):
        self.signing_fixture(identity='A'*40,app_identity='A'*40)
        self.env['MOCK_IDENTITY_MISSING']='1'
        result=self.run_main(); self.assert_signing_failure_preserves_installation(result)
        self.assertIn('unavailable in Keychain',result.stderr)

    def test_explicit_different_identity_cannot_rotate_existing_signer(self):
        self.signing_fixture(identity='A'*40,app_identity='A'*40)
        self.env['CLI_CODESIGN_IDENTITY']='B'*40
        result=self.run_main(); self.assert_signing_failure_preserves_installation(result)
        self.assertIn('Refusing to change',result.stderr)

    def test_conflicting_app_and_cli_signers_abort_safely(self):
        self.signing_fixture(identity='A'*40,app_identity='B'*40)
        result=self.run_main(); self.assert_signing_failure_preserves_installation(result)
        self.assertIn('Refusing to change',result.stderr)

    def test_signing_either_staged_file_can_fail_without_stopping_service(self):
        for kind in ('cli','app'):
            with self.subTest(kind=kind):
                self.signing_fixture(identity='A'*40,app_identity='A'*40)
                self.env['MOCK_SIGN_FAIL']=kind
                self.assert_signing_failure_preserves_installation(self.run_main())
                shutil.rmtree(self.home/'install')

    def test_changed_designated_requirement_aborts_before_replacement(self):
        self.signing_fixture(identity='A'*40,app_identity='A'*40)
        self.env['MOCK_CHANGE_REQUIREMENT']='cli'
        result=self.run_main(); self.assert_signing_failure_preserves_installation(result)
        self.assertIn('designated requirement changed',result.stderr)

    def test_failed_staged_cli_verification_preserves_working_installation(self):
        self.signing_fixture(identity='A'*40,app_identity='A'*40)
        self.env['MOCK_VERIFY_FAIL']='cli'
        self.assert_signing_failure_preserves_installation(self.run_main())

    def test_ambiguous_name_or_adhoc_identity_is_rejected(self):
        self.signing_fixture()
        for value in ('Slingring Local Development','-','A'*39):
            with self.subTest(value=value):
                self.env['CLI_CODESIGN_IDENTITY']=value
                result=self.run_main(); self.assert_signing_failure_preserves_installation(result)
                self.assertIn('40-digit SHA-1',result.stderr)


if __name__=='__main__':unittest.main()
