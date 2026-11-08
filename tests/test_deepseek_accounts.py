"""Official native credential service, temporary keys, no model requests."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import unittest

HGS = Path(os.environ.get('HGS_TEST_BIN', 'target/debug/hgs')).resolve()


@unittest.skipUnless(shutil.which('dsh') and HGS.exists(), 'official dsh and hgs required')
class DeepSeekAccounts(unittest.TestCase):
    def test_private_key_write_restore_identity_and_remote_destination(self):
        with tempfile.TemporaryDirectory(prefix='hgs-key-', dir=os.environ.get('ZERUS_TEST_SHORT_TMPDIR', '/tmp')) as directory:
            root = Path(directory)
            def environment(name):
                home = root / name
                home.mkdir()
                return {'PATH': os.environ['PATH'], 'LANG': 'C.UTF-8', 'HOME': str(home),
                        'DSH_HOME': str(home / '.dsh'), 'DSH_TELEMETRY_DISABLED': '1',
                        'HGS_CONFIG_DIR': str(home / '.config/hgs'), 'HGS_STATE_DIR': str(home / 'state'),
                        'HGS_SELF': name, 'HGS_PEERS': ''}
            local, remote = environment('local'), environment('remote')
            def run(env, *args, payload=None, ok=True):
                result = subprocess.run([str(HGS), *args], env=env, input=json.dumps(payload) if payload else None,
                                        cwd=root, text=True, capture_output=True, timeout=90)
                if ok:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    return json.loads(result.stdout)
                self.assertNotEqual(result.returncode, 0)
                return result
            key = 'sk-temporary-native-key-contract'
            replacement = 'sk-temporary-native-key-replacement'
            try:
                catalog = run(local, 'account', 'ls')
                stale = run(local, 'account', 'set-key', 'native-dsh', '--json', '--revision', 'stale', payload={'api_key': key}, ok=False)
                self.assertNotIn(key, stale.stdout + stale.stderr)
                self.assertFalse(Path(local['DSH_HOME']).exists())
                run(local, 'account', 'rm', 'native-dsh')
                result = run(local, 'account', 'set-key', 'native-dsh', '--json', payload={'api_key': key, 'label': 'API work'})
                self.assertTrue(any(p['id'] == 'native-dsh' for p in result['profiles']))
                self.assertFalse(any(p['id'] == 'native-dsh' for p in result['removed_profiles']))
                self.assertNotIn(key, json.dumps(result))
                status = run(local, 'account', 'inspect', 'native-dsh', '--refresh')
                self.assertEqual(status['status'], 'configured')
                expected = 'api-key:' + hashlib.sha256(('deepseek-api-key\0' + key).encode()).hexdigest()
                self.assertEqual(status['identity']['account_id'], expected)
                credential = Path(local['DSH_HOME']) / '.credentials.yaml'
                self.assertEqual(credential.stat().st_mode & 0o777, 0o600)
                self.assertIn(key, credential.read_text())
                # Exercise the real CLI SSH branch with a private simulated remote home.
                shim = Path(local['HOME']) / '.local/bin'; shim.mkdir(parents=True)
                ssh = shim / 'ssh'
                ssh.write_text('#!/usr/bin/env python3\nimport json,os,pathlib,shlex,sys\n'
                    + 'if sys.argv[-1] == ' + repr('echo "hgs-remote-v2|%OS%|%SystemRoot%|$env:OS|$env:SystemRoot"') + ':\n'
                    + '    print("hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot"); sys.exit(0)\n'
                    + 'pathlib.Path(' + repr(str(root / 'ssh-args.json')) + ').write_text(json.dumps(sys.argv[1:]))\n'
                    + 'args=shlex.split(sys.argv[-1])[1:]\n'
                    + 'os.execve(' + repr(str(HGS)) + ', [' + repr(str(HGS)) + ']+args, ' + repr(remote) + ')\n')
                ssh.chmod(0o700)
                run(local, '@remote', 'account', 'set-key', 'native-dsh', '--json', payload={'api_key': key})
                self.assertNotIn(key, (root / 'ssh-args.json').read_text())
                remote_status = run(remote, 'account', 'inspect', 'native-dsh', '--refresh')
                self.assertEqual(remote_status['identity']['account_id'], expected)
                run(local, '@remote', 'account', 'set-key', 'native-dsh', '--json', payload={'api_key': replacement})
                self.assertIn(key, credential.read_text())
                self.assertNotIn(replacement, credential.read_text())
                self.assertIn(replacement, (Path(remote['DSH_HOME']) / '.credentials.yaml').read_text())
                changed = run(remote, 'account', 'inspect', 'native-dsh')
                self.assertNotEqual(changed['identity']['account_id'], expected)
            finally:
                for env in (local, remote):
                    for path in Path(env['HGS_STATE_DIR']).glob('dsh/hosts/*/host.pid'):
                        try:
                            os.kill(int(path.read_text()), signal.SIGTERM)
                        except ProcessLookupError:
                            pass


if __name__ == '__main__':
    unittest.main()
