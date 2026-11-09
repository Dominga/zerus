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
    request = json.loads(sys.stdin.read())
    process = subprocess.Popen([os.environ['SWARM_TEST_HGS'], *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
    process.stdin.write(json.dumps(request)); process.stdin.close()
    if (root / 'fail-primary').exists() or (root / 'fail-removal').exists():
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
            time.sleep(.0001)
    stdout, stderr = process.stdout.read(), process.stderr.read()
    process.wait(timeout=30)
    if (root / 'lose-reply').exists():
        sys.stderr.write('fixture reply lost'); sys.exit(1)
    sys.stdout.write(stdout); sys.stderr.write(stderr); sys.exit(process.returncode)
os.execve(os.environ['SWARM_TEST_HGS'], [os.environ['SWARM_TEST_HGS'], *args], env)
'''


def project(identifier):
    return dict(id=identifier, name='Shared project', color='#123456', vivid=False,
                folders=[dict(id='folder-shared', machine='a', path='/a', name='')], sessions=[])


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

    def options(self, preview, drained=False):
        return dict(request_id=str(uuid.uuid4()), preview=preview,
                    native_ticket='fixture-native-owner', native_generation=1,
                    previous_lease_drained=drained)

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
        self.assertIn('Is a directory', error)
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

    def test_refused_not_started_continuation_reuses_the_original_request(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        drive = self.root / 'b/swarm/drive-folders.json'
        drive.write_bytes(b'')
        refused = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(refused['outcome'], 'refused_not_started')
        self.assertFalse((self.root / 'b/swarm/adoption-pending.json').exists())
        drive.unlink()
        options['previous_lease_drained'] = True
        settled = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(settled['phase'], 'settled')
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertEqual(sum(line.split()[1:3] == ['swarm', 'adopt'] for line in calls), 2)

    def test_remote_committed_status_permits_only_local_source_membership(self):
        self.empty_destination(False)
        preview = self.call('a', 'adoption-preview', 'b', '--json')
        options = self.options(preview)
        (self.root / 'lose-reply').touch()
        error = self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertIn('fixture reply lost', error)
        (self.root / 'lose-reply').unlink()
        blocked = self.call('a', 'adopt', 'b', '--json', data=options, ok=False)
        self.assertIn('drain', blocked)
        options['previous_lease_drained'] = True
        settled = self.call('a', 'adopt', 'b', '--json', data=options)
        self.assertEqual(settled['phase'], 'settled')
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
            self.assertEqual(remote[component]['source_state'], 'missing')
            self.assertEqual(remote[component]['destination_state'], 'missing')
            self.assertIsNone(remote[component]['original_source_raw_sha256'])

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
            self.assertEqual(remote[component]['source_state'], 'missing')
            self.assertEqual(remote[component]['destination_state'], 'missing')
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
        self.assertEqual(remote['primary_receipt']['source_state'], 'valid_empty')
        self.assertEqual(remote['twins_receipt']['source_state'], 'valid_empty')
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


if __name__ == '__main__':
    unittest.main()
