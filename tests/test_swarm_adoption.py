#!/usr/bin/env python3
"""Empty-peer adoption through the real CLI and isolated fake SSH."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug/hgs')).resolve()
SSH = '''#!/usr/bin/env python3
import json, os, pathlib, shlex, subprocess, sys
if sys.argv[-1].startswith('echo "hgs-remote-v2|'): print('hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot'); sys.exit(0)
root = pathlib.Path(os.environ['SWARM_TEST_ROOT'])
alias = sys.argv[-2]
env = dict(os.environ, HGS_CONFIG_DIR=str(root / alias), HGS_STATE_DIR=str(root / alias / 'state'))
args = shlex.split(sys.argv[-1])[1:]
with (root / 'calls').open('a') as file: file.write(alias + ' ' + ' '.join(args) + '\\n')
if alias == 'b' and (root / 'old-cli').exists() and args[1:2] in (['adoption-preview'], ['adopt']):
    print('unknown adoption command', file=sys.stderr); sys.exit(1)
if alias == 'b' and args[1:2] == ['adopt']:
    raw = sys.stdin.read()
    request = json.loads(raw)
    if (root / 'capture-request').exists():
        (root / 'captured-request.json').write_text(raw)
        print('{}'); sys.exit(0)
    process = subprocess.Popen([os.environ['SWARM_TEST_HGS'], *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
    process.stdin.write(raw); process.stdin.close()
    if (root / 'fail-primary').exists() or (root / 'fail-removal').exists() or (root / 'fail-after-guard-cleared').exists():
        journal = root / 'b/swarm/adoption-journal' / (request['request_id'] + '.json')
        import signal, time
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            phase = json.loads(journal.read_text())['phase'] if journal.exists() else None
            if (root / 'fail-primary').exists() and phase in ('main_started', 'main_committed', 'primary_started'):
                os.kill(process.pid, signal.SIGSTOP)
                sidecar = root / 'b/swarm/session-id-memberships.json'
                if sidecar.exists(): sidecar.unlink()
                sidecar.mkdir()
                (root / 'fault-injected').touch()
                os.kill(process.pid, signal.SIGCONT)
                break
            if (root / 'fail-removal').exists() and phase == 'components_verified':
                guard = root / 'b/swarm/adoption-pending.json'
                if guard.exists():
                    os.kill(process.pid, signal.SIGSTOP)
                    guard.unlink()
                    guard.mkdir()
                    (root / 'fault-injected').touch()
                    os.kill(process.pid, signal.SIGCONT)
                    break
            if (root / 'fail-after-guard-cleared').exists() and phase == 'guard_cleared':
                os.kill(process.pid, signal.SIGKILL)
                (root / 'fault-injected').touch()
                break
            time.sleep(.0001)
    stdout, stderr = process.stdout.read(), process.stderr.read()
    process.wait(timeout=30)
    if (root / 'refuse-once').exists():
        (root / 'refuse-once').unlink()
        (root / 'b/swarm/drive-folders.json').unlink(missing_ok=True)
    if (root / 'lose-reply').exists():
        sys.stderr.write('fixture reply lost'); sys.exit(1)
    if (root / 'duplicate-reply').exists():
        marker = (root / 'duplicate-reply').read_text()
        if marker == 'identical':
            stdout = '{"schema":1,"schema":1,' + stdout[1:]
        elif marker == 'before':
            stdout = '{"phase":"refused_not_started",' + stdout[1:]
        else:
            stdout = stdout[:-1] + ',"phase":"refused_not_started"}'
    if (root / 'tamper-reply').exists():
        import uuid
        body = json.loads(stdout)
        marker = (root / 'tamper-reply').read_text()
        if marker == 'destination':
            body['original']['destination']['node_id'] = str(uuid.uuid4())
        elif marker == 'source':
            body['original']['source']['outer_store_sha256'] = '0' * 64
        elif marker == 'owner':
            body['original']['owner_sha256'] = '0' * 64
        elif marker == 'component':
            body['primary_receipt']['source']['count'] += 1
        elif marker == 'guard':
            body['guard_cleared'] = False
        stdout = json.dumps(body, separators=(',', ':'))
    if (root / 'cap-reply').exists():
        stdout += ' ' * (65536 - len(stdout.encode()))
    if (root / 'overflow-reply').exists():
        stdout += ' ' * (65537 - len(stdout.encode()))
    if (root / 'hold-pipes').exists():
        subprocess.Popen(['/bin/sh', '-c', 'while [ ! -f "$1" ]; do sleep .02; done', 'sh', str(root / 'release-pipes')],
                         stdout=sys.stdout, stderr=sys.stderr,
                         env={'PATH': '/usr/bin:/bin', 'HOME': str(root)})
        (root / 'descendant-holding').touch()
    sys.stdout.write(stdout); sys.stderr.write(stderr); sys.exit(process.returncode)
os.execve(os.environ['SWARM_TEST_HGS'], [os.environ['SWARM_TEST_HGS'], *args], env)
'''


def project(identifier):
    return dict(id=identifier, name='Shared project', color='#123456', vivid=False,
                folders=[dict(id='folder-shared', machine='a', path='/a', name='')], sessions=[])


def enc(value, sort=False):
    return json.dumps(value, separators=(',', ':'), sort_keys=sort).encode()


def sha(value):
    return hashlib.sha256(value).hexdigest()


def struct_op(operation):
    key = operation['key']
    fields = {'project': ['id', 'field'], 'machine': ['id', 'field'], 'folder': ['id'],
              'session': ['machine', 'session'], 'preference': ['field'], 'order': ['field']}[key['kind']]
    return dict(id=operation['id'], actor=operation['actor'], clock=operation['clock'],
                key=dict(kind=key['kind'], **{field: key[field] for field in fields}),
                parents=operation['parents'], value=operation['value'])


def catbytes(catalog):
    return enc(dict(schema=catalog['schema'], swarm_id=catalog['swarm_id'],
                    operations={key: struct_op(operation) for key, operation in sorted(catalog['operations'].items())}))


def rehash(request):
    source = dict(request['source'], configuration_revision='')
    preview = sha(enc(dict(version=1, direction=request['direction'], source=request['source'],
                           destination=request['destination']), sort=True))
    request['preview_sha256'] = preview
    request['request_sha256'] = sha(enc(dict(
        version=1, request_id=request['request_id'], direction=request['direction'], source=source,
        destination=request['destination'], destination_preview_sha256=preview), sort=True))
    request['binding_sha256'] = sha(enc(dict(
        request_sha256=request['request_sha256'], source_alias=request['source_alias'],
        source_configuration_revision=request['source_configuration_revision']), sort=True))


class SwarmAdoption(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='hgs-swarm-adoption-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        (self.root / 'bin').mkdir()
        ssh = self.root / 'bin/ssh'
        ssh.write_text(SSH)
        ssh.chmod(0o755)
        self.home = self.root / 'home'
        self.home.mkdir()
        self.env = dict(os.environ, SWARM_TEST_ROOT=str(self.root), SWARM_TEST_HGS=str(HGS),
                        HOME=str(self.home), USERPROFILE=str(self.home),
                        XDG_CONFIG_HOME=str(self.root / 'xdg'), HGS_TAB='0',
                        PATH=str(self.root / 'bin') + os.pathsep + os.environ['PATH'])
        for key in ['HGS_SELF', 'HGS_PEERS', 'HGS_EXECUTABLE', 'HGS_SESSION', 'HGS_RUN_ID']:
            self.env.pop(key, None)
        for node, peers in [('a', 'c b'), ('b', 'a'), ('c', 'a')]:
            (self.root / node).mkdir()
            (self.root / node / 'config').write_text(f'HGS_SELF="{node}"\nHGS_PEERS="{peers}"\n')
        self.call('a', 'initialize', data={'projects': [project('shared')]})
        self.call('c', 'initialize', data={'projects': []})
        self.join('a', 'c')
        self.join('c', 'a')
        self.call('a', 'sync')
        self.add_sidecars()

    def call(self, node, *args, data=None, ok=True, timeout=40):
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / node), HGS_STATE_DIR=str(self.root / node / 'state'))
        result = subprocess.run([str(HGS), 'swarm', *args], input=json.dumps(data) if data is not None else None,
                                env=env, text=True, capture_output=True, timeout=timeout)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0, result.stderr)
        return result.stderr

    def raw(self, node, *args, data=None):
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / node), HGS_STATE_DIR=str(self.root / node / 'state'))
        return subprocess.run([str(HGS), 'swarm', *args], input=data,
                              env=env, capture_output=True, timeout=40)

    def tree(self, node):
        directory = self.root / node / 'swarm'
        return {str(path.relative_to(directory)): sha(path.read_bytes())
                for path in directory.rglob('*') if path.is_file()}

    def capture_request(self):
        self.empty_destination(False)
        (self.root / 'capture-request').touch()
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        result = self.raw('a', 'adopt', 'b', '--json', data=enc(self.options(preview)))
        self.assertNotEqual(result.returncode, 0, result.stderr)
        request = json.loads((self.root / 'captured-request.json').read_text())
        previous = tuple(request[key] for key in ('preview_sha256', 'request_sha256', 'binding_sha256'))
        rehash(request)
        self.assertEqual(previous, tuple(request[key] for key in ('preview_sha256', 'request_sha256', 'binding_sha256')))
        return request

    def metrics_request(self, total_edges=500736, target_bytes=None):
        request = self.capture_request()
        catalog = json.loads(request['source_catalog_json'])
        node = request['source']['node_id']
        session = str(uuid.uuid4())
        operations = {}
        roots = [str(uuid.uuid4()) for _ in range(1024)]
        for identifier in roots:
            operations[identifier] = dict(id=identifier, actor=node, clock=1,
                key=dict(kind='session', machine=node, session=session), parents=[], value=None)
        remaining = total_edges
        while remaining:
            parents = roots[:min(1024, remaining)]
            remaining -= len(parents)
            identifier = str(uuid.uuid4())
            operations[identifier] = dict(id=identifier, actor=node, clock=2,
                key=dict(kind='session', machine=node, session=session), parents=parents, value=None)
        graph = dict(schema=1, swarm_id=catalog['swarm_id'], operations=operations)

        def update(padding=0):
            canonical = catbytes(graph)
            raw = b' ' * padding + canonical
            request['source_primary_json'] = raw.decode()
            request['source']['primary'] = dict(state='nonempty', swarm_id=graph['swarm_id'],
                raw_sha256=sha(raw), raw_len=len(raw), canonical_sha256=sha(canonical),
                count=len(operations), parent_closure=True)
            rehash(request)
            return len(enc(request))

        update()
        if target_bytes is not None:
            difference = target_bytes - len(enc(request))
            self.assertGreaterEqual(difference, 0, difference)
            update(difference)
            self.assertEqual(len(enc(request)), target_bytes)
        self.assertEqual(sum(len(operation['parents']) for operation in operations.values()), total_edges)
        return request

    def join(self, node, peer):
        preview = self.call(node, 'preview', peer)
        self.call(node, 'join', peer, data=dict(node_id=preview['node_id'], swarm_id=preview['swarm_id'], project_map={}, prefer_peer_memberships=False))

    def add_sidecars(self):
        current = self.call('a', 'get')
        label = 'claude/example/shared'
        self.call('a', 'resolve', data=dict(key=json.dumps(dict(kind='session', machine=current['node_id'], session=label)), versions=[], value='shared'))
        store = json.loads((self.root / 'a/swarm/catalog.json').read_text())
        name_operation = next(op for op in store['catalog']['operations'].values()
                              if op['key']['kind'] == 'session' and op['key']['session'] == label)
        session = str(uuid.uuid4())
        operation = str(uuid.uuid4())
        graph = dict(schema=1, swarm_id=current['swarm_id'], operations={operation: dict(
            id=operation, actor=current['node_id'], clock=1, parents=[],
            key=dict(kind='session', machine=current['node_id'], session=session), value='shared')})
        (self.root / 'a/swarm/session-id-memberships.json').write_text(json.dumps(graph))
        twin = dict(id=str(uuid.uuid4()), actor=current['node_id'], clock=1,
                    machine=current['node_id'], session_id=session, label=label,
                    op=name_operation['id'], project='shared')
        twins = dict(swarm_id=current['swarm_id'], entries={current['node_id'] + '\n' + session: twin})
        (self.root / 'a/swarm/session-id-twins.json').write_text(json.dumps(twins))

    def empty_destination(self, bootstrapped):
        swarm = self.root / 'b/swarm'
        swarm.mkdir(parents=True)
        (swarm / 'catalog.lock').write_bytes(b'')
        store = dict(node_id=str(uuid.uuid4()), catalog=dict(schema=1, swarm_id=str(uuid.uuid4()), operations={}),
                     imported=False, bindings={}, peers={})
        (swarm / 'catalog.json').write_text(json.dumps(store))
        if bootstrapped:
            self.call('b', 'get')

    def options(self, preview):
        return dict(request_id=str(uuid.uuid4()), preview=preview,
                    native_ticket='fixture-native-owner', native_generation=1,
                    lease_id=str(uuid.uuid4()))

    def adopt(self, bootstrapped):
        self.empty_destination(bootstrapped)
        before = self.root / 'b/swarm/gui-import-pending.json'
        marker = before.read_bytes() if before.exists() else None
        source_operations = json.loads((self.root / 'a/swarm/catalog.json').read_text())['catalog']['operations']
        source_primary = (self.root / 'a/swarm/session-id-memberships.json').read_bytes()
        source_twins = (self.root / 'a/swarm/session-id-twins.json').read_bytes()
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        self.assertTrue(preview['eligible'], preview)
        self.assertEqual(preview['drive_status'], 'unsupported_uninitialized_without_reverse_peer')
        options = self.options(preview)
        receipt = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(receipt['phase'], 'settled', receipt)
        destination = json.loads((self.root / 'b/swarm/catalog.json').read_text())
        self.assertTrue(all(destination['catalog']['operations'].get(identifier) == operation
                            for identifier, operation in source_operations.items()))
        self.assertEqual(destination['peers'], {})
        self.assertTrue(all(binding['id'] == destination['node_id'] for binding in destination['bindings'].values()))
        self.assertEqual(json.loads((self.root / 'b/swarm/session-id-memberships.json').read_text()),
                         json.loads(source_primary))
        self.assertEqual(json.loads((self.root / 'b/swarm/session-id-twins.json').read_text()),
                         json.loads(source_twins))
        source = json.loads((self.root / 'a/swarm/catalog.json').read_text())
        self.assertEqual(source['peers']['b']['node'], destination['node_id'])
        self.assertIn('c', source['peers'])
        if marker is not None:
            self.assertEqual(before.read_bytes(), marker)
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(status['journal']['phase'], 'remote_committed')
        remote = status['journal']['receipts']['remote']
        self.assertEqual(remote['primary_receipt']['destination_raw_sha256'], hashlib.sha256(source_primary).hexdigest())
        self.assertEqual(remote['twins_receipt']['destination_raw_sha256'], hashlib.sha256(source_twins).hexdigest())
        self.assertFalse(status['guard_present'])
        calls = (self.root / 'calls').read_text().splitlines() if (self.root / 'calls').exists() else []
        self.assertFalse(any(line.startswith('c ') and line.split()[1:3] == ['swarm', 'adopt'] for line in calls))
        return options

    def test_adopts_into_uninitialized_destination_with_all_history_and_only_source_membership(self):
        self.adopt(False)

    def test_adopts_bootstrapped_pending_and_preserves_the_marker(self):
        self.adopt(True)

    def test_partial_component_failure_holds_the_guard_and_status_does_not_repair(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'fail-primary').touch()
        error = self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertTrue((self.root / 'fault-injected').exists())
        self.assertIn('NotRegular', error)
        guard = self.root / 'b/swarm/adoption-pending.json'
        self.assertTrue(guard.exists())
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(status['journal']['phase'], 'primary_started')
        self.assertTrue(status['guard_present'])
        original = guard.read_bytes()
        self.call('b', 'get', ok=False)
        self.call('b', 'hello', ok=False)
        self.assertEqual(guard.read_bytes(), original)
        duplicate = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(duplicate, status)
        self.assertEqual(guard.read_bytes(), original)

    def test_refused_not_started_continuation_reuses_the_original_live_call(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'b/swarm/drive-folders.json').write_bytes(b'')
        (self.root / 'refuse-once').touch()
        settled = self.call('a', 'adopt', 'b', '--json', data=options, timeout=35)
        self.assertEqual(settled['phase'], 'settled')
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertEqual(sum(line.split()[1:3] == ['swarm', 'adopt'] for line in calls), 2)
        duplicate = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(duplicate, settled)
        options['previous_lease_drained'] = True
        self.call('a', 'adopt', 'b', '--json', data=options, ok=False)

    def test_failed_reply_reconciles_only_inside_the_original_live_call(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'lose-reply').touch()
        settled = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(settled['phase'], 'settled')
        duplicate = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(duplicate, settled)
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertEqual(sum(line.split()[1:3] == ['swarm', 'adopt'] for line in calls), 1)
        self.assertEqual(sum(line.split()[1:3] == ['swarm', 'adoption-status'] for line in calls), 1)

    def test_missing_components_have_explicit_receipts(self):
        (self.root / 'a/swarm/session-id-memberships.json').unlink()
        (self.root / 'a/swarm/session-id-twins.json').unlink()
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        self.call('a', 'adopt', 'b', '--json', data=options)
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        remote = status['journal']['receipts']['remote']
        for component in ('primary_receipt', 'twins_receipt'):
            self.assertEqual(remote[component]['source']['state'], 'missing')
            self.assertEqual(remote[component]['destination']['state'], 'missing')
            self.assertIsNone(remote[component]['source']['raw_sha256'])

    def test_present_empty_destination_becomes_missing_only_after_proved_empty_removal(self):
        (self.root / 'a/swarm/session-id-memberships.json').unlink()
        (self.root / 'a/swarm/session-id-twins.json').unlink()
        self.empty_destination(False)
        destination = json.loads((self.root / 'b/swarm/catalog.json').read_text())
        swarm = destination['catalog']['swarm_id']
        (self.root / 'b/swarm/session-id-memberships.json').write_text(json.dumps(
            dict(schema=1, swarm_id=swarm, operations={})))
        (self.root / 'b/swarm/session-id-twins.json').write_text(json.dumps(
            dict(swarm_id=swarm, entries={})))
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        self.call('a', 'adopt', 'b', '--json', data=options)
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        remote = status['journal']['receipts']['remote']
        for component in ('primary_receipt', 'twins_receipt'):
            self.assertEqual(remote[component]['source']['state'], 'missing')
            self.assertEqual(remote[component]['destination']['state'], 'missing')
            self.assertIsNone(remote[component]['destination_raw_sha256'])
        self.assertFalse(status['guard_present'])

    def test_valid_empty_components_keep_raw_and_canonical_identity(self):
        current = self.call('a', 'get')
        (self.root / 'a/swarm/session-id-memberships.json').write_text(json.dumps(
            dict(schema=1, swarm_id=current['swarm_id'], operations={})))
        (self.root / 'a/swarm/session-id-twins.json').write_text(json.dumps(
            dict(swarm_id=current['swarm_id'], entries={})))
        primary = (self.root / 'a/swarm/session-id-memberships.json').read_bytes()
        twins = (self.root / 'a/swarm/session-id-twins.json').read_bytes()
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        self.call('a', 'adopt', 'b', '--json', data=options)
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        remote = status['journal']['receipts']['remote']
        self.assertEqual(remote['primary_receipt']['source']['state'], 'valid_empty')
        self.assertEqual(remote['twins_receipt']['source']['state'], 'valid_empty')
        self.assertEqual(remote['primary_receipt']['destination_raw_sha256'], hashlib.sha256(primary).hexdigest())
        self.assertEqual(remote['twins_receipt']['destination_raw_sha256'], hashlib.sha256(twins).hexdigest())

    def test_occupied_destination_and_full_body_tamper_refuse_without_writes(self):
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        self.call('b', 'initialize', data={'projects': [project('occupied')]})
        occupied = (self.root / 'b/swarm/catalog.json').read_bytes()
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        self.assertFalse(preview['eligible'])
        self.call('a', 'adopt', 'b', '--json', data=self.options(preview), ok=False)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), occupied)
        shutil.rmtree(self.root / 'b/swarm')
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        twins_path = self.root / 'a/swarm/session-id-twins.json'
        twins = json.loads(twins_path.read_text())
        next(iter(twins['entries'].values()))['clock'] = 2
        twins_path.write_text(json.dumps(twins))
        error = self.call('a', 'adopt', 'b', '--json', data=self.options(preview), ok=False)
        self.assertIn('source changed', error)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), original)

    def test_aggregate_request_cap_refuses_before_journal_or_remote_effect(self):
        path = self.root / 'a/swarm/catalog.json'
        store = json.loads(path.read_text())
        node = store['node_id']
        operations = store['catalog']['operations']
        for index in range(4500):
            identifier = str(uuid.uuid4())
            operations[identifier] = dict(
                id=identifier, actor=node, clock=index + 1, parents=[],
                key=dict(kind='folder', id=f'oversize-{index}'),
                value=dict(project='shared', machine=node, path='/' + 'x' * 8191, name=''))
        path.write_text(json.dumps(store))
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        error = self.call('a', 'adopt', 'b', '--json', data=self.options(preview), ok=False)
        self.assertIn('32MiB', error)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), original)
        self.assertFalse((self.root / 'a/swarm/adoption-journal').exists())
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertFalse(any(line.startswith('b swarm adopt ') for line in calls))

    def test_missing_parent_and_missing_capability_evidence_refuse_before_effects(self):
        path = self.root / 'a/swarm/session-id-memberships.json'
        valid = path.read_bytes()
        graph = json.loads(valid)
        next(iter(graph['operations'].values()))['parents'] = ['missing-parent']
        path.write_text(json.dumps(graph))
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        self.call('a', 'adoption-preview', 'b', '--json', ok=False)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), original)
        path.write_bytes(valid)
        shutil.rmtree(self.root / 'b/swarm')
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        (self.root / 'old-cli').touch()
        error = self.call('a', 'adoption-preview', 'b', '--json', ok=False)
        self.assertIn('unknown adoption command', error)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), original)

    def test_later_gui_import_adds_only_missing_fields_and_removes_marker(self):
        options = self.adopt(True)
        marker = self.root / 'b/swarm/gui-import-pending.json'
        self.assertTrue(marker.exists())
        imported = self.call('b', 'initialize', data={'projects': [project('gui-new')]})
        ids = {project['id'] for project in imported['organization']['projects']}
        self.assertIn('shared', ids)
        self.assertIn('gui-new', ids)
        self.assertFalse(marker.exists())
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(status['journal']['phase'], 'remote_committed')

    def test_unexpected_recovery_metadata_is_not_an_empty_destination(self):
        self.empty_destination(False)
        original = (self.root / 'b/swarm/catalog.json').read_bytes()
        (self.root / 'b/swarm/backup-before-import.json').write_bytes(b'{}')
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        self.assertFalse(preview['eligible'])
        self.assertIn('unexpected catalog metadata', ' '.join(preview['destination']['reasons']))
        self.call('a', 'adopt', 'b', '--json', data=self.options(preview), ok=False)
        self.assertEqual((self.root / 'b/swarm/catalog.json').read_bytes(), original)

    def test_failed_guard_removal_is_not_terminal_and_status_cannot_clear_it(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'fail-removal').touch()
        error = self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertIn('Is a directory', error)
        self.assertTrue((self.root / 'fault-injected').exists())
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertIn(status['journal']['phase'], ('components_verified', 'guard_clearing'))
        self.assertTrue(status['guard_present'])
        self.assertNotIn('remote', status['journal']['receipts'])
        self.call('b', 'get', ok=False)
        duplicate = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(duplicate, status)


    def test_duplicate_raw_maps_refuse_before_source_or_destination_effects(self):
        for mode in ('identical', 'before', 'after'):
            with self.subTest(mode=mode):
                self.setUp()
                path = self.root / 'a/swarm/catalog.json'
                store = json.loads(path.read_text())
                operations = store['catalog']['operations']
                identifier, original = next(iter(operations.items()))
                rewritten = json.loads(json.dumps(original))
                rewritten['value'] = 'rewritten immutable body'
                pair = [original, original] if mode == 'identical' else [original, rewritten]
                if mode == 'after':
                    pair.reverse()
                pairs = [(identifier, value) for value in pair]
                pairs.extend((key, value) for key, value in operations.items() if key != identifier)
                store['catalog']['operations'] = '__RAW_PAIRS__'
                body = enc(store).replace(b'"__RAW_PAIRS__"', b'{' + b','.join(
                    enc(key) + b':' + enc(value) for key, value in pairs) + b'}')
                path.write_bytes(body)
                self.empty_destination(False)
                before = self.tree('b')
                self.call('a', 'adoption-preview', 'b', '--json', ok=False)
                self.assertEqual(path.read_bytes(), body)
                self.assertEqual(self.tree('b'), before)
        self.setUp()
        self.empty_destination(False)
        path = self.root / 'b/swarm/catalog.json'
        store = json.loads(path.read_text())
        store['peers'] = '__RAW_PEERS__'
        body = enc(store).replace(b'"__RAW_PEERS__"', b'{"duplicate":null,"duplicate":null}')
        path.write_bytes(body)
        before = self.tree('b')
        self.call('a', 'adoption-preview', 'b', '--json', ok=False)
        self.assertEqual(path.read_bytes(), body)
        self.assertEqual(self.tree('b'), before)

    def test_rehashed_source_identity_and_duplicate_request_have_no_destination_effect(self):
        request = self.capture_request()
        request['source']['swarm_id'] = str(uuid.uuid4())
        rehash(request)
        before = self.tree('b')
        result = self.raw('b', 'adopt', '--json', data=enc(request))
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'declared source/catalog swarm mismatch', result.stderr)
        self.assertEqual(self.tree('b'), before)
        body = enc(request).replace(b'"request_id":', b'"request_id":"duplicate","request_id":', 1)
        result = self.raw('b', 'adopt', '--json', data=body)
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'duplicate object key', result.stderr)
        self.assertEqual(self.tree('b'), before)

    def test_aggregate_edge_counter_is_pre_effect_and_at_boundary_candidate_commits(self):
        request = self.metrics_request(500736, 21029685)
        before = self.tree('b')
        result = self.raw('b', 'adopt', '--json', data=enc(request))
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'parent edges exceed 500000', result.stderr)
        self.assertEqual(self.tree('b'), before)
        self.setUp()
        request = self.metrics_request(500000)
        result = self.raw('b', 'adopt', '--json', data=enc(request))
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt['phase'], 'remote_committed')
        self.assertFalse((self.root / 'b/swarm/adoption-pending.json').exists())
        self.assertEqual((self.root / 'b/swarm/session-id-memberships.json').read_bytes(),
                         request['source_primary_json'].encode())

    def start_original(self, options):
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'a'), HGS_STATE_DIR=str(self.root / 'a/state'))
        process = subprocess.Popen([str(HGS), 'swarm', 'adopt', 'b', '--json'], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
        self.addCleanup(process.stdout.close)
        self.addCleanup(process.stderr.close)
        process.stdin.write(enc(options))
        process.stdin.close()
        import time
        deadline = time.monotonic() + 15
        while not (self.root / 'descendant-holding').exists() and time.monotonic() < deadline:
            self.assertIsNone(process.poll())
            time.sleep(.01)
        self.assertTrue((self.root / 'descendant-holding').exists())
        return process

    def test_exact_cap_open_pipes_exclude_another_process_until_actual_eof(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'cap-reply').touch()
        (self.root / 'hold-pipes').touch()
        process = self.start_original(options)
        self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])
        error = self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertIn('read-only', error)
        (self.root / 'release-pipes').touch()
        stdout = process.stdout.read()
        process.wait(timeout=40)
        self.assertEqual(process.returncode, 0, process.stderr.read())
        self.assertEqual(json.loads(stdout)['phase'], 'settled')
        duplicate = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(duplicate['phase'], 'settled')

    def test_duplicate_replies_and_overflow_never_write_source_membership(self):
        for mode in ('identical', 'before', 'after', 'overflow'):
            with self.subTest(mode=mode):
                self.setUp()
                self.empty_destination(False)
                preview = self.call('a', 'adoption-preview', 'b', '--json')
                options = self.options(preview)
                (self.root / ('overflow-reply' if mode == 'overflow' else 'duplicate-reply')).write_text(mode)
                self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
                self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])
                journal = self.root / 'a/swarm/adoption-journal' / (options['request_id'] + '.json')
                self.assertEqual(json.loads(journal.read_text())['phase'], 'source_pending')
                self.call('a', 'adopt', 'b', '--json', data=options, ok=False)

    def test_wrong_original_receipts_retain_source_intent_without_membership(self):
        for mode in ('destination', 'source', 'owner', 'component', 'guard'):
            with self.subTest(mode=mode):
                self.setUp()
                self.empty_destination(False)
                preview = self.call('a', 'adoption-preview', 'b', '--json')
                options = self.options(preview)
                (self.root / 'tamper-reply').write_text(mode)
                self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
                self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])
                journal = self.root / 'a/swarm/adoption-journal' / (options['request_id'] + '.json')
                self.assertEqual(json.loads(journal.read_text())['phase'], 'source_pending')
                self.call('a', 'adopt', 'b', '--json', data=options, ok=False)

    def test_timeout_without_eof_remains_manual_and_never_replays(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'hold-pipes').touch()
        process = self.start_original(options)
        stdout = process.stdout.read()
        process.wait(timeout=40)
        self.assertNotEqual(process.returncode, 0, process.stderr.read())
        self.assertEqual(stdout, b'')
        (self.root / 'release-pipes').touch()
        self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])
        self.call('a', 'adopt', 'b', '--json', data=options, ok=False)

    def test_guard_cleared_crash_is_status_only_and_cannot_be_finalized(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'fail-after-guard-cleared').touch()
        self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertTrue((self.root / 'fault-injected').exists())
        status = self.call('b', 'adoption-status', '--json', data={'request_id': options['request_id']})
        self.assertEqual(status['journal']['phase'], 'guard_cleared')
        self.assertFalse(status['guard_present'])
        self.assertNotIn('remote', status['journal']['receipts'])
        before = self.tree('b')
        self.call('b', 'get', ok=False)
        self.call('b', 'adopt', '--json', data={'request_id': options['request_id']}, ok=False)
        self.assertEqual(self.tree('b'), before)
        self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])

    def redirected_primary(self, redirected=True):
        """A valid primary causal edge from an old machine key to its canonical
        new-machine child. Only the main catalog's redirect makes it valid."""
        store_path = self.root / 'a/swarm/catalog.json'
        store = json.loads(store_path.read_text())
        node = store['node_id']
        catalog = store['catalog']
        clock = max(operation['clock'] for operation in catalog['operations'].values()) + 1
        old_machine = str(uuid.uuid4())
        if redirected:
            redirect = str(uuid.uuid4())
            catalog['operations'][redirect] = dict(
                id=redirect, actor=node, clock=clock, parents=[],
                key=dict(kind='machine', id=old_machine, field='redirect'), value=node)
            store_path.write_text(json.dumps(store))
        primary_path = self.root / 'a/swarm/session-id-memberships.json'
        graph = json.loads(primary_path.read_text())
        session = str(uuid.uuid4())
        parent, child = str(uuid.uuid4()), str(uuid.uuid4())
        graph['operations'][parent] = dict(
            id=parent, actor=node, clock=1, parents=[],
            key=dict(kind='session', machine=old_machine, session=session), value='shared')
        graph['operations'][child] = dict(
            id=child, actor=node, clock=2, parents=[parent],
            key=dict(kind='session', machine=node, session=session), value='shared')
        primary_path.write_text(json.dumps(graph))
        return primary_path.read_bytes(), parent, child

    def test_valid_redirected_primary_parent_adopts_with_the_main_context(self):
        primary, parent, child = self.redirected_primary()
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        self.assertTrue(preview['eligible'], preview)
        options = self.options(preview)
        receipt = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(receipt['phase'], 'settled', receipt)
        destination = self.root / 'b/swarm/session-id-memberships.json'
        self.assertEqual(json.loads(destination.read_text()), json.loads(primary))
        source = self.root / 'a/swarm/session-id-memberships.json'
        self.assertEqual(json.loads(destination.read_text()), json.loads(source.read_text()))
        operations = json.loads(destination.read_text())['operations']
        self.assertEqual(operations[child]['parents'], [parent])
        self.assertIn(parent, operations)
        self.assertEqual(json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers']['b']['node'],
                         json.loads((self.root / 'b/swarm/catalog.json').read_text())['node_id'])

    def test_unredirected_primary_parent_refuses_before_any_effect(self):
        self.redirected_primary(redirected=False)
        self.empty_destination(False)
        source, destination = self.tree('a'), self.tree('b')
        self.call('a', 'adoption-preview', 'b', '--json', ok=False)
        self.assertEqual((self.tree('a'), self.tree('b')), (source, destination))

    def test_local_membership_failure_is_write_ahead_and_not_settled(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        membership = self.root / 'a/swarm/membership.json'
        membership.unlink()
        membership.mkdir()
        self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        journal = self.root / 'a/swarm/adoption-journal' / (options['request_id'] + '.json')
        self.assertEqual(json.loads(journal.read_text())['phase'], 'source_membership_started')
        self.assertNotIn('b', json.loads((self.root / 'a/swarm/catalog.json').read_text())['peers'])
        self.call('a', 'adopt', 'b', '--json', data=options, ok=False)


if __name__ == '__main__':
    unittest.main()
