"""Disabled owner route and uncertain-creation fence, real CLI/private tmux only."""
import json
import subprocess
import sys
import time
import unittest

import test_pause

# Borrow fixture helpers, not its unrelated test methods.
Base = type('OriginalSupervisorFixture', (unittest.TestCase,), {
    name: value for name, value in test_pause.Harness.__dict__.items()
    if name not in ('__dict__', '__weakref__') and not name.startswith('test_')})


@unittest.skipUnless(test_pause.TMUX, 'private tmux required')
class OriginalSupervisor(Base):
    def setUp(self):
        super().setUp()
        self.env['TMPDIR'] = str(self.root)
        needle = 'args = sys.argv[1:]\n'
        counter = '''with (home / 'original-starts').open('a') as f:
    f.write(str(os.getpid()) + '\\n')
'''
        self.assertEqual(test_pause.AGENT.count(needle), 1)
        for agent in ('claude', 'codex', 'kimi'):
            self.script(agent, test_pause.AGENT.replace(needle, needle + counter))

    def starts(self):
        return (self.root / 'original-starts').read_text().splitlines()

    def failing_resume(self, publish_owner):
        counter = '''with (home / 'original-starts').open('a') as f:
    f.write(str(os.getpid()) + '\\n')
'''
        failure = '''
if 'resume' in args or '--resume' in args:
    import fcntl
    state = pathlib.Path(os.environ['HGS_STATE_DIR'])
    with (state / '.lock').open('ab') as writer:
        fcntl.flock(writer, fcntl.LOCK_EX)
        records = []
        for path in state.rglob('*.json'):
            try:
                value = json.loads(path.read_text())
            except (OSError, ValueError):
                continue
            if isinstance(value, dict) and value.get('run_id') == os.environ['HGS_RUN_ID']:
                records.append((path, value))
        assert len(records) == 1, records
        path, value = records[0]
        if PUBLISH_OWNER:
            value['original_owner'] = {'nonce': 'late-unknown-original', 'root': os.getpid()}
            staged = path.with_suffix('.late-fixture')
            staged.write_text(json.dumps(value))
            staged.replace(path)
        (home / 'failed-resume-original.json').write_text(json.dumps(value))
    sys.exit(7)
'''.replace('PUBLISH_OWNER', repr(publish_owner))
        needle = 'args = sys.argv[1:]\n'
        self.assertEqual(test_pause.AGENT.count(needle), 1)
        self.script('codex', test_pause.AGENT.replace(needle, needle + counter + failure))

    def test_late_owned_reservation_blocks_auto_continue_and_keeps_exit_evidence(self):
        self.failing_resume(publish_owner=True)
        name = 'codex/p/late-auto'
        self.hgs('codex', 'p', '-n', 'late-auto', '-c', '-d')
        self.wait(lambda: (self.root / 'failed-resume-original.json').exists())
        self.wait(lambda: len(self.starts()) > 1 or self.binding(name).get('exited_at'))
        original = json.loads((self.root / 'failed-resume-original.json').read_text())
        self.assert_reserved_exit(name, original)

    def assert_reserved_exit(self, name, original):
        self.assertEqual(len(self.starts()), 1, 'reserved original was replayed by auto-continue')
        final = self.binding(name)
        for key in ('original_owner', 'pid', 'process_start', 'run_id', 'session_id',
                    'conversation_id', 'expected_id', 'startup_kind', 'transcript'):
            self.assertEqual(final.get(key), original.get(key), key)
        self.assertEqual(final['exit_code'], 7)
        self.assertEqual(final['completion_source'], 'supervisor')
        self.assertTrue(final['exited_at'])
        self.assertFalse(list((self.root / 'state/archives').glob('*.json')))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux loader interleaving fixture')
    def test_final_fallback_writer_rechecks_a_reservation_after_the_first_decision(self):
        import fcntl
        barrier = self.root / 'fallback-barrier.so'
        subprocess.run(['cc', '-shared', '-fPIC',
                        str(test_pause.REPO / 'tests/fixtures/supervisor_fallback_barrier.c'),
                        '-ldl', '-o', str(barrier)], env=self.env, check=True, capture_output=True)
        self.failing_resume(publish_owner=False)
        self.env.update(LD_PRELOAD=str(barrier), G1_FALLBACK_BARRIER=str(self.root))
        name = 'codex/p/final-admission'
        try:
            self.hgs('codex', 'p', '-n', 'final-admission', '-c', '-d')
            self.wait(lambda: (self.root / 'decision-ready').exists())
            selected = self.binding(name)
            state = self.root / 'state'
            with (state / '.lock').open('ab') as writer:
                fcntl.flock(writer, fcntl.LOCK_EX)
                records = [path for path in state.rglob('*.json') if self._same_record(path, selected)]
                self.assertEqual(len(records), 1)
                original = json.loads(records[0].read_text())
                original['original_owner'] = {'nonce': 'post-decision-original', 'root': original['pid']}
                staged = records[0].with_suffix('.fixture')
                staged.write_text(json.dumps(original))
                staged.replace(records[0])
            (self.root / 'decision-release').touch()
            self.wait(lambda: len(self.starts()) > 1 or self.binding(name).get('exited_at'))
            self.assert_reserved_exit(name, original)
        finally:
            (self.root / 'decision-release').touch()

    def test_unreserved_early_resume_failure_still_starts_fresh(self):
        self.failing_resume(publish_owner=False)
        name = 'codex/p/fallback'
        self.hgs('codex', 'p', '-n', 'fallback', '-c', '-d')
        self.wait(lambda: (self.root / 'failed-resume-original.json').exists())
        self.wait(lambda: len(self.starts()) == 2 and self.binding(name).get('activity') == 'idle')
        record = self.binding(name)
        self.assertNotIn('original_owner', record)
        self.assertEqual(record['startup_kind'], 'new')
        self.assertTrue(record['conversation_id'])
        self.hgs('pause', name)
        self.assertEqual(len(self.starts()), 2)

    def test_known_conversation_resume_failure_still_keeps_one_original(self):
        name = self.start('codex')
        self.wait(lambda: self.binding(name).get('activity') == 'idle')
        self.hgs('pause', name)
        conversation = self.binding(name)['conversation_id']
        self.failing_resume(publish_owner=False)
        self.hgs('resume', name, '-d')
        self.wait(lambda: (self.root / 'failed-resume-original.json').exists())
        self.wait(lambda: self.binding(name).get('exited_at'))
        self.assertEqual(len(self.starts()), 2)
        self.assertEqual(self.binding(name)['conversation_id'], conversation)
        self.assertEqual(self.binding(name)['exit_code'], 7)

    def test_default_route_creates_no_owner_metadata_or_endpoint_for_each_agent(self):
        for agent in ('claude', 'codex', 'kimi'):
            with self.subTest(agent=agent):
                name = self.start(agent)
                self.wait(lambda: self.binding(name).get('activity') == 'idle')
                record = self.binding(name)
                self.assertNotIn('original_owner', record)
                self.assertFalse(list(self.root.glob('hgs-owner-*')))
                self.assertNotEqual(record['pid'], record['supervisor']['pid'])
                self.hgs('pause', name)
                self.assertTrue(self.binding(name)['paused'])
        self.assertEqual(len(self.starts()), 3)

    def test_unknown_owned_reservation_is_not_overwritten_by_resume_or_fresh(self):
        name = self.start('codex')
        self.wait(lambda: self.binding(name).get('activity') == 'idle')
        self.hgs('pause', name)
        before = self.binding(name)
        before['original_owner'] = {'nonce': 'unknown-original'}
        records = [p for p in (self.root / 'state').rglob('*.json')
                   if self._same_record(p, before)]
        self.assertEqual(len(records), 1)
        records[0].write_text(json.dumps(before))
        original = records[0].read_bytes()
        starts = self.starts()
        for fresh in (False, True):
            args = ['resume', name, '-d'] + (['--fresh'] if fresh else [])
            result = subprocess.run([str(test_pause.HGS), *args], env=self.env,
                                    text=True, capture_output=True, timeout=20)
            # A detached shell may return Started before its native admission.
            # Wait for that exact private terminal to disappear: it cannot
            # publish another root after its original supervisor has exited.
            end = time.monotonic() + 10
            while subprocess.run([str(self.bin/'tmux'), 'has-session', '-t', '='+name],
                                 env=self.env, capture_output=True).returncode == 0:
                self.assertLess(time.monotonic(), end, result.stdout + result.stderr)
                self.assertEqual(self.starts(), starts, "unknown original creation was replayed")
                time.sleep(.02)
            self.assertEqual(self.starts(), starts)
            self.assertEqual(records[0].read_bytes(), original)

    @staticmethod
    def _same_record(path, expected):
        try:
            value = json.loads(path.read_text())
            return isinstance(value, dict) and value.get('run_id') == expected['run_id'] \
                and value.get('name') == expected['name']
        except (OSError, ValueError):
            return False


# Only a separately compiled private qualification image sets this test flag.
# The product never reads it and its installed production gate remains false.
import os


@unittest.skipUnless(os.environ.get('HGS_TEST_OWNED_SUPERVISOR_BIN'),
                     'requires a private source-constant qualification image')
class OwnedProfile(Base):
    def setUp(self):
        super().setUp()
        self.assertEqual(str(test_pause.HGS), os.environ['HGS_TEST_OWNED_SUPERVISOR_BIN'])
        self.env['TMPDIR'] = str(self.root)

    def tearDown(self):
        # Release this exact self-bounded fixture before its parent namespaces
        # can disappear, including an assertion or setup error.
        (self.root/'release-daemon').touch()
        Base.tearDown(self)

    def test_actual_original_owner_survives_the_old_budget_and_refuses_legacy_termination(self):
        name = self.start('codex')
        self.wait(lambda: self.binding(name).get('activity') == 'idle')
        original = self.binding(name)
        owner = original['original_owner']
        self.assertEqual(owner['owner'], original['supervisor']['pid'])
        self.assertEqual(owner['root'], original['pid'])
        endpoint = test_pause.Path(owner['endpoint'])
        self.assertTrue(endpoint.is_socket())
        time.sleep(31)
        result = subprocess.run([str(test_pause.HGS), 'terminate', name], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('cohort-aware', result.stderr)
        self.assertEqual(self.binding(name)['pid'], original['pid'])
        self.hgs('pause', name)
        self.wait(lambda: not endpoint.exists())
        final = self.binding(name)
        self.assertTrue(final['paused'])
        self.assertEqual(final['original_owner'], owner)
        self.assertTrue(final['exited_at'])

    def test_actual_supervisor_keeps_an_adopted_daemon_after_its_root_exits(self):
        daemon = r'''
import os,pathlib,sys,time
home=pathlib.Path(sys.argv[1]);parent=os.getppid();end=time.monotonic()+10
(home/'daemon-ready').write_text(str(os.getpid()))
while os.getppid()==parent:
    assert time.monotonic()<end
    time.sleep(.002)
(home/'daemon-adopted').write_text(str(os.getppid()))
while not (home/'release-daemon').exists():
    assert time.monotonic()<end
    time.sleep(.002)
sys.exit(17)
'''
        insert = '''p=subprocess.Popen([sys.executable,'-c',DAEMON,str(home)],
    stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
'''.replace('DAEMON', repr(daemon))
        needle = 'event("SessionStart")\nfor line in sys.stdin:'
        self.assertEqual(test_pause.AGENT.count(needle), 1)
        self.script('codex', test_pause.AGENT.replace(needle, 'event("SessionStart")\n' + insert + 'for line in sys.stdin:'))
        name = self.start('codex')
        self.wait(lambda: (self.root/'daemon-ready').exists())
        original = self.binding(name)
        endpoint = test_pause.Path(original['original_owner']['endpoint'])
        self.t('send-keys', '-t', original['pane'], 'quiet-exit', 'Enter')
        self.wait(lambda: (self.root/'daemon-adopted').exists())
        self.assertEqual(int((self.root/'daemon-adopted').read_text()), original['supervisor']['pid'])
        self.assertTrue(endpoint.exists())
        self.assertFalse(self.binding(name).get('exited_at'))
        self.assertFalse(list((self.root/'state/archives').glob('*.json')))
        (self.root/'release-daemon').touch()
        self.wait(lambda: self.binding(name).get('exited_at'))
        self.wait(lambda: not endpoint.exists())
        final = self.binding(name)
        self.assertEqual(final['exit_code'], 0)
        self.assertEqual(final['original_owner'], original['original_owner'])
        self.assertFalse(list((self.root/'state/archives').glob('*.json')))
