#!/usr/bin/env python3
"""Pre-spawn identity handoff through the actual CLI and isolated tmux."""
import json
import os
import hashlib
import pathlib
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

    def use_required_helper(self, changed_run=False):
        """Observe actual production new/fork counts; saved-ID reader uses
        the existing fixture injection until its own constructor is updated."""
        helper = self.root / 'required-helper' / 'hgs'
        helper.parent.mkdir()
        source = r'''#!/usr/bin/env python3
import os, sys, json, pathlib, subprocess, uuid
argv = sys.argv[1:]
if argv and argv[0] == '--run':
    received = argv[1]
    with pathlib.Path(__CALLS__).open('a') as log:
        log.write(json.dumps(dict(count=received, sid=os.environ.get('HGS_SESSION_ID'),
                                 run=os.environ.get('HGS_RUN_ID'), new=os.environ.get('HGS_LAUNCH_SESSION_ID'))) + '\n')
    if received.isdigit() and os.environ.get('HGS_RESUME_SESSION_ID'):
        argv[1] = 'sid1:' + os.environ['HGS_SESSION_ID'] + ':' + os.environ['HGS_RUN_ID'] + ':' + received
    if __CHANGED__:
        fields = argv[1].split(':')
        assert len(fields) == 4 and fields[0] == 'sid1'
        fields[2] = str(uuid.UUID(int=uuid.UUID(os.environ['HGS_RUN_ID']).int ^ 1))
        argv[1] = ':'.join(fields)
        result = subprocess.run([__HGS__] + argv, text=True, capture_output=True)
        destination = pathlib.Path(__RESULT__)
        staging = destination.with_suffix('.tmp')
        staging.write_text(json.dumps(dict(code=result.returncode, stdout=result.stdout[:1024], stderr=result.stderr[:1024])))
        os.replace(staging, destination)
        sys.exit(result.returncode)
os.execv(__HGS__, [__HGS__] + argv)
'''
        source = source.replace('__HGS__', repr(str(test_pause.HGS)))
        source = source.replace('__CALLS__', repr(str(self.root / 'helper-calls.jsonl')))
        source = source.replace('__RESULT__', repr(str(self.root / 'helper-result')))
        source = source.replace('__CHANGED__', repr(changed_run))
        helper.write_text(source)
        helper.chmod(0o700)
        self.env['HGS_EXECUTABLE'] = str(helper)

    def test_selected_helper_required_count_keeps_the_actual_new_and_fork_ids(self):
        self.assertIn('session-launch-identity-v1', self.hgs('--help'))
        self.use_required_helper()
        self.test_new_identity_exists_before_bootstrap_and_never_inherits_the_caller()
        # The constructor replaces the deliberately inherited variables above.
        for key in ('HGS_SESSION_ID', 'HGS_RUN_ID', 'HGS_RESUME_SESSION_ID', 'HGS_LAUNCH_SESSION_ID'):
            self.env.pop(key, None)
        self.test_every_terminal_fork_preallocates_a_distinct_identity_and_keeps_it_on_resume()
        calls = [json.loads(line) for line in (self.root / 'helper-calls.jsonl').read_text().splitlines()]
        fresh = [call for call in calls if call.get('new')]
        self.assertGreaterEqual(len(fresh), 7)
        for call in fresh:
            self.assertEqual(call['count'], f"sid1:{call['sid']}:{call['run']}:0")

    def test_selected_helper_required_count_keeps_a_saved_id_on_resume(self):
        self.use_required_helper()
        for agent in ('claude', 'codex', 'kimi'):
            with self.subTest(agent=agent):
                name = self.start(agent)
                self.wait(lambda: self.binding(name).get('activity') == 'idle')
                self.hgs('pause', name)
                original = self.binding(name)
                # Existing floor fixture only; no production writer is enabled.
                self.move_to_id_layout(name)
                self.resume(name)
                resumed = self.binding(name)
                self.assertEqual(resumed['session_id'], original['session_id'])
                self.assertEqual(resumed['conversation_id'], original['conversation_id'])
                self.assertNotEqual(resumed['run_id'], original['run_id'])
                self.assertEqual(self.terminal_identity(name)['HGS_SESSION_ID'], original['session_id'])

    def test_a_required_helper_refuses_a_changed_run_before_bootstrap(self):
        self.use_required_helper(changed_run=True)
        name = 'codex/p/rejected'
        self.hgs('codex', 'p', '-n', 'rejected', '-d')
        result = self.root / 'helper-result'
        # Observe either a closed refusal or the forbidden first binding;
        # a mutant that bootstraps cannot hide behind the blocked child.
        self.wait(lambda: result.exists() or bool(self.binding(name)))
        self.assertFalse(self.binding(name), 'changed required run reached bootstrap')
        self.assertTrue(result.exists())
        outcome = json.loads(result.read_text())
        self.assertEqual(outcome['code'], 1, outcome)
        self.assertIn('The required launch identity changed', outcome['stderr'])
        self.assertFalse((self.root / 'argv.jsonl').exists())

    def test_required_count_refuses_missing_changed_and_untracked_identity_before_a_command(self):
        sid, run = str(uuid.uuid4()), str(uuid.uuid4())
        raw = f'sid1:{sid}:{run}:0'
        good = dict(self.env, HGS_SESSION='codex/p/required', HGS_SESSION_ID=sid,
                    HGS_RUN_ID=run, HGS_LAUNCH_SESSION_ID=sid, HGS_TRACKING='1')
        cases = [dict(good, HGS_SESSION_ID=''), dict(good, HGS_RUN_ID=str(uuid.uuid4())),
                 dict(good, HGS_LAUNCH_SESSION_ID=''), dict(good, HGS_TRACKING='0'),
                 dict(good, HGS_RESUME_SESSION_ID=sid), dict(good, HGS_SESSION='')]
        for env in cases:
            with self.subTest(changed=[k for k in good if good[k] != env.get(k)]):
                p = subprocess.run([str(test_pause.HGS), '--run', raw, '/bin/echo', 'must-not-start'],
                                   env=env, text=True, capture_output=True, timeout=5)
                self.assertEqual(p.returncode, 1, p.stderr)
                self.assertNotIn('must-not-start', p.stdout)
                self.assertIn('The required launch identity changed', p.stderr)
                self.assertFalse(self.binding('codex/p/required'))
        p = subprocess.run([str(test_pause.HGS), '--run', '0', '/bin/echo', 'legacy-command'],
                           env=dict(self.env, HGS_TRACKING='0'), text=True, capture_output=True, timeout=5)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(p.stdout.strip(), 'legacy-command')

    def test_a_required_auto_recipe_keeps_its_nonzero_resume_count(self):
        self.use_required_helper()
        escaped = ''.join(c if c.isascii() and c.isalnum() else '-' for c in str(self.project.resolve()))
        history = self.root / '.claude' / 'projects' / escaped
        history.mkdir(parents=True)
        (history / 'old.jsonl').write_text('{}\n')
        self.hgs('claude', 'p', '-c', '-n', 'auto-count', '-d')
        self.wait(lambda: self.binding('claude/p/auto-count').get('conversation_id'))
        calls = [json.loads(line) for line in (self.root / 'helper-calls.jsonl').read_text().splitlines()]
        self.assertEqual(len(calls), 1)
        fields = calls[0]['count'].split(':')
        self.assertEqual(fields[:3], ['sid1', calls[0]['sid'], calls[0]['run']])
        self.assertGreater(int(fields[3]), 0)
        self.assertIn('-c', json.loads((self.root / 'argv.jsonl').read_text().splitlines()[-1])['argv'])

    def test_an_uncertain_capability_probe_keeps_the_integer_launch_recipe(self):
        marker = self.root / 'unexpected-helper-dispatch'
        for kind in ('overflow', 'timeout', 'replacement'):
            with self.subTest(kind=kind):
                helper = self.bin / ('capability-' + kind)
                source = r'''#!/usr/bin/env python3
import os, sys, pathlib, time
if sys.argv[1:] == ['--help']:
    print('session-launch-identity-v1', flush=True)
    if __KIND__ == 'overflow':
        print('x' * (70 << 10))
    elif __KIND__ == 'timeout':
        time.sleep(10)
    else:
        path = pathlib.Path(sys.argv[0])
        new = path.with_suffix('.new')
        new.write_bytes(path.read_bytes())
        new.chmod(0o700)
        os.replace(new, path)
    sys.exit(0)
pathlib.Path(__MARKER__).touch()
os.execv(__HGS__, [__HGS__] + sys.argv[1:])
'''
                helper.write_text(source.replace('__KIND__', repr(kind)).replace('__MARKER__', repr(str(marker)))
                                  .replace('__HGS__', repr(str(test_pause.HGS))))
                helper.chmod(0o700)
                self.env['HGS_EXECUTABLE'] = str(helper)
                name = 'codex/p/capability-' + kind
                self.hgs('codex', 'p', '-n', 'capability-' + kind, '-d')
                self.wait(lambda: self.binding(name).get('conversation_id'))
                self.assertTrue(marker.exists())
                self.assertFalse((self.root / 'state' / 'sessions').exists())
                self.wait(lambda: self.binding(name).get('activity') == 'idle')
                self.hgs('pause', name)

    @unittest.skipUnless(os.environ.get('HGS_TEST_OLD_CLI'), 'requires the verified old CLI fixture')
    def test_an_old_selected_helper_keeps_the_integer_recipe_without_parent_id_markers(self):
        old = pathlib.Path(os.environ['HGS_TEST_OLD_CLI'])
        self.assertEqual(hashlib.sha256(old.read_bytes()).hexdigest(),
                         '146731d47b4aa7c9787096bb12cf3d9054e0b043194fca245fa5a27f2988bbf5')
        parent = str(uuid.uuid4())
        self.env.update(HGS_EXECUTABLE=str(old), HGS_SESSION_ID=parent,
                        HGS_LAUNCH_SESSION_ID=parent, HGS_RUN_ID=str(uuid.uuid4()))
        name = self.start('codex')
        record = self.binding(name)
        self.assertFalse(record.get('session_id'), 'the literal floor has no SID field')
        self.assertNotEqual(record['run_id'], self.env['HGS_RUN_ID'])
        read = subprocess.run([str(self.bin / 'tmux'), 'show-environment', '-t', name, 'HGS_SESSION_ID'],
                              env=self.env, text=True, capture_output=True, timeout=5)
        self.assertEqual(read.returncode, 0, read.stderr)
        self.assertEqual(read.stdout.strip(), 'HGS_SESSION_ID=')
        self.assertEqual(json.loads((self.root / 'argv.jsonl').read_text().splitlines()[-1])['hgs_session_id'], '')
        self.assertFalse((self.root / 'state' / 'sessions').exists())

    @unittest.skipUnless(os.environ.get('HGS_TEST_OLD_CLI'), 'requires the verified old CLI fixture')
    def test_literal_old_helper_refuses_the_required_count_before_bootstrap(self):
        import fcntl
        old = pathlib.Path(os.environ['HGS_TEST_OLD_CLI'])
        self.assertEqual(hashlib.sha256(old.read_bytes()).hexdigest(),
                         '146731d47b4aa7c9787096bb12cf3d9054e0b043194fca245fa5a27f2988bbf5')
        name = self.start('codex')
        self.wait(lambda: self.binding(name).get('activity') == 'idle')
        before = self.binding(name)
        argv = (self.root / 'argv.jsonl').read_bytes()
        marker = self.root / 'unexpected-old-bootstrap'
        agent = self.bin / 'codex'
        body = agent.read_text()
        line, rest = body.split('\n', 1)
        agent.write_text(line + '\nimport pathlib; pathlib.Path(' + repr(str(marker)) + ').touch()\n' + rest)

        def state_snapshot():
            state = self.root / 'state'
            base = state.lstat()
            result = {'.': ('dir', base.st_dev, base.st_ino, base.st_mode)}
            paths = sorted(state.rglob('*'))
            self.assertLess(len(paths), 512)
            total = 0
            for path in paths:
                stat = path.lstat()
                key = str(path.relative_to(state))
                if path.is_dir():
                    result[key] = ('dir', stat.st_dev, stat.st_ino, stat.st_mode)
                else:
                    self.assertTrue(path.is_file() and not path.is_symlink())
                    total += stat.st_size
                    self.assertLess(total, 16 << 20)
                    result[key] = ('file', stat.st_dev, stat.st_ino, stat.st_mode,
                                   stat.st_size, stat.st_mtime_ns, hashlib.sha256(path.read_bytes()).hexdigest())
            return result

        def terminals():
            p = subprocess.run([str(self.bin / 'tmux'), 'list-panes', '-a', '-F',
                                '#{session_id}:#{session_name}:#{pane_id}:#{pane_pid}'],
                               env=self.env, text=True, capture_output=True, timeout=5)
            self.assertEqual(p.returncode, 0, p.stderr)
            return sorted(p.stdout.splitlines())

        pids = sorted({before['pid'], before['supervisor']['pid']})
        def processes():
            return subprocess.check_output(['ps', '-p', ','.join(map(str, pids)),
                                            '-o', 'pid=,ppid=,lstart=,command='], text=True, timeout=5)

        sid, run = str(uuid.uuid4()), str(uuid.uuid4())
        env = dict(self.env, HGS_SESSION=name, HGS_SESSION_ID=sid, HGS_RUN_ID=run,
                   HGS_LAUNCH_SESSION_ID=sid, HGS_AGENT='codex', HGS_TRACKING='1')
        # A partial bootstrap would wait on this original private writer. A
        # genuine old parser refusal never reaches it or changes any state.
        with (self.root / 'state' / '.lock').open('r+b') as writer:
            fcntl.flock(writer, fcntl.LOCK_EX)
            files, panes, native = state_snapshot(), terminals(), processes()
            p = subprocess.run([str(old), '--run', f'sid1:{sid}:{run}:0', str(agent)],
                               env=env, text=True, capture_output=True, timeout=5)
            self.assertEqual(p.returncode, 1, p.stderr)
            self.assertEqual(p.stdout, '')
            self.assertLessEqual(len(p.stderr.encode()), 1024)
            self.assertIn('invalid --run resume-argument count', p.stderr)
            self.assertNotIn('panicked', p.stderr)
            self.assertEqual(state_snapshot(), files)
            self.assertEqual(terminals(), panes)
            self.assertEqual(processes(), native)
            self.assertFalse(marker.exists(), 'the old helper must not even probe the agent')
        self.assertEqual(self.binding(name), before)
        self.assertEqual((self.root / 'argv.jsonl').read_bytes(), argv)
        absent = self.root / 'old-absent-state'
        self.assertFalse(absent.exists())
        p = subprocess.run([str(old), '--run', f'sid1:{sid}:{run}:0', str(agent)],
                           env=dict(env, HGS_STATE_DIR=str(absent)), text=True, capture_output=True, timeout=5)
        self.assertEqual(p.returncode, 1, p.stderr)
        self.assertEqual(p.stdout, '')
        self.assertLessEqual(len(p.stderr.encode()), 1024)
        self.assertIn('invalid --run resume-argument count', p.stderr)
        self.assertNotIn('panicked', p.stderr)
        self.assertFalse(absent.exists(), 'even an absent state root must stay absent')
        self.assertFalse(marker.exists())
        self.assertEqual(terminals(), panes)
        self.assertEqual(processes(), native)

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
                self.wait(lambda: self.binding(target).get('activity') == 'idle' and
                          not self.binding(target).get('expected_id'))
                self.hgs('pause', target)
                self.resume(target)
                resumed = self.binding(target)
                self.assertEqual(resumed['session_id'], child['session_id'])
                self.assertNotEqual(resumed['run_id'], child['run_id'])
                self.assertEqual(self.terminal_identity(target)['HGS_SESSION_ID'], child['session_id'])


if __name__ == '__main__':
    unittest.main()
