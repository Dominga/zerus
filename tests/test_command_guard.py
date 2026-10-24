"""Text-only corpus and real bridge middleware; never execute fixture commands."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
NODE = shutil.which('node')
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug' / ('hgs.exe' if os.name == 'nt' else 'hgs')))


@unittest.skipUnless(NODE, 'node is required for the DeepSeek command guard')
class CommandGuard(unittest.TestCase):
    def test_shared_corpus_and_bridge_middleware(self):
        result = subprocess.run(
            [NODE, str(REPO / 'tests/fixtures/command-guard-runner.mjs')],
            capture_output=True, text=True, timeout=20, cwd=REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


@unittest.skipUnless(HGS.is_file(), 'built hgs is required for the hook entry test')
class HookCommandGuard(unittest.TestCase):
    def test_refuses_before_state_lock_or_child_routing(self):
        with tempfile.TemporaryDirectory(prefix='hgs-guard-') as directory:
            root = Path(directory)
            # Enable only this disposable CLI profile; no host is installed or
            # started. The hook must refuse before consulting its run records.
            (root / 'config').mkdir()
            (root / 'config/local-sessions').write_text('fixture\n')
            env = {k: v for k, v in os.environ.items() if not k.startswith('HGS_')}
            env.update(HOME=str(root), USERPROFILE=str(root), HGS_SESSION='fixture/guard',
                       HGS_RUN_ID='fixture-run', HGS_STATE_DIR=str(root / 'state'),
                       HGS_CONFIG_DIR=str(root / 'config'), HGS_SELF='fixture', HGS_PEERS='')
            for key, command in [('command', "$home='fixture'"), ('cmd', ['pwsh', '-Command', "$home='fixture'"])]:
                for child in ['', 'fixture-child']:
                    event = {'hook_event_name': 'PreToolUse', 'session_id': 'fixture-session',
                             'agent_id': child, 'tool_input': {key: command}, 'permission_mode': 'bypass'}
                    result = subprocess.run([str(HGS), '__state', 'hook'], input=json.dumps(event),
                                            capture_output=True, text=True, env=env, timeout=10)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertIn('Zerus blocked this command:', result.stderr)
                    self.assertIn('$appHome', result.stderr)
                    self.assertFalse((root / 'state').exists(), 'refusal must precede state I/O')


if __name__ == '__main__':
    unittest.main()
