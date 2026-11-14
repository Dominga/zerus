"""New-name policy and legacy selectors, using only the private lifecycle fixture."""
import json
import os
from pathlib import Path
import subprocess
import unittest

import test_pause

test_pause.HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()
Base = type('NameFixture', (unittest.TestCase,), {name: value for name, value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__', '__weakref__') and not name.startswith('test_')})


class SessionTags(Base):
    def refused(self, *args):
        result = subprocess.run([str(test_pause.HGS), *args], env=self.env, capture_output=True, text=True, timeout=20)
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn('Names cannot contain shell syntax or invisible formatting characters.', result.stderr)

    def test_new_names_refuse_before_native_launch_or_rename(self):
        for agent in ('claude', 'codex', 'kimi', 'dsh', 'opencode'):
            for tag in ('bad$tag', 'bad;tag', 'bad\u200btag', 'bad\u2019tag'):
                self.refused('--dry-run', agent, 'p', '--new', '-n', tag, '-d')
        # A missing -n label would create a session even without --new.
        self.refused('--dry-run', 'claude', 'p', '-n', 'bad$tag', '-d')
        self.refused('--dry-run', 'rename', 'claude/p/old$tag', 'claude/p/bad$tag')
        self.assertFalse((self.root / 'argv.jsonl').exists(), 'no native agent is launched')

    def test_legacy_archive_is_readable_and_can_be_renamed_to_safe_tag(self):
        name = self.start('claude', 'legacy')
        self.hgs('pause', name)
        self.hgs('archive', name)
        archives = list((self.root / 'state/archives').glob('*.json'))
        self.assertEqual(1, len(archives))
        path = archives[0]
        record = json.loads(path.read_text())
        identity = (record['run_id'], record['conversation_id'])
        # A synthetic pre-policy archive, never an owner record.
        record['name'] = 'claude/p/old$tag'
        path.write_text(json.dumps(record))
        inspected = json.loads(self.hgs('inspect', record['name'], '--archive', path.stem))
        self.assertEqual(record['name'], inspected['name'])
        self.hgs('rename', record['name'], 'claude/p/safe name', '--archive', path.stem)
        updated = json.loads(path.read_text())
        self.assertEqual('claude/p/safe name', updated['name'])
        self.assertEqual(identity, (updated['run_id'], updated['conversation_id']))
