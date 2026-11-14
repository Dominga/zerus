"""Opt-in native refusal proof; private records and a scripted terminal only."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock
import uuid

import test_input
from test_delivery_receipt import MACHINE, SESSION


@unittest.skipUnless(test_input.TMUX and os.name != 'nt', 'private Unix terminal fixture')
class NativeSendOutcome(unittest.TestCase):
    def setUp(self):
        self.fixture = test_input.InputTransport(methodName='runTest')
        with mock.patch.object(tempfile, 'tempdir', str(Path(tempfile.gettempdir()).resolve())):
            self.fixture.setUp()
        self.addCleanup(self.fixture.tearDown)
        f = self.fixture
        f.env['HGS_PEERS'] = ''
        f.record.update(session_id=SESSION, phase='input')
        f.write_record()
        folder = f.root / 'config/swarm'
        folder.mkdir(parents=True, mode=0o700)
        (f.root / 'config').chmod(0o700)
        catalog = folder / 'catalog.json'
        catalog.write_text(json.dumps(dict(node_id=MACHINE,
            catalog=dict(schema=1, swarm_id=SESSION, operations={}), bindings={})))
        catalog.chmod(0o600)
        home = f.root / 'native-codex'
        home.mkdir(mode=0o700)
        f.env['CODEX_HOME'] = str(home)
        self.request = f.payload(text='  exact original e\u0301\nkept message  ')
        self.raw = '{\n ' + json.dumps(self.request, separators=(',', ':'))[1:-1] + '\n}\n'
        self.receipt = f.state / 'input_receipts' / (self.request['request_id'] + '.json')

    def send(self, raw=None, versioned=True):
        return subprocess.run([str(test_input.HGS), 'send', self.fixture.name,
            *(['--delivery-outcome-v2'] if versioned else []), '--json'],
            input=self.raw if raw is None else raw, text=True, capture_output=True,
            env=self.fixture.env, timeout=10)

    def lookup(self, **updates):
        query = dict(schema=2, request_id=str(uuid.uuid4()), message_request_id=self.request['request_id'],
            expected_machine_id=MACHINE, expected_session_id=SESSION,
            expected_run_id=self.fixture.run_id, expected_conversation_id=self.fixture.conversation_id,
            provider='codex', native_digest=hashlib.sha256(self.raw.encode()).hexdigest())
        query.update(updates)
        p = subprocess.run([str(test_input.HGS), 'delivery', 'receipt', self.fixture.name, '--json'],
            input=json.dumps(query), text=True, capture_output=True, env=self.fixture.env, timeout=10)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        return json.loads(p.stdout)

    def refused(self):
        before = self.fixture.received()
        p = self.send()
        self.assertEqual(p.returncode, 1, p.stdout + p.stderr)
        self.assertTrue(p.stdout.strip(), 'closed native refusal must survive as a versioned outcome')
        result = json.loads(p.stdout)
        self.assertEqual((result['schema'], result['status'], result['proof']),
                         (2, 'not_offered', 'native_input_lock'))
        self.assertEqual((result['request_id'], result['name'], result['machine_id'], result['session_id']),
                         (self.request['request_id'], self.fixture.name, MACHINE, SESSION))
        self.assertEqual((result['run_id'], result['conversation_id'], result['provider'], result['native_digest']),
                         (self.fixture.run_id, self.fixture.conversation_id, 'codex',
                          hashlib.sha256(self.raw.encode()).hexdigest()))
        self.assertEqual(self.fixture.received(), before)
        record = json.loads(self.fixture.record_path.read_text())
        self.assertNotIn('input_pending_at', record)
        self.assertNotIn('input_pending_request', record)
        return result

    def test_live_refusal_is_exact_and_terminal_only_when_opted_in(self):
        result = self.refused()
        original = self.receipt.read_bytes(), self.receipt.stat().st_mtime_ns
        # Becoming ready cannot turn this spent ID into input, through either API.
        self.fixture.record['phase'] = 'idle'
        self.fixture.write_record()
        before = self.fixture.received()
        duplicate = self.send()
        self.assertEqual(duplicate.returncode, 1, duplicate.stderr)
        self.assertEqual(json.loads(duplicate.stdout), result)
        self.assertEqual((self.receipt.read_bytes(), self.receipt.stat().st_mtime_ns), original)
        legacy = self.send(versioned=False)
        self.assertNotEqual(legacy.returncode, 0)
        self.assertEqual(self.fixture.received(), before)
        for raw in (self.raw + ' ', self.raw.replace('kept message', 'changed message')):
            p = self.send(raw)
            self.assertNotEqual(p.returncode, 0)
            self.assertNotIn('"not_offered"', p.stdout)
        self.assertEqual(self.fixture.received(), before)
        recovered = self.lookup()
        self.assertEqual((recovered['schema'], recovered['status'], recovered['proof']),
                         (2, 'not_offered', 'native_input_lock'))
        self.assertEqual(recovered['message_request_id'], self.request['request_id'])
        self.assertEqual(self.lookup(schema=1)['status'], 'unknown')
        self.assertEqual((self.receipt.read_bytes(), self.receipt.stat().st_mtime_ns), original)
        self.assertEqual(self.fixture.received(), before)

    def test_legacy_negative_stays_retryable_and_is_never_v2_lookup_proof(self):
        p = self.send(versioned=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(p.stdout, '')
        self.assertEqual(json.loads(self.receipt.read_bytes())['status'], 'refused_before_input')
        self.assertEqual(self.lookup()['status'], 'unknown')
        self.fixture.record['phase'] = 'idle'
        self.fixture.write_record()
        p = self.send(versioned=False)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertEqual(json.loads(p.stdout)['status'], 'submitted')

    def test_foreign_corrupt_missing_or_unrecognized_terminal_evidence_stays_unknown(self):
        self.refused()
        original = self.receipt.read_bytes()
        record = json.loads(original)
        for key, value in [('refusal_schema', 1), ('status', 'in_progress'), ('status', 'preparing'),
            ('request_id', str(uuid.uuid4())), ('name', 'codex/foreign'), ('run_id', 'foreign'),
            ('conversation_id', 'foreign'), ('digest', 'b' * 64), ('first_message', True),
            ('transport', 'host'), ('reason', ''), ('expires_at', 0)]:
            with self.subTest(key=key, value=value):
                changed = {**record, key: value}
                self.receipt.write_text(json.dumps(changed))
                before = self.receipt.read_bytes(), self.fixture.received()
                result = self.lookup()
                self.assertEqual(result['status'], 'unknown')
                self.assertNotIn('proof', result)
                self.assertEqual((self.receipt.read_bytes(), self.fixture.received()), before)
        for key in ('machine_id', 'session_id', 'provider', 'namespace'):
            changed = json.loads(original)
            changed['delivery_binding'][key] = 'foreign'
            self.receipt.write_text(json.dumps(changed))
            self.assertEqual(self.lookup()['status'], 'unknown', key)
        for data in (b'{bad', original[:-1] + b',"refusal_schema":2}',
                     original[:-1] + b',"delivery_binding":null}', original + b' ' * 8192):
            self.receipt.write_bytes(data)
            self.assertEqual(self.lookup()['status'], 'unknown')
        self.receipt.unlink()
        self.assertEqual(self.lookup()['status'], 'unknown')
        target = self.receipt.parent / 'foreign-original.json'
        target.write_bytes(original)
        target.chmod(0o600)
        self.receipt.symlink_to(target)
        self.assertEqual(self.lookup()['status'], 'unknown')
        self.receipt.unlink()
        os.mkfifo(self.receipt, mode=0o600)
        self.assertEqual(self.lookup()['status'], 'unknown')

    def test_foreign_query_never_settles_the_original_and_reads_never_repair(self):
        self.refused()
        original = self.receipt.read_bytes(), self.receipt.stat().st_mtime_ns
        for key, value in [('expected_session_id', str(uuid.uuid4())), ('expected_run_id', 'foreign'),
                           ('expected_conversation_id', 'foreign'), ('provider', 'kimi'),
                           ('native_digest', 'b' * 64), ('agent_id', 'foreign')]:
            result = self.lookup(**{key: value})
            self.assertEqual(result['status'], 'unknown', key)
            self.assertNotIn('proof', result)
        self.assertEqual((self.receipt.read_bytes(), self.receipt.stat().st_mtime_ns), original)

    def test_missing_binding_spends_the_opted_in_uuid_without_claiming_not_offered(self):
        del self.fixture.record['session_id']
        self.fixture.write_record()
        # An invalid selected record must not be upgraded using a guessed ID.
        self.fixture.record['session_id'] = 'not-a-session-id'
        self.fixture.write_record()
        p = self.send()
        self.assertNotEqual(p.returncode, 0)
        self.assertNotIn('"not_offered"', p.stdout)
        self.assertEqual(self.lookup()['status'], 'unknown')
        self.assertEqual(self.fixture.received(), b'')

    def test_post_marker_failure_is_unknown_even_when_stderr_claims_not_offered(self):
        f = self.fixture
        f.record['phase'] = 'idle'
        f.write_record()
        original = (f.bin / 'tmux').read_text()
        f.script('tmux', '#!/bin/sh\nif [ "$1" = paste-buffer ]; then '
                 'echo "not offered; refused before typing" >&2; exit 42; fi\n' + original.split('\n', 1)[1])
        p = self.send()
        self.assertNotEqual(p.returncode, 0)
        self.assertNotIn('"not_offered"', p.stdout)
        retained = self.receipt.read_bytes()
        self.assertEqual(json.loads(retained)['status'], 'in_progress')
        self.assertEqual(self.lookup()['status'], 'unknown')
        again = self.send()
        self.assertNotEqual(again.returncode, 0)
        self.assertEqual(self.receipt.read_bytes(), retained)
        self.assertEqual(f.received(), b'')


if __name__ == '__main__':
    unittest.main()
