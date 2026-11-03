"""Private filesystem plus recorded subprocesses; no launchd or GUI is reached."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('mac_startup_updater', ROOT / 'tray/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)


class MacDesktopStartupOrder(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='zdesk-order-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.installed = self.root / 'Applications/hgs-tray.app'
        self.backend = self.installed / 'Contents/MacOS/hgs'
        self.backend.parent.mkdir(parents=True)
        self.backend.write_bytes(b'private simulated signed backend; never executed')
        self.runner = self.root / 'bin/hgs'
        self.calls = []
        for name, value in [('MAC', True), ('REFRESH_MAC_DESKTOP', self.runner), ('CLI', self.runner)]:
            selected = patch.object(updater, name, value)
            selected.start()
            self.addCleanup(selected.stop)
        self.manager = patch.object(updater.subprocess, 'run', side_effect=self.command)
        self.manager.start()
        self.addCleanup(self.manager.stop)
        self.invocation = patch.object(updater, 'run', side_effect=self.execute)
        self.invocation.start()
        self.addCleanup(self.invocation.stop)

    def command(self, args, **kwargs):
        self.calls.append(tuple(map(str, args)))
        # The retained GUI job is already absent after bootout.
        return subprocess.CompletedProcess(args, 1 if args[1] == 'print' else 0)

    def execute(self, args, **kwargs):
        self.calls.append(tuple(map(str, args)))
        return ''

    def test_installed_backend_is_refreshed_before_any_gui_manager_start(self):
        before = self.backend.read_bytes()
        updater.start(self.installed)
        self.assertEqual(self.calls[0], (sys.executable,
            str(ROOT / 'scripts/install-macos-desktop-service.py'), str(self.backend),
            '--restart', '--runner-hgs', str(self.runner)))
        self.assertEqual(self.calls[1][0:2], ('launchctl', 'bootout'))
        self.assertEqual(self.calls[-1][0:2], ('launchctl', 'bootstrap'))
        self.assertEqual(self.backend.read_bytes(), before)

    def test_failed_backend_refresh_cannot_start_or_replace_the_gui_job(self):
        self.invocation.stop()
        with patch.object(updater, 'run', side_effect=subprocess.CalledProcessError(1, ['private-helper'])):
            with self.assertRaises(subprocess.CalledProcessError):
                updater.start(self.installed)
        self.assertEqual(self.calls, [])

    def test_a_missing_paired_helper_never_falls_back_to_the_runner(self):
        self.backend.unlink()
        record = self.installed / 'Contents/Resources/zerus-local-pair.json'
        record.parent.mkdir()
        record.write_text('{}')
        with self.assertRaisesRegex(updater.UpdateError, 'paired desktop helper is missing'):
            updater.start(self.installed)
        self.assertEqual(self.calls, [])

    def test_restored_unpaired_gui_refreshes_ordinary_backend_without_pair_override(self):
        self.backend.unlink()
        updater.start(self.installed)
        self.assertEqual(self.calls[0], (sys.executable,
            str(ROOT / 'scripts/install-macos-desktop-service.py'), str(self.runner), '--restart'))
        self.assertEqual(self.calls[-1][0:2], ('launchctl', 'bootstrap'))

    def test_ordinary_updater_entry_keeps_its_existing_manager_contract(self):
        with patch.object(updater, 'REFRESH_MAC_DESKTOP', None):
            updater.start(self.installed)
        self.assertEqual(self.calls[0][0:2], ('launchctl', 'bootout'))
        self.assertFalse(any('--restart' in call for call in self.calls))


if __name__ == '__main__':
    unittest.main()
