"""The updater's switch rules (tray/update-installed.py): GUI kinds, drafts
roots, the rollback slot, the numbered Dev guard and the Qt settings check.
Fake HOME, fake bundles (com.example.zerus-switch-test) and a fake `defaults`;
nothing reaches an installed GUI, its LaunchAgent or a real settings domain."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('update_installed', ROOT / 'tray/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)

TEST_ID = 'com.example.zerus-switch-test'
DEFAULTS = '''#!{python}
import pathlib, sys
domain = pathlib.Path({root!r}) / 'domain.plist'
assert sys.argv[1:] == ['export', 'com.hgdev.hgs-tray', '-'], sys.argv
if not domain.exists():
    sys.stderr.write('Domain com.hgdev.hgs-tray does not exist\\n')
    sys.exit(1)
sys.stdout.buffer.write(domain.read_bytes())
'''


class Fixture(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='zts-')).resolve()
        self.addCleanup(shutil.rmtree, self.root, True)
        self.home = self.root / 'home'
        self.home.mkdir()
        rollback = self.home / '.local/share/hgs/desktop-previous'
        self.defaults = self.root / 'defaults'
        self.defaults.write_text(DEFAULTS.format(python=sys.executable, root=str(self.root)))
        self.defaults.chmod(0o755)
        self.lsregister = self.root / 'lsregister'
        self.lsregister.write_text(f'#!/bin/sh\necho "$@" >> {self.root}/lsregister.log\n')
        self.lsregister.chmod(0o755)
        for name, value in {'HOME': self.home, 'ROLLBACK': rollback,
                            'SETTINGS_SNAPSHOT': rollback / 'com.hgdev.hgs-tray.plist',
                            'SNAPSHOTS': self.home / '.local/state/hgs/desktop-update',
                            'LSREGISTER': self.lsregister, 'DEFAULTS': str(self.defaults)}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        environment = mock.patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.home / '.config'),
                                                   'XDG_DATA_HOME': str(self.home / '.local/share')})
        environment.start()
        self.addCleanup(environment.stop)

    def bundle(self, name, kind, identifier=TEST_ID):
        app = self.root / 'bundles' / name / 'hgs-tray.app'
        info = {'CFBundleIdentifier': identifier, 'CFBundleExecutable': 'hgs-tray'}
        if kind == 'tauri':
            info['ZerusGUI'] = 'tauri'
        (app / 'Contents/MacOS').mkdir(parents=True)
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        (app / 'Contents/MacOS/hgs-tray').write_text(name)
        return app

    def draft(self, root, name, text, blob=None):
        folder = root / name
        folder.mkdir(parents=True)
        attachments = []
        if blob is not None:
            digest = hashlib.sha256(blob).hexdigest()
            (folder / f'{digest}.bin').write_bytes(blob)
            attachments.append({'name': 'a.txt', 'mime': 'text/plain', 'reference': '[1]', 'sha256': digest, 'bytes': len(blob)})
        (folder / 'draft.json').write_text(json.dumps({'version': 1, 'key': name, 'text': text, 'attachments': attachments}))


class Switch(Fixture):
    def test_the_gui_kind_comes_from_the_info_plist(self):
        self.assertEqual(updater.gui_kind(updater.bundle_info(self.bundle('qt', 'qt'))), 'qt')
        self.assertEqual(updater.gui_kind(updater.bundle_info(self.bundle('client', 'tauri'))), 'tauri')
        self.assertEqual(updater.bundle_info(self.root / 'missing.app'), {})
        script = [sys.executable, str(ROOT / 'tray/update-installed.py'), '--installed-gui']
        for path, kind in ((self.bundle('cli', 'tauri'), 'tauri'), (self.root / 'none.app', 'none')):
            result = subprocess.run(script + [str(path)], capture_output=True, text=True, timeout=30,
                                    env=dict(os.environ, HOME=str(self.home)))
            self.assertEqual((result.returncode, result.stdout.strip()), (0, kind), result.stderr)

    def test_the_client_drafts_root_follows_the_bundle_identifier(self):
        with mock.patch.object(updater, 'MAC', True):
            self.assertEqual(updater.client_drafts_root('com.hgdev.hgs-tray'),
                             self.home / 'Library/Application Support/com.hgdev.hgs-tray/drafts')
        with mock.patch.object(updater, 'MAC', False):
            self.assertEqual(updater.client_drafts_root('dev.zerus.next'),
                             self.home / '.local/share/dev.zerus.next/drafts')

    def test_each_client_root_joins_qt_once(self):
        qt = {'CFBundleIdentifier': 'com.hgdev.hgs-tray'}
        client = {'CFBundleIdentifier': 'com.hgdev.hgs-tray', 'ZerusGUI': 'tauri'}
        self.assertEqual([label for label, _ in updater.draft_roots(qt, qt)], ['qt'])
        self.assertEqual([label for label, _ in updater.draft_roots(qt, client, client)],
                         ['qt', 'client-com.hgdev.hgs-tray'])

    def test_snapshots_and_checks_cover_every_root(self):
        client = {'CFBundleIdentifier': TEST_ID, 'ZerusGUI': 'tauri'}
        roots = updater.draft_roots({}, client)
        qt_root, client_root = roots[0][1], roots[1][1]
        self.draft(qt_root, 'qt-draft', 'from Qt')
        self.draft(client_root, 'client-draft', 'from the client', blob=b'attachment')
        self.draft(client_root, 'empty', '')
        saved = self.root / 'saved'
        self.assertEqual(updater.snapshot(saved, roots), 3)
        self.assertEqual(saved.stat().st_mode & 0o777, 0o700)
        self.assertEqual((saved / f'client-{TEST_ID}/client-draft/draft.json').stat().st_mode & 0o777, 0o600)
        self.assertEqual(updater.changed_drafts(saved, roots), (2, []))
        blob = next((client_root / 'client-draft').glob('*.bin'))
        blob.write_bytes(b'changed')
        shutil.rmtree(qt_root / 'qt-draft')
        preserved, problems = updater.changed_drafts(saved, roots)
        self.assertEqual(problems, ['qt-draft: missing', f'client-{TEST_ID} client-draft: attachment changed'])
        link = self.root / 'linked'
        (client_root / 'link').symlink_to(link)
        with self.assertRaisesRegex(updater.UpdateError, 'symbolic links'):
            updater.snapshot(self.root / 'again', roots)

    def test_the_slot_keeps_qt_only_after_the_switch(self):
        self.assertTrue(updater.keeps_slot('tauri', 'qt', 'tauri'))
        for kinds in (('qt', 'qt', 'tauri'), ('tauri', 'tauri', 'tauri'), ('tauri', None, 'tauri'),
                      ('tauri', 'qt', 'qt'), (None, 'qt', 'tauri'), ('qt', None, 'qt')):
            self.assertFalse(updater.keeps_slot(*kinds), kinds)

    def test_a_numbered_dev_install_is_never_replaced_by_an_unnumbered_build(self):
        numbered = {'version': '0.47.0', 'devBuildNumber': 1413}
        refusal = updater.numbered_refusal(numbered, {'version': '0.47.0', 'devBuildNumber': 0})
        self.assertIn('Dev build 1413', refusal)
        self.assertIn(updater.DEV_GUARD, refusal)
        self.assertIsNone(updater.numbered_refusal(numbered, {'devBuildNumber': 1414}))
        self.assertIsNone(updater.numbered_refusal(numbered, {'devBuildNumber': 0, 'previewLabel': 'task-board'}))
        self.assertIsNone(updater.numbered_refusal(None, {'devBuildNumber': 0}))
        self.assertIsNone(updater.numbered_refusal({'devBuildNumber': 0}, {'devBuildNumber': 0}))

    def test_the_refusal_says_what_the_reference_says(self):
        # The coordinator's ruling: the same wording in docs/reference.md and the refusal.
        words = lambda text: ' '.join(text.replace('`', '').split())
        self.assertIn(words(updater.DEV_GUARD), words((ROOT / 'docs/reference.md').read_text()))

    def test_an_on_disk_install_asks_the_updater_first(self):
        installed, built = self.bundle('installed', 'qt'), self.bundle('built', 'qt')
        infos = {installed: {'version': '0.47.0', 'devBuildNumber': 1413}, built: {'version': '0.47.0', 'devBuildNumber': 0}}
        with mock.patch.object(updater, 'build_info', lambda path: infos[Path(path).resolve()]), \
                mock.patch.object(updater, 'installed_build_info', lambda path: infos[Path(path).resolve()]):
            with self.assertRaisesRegex(updater.UpdateError, 'Dev build 1413.*Nothing was replaced'):
                updater.check_replace(built, installed)
            infos[built] = {'version': '0.47.0', 'devBuildNumber': 0, 'previewLabel': 'task-board'}
            self.assertEqual(updater.check_replace(built, installed), 0)
            client = self.bundle('client', 'tauri')
            infos[client] = {'version': '0.47.0', 'devBuildNumber': 1414}
            with mock.patch.object(updater, 'MAC', True):
                with self.assertRaisesRegex(updater.UpdateError, 'needs the live update'):
                    updater.check_replace(client, installed)
            self.assertEqual(updater.check_replace(built, self.root / 'missing.app'), 0, 'nothing installed yet')

    def test_settings_changes_separate_qt_keys_from_cocoa_keys(self):
        before = {'attention.observed': b'{}', 'workspace.reduceMotion': False, 'NSStatusItem Preferred Position Item-0': 900.0}
        after = {'attention.observed': b'{"x":1}', 'NSStatusItem Preferred Position Item-0': 880.0,
                 'NSStatusItem Visible Item-0': True, 'surprise.key': 1}
        self.assertEqual(updater.settings_changes(before, after),
                         (['attention.observed', 'workspace.reduceMotion'], ['surprise.key'],
                          ['NSStatusItem Preferred Position Item-0', 'NSStatusItem Visible Item-0']))
        self.assertEqual(updater.settings_changes(before, dict(before)), ([], [], []))

    def test_the_switch_keeps_and_checks_qt_settings_through_defaults(self):
        domain = self.root / 'domain.plist'
        with self.assertRaisesRegex(updater.UpdateError, 'cannot read the com.hgdev.hgs-tray settings'):
            updater.save_settings_snapshot()
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{}', 'NSWindow Frame main': 'x'}))
        updater.save_settings_snapshot()
        self.assertEqual(updater.SETTINGS_SNAPSHOT.stat().st_mode & 0o777, 0o600)
        self.assertEqual(updater.settings_report(), ([], []))
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{}', 'NSWindow Frame main': 'y',
                                           'NSStatusItem Visible Item-0': True}))
        self.assertEqual(updater.settings_report(),
                         ([], ['Cocoa settings written since the switch: NSStatusItem Visible Item-0, NSWindow Frame main']))
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{"changed":1}'}))
        problems, _ = updater.settings_report(rollback=True)
        self.assertIn('Qt settings changed or missing: attention.observed', problems[0])
        self.assertIn(f'defaults import com.hgdev.hgs-tray {updater.SETTINGS_SNAPSHOT}', problems[1])


class SlotAndRestore(Fixture):
    def stage_of(self, bundle, installed):
        stage = installed.parent / '.stage' / installed.name
        shutil.copytree(bundle, stage)
        return stage

    def test_a_client_update_keeps_qt_as_the_rollback_copy_and_can_restore_the_old_client(self):
        installed = self.root / 'Applications/hgs-tray.app'
        shutil.copytree(self.bundle('client-1', 'tauri'), installed)
        slot = updater.ROLLBACK / 'hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), slot)
        aside = self.root / 'update/previous'
        # The fake bundles are unsigned: on macOS replace() would verify them with codesign.
        with mock.patch.object(updater, 'run', lambda *a, **k: ''):
            previous = updater.replace(self.stage_of(self.bundle('client-2', 'tauri'), installed), installed, aside)
        self.assertEqual((installed / 'Contents/MacOS/hgs-tray').read_text(), 'client-2')
        self.assertEqual((slot / 'Contents/MacOS/hgs-tray').read_text(), 'qt', 'Qt stays the rollback copy')
        self.assertEqual(previous, aside / 'hgs-tray.app')
        with mock.patch.object(updater, 'stop_started', lambda installed: None):
            self.assertTrue(updater.roll_back(installed, previous))
        self.assertEqual((installed / 'Contents/MacOS/hgs-tray').read_text(), 'client-1',
                         'a failing new client brings back the client it replaced, not Qt')

    def test_the_switch_moves_qt_into_the_slot_and_unregisters_it(self):
        installed = self.root / 'Applications/hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), installed)
        with mock.patch.object(updater, 'MAC', True), mock.patch.object(updater, 'run', lambda *a, **k: ''):
            previous = updater.replace(self.stage_of(self.bundle('client', 'tauri'), installed), installed)
        self.assertEqual(previous, updater.ROLLBACK / 'hgs-tray.app')
        self.assertEqual((previous / 'Contents/MacOS/hgs-tray').read_text(), 'qt')
        log = (self.root / 'lsregister.log').read_text().splitlines()
        self.assertEqual(log, [f'-u {previous}', f'-f {installed}'])


if __name__ == '__main__':
    unittest.main()
