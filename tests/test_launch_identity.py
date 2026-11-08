#!/usr/bin/env python3
"""Pre-spawn identity handoff through the actual CLI and isolated tmux."""
import json
import subprocess
import unittest
import uuid

import test_fork
import test_pause


class LaunchIdentity(test_fork.Base):
    def setUp(self):
        original = test_pause.AGENT
        probe = '''if sys.argv[1:] == ['--help']:
    barrier = home / 'hold-launch-help'
    if barrier.exists() and os.environ.get('TMUX_PANE'):
        (home / 'launch-help-entered').touch()
        while barrier.exists():
            time.sleep(0.01)
'''
        source = test_fork.AGENT.replace("if sys.argv[1:] == ['--help']:\n", probe, 1)
        test_pause.AGENT = source
        try:
            super().setUp()
        finally:
            test_pause.AGENT = original

    def terminal_identity(self, name):
        result = {}
        for key in ('HGS_SESSION_ID', 'HGS_RUN_ID'):
            read = subprocess.run([str(self.bin / 'tmux'), 'show-environment', '-t', name, key],
                                  env=self.env, capture_output=True, text=True, timeout=5)
            self.assertEqual(read.returncode, 0, read.stderr)
            value = read.stdout.strip()
            self.assertTrue(value.startswith(key + '='), value)
            result[key] = value.split('=', 1)[1]
            self.assertRegex(result[key], r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
            self.assertEqual(str(uuid.UUID(result[key])), result[key])
            self.assertNotEqual(uuid.UUID(result[key]).int, 0)
        return result

    def test_new_identity_exists_before_bootstrap_and_never_inherits_the_caller(self):
        barrier = self.root / 'hold-launch-help'
        barrier.touch()
        parent = str(uuid.uuid4())
        self.env.update(HGS_SESSION_ID=parent, HGS_RUN_ID=str(uuid.uuid4()),
                        HGS_RESUME_SESSION_ID=parent, HGS_LAUNCH_SESSION_ID=parent)
        name = 'codex/p/allocated'
        try:
            self.hgs('codex', 'p', '-n', 'allocated', '-d')
            self.wait(lambda: (self.root / 'launch-help-entered').exists())
            self.assertEqual(self.binding(name), {}, 'the probe must precede the first binding')
            identity = self.terminal_identity(name)
            self.assertNotEqual(identity['HGS_SESSION_ID'], parent)
        finally:
            barrier.unlink(missing_ok=True)
        self.wait(lambda: self.binding(name).get('conversation_id'))
        record = self.binding(name)
        self.assertEqual(record['session_id'], identity['HGS_SESSION_ID'])
        self.assertEqual(record['run_id'], identity['HGS_RUN_ID'])
        self.assertNotEqual(record['session_id'], record['run_id'])
        argv = [json.loads(line) for line in (self.root / 'argv.jsonl').read_text().splitlines()]
        self.assertEqual(argv[-1]['hgs_session_id'], record['session_id'])

    def test_every_terminal_fork_preallocates_a_distinct_identity_and_keeps_it_on_resume(self):
        for agent in ('claude', 'codex', 'kimi'):
            with self.subTest(agent=agent):
                source = self.start(agent)
                original = self.binding(source)
                self.hgs('fork', source, '-n', 'child', '-d',
                         '--expected-run-id', original['run_id'],
                         '--expected-conversation-id', original['conversation_id'])
                target = f'{agent}/p/child'
                self.wait(lambda: self.binding(target).get('conversation_id'))
                child = self.binding(target)
                identity = self.terminal_identity(target)
                self.assertEqual(identity['HGS_SESSION_ID'], child['session_id'])
                self.assertEqual(identity['HGS_RUN_ID'], child['run_id'])
                self.assertNotEqual(child['session_id'], original['session_id'])
                self.assertEqual(self.binding(source)['run_id'], original['run_id'])
                self.hgs('pause', target)
                self.resume(target)
                resumed = self.binding(target)
                self.assertEqual(resumed['session_id'], child['session_id'])
                self.assertNotEqual(resumed['run_id'], child['run_id'])
                self.assertEqual(self.terminal_identity(target)['HGS_SESSION_ID'], child['session_id'])


if __name__ == '__main__':
    unittest.main()
