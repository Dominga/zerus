#!/usr/bin/env python3
"""Explicit real-controller prerequisite, isolated tmux/PTY and original bytes.

This is not an official-agent compatibility qualification. The selected tmux
wrapper returns the real private server's output, then exits while one private
descendant retains its original streams until the driver closes its endpoint.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import time
import urllib.parse
import uuid

import test_effort
import test_input


SHIM = r'''
import fcntl, json, os, pathlib, socket, subprocess, sys
root = pathlib.Path(os.environ['INPUT_FIXTURE'])
state = pathlib.Path(os.environ['HGS_STATE_DIR'])
result = subprocess.run([REAL_TMUX, '-S', str(root/'socket'), '-f', '/dev/null', *sys.argv[1:]], capture_output=True)
held = False
try:
    with (state/'.lock').open('r+b') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(lock, fcntl.LOCK_UN)
        except BlockingIOError:
            held = True
except FileNotFoundError:
    pass
if held and sys.argv[1:2] == [os.environ.get('CUSTODY_HOLD_CALL','list-panes')]:
    try:
        fd = os.open(root/'custody-read-armed', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        os.close(fd)
    except FileExistsError:
        held = False
else:
    held = False
if held:
    # This is a fixed fixture-only descendant. It inherits THESE original
    # stdout/stderr descriptors; the real tmux subprocess above is settled.
    code = "import socket,sys; p=socket.socket(socket.AF_UNIX); p.connect(sys.argv[1]); p.sendall(b'ready'); assert p.recv(1)==b''"
    subprocess.Popen([sys.executable, '-c', code, str(root/'custody-release.sock')], stdin=subprocess.DEVNULL)
    own = pathlib.Path('/proc/self/stat').read_text().rsplit(')',1)[1].split()
    (root/'custody-original-helper.json').write_text(json.dumps(dict(pid=os.getpid(),birth=own[19])))
sys.stdout.buffer.write(result.stdout)
sys.stderr.buffer.write(result.stderr)
sys.stdout.buffer.flush(); sys.stderr.buffer.flush()
sys.exit(result.returncode)
'''


def setup_fixture():
    fixture = test_effort.EffortTransport(methodName='runTest')
    try:
        fixture.setUp()
        return fixture
    except BaseException:
        # setUp registers the exact private tmux socket before launching its
        # pane. Cleanup also runs if later seeding/setup assertions fail.
        try:
            fixture.doCleanups()
        finally:
            if hasattr(fixture, 'temp'):
                fixture.temp.cleanup()
        raise


def contended(file):
    try:
        fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        return True
    fcntl.flock(file, fcntl.LOCK_UN)
    return False


def wait_until(condition, timeout, message):
    until = time.monotonic() + timeout
    while not condition():
        if time.monotonic() >= until:
            raise AssertionError(message)
        time.sleep(.005)


def qualify(args, kind, hold='list-panes'):
    fixture = setup_fixture()
    peer = frontend = listener = None
    try:
        fixture.state.chmod(0o700)
        fixture.env['CUSTODY_HOLD_CALL'] = hold
        for key in list(fixture.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key=='HGS_RUN_ID':
                fixture.env.pop(key)
        source = fixture.root/'custody-record.json'
        source.write_text(json.dumps(fixture.record))
        seed_env = dict(fixture.env, ZERUS_PICKER_EFFORT_SEED='1',
                        ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
        seeded = subprocess.run([args.test_image, '--exact',
            'state::effort::picker_scope::tests::effort::seed_actual_effort_record',
            '--ignored', '--test-threads=1'], env=seed_env, capture_output=True, timeout=10)
        assert seeded.returncode == 0, seeded.stdout.decode()+seeded.stderr.decode()
        fixture.record = json.loads(source.read_bytes())
        sid = fixture.record['session_id']
        fixture.record_path = fixture.state/'sessions'/f'{sid}.json'
        assert json.loads(fixture.record_path.read_bytes()) == fixture.record
        listener = socket.socket(socket.AF_UNIX)
        listener.bind(str(fixture.root/'custody-release.sock'))
        listener.listen(1); listener.settimeout(6)
        shim = fixture.root/'custody-tmux.py'
        shim.write_text('REAL_TMUX='+repr(test_input.TMUX)+'\n'+SHIM)
        fixture.script('tmux', '#!/bin/sh\nexec '+shlex.quote(sys.executable)+' '+shlex.quote(str(shim))+' "$@"\n')
        request = fixture.payload('high' if hold == 'send-keys' else 'xhigh')
        raw = ('{\n  '+json.dumps(request, separators=(',', ':'))[1:-1]+'\n}\n').encode()
        digest = hashlib.sha256(raw).hexdigest()
        command = [str(test_input.HGS), 'effort', fixture.name, '--session-id', sid, '--json']
        started = time.monotonic()
        frontend = subprocess.Popen(command, env=fixture.env, stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        frontend.stdin.write(raw); frontend.stdin.close()
        peer, _ = listener.accept()
        assert peer.recv(5) == b'ready'
        # Hash the actual loaded kernel image, even if a simultaneous owner
        # build has atomically replaced the executable's pathname.
        with (Path('/proc')/str(frontend.pid)/'exe').open('rb') as loaded:
            loaded_identity=os.fstat(loaded.fileno())
            loaded_sha256=hashlib.file_digest(loaded,'sha256').hexdigest()
        with (fixture.state/'.lock').open('r+b') as root_lock, \
                (fixture.state/'native-input'/f'{sid}.lock').open('r+b') as sid_lock:
            before = (os.fstat(root_lock.fileno()), os.fstat(sid_lock.fileno()))
            initial = [contended(root_lock), contended(sid_lock)]
            wait_until(lambda: (fixture.root/'custody-original-helper.json').exists(), 2,
                       'original intercepted helper identity was not recorded')
            helper = json.loads((fixture.root/'custody-original-helper.json').read_bytes())
            status_path = Path('/proc')/str(helper['pid'])/'stat'
            exit_observation = {}
            def original_exit():
                try:
                    status = status_path.read_text().rsplit(')',1)[1].split()
                    assert status[19]==helper['birth'], 'original helper birth identity changed'
                    exited0 = status[0]=='Z' and int(status[49])==0
                    if exited0: exit_observation['kernel']='zombie_exit0'
                    return exited0
                except FileNotFoundError:
                    exit_observation['kernel']='reaped_status_requires_owned_report_or_trace'
                    return args.expect=='green'  # final owned-report/trace qualification supplies actual wait
            wait_until(original_exit, 2, 'original intercepted read helper did not actually exit0')
            # The same original finite operation budget, including all startup,
            # is the requirement. No release is sent before this observation.
            while frontend.poll() is None and time.monotonic()-started < args.budget:
                time.sleep(.01)
            settled_before_release = frontend.poll() is not None
            after = [contended(root_lock), contended(sid_lock)]
            peer.setblocking(False)
            try:
                value = peer.recv(1, socket.MSG_PEEK)
                assert False, f'original descendant closed unexpectedly: {value!r}'
            except BlockingIOError:
                pass
            assert all((a.st_dev, a.st_ino) == (b.st_dev, b.st_ino) for a, b in
                       zip(before, (os.fstat(root_lock.fileno()), os.fstat(sid_lock.fileno()))))
            evidence = dict(kind=kind, held_original_call=hold, raw_sha256=digest, request_id=request['request_id'],
                actual_frontend_image_sha256=loaded_sha256,
                actual_frontend_image_object=[loaded_identity.st_dev,loaded_identity.st_ino],
                original_root_sid_objects_unchanged=True,
                original_root_object=[before[0].st_dev,before[0].st_ino],
                original_sid_object=[before[1].st_dev,before[1].st_ino],
                original_intercepted_helper=helper,
                original_read_helper_exit_observation=exit_observation,
                before_frontend_budget_contended=initial,
                frontend_settled_before_descendant_release=settled_before_release,
                after_frontend_budget_contended=after,
                descendant_still_owns_original_streams=True)
            peer.close(); peer = None
            # Original endpoint closes on every path. Only then may the real
            # existing controller finish and this private fixture be removed.
            wait_until(lambda: frontend.poll() is not None, 10, 'controller did not finish after original EOF')
            stdout = frontend.stdout.read(); stderr = frontend.stderr.read()
            code = frontend.wait()
            wait_until(lambda: not contended(root_lock) and not contended(sid_lock), 6,
                       'physical settlement did not release original exclusions')
        receipt_path = fixture.state/'effort_receipts'/f"{request['request_id']}.json"
        receipt = json.loads(receipt_path.read_bytes()) if receipt_path.exists() else None
        evidence.update(frontend_code=code, stderr=stderr.decode(),
                        status=receipt.get('status') if receipt else None,
                        receipt_preserved_original_digest=bool(receipt and receipt['digest']==digest),
                        native_keys=fixture.received().hex())
        if args.expect == 'red':
            assert not settled_before_release, evidence
            assert initial == [True, False], evidence
            assert after == [True, False], evidence
            assert code == 0 and receipt['status']=='confirmed', evidence
            assert json.loads(stdout)['request_id']==request['request_id'], evidence
            assert receipt['digest']==digest and fixture.received()==b'', evidence
            duplicate=subprocess.run(command,env=fixture.env,input=raw,capture_output=True,timeout=5)
            assert duplicate.returncode==0 and json.loads(duplicate.stdout)['request_id']==request['request_id'], evidence
            changed=subprocess.run(command,env=fixture.env,input=raw+b' ',capture_output=True,timeout=5)
            assert changed.returncode!=0 and fixture.received()==b'', evidence
            evidence['same_uuid_completed_lookup_preserved']=True
            evidence['changed_raw_same_uuid_refused']=True
        else:
            assert settled_before_release and after==[True,True], evidence
            assert code != 0 and (receipt is None or receipt['status']!='confirmed'), evidence
            expected_keys = b'\x1b,' if hold == 'send-keys' else b''
            assert fixture.received()==expected_keys, evidence
            if hold == 'send-keys':
                assert receipt['status'] == 'in_progress' and receipt['digest'] == digest, evidence
                unresolved = json.loads(fixture.record_path.read_bytes())
                assert 'settings_unconfirmed_at' in unresolved and unresolved.get('effort') != 'high', evidence
                evidence['possible_send_stays_unknown_without_late_record_or_receipt_commit'] = True
        return evidence
    finally:
        if peer is not None: peer.close()
        if listener is not None: listener.close()
        if frontend is not None and frontend.poll() is None:
            # Closing the private peer above supplies EOF; never kill a native
            # agent or infer scope settlement from a process-group signal.
            frontend.wait(timeout=10)
        try:
            fixture.tearDown()
        finally:
            fixture.doCleanups()


def functional(args, kind):
    """A genuine key-changing existing ID Effort request and cached ordering."""
    fixture = setup_fixture()
    try:
        fixture.state.chmod(0o700)
        for key in list(fixture.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key == 'HGS_RUN_ID':
                fixture.env.pop(key)
        source = fixture.root/'custody-record.json'
        source.write_text(json.dumps(fixture.record))
        seed_env = dict(fixture.env, ZERUS_PICKER_EFFORT_SEED='1',
                        ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
        seeded = subprocess.run([args.test_image, '--exact',
            'state::effort::picker_scope::tests::effort::seed_actual_effort_record',
            '--ignored', '--test-threads=1'], env=seed_env, capture_output=True, timeout=10)
        assert seeded.returncode == 0, seeded.stdout.decode()+seeded.stderr.decode()
        fixture.record = json.loads(source.read_bytes())
        sid = fixture.record['session_id']
        path = fixture.state/'sessions'/f'{sid}.json'
        request = fixture.payload('high')
        raw = ('{\n  '+json.dumps(request, separators=(',', ':'))[1:-1]+'\n}\n').encode()
        command = [str(test_input.HGS), 'effort', fixture.name, '--session-id', sid, '--json']
        defaults = (fixture.root/'.codex/models_cache.json').read_bytes()
        result = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=18)
        assert result.returncode == 0, (result.stdout.decode(), result.stderr.decode())
        receipt = json.loads(result.stdout)
        assert receipt['status'] == 'confirmed' and receipt['request_id'] == request['request_id']
        assert receipt['digest'] == hashlib.sha256(raw).hexdigest()
        assert json.loads(path.read_bytes())['effort'] == 'high'
        assert fixture.received() == b'\x1b,', fixture.received()
        assert (fixture.root/'.codex/models_cache.json').read_bytes() == defaults
        assert 'settings_unconfirmed_at' not in json.loads(path.read_bytes())
        fixture.state.chmod(0o755)
        cached_public = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert cached_public.returncode == 0 and cached_public.stdout == result.stdout, cached_public.stderr
        fixture.state.chmod(0o700)
        # Cache permissions precede current run/conversation/native/readiness
        # admission. The saved ID still resolves, while live binding is stale.
        changed = json.loads(path.read_bytes())
        changed.update(run_id='changed-run', conversation_id='changed-conversation',
                       pane='%999999', pid=999999, activity='busy', phase='working')
        path.write_text(json.dumps(changed))
        duplicate = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert duplicate.returncode == 0 and duplicate.stdout == result.stdout, duplicate.stderr
        different = subprocess.run(command, env=fixture.env, input=raw+b' ', capture_output=True, timeout=5)
        assert different.returncode != 0 and fixture.received() == b'\x1b,'
        receipt_path = fixture.state/'effort_receipts'/f"{request['request_id']}.json"
        uncertain = dict(receipt, status='in_progress')
        receipt_path.write_text(json.dumps(uncertain))
        unknown = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert unknown.returncode != 0 and fixture.received() == b'\x1b,'
        receipt_path.write_bytes(result.stdout.rstrip(b'\n'))
        path.unlink()
        missing_id = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert missing_id.returncode != 0
        label = subprocess.run([str(test_input.HGS), 'effort', fixture.name, '--json'],
            env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert label.returncode == 0 and label.stdout == result.stdout, label.stderr
        assert fixture.received() == b'\x1b,'
        return dict(kind=kind, request_id=request['request_id'], raw_sha256=receipt['digest'],
            actual_key_changing_confirmed=True, native_keys='1b2c', defaults_unchanged=True,
            own_marker_cleared=True, cached_changed_binding_preserved=True,
            cache_does_not_require_native_scope_directory_floor=True,
            changed_raw_and_unknown_refused=True, explicit_missing_id_refused=True,
            label_cached_after_record_gone=True)
    finally:
        try:
            fixture.tearDown()
        finally:
            fixture.doCleanups()


def uncertain(args, kind):
    """A delivered key without native reaction retains its UUID and own mark."""
    fixture = setup_fixture()
    try:
        fixture.state.chmod(0o700)
        for key in list(fixture.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key == 'HGS_RUN_ID':
                fixture.env.pop(key)
        source = fixture.root/'custody-record.json'
        source.write_text(json.dumps(fixture.record))
        seed_env = dict(fixture.env, ZERUS_PICKER_EFFORT_SEED='1',
                        ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
        seeded = subprocess.run([args.test_image, '--exact',
            'state::effort::picker_scope::tests::effort::seed_actual_effort_record',
            '--ignored', '--test-threads=1'], env=seed_env, capture_output=True, timeout=10)
        assert seeded.returncode == 0, seeded.stdout.decode()+seeded.stderr.decode()
        record = json.loads(source.read_bytes())
        path = fixture.state/'sessions'/f"{record['session_id']}.json"
        request = fixture.payload('high')
        raw = json.dumps(request).encode()
        command = [str(test_input.HGS), 'effort', fixture.name, '--session-id', record['session_id'], '--json']
        (fixture.root/'freeze').touch()
        result = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=18)
        assert result.returncode != 0
        receipt_path = fixture.state/'effort_receipts'/f"{request['request_id']}.json"
        receipt = json.loads(receipt_path.read_bytes())
        marked = json.loads(path.read_bytes())
        assert receipt['status'] == 'in_progress' and receipt['digest'] == hashlib.sha256(raw).hexdigest()
        assert isinstance(marked['settings_unconfirmed_at'], (float, int))
        assert fixture.received() == b'\x1b,', fixture.received()
        duplicate = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert duplicate.returncode != 0 and fixture.received() == b'\x1b,'
        other = fixture.payload('high')
        refused = subprocess.run(command, env=fixture.env, input=json.dumps(other).encode(), capture_output=True, timeout=5)
        assert refused.returncode != 0 and fixture.received() == b'\x1b,'
        assert not (fixture.state/'effort_receipts'/f"{other['request_id']}.json").exists()
        # Keep the established timestamp semantics: a later native event can
        # clear that gate for a NEW UUID; it cannot replay the old in_progress.
        marked['last_event_at'] = marked['settings_unconfirmed_at'] + 1
        path.write_text(json.dumps(marked))
        (fixture.root/'freeze').unlink()
        later = subprocess.run(command, env=fixture.env, input=json.dumps(other).encode(), capture_output=True, timeout=18)
        assert later.returncode == 0, later.stderr
        assert json.loads(later.stdout)['status'] == 'confirmed'
        assert fixture.received() == b'\x1b,\x1b,'
        duplicate = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert duplicate.returncode != 0 and fixture.received() == b'\x1b,\x1b,'
        assert json.loads(receipt_path.read_bytes()) == receipt
        return dict(kind=kind, possible_delivery_left_original_unknown=True,
            own_marker_retained_on_failure=True, no_cleanup_escape=True,
            new_uuid_blocked_without_event=True, later_event_keeps_existing_timestamp_contract=True,
            old_uuid_never_replayed=True)
    finally:
        try:
            fixture.tearDown()
        finally:
            fixture.doCleanups()


def routing(args, kind):
    fixture = setup_fixture()
    process = None
    try:
        fixture.state.chmod(0o700)
        for key in list(fixture.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key == 'HGS_RUN_ID':
                fixture.env.pop(key)
        source = fixture.root/'custody-record.json'
        source.write_text(json.dumps(fixture.record))
        seed_env = dict(fixture.env, ZERUS_PICKER_EFFORT_SEED='1',
                        ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
        seeded = subprocess.run([args.test_image, '--exact',
            'state::effort::picker_scope::tests::effort::seed_actual_effort_record',
            '--ignored', '--test-threads=1'], env=seed_env, capture_output=True, timeout=10)
        assert seeded.returncode == 0, seeded.stdout.decode()+seeded.stderr.decode()
        record = json.loads(source.read_bytes())
        path = fixture.state/'sessions'/f"{record['session_id']}.json"
        hello = subprocess.run([str(test_input.HGS), 'swarm', 'hello'], env=fixture.env,
            capture_output=True, timeout=8)
        assert hello.returncode == 0, hello.stderr
        machine = json.loads(hello.stdout)['node_id']
        catalog = fixture.root/'config/swarm/catalog.json'
        original_catalog = catalog.read_bytes()
        command = [str(test_input.HGS), 'effort', fixture.name, '--session-id', record['session_id'], '--json']
        raw = json.dumps(fixture.payload('xhigh')).encode()
        wrong = subprocess.run([str(test_input.HGS), '--expect-machine', str(uuid.uuid4()), *command[1:]],
            env=fixture.env, input=raw, capture_output=True, timeout=5)
        assert wrong.returncode != 0 and fixture.received() == b''
        matching = subprocess.run([str(test_input.HGS), '--expect-machine', machine, *command[1:]],
            env=fixture.env, input=raw, capture_output=True, timeout=18)
        assert matching.returncode == 0 and json.loads(matching.stdout)['status'] == 'confirmed', matching.stderr

        def held_start(argv, payload):
            nonlocal process
            guard = (fixture.state/'.lock').open('r+b')
            fcntl.flock(guard, fcntl.LOCK_EX)
            process = subprocess.Popen(argv, env=fixture.env, stdin=subprocess.PIPE,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            process.stdin.write(payload); process.stdin.close()
            slot = fixture.state/'.native-picker-host.lock'
            def captured():
                assert process.poll() is None, process.stderr.read().decode()
                if not slot.exists(): return False
                with slot.open('r+b') as file: return contended(file)
            try:
                wait_until(captured, 5, 'original public selector was not captured before Root')
            except BaseException:
                guard.close()
                raise
            return guard

        changing = fixture.payload('high')
        guard = held_start([str(test_input.HGS), '--expect-machine', machine, *command[1:]],
                           json.dumps(changing).encode())
        try:
            changed = json.loads(original_catalog)
            changed['node_id'] = str(uuid.uuid4())
            catalog.write_text(json.dumps(changed))
        finally:
            guard.close()
        process.wait(timeout=8)
        assert process.returncode != 0 and fixture.received() == b''
        assert not (fixture.state/'effort_receipts'/f"{changing['request_id']}.json").exists()
        process.stdout.close(); process.stderr.close(); process = None
        catalog.write_bytes(original_catalog)
        def free_slot():
            with (fixture.state/'.native-picker-host.lock').open('r+b') as file:
                return not contended(file)
        wait_until(free_slot, 3, 'failed original machine guard retained capacity')

        # The raw request deliberately expects a newer conversation. The
        # original reference supplied the older one; it must survive self-exec.
        newer = fixture.payload('high')
        newer['expected_conversation_id'] = str(uuid.uuid4())
        reference = 'zerus://session?'+urllib.parse.urlencode(dict(machine=machine,
            host='test', name=fixture.name, session=record['session_id'],
            conversation=record['conversation_id'], run=record['run_id']), quote_via=urllib.parse.quote)
        guard = held_start([str(test_input.HGS), 'effort', reference, '--json'], json.dumps(newer).encode())
        try:
            changed = dict(record, conversation_id=newer['expected_conversation_id'])
            path.write_text(json.dumps(changed))
        finally:
            guard.close()
        process.wait(timeout=8)
        assert process.returncode != 0 and fixture.received() == b''
        assert not (fixture.state/'effort_receipts'/f"{newer['request_id']}.json").exists()
        return dict(kind=kind, actual_cli_matching_machine_accepted=True,
            mismatching_machine_refused=True, changed_original_machine_before_root_refused=True,
            original_reference_conversation_survived_self_exec=True,
            newer_raw_conversation_cannot_replace_original_reference=True,
            no_refused_request_key_or_receipt=True)
    finally:
        if process is not None:
            if process.poll() is None: process.wait(timeout=18)
            process.stdout.close(); process.stderr.close()
        try:
            fixture.tearDown()
        finally:
            fixture.doCleanups()


def ambiguous(args, kind):
    """Soft label classification cannot overwrite a selected late refusal."""
    fixture = setup_fixture()
    try:
        fixture.state.chmod(0o700)
        for key in list(fixture.env):
            if key.startswith(('HGS_SESSION', 'HGS_RESUME_SESSION')) or key == 'HGS_RUN_ID':
                fixture.env.pop(key)
        source = fixture.root/'custody-record.json'
        source.write_text(json.dumps(fixture.record))
        seed_env = dict(fixture.env, ZERUS_PICKER_EFFORT_SEED='1',
                        ZERUS_PICKER_EFFORT_RECORD=str(source), ZERUS_PICKER_EFFORT_KIND=kind)
        seeded = subprocess.run([args.test_image, '--exact',
            'state::effort::picker_scope::tests::effort::seed_actual_effort_record',
            '--ignored', '--test-threads=1'], env=seed_env, capture_output=True, timeout=10)
        assert seeded.returncode == 0, seeded.stdout.decode()+seeded.stderr.decode()
        record = json.loads(source.read_bytes())
        sibling = dict(record, session_id=str(uuid.uuid4()), run_id=str(uuid.uuid4()),
                       conversation_id=str(uuid.uuid4()))
        sibling_path = fixture.state/'sessions'/f"{sibling['session_id']}.json"
        sibling_path.write_text(json.dumps(sibling))
        index = fixture.state/'labels'/f'{hashlib.sha256(fixture.name.encode()).hexdigest()}.json'
        value = json.loads(index.read_bytes())
        value['ids'].append(sibling['session_id'])
        index.write_text(json.dumps(value))
        manifest_path = fixture.state/'labels/.complete'
        manifest = json.loads(manifest_path.read_bytes())
        manifest['labels'][index.stem] = hashlib.sha256(index.read_bytes()).hexdigest()
        manifest_path.write_text(json.dumps(manifest))
        command = [str(test_input.HGS), 'effort', fixture.name, '--json']
        ultra = fixture.payload('ultra')
        refused = subprocess.run(command, env=fixture.env, input=json.dumps(ultra).encode(),
            capture_output=True, timeout=18)
        assert refused.returncode == 1 and b'Ultra requires' in refused.stderr, (refused.stdout, refused.stderr)
        assert refused.stdout == b'' and fixture.received() == b''
        assert not (fixture.state/'effort_receipts'/f"{ultra['request_id']}.json").exists()
        request = fixture.payload('high')
        raw = json.dumps(request).encode()
        actual = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=18)
        assert actual.returncode == 0 and json.loads(actual.stdout)['status'] == 'confirmed', actual.stderr
        assert fixture.received() == b'\x1b,'
        sibling['run_id'] = record['run_id']
        sibling['conversation_id'] = record['conversation_id']
        sibling_path.write_text(json.dumps(sibling))
        another = fixture.payload('high')
        multiple = subprocess.run(command, env=fixture.env, input=json.dumps(another).encode(),
            capture_output=True, timeout=8)
        assert multiple.returncode == 5 and json.loads(multiple.stdout)['total'] == 2, (multiple.stdout, multiple.stderr)
        assert fixture.received() == b'\x1b,'
        # A matching completed UUID remains a cache hit despite current target
        # ambiguity; it cannot be forced through the new native admission.
        duplicate = subprocess.run(command, env=fixture.env, input=raw, capture_output=True, timeout=8)
        assert duplicate.returncode == 0 and duplicate.stdout == actual.stdout, duplicate.stderr
        # A uniquely raw-guarded LEGACY first holder stays in the old lane,
        # whose genuine native refusal must not become stale ambiguity5.
        legacy = dict(record, session_id=str(uuid.uuid4()),run_id=str(uuid.uuid4()),
                      conversation_id=str(uuid.uuid4()))
        legacy.pop('layout')
        first = fixture.state/f'{hashlib.sha256(fixture.name.encode()).hexdigest()}.json'
        first.write_text(json.dumps(legacy))
        legacy_request = fixture.payload('high')
        legacy_request.update(expected_run_id=legacy['run_id'],expected_conversation_id=legacy['conversation_id'])
        old = subprocess.run(command, env=fixture.env, input=json.dumps(legacy_request).encode(),
            capture_output=True, timeout=8)
        assert old.returncode == 1 and b'exact agent process' in old.stderr, (old.stdout, old.stderr)
        assert old.stdout == b'' and fixture.received() == b'\x1b,'
        return dict(kind=kind, raw_unique_id_executes_real_controller=True,
            selected_late_refusal_not_replaced_by_stale_ambiguity=True,
            truly_ambiguous_raw_guards_keep_exit5_report=True,
            cached_receipt_precedes_current_ambiguity=True,
            unique_legacy_native_refusal_preserved=True)
    finally:
        try:
            fixture.tearDown()
        finally:
            fixture.doCleanups()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-image', required=True)
    parser.add_argument('--expect', choices=('red','green'), required=True)
    parser.add_argument('--budget', type=float, default=16)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--functional-only', action='store_true')
    args = parser.parse_args()
    assert sys.platform.startswith('linux'), 'kernel-exit baseline currently qualifies Linux only'
    assert test_input.TMUX, 'private tmux fixture requires tmux'
    rows = [] if args.functional_only else [qualify(args, kind) for kind in ('fresh','moved')]
    if args.expect == 'green' and not args.functional_only:
        rows += [qualify(args, kind, 'send-keys') for kind in ('fresh','moved')]
    result = dict(expected=args.expect, cases=rows,
                  boundary='real production hgs effort controller; scripted native PTY, not official-agent qualification')
    if args.expect == 'green':
        result['functional'] = [functional(args, kind) for kind in ('fresh','moved')]
        result['uncertain'] = [uncertain(args, kind) for kind in ('fresh','moved')]
        result['routing'] = [routing(args, kind) for kind in ('fresh','moved')]
        result['ambiguous'] = [ambiguous(args, kind) for kind in ('fresh','moved')]
    if args.evidence: args.evidence.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
