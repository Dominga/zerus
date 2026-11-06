#!/usr/bin/env python3
"""Private protocol fixture, including the actual original-child cache probe.

This models actor receipts; it is not installed frontend authority evidence.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import sys
import time

root = Path(os.path.abspath(__file__)).parent
config = json.loads((root / 'fixture.json').read_text())
args = sys.argv[1:]
abi = 'desktop-ownership-v1'
journal = '00000000-0000-4000-8000-000000000005'
epoch = '00000000-0000-4000-8000-000000000006'
value = json.loads(sys.stdin.readline()) if args in [
    ['desktop', 'frontend-request', '--json'], ['desktop', 'ownership-result', '--json']] else {}


def status():
    if (root / 'current.json').exists():
        return json.loads((root / 'current.json').read_text())
    return dict(abi=abi, configuration=config.get('configuration', 'a' * 64),
                journal=None if config.get('never_service') else journal,
                revision=0 if config.get('never_service') else 3, state=config.get('state', 'passive'),
                rearm_required=config.get('rearm_required', False), request_id=None,
                owner_epoch=epoch if config.get('existing_grant') else None,
                identity_current=config.get('existing_grant', False))


def receipt(request):
    current = status()
    counted = None
    if config.get('refuse_count') is not None:
        path = root / 'refusal-attempts'
        counted = int(path.read_text()) + 1 if path.exists() else 1
        path.write_text(str(counted))
    if config.get('refuse') and ((counted is not None and counted <= config['refuse_count'])
            or (counted is None and (not config.get('refuse_once') or not (root / 'refused-once').exists()))):
        (root / 'refused-once').touch()
        return dict(abi=abi, request_id=request['request_id'], configuration=current['configuration'],
                    journal=current['journal'], revision=current['revision'], action=request['action'],
                    fingerprint=hashlib.sha256(json.dumps(request, separators=(',', ':')).encode()).hexdigest(),
                    accepted=False, durable=False, outcome=dict(state='refused', code=config['refuse']), current=current)
    revision = request['revision'] + 1
    native = request['action'] == 'native_takeover'
    current.update(revision=revision, state='native' if native else 'service',
                   request_id=request['request_id'], owner_epoch=None if native else epoch,
                   identity_current=True, rearm_required=False)
    outcome = dict(state='native_ready', prior_owner_epoch=epoch, revoked=True,
                   checkpointed=True, drained=True) if native else dict(state='granted', owner_epoch=epoch)
    if config.get('pending') and not (root / 'settle').exists():
        outcome = dict(state='pending')
        current.update(state='draining', identity_current=False)
    if config.get('stale'):
        current.update(revision=revision + 1, state='revoked', rearm_required=True)
    answer = dict(abi=abi, request_id=request['request_id'], configuration=current['configuration'],
                  journal=journal, fingerprint=hashlib.sha256(json.dumps(request, separators=(',', ':')).encode()).hexdigest(),
                  action=request['action'], revision=revision, accepted=True, durable=True,
                  outcome=outcome, current=current)
    if config.get('never_service') and native:
        current.update(journal=None, revision=0, state='native', owner_epoch=None)
        answer.update(journal=None, revision=0, durable=False, outcome=dict(state='native_absent'))
    if config.get('bad_receipt'):
        answer[config['bad_receipt']] = 'b' * 64 if config['bad_receipt'] != 'request_id' else epoch
    (root / 'current.json').write_text(json.dumps(current))
    return answer


if args in [['desktop', 'prepare-state', '--json'], ['desktop', 'ownership-status', '--json'], ['desktop', 'ownership-result', '--json'],
            ['desktop', 'frontend-request', '--json']] or args == ['--help']:
    with (root / 'calls').open('a') as log:
        log.write(json.dumps(dict(args=args, value=value, pid=os.getpid())) + '\n')

if args == ['--help']:
    if config.get('observation_help_pipe') and (root / 'observation-help-armed').exists() and os.fork() == 0:
        (root / 'observation-help-pipe-held').touch()
        until = time.monotonic() + 15
        while not (root / 'observation-pipe-release').exists() and time.monotonic() < until:
            time.sleep(.005)
        os._exit(0)
    print('Desktop service ABI: desktop-v1\nDesktop catalog changes ABI: desktop-catalog-changes-v1\n'
          'Desktop catalog relay ABI: desktop-catalog-relay-v1\nDesktop pending read import ABI: desktop-pending-reads-v1')
    if config.get('frontend_abi', True):
        print('Desktop frontend request ABI: desktop-ui-request-v1')
    if config.get('ownership_abi', True):
        print('Desktop ownership ABI: ' + abi)
    if config.get('prepare_state'):
        print('Desktop state preparation: desktop-state-prepare-v1')
    if config.get('retained_import_abi'):
        print('Desktop import ABI: desktop-import-v1')
    if config.get('observation_abi'):
        print('Desktop ownership observation ABI: desktop-ownership-observation-v1')
elif config.get('retained_import_abi') and args[:2] in [
        ['desktop', 'import-evidence'], ['desktop', 'import-status']]:
    exec(compile((root / 'import-fixture.py').read_text(), 'import-fixture.py', 'exec'))
elif args == ['desktop', 'prepare-state', '--json']:
    if config.get('prepare_refused'):
        sys.exit(1)
    print(json.dumps(dict(prepared=True)))
elif args == ['desktop', 'ownership-status', '--json']:
    if (root / 'mode-ownership-hold').exists():
        (root / 'mode-ownership-entered').touch()
        until = time.monotonic() + 15
        while not (root / 'mode-ownership-release').exists():
            if time.monotonic() > until:
                raise SystemExit('private fixture barrier deadline')
            time.sleep(.005)
    if config.get('observation_help_pipe'):
        (root / 'observation-help-armed').touch()
    print(json.dumps(status()))
elif args == ['desktop', 'frontend-request', '--json']:
    request = value['request']
    if request['op'] not in ('desktop_ownership', 'desktop_ownership_observe'):
        import io
        sys.stdin = io.StringIO(json.dumps(value) + '\n')
        exec(compile((root / 'catalog-fixture.py').read_text(), 'catalog-fixture.py', 'exec'))
        raise SystemExit(0)
    assert value['abi'] == 'desktop-ui-request-v1'
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(2)
        stream.connect(str(root / 'native-cache.sock'))
        pid, uid, _ = struct.unpack('3i', stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        assert pid == os.getppid() and uid == os.getuid()
        stream.sendall(('desktop-ui-request-v1 ' + value['attempt'] + '\n').encode())
        evidence = json.loads(stream.makefile('rb').readline(4097))['request']
    assert evidence['child_pid'] == os.getpid()
    assert evidence['attempt'] == value['attempt']
    observed_query = request.get('query') if request['op'] == 'desktop_ownership_observe' else None
    assert evidence['request_id'] == (observed_query['request_id'] if observed_query else request['request_id'])
    assert evidence['ownership_mode'] == ('native' if request.get('action') == 'native_takeover' else 'service')
    assert evidence['fingerprint'] == hashlib.sha256(json.dumps(request, separators=(',', ':')).encode()).hexdigest()
    if observed_query:
        # Actor result policy MODEL only; the Rust actor controls independently
        # compare actual captured Origin objects. No issued receipt is made.
        (root / 'observation-bound').touch()
        if (root / 'observation-hold').exists():
            (root / 'observation-entered').touch()
            until = time.monotonic() + 15
            while not (root / 'observation-release').exists():
                if time.monotonic() >= until:
                    raise SystemExit('private observation deadline')
                time.sleep(.005)
        current = status()
        live_config = json.loads((root / 'fixture.json').read_text())
        live_epoch = (root / 'runtime-epoch').read_text() if (root / 'runtime-epoch').exists() else '00000000-0000-4000-8000-000000000001'
        valid = (not live_config.get('observation_origin_mismatch')
                 and observed_query['service_epoch'] == live_epoch
                 and observed_query['generation'] == live_config.get('status_generation', 1)
                 and current['state'] == 'service' and current['identity_current']
                 and not current['rearm_required']
                 and all(current[k] == observed_query[k] for k in ('configuration', 'journal', 'owner_epoch')))
        if config.get('observation_pipe') and os.fork() == 0:
            (root / 'observation-pipe-held').touch()
            until = time.monotonic() + 15
            while not (root / 'observation-pipe-release').exists() and time.monotonic() < until:
                time.sleep(.005)
            os._exit(0)
        if valid:
            answer = dict(abi='desktop-ownership-observation-v1', query=observed_query, current=current)
            if config.get('observation_bad_epoch'):
                answer['query'] = dict(observed_query, service_epoch=epoch)
        else:
            answer = dict(request_id=observed_query['request_id'], code='ownership_observation_unavailable')
        print(json.dumps(answer))
        raise SystemExit(0)
    (root / 'original.json').write_text(json.dumps(request))
    (root / 'bound').touch()
    if config.get('lost') and (not config.get('lost_once') or not (root / 'lost-once').exists()):
        (root / 'lost-once').touch()
        raise SystemExit('private lost reply')
    if config.get('held_pipe') and os.fork() == 0:
        (root / 'pipe-held').touch()
        until = time.monotonic() + 15
        while not (root / 'release-pipe').exists() and time.monotonic() < until:
            time.sleep(.005)
        os._exit(0)
    if config.get('wire_refusal'):
        print(json.dumps(dict(request_id=request['request_id'], accepted=False,
                              durable=False, code=config['wire_refusal'])))
    else:
        print(json.dumps(receipt(request)))
elif args == ['desktop', 'ownership-result', '--json']:
    original = json.loads((root / 'original.json').read_text())
    assert value['original_request_id'] == original['request_id']
    assert value['fingerprint'] == hashlib.sha256(json.dumps(original, separators=(',', ':')).encode()).hexdigest()
    assert value['journal'] == original['journal'] and value['revision'] == original['revision']
    missing = config.get('missing', False) or config.get('lost_once', False)
    print(json.dumps(dict(abi=abi, request_id=value['request_id'], configuration='a' * 64,
                         found=not missing, definitive=missing and config.get('definitive', False),
                         receipt=None if missing else receipt(original))))
else:
    # The existing bounded catalog/watch fixture uses the same isolated root.
    exec(compile((root / 'catalog-fixture.py').read_text(), 'catalog-fixture.py', 'exec'))
