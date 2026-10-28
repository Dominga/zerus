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

    def test_a_half_deleted_bundle_has_no_kind(self):
        half = self.bundle('half', 'qt')
        (half / 'Contents/Info.plist').unlink()
        self.assertIsNone(updater.bundle_kind(half))
        self.assertIsNone(updater.bundle_kind(self.root / 'missing.app'))
        self.assertEqual(updater.bundle_kind(self.bundle('client', 'tauri')), 'tauri')

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
                # The installer's guard before a live update: the switch itself is the updater's.
                self.assertEqual(updater.check_replace(client, installed, live=True), 0)
            self.assertEqual(updater.check_replace(built, self.root / 'missing.app'), 0, 'nothing installed yet')

    def test_settings_changes_separate_qt_keys_from_cocoa_keys(self):
        before = {'attention.observed': b'{}', 'workspace.reduceMotion': False, 'NSStatusItem Preferred Position Item-0': 900.0}
        after = {'attention.observed': b'{"x":1}', 'NSStatusItem Preferred Position Item-0': 880.0,
                 'NSStatusItem Visible Item-0': True, 'surprise.key': 1, 'WebContinuousSpellCheckingEnabled': True}
        self.assertEqual(updater.settings_changes(before, after),
                         (['attention.observed', 'workspace.reduceMotion'], ['surprise.key'],
                          ['NSStatusItem Preferred Position Item-0', 'NSStatusItem Visible Item-0',
                           'WebContinuousSpellCheckingEnabled']))
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
        problems, _ = updater.settings_report(before_qt=True)
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


class UpdateFlow(Fixture):
    """update() with MAC=True on any host: processes, launchd and codesign are
    stubbed, so the slot, the restore and the settings checks run as on a Mac."""

    def setUp(self):
        super().setUp()
        self.installed = self.root / 'Applications/hgs-tray.app'
        self.slot = updater.ROLLBACK / 'hgs-tray.app'
        self.running = True
        # wait_started answers, in order; 200 once they run out.
        self.starts = []
        self.events = []
        # What Qt or the client writes into the settings domain when it starts.
        self.on_start = None
        stubs = {
            'MAC': True,
            'run': lambda *a, **k: '',
            'build_info': lambda path: {'version': '0.47.0', 'versionName': self.variant(path), 'devBuildNumber': 1413},
            'gui_pids': lambda installed: [100] if self.running else [],
            'quit_gui': lambda pid, installed: self.events.append('quit'),
            'socket_path': lambda pid: None,
            'native': dict,
            'start': self.start,
            'wait_started': lambda installed, path, old: self.starts.pop(0) if self.starts else 200,
            'stop_started': lambda installed: None,
            'running_image_matches': lambda pid, installed: True,
            'provenance': lambda *a: None,
        }
        for name, value in stubs.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def start(self, installed):
        self.events.append(('start', self.variant(installed)))
        if self.on_start:
            self.on_start()

    def variant(self, path):
        return (Path(path) / 'Contents/MacOS/hgs-tray').read_text()

    def bundle(self, name, kind, identifier=TEST_ID):
        app = self.root / 'bundles' / name / 'hgs-tray.app'
        return app if app.exists() else super().bundle(name, kind, identifier)

    def install(self, name, kind, path):
        shutil.copytree(self.bundle(name, kind), path)

    def domain(self, values):
        (self.root / 'domain.plist').write_bytes(plistlib.dumps(values))

    def test_a_client_that_does_not_start_brings_back_the_client_it_replaced(self):
        self.install('client-1', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.starts = [None]
        with self.assertRaisesRegex(updater.UpdateError, 'did not start; the previous GUI was restored'):
            updater.update(self.bundle('client-2', 'tauri'), self.installed)
        self.assertEqual(self.variant(self.installed), 'client-1', 'the replaced client, not the Qt copy')
        self.assertEqual(self.variant(self.slot), 'qt', 'Qt stays the rollback copy')

    def test_a_rollback_that_does_not_start_keeps_the_qt_copy(self):
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.starts = [None]
        with self.assertRaisesRegex(updater.UpdateError, 'did not start; the previous GUI was restored'):
            updater.roll_back_installed(self.installed)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client', 'qt'))

    def test_a_rollback_takes_the_slot_only_once_qt_runs(self):
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt', 'client'))
        self.assertEqual(updater.ROLLBACK.stat().st_mode & 0o777, 0o700)
        self.assertIn(f'-u {self.slot}', (self.root / 'lsregister.log').read_text().splitlines())

    def test_an_interrupted_rollback_keeps_the_qt_copy_and_the_client(self):
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.running = False
        rename = os.rename

        def interrupted(source, target, *args, **kwargs):
            if Path(target) == self.installed and Path(source).parent.name.startswith('.hgs-tray-update-'):
                raise KeyboardInterrupt
            return rename(source, target, *args, **kwargs)

        with mock.patch.object(updater.os, 'rename', interrupted), self.assertRaises(KeyboardInterrupt):
            updater.roll_back_installed(self.installed)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client', 'qt'))
        self.assertEqual(list(self.installed.parent.glob('.hgs-tray-update-*')), [])

    def test_an_install_over_nothing_leaves_the_slot_alone(self):
        self.install('qt', 'qt', self.slot)
        self.running = False
        self.assertEqual(updater.update(self.bundle('client', 'tauri'), self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client', 'qt'))

    def test_leaving_the_client_for_qt_checks_settings_before_qt_starts(self):
        updater.ROLLBACK.mkdir(parents=True)
        updater.SETTINGS_SNAPSHOT.write_bytes(plistlib.dumps({'attention.observed': b'{}'}))
        for how, self.running in (('rollback', True), ('install', True), ('rollback', False), ('install', False)):
            with self.subTest(how=how, running=self.running):
                shutil.rmtree(self.installed, ignore_errors=True)
                shutil.rmtree(self.slot, ignore_errors=True)
                self.install('client', 'tauri', self.installed)
                self.install('qt', 'qt', self.slot)
                # The client changed a Qt key; Qt rewrites it as soon as it starts.
                self.domain({'attention.observed': b'{"client":1}'})
                self.on_start = lambda: self.domain({'attention.observed': b'{}'})
                outcome = 'the new GUI runs, but ' if self.running else 'the new GUI is installed, but '
                with self.assertRaisesRegex(updater.UpdateError, outcome + 'Qt settings changed or missing: attention.observed') as raised:
                    if how == 'rollback':
                        updater.roll_back_installed(self.installed)
                    else:
                        updater.update(self.bundle('qt-new', 'qt'), self.installed)
                self.assertIn('defaults import com.hgdev.hgs-tray', str(raised.exception))
                self.assertEqual(self.variant(self.installed), 'qt' if how == 'rollback' else 'qt-new')

    def test_a_switch_to_the_client_checks_settings_once_it_runs(self):
        self.install('qt', 'qt', self.installed)
        self.domain({'attention.observed': b'{}'})
        # The client writes a Qt key after it starts: the switch-time copy shows it.
        self.on_start = lambda: self.domain({'attention.observed': b'{"client":1}'})
        with self.assertRaisesRegex(updater.UpdateError, 'Qt settings changed or missing: attention.observed') as raised:
            updater.update(self.bundle('client', 'tauri'), self.installed)
        self.assertNotIn('defaults import', str(raised.exception), 'no restore line while the client runs')
        self.assertTrue(updater.SETTINGS_SNAPSHOT.exists())

    def test_qt_may_write_its_settings_once_it_runs_again(self):
        updater.ROLLBACK.mkdir(parents=True)
        updater.SETTINGS_SNAPSHOT.write_bytes(plistlib.dumps({'attention.observed': b'{}'}))
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.domain({'attention.observed': b'{}'})
        self.on_start = lambda: self.domain({'attention.observed': b'{"qt":1}'})
        self.assertEqual(updater.roll_back_installed(self.installed), 0)

    def test_a_stopped_rollback_takes_the_slot_too(self):
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.running = False
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt', 'client'))
        self.assertEqual(self.events, [])

    def test_an_update_interrupted_twice_keeps_its_copies(self):
        # The new GUI cannot move in and the old one cannot move back: the
        # update's folder is all that is left, so it stays.
        self.install('client-1', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.running = False
        rename = os.rename

        def interrupted(source, target, *args, **kwargs):
            if Path(target) == self.installed:
                raise KeyboardInterrupt
            return rename(source, target, *args, **kwargs)

        with mock.patch.object(updater.os, 'rename', interrupted), self.assertRaises(KeyboardInterrupt):
            updater.update(self.bundle('client-2', 'tauri'), self.installed)
        self.assertFalse(self.installed.exists())
        [folder] = self.installed.parent.glob('.hgs-tray-update-*')
        self.assertEqual((self.variant(folder / 'hgs-tray.app'), self.variant(folder / 'previous/hgs-tray.app')),
                         ('client-2', 'client-1'))
        self.assertEqual(self.variant(self.slot), 'qt')

    def test_rolling_back_to_the_client_again_does_not_report_qt_writes(self):
        # After a rollback Qt ran and wrote its keys; going back to the client is a new switch.
        updater.ROLLBACK.mkdir(parents=True)
        updater.SETTINGS_SNAPSHOT.write_bytes(plistlib.dumps({'attention.observed': b'{}'}))
        self.install('qt', 'qt', self.installed)
        self.install('client', 'tauri', self.slot)
        self.domain({'attention.observed': b'{"qt":1}'})
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client', 'qt'))

    def test_symbolic_links_are_never_rolled_back(self):
        self.install('client', 'tauri', self.installed)
        updater.ROLLBACK.mkdir(parents=True)
        self.slot.symlink_to(self.bundle('elsewhere', 'qt'))
        with self.assertRaisesRegex(updater.UpdateError, 'symbolic link'):
            updater.roll_back_installed(self.installed)
        self.assertEqual(self.variant(self.installed), 'client')
        self.slot.unlink()
        self.install('qt', 'qt', self.slot)
        shutil.rmtree(self.installed)
        self.installed.symlink_to(self.bundle('linked', 'tauri'))
        with self.assertRaisesRegex(updater.UpdateError, 'symbolic link'):
            updater.roll_back_installed(self.installed)
        self.assertTrue(self.installed.is_symlink())
        self.assertEqual(self.variant(self.slot), 'qt')


if __name__ == '__main__':
    unittest.main()
