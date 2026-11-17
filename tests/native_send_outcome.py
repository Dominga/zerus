"""The same real-agent preflight control, with no public model or owner state."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import uuid

import native_effort_id
import test_input


def refused_draft(case, name, record, screen, calls):
    state = Path(case.env['HGS_STATE_DIR']).resolve()
    case.assertTrue(state.is_relative_to(case.root.resolve()))
    path = (state / 'sessions' / (record['session_id'] + '.json') if record.get('layout') == 'id'
            else state / (hashlib.sha256(name.encode()).hexdigest() + '.json'))
    raw_record = json.loads(path.read_bytes())
    if not raw_record.get('session_id'):
        raw_record['session_id'] = str(uuid.uuid4())
        path.write_text(json.dumps(raw_record))
        path.chmod(0o600)
    case.assertEqual((raw_record['run_id'], raw_record.get('conversation_id')),
                     (record['run_id'], record.get('conversation_id')))
    folder = Path(case.env['HGS_CONFIG_DIR']) / 'swarm'
    case.assertTrue(folder.resolve().is_relative_to(case.root.resolve()))
    catalog = folder / 'catalog.json'
    if not catalog.exists():
        folder.mkdir(mode=0o700, parents=True, exist_ok=True)
        catalog.write_text(json.dumps(dict(node_id=str(uuid.uuid4()),
            catalog=dict(schema=1, swarm_id=str(uuid.uuid4()), operations={}), bindings={})))
        catalog.chmod(0o600)
    machine = json.loads(catalog.read_bytes())['node_id']
    keys = native_effort_id.key_log(case)
    terminal = getattr(case, 'tmux', None) or case.t
    draft = 'kept-native-draft'
    terminal('send-keys', '-t', raw_record['pane'], '-l', '--', draft)
    until = time.monotonic() + 10
    while draft not in screen():
        case.assertLess(time.monotonic(), until, 'the native composer never displayed the private draft')
        time.sleep(.02)
    before_keys, before_calls = keys(), calls()
    request = dict(request_id=str(uuid.uuid4()), text='Never send this refusal control.',
        expected_run_id=raw_record['run_id'], expected_conversation_id=raw_record.get('conversation_id') or '')
    raw = '{\n ' + json.dumps(request, separators=(',', ':'))[1:-1] + '\n}\n'
    command = [str(test_input.HGS), 'send', name, '--delivery-outcome-v2', '--json']
    result = subprocess.run(command, input=raw, text=True, capture_output=True, env=case.env, timeout=15)
    case.assertEqual(result.returncode, 1, result.stdout + result.stderr)
    proof = json.loads(result.stdout)
    case.assertEqual((proof['schema'], proof['status'], proof['proof']), (2, 'not_offered', 'native_input_lock'))
    case.assertEqual((proof['request_id'], proof['machine_id'], proof['session_id']),
                     (request['request_id'], machine, raw_record['session_id']))
    case.assertEqual((proof['run_id'], proof['conversation_id'], proof['native_digest']),
                     (request['expected_run_id'], request['expected_conversation_id'],
                      hashlib.sha256(raw.encode()).hexdigest()))
    original = state / 'input_receipts' / (request['request_id'] + '.json')
    before = original.read_bytes(), original.stat().st_mtime_ns
    duplicate = subprocess.run(command, input=raw, text=True, capture_output=True, env=case.env, timeout=15)
    case.assertEqual(duplicate.returncode, 1, duplicate.stdout + duplicate.stderr)
    case.assertEqual(json.loads(duplicate.stdout), proof)
    query = dict(schema=2, request_id=str(uuid.uuid4()), message_request_id=request['request_id'],
        expected_machine_id=machine, expected_session_id=raw_record['session_id'],
        expected_run_id=request['expected_run_id'], expected_conversation_id=request['expected_conversation_id'],
        provider=raw_record['agent'], native_digest=proof['native_digest'])
    recovered = subprocess.run([str(test_input.HGS), 'delivery', 'receipt', name, '--json'],
        input=json.dumps(query), text=True, capture_output=True, env=case.env, timeout=10)
    case.assertEqual(recovered.returncode, 0, recovered.stdout + recovered.stderr)
    case.assertEqual(json.loads(recovered.stdout)['status'], 'not_offered')
    case.assertEqual((original.read_bytes(), original.stat().st_mtime_ns), before)
    current = json.loads(path.read_bytes())
    for key in ('input_pending_at', 'input_pending_request', 'input_typing'):
        case.assertEqual(current.get(key), raw_record.get(key), key)
    case.assertEqual(keys(), before_keys, 'a refused send or its lookup typed a key')
    case.assertEqual(calls(), before_calls, 'a refused send reached the private model')
    case.assertIn(draft, screen())
    # The connector qualification invokes its real Process/claim/journal path
    # only when this private harness supplies the freshly built test executable.
    if probe := os.environ.get('HGS_MOBILE_OUTCOME_PROBE'):
        case.assertEqual(Path(case.env['HOME']).resolve(), case.root.resolve())
        native_payload = dict(request, request_id=str(uuid.uuid4()), attachments=[])
        raw = json.dumps(native_payload, sort_keys=True, ensure_ascii=False).encode()
        binding = dict(schema=2, message_request_id=native_payload['request_id'], computer_id='fixture',
            machine_id=machine, session_id=raw_record['session_id'], session=name, provider=raw_record['agent'],
            run_id=request['expected_run_id'], conversation_id=request['expected_conversation_id'],
            native_encoding='python_default', native_digest=hashlib.sha256(raw).hexdigest())
        binding_hash = hashlib.sha256(json.dumps(binding, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()
        wrapper = dict(schema=2, request_id=native_payload['request_id'], binding=binding, binding_sha256=binding_hash, payload=native_payload)
        message = dict(request_id=native_payload['request_id'], computer_id='fixture', operation='send_outcome', session=name, payload=wrapper)
        job = case.root / 'connector-probe.json'; output = case.root / 'connector-proof.json'
        job.write_text(json.dumps(dict(hgs=str(test_input.HGS),request=message,journal=str(case.root/'connector-journal'),output=str(output))))
        env = dict(case.env, ZERUS_OUTCOME_PROBE_INPUT=str(job))
        process = subprocess.run([probe, '--ignored', '--exact', 'private_native_connector_refusal', '--nocapture'],
            env=env, capture_output=True, text=True, timeout=25)
        case.assertEqual(process.returncode,0,process.stdout+process.stderr)
        receipt = json.loads(output.read_bytes())
        case.assertEqual(receipt['result']['binding'],binding)
        case.assertEqual(receipt['result']['status'],'not_offered')
        case.assertEqual(keys(),before_keys,'the connector or its duplicate typed a key')
        case.assertEqual(calls(),before_calls,'the connector refusal reached the private model')
        case.assertIn(draft,screen())
        print('ZERUS_MOBILE_NOT_OFFERED='+json.dumps(dict(agent=raw_record['agent'],native_process=True,
            journal_once=True,no_keys=True,no_model_call=True)),flush=True)
    print('ZERUS_NATIVE_NOT_OFFERED=' + json.dumps(dict(case=case.id(), agent=raw_record['agent'],
        status=proof['status'], raw_sha256=proof['native_digest'], no_keys=True, no_model_call=True,
        duplicate_unchanged=True, read_only_lookup=True)), flush=True)
