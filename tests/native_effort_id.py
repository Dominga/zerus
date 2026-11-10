"""Shared actual-CLI ID Effort contract for maintained real native fixtures.

The runner supplies a separately built, hash-pinned Rust test image for the
isolated owner-transfer setup. Every Effort request uses the production CLI.
The native executable/profile/terminal remain owned by the provider fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import uuid

import test_input

SEED_METHOD = 'state::effort::picker_scope::tests::effort::seed_actual_effort_record'


def image_digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def moved_record(case, name, record):
    """Move this exact saved legacy fixture through the real transition code."""
    image = Path(os.environ['ZERUS_PICKER_EFFORT_SEED_IMAGE']).resolve(strict=True)
    expected = os.environ['ZERUS_PICKER_EFFORT_SEED_SHA256']
    case.assertEqual(image_digest(image), expected)
    root = Path(case.root).resolve()
    state = Path(case.env['HGS_STATE_DIR']).resolve()
    case.assertEqual(Path(case.env['HOME']).resolve(), root)
    case.assertEqual(state.parent, root)
    state.chmod(0o700)
    case.assertEqual(record['name'], name)
    case.assertIn(record['agent'], ('claude', 'codex', 'kimi'))
    original = {key: record.get(key) for key in (
        'session_id', 'run_id', 'conversation_id', 'pid', 'process_start',
        'supervisor', 'terminal_name', 'agent', 'agent_home')}
    source = root / 'native-effort-record.json'
    source.write_text(json.dumps(record))
    env = dict(case.env, ZERUS_PICKER_EFFORT_SEED='1',
               ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND='moved')
    seeded = subprocess.run([str(image), '--exact', SEED_METHOD, '--ignored', '--test-threads=1'],
                            env=env, stdin=subprocess.DEVNULL, capture_output=True,
                            text=True, timeout=15)
    case.assertEqual(seeded.returncode, 0, seeded.stdout + seeded.stderr)
    case.assertEqual(image_digest(image), expected)
    current = json.loads(source.read_bytes())
    sid = current['session_id']
    case.assertEqual(str(uuid.UUID(sid)), sid)
    case.assertEqual(current['layout'], 'id')
    if original['session_id']:
        case.assertEqual(sid, original['session_id'])
    for key, value in original.items():
        if key != 'session_id' and value is not None:
            case.assertEqual(current.get(key), value, key)
    path = state / 'sessions' / (sid + '.json')
    case.assertEqual(json.loads(path.read_bytes()), current)
    legacy = state / (hashlib.sha256(name.encode()).hexdigest() + '.json')
    case.assertFalse(legacy.exists(), 'the original legacy record was not retained by the transition')
    return current, path, legacy


def key_log(case):
    """Observe actual attempts while forwarding to the unchanged private tmux."""
    original = Path(case.bin) / 'tmux-native-effort-original'
    current = Path(case.bin) / 'tmux'
    current.rename(original)
    log = Path(case.root) / 'native-effort-keys.jsonl'
    script = Path(case.root) / 'native-effort-tmux.py'
    script.write_text('import json,pathlib,subprocess,sys\n'
                      'args=sys.argv[1:]\n'
                      "if args[:1]==['send-keys']:\n"
                      ' with pathlib.Path(' + repr(str(log)) + ").open('a') as file:\n"
                      "  file.write(json.dumps(args)+'\\n')\n"
                      'raise SystemExit(subprocess.call([' + repr(str(original)) + ']+args))\n')
    case.script('tmux', '#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' ' +
                shlex.quote(str(script)) + ' "$@"\n')

    def attempts():
        return log.read_bytes() if log.exists() else b''
    return attempts


def inspect(case, name, sid):
    result = subprocess.run([str(test_input.HGS), 'inspect', name, '--session-id', sid],
                            env=case.env, text=True, capture_output=True, timeout=30)
    case.assertEqual(result.returncode, 0, result.stdout + result.stderr)
    return json.loads(result.stdout)


def change_and_replay(case, name, record, path, effort, attempts):
    sid = record['session_id']
    request = dict(request_id=str(uuid.uuid4()), expected_run_id=record['run_id'],
                   expected_conversation_id=record['conversation_id'], effort=effort)
    raw = ('{\n  ' + json.dumps(request, separators=(',', ':'))[1:-1] + '\n}\n')
    command = [str(test_input.HGS), 'effort', name, '--session-id', sid, '--json']

    def call(body):
        return subprocess.run(command, env=case.env, input=body, text=True,
                              capture_output=True, timeout=30)

    before = attempts()
    result = call(raw)
    case.assertEqual(result.returncode, 0, result.stdout + result.stderr)
    receipt = json.loads(result.stdout)
    case.assertEqual(receipt['status'], 'confirmed')
    case.assertEqual((receipt['name'], receipt['scope']), (name, 'session'))
    case.assertEqual(receipt['request_id'], request['request_id'])
    case.assertEqual(receipt['digest'], hashlib.sha256(raw.encode()).hexdigest())
    case.assertEqual((receipt['run_id'], receipt['conversation_id'], receipt['effort']),
                     (record['run_id'], record['conversation_id'], effort))
    after = attempts()
    case.assertNotEqual(after, before, 'the native effort transaction delivered no keys')
    repeated = call(raw)
    case.assertEqual(repeated.returncode, 0, repeated.stdout + repeated.stderr)
    case.assertEqual(json.loads(repeated.stdout), receipt)
    case.assertEqual(attempts(), after, 'the same request typed into the native agent twice')
    changed = call(raw + ' ')
    case.assertNotEqual(changed.returncode, 0, changed.stdout)
    case.assertIn('already used', changed.stderr)
    case.assertEqual(attempts(), after, 'a changed raw request reached the native agent')
    current = json.loads(path.read_bytes())
    for key in ('session_id', 'run_id', 'conversation_id', 'pid', 'process_start',
                'supervisor', 'terminal_name', 'agent', 'agent_home'):
        case.assertEqual(current.get(key), record.get(key), key)
    case.assertEqual(current['effort'], effort)
    case.assertNotIn('settings_unconfirmed_at', current)
    print('ZERUS_NATIVE_ID_EFFORT=' + json.dumps(dict(
        case=case.id(), agent=record['agent'], kind='LegacyMoved',
        request_id=request['request_id'], raw_sha256=receipt['digest'],
        hgs_sha256=image_digest(test_input.HGS),
        seed_sha256=os.environ['ZERUS_PICKER_EFFORT_SEED_SHA256'],
        confirmed=True, duplicate_typed=False, changed_raw_refused=True)))
    return current
