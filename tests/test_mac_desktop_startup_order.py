"""Private filesystem plus recorded subprocesses; no launchd or GUI is reached."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('mac_startup_updater', ROOT / 'packaging/desktop/update-installed.py')
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


class Step4ServiceOrder(unittest.TestCase):
    """Execute the real shell tail, updater.start and real registration helper."""
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='zstep4-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.home = self.root / 'home'
        self.home.mkdir()
        self.installed = self.home / 'Applications/hgs-tray.app'
        for path in (self.installed / 'Contents/MacOS/hgs', self.root / 'bin/hgs'):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('private synthetic CLI; never executed')
            path.chmod(0o755)
        self.driver = ROOT / 'tests/fixtures/mac_desktop_startup_driver.py'
        self.state = self.root / 'launchd-model.json'
        self.environment = dict(os.environ, HOME=str(self.home), TEST_PYTHON=sys.executable,
            TEST_DRIVER=str(self.driver), TEST_REPO=str(ROOT), TEST_ROOT=str(self.root),
            SCRIPT_DIR=str(self.root / 'packaging/desktop'), REPO=str(self.root), HGS_BIN=str(self.root / 'bin/hgs'),
            APP_DEST=str(self.installed), BUILD_APP=str(self.root / 'build.app'),
            PLIST_DEST=str(self.home / 'Library/LaunchAgents/com.hgdev.hgs-tray.plist'),
            LABEL='com.hgdev.hgs-tray', RELOAD='0', RESTART_RUNNING='1', CHANGED='0')

    def step4(self, update):
        script = (ROOT / 'packaging/desktop/install.sh').read_text()
        begin = script.index('# --- 4. load ')
        end = script.index('\nHINT_SELFTEST=', begin)
        prefix = r'''set -euo pipefail
python3() { "$TEST_PYTHON" "$TEST_DRIVER" "$TEST_REPO" "$TEST_ROOT" python "$@"; }
launchctl() { "$TEST_PYTHON" "$TEST_DRIVER" "$TEST_REPO" "$TEST_ROOT" launchctl "$@"; }
DESKTOP_HGS="$APP_DEST/Contents/MacOS/hgs"
DESKTOP_ARGS=(--restart --runner-hgs "$HGS_BIN")
'''
        result = subprocess.run(['bash', '-s'], input=prefix + script[begin:end],
            env=dict(self.environment, UPDATE=str(update)), text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(self.state.read_text())['events']

    def test_changed_backend_has_no_second_restart_after_gui_bootstrap(self):
        subprocess.run([sys.executable, str(self.driver), str(ROOT), str(self.root), 'init'],
                       check=True, timeout=10)
        self.assertEqual(self.step4(1), ['desktop:bootout', 'desktop:bootstrap', 'gui:bootstrap'])

    def test_non_updater_install_refreshes_service_before_its_first_gui(self):
        self.assertEqual(self.step4(0), ['desktop:bootstrap', 'gui:bootstrap'])


if __name__ == '__main__':
    unittest.main()
