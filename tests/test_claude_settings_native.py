"""Real Claude /model picker in a private home and socket, with synthetic history.

Run with HGS_CLAUDE_TEST_BIN=/absolute/path/to/claude (otherwise skipped). A
fixture API key and an unused loopback endpoint keep every request local.
"""
import datetime
import copy
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

import test_input as fixtures
import test_claude_questions_native as native

EFFORTS = [dict(id=value) for value in ['low', 'medium', 'high', 'xhigh', 'max']]
CATALOG = dict(version=2, fetchedAt=0, staleAt=0, catalog=dict(surface='cc', state={}, config=dict(id='cc', models=[
    dict(id='claude-opus-5-5', name='Opus 5.5', short_name='Opus', section='main', thinking=dict(type='effort', effort_options=EFFORTS)),
    dict(id='claude-fable-5-1', name='Fable 5.1', short_name='Fable', section='main', thinking=dict(type='effort', effort_options=EFFORTS)),
    dict(id='claude-sonnet-5-5', name='Sonnet 5.5', short_name='Sonnet', section='main', thinking=dict(type='effort', effort_options=EFFORTS)),
    dict(id='claude-haiku-5-5', name='Haiku 5.5', short_name='Haiku', section='main', thinking=dict(type='effort', effort_options=EFFORTS))])))


@unittest.skipUnless(native.CLAUDE, 'Set HGS_CLAUDE_TEST_BIN for isolated native model settings coverage')
class NativeClaudeSettings(unittest.TestCase):
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    write_record = fixtures.InputTransport.write_record
    wait_for = native.NativeClaudeQuestions.wait_for
    screen = native.NativeClaudeQuestions.screen

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='hgs-claude-settings-native-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / '.local/bin'
        self.bin.mkdir(parents=True)
        self.state = self.root / 'state'
        self.state.mkdir()
        work = self.root / 'work'
        work.mkdir()
        self.config = self.root / '.claude'
        (self.config / 'cache/model-catalog').mkdir(parents=True)
        self.catalog = copy.deepcopy(CATALOG)
        served = '_served_' in self._testMethodName
        if served:
            self.catalog['catalog']['config']['models'].extend([
                dict(id=model, name=label, short_name='Fable', section='overflow',
                     thinking=dict(type='effort', effort_options=EFFORTS))
                for model, label in [('claude-fable-5-2', 'Fable 5.2'), ('claude-fable-5', 'Fable 5')]])
        (self.config / 'cache/model-catalog/fixture-000000000000-cc.json').write_text(json.dumps(self.catalog))
        (self.root / '.claude.json').write_text(json.dumps(dict(
            hasCompletedOnboarding=True, hasSeenAutoDefaultNudge=True, theme='dark',
            customApiKeyResponses=dict(approved=['fixture-key'], rejected=[]),
            projects={str(work): dict(hasTrustDialogAccepted=True, hasCompletedProjectOnboarding=True)})))
        self.settings_file = self.config / 'settings.json'
        self.settings_file.write_text('{"model":"opus","effortLevel":"medium"}\n' if served else '{}\n')
        self.catalog_fixture = None
        if served:
            from claude_catalog_fixture import CatalogFixture
            self.catalog_fixture = CatalogFixture(self.root, self.catalog)
            self.addCleanup(self.catalog_fixture.close)
            self.catalog_fixture.seed(self.root)
        self.conversation_id = str(uuid.uuid4())
        self.transcript = self.config / 'projects' / re.sub(r'[^A-Za-z0-9]', '-', str(work)) / (self.conversation_id + '.jsonl')
        self.transcript.parent.mkdir(parents=True)
        resumed = 'resumed' in self._testMethodName
        if resumed:
            # One cached reply makes Claude warn before a change re-reads history.
            stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
            first = str(uuid.uuid4())
            common = dict(sessionId=self.conversation_id, cwd=str(work), timestamp=stamp,
                          isSidechain=False, userType='external', version='2.1.296')
            self.transcript.write_text('\n'.join(json.dumps(row) for row in [
                dict(common, type='user', uuid=first, parentUuid=None,
                     message=dict(role='user', content='Synthetic earlier request.')),
                dict(common, type='assistant', uuid=str(uuid.uuid4()), parentUuid=first,
                     requestedModel='claude-opus-5-5', effort='medium',
                     message=dict(id='fixture-reply', type='message', role='assistant', model='claude-opus-5-5',
                                  content=[dict(type='text', text='Synthetic earlier reply.')],
                                  stop_reason='end_turn', stop_sequence=None,
                                  usage=dict(input_tokens=10, output_tokens=50,
                                             cache_read_input_tokens=0, cache_creation_input_tokens=5000))),
                dict(type='cost-state', sessionId=self.conversation_id, totalCostUSD=0.01, totalAPIDuration=1000,
                     totalAPIDurationWithoutRetries=1000, totalToolDuration=0, totalLinesAdded=0, totalLinesRemoved=0,
                     totalDuration=2000, startTime=int(time.time() * 1000) - 5000,
                     modelUsage={'claude-opus-5-5': dict(inputTokens=10, outputTokens=50, cacheReadInputTokens=0,
                                                         cacheCreationInputTokens=5000, webSearchRequests=0, costUSD=0.01)},
                     hasUnknownModelCost=False)]) + '\n')
        self.socket = self.root / 'socket'
        self.script('tmux', '#!/bin/sh\nexec ' + shlex.quote(fixtures.TMUX) + ' -S ' +
                    shlex.quote(str(self.socket)) + ' -f /dev/null "$@"\n')
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('HGS_')}
        self.env.update(HOME=str(self.root), HGS_STATE_DIR=str(self.state),
                        HGS_CONFIG_DIR=str(self.root / 'config'), HGS_SELF='test',
                        PATH=str(self.bin) + ':' + os.environ['PATH'])
        self.env.pop('TMUX', None)
        self.env.pop('TMUX_PANE', None)
        self.name = 'claude/settings-test'
        self.run_id = str(uuid.uuid4())
        self.addCleanup(lambda: fixtures.InputTransport.tearDown(self))
        if 'hook' in self._testMethodName:
            # The entry hgs registers for Claude Code 2.1.296, run as this run's hook.
            hook = dict(HGS_SESSION=self.name, HGS_RUN_ID=self.run_id, HGS_STATE_DIR=str(self.state),
                        HGS_CONFIG_DIR=str(self.root / 'config'), PATH=self.env['PATH'])
            self.hook_capture = self.root / 'post-model-switch.json'
            observer = self.root / 'post-model-switch.py'
            observer.write_text(
                'import os,pathlib,sys,tempfile\n'
                'raw=sys.stdin.buffer.read(65537)\n'
                'assert len(raw)<=65536\n'
                'capture=pathlib.Path(' + repr(str(self.hook_capture)) + ')\n'
                'with tempfile.NamedTemporaryFile(dir=capture.parent,delete=False) as evidence:\n'
                ' temporary=pathlib.Path(evidence.name)\n'
                ' try:\n'
                '  evidence.write(raw);evidence.flush();os.replace(temporary,capture)\n'
                ' finally:temporary.unlink(missing_ok=True)\n'
                'with tempfile.TemporaryFile() as original:\n'
                ' original.write(raw);original.flush();original.seek(0)\n'
                ' os.dup2(original.fileno(),0)\n'
                ' os.execv(' + repr(str(fixtures.HGS)) + ',' +
                repr([str(fixtures.HGS), '__state', 'hook']) + ')\n')
            command = ('env ' + ' '.join(shlex.quote(key + '=' + value) for key, value in hook.items()) + ' '
                       + shlex.quote(sys.executable) + ' ' + shlex.quote(str(observer)))
            self.settings_file.write_text(json.dumps(dict(hooks=dict(PostModelSwitch=[
                dict(hooks=[dict(type='command', command=command, timeout=5)])]))))
        launch = ['--resume', self.conversation_id] if resumed else ['--session-id', self.conversation_id]
        native_environment = dict(HOME=str(self.root), PATH=os.environ['PATH'],
                                  SHELL='/bin/bash', TERM='xterm-256color', LANG='C.UTF-8',
                                  DISABLE_AUTOUPDATER='1')
        if self.catalog_fixture:
            native_environment.update(self.catalog_fixture.environment())
        else:
            native_environment.update(CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1',
                                      ANTHROPIC_API_KEY='fixture-key', ANTHROPIC_BASE_URL='http://127.0.0.1:1')
        self.tmux('new-session', '-d', '-s', self.name, '-x', '120', '-y', '36', '-c', str(work),
                  'env', '-i', *[key + '=' + value for key, value in native_environment.items()],
                  native.CLAUDE, *launch)
        self.pane = self.tmux('display-message', '-p', '-t', '=' + self.name + ':', '#{pane_id}').strip()
        self.wait_for(lambda: '· /effort' in self.screen())
        if resumed:
            self.assertIn('Synthetic earlier reply.', self.screen())
        pid = int(self.tmux('display-message', '-p', '-t', self.pane, '#{pane_pid}'))
        start = subprocess.check_output(['ps', '-p', str(pid), '-o', 'lstart='],
                                        env=dict(self.env, LC_ALL='C', TZ='UTC'), text=True).strip()
        self.tmux('set-option', '-t', '=' + self.name + ':', '@hgs_run', self.run_id)
        self.record_path = self.state / (hashlib.sha256(self.name.encode()).hexdigest() + '.json')
        self.record = dict(version=1, name=self.name, agent='claude', run_id=self.run_id,
                           conversation_id=self.conversation_id, transcript=str(self.transcript),
                           agent_home=str(self.config), model='claude-opus-5-5', effort='medium',
                           launch_dir=str(work), pane=self.pane, pid=pid, process_start=start,
                           activity='idle', phase='idle', active_tools={}, subagents={}, last_event_at=time.time())
        self.write_record()
        # Record only the adapter's input, after native startup.
        logger = self.bin / 'tmux-log.py'
        logger.write_text('import json,pathlib,subprocess,sys\n' +
                          'args=sys.argv[1:]\n' +
                          "if args and args[0]=='send-keys':\n" +
                          ' with pathlib.Path(' + repr(str(self.root / 'sent-keys')) +
                          ").open('a') as log:log.write(json.dumps(args)+'\\n')\n" +
                          'sys.exit(subprocess.call(' + repr([fixtures.TMUX, '-S', str(self.socket), '-f', '/dev/null']) + '+args))\n')
        self.script('tmux', '#!/bin/sh\nexec python3 ' + shlex.quote(str(logger)) + ' "$@"\n')

    def run_hgs(self, *args, payload=None):
        changing = args and args[0] in ('settings', 'effort')
        before = self.settings_file.read_bytes() if changing else None
        try:
            return subprocess.run([str(fixtures.HGS), *args], env=self.env, text=True, capture_output=True, timeout=60,
                                  input=None if payload is None else json.dumps(payload))
        finally:
            if changing:
                self.assertEqual(self.settings_file.read_bytes(), before,
                                 'A session change rewrote the account model/effort defaults')

    def inspect(self):
        result = self.run_hgs('inspect', self.name)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def change(self, model, effort):
        payload = dict(request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                       expected_conversation_id=self.conversation_id, model=model, effort=effort)
        result = self.run_hgs('settings', self.name, '--json', payload=payload)
        self.assertEqual(result.returncode, 0, result.stderr + '\n' + self.screen())
        return json.loads(result.stdout)

    def keys(self):
        path = self.root / 'sent-keys'
        return [json.loads(line)[3:] for line in path.read_text().splitlines()] if path.exists() else []

    def receipts(self):
        rows = native.records(self.transcript)
        return [row['message']['content'] for row in rows
                if row.get('type') == 'user' and isinstance(row.get('message', {}).get('content'), str)
                and 'Set model to' in row['message']['content']]

    def test_fresh_session_changes_model_and_effort_without_defaults(self):
        info = self.inspect()
        self.assertTrue(info['settings_change_supported'], info['settings_change_reason'])
        before = self.settings_file.read_bytes()
        self.assertEqual(self.change('claude-sonnet-5-5', 'high')['status'], 'applied')
        self.assertEqual(self.receipts(), ['<local-command-stdout>Set model to `Sonnet 5.5` for this session only with `high` effort</local-command-stdout>'])
        keys = self.keys()
        self.assertEqual(keys[0], ['-l', '/model'])
        self.assertEqual(keys[-1], ['s'])
        self.assertNotIn(['Enter'], keys[2:])
        self.assertIn('● high · /effort', self.screen())
        info = self.inspect()
        self.assertEqual((info['model'], info['effort']), ('claude-sonnet-5-5', 'high'))
        # Effort alone, on the session's own row of the picker.
        self.assertEqual(self.change('claude-sonnet-5-5', 'low')['status'], 'applied')
        self.assertIn('○ low · /effort', self.screen())
        self.assertEqual(self.inspect()['effort'], 'low')
        self.assertEqual(self.settings_file.read_bytes(), before)

    def test_id_settings_unprimed_model_and_effort_preserve_shared_defaults(self):
        import native_effort_id as identity
        import native_settings_id as settings_contract
        before = self.settings_file.read_bytes()
        self.assertFalse(self.transcript.exists(), 'the first model receipt must not be primed')
        record, path, legacy = identity.moved_record(self, self.name, self.record)
        self.record, self.record_path = record, path
        attempts = identity.key_log(self, existing=self.root / 'sent-keys')
        record = settings_contract.change_and_replay(self, self.name, record, path,
                                                    'claude-sonnet-5-5', 'high', attempts)
        self.assertTrue(self.transcript.exists(), '/model did not create its genuine transcript')
        self.assertEqual(self.receipts(), ['<local-command-stdout>Set model to `Sonnet 5.5` for this session only with `high` effort</local-command-stdout>'])
        self.assertEqual(self.keys()[0], ['-l', '/model'])
        self.assertEqual(self.keys()[-1], ['s'])
        self.assertEqual(self.settings_file.read_bytes(), before)
        self.assertFalse(legacy.exists())

    def test_id_effort_preserves_raw_request_identity_and_shared_defaults(self):
        import native_effort_id as contract
        before = self.settings_file.read_bytes()
        record, path, legacy = contract.moved_record(self, self.name, self.record)
        self.record, self.record_path = record, path
        attempts = contract.key_log(self, existing=self.root / 'sent-keys')
        observed = contract.inspect(self, self.name, record['session_id'])
        self.assertTrue(observed['effort_change_supported'], observed['effort_change_reason'])
        choices = ('low', 'high') if observed['effort'] == 'high' else ('high', 'low')
        for wanted in choices:
            self.assertIn(wanted, observed['effort_options'])
            record = contract.change_and_replay(self, self.name, record, path, wanted, attempts)
            self.assertEqual(self.settings_file.read_bytes(), before)
            self.assertIn(('● high' if wanted == 'high' else '○ low') + ' · /effort', self.screen())
            self.assertEqual(contract.inspect(self, self.name, record['session_id'])['effort'], wanted)
        self.assertEqual(self.settings_file.read_bytes(), before)
        self.assertFalse(legacy.exists(), 'a native hook recreated the retired legacy record')

    def test_terminal_model_change_reaches_hgs_through_its_hook(self):
        before = self.settings_file.read_bytes()
        # The person changes the model in Terminal, for this session only.
        self.tmux('send-keys', '-t', self.pane, '-l', '/model')
        time.sleep(.3)
        self.tmux('send-keys', '-t', self.pane, 'Enter')
        self.wait_for(lambda: 's to use this session only' in self.screen())
        for _ in range(8):
            row = next((line for line in self.screen().splitlines() if re.match(r'\s*❯ \d\.', line)), '')
            if 'Fable' in row:
                break
            self.tmux('send-keys', '-t', self.pane, 'Down')
            time.sleep(.2)
        self.assertIn('Fable', row, self.screen())
        self.tmux('send-keys', '-t', self.pane, 's')
        # Claude's PostModelSwitch names the exact ID, not the picker's display name.
        self.wait_for(self.hook_capture.exists)
        event = json.loads(self.hook_capture.read_bytes())
        print('ZERUS_NATIVE_MODEL_SWITCH=' + json.dumps({
            key: event.get(key) for key in ('hook_event_name', 'session_id', 'from_model', 'to_model')}))
        self.assertEqual(event['hook_event_name'], 'PostModelSwitch')
        self.assertEqual(event['session_id'], self.conversation_id)
        self.assertEqual(event['from_model'], self.record['model'])
        target = event['to_model']
        self.assertRegex(target, r'^claude-fable-[A-Za-z0-9][A-Za-z0-9._-]*(?:\[1m\])?$')
        self.assertNotEqual(target, event['from_model'])
        record = lambda: json.loads(self.record_path.read_text())
        self.wait_for(lambda: record()['model'] == target)
        self.assertEqual(record()['model_source_path'], str(self.transcript))
        self.assertEqual(self.inspect()['model'], target)
        self.assertEqual(self.settings_file.read_bytes(), before)

    def test_resumed_conversation_confirms_claudes_cache_notice(self):
        before = self.settings_file.read_bytes()
        self.assertEqual(self.change('claude-haiku-5-5', 'medium')['status'], 'applied')
        keys = self.keys()
        self.assertEqual(keys[-2:], [['s'], ['Enter']])
        self.assertTrue(self.receipts()[-1].startswith('<local-command-stdout>Set model to `Haiku 5.5` for this session only'))
        self.assertEqual(self.inspect()['model'], 'claude-haiku-5-5')
        self.assertEqual(self.settings_file.read_bytes(), before)

    def test_resumed_served_hidden_model_expands_and_preserves_account_defaults(self):
        self.assertEqual(self.change('claude-fable-5-2', 'xhigh')['status'], 'applied')
        self.assertIn('Set model to `Fable 5.2` for this session only', self.receipts()[-1])
        self.assertEqual(self.inspect()['model'], 'claude-fable-5-2')
        self.assertIn(['s'], self.keys())
        self.assertNotIn('Select model', self.screen())
        self.assertIn('xhigh · /effort', self.screen())

    def test_resumed_served_ordinary_model_preserves_account_defaults(self):
        self.assertEqual(self.change('claude-sonnet-5-5', 'high')['status'], 'applied')
        self.assertIn('Set model to `Sonnet 5.5` for this session only', self.receipts()[-1])
        self.assertNotIn('Select model', self.screen())

    def test_resumed_served_effort_only_preserves_account_defaults(self):
        for command, effort in [('settings', 'high'), ('effort', 'low')]:
            if command == 'settings':
                self.assertEqual(self.change('claude-opus-5-5', effort)['status'], 'applied')
            else:
                result = self.run_hgs('effort', self.name, '--json', payload=dict(
                    request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
                    expected_conversation_id=self.conversation_id, effort=effort))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)['status'], 'confirmed')
            self.assertEqual(self.inspect()['effort'], effort)
            self.assertNotIn('Select model', self.screen())

    def test_resumed_served_unoffered_model_closes_the_expanded_picker(self):
        stale = copy.deepcopy(self.catalog)
        stale['catalog']['config']['models'].append(dict(
            id='claude-fable-unavailable', name='Unavailable Fable', section='main',
            thinking=dict(type='effort', effort_options=EFFORTS)))
        (self.config / 'cache/model-catalog/stale-fixture-cc.json').write_text(json.dumps(stale))
        result = self.run_hgs('settings', self.name, '--json', payload=dict(
            request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
            expected_conversation_id=self.conversation_id, model='claude-fable-unavailable', effort='high'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('model is not offered', result.stderr)
        self.assertEqual(self.keys()[-1], ['Escape'])
        self.assertNotIn(['s'], self.keys())
        self.assertNotIn('Select model', self.screen())

    def test_resumed_served_collapsed_picker_timeout_is_cancelled(self):
        logger = self.bin / 'tmux-log.py'
        ignored = self.root / 'ignored-section-expansion'
        capture = [fixtures.TMUX, '-S', str(self.socket), '-f', '/dev/null',
                   'capture-pane', '-p', '-t', self.pane]
        injection = (
            "if args[:1]==['send-keys'] and args[-1:] == ['Enter']:\n"
            ' screen=subprocess.check_output(' + repr(capture) + ',text=True)\n'
            " if 'Select model' in screen and 'Enter to expand' in screen:\n"
            '  pathlib.Path(' + repr(str(ignored)) + ").write_text('ignored once')\n"
            '  sys.exit(0)\n')
        logger.write_text(logger.read_text().replace('sys.exit(subprocess.call(',
                                                    injection + 'sys.exit(subprocess.call('))
        result = self.run_hgs('settings', self.name, '--json', payload=dict(
            request_id=str(uuid.uuid4()), expected_run_id=self.run_id,
            expected_conversation_id=self.conversation_id, model='claude-fable-5-2', effort='xhigh'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('provider did not confirm the expected change', result.stderr)
        self.assertTrue(ignored.exists(), 'the native collapsed-section control was not reached')
        self.assertEqual(self.keys()[-1], ['Escape'])
        self.assertNotIn(['s'], self.keys())
        self.assertNotIn('Select model', self.screen())

    def test_resumed_served_id_failure_closes_only_its_original_picker(self):
        import native_effort_id as contract
        record, path, legacy = contract.moved_record(self, self.name, self.record)
        self.record, self.record_path = record, path
        stale = copy.deepcopy(self.catalog)
        for model in stale['catalog']['config']['models']:
            if model['id'] == 'claude-opus-5-5':
                model['name'] = 'Unavailable Opus'
        (self.config / 'cache/model-catalog/stale-fixture-cc.json').write_text(json.dumps(stale))
        payload = dict(request_id=str(uuid.uuid4()).upper(), expected_run_id=self.run_id,
                       expected_conversation_id=self.conversation_id, effort='high')
        result = self.run_hgs('effort', self.name, '--session-id', record['session_id'], '--json', payload=payload)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('model is not offered', result.stderr)
        self.assertEqual(self.keys()[-1], ['Escape'])
        self.assertNotIn(['s'], self.keys())
        self.assertNotIn('Select model', self.screen())
        self.assertFalse(legacy.exists())
        before = self.keys()
        repeated = self.run_hgs('effort', self.name, '--session-id', record['session_id'], '--json', payload=payload)
        self.assertNotEqual(repeated.returncode, 0)
        self.assertEqual(self.keys(), before, 'an uncertain request replayed native keys')


if __name__ == '__main__':
    unittest.main()
