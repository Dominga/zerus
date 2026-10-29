"""First-use catalog bootstrap through isolated CLI and organization streams."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

from test_watch import Stream


REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', REPO / 'target/debug/hgs')).resolve()


class SwarmBootstrap(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='hgs-swarm-bootstrap-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.home = self.root / 'home'
        self.bin = self.home / '.local/bin'
        self.bin.mkdir(parents=True)
        # This fixture has no sessions or native agents and never reaches a
        # real tmux server, service manager or SSH connection.
        (self.bin / 'tmux').write_text('#!/bin/sh\nexit 1\n')
        (self.bin / 'tmux').chmod(0o755)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith('HGS_') and key not in ('TMUX', 'TMUX_PANE')}
        self.env.update(HOME=str(self.home), USERPROFILE=str(self.home),
                        XDG_CONFIG_HOME=str(self.root / 'xdg'),
                        APPDATA=str(self.root / 'roaming'),
                        HGS_CONFIG_DIR=str(self.root / 'config'),
                        HGS_STATE_DIR=str(self.root / 'state'),
                        HGS_SELF='example-local', HGS_PEERS='', HGS_TAB='0',
                        PATH=str(self.bin) + os.pathsep + os.environ['PATH'])
        self.catalog = self.root / 'config/swarm/catalog.json'
        self.marker = self.catalog.with_name('gui-import-pending.json')

    def call(self, *args, payload=None, timeout=10):
        result = subprocess.run([str(HGS), 'swarm', *args], env=self.env,
                                input=json.dumps(payload) if payload is not None else '',
                                text=True, capture_output=True, timeout=timeout)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def qt_store(self):
        if sys.platform == 'darwin':
            path = self.home / 'Library/Preferences/com.hgdev.hgs-tray.plist'
        elif sys.platform == 'win32':
            path = self.root / 'roaming/hgdev/hgs-tray.ini'
        else:
            path = self.root / 'xdg/hgdev/hgs-tray.conf'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b'unparsed synthetic Qt organization store')
        return path

    def organization(self):
        stream = Stream(self)
        stream.next(lambda frame: frame['t'] == 'hello')
        stream.send(op='subscribe', topic='organization')
        frame = stream.next(lambda frame: frame['t'] == 'snapshot'
                            and frame['topic'] == 'organization')
        rows = {item['key']: item['row'] for item in frame['items']}
        self.assertTrue(frame['last'])
        return stream, rows['organization']

    def test_get_bootstraps_without_name_derived_projects_and_keeps_refreshing_machine_metadata(self):
        first = self.call('get')
        self.assertTrue(first['initialized'])
        self.assertEqual(first['written'], {})
        self.assertEqual(first['organization']['projects'], [])
        stored = json.loads(self.catalog.read_bytes())
        self.assertTrue(stored['catalog']['operations'])
        self.assertTrue(all(op['key']['kind'] == 'machine' for op in stored['catalog']['operations'].values()))
        self.assertEqual(json.loads(self.marker.read_bytes())['node_id'], first['node_id'])
        before = self.catalog.read_bytes()
        self.env['HGS_SELF'] = 'example-renamed'
        second = self.call('get')
        self.assertEqual(second['node_id'], first['node_id'])
        self.assertNotEqual(self.catalog.read_bytes(), before)
        stored = json.loads(self.catalog.read_bytes())
        self.assertEqual(stored['bindings']['example-renamed']['id'], first['node_id'])
        self.assertTrue(any(op['key']['kind'] == 'machine' and op['value'] == 'example-renamed'
                            for op in stored['catalog']['operations'].values()))

    def test_get_with_any_qt_store_keeps_its_stable_uninitialized_node_without_bootstrapping(self):
        qt = self.qt_store()
        before = qt.read_bytes()
        snapshot = self.call('get')
        self.assertFalse(snapshot['initialized'])
        self.assertTrue(self.catalog.exists())
        uuid.UUID(snapshot['node_id'])
        self.assertEqual(self.call('get')['node_id'], snapshot['node_id'])
        self.assertFalse(self.marker.exists())
        self.assertEqual(qt.read_bytes(), before)

    def test_a_corrupt_pending_marker_keeps_get_successful_and_the_catalog_unchanged(self):
        self.qt_store().unlink()
        # Create the same uninitialized state as hello without bootstrapping.
        stream = Stream(self)
        stream.next(lambda frame: frame['t'] == 'hello')
        self.assertEqual(stream.close(), 0)
        before = self.catalog.read_bytes()
        for contents in (b'{broken', b'{"version":2,"node_id":"future-node"}'):
            with self.subTest(contents=contents):
                self.marker.write_bytes(contents)
                self.assertFalse(self.call('get')['initialized'])
                self.assertEqual(self.catalog.read_bytes(), before)
                self.assertEqual(self.marker.read_bytes(), contents)

    def test_a_corrupt_pending_marker_keeps_watch_readable_and_warns_only_once(self):
        stream = Stream(self)
        stream.next(lambda frame: frame['t'] == 'hello')
        before = self.catalog.read_bytes()
        self.marker.write_bytes(b'{broken')
        for _ in range(2):
            stream.send(op='subscribe', topic='organization')
            frame = stream.next(lambda frame: frame['t'] == 'snapshot'
                                and frame['topic'] == 'organization')
            snapshot = next(item['row'] for item in frame['items'] if item['key'] == 'organization')
            self.assertFalse(snapshot['initialized'])
        self.assertEqual(stream.close(), 0)
        self.assertEqual(stream.process.stderr.read().count('catalog bootstrap skipped:'), 1)
        self.assertEqual(self.catalog.read_bytes(), before)
        self.assertEqual(self.marker.read_bytes(), b'{broken')

    def test_initial_organization_watch_bootstraps_and_a_later_gui_import_preserves_edits(self):
        stream, snapshot = self.organization()
        self.assertTrue(snapshot['initialized'])
        self.assertEqual(snapshot['organization']['projects'], [])
        self.assertTrue(self.marker.exists())
        qt = self.qt_store()
        before = qt.read_bytes()
        gui = {'projects': [{'id': 'gui-project', 'name': 'GUI project', 'color': '#123456',
                             'sessions': [], 'folders': []}]}
        imported = self.call('initialize', payload=gui)
        self.assertFalse(self.marker.exists())
        desired = json.loads(json.dumps(imported['organization']))
        desired['projects'][0]['name'] = 'Shared edit'
        self.call('apply', payload=dict(base=imported['organization'], desired=desired,
                                       versions=imported['versions']))
        stale = dict(gui, projects=gui['projects'] + [dict(gui['projects'][0], id='not-imported-again')])
        repeated = self.call('initialize', payload=stale)
        self.assertEqual([p['name'] for p in repeated['organization']['projects']], ['Shared edit'])
        self.assertEqual(qt.read_bytes(), before)
        self.assertEqual(stream.close(), 0)

    def test_get_and_the_first_watch_subscription_race_on_one_catalog_identity(self):
        stream = Stream(self)
        hello = stream.next(lambda frame: frame['t'] == 'hello')
        self.assertFalse(json.loads(self.catalog.read_bytes())['imported'])
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            got = pool.submit(self.call, 'get')
            stream.send(op='subscribe', topic='organization')
            frame = stream.next(lambda frame: frame['t'] == 'snapshot' and frame['topic'] == 'organization')
            snapshot = next(item['row'] for item in frame['items'] if item['key'] == 'organization')
            got = got.result(timeout=15)
        stored = json.loads(self.catalog.read_bytes())
        self.assertTrue(snapshot['initialized'])
        self.assertTrue(got['initialized'])
        self.assertEqual(got['node_id'], hello['machine_id'])
        self.assertEqual(snapshot['node_id'], hello['machine_id'])
        self.assertEqual(json.loads(self.marker.read_bytes())['node_id'], stored['node_id'])
        self.assertEqual(stream.close(), 0)

    def test_watch_refresh_does_not_retry_until_an_explicit_new_subscription(self):
        qt = self.qt_store()
        stream, snapshot = self.organization()
        self.assertFalse(snapshot['initialized'])
        qt.unlink()
        self.call('resolve', payload=dict(key=json.dumps(dict(kind='project', id='watch-project', field='alive')),
                                         versions=[], value=True))
        frame = stream.next(lambda frame: frame['t'] == 'delta' and frame['topic'] == 'organization')
        snapshot = next(item['row'] for item in frame['upsert'] if item['key'] == 'organization')
        self.assertFalse(snapshot['initialized'])
        self.assertFalse(self.marker.exists())
        stream.send(op='subscribe', topic='organization')
        frame = stream.next(lambda frame: frame['t'] == 'snapshot' and frame['topic'] == 'organization')
        snapshot = next(item['row'] for item in frame['items'] if item['key'] == 'organization')
        self.assertTrue(snapshot['initialized'])
        self.assertTrue(self.marker.exists())
        self.assertEqual(self.call('get')['organization']['projects'][0]['id'], 'watch-project')
        self.assertEqual(stream.close(), 0)

    def test_initial_organization_watch_leaves_a_computer_with_qt_settings_uninitialized(self):
        qt = self.qt_store()
        before = qt.read_bytes()
        stream, snapshot = self.organization()
        self.assertFalse(snapshot['initialized'])
        self.assertFalse(self.marker.exists())
        self.assertEqual(qt.read_bytes(), before)
        self.assertEqual(stream.close(), 0)


if __name__ == '__main__':
    unittest.main()
