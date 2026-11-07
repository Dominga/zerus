"""Exercise opt-out guards in the actual shell scripts without running builders."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "posix", "Unix release-script stubs; Windows has its own native contracts")
class TestSchedulingGuards(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='zerus-test-scheduling-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.shell = shutil.which("bash", path="/bin:/usr/bin")
        self.assertIsNotNone(self.shell, "System Bash is required for the release scripts")
        self.bin = self.root / 'bin'; self.bin.mkdir()
        for name, text in {'cargo': 'echo reached-builder; exit 77', 'tmux': 'exit 0',
                           'git': 'printf "%s\\n" aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                           'id': 'echo 1000',
                           'makepkg': 'if [ "$1" = --printsrcinfo ]; then cat "' + str(ROOT / 'packaging/aur/zerus-git/.SRCINFO') + '"; else echo reached-builder; exit 77; fi'}.items():
            p = self.bin / name; p.write_text('#!' + self.shell + '\n' + text + '\n'); p.chmod(0o755)
        for name in ('VERSION', 'DEV_VERSION', 'scripts/build-arch-package.sh', 'scripts/ci/macos-bundle.sh',
                     'packaging/aur/zerus-git/PKGBUILD', 'packaging/aur/zerus-git/.SRCINFO'):
            p = self.root / name; p.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(ROOT / name, p)

    def run_script(self, script, args=(), **values):
        env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + '/bin:/usr/bin',
               'ZERUS_RUN_TESTS': 'false', 'ZERUS_MACOS_TESTS_IN_VERIFY': '',
               'ZERUS_RELEASE_CHANNEL': '', 'ZERUS_RELEASE_VERSION': '', 'ZERUS_DEV_BUILD_NUMBER': ''}
        env.update(values)
        return subprocess.run([self.shell, str(self.root / script), *args], env=env,
                              capture_output=True, text=True, timeout=10)

    def test_system_bash_outside_usr_bin_still_exercises_the_real_script(self):
        original = subprocess.run
        def no_usr_bin_bash(argv, *args, **kwargs):
            if argv[0] == '/usr/bin/bash':
                raise FileNotFoundError('Darwin has /bin/bash, not /usr/bin/bash')
            return original(argv, *args, **kwargs)
        with patch.object(subprocess, 'run', side_effect=no_usr_bin_bash):
            try:
                result = self.run_script('scripts/ci/macos-bundle.sh', ZERUS_RUN_TESTS='true')
            except FileNotFoundError:
                self.fail('Scheduling control requires /usr/bin/bash')
        self.assertEqual(result.returncode, 77, result.stderr)
        self.assertIn('reached-builder', result.stdout)

    def test_stable_bundle_opt_out_requires_separate_tests_and_the_exact_version(self):
        version = (ROOT / 'VERSION').read_text().strip()
        for flag, release, expected in [('', version, 2), ('true', version, 77), ('true', '0.0.1', 2)]:
            with self.subTest(flag=flag, release=release):
                result = self.run_script('scripts/ci/macos-bundle.sh', ZERUS_RELEASE_CHANNEL='stable',
                                         ZERUS_MACOS_TESTS_IN_VERIFY=flag, ZERUS_RELEASE_VERSION=release)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual('reached-builder' in result.stdout, expected == 77)
        result = self.run_script('scripts/ci/macos-bundle.sh', ZERUS_RELEASE_CHANNEL='nightly', ZERUS_MACOS_TESTS_IN_VERIFY='true')
        self.assertEqual(result.returncode, 2)

    def test_dev_and_ordinary_test_defaults_are_preserved(self):
        release = (ROOT / 'DEV_VERSION').read_text().strip() + '-dev.123'
        result = self.run_script('scripts/ci/macos-bundle.sh', ZERUS_RELEASE_CHANNEL='dev',
                                 ZERUS_DEV_BUILD_NUMBER='123', ZERUS_RELEASE_VERSION=release)
        self.assertEqual(result.returncode, 77, result.stderr)
        result = self.run_script('scripts/ci/macos-bundle.sh', ZERUS_RUN_TESTS='true')
        self.assertEqual(result.returncode, 77, result.stderr)

    def test_arch_parallel_source_checks_must_name_this_checkout(self):
        for source, expected in [('a' * 40, 77), ('b' * 40, 2), ('', 2), ('main', 2)]:
            with self.subTest(source=source):
                result = self.run_script('scripts/build-arch-package.sh', ('--checked-source', source))
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual('reached-builder' in result.stdout, expected == 77)
        result = self.run_script('scripts/build-arch-package.sh', ('--build-only',))
        self.assertEqual(result.returncode, 2)


if __name__ == '__main__':
    unittest.main()
