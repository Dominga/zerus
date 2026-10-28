"""On macOS: switch, client update and rollback with ad-hoc signed fake
bundles (com.example.zerus-switch-test) in a fake HOME. lsregister is replaced
by /usr/bin/true and `defaults` by a fixture, so neither LaunchServices nor a
real settings domain changes. No GUI runs: quitting, starting and waiting are
recorded stand-ins, so launchd and the real com.hgdev.hgs-tray agent are never
reached, while codesign checks every copy for real."""
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
spec = importlib.util.spec_from_file_location('update_installed_mac', ROOT / 'tray/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)


@unittest.skipUnless(sys.platform == 'darwin', 'codesign and bundles are macOS-only')
class MacSwitch(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='zms-', dir='/tmp')).resolve()
        self.addCleanup(shutil.rmtree, self.root, True)
        self.home = self.root / 'home'
        self.home.mkdir()
        (self.root / 'domain.plist').write_bytes(plistlib.dumps({'attention.observed': b'{}'}))
        defaults = self.root / 'defaults'
        defaults.write_text(f'#!/bin/sh\ncat {self.root}/domain.plist\n')
        defaults.chmod(0o755)
        rollback = self.home / '.local/share/hgs/desktop-previous'
        for name, value in {'HOME': self.home, 'ROLLBACK': rollback, 'SETTINGS_SNAPSHOT': rollback / 'com.hgdev.hgs-tray.plist',
                            'SNAPSHOTS': self.home / '.local/state/hgs/desktop-update',
                            'PROVENANCE': self.home / '.local/share/hgs/build-provenance.json',
                            'CLI': self.home / '.local/bin/hgs', 'HELPER': self.home / '.local/bin/hgs_state.py',
                            'PLIST': self.home / 'Library/LaunchAgents/com.hgdev.hgs-tray.plist',
                            'LSREGISTER': Path('/usr/bin/true'), 'DEFAULTS': str(defaults)}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        # Stand-ins for everything that would reach launchd or a running GUI.
        self.calls = []
        for name, value in {'gui_pids': lambda installed: [100] if installed.exists() else [],
                            'quit_gui': lambda pid, installed: self.calls.append('quit'),
                            'start': lambda installed: self.calls.append(('start', self.variant(installed))),
                            'wait_started': lambda installed, path, old: 200,
                            'stop_started': lambda installed: self.calls.append('stop'),
                            'socket_path': lambda pid: None, 'native': dict,
                            'running_image_matches': lambda pid, installed: True,
                            'provenance': lambda *args: None}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.installed = self.root / 'Applications/hgs-tray.app'

    def bundle(self, name, kind, number=1413):
        app = self.root / 'bundles' / name / 'Zerus.app'
        (app / 'Contents/MacOS').mkdir(parents=True)
        info = {'CFBundleIdentifier': 'com.example.zerus-switch-test', 'CFBundleExecutable': 'hgs-tray',
                'CFBundlePackageType': 'APPL'}
        if kind == 'tauri':
            info['ZerusGUI'] = 'tauri'
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        build_info = json.dumps({'version': '0.47.0', 'versionName': name, 'devBuildNumber': number})
        source = self.root / f'{name}.c'
        source.write_text('#include <stdio.h>\nint main(void) { puts(%s); return 0; }\n' % json.dumps(build_info))
        subprocess.run(['cc', '-o', str(app / 'Contents/MacOS/hgs-tray'), str(source)], check=True)
        subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True, capture_output=True)
        return app

    def variant(self, path):
        return json.loads(subprocess.check_output([str(updater.executable(path)), '--build-info']))['versionName']

    def test_switch_client_update_and_rollback(self):
        # Nothing is installed yet, so nothing runs: Qt is installed without a start.
        self.assertEqual(updater.update(self.bundle('qt', 'qt'), self.installed), 0)
        self.assertEqual(self.calls, [])
        self.assertEqual(updater.update(self.bundle('client-1', 'tauri'), self.installed), 0)
        slot = updater.ROLLBACK / 'hgs-tray.app'
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('client-1', 'qt'))
        self.assertTrue(updater.SETTINGS_SNAPSHOT.exists(), 'the switch keeps Qt settings beside the Qt copy')
        self.assertEqual(updater.update(self.bundle('client-2', 'tauri'), self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('client-2', 'qt'))
        with self.assertRaisesRegex(updater.UpdateError, 'Dev build 1413'):
            updater.update(self.bundle('unnumbered', 'tauri', number=0), self.installed)
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('qt', 'client-2'))
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(self.installed)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(slot)], check=True)
        self.assertEqual(self.calls, ['quit', ('start', 'client-1'), 'quit', ('start', 'client-2'), 'quit', ('start', 'qt')])
        self.assertEqual(list(self.installed.parent.glob('.hgs-tray-*')), [], 'nothing kept: every start was verified')


if __name__ == '__main__':
    unittest.main()
