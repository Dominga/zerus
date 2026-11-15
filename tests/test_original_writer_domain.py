"""Original main-thread supervisor, private const-true image and fake agent."""
import json
import os
import subprocess
from pathlib import Path
import unittest

import test_pause

Base = type('OriginalDomainFixture', (unittest.TestCase,), {
    name: value for name, value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__', '__weakref__') and not name.startswith('test_')})


@unittest.skipUnless(os.environ.get('HGS_TEST_ORIGINAL_DOMAIN_BIN'),
                     'requires privately compiled original-owner image')
class OriginalWriterDomain(Base):
    def setUp(self):
        super().setUp()
        self.assertEqual(Path(os.environ['HGS_TEST_ORIGINAL_DOMAIN_BIN']), test_pause.HGS)
        needle = 'args = sys.argv[1:]\n'
        check = '''
if os.environ.get('HGS_SESSION_ID') and '--version' not in args:
    domain = pathlib.Path(os.environ['HGS_STATE_DIR']) / 'terminating-writers' / os.environ['HGS_SESSION_ID'] / os.environ['HGS_RUN_ID']
    found = {name: (domain / name).is_file() for name in ['gate', 'lock', 'pending.json']}
    (home / 'domain-at-entry.json').write_text(json.dumps(dict(found=found, domain=str(domain), session=os.environ['HGS_SESSION_ID'], run=os.environ['HGS_RUN_ID'])))
'''
        self.assertEqual(test_pause.AGENT.count(needle), 1)
        self.script('codex', test_pause.AGENT.replace(needle, needle + check))

    def entry(self):
        try:
            return json.loads((self.root / 'domain-at-entry.json').read_text())
        except (OSError, ValueError):
            return None

    def test_domain_is_complete_before_the_actual_original_child_enters(self):
        name = self.start('codex')
        self.wait(lambda: self.entry() is not None)
        entry = self.entry()
        self.assertTrue(all(entry['found'].values()),
                        'actual child entered before its complete original domain: ' + str(entry))
        ledger = json.loads((Path(entry['domain']) / 'pending.json').read_text())
        self.assertEqual(ledger['scope'], dict(session=entry['session'], run=entry['run']))
        self.assertEqual(ledger['pending'], [])
        self.assertIn('original_owner', self.binding(name))

    def failed_creation(self, kind):
        # Keep only this fixture's socket alive after the failed pane exits,
        # so the synchronous supervisor replay reaches locked admission.
        hold = self.t('new-session', '-d', '-P', '-F', '#{pane_id}',
                      '-s', 'fixture-hold', 'sleep', '60').strip()
        state = self.root / 'state'
        state.mkdir(mode=0o700)
        base = state / 'terminating-writers'
        if kind == 'link':
            foreign = self.root / 'foreign'
            foreign.mkdir(mode=0o700)
            (foreign / 'retained').write_text('unrelated')
            base.symlink_to(foreign)
        elif kind == 'fifo':
            os.mkfifo(base, 0o600)
        else:
            base.write_text('retained partial namespace')
            base.chmod(0o600)
        name = 'codex/p/refused'
        self.hgs('codex', 'p', '-n', 'refused', '-d')
        self.wait(lambda: self.binding(name).get('original_owner'))
        original = self.binding(name)
        result = subprocess.run([str(test_pause.HGS), 'resume', name, '-d'],
                                env=self.env, text=True, capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('conversation ID is not confirmed', result.stderr)
        self.wait(lambda: subprocess.run([str(self.bin / 'tmux'), 'has-session', '-t', '=' + name],
                                        env=self.env, capture_output=True).returncode != 0)
        self.t('rename-session', '-t', '=fixture-hold', name)
        # Execute the original supervisor entry synchronously; a detached
        # launch's outer success only acknowledges a pane, not its outcome.
        replay_env = dict(self.env, HGS_SESSION=name, HGS_AGENT='codex',
                          HGS_EXECUTABLE=str(test_pause.HGS),
                          HGS_RUN_ID=original['run_id'], HGS_SESSION_ID=original['session_id'],
                          HGS_RESUME_SESSION_ID=original['session_id'] if original.get('layout') == 'id' else '',
                          HGS_TERMINAL_NAME=name, TMUX_PANE=hold)
        replay = subprocess.run([str(test_pause.HGS), '--run', '0', 'codex'],
                                env=replay_env, text=True, capture_output=True, timeout=15)
        self.assertNotEqual(replay.returncode, 0, replay.stdout + replay.stderr)
        self.assertIn('Original creation', replay.stderr)
        self.assertEqual(self.binding(name)['original_owner'], original['original_owner'])
        self.assertEqual(self.binding(name)['run_id'], original['run_id'])
        def writer_free():
            import fcntl  # Only the opted-in Unix fixture reaches this helper.
            try:
                with (state / '.lock').open('r+b') as writer:
                    fcntl.flock(writer, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return True
            except BlockingIOError:
                return False
        self.wait(writer_free)
        self.assertIsNone(self.entry(), 'uncertain domain creation started a native child')
        if kind == 'link':
            self.assertEqual(list(foreign.iterdir()), [foreign / 'retained'])
        elif kind == 'file':
            self.assertEqual(base.read_text(), 'retained partial namespace')

    def test_a_linked_domain_refuses_before_child_and_keeps_original_uncertainty(self):
        self.failed_creation('link')

    def test_a_fifo_domain_refuses_without_blocking_the_writer_or_replaying(self):
        self.failed_creation('fifo')

    def test_a_partial_domain_refuses_before_child_and_is_not_reset(self):
        self.failed_creation('file')
