"""The updater's switch rules (tray/update-installed.py): GUI kinds, drafts
roots, the rollback slot, the numbered Dev guard and the Qt settings check.
Fake HOME, fake bundles (com.example.zerus-switch-test) and a fake `defaults`;
nothing reaches an installed GUI, its LaunchAgent or a real settings domain."""
import errno
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
        if app.exists():
            return app
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
        result = subprocess.run([sys.executable, str(ROOT / 'tray/update-installed.py'), '--installed-gui', str(half)],
                                capture_output=True, text=True, timeout=30, env=dict(os.environ, HOME=str(self.home)))
        self.assertEqual((result.returncode, result.stdout.strip()), (0, 'none'), result.stderr)
        self.assertIsNone(updater.bundle_kind(self.root / 'missing.app'))
        self.assertEqual(updater.bundle_kind(self.bundle('client', 'tauri')), 'tauri')

    def test_the_slot_keeps_qt_only_after_the_switch(self):
        # Qt stays while the client replaces anything but Qt: a client, an incomplete bundle or nothing.
        for kinds in (('tauri', 'qt', 'tauri'), (None, 'qt', 'tauri')):
            self.assertTrue(updater.keeps_slot(*kinds), kinds)
        for kinds in (('qt', 'qt', 'tauri'), ('tauri', 'tauri', 'tauri'), ('tauri', None, 'tauri'),
                      ('tauri', 'qt', 'qt'), ('qt', None, 'qt')):
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
    def setUp(self):
        super().setUp()
        self.installed = self.root / 'Applications/hgs-tray.app'
        self.slot = updater.ROLLBACK / 'hgs-tray.app'
        for name, value in {'MAC': True, 'run': lambda *a, **k: ''}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def stage_of(self, bundle):
        stage = self.root / 'Applications/.stage' / self.installed.name
        shutil.copytree(bundle, stage)
        return stage

    def text(self, bundle):
        return (bundle / 'Contents/MacOS/hgs-tray').read_text()

    def test_the_swap_sets_the_installed_gui_aside_and_never_touches_the_slot(self):
        shutil.copytree(self.bundle('client-1', 'tauri'), self.installed)
        shutil.copytree(self.bundle('qt', 'qt'), self.slot)
        aside = self.root / 'Applications/.update/previous/hgs-tray.app'
        updater.swap_in(self.stage_of(self.bundle('client-2', 'tauri')), self.installed, aside)
        self.assertEqual((self.text(self.installed), self.text(aside), self.text(self.slot)), ('client-2', 'client-1', 'qt'))
        self.assertEqual((self.root / 'lsregister.log').read_text().splitlines(), [f'-f {self.installed}', f'-u {aside}'])
        updater.put_back(self.installed, aside, self.root / 'Applications/.update/new/hgs-tray.app')
        self.assertEqual(self.text(self.installed), 'client-1', 'a failing new client brings back the client it replaced')

    def test_a_failed_swap_moves_the_replaced_gui_back(self):
        shutil.copytree(self.bundle('client-1', 'tauri'), self.installed)
        aside = self.root / 'Applications/.update/previous/hgs-tray.app'

        def codesign(*args, **kwargs):
            raise subprocess.CalledProcessError(1, ['codesign'])
        with mock.patch.object(updater, 'run', codesign), self.assertRaises(subprocess.CalledProcessError):
            updater.swap_in(self.stage_of(self.bundle('client-2', 'tauri')), self.installed, aside)
        self.assertEqual(self.text(self.installed), 'client-1')
        self.assertFalse(aside.exists())

    def test_the_slot_takes_the_replaced_gui_only_once_it_is_in_and_unregisters_it(self):
        shutil.copytree(self.bundle('qt-old', 'qt'), self.slot)
        previous = self.root / 'Applications/.update/previous/hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), previous)
        updater.take_slot(previous)
        self.assertEqual(self.text(self.slot), 'qt')
        self.assertEqual([p.name for p in updater.ROLLBACK.iterdir()], ['hgs-tray.app'], 'no leftovers')
        self.assertIn(f'-u {self.slot}', (self.root / 'lsregister.log').read_text().splitlines())
        self.assertEqual(updater.ROLLBACK.stat().st_mode & 0o777, 0o700)

    def test_across_file_systems_the_original_goes_only_once_the_slot_holds_its_copy(self):
        shutil.copytree(self.bundle('qt-old', 'qt'), self.slot)
        previous = self.root / 'Applications/.update/previous/hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), previous)
        rename = os.rename

        def across(source, target, *args, **kwargs):
            if Path(source) == previous:
                raise OSError(errno.EXDEV, 'Invalid cross-device link')
            return rename(source, target, *args, **kwargs)
        with mock.patch.object(updater.os, 'rename', across):
            updater.take_slot(previous)
        self.assertEqual(self.text(self.slot), 'qt')
        self.assertFalse(previous.exists())
        self.assertEqual([p.name for p in updater.ROLLBACK.iterdir()], ['hgs-tray.app'])

    def test_an_interrupted_slot_change_keeps_both_copies_and_says_where(self):
        # N3: at every rename, the slot keeps the old copy or holds the new one,
        # and the new one is never left where a later change would remove it.
        rename = os.rename
        for point in range(1, 4):
            with self.subTest(point=point):
                shutil.rmtree(updater.ROLLBACK, ignore_errors=True)
                shutil.copytree(self.bundle('qt-old', 'qt'), self.slot)
                previous = self.root / 'Applications/.update/previous/hgs-tray.app'
                shutil.rmtree(previous.parent, ignore_errors=True)
                shutil.copytree(self.bundle('qt', 'qt'), previous)
                calls, lines = [], []

                def interrupted(source, target, *args, **kwargs):
                    calls.append(target)
                    if len(calls) == point:
                        raise KeyboardInterrupt
                    return rename(source, target, *args, **kwargs)
                with mock.patch.object(updater.os, 'rename', interrupted), \
                        mock.patch.object(updater, 'say', lines.append), self.assertRaises(KeyboardInterrupt):
                    updater.take_slot(previous)
                self.assertEqual(self.text(self.slot), 'qt-old')
                [kept] = [path for path in (previous, *updater.ROLLBACK.glob('.incoming-*/hgs-tray.app')) if path.exists()]
                self.assertEqual(self.text(kept), 'qt')
                self.assertIn(str(kept), lines[-1])


class Flow(Fixture):
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
        self.lines = []
        stubs = {
            'say': self.lines.append,
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

    def install(self, name, kind, path):
        shutil.copytree(self.bundle(name, kind), path)

    def domain(self, values):
        (self.root / 'domain.plist').write_bytes(plistlib.dumps(values))


class UpdateFlow(Flow):
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

    def test_a_stopped_rollback_keeps_the_slot_and_the_client(self):
        # Nothing starts Qt, so nothing verifies it: the slot stays, and the
        # client is kept where the update says (I1, I2).
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        self.running = False
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt', 'qt'))
        [kept] = self.installed.parent.glob('.hgs-tray-kept-*/previous/hgs-tray.app')
        self.assertEqual(self.variant(kept), 'client')
        self.assertIn(f'the replaced GUI is kept in {kept}', self.lines[-1])
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
        [folder] = self.installed.parent.glob('.hgs-tray-kept-*')
        self.assertEqual((self.variant(folder / 'hgs-tray.app'), self.variant(folder / 'previous/hgs-tray.app')),
                         ('client-2', 'client-1'))
        self.assertEqual(self.variant(self.slot), 'qt')
        self.assertIn(f'nothing complete is installed at {self.installed}', self.lines[-1])
        self.assertIn(f'the replaced GUI is kept in {folder / "previous/hgs-tray.app"}', self.lines[-1])

    def test_a_second_rollback_interrupted_while_the_client_starts_keeps_qt(self):
        # N1: Qt was set aside for the client; Ctrl-C in the wait must not lose it.
        self.install('qt', 'qt', self.installed)
        self.install('client', 'tauri', self.slot)
        self.domain({'attention.observed': b'{}'})

        def interrupted(*args):
            raise KeyboardInterrupt
        with mock.patch.object(updater, 'wait_started', interrupted), self.assertRaises(KeyboardInterrupt):
            updater.roll_back_installed(self.installed)
        [kept] = self.installed.parent.glob('.hgs-tray-kept-*/previous/hgs-tray.app')
        self.assertEqual((self.variant(self.installed), self.variant(self.slot), self.variant(kept)),
                         ('client', 'client', 'qt'))
        self.assertIn(f'the replaced GUI is kept in {kept}', self.lines[-1])

    def test_a_bootstrap_failure_restores_qt_and_keeps_the_slot(self):
        # N1: launchctl bootstrap fails for the client; Qt comes back, the slot keeps the client.
        self.install('qt', 'qt', self.installed)
        self.install('client', 'tauri', self.slot)
        self.domain({'attention.observed': b'{}'})
        calls = []

        def start(installed):
            calls.append(self.variant(installed))
            if len(calls) == 1:
                raise subprocess.CalledProcessError(5, ['launchctl', 'bootstrap'])
        with mock.patch.object(updater, 'start', start), \
                self.assertRaisesRegex(updater.UpdateError, 'did not start .*bootstrap.*; the previous GUI was restored and started'):
            updater.roll_back_installed(self.installed)
        self.assertEqual(calls, ['client', 'qt'])
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt', 'client'))
        self.assertEqual(list(self.installed.parent.glob('.hgs-tray-*')), [])

    def test_a_failed_qt_install_over_the_client_keeps_the_last_qt(self):
        # N2: an ordinary client-to-Qt install (HGS_MAC_GUI=qt) leaves the slot alone until Qt runs.
        self.install('client', 'tauri', self.installed)
        self.install('qt-last', 'qt', self.slot)
        self.domain({'attention.observed': b'{}'})
        self.starts = [None]
        with self.assertRaisesRegex(updater.UpdateError, 'the previous GUI was restored'):
            updater.update(self.bundle('qt-new', 'qt'), self.installed)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client', 'qt-last'))
        self.assertEqual(updater.update(self.bundle('qt-new', 'qt'), self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt-new', 'client'))

    def test_an_incomplete_install_never_takes_the_slot(self):
        # N2: a half-deleted bundle (an interrupted rm -rf / cp -R) is not a GUI.
        self.install('client-1', 'tauri', self.installed)
        (self.installed / 'Contents/Info.plist').unlink()
        self.install('qt', 'qt', self.slot)
        self.assertEqual(updater.update(self.bundle('client-2', 'tauri'), self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('client-2', 'qt'))
        self.assertIn('the incomplete bundle that was installed (no Info.plist identity) was removed', self.lines[-1])
        self.assertEqual(list(self.installed.parent.glob('.hgs-tray-*')), [])

    def test_an_unreadable_settings_copy_is_a_reported_problem(self):
        # N4: the check reports it and Zerus starts again; nothing raises after the quit.
        self.install('client', 'tauri', self.installed)
        self.install('qt', 'qt', self.slot)
        updater.rollback_dir()
        updater.SETTINGS_SNAPSHOT.write_bytes(b'not a plist')
        self.domain({'attention.observed': b'{}'})
        with self.assertRaisesRegex(updater.UpdateError, 'the new GUI runs, but cannot read the copy of the Qt settings'):
            updater.roll_back_installed(self.installed)
        self.assertEqual(self.events, ['quit', ('start', 'qt')])
        self.assertEqual((self.variant(self.installed), self.variant(self.slot)), ('qt', 'client'))

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

    def test_symbolic_links_are_never_followed_by_an_update(self):
        # I4 and N5: the installed path (also a dangling link), the rollback folder and its slot.
        elsewhere = self.root / 'elsewhere'
        elsewhere.mkdir(mode=0o755)
        cases = {
            'installed': lambda: self.installed.symlink_to(self.root / 'gone.app'),
            'rollback folder': lambda: updater.ROLLBACK.symlink_to(elsewhere),
            'slot': lambda: self.slot.symlink_to(self.bundle('linked', 'qt')),
        }
        for name, link in cases.items():
            with self.subTest(link=name):
                for path in (self.installed, self.slot, updater.ROLLBACK):
                    if path.is_symlink():
                        path.unlink()
                shutil.rmtree(self.installed, ignore_errors=True)
                shutil.rmtree(updater.ROLLBACK, ignore_errors=True)
                self.installed.parent.mkdir(parents=True, exist_ok=True)
                updater.ROLLBACK.parent.mkdir(parents=True, exist_ok=True)
                if name != 'installed':
                    self.install('qt', 'qt', self.installed)
                if name == 'slot':
                    updater.ROLLBACK.mkdir()
                link()
                self.running = False
                with self.assertRaisesRegex(updater.UpdateError, 'is a symbolic link; nothing was replaced'):
                    updater.update(self.bundle('client', 'tauri'), self.installed)
                self.assertEqual(sorted(p.name for p in elsewhere.iterdir()), [])
                self.assertEqual(elsewhere.stat().st_mode & 0o777, 0o755)
                self.assertEqual(list(self.installed.parent.glob('.hgs-tray-*')), [])


class Invariants(Flow):
    """The ruling on the updater (webui, 2026-10-28), over every direction,
    start result and fault point:
    I1 no GUI bundle is deleted before its replacement was verified running,
       and then only one that policy supersedes or that has a copy elsewhere;
    I2 the slot changes only after a verified start;
    I3 every failure or interrupt restores the set-aside GUI or keeps it where
       a printed line names it;
    I4 links are refused (test_symbolic_links_are_never_followed_by_an_update).
    Fault points are counted, not listed: each rename, codesign and lsregister
    call, removal and copy, in turn, raises Ctrl-C or an OSError."""

    # installed, slot, build (None: --rollback), what policy supersedes once the new GUI runs
    DIRECTIONS = {
        'client to client': (('client-1', 'tauri'), ('qt', 'qt'), ('client-2', 'tauri'), {'client-1'}),
        'Qt to client': (('qt', 'qt'), ('qt-old', 'qt'), ('client', 'tauri'), {'qt-old'}),
        'client to Qt': (('client', 'tauri'), ('qt-last', 'qt'), ('qt-new', 'qt'), {'qt-last'}),
        '--rollback': (('client', 'tauri'), ('qt', 'qt'), None, set()),
        'second --rollback': (('qt', 'qt'), ('client', 'tauri'), None, set()),
        'install over nothing': (None, ('qt', 'qt'), ('client', 'tauri'), set()),
    }
    STARTS = ('ok', 'start() raises', 'wait times out', 'Ctrl-C in wait_started', 'not running')

    def setUp(self):
        super().setUp()
        settings = {'attention.observed': b'{}'}
        for name, value in {'wait_started': self.wait,
                            'export_settings': lambda: (plistlib.dumps(settings), dict(settings))}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def start(self, installed):
        self.started += 1
        if self.mode == 'start() raises' and self.started == 1:
            raise subprocess.CalledProcessError(5, ['launchctl', 'bootstrap'])

    def wait(self, installed, path, old):
        self.waited += 1
        if self.waited == 1 and self.mode == 'wait times out':
            return None
        if self.waited == 1 and self.mode == 'Ctrl-C in wait_started':
            raise KeyboardInterrupt
        self.verified |= self.found().get(self.installed, (None,))[0] == self.requested
        return 200

    def found(self):
        """Every bundle outside the build sources: (variant, kind), or None when incomplete."""
        bundles = {}
        for path in self.root.rglob('hgs-tray.app'):
            if path.relative_to(self.root).parts[0] == 'bundles':
                continue
            info, executable = updater.bundle_info(path), path / 'Contents/MacOS/hgs-tray'
            complete = info.get('CFBundleIdentifier') and executable.is_file()
            bundles[path] = (executable.read_text(), updater.gui_kind(info)) if complete else None
        return bundles

    def faulty(self, fault, point):
        """Patches that count every interruptible step and fail the `point`-th."""
        count = [0]

        def wrap(function):
            def step(*args, **kwargs):
                count[0] += 1
                if count[0] == point:
                    raise fault() if fault is KeyboardInterrupt else fault(errno.EIO, 'injected')
                return function(*args, **kwargs)
            return step
        targets = [(updater.os, 'rename'), (updater.shutil, 'copytree'), (updater.shutil, 'rmtree'),
                   (updater, 'run'), (updater, 'register'), (updater, 'unregister'), (updater, 'remove')]
        return count, [mock.patch.object(target, name, wrap(getattr(target, name))) for target, name in targets]

    def case(self, direction, mode, fault, point):
        installed, slot, built, superseded = self.DIRECTIONS[direction]
        for path in (self.installed.parent, updater.ROLLBACK):
            shutil.rmtree(path, ignore_errors=True)
        if installed:
            self.install(*installed, self.installed)
        if slot:
            self.install(*slot, self.slot)
        self.lines.clear()
        self.mode, self.started, self.waited, self.verified = mode, 0, 0, False
        self.running = mode != 'not running' and installed is not None
        self.requested = (built or slot)[0]
        count, patches = self.faulty(fault, point)
        error = None
        for patcher in patches:
            patcher.start()
        try:
            if built is None:
                updater.roll_back_installed(self.installed)
            else:
                updater.update(self.bundle(*built), self.installed)
        except (Exception, KeyboardInterrupt) as raised:
            error = raised
        finally:
            for patcher in patches:
                patcher.stop()

        bundles = self.found()
        complete = {path: found for path, found in bundles.items() if found}
        output = '\n'.join(self.lines) + f'\n{error}'
        named = lambda path: path in (self.installed, self.slot) or any(
            str(folder) in output for folder in (path, path.parent, path.parent.parent))
        context = f'{output}\n{sorted(map(str, bundles))}'
        self.assertNotIn(self.installed, set(bundles) - set(complete), 'never an incomplete installed GUI')
        # I1, I3: every original GUI survives, where the owner can find it, unless
        # policy superseded it after the new GUI was verified running.
        for variant in {installed and installed[0], slot[0]} - {None} - (superseded if self.verified else set()):
            places = [path for path, found in complete.items() if found[0] == variant and named(path)]
            self.assertTrue(places, f'{variant} lost or unnamed: {context}')
        self.assertTrue(any(kind == 'qt' for _, kind in complete.values()), f'no complete Qt: {context}')
        self.assertTrue(any(kind == 'tauri' for _, kind in complete.values()) or (built and built[1] == 'tauri'),
                        f'no client: {context}')
        # I2: the slot changes only after a verified start.
        if not self.verified:
            self.assertEqual(complete.get(self.slot, (None,))[0], slot[0], context)
        # A line names the installed path whenever it does not hold the requested GUI.
        if complete.get(self.installed, (None,))[0] != self.requested:
            self.assertIn(str(self.installed), output, context)
        if error is None:
            self.assertEqual(complete[self.installed][0], self.requested)
        return count[0]

    def test_every_direction_start_result_and_fault_point_keeps_the_invariants(self):
        cases = 0
        for direction in self.DIRECTIONS:
            for mode in self.STARTS:
                for fault in (KeyboardInterrupt, OSError):
                    point = 0
                    while True:
                        with self.subTest(direction=direction, start=mode, fault=fault.__name__, point=point):
                            steps = self.case(direction, mode, fault, point)
                        cases += 1
                        point += 1
                        if point > steps:
                            break
        self.assertGreater(cases, 600)


if __name__ == '__main__':
    unittest.main()
