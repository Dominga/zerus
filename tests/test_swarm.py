#!/usr/bin/env python3
"""Catalog replication through the real CLI, with isolated nodes and fake SSH."""
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug/hgs')).resolve()
SSH = '''#!/usr/bin/env python3
import json, os, pathlib, shlex, subprocess, sys
if sys.argv[-1].startswith('echo "hgs-remote-v2|'): print('hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot'); sys.exit(0)
root = pathlib.Path(os.environ['SWARM_TEST_ROOT'])
alias = sys.argv[-2]
if (root / ('offline-' + alias)).exists():
    print('fixture offline', file=sys.stderr); sys.exit(255)
env = dict(os.environ, HGS_CONFIG_DIR=str(root / alias), HGS_STATE_DIR=str(root / alias / 'state'))
args = shlex.split(sys.argv[-1])[1:]
with (root / 'calls').open('a') as file: file.write(alias + ' ' + ' '.join(args) + '\\n')
if (root / ('spoil-' + alias)).exists() and args[1:2] == [json.loads((root / ('spoil-' + alias)).read_text())['command']]:
    # A member whose replica answer loses one field, or carries it in another
    # form: {command, field[, value]}.
    spoil = json.loads((root / ('spoil-' + alias)).read_text())
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=sys.stdin.read(), env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    if result.returncode: sys.exit(result.returncode)
    output = json.loads(result.stdout)
    if 'value' in spoil: output[spoil['field']] = spoil['value']
    else: output.pop(spoil['field'], None)
    print(json.dumps(output)); sys.exit(0)
if (root / ('nolabels-' + alias)).exists() and args[1:2] in (['hello'], ['session-labels']):
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=sys.stdin.read(), env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    if result.returncode: sys.exit(result.returncode)
    output = json.loads(result.stdout)
    context = output.get('session_labels', output)
    if isinstance(context, dict): context.get('context', {}).pop('labels_by_id', None)
    print(json.dumps(output)); sys.exit(0)
if (root / ('linger-' + alias)).exists() and args[1:2] == ['hello']:
    # The answer comes at once, but a descendant keeps the pipe open.
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=sys.stdin.read(), env=env, text=True, capture_output=True)
    sys.stdout.write(result.stdout); sys.stdout.flush()
    subprocess.Popen(['sleep', (root / ('linger-' + alias)).read_text()], stdout=sys.stdout)
    sys.exit(result.returncode)
if (root / ('reset-store-' + alias)).exists() and args[1:2] == ['replica-inventory']:
    # The member's replica root is replaced after its hello: the store
    # identity it answers with is created again.
    (root / ('reset-store-' + alias)).unlink()
    (root / alias / 'state' / 'replica' / 'store-id').unlink()
if (root / ('noreplica-' + alias)).exists():
    # A member from before task replication: its hello has no replica field and
    # it knows no replica commands.
    if args[1:2] in (['replica-inventory'], ['replica-exchange']):
        print('usage: hgs swarm get | initialize', file=sys.stderr); sys.exit(1)
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=sys.stdin.read(), env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    if result.returncode: sys.exit(result.returncode)
    output = json.loads(result.stdout); output.pop('replica', None)
    print(json.dumps(output)); sys.exit(0)
if (root / ('preference-' + alias)).exists():
    # A member from before the shared project order: it lists the kinds it
    # accepts, without "order", and rejects that kind.
    request = sys.stdin.read()
    if args[1:] == ['exchange'] and any(op['key']['kind'] == 'order' for op in json.loads(request)['operations']):
        print('unknown variant from a newer member', file=sys.stderr); sys.exit(1)
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=request, env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    if result.returncode: sys.exit(result.returncode)
    output = json.loads(result.stdout)
    if 'kinds' in output: output['kinds'] = [kind for kind in output['kinds'] if kind != 'order']
    print(json.dumps(output)); sys.exit(0)
if (root / ('nosessionid-' + alias)).exists():
    # A member from before session membership by ID (v0.47.0): it lists its
    # kinds without "session_id", shows no twins and, as its exchange denies
    # unknown fields, refuses a request that carries them.
    request = sys.stdin.read()
    if args[1:] == ['exchange'] and {'twins', 'known_twins', 'id_memberships', 'known_id_memberships'} & set(json.loads(request)):
        print('unknown field from a newer member', file=sys.stderr); sys.exit(1)
    result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=request, env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    if result.returncode: sys.exit(result.returncode)
    output = json.loads(result.stdout)
    if 'kinds' in output: output['kinds'] = [kind for kind in output['kinds'] if kind not in ('session_id', 'session_id_memberships_v1')]
    for field in ('twins', 'known_twins', 'id_memberships', 'known_id_memberships'): output.pop(field, None)
    print(json.dumps(output)); sys.exit(0)
if not (root / ('old-' + alias)).exists():
    os.execve(os.environ['SWARM_TEST_HGS'], [os.environ['SWARM_TEST_HGS'], *args], env)
# A member from before shared preferences: it rejects unknown fields and key
# kinds and never advertises the kinds it accepts.
request = sys.stdin.read()
if args[1:] == ['exchange']:
    data = json.loads(request)
    if 'kinds' in data or any(op['key']['kind'] not in ('project', 'folder', 'session', 'machine') for op in data['operations']):
        print('unknown field or variant from a newer member', file=sys.stderr); sys.exit(1)
result = subprocess.run([os.environ['SWARM_TEST_HGS'], *args], input=request, env=env, text=True, capture_output=True)
sys.stderr.write(result.stderr)
if result.returncode: sys.exit(result.returncode)
output = json.loads(result.stdout)
output.pop('kinds', None)
print(json.dumps(output))
'''


def noise(seed, length):
    """Text that does not compress, as task documents are compressed."""
    out = ''
    while len(out) < length:
        out += hashlib.sha256(f'{seed}-{len(out)}'.encode()).hexdigest()
    return out[:length]


def project(id, name, machine, path):
    return dict(id=id, name=name, color='#123456', vivid=False,
                folders=[dict(id='folder-' + id, machine=machine, path=path, name='')], sessions=[])


class Swarm(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='hgs-swarm-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        (self.root / 'bin').mkdir()
        ssh = self.root / 'bin/ssh'; ssh.write_text(SSH); ssh.chmod(0o755)
        self.env = dict(os.environ, SWARM_TEST_ROOT=str(self.root), SWARM_TEST_HGS=str(HGS),
                        PATH=str(self.root / 'bin') + ':' + os.environ['PATH'], HGS_TAB='0')
        # Commands of the test are the person's, never the agent's that runs it.
        for key in ['HGS_SELF', 'HGS_PEERS', 'HGS_EXECUTABLE', 'HGS_SESSION', 'HGS_RUN_ID']:
            self.env.pop(key, None)
        for node, peers in [('a', 'b'), ('b', 'a c'), ('c', 'b')]:
            (self.root / node).mkdir()
            (self.root / node / 'config').write_text(f'HGS_SELF="{node}"\nHGS_PEERS="{peers}"\n')
            (self.root / node / 'machines.local.json').write_text(json.dumps({'machines': {peers.split()[0]: dict(hostname='host-secret.invalid', user='private-user', identity_file='/private/key', enabled=True)}}))
            self.call(node, 'initialize', data={'projects': [project(node, 'Same name', node, '/' + node)]})

    def call(self, node, *args, data=None, ok=True):
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / node), HGS_STATE_DIR=str(self.root / node / 'state'))
        result = subprocess.run([str(HGS), 'swarm', *args], input=json.dumps(data) if data is not None else None,
                                env=env, text=True, capture_output=True, timeout=40)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode, 0)
        return result.stderr

    def task(self, node, *args, text=None):
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / node), HGS_STATE_DIR=str(self.root / node / 'state'))
        result = subprocess.run([str(HGS), 'task', *args], input=text, env=env, text=True, capture_output=True, timeout=40)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def tasks(self, node):
        return sorted(t['text'] for t in self.task(node, 'list', '--project', 'a')['tasks'])

    def exchanges(self, alias=''):
        calls = self.root / 'calls'
        lines = calls.read_text().splitlines() if calls.exists() else []
        return sum('replica-exchange' in line and line.startswith(alias) for line in lines)

    def join(self, node, peer, mapping=None, prefer_peer=False):
        preview = self.call(node, 'preview', peer)
        return self.call(node, 'join', peer, data=dict(node_id=preview['node_id'], swarm_id=preview['swarm_id'], project_map=mapping or {}, prefer_peer_memberships=prefer_peer))

    def patch(self, node, before, desired):
        return self.call(node, 'apply', data=dict(base=before['organization'], desired=desired, versions=before['versions']))

    def converge(self):
        for node in ['a', 'b', 'c', 'b', 'a', 'b']:
            self.call(node, 'sync')
        snapshots = [self.call(node, 'get') for node in ['a', 'b', 'c']]
        self.assertEqual(len({s['digest'] for s in snapshots}), 1)
        return snapshots

    def test_the_cli_observation_keeps_the_unfinished_native_writer_floor_off(self):
        self.env['XDG_CONFIG_HOME'] = str(self.root / 'xdg')
        (self.root / 'a/config').write_text('HGS_SELF="a"\nHGS_PEERS=""\n')
        (self.root / 'a/machines.local.json').write_text('{"machines":{}}')
        context = self.call('a', 'session-labels-context', '--json')
        observation = self.call('a', 'session-labels', '--json')
        self.assertIs(observation.get('native_continuity'), False)
        self.assertEqual(observation['context'], context)
        self.assertNotIn('native_continuity', context, 'connector Context stays unchanged')

    def test_label_gate_needs_every_fresh_cli_and_actual_connector(self):
        self.env['XDG_CONFIG_HOME'] = str(self.root / 'xdg')
        # Only A and B join this swarm. C is not part of its declared roster.
        (self.root / 'b/config').write_text('HGS_SELF="b"\nHGS_PEERS="a"\n')
        gate = lambda: self.call('a', 'session-labels-gate', '--json')['ready']
        self.assertFalse(gate())
        self.join('a', 'b'); self.join('b', 'a')
        for node in ('a', 'b', 'a', 'b', 'a'):
            self.call(node, 'sync')
        self.assertTrue(gate())
        # A fresh hello from the foundation build invalidates the earlier
        # positive, despite an unchanged package version and membership.
        old = self.root / 'nolabels-b'; old.touch()
        self.call('a', 'sync'); self.assertFalse(gate())
        old.unlink(); self.call('a', 'sync'); self.assertTrue(gate())
        offline = self.root / 'offline-b'; offline.touch()
        self.call('a', 'sync'); self.assertFalse(gate())
        offline.unlink(); self.call('a', 'sync'); self.assertTrue(gate())
        # The canonical slot alone requires the RUNNING responder. An absent
        # or old connector cannot be replaced by this new CLI's help marker.
        connector = self.root / 'xdg/hgs/mobile/connector.json'
        connector.parent.mkdir(parents=True, mode=0o700); connector.write_text('{}'); connector.chmod(0o600)
        self.assertFalse(gate())
        started = time.monotonic(); self.call('a', 'sync')
        self.assertLess(time.monotonic() - started, 8)
        self.assertFalse(gate())
        connector.unlink()
        # Scope restoration alone never resurrects an old positive.
        self.assertFalse(gate())
        self.call('a', 'sync'); self.assertTrue(gate())
        # A configured machine which has never answered is also closed.
        (self.root / 'a/config').write_text('HGS_SELF="a"\nHGS_PEERS="b c"\n')
        self.assertFalse(gate())

    def test_third_node_transitive_sync_access_filter_and_private_configuration(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        a, b, c = self.converge()
        self.assertEqual({p['id'] for p in c['organization']['projects']}, {'a', 'b', 'c'})
        # C only configured B. Receiving A's identity cannot grant access to A.
        projects = {p['id']: p for p in c['organization']['projects']}
        self.assertFalse(projects['a']['accessible'])
        self.assertTrue(projects['b']['accessible'])
        self.assertTrue(projects['c']['accessible'])
        exported = json.dumps(self.call('c', 'export'))
        for secret in ['host-secret', 'private-user', '/private/key', 'identity_file', 'hostname']:
            self.assertNotIn(secret, exported)
        # A folder on a configured computer remains visible while offline.
        (self.root / 'offline-b').touch()
        self.assertFalse(self.call('c', 'sync')['b']['ok'])
        self.assertTrue(next(p for p in self.call('c', 'get')['organization']['projects'] if p['id'] == 'b')['accessible'])

    def test_offline_conflicts_deletion_resolution_and_repeated_sync(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        a, b, c = self.converge()
        first, second = copy.deepcopy(a['organization']), copy.deepcopy(b['organization'])
        next(p for p in first['projects'] if p['id'] == 'a')['name'] = 'First edit'
        next(p for p in second['projects'] if p['id'] == 'a')['name'] = 'Second edit'
        first['projects'] = [p for p in first['projects'] if p['id'] != 'c']
        self.patch('a', a, first); self.patch('b', b, second)
        third = copy.deepcopy(c['organization'])
        next(p for p in third['projects'] if p['id'] == 'c')['name'] = 'Late offline edit'
        self.patch('c', c, third)
        a, b, c = self.converge()
        self.assertNotIn('c', [p['id'] for p in c['organization']['projects']])
        conflict = next(c for c in a['conflicts'] if c['field'].get('field') == 'name' and c['field'].get('id') == 'a')
        self.assertEqual({v['value'] for v in conflict['variants']}, {'First edit', 'Second edit'})
        self.call('a', 'resolve', data=dict(key=conflict['key'], versions=[v['id'] for v in conflict['variants']], value='Resolved'))
        a, b, c = self.converge()
        self.assertEqual(a['conflicts'], [])
        self.assertEqual(next(p for p in c['organization']['projects'] if p['id'] == 'a')['name'], 'Resolved')
        self.assertEqual(a['digest'], self.converge()[0]['digest'])

    def test_a_selection_move_from_a_stale_base_conflicts_only_on_the_member_another_writer_moved(self):
        # zerus-5wmh.18.1: Move to project for a selection is one apply with
        # every member. A second writer moves one of them between this
        # writer's get and its apply; hgs writes each member with its own
        # observed heads, so only that member becomes a conflict for review.
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        desired['projects'] += [dict(id=p, name=p.title(), color='#123456', folders=[], sessions=[]) for p in ('left', 'right', 'middle')]
        self.patch('a', before, desired)
        x, y, z = (f'a\ncodex/example/{n}' for n in ('x', 'y', 'z'))

        def assignment(snapshot, sessions, project_id):
            desired = copy.deepcopy(snapshot['organization'])
            desired['session_assignments_explicit'] = True
            for project in desired['projects']:
                project['sessions'] = [s for s in project['sessions'] if s not in sessions]
                if project['id'] == project_id:
                    project['sessions'] += sessions
            return desired

        def members(snapshot, project_id):
            return next(p for p in snapshot['organization']['projects'] if p['id'] == project_id)['sessions']

        start = self.call('a', 'get')
        self.patch('a', start, assignment(start, [x, y, z], 'left'))
        seen = self.call('a', 'get')                       # this writer's get
        other = self.call('a', 'get')                      # the second writer's get
        self.patch('a', other, assignment(other, [x], 'right'))
        result = self.patch('a', seen, assignment(seen, [x, y, z], 'middle'))
        self.assertTrue(result['written'])
        after = self.call('a', 'get')
        self.assertEqual(len(after['conflicts']), 1, after['conflicts'])
        conflict = after['conflicts'][0]
        self.assertIn('codex/example/x', conflict['key'])
        self.assertEqual(len(conflict['variants']), 2)
        # y and z land in the chosen project; nothing else changed.
        self.assertTrue({y, z} <= set(members(after, 'middle')), members(after, 'middle'))
        self.assertEqual(members(after, 'left'), [])
        self.assertEqual({p['id'] for p in after['organization']['projects']}, {p['id'] for p in seen['organization']['projects']})

    def test_join_does_not_merge_names_without_explicit_mapping(self):
        self.join('a', 'b')
        self.assertEqual(len(self.call('a', 'get')['organization']['projects']), 2)

    def test_observation_and_concurrent_inbox_cannot_undo_concrete_projects(self):
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        desired['projects'].append(dict(id='ungrouped', name='Inbox', color='#123456', folders=[], sessions=[]))
        self.patch('a', before, desired)
        self.join('a', 'b'); self.join('b', 'a')

        def sync():
            for node in ('a', 'b', 'a'):
                self.call(node, 'sync')
            snapshots = [self.call(node, 'get') for node in ('a', 'b')]
            self.assertEqual(snapshots[0]['digest'], snapshots[1]['digest'])
            return snapshots

        def assignment(snapshot, session, project_id):
            desired = copy.deepcopy(snapshot['organization'])
            desired['session_assignments_explicit'] = True
            for project in desired['projects']:
                project['sessions'] = [s for s in project['sessions'] if s != session]
                if project['id'] == project_id:
                    project['sessions'].append(session)
            return desired

        # Qt's firstObservationDoesNotRaceAnotherDesktopAssignment verifies
        # that real GUI initialize/apply payloads omit passive memberships.
        # Replay that contract through isolated CLI nodes in both write orders.
        for observer_first in (True, False):
            a, b = sync()
            session = f'a\ncodex/example/launch-{int(observer_first)}'
            observed = copy.deepcopy(a['organization'])
            observed['session_assignments_explicit'] = True
            observed['runs'] = {'a\nrun-one': session}  # Local presentation only.
            launched = assignment(b, session, 'b')
            for node, base, desired in ([('a', a, observed), ('b', b, launched)] if observer_first
                                        else [('b', b, launched), ('a', a, observed)]):
                result = self.patch(node, base, desired)
                if node == 'a':
                    self.assertEqual(result['written'], {})
            for snapshot in sync():
                self.assertEqual(snapshot['conflicts'], [])
                self.assertIn(session, next(p for p in snapshot['organization']['projects'] if p['id'] == 'b')['sessions'])

        # A concurrent Inbox choice loses to one known concrete project. The
        # observed versions still include it, so a later explicit Inbox move wins.
        a, b = sync(); session = 'a\ncodex/example/explicit-choice'
        self.patch('b', b, assignment(b, session, 'b'))
        sync()
        self.patch('a', a, assignment(a, session, 'ungrouped'))
        a, b = sync()
        self.assertEqual(a['conflicts'], [])
        self.assertIn(session, next(p for p in a['organization']['projects'] if p['id'] == 'b')['sessions'])
        self.patch('a', a, assignment(a, session, 'ungrouped'))
        for snapshot in sync():
            self.assertEqual(snapshot['conflicts'], [])
            self.assertIn(session, next(p for p in snapshot['organization']['projects'] if p['id'] == 'ungrouped')['sessions'])
        # Neither the local fallback marker nor the additive intent hint is a
        # new shared operation or stored catalog field for resident peers.
        exported = json.dumps(self.call('a', 'export'))
        self.assertNotIn('unassigned_sessions', exported)
        self.assertNotIn('session_assignments_explicit', exported)

    def test_additive_assignment_hint_keeps_legacy_catalog_operations(self):
        # Also run this contract against a retained pre-fix hgs binary: the hint
        # is organization metadata, not a new Patch field or operation kind.
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        desired['session_assignments_explicit'] = True
        desired['projects'].append(dict(id='ungrouped', name='Inbox', color='#123456', folders=[], sessions=[]))
        desired['projects'][0]['sessions'] = ['a\ncodex/example/review']
        saved = self.patch('a', before, desired)
        desired = copy.deepcopy(saved['organization']); desired['session_assignments_explicit'] = True
        for project in desired['projects']:
            project['sessions'] = ['a\ncodex/example/review'] if project['id'] == 'ungrouped' else []
        saved = self.patch('a', saved, desired)
        self.assertEqual(saved['conflicts'], [])
        self.assertEqual(next(p for p in saved['organization']['projects'] if p['id'] == 'ungrouped')['sessions'], ['a\ncodex/example/review'])
        exported = json.dumps(self.call('a', 'export'))
        self.assertNotIn('session_assignments_explicit', exported)
        self.assertNotIn('unassigned_sessions', exported)

    def test_explicit_initial_mapping_preserves_both_folder_locations_and_backup(self):
        self.join('a', 'b', {'a': 'b'})
        projects = self.call('a', 'get')['organization']['projects']
        self.assertEqual(len(projects), 1)
        self.assertEqual({f['path'] for f in projects[0]['folders']}, {'/a', '/b'})
        self.assertTrue(list((self.root / 'a/swarm').glob('backup-before-join-*.json')))

    def test_local_unc_folders_never_reach_the_catalog(self):
        # WhiteWh shares drive paths, but this desktop's network-share folders
        # remain local. Keep those distinct rather than undoing the fork feature.
        self.join('a', 'b')
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        project_a = next(p for p in desired['projects'] if p['id'] == 'a')
        project_a['name'] = 'Renamed with local shares'
        project_a['folders'] += [dict(id='unc', machine='a', path='//server/share/dir', name='Share'),
                                 dict(id='by-id', machine='desktop', machine_id=before['node_id'], path=r'\\server\share\private', name='')]
        local_shares = copy.deepcopy(desired)
        self.patch('a', before, desired); self.call('a', 'sync')
        for node in ('a', 'b'):
            projects = {p['id']: p for p in self.call(node, 'get')['organization']['projects']}
            self.assertEqual(projects['a']['name'], 'Renamed with local shares')
            self.assertEqual([f['path'] for f in projects['a']['folders']], ['/a'])
        exported = json.dumps(self.call('a', 'export'))
        self.assertNotIn('//server/share', exported)
        self.assertNotIn('private', exported)
        # Removing these local-only shares is still no catalog change.
        saved = self.call('a', 'get'); desired = copy.deepcopy(saved['organization'])
        self.patch('a', dict(saved, organization=local_shares), desired)
        self.assertEqual(saved['digest'], self.call('a', 'get')['digest'])
        for folder in [dict(id='relative', machine='a', path='work', name=''),
                       dict(id='drive-relative', machine='b', path='C:work', name=''),
                       dict(id='root-relative', machine='a', path=r'\work', name=''),
                       dict(id='invalid-new-machine', machine='windows-desktop', path='c:/bad'+chr(0), name='')]:
            failing = copy.deepcopy(desired)
            next(p for p in failing['projects'] if p['id'] == 'a')['folders'].append(folder)
            with self.subTest(folder=folder['id']):
                self.call('a', 'apply', data=dict(base=saved['organization'], desired=failing, versions=saved['versions']), ok=False)
        self.assertEqual(saved['digest'], self.call('a', 'get')['digest'])
        self.assertNotIn('windows-desktop', json.dumps(self.call('a', 'export')))

    def test_windows_drive_folders_replicate_with_exact_machine_identity(self):
        # Intentional WhiteWh contract (4b51c490): share descriptions, not files.
        self.join('a', 'b')
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        node_a, node_b = before['node_id'], self.call('b', 'get')['node_id']
        added = [dict(id='drive', machine='a', path=r'C:\work', name='Drive'),
                 dict(id='by-id', machine='desktop', machine_id=node_a, path='c:/Users/user/project', name=''),
                 dict(id='peer-drive', machine='b', path=r'D:\other', name='Peer drive')]
        next(p for p in desired['projects'] if p['id'] == 'a')['folders'].extend(added)
        self.patch('a', before, desired); self.call('a', 'sync')
        for node in ('a', 'b'):
            project_a = next(p for p in self.call(node, 'get')['organization']['projects'] if p['id'] == 'a')
            folders = {f['id']: f for f in project_a['folders']}
            for folder in added:
                self.assertEqual(folders[folder['id']]['path'], folder['path'])
                self.assertEqual(folders[folder['id']]['machine_id'], node_b if folder['id']=='peer-drive' else node_a)
            exported=json.dumps(self.call(node,'export'))
            self.assertNotIn('host-secret.invalid',exported)
            self.assertNotIn('/private/key',exported)
        saved = self.call('a','get'); desired = copy.deepcopy(saved['organization'])
        next(p for p in desired['projects'] if p['id']=='a')['folders'] = [dict(id='folder-a',machine='a',path='/a',name='')]
        self.patch('a',saved,desired); self.call('a','sync')
        for node in ('a','b'):
            project_a=next(p for p in self.call(node,'get')['organization']['projects'] if p['id']=='a')
            self.assertEqual([f['path'] for f in project_a['folders']],['/a'])

    def test_a_peers_unc_shaped_folder_syncs_as_a_posix_path(self):
        # A Windows GUI can store "//server/share" for a POSIX peer. The catalog
        # holds it like any folder, so renaming and removing it sync.
        self.join('a', 'b')
        def folders(node):
            return [f for p in self.call(node, 'get')['organization']['projects'] if p['id'] == 'a' for f in p['folders']]
        def edit(change):
            before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
            change(next(p for p in desired['projects'] if p['id'] == 'a')['folders'])
            self.patch('a', before, desired); self.call('a', 'sync')
        edit(lambda items: items.append(dict(id='unc', machine='b', path='//server/share/dir', name='')))
        node_b = self.call('b', 'get')['node_id']
        for node in ('a', 'b'):
            unc = next(f for f in folders(node) if f['id'] == 'unc')
            self.assertEqual((unc['path'], unc['machine_id']), ('//server/share/dir', node_b))
        edit(lambda items: next(f for f in items if f['id'] == 'unc').update(name='Share'))
        self.assertEqual(next(f for f in folders('b') if f['id'] == 'unc')['name'], 'Share')
        edit(lambda items: items.remove(next(f for f in items if f['id'] == 'unc')))
        for node in ('a', 'b'):
            self.assertEqual([f['path'] for f in folders(node)], ['/a'])

    def test_remote_payload_validation_keeps_store_unchanged(self):
        before = self.call('a', 'get')
        self.call('a', 'exchange', data=dict(swarm_id=before['swarm_id'], known=[], operations=[dict(id='bad')]), ok=False)
        self.assertEqual(before['digest'], self.call('a', 'get')['digest'])
        self.call('a', 'exchange', data=dict(swarm_id=self.call('b', 'get')['swarm_id'], known=[], operations=[]), ok=False)

    def test_removing_a_coalesced_folder_removes_observed_duplicates(self):
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        desired['projects'][0]['folders'][0].update(machine='b', machine_id='', path='/b')
        self.patch('a', before, desired)
        self.join('a', 'b', {'a': 'b'})
        before = self.call('a', 'get')
        self.assertEqual(len(before['organization']['projects'][0]['folders']), 1)
        desired = copy.deepcopy(before['organization']); desired['projects'][0]['folders'] = []
        self.patch('a', before, desired); self.call('a', 'sync')
        self.assertEqual(self.call('a', 'get')['organization']['projects'][0]['folders'], [])
        self.assertEqual(self.call('b', 'get')['organization']['projects'][0]['folders'], [])

    def test_move_and_copy_folder_links_replicate_without_moving_sessions(self):
        self.join('a', 'b')
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        source = next(p for p in desired['projects'] if p['id'] == 'a')
        target = next(p for p in desired['projects'] if p['id'] == 'b')
        source['sessions'] = ['a\ncodex/example/session']
        original = copy.deepcopy(source['folders'][0])
        session_addresses = {'a\ncodex/example/session', original['machine_id'] + '\ncodex/example/session'}
        duplicate = copy.deepcopy(original); duplicate['id'] = 'copied-folder'
        target['folders'].append(duplicate)
        destination = project('destination', 'Destination', 'a', '/unused')
        destination['folders'] = []; desired['projects'].append(destination)
        self.patch('a', before, desired); self.call('a', 'sync')
        for node in ('a', 'b'):
            groups = {p['id']: p for p in self.call(node, 'get')['organization']['projects']}
            self.assertEqual([f['id'] for f in groups['a']['folders']], [original['id']])
            copied = next(f for f in groups['b']['folders'] if f['id'] == 'copied-folder')
            self.assertEqual((copied['machine_id'], copied['path'], copied['name']),
                             (original['machine_id'], original['path'], original['name']))
            self.assertEqual(len(groups['a']['sessions']), 1)
            self.assertIn(groups['a']['sessions'][0], session_addresses)
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        groups = {p['id']: p for p in desired['projects']}
        groups['destination']['folders'] = groups['a']['folders']; groups['a']['folders'] = []
        self.patch('a', before, desired); self.call('a', 'sync')
        for node in ('a', 'b'):
            groups = {p['id']: p for p in self.call(node, 'get')['organization']['projects']}
            self.assertEqual(groups['a']['folders'], [])
            self.assertEqual([f['id'] for f in groups['destination']['folders']], [original['id']])
            self.assertEqual(len(groups['a']['sessions']), 1)
            self.assertIn(groups['a']['sessions'][0], session_addresses)
            self.assertEqual(len(groups['b']['folders']), 2)
        # Removing the moved link must leave its copied sibling intact on both nodes.
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        next(p for p in desired['projects'] if p['id'] == 'destination')['folders'] = []
        self.patch('a', before, desired); self.call('a', 'sync')
        for node in ('a', 'b'):
            groups = {p['id']: p for p in self.call(node, 'get')['organization']['projects']}
            self.assertEqual(groups['destination']['folders'], [])
            self.assertEqual({f['id'] for f in groups['b']['folders']}, {'folder-b', 'copied-folder'})

    def test_worker_syncs_without_a_gui_or_reverse_connection(self):
        self.join('a', 'b')
        self.assertEqual(self.call('b', 'get')['peers'], {})
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'a'), HGS_STATE_DIR=str(self.root / 'a/state'))
        worker = subprocess.Popen([str(HGS), 'swarm', 'worker'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
            next(p for p in desired['projects'] if p['id'] == 'a')['name'] = 'Changed with GUI closed'
            self.patch('a', before, desired)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                actual = self.call('b', 'get')
                if next(p for p in actual['organization']['projects'] if p['id'] == 'a')['name'] == 'Changed with GUI closed':
                    break
                time.sleep(.1)
            else:
                self.fail('headless worker did not replicate the change')
        finally:
            worker.terminate(); worker.communicate(timeout=5)

    def test_worker_is_reported_and_isolated_directories_never_restart_services(self):
        # Service-manager fakes record any attempt; hgs puts HOME/.local/bin first on PATH.
        home = self.root / 'home'
        (home / '.local/bin').mkdir(parents=True)
        for directory in (home / '.local/bin', self.root / 'bin'):
            for name in ('systemctl', 'launchctl'):
                fake = directory / name
                fake.write_text(f'#!/bin/sh\necho "{name} $*" >> "{self.root}/service-calls"\n'); fake.chmod(0o755)
        self.env['HOME'] = str(home)
        swarm = self.root / 'a/swarm'
        self.assertEqual(self.call('a', 'get')['worker'], {'running': False, 'current': False})
        with (swarm / 'worker.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            # A released worker holds the lock without recording its version.
            self.assertEqual(self.call('a', 'get')['worker'], {'running': True, 'current': False})
            self.call('a', 'preference', 'set', 'mobile_delivery', 'away_5', '--json')
            inventory = self.call('a', 'inventory')
            self.call('a', 'exchange', data={'swarm_id': inventory['swarm_id'], 'known': [], 'operations': []})
            (swarm / 'worker.json').write_text(json.dumps({'version': '0.0.1', 'pid': os.getpid(), 'started_at': 1}))
            self.assertEqual(self.call('a', 'get')['worker'], {'running': True, 'current': False})
        # HGS_CONFIG_DIR is not the directory the service uses.
        self.assertFalse((self.root / 'service-calls').exists())
        self.assertFalse((swarm / 'worker-restart.json').exists())
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'a'), HGS_STATE_DIR=str(self.root / 'a/state'))
        worker = subprocess.Popen([str(HGS), 'swarm', 'worker'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 10
            while self.call('a', 'get')['worker'] != {'running': True, 'current': True} and time.monotonic() < deadline:
                time.sleep(.1)
            self.assertEqual(self.call('a', 'get')['worker'], {'running': True, 'current': True})
            marker = json.loads((swarm / 'worker.json').read_text())
            version = subprocess.run([str(HGS), '--version'], env=env, text=True, capture_output=True, check=True).stdout.split()[1]
            self.assertEqual((marker['version'], marker['pid']), (version, worker.pid))
            self.assertLessEqual(abs(marker['started_at'] - time.time()), 60)
        finally:
            worker.terminate(); worker.communicate(timeout=5)
        self.assertEqual(self.call('a', 'get')['worker'], {'running': False, 'current': False})
        self.assertFalse((self.root / 'service-calls').exists())

    def test_task_feedback_forwards_its_original_identity_and_recovers_read_only(self):
        from tests.test_tasks import agent_record
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c'); self.converge()
        # Native input is deliberately unavailable on these synthetic nodes.
        # Prove forwarded persistence/refusal without addressing a real agent.
        tmux = self.root / 'bin/tmux'; tmux.write_text('#!/bin/sh\nexit 1\n'); tmux.chmod(0o755)
        state = self.root / 'b/state'; state.mkdir(exist_ok=True)
        run = str(uuid.uuid4()); name = 'codex/fixture-worker'
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root/'b'), HGS_STATE_DIR=str(state), HGS_SESSION=name, HGS_RUN_ID=run)
        agent_record(state, env, name, run)
        task = self.task('b','add','--project','b','--target',name,text='Remote feedback contract')['task']
        started = subprocess.run([str(HGS),'task','start',task['uuid'],'--project','b','--json'],env=env,text=True,capture_output=True)
        self.assertEqual(started.returncode,0,started.stderr)
        task = json.loads(started.stdout)['task']; self.converge()
        identity = str(uuid.uuid4()); content = json.dumps(dict(text='Feedback across computers',attachments=[]))
        args = ['message',task['uuid'],'--project','b','--request-id',identity,'--mode','send','--expect-dispatch',task['dispatch_id'],'--content-json']
        reply = self.task('a',*args,text=content)
        self.assertEqual(reply['message']['request_id'],identity)
        self.assertEqual(reply['message']['delivery']['state'],'failed')
        checked = self.task('a',*args,'--recover-only',text=content)
        self.assertEqual(checked['message']['delivery']['state'],'failed')
        original = self.task('b','get',task['uuid'],'--project','b')['task']
        messages = [e for e in original['log'] if e['kind']=='message']
        self.assertEqual(len(messages),1); self.assertEqual(messages[0]['text'],'Feedback across computers')

    def test_tasks_replicate_after_the_catalog(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        first = self.task('a', 'add', '--project', 'a', text='Add rate limiting')['task']
        self.task('a', 'add', '--project', 'a', text='Document the setup')
        # b fetches them from a, and c from b.
        self.call('b', 'sync'); self.call('c', 'sync')
        for node in 'bc':
            self.assertEqual(self.tasks(node), ['Add rate limiting', 'Document the setup'], node)
        # Changes made on two members at once merge everywhere.
        self.task('c', 'comment', first['id'], '--project', 'a', text='Use the config value.')
        self.task('a', 'stage', first['id'], 'backlog', '--project', 'a')
        self.converge()
        for node in 'abc':
            view = self.task(node, 'get', first['id'], '--project', 'a')['task']
            self.assertEqual(view['stage'], 'backlog', node)
            self.assertIn('Use the config value.', [entry.get('text') for entry in view['log']], node)
        hellos = [self.call(node, 'hello')['replica'] for node in 'abc']
        self.assertEqual(len({json.dumps(h['projects'], sort_keys=True) for h in hellos}), 1)
        self.assertEqual({(h['protocol'], h['schema']) for h in hellos}, {(1, 1)})
        # Equal digests: a sync exchanges no project.
        before = self.exchanges(); self.call('a', 'sync'); self.call('b', 'sync')
        self.assertEqual(self.exchanges(), before)

    def test_the_cutover_status_names_each_member_s_replica(self):
        # zerus-cg5h.4 S2b.2 C2: what each member's hello and last exchange
        # said, information only.
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        task = self.task('a', 'add', '--project', 'a', text='Add rate limiting')['task']
        self.converge()
        hello = self.call('b', 'hello')['replica']
        self.assertEqual(hello['task_schema'], 7)
        self.assertRegex(hello['store'], r'^[0-9a-f-]{36}$')
        self.assertEqual(self.call('b', 'hello')['replica']['store'], hello['store'])
        status = self.task('b', 'cutover', '--project', 'a')
        members = {m['alias']: m for m in status['members']}
        self.assertEqual(set(members), {'a', 'c'})
        for alias in 'ac':
            self.assertEqual(members[alias]['task_schema'], 7, alias)
            self.assertRegex(members[alias]['store'], r'^[0-9a-f-]{36}$')
            self.assertEqual(members[alias]['inventory']['complete'], True, alias)
        self.assertNotEqual(members['a']['store'], members['c']['store'])
        self.assertEqual((status['phase'], status['closed']), ('not_cut_over', None))
        # A cutover written here after the members' last exchange keeps it
        # open until they exchange again.
        tasks = next(p for p in (self.root / 'b/state/replica').iterdir() if p.is_dir()) / 'tasks'
        (tasks / (task['uuid'] + '.managed')).write_text('{"saved":{}}')
        status = self.task('b', 'cutover', '--project', 'a')
        self.assertEqual(status['phase'], 'cutting_over')
        self.assertEqual({k: status['closed'][k] for k in ('member', 'project', 'reason')},
                         {'member': 'a', 'project': 'a', 'reason': 'inventory_before_cutover'})
        self.assertIn("predates the latest cutover", status['closed_text'])
        # A member whose Zerus does not replicate tasks receives none: not eligible.
        (self.root / 'noreplica-c').touch()
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual({m['alias'] for m in self.task('b', 'cutover', '--project', 'a')['members']}, {'a'})

    def fence_check(self, node, task):
        return self.task(node, 'cutover', '--project', 'a', '--task', task, '--check')

    def test_the_fence_check_covers_every_writer_over_verified_routes(self):
        # zerus-cg5h.4 S2b.2 C3c1 (review-n's A-B-C chain): a has only b as a
        # peer, and c holds a's tasks through b. The writer universe covers c
        # too, reached through b's relay and verified by its own hello.
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        task = self.task('a', 'add', '--project', 'a', text='Add rate limiting')['task']
        ids = {n: self.call(n, 'hello')['node_id'] for n in 'abc'}
        before = self.fence_check('a', task['id'])
        self.assertEqual(set(before['universe']['nodes']), {ids['b']})
        # c joins through b: it is covered, and the incarnation changes.
        self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        self.assertEqual(sorted(self.call('b', 'hello')['replica']['peers']), sorted([ids['a'], ids['c']]))
        self.assertEqual(self.call('b', 'hello')['replica']['fence'], 1)
        check = self.fence_check('a', task['id'])
        nodes = check['universe']['nodes']
        self.assertEqual(set(nodes), {ids['b'], ids['c']}, check)
        self.assertEqual(nodes[ids['b']]['route'], [ids['b']])
        self.assertEqual(nodes[ids['c']]['route'], [ids['b'], ids['c']])
        for node in nodes.values():
            self.assertEqual((node['protocol'], node['task_schema'], node['fence'], node['closed']),
                             (1, 7, 1, None), node)
            self.assertRegex(node['store'], r'^[0-9a-f-]{36}$')
        self.assertEqual(nodes[ids['c']]['store'], self.call('c', 'hello')['replica']['store'])
        self.assertEqual((check['universe_closed'], check['proof'], check['task']), (None, None, task['uuid']))
        # An unfloored task has no proof: the fence closes before any exchange.
        self.assertEqual(check['closed'], {'node': None, 'reason': 'no_floor'})
        self.assertNotEqual(check['incarnation'], before['incarnation'])
        self.assertEqual(check['universe']['me']['node'], ids['a'])
        # A second read of an unchanged universe has the same incarnation.
        self.assertEqual(self.fence_check('a', task['id'])['incarnation'], check['incarnation'])
        # A store replaced behind the relay changes it (membership ABA).
        (self.root / 'c/state/replica/store-id').unlink()
        changed = self.fence_check('a', task['id'])
        self.assertNotEqual(changed['incarnation'], check['incarnation'])
        self.assertEqual(changed['universe_closed'], None)
        # A node answering as another computer behind the relay is unverified.
        (self.root / 'spoil-c').write_text(json.dumps({'command': 'hello', 'field': 'node_id',
                                                        'value': str(uuid.UUID(int=3))}))
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed'],
                         {'node': ids['c'], 'reason': 'unverified'})
        (self.root / 'spoil-c').unlink()
        # A hop that answers the relay from another store, or names none,
        # breaks the route's binding to the store its hello showed.
        for value in [{'value': str(uuid.UUID(int=5))}, {}]:
            (self.root / 'spoil-b').write_text(json.dumps({'command': 'replica-relay', 'field': 'store', **value}))
            self.assertEqual(self.fence_check('a', task['id'])['universe_closed'],
                             {'node': ids['c'], 'reason': 'unverified'}, value)
        (self.root / 'spoil-b').unlink()
        # The same for a direct peer, which only this computer checks.
        (self.root / 'spoil-b').write_text(json.dumps({'command': 'hello', 'field': 'node_id',
                                                        'value': str(uuid.UUID(int=4))}))
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed'],
                         {'node': ids['b'], 'reason': 'unverified'})
        (self.root / 'spoil-b').unlink()
        # A node that does not answer keeps the fence closed.
        (self.root / 'offline-c').touch()
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed'],
                         {'node': ids['c'], 'reason': 'unreachable'})
        (self.root / 'offline-c').unlink()
        # A hop without task replication excludes only itself: the node
        # behind it, still a catalog machine, has no route and closes it.
        (self.root / 'noreplica-b').touch()
        check = self.fence_check('a', task['id'])
        nodes = check['universe']['nodes']
        self.assertEqual((nodes[ids['b']]['no_replica'], nodes[ids['b']]['closed']), (True, None))
        self.assertEqual((nodes[ids['c']]['route'], nodes[ids['c']]['closed']), ([], 'unroutable'))
        self.assertEqual(check['universe_closed'], {'node': ids['c'], 'reason': 'unroutable'})
        (self.root / 'noreplica-b').unlink()
        # The check runs only with --check, and changes no task.
        self.assertEqual(self.task('a', 'get', task['id'], '--project', 'a')['task']['stage'], task['stage'])

    def test_the_fence_check_covers_unreadable_incoming_and_re_enrolled_writers(self):
        # C3c1 R2 (desktop-u and review-n on #1452).
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        task = self.task('a', 'add', '--project', 'a', text='Add rate limiting')['task']
        ids = {n: self.call(n, 'hello')['node_id'] for n in 'abc'}
        baseline = self.fence_check('a', task['id'])
        self.assertEqual(baseline['universe_closed'], None, baseline)
        # A member whose replica cannot be read stays a writer and closes it.
        replica = self.root / 'b/state/replica'
        replica.rename(self.root / 'b-replica')
        replica.write_text('not a directory')
        check = self.fence_check('a', task['id'])
        node = check['universe']['nodes'][ids['b']]
        self.assertEqual((node['no_replica'], node['closed']), (False, 'unavailable'), node)
        self.assertEqual(check['universe_closed'], {'node': ids['b'], 'reason': 'unavailable'})
        replica.unlink(); (self.root / 'b-replica').rename(replica)
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed'], None)
        # A writer that only joined b (b never enrolled it) holds a's tasks
        # through b; b's catalog knows it, so a covers it: no route closes it.
        self.join('c', 'b')
        self.call('c', 'sync')
        self.assertNotIn(ids['c'], self.call('b', 'hello')['replica']['peers'])
        self.assertIn(ids['c'], self.call('b', 'hello')['replica']['writers'])
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed'], {'node': ids['c'], 'reason': 'unroutable'})
        self.join('b', 'c')
        self.converge()
        reached = self.fence_check('a', task['id'])
        self.assertEqual(reached['universe_closed'], None, reached)
        # Membership ABA: b disconnected and enrolled again, with its node and
        # store unchanged, is another incarnation.
        before = reached['incarnation']
        self.call('a', 'disconnect', 'b')
        self.assertEqual(self.fence_check('a', task['id'])['universe_closed']['reason'], 'unroutable')
        self.join('a', 'b')
        again = self.fence_check('a', task['id'])
        self.assertEqual(again['universe_closed'], None, again)
        self.assertNotEqual(again['incarnation'], before)
        self.assertNotEqual(again['universe']['me']['membership'], reached['universe']['me']['membership'])

    def test_replica_task_answers_from_its_reread_and_its_own_store(self):
        # C3c2: the fence's task exchange over the real CLI, directly and
        # through a relay; it grants nothing and writes no floor.
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        task = self.task('a', 'add', '--project', 'a', text='Add rate limiting')['task']
        self.converge()
        hello = {n: self.call(n, 'hello') for n in 'abc'}
        swarm = hello['a']['swarm_id']
        request = dict(swarm_id=swarm, project='a', task=task['uuid'], version={}, updates=[],
                       store=hello['b']['replica']['store'], remaining_ms=5000)
        reply = self.call('b', 'replica-task', data=request)
        self.assertEqual((reply['node_id'], reply['store'], reply['task']),
                         (hello['b']['node_id'], request['store'], task['uuid']))
        self.assertEqual((reply['loads'], reply['applied'], reply['floor']), (True, True, False), reply)
        self.assertIsInstance(reply['stored_schema'], int)
        self.assertTrue(reply['version'] and all(k.isdigit() and v > 0 for k, v in reply['version'].items()), reply)
        self.assertEqual(len(reply['updates']), 1)
        # Its own version sent back: the same version answers.
        self.assertEqual(self.call('b', 'replica-task', data=dict(request, version=reply['version']))['version'],
                         reply['version'])
        # Asked as another store, it imports nothing and names its own.
        other = self.call('b', 'replica-task', data=dict(request, store=str(uuid.UUID(int=7))))
        self.assertEqual((other['store'], 'version' in other), (request['store'], False))
        # A version not in its canonical form is refused.
        self.call('b', 'replica-task', data=dict(request, version={'012': 1}), ok=False)
        # Through b's relay to c, bound to c's store.
        relay = dict(swarm_id=swarm, to=hello['c']['node_id'], command='replica-task', hops=0, remaining_ms=5000,
                     input=dict(request, store=hello['c']['replica']['store']))
        relayed = self.call('b', 'replica-relay', data=relay)
        self.assertEqual((relayed['to'], relayed['reply']['store'], relayed['reply']['loads']),
                         (hello['c']['node_id'], hello['c']['replica']['store'], True))
        # The check closes before any exchange for a task that never cut over,
        # and reads, never writes, the task's certificate (C3e).
        check = self.fence_check('a', task['id'])
        self.assertEqual(check['closed'], {'node': None, 'reason': 'no_floor'})
        self.assertEqual(check['certificate'], {'valid': False, 'reason': 'no_certificate'})
        # review-win-c on #1471: a FIFO at the certificate's path or the
        # member records' is refused, never waited on past the check's budget.
        fence = self.root / 'a/state/replica' / hashlib.sha256(b'a').hexdigest() / 'fence'
        fence.mkdir(exist_ok=True)
        os.mkfifo(fence / f"{task['uuid']}.json")
        members = self.root / 'a/swarm/replica-members.json'
        members.unlink(missing_ok=True)
        os.mkfifo(members)
        started = time.monotonic()
        check = self.fence_check('a', task['id'])
        self.assertLess(time.monotonic() - started, 15)
        self.assertEqual(check['certificate'], {'valid': False, 'reason': 'certificate_not_regular'})

    def test_a_relay_never_answers_after_its_budget(self):
        # review-n on #1452: an answer whose pipe a descendant keeps open past
        # the relay's budget is never accepted after it.
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        ids = {n: self.call(n, 'hello')['node_id'] for n in 'abc'}
        swarm = self.call('b', 'hello')['swarm_id']
        relay = dict(swarm_id=swarm, to=ids['c'], command='hello', input={}, hops=0, remaining_ms=800)
        self.assertEqual(self.call('b', 'replica-relay', data=relay)['reply']['node_id'], ids['c'])
        (self.root / 'linger-c').write_text('2.5')
        started = time.monotonic()
        error = self.call('b', 'replica-relay', data=relay, ok=False)
        elapsed = time.monotonic() - started
        (self.root / 'linger-c').unlink()
        self.assertLess(elapsed, 2.0, error)
        self.assertIn('timed out', error)

    def test_a_member_whose_hello_fails_has_no_complete_project(self):
        # review-n on #1432: a failed hello records every project known here
        # as unreachable for that member, never leaving an old completion.
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        self.task('a', 'add', '--project', 'a', text='Offline outcome fixture')
        for n in 'abab':
            self.call(n, 'sync')
        path = self.root / 'b/swarm/replica-members.json'
        before = json.loads(path.read_text())['a']['projects']['a']
        self.assertTrue(before['complete'])
        (self.root / 'offline-a').touch()
        self.assertFalse(self.call('b', 'sync')['a']['ok'])
        after = json.loads(path.read_text())['a']['projects']['a']
        self.assertEqual((after['complete'], after.get('reason')), (False, 'unreachable'))
        (self.root / 'offline-a').unlink()
        self.call('b', 'sync')
        self.assertTrue(json.loads(path.read_text())['a']['projects']['a']['complete'])

    def damage_between_inventory_and_exchange(self, owner):
        """review-n on #1432 R2: a task of `owner` (b remote, a local) is
        damaged after a healthy inventory and just before the exchange: the
        exchange is not complete."""
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        task = self.task(owner, 'add', '--project', 'a', text='Damage between inventory and exchange')
        project = next(p for p in (self.root / owner / 'state/replica').iterdir()
                       if p.is_dir() and (p / 'project-id').read_text().strip() == 'a')
        target = project / 'tasks' / (task['task']['uuid'] + '.loro')
        self.assertTrue(target.is_file())
        flag = self.root / 'damage-before-exchange-b'
        flag.write_text(str(target))
        ssh = self.root / 'bin/ssh'
        script = ssh.read_text()
        needle = "if not (root / ('old-' + alias)).exists():"
        hook = ("if args[1:2] == ['replica-exchange'] and (root / ('damage-before-exchange-' + alias)).exists():\n"
                "    trigger = root / ('damage-before-exchange-' + alias)\n"
                "    pathlib.Path(trigger.read_text()).write_bytes(b'damaged after a healthy inventory')\n"
                "    trigger.unlink()\n")
        self.assertIn(needle, script)
        ssh.write_text(script.replace(needle, hook + needle, 1))
        self.call('a', 'sync')
        self.assertFalse(flag.exists(), 'the fixture did not reach the exchange')
        record = json.loads((self.root / 'a/swarm/replica-members.json').read_text())['b']['projects']['a']
        damaged = self.task(owner, 'list', '--project', 'a')['damaged']
        self.assertEqual(len(damaged), 1)
        self.assertFalse(record['complete'], record)
        self.assertEqual(record.get('reason'), 'damaged_tasks', record)

    def test_absent_or_unreadable_exchange_evidence_is_never_complete(self):
        # review-n on #1432 R3: a replica answer that leaves out, or garbles,
        # its refusals, import count, damaged count or pages records the
        # project as malformed; the untouched answer is the control.
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        spoils = [None,
                  ('replica-exchange', 'refused'), ('replica-exchange', 'refused', None),
                  ('replica-exchange', 'refused', {}), ('replica-exchange', 'refused', 'none'),
                  ('replica-exchange', 'refused', [{'uuid': 1}]),
                  ('replica-exchange', 'imported'), ('replica-exchange', 'imported', '1'),
                  # zerus-tfqd6: more than the page carried, up to the largest
                  # unsigned count, is unknown and never added up.
                  ('replica-exchange', 'imported', 1), ('replica-exchange', 'imported', 2 ** 64 - 1),
                  ('replica-exchange', 'refused', [{'uuid': str(uuid.UUID(int=1)), 'error': 'not sent'}]),
                  ('replica-exchange', 'damaged'), ('replica-exchange', 'damaged', -1),
                  ('replica-exchange', 'damaged', '0'), ('replica-exchange', 'more'),
                  ('replica-exchange', 'more', 'false'), ('replica-exchange', 'updates', 'none'),
                  ('replica-inventory', 'damaged'), ('replica-inventory', 'damaged', 0.5),
                  # zerus-cg5h.4.5: a reply that names no store that answered.
                  ('replica-exchange', 'store'), ('replica-exchange', 'store', 'not a store'),
                  ('replica-inventory', 'store'), ('replica-inventory', 'store', None),
                  ('replica-inventory', 'inventory', [])]
        path = self.root / 'a/swarm/replica-members.json'
        flag = self.root / 'spoil-b'
        for n, spoil in enumerate(spoils):
            with self.subTest(spoil=spoil):
                # A task added there makes the digests differ, so the pages are exchanged.
                self.task('b', 'add', '--project', 'a', text=f'Exchange evidence fixture {n}')
                if spoil is None:
                    flag.unlink(missing_ok=True)
                else:
                    flag.write_text(json.dumps(dict(zip(('command', 'field', 'value'), spoil))))
                command = 'b swarm ' + ('replica-exchange' if spoil is None else spoil[0])
                before = (self.root / 'calls').read_text().count(command)
                self.call('a', 'sync')
                self.assertGreater((self.root / 'calls').read_text().count(command), before,
                                   'the sync never reached the answer')
                record = json.loads(path.read_text())['b']['projects']['a']
                if spoil is None:
                    self.assertTrue(record['complete'], record)
                else:
                    self.assertFalse(record['complete'], record)
                    self.assertEqual(record.get('reason'), 'malformed', record)
        flag.unlink(missing_ok=True)
        self.task('b', 'add', '--project', 'a', text='Exchange evidence fixture, healthy again')
        self.call('a', 'sync')
        self.assertTrue(json.loads(path.read_text())['b']['projects']['a']['complete'])

    def test_a_store_replaced_after_its_hello_is_a_changed_identity(self):
        # zerus-cg5h.4.5: each reply is bound to the store the hello showed.
        # A replica root replaced between the hello and the exchange, or an
        # exchange answered by another store, is never complete; the next
        # hello observes the new store and completes again.
        self.join('a', 'b'); self.join('b', 'a')
        for n in 'abab':
            self.call(n, 'sync')
        path = self.root / 'a/swarm/replica-members.json'
        store_id = self.root / 'b/state/replica/store-id'
        before = store_id.read_text().strip()
        self.task('b', 'add', '--project', 'a', text='Store binding fixture')
        flag = self.root / 'reset-store-b'
        flag.touch()
        self.call('a', 'sync')
        self.assertFalse(flag.exists(), 'the fixture did not reach the inventory')
        self.assertNotEqual(store_id.read_text().strip(), before)
        record = json.loads(path.read_text())['b']
        self.assertEqual(record['store'], before, record)
        self.assertFalse(record['projects']['a']['complete'], record)
        self.assertEqual(record['projects']['a'].get('reason'), 'changed_identity', record)
        self.call('a', 'sync')
        record = json.loads(path.read_text())['b']
        self.assertEqual(record['store'], store_id.read_text().strip())
        self.assertTrue(record['projects']['a']['complete'], record)
        # Another store answering the exchange alone does the same.
        self.task('b', 'add', '--project', 'a', text='Store binding fixture, exchange')
        spoil = self.root / 'spoil-b'
        spoil.write_text(json.dumps({'command': 'replica-exchange', 'field': 'store',
                                     'value': str(uuid.UUID(int=2))}))
        self.call('a', 'sync')
        spoil.unlink()
        record = json.loads(path.read_text())['b']['projects']['a']
        self.assertEqual((record['complete'], record.get('reason')), (False, 'changed_identity'), record)

    def test_a_task_damaged_there_after_its_inventory_is_not_complete(self):
        self.damage_between_inventory_and_exchange('b')

    def test_a_task_damaged_here_after_its_inventory_is_not_complete(self):
        self.damage_between_inventory_and_exchange('a')

    def test_a_large_project_travels_in_pages(self):
        self.join('a', 'b'); self.join('b', 'a')
        self.call('a', 'sync'); self.call('b', 'sync')
        for n in range(20):
            self.task('a', 'add', '--project', 'a', text=noise(n, 60000))
        before = self.exchanges()
        self.assertTrue(self.call('b', 'sync')['a']['ok'])
        self.assertEqual(len(self.tasks('b')), 20)
        self.assertGreaterEqual(self.exchanges() - before, 3)

    def test_members_without_task_replication_are_skipped_and_named(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        self.task('b', 'add', '--project', 'a', text='Add rate limiting')
        (self.root / 'noreplica-c').touch()  # c now answers like a release without task replication
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual(self.tasks('c'), [])
        peers = self.call('b', 'get')['peers']
        self.assertTrue(peers['c']['tasks_outdated'])
        self.assertNotIn('tasks_outdated', peers['a'])
        # After the update it receives the tasks and the notice clears.
        (self.root / 'noreplica-c').unlink()
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual(self.tasks('c'), ['Add rate limiting'])
        self.assertNotIn('tasks_outdated', self.call('b', 'get')['peers']['c'])

    def test_primary_id_grouping_survives_a_label_change_and_a_peer_round_trip(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c'); self.converge()
        original = self.call('a', 'get')
        machine, swarm = original['node_id'], original['swarm_id']
        ids = [str(uuid.uuid4()), str(uuid.uuid4())]
        state = self.root / 'a/state'
        (state / 'sessions').mkdir(parents=True, exist_ok=True)
        name = 'claude/example/shared'
        # Name layout A and ID layout B coexist. The ID index only routes; the
        # production reader still verifies the exact record/run/conversation.
        for index, session in enumerate(ids):
            record = dict(version=1, agent='claude', name=name, session_id=session,
                          run_id=f'run-{index}', conversation_id=f'conversation-{index}', state='paused')
            if index: record['layout'] = 'id'
            filename = session if index else hashlib.sha256(name.encode()).hexdigest()
            ((state / 'sessions' if index else state) / (filename + '.json')).write_text(json.dumps(record))
            (state / 'ids').mkdir(exist_ok=True)
            (state / 'ids' / (session + '.json')).write_text(json.dumps(dict(version=1, name=name, kind='id' if index else 'record')))
        def project(session, index, selected=None):
            request = dict(request_id=str(uuid.uuid4()), name=name, session_id=session,
                expected_run_id=f'run-{index}', expected_conversation_id=f'conversation-{index}', expected_machine_id=machine)
            if selected is None:
                return self.call('a', 'session-project', '--json', data=request)
            current = self.call('a', 'session-project', '--json', data=request)
            self.assertEqual(current['status'], 'ok', current)
            request.update(swarm_id=swarm, membership_token=current['membership_token'],
                           catalog_epoch=current['catalog_epoch'], project_id=selected)
            result = self.call('a', 'assign-session', '--json', data=request)
            self.assertEqual(result['status'], 'assigned', result)
            self.assertEqual(self.call('a', 'assign-session', '--json', data=request), result)
        project(ids[0], 0, 'a'); project(ids[1], 1, 'b')
        self.assertEqual(project(ids[0], 0)['current_project_id'], 'a')
        self.assertEqual(project(ids[1], 1)['current_project_id'], 'b')
        # A real saved-session rename uses only an isolated tmux socket.
        # No server or native agent is started for this metadata fixture.
        import shlex
        tmux = shutil.which('tmux')
        self.assertIsNotNone(tmux)
        socket_dir = self.root / 'private-sockets'; socket_dir.mkdir(mode=0o700)
        wrapper = self.root / 'bin/tmux'
        wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(tmux) + ' -S ' + shlex.quote(str(socket_dir / 'test.sock')) + ' -f /dev/null "$@"\n')
        wrapper.chmod(0o700)
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'a'), HGS_STATE_DIR=str(state))
        renamed = 'claude/example/renamed'
        result = subprocess.run([str(HGS), 'rename', name, '--session-id', ids[1], renamed], env=env, text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        name = renamed
        self.assertEqual(project(ids[1], 1)['current_project_id'], 'b')
        self.assertTrue(self.call('b', 'sync')['a']['ok'])
        self.assertTrue(self.call('a', 'sync')['b']['ok'])
        for node in 'ab':
            records = self.call(node, 'get')['session_id_memberships']['records']
            self.assertEqual({r['session_id']: r['project'] for r in records}, dict(zip(ids, ['a', 'b'])))
            catalog = json.loads((self.root / node / 'swarm/catalog.json').read_text())
            self.assertFalse(any(op['key']['session'] in ids for op in catalog['catalog']['operations'].values()
                                 if op['key']['kind'] == 'session'))

    def test_primary_id_operations_are_not_sent_to_an_old_peer(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c'); self.converge()
        original = self.call('a', 'get')
        session = str(uuid.uuid4()); opid = str(uuid.uuid4())
        graph = dict(schema=1, swarm_id=original['swarm_id'], operations={opid: dict(
            id=opid, actor=original['node_id'], clock=1, parents=[],
            key=dict(kind='session', machine=original['node_id'], session=session), value='a')})
        path = self.root / 'a/swarm/session-id-memberships.json'; path.write_text(json.dumps(graph))
        (self.root / 'nosessionid-b').touch()
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        desired['projects'][0]['name'] += ' update'
        self.patch('a', before, desired)
        self.assertTrue(self.call('a', 'sync')['b']['ok'])
        self.assertEqual(self.call('b', 'get')['session_id_memberships']['records'], [])
        (self.root / 'nosessionid-b').unlink()
        self.assertTrue(self.call('a', 'sync')['b']['ok'])
        self.assertEqual(self.call('b', 'get')['session_id_memberships']['records'][0]['session_id'], session)

    def test_membership_by_id_waits_for_every_member_to_list_the_kind(self):
        # zerus-ucck P2c-1: a desktop may read membership by ID only while
        # every machine of the catalog lists the session_id kind.
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        gate = lambda: self.call('b', 'get')['membership_by_id']
        self.assertEqual(gate(), {'ready': True, 'missing': []})
        (self.root / 'nosessionid-c').touch()  # c now answers like a release before the kind
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        c = self.call('c', 'get')['node_id']
        self.assertEqual(gate(), {'ready': False, 'missing': [c]})
        # After the update the gate opens again.
        (self.root / 'nosessionid-c').unlink()
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual(gate(), {'ready': True, 'missing': []})

    def test_twins_travel_only_between_members_that_list_the_kind(self):
        # zerus-ucck P2c-1 (#1413 review): session ID memberships live beside
        # the catalog and reach only members that list the kind.
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        (self.root / 'nosessionid-c').touch()
        a = self.call('a', 'get')
        session = str(uuid.uuid4())
        twin = {'id': str(uuid.uuid4()), 'actor': a['node_id'], 'clock': 1, 'machine': a['node_id'], 'session_id': session,
                'label': 'codex/work/one', 'op': str(uuid.uuid4()), 'project': None}
        twins = lambda node: self.root / node / 'swarm' / 'session-id-twins.json'
        twins('a').write_text(json.dumps({'swarm_id': a['swarm_id'], 'entries': {a['node_id'] + '\n' + session: twin}}))
        self.assertTrue(self.call('b', 'sync')['a']['ok'])
        self.assertEqual(json.loads(twins('b').read_text())['entries'][a['node_id'] + '\n' + session], twin)
        # #1413 review (N5): a change on b makes the digests differ, so b and c
        # really exchange; c, which refuses twin fields, must still sync.
        before = self.call('b', 'get')
        desired = copy.deepcopy(before['organization'])
        desired['projects'][0]['name'] += ' renamed'
        self.patch('b', before, desired)
        exchanges = lambda: sum(line.startswith('c ') and 'swarm exchange' in line
                                for line in (self.root / 'calls').read_text().splitlines())
        sent = exchanges()
        self.assertTrue(self.call('b', 'sync')['c']['ok'], 'a member without the kind is never sent one')
        self.assertGreater(exchanges(), sent, 'b and c exchanged')
        self.assertFalse(twins('c').exists())
        for node in 'abc':
            catalog = (self.root / node / 'swarm' / 'catalog.json').read_text()
            self.assertNotIn('session_id', catalog)
        # A requester that names its kinds gets only those; one that names none
        # (v0.47.0 and older) gets only the kinds v0.47.0 parses.
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'b'), HGS_STATE_DIR=str(self.root / 'b' / 'state'))
        exported = subprocess.run([str(HGS), 'swarm', 'export', '--kinds'], input=json.dumps({'kinds': ['project', 'session']}),
                                  env=env, text=True, capture_output=True, timeout=40)
        self.assertEqual(exported.returncode, 0, exported.stderr)
        kinds = {op['key']['kind'] for op in json.loads(exported.stdout)['catalog']['operations'].values()}
        self.assertLessEqual(kinds, {'project', 'session'})
        self.assertIn('project', kinds)

    def test_the_worker_sends_a_new_task_within_seconds(self):
        self.join('a', 'b'); self.join('b', 'a')
        self.call('a', 'sync'); self.call('b', 'sync')
        env = dict(self.env, HGS_CONFIG_DIR=str(self.root / 'a'), HGS_STATE_DIR=str(self.root / 'a/state'))
        worker = subprocess.Popen([str(HGS), 'swarm', 'worker'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            # Its first round is over; the next scheduled one is 30 s away.
            synced = self.call('a', 'get')['peers']['b']['last_sync']
            deadline = time.monotonic() + 10
            while self.call('a', 'get')['peers']['b']['last_sync'] == synced and time.monotonic() < deadline:
                time.sleep(.1)
            self.task('a', 'add', '--project', 'a', text='Add rate limiting')
            deadline = time.monotonic() + 10
            while self.tasks('b') != ['Add rate limiting'] and time.monotonic() < deadline:
                time.sleep(.2)
            self.assertEqual(self.tasks('b'), ['Add rate limiting'])
        finally:
            worker.terminate(); worker.communicate(timeout=5)

    def test_a_task_store_that_cannot_be_read_never_stops_the_catalog(self):
        self.join('a', 'b'); self.join('b', 'a')
        self.call('a', 'sync'); self.call('b', 'sync')
        self.task('a', 'add', '--project', 'a', text='Add rate limiting')
        # A directory of a's replicas that names no valid project.
        broken = self.root / 'a/state/replica' / ('0' * 64)
        broken.mkdir(parents=True); (broken / 'project-id').write_text('')
        before = self.call('a', 'get'); desired = copy.deepcopy(before['organization'])
        next(p for p in desired['projects'] if p['id'] == 'a')['name'] = 'Renamed beside a broken store'
        self.patch('a', before, desired)
        self.assertEqual(set(self.call('a', 'hello')['replica']['projects']), {'a'})
        self.assertTrue(self.call('b', 'sync')['a']['ok'])
        names = [p['name'] for p in self.call('b', 'get')['organization']['projects'] if p['id'] == 'a']
        self.assertEqual(names, ['Renamed beside a broken store'])
        self.assertEqual(self.tasks('b'), ['Add rate limiting'])

    def test_refused_tasks_neither_stall_an_exchange_nor_stop_other_projects(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        # a holds more than a page of project b's tasks under project a too; b refuses them there.
        for n in range(20):
            self.task('a', 'add', '--project', 'b', text=noise(n, 60000))
        replica = self.root / 'a/state/replica'
        project_a = replica / hashlib.sha256(b'a').hexdigest()
        (project_a / 'tasks').mkdir(parents=True, exist_ok=True)
        (project_a / 'project-id').write_text('a')
        for file in (replica / hashlib.sha256(b'b').hexdigest() / 'tasks').iterdir():
            shutil.copy(file, project_a / 'tasks' / file.name)
        self.task('a', 'add', '--project', 'c', text='Still replicated')
        before = self.exchanges('a ')
        self.assertTrue(self.call('b', 'sync')['a']['ok'])
        # Project b's pages arrive; the refused page of project a is not asked for again and again.
        self.assertLessEqual(self.exchanges('a ') - before, 6)
        self.assertEqual(len(self.task('b', 'list', '--project', 'b')['tasks']), 20)
        self.assertEqual([t['text'] for t in self.task('b', 'list', '--project', 'c')['tasks']], ['Still replicated'])

    def test_replica_requests_name_their_swarm_and_known_projects(self):
        self.join('a', 'b'); self.join('b', 'a')
        self.call('a', 'sync')
        self.task('a', 'add', '--project', 'a', text='Add rate limiting')
        hello = self.call('a', 'hello')
        swarm = hello['swarm_id']
        # Answers name their computer; a later hgs may add fields to a request.
        reply = self.call('a', 'replica-inventory', data={'swarm_id': swarm, 'project': 'a', 'stage': 'live'})
        self.assertEqual(reply['node_id'], hello['node_id'])
        self.assertEqual(len(reply['inventory']['tasks']), 1)
        # A request from another swarm, or for a project nobody here knows, is refused.
        self.assertIn('another swarm', self.call('a', 'replica-inventory', data={'swarm_id': 'elsewhere', 'project': 'a'}, ok=False))
        self.assertIn('swarm_id', self.call('a', 'replica-inventory', data={'project': 'a'}, ok=False))
        replica = self.root / 'a/state/replica'
        before = sorted(p.name for p in replica.iterdir())
        self.assertIn('no project', self.call('a', 'replica-exchange', data={'swarm_id': swarm, 'project': 'nowhere', 'inventory': {'project': {}, 'tasks': {}}}, ok=False))
        self.assertEqual(sorted(p.name for p in replica.iterdir()), before)
        # Only hello reads the task digests.
        self.assertNotIn('replica', self.call('a', 'inventory'))

    def test_a_held_project_does_not_hold_up_hello(self):
        self.task('a', 'add', '--project', 'a', text='Add rate limiting')
        lock = self.root / 'a/state/replica' / hashlib.sha256(b'a').hexdigest() / '.lock'
        with lock.open('a') as file:
            fcntl.flock(file, fcntl.LOCK_EX)
            started = time.monotonic()
            hello = self.call('a', 'hello')
            self.assertLess(time.monotonic() - started, 10)
            self.assertNotIn('a', hello['replica']['projects'])
        self.assertIn('a', self.call('a', 'hello')['replica']['projects'])

    def test_initial_join_can_keep_existing_swarm_session_placement(self):
        for node in ['a', 'b']:
            before = self.call(node, 'get'); desired = copy.deepcopy(before['organization'])
            desired['projects'][0]['sessions'] = ['b\ncodex/shared/session']
            self.patch(node, before, desired)
        self.join('a', 'b', prefer_peer=True)
        result = self.call('a', 'get')
        self.assertEqual(result['conflicts'], [])
        owner = next(p['id'] for p in result['organization']['projects'] if 'b\ncodex/shared/session' in p['sessions'])
        self.assertEqual(owner, 'b')

    def test_join_maps_membership_before_comparing_and_concrete_beats_peer_inbox(self):
        session = 'b\ncodex/shared/mapped'
        for node in ['a', 'b']:
            before = self.call(node, 'get'); desired = copy.deepcopy(before['organization'])
            desired['projects'][0]['sessions'] = [session]
            self.patch(node, before, desired)
        self.join('a', 'b', mapping={'a': 'b'}, prefer_peer=False)
        result = self.call('a', 'get')
        self.assertEqual(result['conflicts'], [])
        self.assertIn(session, next(p for p in result['organization']['projects'] if p['id'] == 'b')['sessions'])
        exported = self.call('a', 'export')['catalog']['operations'].values()
        heads = [o for o in exported if o['key'].get('session') == session.split('\n', 1)[1]]
        self.assertTrue(all(o['value'] == 'b' for o in heads))

    def test_concrete_local_project_beats_peer_inbox_even_when_keeping_peer_placement(self):
        session = 'b\ncodex/shared/concrete'
        for node in ['a', 'b']:
            before = self.call(node, 'get'); desired = copy.deepcopy(before['organization'])
            if node == 'b':
                desired['projects'].append(dict(id='ungrouped',name='Inbox',color='#123456',folders=[],sessions=[session]))
            else:
                desired['projects'][0]['sessions'] = [session]
            self.patch(node, before, desired)
        self.join('a', 'b', prefer_peer=True)
        result = self.call('a', 'get')
        self.assertEqual(result['conflicts'], [])
        self.assertIn(session, next(p for p in result['organization']['projects'] if p['id'] == 'a')['sessions'])

    def test_concurrent_delivery_choices_converge_without_conflict_review(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        self.call('a', 'preference', 'set', 'mobile_delivery', 'away_5', '--json')
        self.call('c', 'preference', 'set', 'mobile_delivery', 'immediate', '--json')
        snapshots = self.converge()
        values = {self.call(node, 'preference', 'get', '--json')['mobile_delivery'] for node in 'abc'}
        self.assertEqual(len(values), 1)
        self.assertIn(values.pop(), {'away_5', 'immediate'})
        for snapshot in snapshots:
            self.assertEqual(snapshot['conflicts'], [])
            self.assertEqual(snapshot['preferences'], snapshots[0]['preferences'])
        self.call('b', 'preference', 'set', 'mobile_delivery', 'away_30', '--json')
        self.converge()
        self.assertEqual({self.call(node, 'preference', 'get', '--json')['mobile_delivery'] for node in 'abc'}, {'away_30'})

    def test_joining_keeps_the_swarm_delivery_choice(self):
        self.call('b', 'preference', 'set', 'mobile_delivery', 'immediate', '--json')
        self.call('a', 'preference', 'set', 'mobile_delivery', 'away_5', '--json')
        self.join('a', 'b')
        self.assertEqual(self.call('a', 'preference', 'get', '--json'), {'mobile_delivery': 'immediate'})
        self.assertEqual(self.call('a', 'get')['conflicts'], [])
        self.call('c', 'preference', 'set', 'mobile_delivery', 'away_30', '--json')
        self.join('c', 'b')
        self.assertEqual(self.call('c', 'preference', 'get', '--json'), {'mobile_delivery': 'immediate'})

    def test_older_members_keep_project_sync_without_the_delivery_choice(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        (self.root / 'old-c').touch()  # c now answers b like a release without shared preferences
        self.call('a', 'preference', 'set', 'mobile_delivery', 'away_10', '--json')
        for node in ('a', 'b', 'b'):
            self.assertTrue(all(row['ok'] for row in self.call(node, 'sync').values()), node)
        self.assertEqual(self.call('b', 'preference', 'get', '--json'), {'mobile_delivery': 'away_10'})
        self.assertEqual(self.call('c', 'preference', 'get', '--json'), {'mobile_delivery': 'away'})
        before = self.call('b', 'get')
        desired = copy.deepcopy(before['organization'])
        next(p for p in desired['projects'] if p['id'] == 'b')['name'] = 'Renamed after the preference'
        self.patch('b', before, desired)
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        projects = self.call('c', 'get')['organization']['projects']
        self.assertEqual(next(p for p in projects if p['id'] == 'b')['name'], 'Renamed after the preference')
        peers = self.call('b', 'get')['peers']
        self.assertTrue(peers['c']['catalog_outdated'])
        self.assertNotIn('catalog_outdated', peers['a'])
        # An older hgs on b itself still reads b's catalog: peers keep their released fields.
        stored = json.loads((self.root / 'b/swarm/catalog.json').read_text())
        self.assertEqual({field for peer in stored['peers'].values() for field in peer}, {'node', 'last_sync', 'error'})
        # A legacy requester omits accepted kinds and receives only legacy kinds.
        inventory = self.call('b', 'inventory')
        response = self.call('b', 'exchange', data={'swarm_id': inventory['swarm_id'], 'known': [], 'operations': []})
        self.assertTrue(response['operations'])
        self.assertNotIn('preference', {op['key']['kind'] for op in response['operations']})
        # After the update the member receives the choice and the notice clears.
        (self.root / 'old-c').unlink()
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual(self.call('c', 'preference', 'get', '--json'), {'mobile_delivery': 'away_10'})
        self.assertNotIn('catalog_outdated', self.call('b', 'get')['peers']['c'])


    def order(self, node, ids):
        before = self.call(node, 'get')
        desired = copy.deepcopy(before['organization'])
        projects = {p['id']: p for p in desired['projects']}
        desired['projects'] = [projects[id] for id in ids] + [p for p in desired['projects'] if p['id'] not in ids]
        desired['project_order'] = [p['id'] for p in desired['projects']]
        return self.patch(node, before, desired)

    def shown(self, node):
        organization = self.call(node, 'get')['organization']
        return [p['id'] for p in organization['projects']], organization.get('project_order')

    def test_the_project_order_is_shared_by_every_member(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        self.assertEqual(self.shown('a'), (['a', 'b', 'c'], None))
        self.order('c', ['c', 'a', 'b'])
        snapshots = self.converge()
        for node in 'abc':
            self.assertEqual(self.shown(node), (['c', 'a', 'b'], ['c', 'a', 'b']), node)
        self.assertEqual({len(s['conflicts']) for s in snapshots}, {0})
        # A desktop without the shared order applies its own order as before;
        # the swarm's order stays.
        before = self.call('a', 'get')
        desired = copy.deepcopy(before['organization'])
        desired.pop('project_order')
        desired['projects'].reverse()
        next(p for p in desired['projects'] if p['id'] == 'a')['name'] = 'Renamed by an older desktop'
        self.patch('a', before, desired)
        self.converge()
        self.assertEqual(self.shown('b'), (['c', 'a', 'b'], ['c', 'a', 'b']))

    def test_joining_keeps_the_swarm_project_order(self):
        self.order('b', ['b'])
        self.order('a', ['a'])
        self.join('a', 'b')
        self.assertEqual(self.shown('a'), (['b', 'a'], ['b', 'a']))

    def test_a_joiner_brings_its_mapped_order_to_a_swarm_without_one(self):
        before = self.call('c', 'get')
        desired = copy.deepcopy(before['organization'])
        desired['projects'].append(project('d', 'Other', 'c', '/d'))
        desired['project_order'] = ['c', 'd']
        self.patch('c', before, desired)
        # c's project c becomes b's project b; unmapped, d would come first.
        self.join('c', 'b', mapping={'c': 'b'})
        self.assertEqual(self.shown('c'), (['b', 'd'], ['b', 'd']))

    def test_members_from_before_the_shared_order_keep_syncing_projects(self):
        self.join('a', 'b'); self.join('b', 'a'); self.join('c', 'b'); self.join('b', 'c')
        self.converge()
        (self.root / 'preference-c').touch()  # c answers b like a release without the shared order
        self.order('a', ['b', 'c', 'a'])
        self.call('a', 'preference', 'set', 'mobile_delivery', 'away_10', '--json')
        for node in ('a', 'b', 'b'):
            self.assertTrue(all(row['ok'] for row in self.call(node, 'sync').values()), node)
        self.assertEqual(self.shown('b'), (['b', 'c', 'a'], ['b', 'c', 'a']))
        self.assertEqual(self.shown('c'), (['a', 'b', 'c'], None))
        self.assertEqual(self.call('c', 'preference', 'get', '--json'), {'mobile_delivery': 'away_10'})
        self.assertNotIn('catalog_outdated', self.call('b', 'get')['peers']['c'])
        (self.root / 'preference-c').unlink()
        self.assertTrue(self.call('b', 'sync')['c']['ok'])
        self.assertEqual(self.shown('c'), (['b', 'c', 'a'], ['b', 'c', 'a']))


if __name__ == '__main__':
    unittest.main()
