#!/usr/bin/env python3
"""Actual Settings CLI on a private scripted tmux, never official-agent evidence.

The baseline is allowed to refuse safely before any helper. That is evidence,
not a reason to bypass the old public selector or native guards.
"""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import time
import uuid

import qualify_effort_custody as custody
import test_input
import test_settings

SEED = 'state::effort::picker_scope::tests::effort::seed_actual_effort_record'


def digest(path):
    with Path(path).open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


@contextlib.contextmanager
def fixture():
    case = test_settings.SessionSettings(methodName='runTest')
    try:
        case.setUp()
        for key in list(case.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key == 'HGS_RUN_ID':
                case.env.pop(key)
        case.state.chmod(0o700)
        yield case
    finally:
        try:
            if hasattr(case, 'socket'):
                case.tearDown()
        finally:
            case.doCleanups()
            if hasattr(case, 'temp'):
                case.temp.cleanup()


def seed(case, args, kind):
    source = case.root / 'settings-original-record.json'
    source.write_text(json.dumps(case.record))
    env = dict(case.env, ZERUS_PICKER_EFFORT_SEED='1',
               ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
    result = subprocess.run([args.test_image, '--exact', SEED, '--ignored', '--test-threads=1'],
                            env=env, capture_output=True, timeout=15)
    assert result.returncode == 0, (result.stdout, result.stderr)
    case.record = json.loads(source.read_bytes())
    sid = case.record['session_id']
    case.record_path = case.state / 'sessions' / (sid + '.json')
    assert json.loads(case.record_path.read_bytes()) == case.record
    return sid


def payload(case, **extra):
    return dict(request_id=str(uuid.uuid4()), expected_run_id=case.run_id,
                expected_conversation_id=case.conversation_id,
                model='gpt-6-sol', effort='low', **extra)


def raw(request):
    return ('{\n ' + json.dumps(request, separators=(',', ':'))[1:-1] + '\n}\n').encode()


def command(case, sid=None):
    return [str(test_input.HGS), 'settings', case.name, *(['--session-id', sid] if sid else []), '--json']


def call(case, sid, body):
    return subprocess.run(command(case, sid), input=body, env=case.env, capture_output=True, timeout=20)


def successful(result, status):
    assert result.returncode == 0, (result.stdout, result.stderr)
    receipt = json.loads(result.stdout)
    assert receipt['status'] == status, receipt
    return receipt



def observer(case, args, request_id=None, original_id=None, point=None):
    """One fixture-only observer; every native call still uses its real socket."""
    log = case.root / 'settings-calls.jsonl'
    script = case.root / 'settings-observer.py'
    script.write_text('REAL_TMUX=' + repr(test_input.TMUX) + '\n' + r'''import json,os,pathlib,subprocess,sys
root=pathlib.Path(os.environ['INPUT_FIXTURE']);state=pathlib.Path(os.environ['HGS_STATE_DIR']);argv=sys.argv[1:]
log=root/'settings-calls.jsonl'
assert not log.exists() or log.stat().st_size < (1<<20)
with log.open('a') as file:file.write(json.dumps(argv)+'\n')
def read(path):
 try:return json.loads(path.read_bytes())
 except FileNotFoundError:return {}
w=read(state/'settings_receipts'/(os.environ.get('SETTINGS_FAULT_W','none')+'.json'))
u=read(state/'settings_receipts'/(os.environ.get('SETTINGS_FAULT_U','none')+'.json'))
record=read(pathlib.Path(os.environ['SETTINGS_RECORD']))
point=os.environ.get('SETTINGS_FAULT_POINT','')
progress=w.get('status')=='in_progress';inner=u.get('status');marked=bool(record.get('settings_unconfirmed_at'))
removed=not record.get('pending_settings')
hit=(point=='after-W' and progress and inner=='scheduled') or (point=='after-U' and progress and inner=='in_progress' and not marked) or (point=='after-marker' and progress and marked) or (point=='after-record' and progress and inner=='in_progress' and removed and not marked) or (point=='after-U-applied' and progress and inner=='applied') or (point=='schedule-record' and progress and record.get('pending_settings',{}).get('request_id')==os.environ.get('SETTINGS_FAULT_W'))
if hit and not (root/'fault-disabled').exists():
 (root/'fault-hit').write_text(point);sys.stderr.write('isolated Settings boundary refusal\n');sys.exit(65)
result=subprocess.run([REAL_TMUX,'-S',str(root/'socket'),'-f','/dev/null',*argv],capture_output=True)
if point in ('replace-U','replace-pending') and progress and inner=='in_progress' and marked and not (root/'fault-hit').exists():
 if point=='replace-U':
  target=state/'settings_receipts'/(os.environ['SETTINGS_FAULT_U']+'.json');replacement=target.with_suffix('.fixture-replaced')
  fd=os.open(replacement,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
  with os.fdopen(fd,'wb') as file:file.write(target.read_bytes())
  os.replace(replacement,target)
 else:
  record['pending_settings']['request_id']='changed-pending-request';pathlib.Path(os.environ['SETTINGS_RECORD']).write_text(json.dumps(record))
 (root/'fault-hit').write_text(point)
if point=='cid-assigned' and progress and argv[:1]==['send-keys'] and not (root/'fault-hit').exists():
 record['conversation_id']='new-native-conversation';pathlib.Path(os.environ['SETTINGS_RECORD']).write_text(json.dumps(record));(root/'fault-hit').write_text(point)
if point=='after-key' and progress and argv[:1]==['send-keys'] and not (root/'fault-disabled').exists():
 (root/'fault-hit').write_text(point);sys.stderr.write('isolated possible-key refusal\n');sys.exit(65)
sys.stdout.buffer.write(result.stdout);sys.stderr.buffer.write(result.stderr);sys.exit(result.returncode)
''')
    case.env['SETTINGS_RECORD'] = str(case.record_path)
    if request_id:
        case.env.update(SETTINGS_FAULT_W=request_id, SETTINGS_FAULT_U=original_id or 'none',
                        SETTINGS_FAULT_POINT=point or '')
    case.script('tmux', '#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' ' + shlex.quote(str(script)) + ' "$@"\n')

    def read_log():
        if not log.exists():
            return b''
        assert log.stat().st_size <= 1 << 20
        return log.read_bytes()
    return read_log


def fault(args, kind, point):
    with fixture() as case:
        # A literal legacy-written scheduled U is retained unchanged through the
        # actual move; no copied/reconstructed receipt supplies replay consent.
        case.record.update(activity='busy', phase='working'); case.write_record()
        sid = seed(case, args, kind) if kind == 'fresh' else None
        original = payload(case)
        original_raw = raw(original)
        successful(call(case, sid, original_raw), 'scheduled')
        case.record = json.loads(case.record_path.read_bytes())
        case.record.update(activity='idle', phase='idle'); case.write_record()
        if sid is None:
            sid = seed(case, args, kind)
        request = payload(case, **({} if point == 'schedule-record' else
                                  {'expected_pending_id': original['request_id']}))
        if point == 'schedule-record':
            case.record.update(activity='busy', phase='working'); case.write_record()
        body = raw(request)
        calls = observer(case, args, request['request_id'], original['request_id'], point)
        result = call(case, sid, body)
        assert result.returncode == 1 and (case.root / 'fault-hit').read_text() == point, (result.stdout, result.stderr)
        sent = case.received()
        w = json.loads((case.state / 'settings_receipts' / (request['request_id'] + '.json')).read_bytes())
        u = json.loads((case.state / 'settings_receipts' / (original['request_id'] + '.json')).read_bytes())
        assert w['status'] == 'in_progress' and w['digest'] == hashlib.sha256(body).hexdigest()
        if point in ('after-W', 'after-U', 'after-marker', 'schedule-record', 'replace-U', 'replace-pending'):
            assert sent == b'', sent
        if point == 'after-key':
            assert sent == b'/model', sent
        assert u['status'] == ('scheduled' if point in ('after-W', 'schedule-record') else
                               'applied' if point == 'after-U-applied' else 'in_progress')
        (case.root / 'fault-disabled').touch()
        before = calls()
        repeated = call(case, sid, body)
        assert repeated.returncode == 1 and b'may already have reached' in repeated.stderr, repeated.stderr
        assert calls() == before and case.received() == sent, 'Unknown request was replayed'
        recovered = False
        if point in ('schedule-record', 'after-U'):
            case.record = json.loads(case.record_path.read_bytes())
            case.record.update(activity='idle', phase='idle'); case.write_record()
            next_request = payload(case)
            successful(call(case, sid, raw(next_request)), 'applied')
            assert 'pending_settings' not in json.loads(case.record_path.read_bytes())
            message = dict(request_id=str(uuid.uuid4()), expected_run_id=case.run_id,
                           expected_conversation_id=case.conversation_id, text='Explicit Settings recovery completed.')
            delivered = subprocess.run([str(test_input.HGS), 'send', case.name, '--session-id', sid, '--json'],
                env=case.env, input=json.dumps(message).encode(), capture_output=True, timeout=15)
            assert delivered.returncode == 0, (delivered.stdout, delivered.stderr)
            assert json.loads(delivered.stdout)['status'] == 'submitted'
            recovered = True
        return dict(kind=kind, boundary=point, request_id=request['request_id'], original_pending=original['request_id'],
                    raw_sha256=w['digest'], status_after_failure=w['status'], native_keys_hex=sent.hex(),
                    unknown_not_replayed=True, explicit_supersede_recovered=recovered)

def functional(args, kind, mode):
    with fixture() as case:
        before_defaults = case.cache.read_bytes()
        legacy = None
        if mode == 'legacy-pending':
            case.record.update(activity='busy', phase='working')
            case.write_record()
            original = payload(case)
            body = raw(original)
            saved = successful(call(case, None, body), 'scheduled')
            legacy = (original, body, saved)
            case.record = json.loads(case.record_path.read_bytes())
            case.record.update(activity='idle', phase='idle')
            case.write_record()
        sid = seed(case, args, kind)
        calls = observer(case, args)
        if mode == 'empty-cid':
            case.fresh()
        if mode == 'scheduled':
            case.record.update(activity='busy', phase='working')
            case.write_record()
        request = payload(case, **({'expected_pending_id': legacy[0]['request_id']} if legacy else {}))
        body = raw(request)
        result = call(case, sid, body)
        saved = successful(result, 'scheduled' if mode == 'scheduled' else 'applied')
        assert saved['request_id'] == request['request_id']
        assert saved['digest'] == hashlib.sha256(body).hexdigest()
        receipt_path = case.state / 'settings_receipts' / (request['request_id'] + '.json')
        assert json.loads(receipt_path.read_bytes()) == saved
        assert not (case.state / 'effort_receipts' / (request['request_id'] + '.json')).exists()
        sent = case.received()
        if mode == 'scheduled':
            assert sent == b''
            assert saved['apply_when'] == 'ready'
            cached = call(case, sid, body)
            assert cached.returncode == 0 and cached.stdout == result.stdout
            assert case.received() == b''
            case.record = json.loads(case.record_path.read_bytes())
            case.record.update(activity='idle', phase='idle')
            case.write_record()
            apply = payload(case, expected_pending_id=request['request_id'])
            applied = successful(call(case, sid, raw(apply)), 'applied')
            assert applied['request_id'] != request['request_id']
            assert json.loads(receipt_path.read_bytes())['status'] == 'applied'
            sent = case.received()
        else:
            assert sent.startswith(b'/model') and sent.endswith(b's'), sent
            assert 'pending_settings' not in json.loads(case.record_path.read_bytes())
        before_calls = calls()
        duplicate = call(case, sid, body)
        assert calls() == before_calls, 'cached Settings response invoked a helper'
        assert duplicate.returncode == 0, duplicate.stderr
        assert case.received() == sent
        changed = call(case, sid, body + b' ')
        assert changed.returncode == 1 and b'already used' in changed.stderr, changed.stderr
        assert case.received() == sent
        assert calls() == before_calls, 'conflicting raw Settings request invoked a helper'
        if legacy:
            assert json.loads((case.state / 'settings_receipts' / (legacy[0]['request_id'] + '.json')).read_bytes())['status'] == 'applied'
        assert case.cache.read_bytes() == before_defaults
        assert not (case.root / '.codex/config.toml').exists()
        current = json.loads(case.record_path.read_bytes())
        assert current['model'] == 'gpt-6-sol' and current['effort'] == 'low'
        assert 'settings_unconfirmed_at' not in current
        if mode == 'empty-cid':
            assert not current.get('conversation_id')
            assert current['run_id'] == case.run_id
        # Matching cached bytes/known conflicts precede live native admission.
        current.update(run_id='changed-run', conversation_id='changed-conversation', pid=999999, pane='%999999')
        case.record_path.write_text(json.dumps(current))
        before_calls = calls()
        assert call(case, sid, body).returncode == 0
        conflict = call(case, sid, body + b' ')
        assert conflict.returncode == 1 and b'already used' in conflict.stderr, conflict.stderr
        value = json.loads(receipt_path.read_bytes()); value['status'] = 'in_progress'
        receipt_path.write_text(json.dumps(value))
        unknown = call(case, sid, body)
        assert unknown.returncode == 1 and b'may already have reached' in unknown.stderr, unknown.stderr
        assert case.received() == sent and calls() == before_calls
        return dict(kind=kind, mode=mode, request_id=request['request_id'], raw_sha256=saved['digest'],
                    applied=True, changed_raw_refused=True, unknown_refused=True, keys_once=True)



def empty_cid_race(args, kind):
    with fixture() as case:
        sid = seed(case, args, kind)
        case.fresh()
        request = payload(case)
        body = raw(request)
        calls = observer(case, args, request['request_id'], point='cid-assigned')
        result = call(case, sid, body)
        assert result.returncode == 1 and (case.root / 'fault-hit').read_text() == 'cid-assigned', (result.stdout, result.stderr)
        assert case.received() == b'/model', case.received()
        current = json.loads(case.record_path.read_bytes())
        assert current['conversation_id'] == 'new-native-conversation'
        assert current['model'] != 'gpt-6-sol'
        saved = json.loads((case.state / 'settings_receipts' / (request['request_id'] + '.json')).read_bytes())
        assert saved['status'] == 'in_progress' and saved['conversation_id'] == ''
        before = calls()
        repeated = call(case, sid, body)
        assert repeated.returncode == 1 and b'may already have reached' in repeated.stderr
        assert calls() == before and case.received() == b'/model'
        assert json.loads(case.record_path.read_bytes()) == current, 'late write overwrote a changed CID'
        return dict(kind=kind, empty_cid_was_exact=True, assigned_cid_refused=True, no_late_key_or_write=True)

def physical(args, kind, held_call):
    with fixture() as case:
        sid = seed(case, args, kind)
        listener = socket.socket(socket.AF_UNIX)
        peer = None
        frontend = None
        try:
            listener.bind(str(case.root / 'custody-release.sock'))
            listener.listen(1); listener.settimeout(.1)
            case.env['CUSTODY_HOLD_CALL'] = held_call
            shim = case.root / 'custody-tmux.py'
            shim.write_text('REAL_TMUX=' + repr(test_input.TMUX) + '\n' + custody.SHIM)
            case.script('tmux', '#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' ' + shlex.quote(str(shim)) + ' "$@"\n')
            request = payload(case)
            body = raw(request)
            before = time.monotonic()
            frontend = subprocess.Popen(command(case, sid), env=case.env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            frontend.stdin.write(body); frontend.stdin.close()
            end = before + 6
            while peer is None and frontend.poll() is None and time.monotonic() < end:
                try:
                    peer, _ = listener.accept()
                except socket.timeout:
                    pass
            if peer is None:
                assert frontend.poll() is not None, 'no held helper or settled original refusal'
                output, error = frontend.stdout.read(), frontend.stderr.read()
                assert args.expect == 'red' and frontend.returncode != 0, (output, error)
                assert case.received() == b'', 'baseline refusal came after input'
                return dict(kind=kind, held_call=held_call, disposition='old-public-safe-refusal',
                            code=frontend.returncode, stderr=error.decode(errors='replace')[:1000])
            assert peer.recv(5) == b'ready'
            with (case.state / '.lock').open('r+b') as root_lock, \
                    (case.state / 'native-input' / (sid + '.lock')).open('r+b') as sid_lock:
                ids = [dict(device=os.fstat(f.fileno()).st_dev, inode=os.fstat(f.fileno()).st_ino)
                       for f in (root_lock, sid_lock)]
                custody.wait_until(lambda: (case.root / 'custody-original-helper.json').exists(), 2, 'missing helper identity')
                helper = json.loads((case.root / 'custody-original-helper.json').read_bytes())
                stat = Path('/proc') / str(helper['pid']) / 'stat'
                def exited():
                    try:
                        fields = stat.read_text().rsplit(')', 1)[1].split()
                    except FileNotFoundError:
                        return True
                    assert fields[19] == helper['birth']
                    return fields[0] == 'Z'
                custody.wait_until(exited, 3, 'original helper did not actually exit')
                while time.monotonic() - before < args.budget:
                    time.sleep(.02)
                returned = frontend.poll() is not None
                held = [custody.contended(root_lock), custody.contended(sid_lock)]
                sent = case.received()
                if args.expect == 'green':
                    assert returned and held == [True, True], (returned, held)
                    assert sent == (b'/model' if held_call == 'send-keys' else b''), sent
                else:
                    assert not returned or held != [True, True], 'old candidate unexpectedly satisfies new physical control'
                peer.close(); peer = None
                frontend.wait(timeout=8)
                custody.wait_until(lambda: not custody.contended(root_lock) and not custody.contended(sid_lock), 5, 'original custody did not settle')
                if args.expect == 'green':
                    assert case.received() == sent, 'late native input after deadline/EOF'
                    path = case.state / 'settings_receipts' / (request['request_id'] + '.json')
                    if path.exists():
                        saved = json.loads(path.read_bytes())
                        assert saved['status'] == 'in_progress' and saved['digest'] == hashlib.sha256(body).hexdigest()
                    if held_call == 'send-keys':
                        assert path.exists() and 'settings_unconfirmed_at' in json.loads(case.record_path.read_bytes())
                return dict(kind=kind, held_call=held_call, request_id=request['request_id'],
                            raw_sha256=hashlib.sha256(body).hexdigest(), original_helper=helper,
                            original_objects=ids, original_helper_exited_before_probe=True,
                            frontend_returned_before_release=returned, exclusion_after_deadline=held,
                            delivered_hex=sent.hex(), settled_only_after_original_eof=True)
        finally:
            if peer is not None:
                peer.close()
            listener.close()
            if frontend is not None and frontend.poll() is None:
                try:
                    frontend.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    frontend.kill(); frontend.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hgs', type=Path, required=True)
    parser.add_argument('--test-image', required=True)
    parser.add_argument('--expect', choices=('red', 'green'), required=True)
    parser.add_argument('--budget', type=float, default=16)
    parser.add_argument('--functional-only', action='store_true')
    parser.add_argument('--physical-only', action='store_true')
    parser.add_argument('--faults-only', action='store_true')
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform.startswith('linux'), 'physical original-exit evidence requires Linux'
    assert test_input.TMUX
    test_input.HGS = args.hgs.resolve(strict=True)
    cli_hash, seed_hash = digest(test_input.HGS), digest(args.test_image)
    rows = [] if (args.functional_only or args.faults_only) else [physical(args, kind, call_name)
        for kind in ('fresh', 'moved') for call_name in ('list-panes', 'send-keys')]
    functions = [] if (args.expect == 'red' or args.physical_only or args.faults_only) else [functional(args, kind, mode)
        for kind in ('fresh', 'moved') for mode in ('direct', 'scheduled', 'empty-cid')]
    if args.expect == 'green' and not (args.physical_only or args.faults_only):
        functions.append(functional(args, 'moved', 'legacy-pending'))
    faults = [] if args.expect == 'red' or args.physical_only or args.functional_only else [
        fault(args, kind, point) for kind in ('fresh', 'moved') for point in ('after-W', 'after-U', 'after-marker', 'after-key',
                                                  'after-record', 'after-U-applied', 'schedule-record', 'replace-U', 'replace-pending')]
    races = [] if args.expect == 'red' or args.physical_only else [empty_cid_race(args, kind) for kind in ('fresh', 'moved')]
    assert digest(test_input.HGS) == cli_hash and digest(args.test_image) == seed_hash
    result = dict(hgs_sha256=cli_hash, test_image_sha256=seed_hash, expected=args.expect,
                  physical=rows, functional=functions, faults=faults, cid_races=races, skipped=0,
                  boundary='actual production Settings CLI, scripted native PTY; not official native compatibility')
    args.evidence.parent.mkdir(parents=True, exist_ok=True)
    args.evidence.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
