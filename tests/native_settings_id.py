"""Shared genuine native Settings contract, entered through the non-test CLI.

Callers own an isolated official package, HOME/config/socket and raw record.
Moving that same legacy record uses the already sealed private transition seed.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import time
import uuid

import native_effort_id as identity
import test_input


def change_and_replay(case, name, record, path, model, effort, attempts):
    sid = record['session_id']
    request = dict(request_id=str(uuid.uuid4()), expected_run_id=record['run_id'],
                   expected_conversation_id=record.get('conversation_id') or '', model=model, effort=effort)
    raw = ('{\n ' + json.dumps(request, separators=(',', ':'))[1:-1] + '\n}\n')
    command = [str(test_input.HGS), 'settings', name, '--session-id', sid, '--json']

    def call(body):
        return subprocess.run(command, env=case.env, input=body, text=True,
                              capture_output=True, timeout=30)

    before = attempts()
    started = time.monotonic()
    result = call(raw)
    seconds = time.monotonic() - started
    if result.returncode:
        diagnostic = dict(request_id=request['request_id'], seconds=round(seconds, 3),
                          raw_sha256=hashlib.sha256(raw.encode()).hexdigest())
        for label, source in [('record', path), ('receipt', Path(case.env['HGS_STATE_DIR']) /
                              'settings_receipts' / (request['request_id'] + '.json'))]:
            try:
                value = json.loads(identity.bounded_bytes(source, 64 << 10))
                diagnostic[label] = {key: value.get(key) for key in (
                    'status', 'request_id', 'digest', 'session_id', 'run_id', 'conversation_id',
                    'model', 'effort', 'settings_unconfirmed_at', 'pending_settings')}
            except Exception as error:
                diagnostic[label] = dict(snapshot_error=str(error)[:500])
        diagnostic['attempts_tail'] = attempts()[-8192:].decode(errors='replace')
        print('ZERUS_NATIVE_ID_SETTINGS_FAILURE=' + json.dumps(diagnostic), flush=True)
    case.assertEqual(result.returncode, 0, result.stdout + result.stderr)
    receipt = json.loads(result.stdout)
    case.assertEqual(receipt['status'], 'applied')
    case.assertEqual((receipt['request_id'], receipt['digest']),
                     (request['request_id'], hashlib.sha256(raw.encode()).hexdigest()))
    case.assertEqual((receipt['model'], receipt['effort'], receipt['scope']), (model, effort, 'session'))
    case.assertEqual((receipt['run_id'], receipt['conversation_id']),
                     (request['expected_run_id'], request['expected_conversation_id']))
    after = attempts()
    case.assertNotEqual(after, before, 'the actual native Settings controller delivered no keys')
    repeated = call(raw)
    case.assertEqual(repeated.returncode, 0, repeated.stdout + repeated.stderr)
    case.assertEqual(json.loads(repeated.stdout), receipt)
    case.assertEqual(attempts(), after, 'the same raw Settings request typed twice')
    changed = call(raw + ' ')
    case.assertEqual(changed.returncode, 1, changed.stdout + changed.stderr)
    case.assertIn('already used', changed.stderr)
    case.assertEqual(attempts(), after, 'changed raw reached the native provider')
    saved = Path(case.env['HGS_STATE_DIR']) / 'settings_receipts' / (request['request_id'] + '.json')
    case.assertEqual(json.loads(saved.read_bytes()), receipt)
    case.assertFalse((saved.parent.parent / 'effort_receipts' / saved.name).exists())
    current = json.loads(path.read_bytes())
    for key in ('session_id', 'run_id', 'conversation_id', 'pid', 'process_start',
                'supervisor', 'terminal_name', 'agent', 'agent_home'):
        case.assertEqual(current.get(key), record.get(key), key)
    case.assertEqual((current['model'], current['effort']), (model, effort))
    case.assertNotIn('settings_unconfirmed_at', current)
    case.assertNotIn('pending_settings', current)
    case.assertEqual(current['resume_settings'], dict(model=model, effort=effort))
    print('ZERUS_NATIVE_ID_SETTINGS=' + json.dumps(dict(
        case=case.id(), agent=record['agent'], kind='LegacyMoved', request_id=request['request_id'],
        raw_sha256=receipt['digest'], hgs_sha256=identity.image_digest(test_input.HGS),
        seconds=round(seconds, 3), confirmed=True, repeated_typed=False, changed_raw_refused=True,
        empty_conversation=request['expected_conversation_id'] == '')), flush=True)
    return current
