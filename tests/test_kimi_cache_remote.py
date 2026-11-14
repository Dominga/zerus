"""Kimi cache answers keep caller refusal on this side of a fake SSH hop."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid

import test_remote_login_shells as remote


@unittest.skipIf(os.name == 'nt', 'Private POSIX peer login-shell fixture')
class KimiCacheRemote(unittest.TestCase):
    def setUp(self):
        remote.RemoteLoginShells.setUp(self)
        bindir = self.root / 'bin'
        bindir.mkdir()
        ssh = bindir / 'ssh'
        ssh.write_text(remote.FAKE_SSH)
        ssh.chmod(0o700)
        self.env.pop('HGS_SSH', None)
        self.env['PATH'] = str(bindir) + os.pathsep + self.env['PATH']
        self.env['REMOTE_LOGIN_SHELL'] = remote.login_shells()['sh']
        (self.peer / '.local/bin/hgs').write_text(
            remote.PEER_HGS.replace("'hooks-health-v1'", "'hooks-health-v1' 'guarded-create-v1'"))
        machine = '55555555-5555-4555-8555-555555555555'
        self.forms = (
            ['@peer', 'answer', 'kimi/example/child', '--json'],
            ['@peer', '--expect-machine', machine, 'answer', 'kimi/example/child', '--json'],
            ['@peer', '--expect-machine', machine, '--client', 'fixture', 'answer', 'kimi/example/child', '--json'],
            ['@peer', '__state', 'answer', 'kimi/example/child', '--json'],
            ['@peer', '--expect-machine', machine, '__state', 'answer', 'kimi/example/child', '--json'],
            ['@peer', '--expect-machine', machine, '--client', 'fixture', '__state', 'answer', 'kimi/example/child', '--json'],
        )
        self.agent = dict(HGS_SESSION='codex/fixture/parent', HGS_RUN_ID='private-parent-run')

    def payload(self, question=None, choice=0):
        value = dict(request_id=str(uuid.uuid4()), expected_run_id='private-child-run',
                     expected_conversation_id='private-child-conversation',
                     question_id=question or 'kimi-cache:' + 'a' * 64,
                     expected_question_hash='a' * 64,
                     answers=[dict(question_id='cache', selected_option_ids=['cache_' + str(choice)],
                                   text='' if question is None else 'Пример \\ kimi-cache: is ordinary text')])
        return b' \t\r\n' + json.dumps(value, ensure_ascii=False, indent=2).replace('\n', '\r\n').encode() + b'\r\n\t '

    def call(self, form, body, caller=None):
        (self.root / 'ssh-args.json').unlink(missing_ok=True)
        for name in ['argv.json', 'stdin']:
            (self.peer / name).unlink(missing_ok=True)
        return subprocess.run([str(remote.HGS), *form], input=body, capture_output=True,
                              timeout=30, env=dict(self.env, **(caller or {})))

    def assert_refused(self, result):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(b"Kimi's context choice", result.stderr)
        self.assertFalse((self.root / 'ssh-args.json').exists(), 'refusal must precede every SSH/probe')
        self.assertFalse((self.peer / 'stdin').exists())

    def assert_forwarded(self, result, body):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.root / 'ssh-args.json').is_file())
        self.assertEqual((self.peer / 'stdin').read_bytes(), body, 'forwarding must preserve the original bytes')

    def test_agent_cache_answers_refuse_every_option_and_wrapper_before_ssh(self):
        for form in self.forms:
            for caller in [self.agent, dict(HGS_AGENT='dsh')]:
                for choice in range(4):
                    with self.subTest(form=form, caller=caller, choice=choice):
                        self.assert_refused(self.call(form, self.payload(choice=choice), caller))

    def test_native_background_agent_without_markers_cannot_forward_even_compact(self):
        state = Path(self.env['HGS_STATE_DIR'])
        state.mkdir(exist_ok=True)
        window = subprocess.Popen(['sleep', '60'])
        self.addCleanup(window.wait)
        self.addCleanup(window.kill)
        name = 'claude/fixture/parent'
        conversation, job = str(uuid.uuid4()), str(uuid.uuid4())
        start = subprocess.check_output(['ps', '-p', str(window.pid), '-o', 'lstart='],
                                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        record = dict(version=1, name=name, agent='claude', run_id='private-parent-run',
                      conversation_id=conversation, session_id=str(uuid.uuid4()),
                      activity='idle', phase='idle', active_tools={}, subagents={},
                      pid=window.pid, process_start=start,
                      claude_registry_home=str(self.root / 'claude'))
        (state / (hashlib.sha256(name.encode()).hexdigest() + '.json')).write_text(json.dumps(record))
        registry = self.root / 'claude/sessions'
        registry.mkdir(parents=True)
        (registry / f'{window.pid}.json').write_text(json.dumps(
            dict(pid=window.pid, kind='interactive', parkedJobId=job)))
        (registry / f'{os.getpid()}.json').write_text(json.dumps(
            dict(pid=os.getpid(), kind='bg', jobId=job, sessionId=conversation)))
        caller = dict(CLAUDE_PID=str(os.getpid()), CLAUDE_CODE_SESSION_ID=conversation,
                      CLAUDE_CONFIG_DIR=str(self.root / 'claude'))
        self.assert_refused(self.call(self.forms[0], self.payload(choice=0), caller))

    def test_escaped_source_prefix_is_still_refused_before_ssh(self):
        body = self.payload().replace(b'kimi-cache:', b'kimi-\\u0063ache:', 1)
        self.assertEqual(json.loads(body)['question_id'], 'kimi-cache:' + 'a' * 64)
        self.assert_refused(self.call(self.forms[0], body, self.agent))

    def test_person_and_client_wrappers_keep_all_cache_answer_bytes(self):
        for form in self.forms:
            for choice in range(4):
                with self.subTest(form=form, choice=choice):
                    body = self.payload(choice=choice)
                    self.assert_forwarded(self.call(form, body), body)

    def test_other_answer_sources_keep_exact_bytes_for_agents_and_people(self):
        for form in self.forms:
            for caller in [{}, self.agent]:
                for question in ['ordinary-question', 'kimi-cachex:' + 'b' * 64, 'hook:kimi-cache:' + 'c' * 64]:
                    with self.subTest(form=form, caller=caller, question=question):
                        body = self.payload(question=question)
                        self.assert_forwarded(self.call(form, body, caller), body)
        body = b' \n{not JSON}\x00\xff'
        self.assert_forwarded(self.call(self.forms[0], body, self.agent), body)


if __name__ == '__main__':
    unittest.main()
