"""Claude hook caller proof in private state and a private tmux socket."""
import hashlib
import json
import os
import subprocess
import sys
import time
import unittest
import uuid
from pathlib import Path

if os.name == "posix":
    import fcntl

import test_input as fixture


@unittest.skipUnless(sys.platform in ('linux', 'darwin'), 'Unix Claude caller proof')
class ClaudeHookCaller(unittest.TestCase):
    setUp = fixture.InputTransport.setUp
    tearDown = fixture.InputTransport.tearDown
    script = fixture.InputTransport.script
    tmux = fixture.InputTransport.tmux
    wait_for = fixture.InputTransport.wait_for
    write_record = fixture.InputTransport.write_record

    def prepare(self, owned=False, legacy=False):
        self.record.update(agent='claude', activity='idle', phase='idle')
        if owned:
            self.record.update(pid=os.getpid(), process_start=subprocess.check_output(
                ['ps', '-p', str(os.getpid()), '-o', 'lstart='],
                env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip())
        if legacy:
            self.record.update(pid=None, process_start='')
        self.write_record()
        return dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run_id, HGS_AGENT='claude')

    def event(self):
        return dict(hook_event_name='PreToolUse', session_id=self.conversation_id,
                    tool_name='Bash', tool_use_id='toolu_caller_fixture', tool_input=dict(command='true'))

    def run_hook(self, env, event=None):
        return subprocess.run([str(fixture.HGS), '__state', 'hook'], input=json.dumps(event or self.event()),
                              text=True, capture_output=True, env=env, timeout=12)

    def waiting_question(self, background=False, blocking_transcript=False):
        other = (self.record['pid'], self.record['process_start'])
        env = self.prepare(owned=not background)
        if background:
            config = self.root / 'claude-config'
            directory = config / 'sessions'; directory.mkdir(parents=True)
            (directory / f'{os.getpid()}.json').write_text(json.dumps(dict(
                pid=os.getpid(), kind='bg', jobId='private-held-job')))
            self.hold_path = directory / f'{other[0]}.json'
            self.hold_path.write_text(json.dumps(dict(pid=other[0], parkedJobId='private-held-job')))
            self.record['agent_home'] = str(config); self.write_record()
            env.update(CLAUDE_PID=str(os.getpid()), CLAUDE_CODE_SESSION_ID=self.conversation_id,
                       CLAUDE_CONFIG_DIR=str(config))
        items = [dict(question='Which release?', header='Release', options=[
            dict(label='Stable', description='Stable fixture'), dict(label='Preview', description='Preview fixture')])]
        event = dict(hook_event_name='PreToolUse', session_id=self.conversation_id,
                     tool_name='AskUserQuestion', tool_use_id='toolu_caller_fixture', tool_input=dict(questions=items))
        made = self.run_hook(env, event)
        self.assertEqual(made.returncode, 0, made.stderr)
        if blocking_transcript:
            self.transcript_fifo = self.root / (self.conversation_id + '.jsonl')
            os.mkfifo(self.transcript_fifo)
            record = json.loads(self.record_path.read_text())
            record['transcript'] = str(self.transcript_fifo)
            self.record_path.write_text(json.dumps(record))
        event.update(hook_event_name='PermissionRequest')
        worker = subprocess.Popen([str(fixture.HGS), '__state', 'answer-channel'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            env=dict(env, HGS_ANSWER_CHANNEL_SECONDS='5'))
        worker.stdin.write(json.dumps(event)); worker.stdin.close(); worker.stdin = None
        def cleanup():
            if worker.poll() is None: worker.terminate()
            try: worker.communicate(timeout=8)
            except subprocess.TimeoutExpired:
                worker.kill(); worker.communicate(timeout=8)
        self.addCleanup(cleanup)
        self.wait_for(lambda: next(self.state.glob('native_answers/*/*/waiting.json'), None))
        waiting = next(self.state.glob('native_answers/*/*/waiting.json'))
        return worker, waiting.parent, other

    def offer(self, directory):
        payload = dict(request_id=str(uuid.uuid4()), answers={'Which release?': 'Stable'})
        temporary = directory / 'answer.tmp'
        temporary.write_text(json.dumps(payload)); temporary.replace(directory / 'answer.json')

    def change_pin(self, other):
        record = json.loads(self.record_path.read_text())
        record.update(pid=other[0], process_start=other[1])
        temporary = self.record_path.with_suffix('.tmp')
        temporary.write_text(json.dumps(record)); temporary.replace(self.record_path)

    def record_host(self, corrupt=False):
        directory = self.state / 'dsh/hosts/private-caller-fixture'
        directory.mkdir(parents=True)
        if sys.platform == 'linux':
            birth = Path(f'/proc/{os.getpid()}/stat').read_text().rsplit(')', 1)[1].split()[19]
        else:
            import ctypes
            import struct
            data = ctypes.create_string_buffer(136)
            lib = ctypes.CDLL('/usr/lib/libproc.dylib')
            self.assertEqual(lib.proc_pidinfo(os.getpid(), 3, 0, data, len(data)), len(data))
            seconds, micros = struct.unpack_from('QQ', data.raw, 120)
            birth = f'{seconds}:{micros}'
        value = dict(state='running', version=1, pid=os.getpid(), birth=birth, pid_file=[0,0,0,0])
        (directory / 'host.identity.json').write_text('{' if corrupt else json.dumps(value))

    def test_copied_markers_cannot_write_a_pinned_claude_hook(self):
        env = self.prepare()
        before = self.record_path.read_bytes()
        for agent in ('claude', 'codex', ''):
            with self.subTest(marker_agent=agent):
                result = self.run_hook(dict(env, HGS_AGENT=agent))
                self.assertEqual(self.record_path.read_bytes(), before, 'copied markers must not post a hook for another process')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')
                self.assertIn('caller', result.stderr.lower())

    def test_own_native_process_tree_keeps_foreground_hooks(self):
        result = self.run_hook(self.prepare(owned=True))
        self.assertEqual(result.returncode, 0, result.stderr)
        saved = json.loads(self.record_path.read_text())
        self.assertEqual(saved['phase'], 'tool')
        self.assertIn('toolu_caller_fixture', saved['active_tools'])

    def test_fixture_hook_runs_below_the_original_agent_without_borrowing_its_pin(self):
        env = self.prepare()
        pinned = {key: self.record[key] for key in ('pid', 'process_start', 'run_id', 'conversation_id')}
        # The positive producer is the real private terminal process, with its
        # original PID/birth. The test controller must remain a foreign caller.
        fixture.InputTransport.claude_hook(self, 'UserPromptSubmit')
        saved = json.loads(self.record_path.read_text())
        self.assertEqual({key: saved[key] for key in pinned}, pinned)
        self.assertEqual((saved['activity'], saved['phase']), ('busy', 'working'))
        before = self.record_path.read_bytes()
        refused = self.run_hook(env)
        self.assertNotEqual(refused.returncode, 0, refused.stderr)
        self.assertEqual(self.record_path.read_bytes(), before)

    def test_unpinned_legacy_hooks_keep_behavior_with_a_visible_diagnostic(self):
        result = self.run_hook(self.prepare(legacy=True))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.record_path.read_text())['phase'], 'tool')
        self.assertIn('legacy', result.stderr.lower())

    def test_missing_pin_in_a_modern_launch_is_not_a_legacy_bypass(self):
        env = self.prepare(legacy=True)
        self.record['run_identity_version'] = 1
        self.write_record()
        before = self.record_path.read_bytes()
        result = self.run_hook(env)
        self.assertEqual(self.record_path.read_bytes(), before)
        self.assertNotEqual(result.returncode, 0)

    def test_malformed_pins_never_use_the_legacy_exception(self):
        env = self.prepare()
        for pid, start in [('invalid', ''), (2**40, ''), (None, []), (-1, '')]:
            with self.subTest(pid=pid, start=start):
                self.record.update(pid=pid, process_start=start); self.write_record()
                before = self.record_path.read_bytes()
                result = self.run_hook(env)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.record_path.read_bytes(), before)

    def test_a_recorded_host_cannot_borrow_its_claude_ancestors_pin(self):
        env = self.prepare(owned=True)
        self.record_host()
        before = self.record_path.read_bytes()
        result = self.run_hook(env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.record_path.read_bytes(), before)

    def test_unreadable_host_evidence_never_relaxes_a_pinned_record(self):
        env = self.prepare(owned=True)
        self.record_host(corrupt=True)
        before = self.record_path.read_bytes()
        result = self.run_hook(env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.record_path.read_bytes(), before)

    def test_copied_background_pid_cannot_write_a_continuation_link(self):
        env = self.prepare()
        pid = self.record['pid']
        config = self.root / 'claude-config'
        (config / 'sessions').mkdir(parents=True)
        (config / 'sessions' / f'{pid}.json').write_text(json.dumps(dict(
            pid=pid, kind='bg', jobId='private-background-job', parkedJobId='private-background-job')))
        env.update(HGS_SESSION='claude/example/missing', HGS_RUN_ID='missing-run',
                   CLAUDE_PID=str(pid), CLAUDE_CODE_SESSION_ID=self.conversation_id,
                   CLAUDE_CONFIG_DIR=str(config))
        before = self.record_path.read_bytes()
        result = self.run_hook(env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.record_path.read_bytes(), before)
        self.assertFalse((self.state / 'background_jobs').exists())

    def test_caller_selected_registry_cannot_fabricate_a_background_hold(self):
        env = self.prepare()
        self.record['agent_home'] = str(self.root / 'recorded-claude-config')
        self.write_record()
        config = self.root / 'forged-claude-config'
        directory = config / 'sessions'; directory.mkdir(parents=True)
        for pid, entry in [(os.getpid(), dict(kind='bg', jobId='forged-job')),
                           (self.record['pid'], dict(parkedJobId='forged-job'))]:
            (directory / f'{pid}.json').write_text(json.dumps(dict(pid=pid, **entry)))
        env.update(CLAUDE_PID=str(os.getpid()), CLAUDE_CODE_SESSION_ID=self.conversation_id,
                   CLAUDE_CONFIG_DIR=str(config))
        before = self.record_path.read_bytes()
        self.run_hook(env)
        self.assertEqual(self.record_path.read_bytes(), before,
                         'caller-selected registry files must not authorize another window')
        self.assertFalse((self.state / 'background_jobs').exists())

    def test_caller_home_cannot_fabricate_a_legacy_background_hold(self):
        env = self.prepare()
        forged_home = self.root / 'forged-home'
        config = forged_home / '.claude'
        directory = config / 'sessions'; directory.mkdir(parents=True)
        for pid, entry in [(os.getpid(), dict(kind='bg', jobId='forged-home-job')),
                           (self.record['pid'], dict(parkedJobId='forged-home-job'))]:
            (directory / f'{pid}.json').write_text(json.dumps(dict(pid=pid, **entry)))
        env.update(HOME=str(forged_home), CLAUDE_PID=str(os.getpid()),
                   CLAUDE_CODE_SESSION_ID=self.conversation_id, CLAUDE_CONFIG_DIR=str(config))
        before = self.record_path.read_bytes()
        self.run_hook(env)
        self.assertEqual(self.record_path.read_bytes(), before,
                         'caller HOME must not supply missing background account authority')
        self.assertFalse((self.state / 'background_jobs').exists())

    def test_copied_markers_cannot_register_a_pinned_answer_channel(self):
        env = self.prepare()
        inputs = dict(command='printf caller-fixture')
        digest = hashlib.sha256(json.dumps(inputs, separators=(',', ':'), sort_keys=True).encode()).hexdigest()
        event = dict(hook_event_name='PermissionRequest', tool_name='Bash',
                     session_id=self.conversation_id, tool_input=inputs)
        self.record.update(phase='approval', pending_approval=event, pending_approval_at=time.time(),
                           claude_tool_uses=dict(toolu_caller_fixture=dict(tool_name='Bash', input=digest, at=time.time()-1)))
        self.write_record()
        result = subprocess.run([str(fixture.HGS), '__state', 'answer-channel'], input=json.dumps(event),
                                text=True, capture_output=True, env=dict(env, HGS_ANSWER_CHANNEL_SECONDS='0.1'), timeout=12)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertEqual(list(self.state.glob('native_answers/*/*/waiting.json')), [],
                         'an unproven worker must not acquire a channel or consume its offers')

    def test_a_claude_channel_cannot_borrow_another_providers_hook_card(self):
        self.record.update(agent='kimi', activity='idle', phase='idle'); self.write_record()
        env = dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run_id, HGS_AGENT='claude')
        items = [dict(question='Which release?', header='Release', options=[
            dict(label='Stable', description='Stable fixture'), dict(label='Preview', description='Preview fixture')])]
        event = dict(hook_event_name='PreToolUse', session_id=self.conversation_id,
                     tool_name='AskUserQuestion', tool_use_id='toolu_provider_fixture', tool_input=dict(questions=items))
        # Other providers retain the shared hook path and their read-only cards.
        made = self.run_hook(env, event)
        self.assertEqual(made.returncode, 0, made.stderr)
        self.assertTrue(json.loads(self.record_path.read_text())['pending_questions'])
        event['hook_event_name'] = 'PermissionRequest'
        result = subprocess.run([str(fixture.HGS), '__state', 'answer-channel'], input=json.dumps(event),
            text=True, capture_output=True, env=dict(env, HGS_ANSWER_CHANNEL_SECONDS='0.1'), timeout=12)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertEqual(list(self.state.glob('native_answers/*/*/waiting.json')), [],
                         'a non-Claude record must never authorize a Claude channel')

    def test_a_channel_never_downgrades_when_its_record_changes_provider(self):
        worker, directory, _ = self.waiting_question()
        record = json.loads(self.record_path.read_text()); record['agent'] = 'codex'
        self.record_path.write_text(json.dumps(record))
        self.offer(directory)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(output, '')
        self.assertIn('caller', errors.lower())
        self.assertTrue((directory / 'answer.json').exists())
        self.assertFalse((directory / 'claimed.json').exists())

    def test_a_proven_question_worker_keeps_the_normal_answer_protocol(self):
        worker, directory, _ = self.waiting_question()
        self.offer(directory)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(worker.returncode, 0, errors)
        self.assertEqual(json.loads(output)['hookSpecificOutput']['decision']['behavior'], 'allow')
        self.assertTrue((directory / 'emitted.json').exists())

    def test_a_held_background_job_keeps_the_answer_channel(self):
        worker, directory, _ = self.waiting_question(background=True)
        self.offer(directory)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(worker.returncode, 0, errors)
        self.assertEqual(json.loads(output)['hookSpecificOutput']['decision']['behavior'], 'allow')
        self.assertTrue((directory / 'emitted.json').exists())

    def test_released_background_hold_cannot_claim_a_waiting_offer(self):
        worker, directory, _ = self.waiting_question(background=True)
        self.hold_path.unlink()
        self.offer(directory)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(output, '')
        self.assertIn('caller', errors.lower())
        self.assertTrue((directory / 'answer.json').exists())
        self.assertFalse((directory / 'claimed.json').exists())

    def test_lost_pin_before_claim_leaves_the_offer_unclaimed(self):
        worker, directory, other = self.waiting_question()
        self.change_pin(other)
        time.sleep(.3)  # The worker's normal poll observes the new live record.
        self.offer(directory)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(output, '')
        self.assertIn('caller', errors.lower())
        self.assertTrue((directory / 'answer.json').exists())
        self.assertFalse((directory / 'claimed.json').exists())
        self.assertFalse((directory / 'emitted.json').exists())

    @unittest.skipUnless(sys.platform == 'linux', 'Linux private FIFO-open witness')
    def test_preclaim_proof_rereads_after_a_slow_transcript_check(self):
        worker, directory, other = self.waiting_question(blocking_transcript=True)
        self.wait_for(lambda: 'partner' in Path(f'/proc/{worker.pid}/wchan').read_text())
        # The worker already read its current record and is opening our FIFO.
        # Swap the still-live pin and offer before allowing that read to finish.
        self.change_pin(other)
        self.offer(directory)
        held = os.open(self.transcript_fifo, os.O_PATH)
        try:
            # Keep the blocked open on its original inode, while subsequent
            # transcript checks see a regular empty file and cannot block.
            self.transcript_fifo.unlink(); self.transcript_fifo.write_text('')
            fd = os.open(f'/proc/self/fd/{held}', os.O_WRONLY | os.O_NONBLOCK)
            os.close(fd)
        finally:
            os.close(held)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(output, '')
        self.assertIn('caller', errors.lower())
        self.assertTrue((directory / 'answer.json').exists())
        self.assertFalse((directory / 'claimed.json').exists())
        self.assertFalse((directory / 'emitted.json').exists())

    @unittest.skipUnless(sys.platform == 'linux', 'Linux private child lock-wait witness')
    def test_lost_pin_after_claim_preserves_uncertainty_without_decision(self):
        worker, directory, other = self.waiting_question()
        with (directory / 'handover.lock').open('w') as held:
            fcntl.flock(held, fcntl.LOCK_EX)
            self.offer(directory)
            # This is our owned hgs child, blocked in claim() after its initial
            # caller recheck. No PID or descriptor of an owner session is read.
            self.wait_for(lambda: 'lock' in Path(f'/proc/{worker.pid}/wchan').read_text())
            self.change_pin(other)
            fcntl.flock(held, fcntl.LOCK_UN)
        output, errors = worker.communicate(timeout=8)
        self.assertEqual(output, '')
        self.assertIn('caller', errors.lower())
        self.assertTrue((directory / 'claimed.json').exists())
        self.assertFalse((directory / 'emitted.json').exists())


if __name__ == '__main__':
    unittest.main()
