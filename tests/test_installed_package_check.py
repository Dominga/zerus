"""Installed-package fixtures must not inherit another profile's XDG roots."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('installed_package_check', ROOT / 'scripts/check-installed-package.py')
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class PrivateProfile(unittest.TestCase):
    def test_inherited_xdg_and_fake_manager_stay_in_the_same_private_home(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); home = root / 'home'; home.mkdir(mode=0o700)
            foreign = root / 'foreign'; foreign.mkdir(mode=0o700)
            marker = foreign / 'keep'; marker.write_bytes(b'unchanged')
            sockets = home / 'tmux'; sockets.mkdir(mode=0o700)
            inherited = dict(os.environ, XDG_CONFIG_HOME=str(foreign), XDG_STATE_HOME=str(foreign),
                             HGS_STATE_DIR='/owner', ZERUS_CLIENT_DESKTOP_SERVICE='1')
            env = PACKAGE.isolated_environment(home, sockets, inherited)
            env.update(FIXTURE_VENV=str(home/'venv'), FIXTURE_CONFIG=str(home/'connector.json'))
            systemctl = home / 'fake-systemctl.py'; systemctl.write_text(PACKAGE.SYSTEMCTL)
            selected = Path(env['XDG_CONFIG_HOME']) / 'systemd/user/zerus-mobile-connector.service.d'
            selected.mkdir(parents=True)
            (selected/'50-zerus-killmode.conf').write_text('[Service]\nKillMode=process\n')
            subprocess.run([sys.executable, str(systemctl), '--user', 'daemon-reload'], env=env, check=True)
            self.assertEqual(json.loads((home/'loaded.json').read_text())['KillMode'], 'process')
            self.assertEqual(list(foreign.iterdir()), [marker])
            self.assertEqual(marker.read_bytes(), b'unchanged')
            self.assertNotIn('ZERUS_CLIENT_DESKTOP_SERVICE', env)
            self.assertEqual(env['HGS_STATE_DIR'], str(home/'state'))
            for key in ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
                self.assertTrue(Path(env[key]).is_relative_to(home))
                self.assertEqual(Path(env[key]).stat().st_mode & 0o777, 0o700)


if __name__ == '__main__':
    unittest.main()
