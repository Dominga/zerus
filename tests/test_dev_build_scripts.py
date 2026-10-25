"""SDK-free Dev packaging policy checks; never run a packaging/build helper."""
import ast
from contextlib import chdir
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
ARCH = (ROOT / 'scripts/build-arch-package.sh').read_text()
MAC = (ROOT / 'scripts/ci/macos-bundle.sh').read_text()
WORKFLOW = (ROOT / '.github/workflows/macos-bundle.yml').read_text()


class DevBuildScriptTests(unittest.TestCase):
    def guard(self, source, variable, *arguments, **environment):
        bash = shutil.which('bash')
        if bash is None:
            self.skipTest('Pure shell policy controls require Bash')
        # Only the built-in argument/channel guard before the first cd is run.
        # An empty PATH also prevents accidental compiler/helper invocation.
        prefix = source.split('\ncd ', 1)[0]
        return subprocess.run([bash, '-c', prefix + f'\nprintf "%s" "${variable}"\n',
                               'synthetic-policy', *arguments],
                              env={'PATH': '', 'LC_ALL': 'C', **environment},
                              capture_output=True, text=True, timeout=5)

    def test_arch_full_tests_are_the_default_and_dev_optout_is_explicit(self):
        result = self.guard(ARCH, 'build_only')
        self.assertEqual((result.returncode, result.stdout), (0, 'false'))
        result = self.guard(ARCH, 'build_only', '--build-only',
                            ZERUS_RELEASE_CHANNEL='dev', ZERUS_DEV_BUILD_NUMBER='17')
        self.assertEqual((result.returncode, result.stdout), (0, 'true'))
        self.assertIn('makepkg_args=(--cleanbuild --force --noconfirm)', ARCH)
        self.assertIn('if [ "$build_only" = true ]; then makepkg_args+=(--nocheck); fi', ARCH)
        self.assertIn('makepkg "${makepkg_args[@]}"', ARCH)

    def test_arch_refuses_other_channels_missing_identity_and_unknown_flags(self):
        for channel in ('', 'stable', 'nightly', 'other'):
            with self.subTest(channel=channel):
                result = self.guard(ARCH, 'build_only', '--build-only',
                                    ZERUS_RELEASE_CHANNEL=channel, ZERUS_DEV_BUILD_NUMBER='17')
                self.assertEqual(result.returncode, 2)
        for number in ('', '0', '-1', '01', '1.5', '17 extra'):
            with self.subTest(number=number):
                self.assertEqual(self.guard(ARCH, 'build_only', '--build-only',
                                           ZERUS_RELEASE_CHANNEL='dev', ZERUS_DEV_BUILD_NUMBER=number).returncode, 2)
        for arguments in (('--nocheck',), ('--build-only', 'extra')):
            self.assertEqual(self.guard(ARCH, 'build_only', *arguments).returncode, 2)

    def test_mac_full_tests_are_default_and_only_numbered_dev_may_opt_out(self):
        self.assertEqual(self.guard(MAC, 'run_tests').stdout, 'true')
        result = self.guard(MAC, 'run_tests', ZERUS_RUN_TESTS='false',
                            ZERUS_RELEASE_CHANNEL='dev', ZERUS_DEV_BUILD_NUMBER='17')
        self.assertEqual((result.returncode, result.stdout), (0, 'false'))
        for channel in ('', 'stable', 'nightly', 'other'):
            with self.subTest(channel=channel):
                self.assertEqual(self.guard(MAC, 'run_tests', ZERUS_RUN_TESTS='false',
                                           ZERUS_RELEASE_CHANNEL=channel, ZERUS_DEV_BUILD_NUMBER='17').returncode, 2)
        for flag in ('0', 'FALSE', 'no', 'true extra'):
            with self.subTest(flag=flag):
                self.assertEqual(self.guard(MAC, 'run_tests', ZERUS_RUN_TESTS=flag).returncode, 2)
        for number in ('', '0', '-1', '01'):
            with self.subTest(number=number):
                self.assertEqual(self.guard(MAC, 'run_tests', ZERUS_RUN_TESTS='false',
                                           ZERUS_RELEASE_CHANNEL='dev', ZERUS_DEV_BUILD_NUMBER=number).returncode, 2)

    def rewrite_recipe(self, root, recipe, build_only):
        copied = root / 'disposable/PKGBUILD'
        copied.parent.mkdir(exist_ok=True)
        copied.write_text(recipe)
        body = re.search(r"(?ms)^python3 - .*? <<'PY'\n(.*?)^PY$", ARCH)[1]
        # Exercise only the embedded stdlib text transform, in this process.
        # No makepkg, Git, compiler, package validation helper or shell script.
        ast.parse(body)
        with patch.object(sys, 'argv', ['fixture', str(copied), str(root), build_only]):
            exec(compile(body, 'disposable-recipe-transform', 'exec'), {})
        return copied.read_text()

    def test_only_disposable_recipe_changes_and_default_keeps_testing_setting(self):
        recipe_path = ROOT / 'packaging/aur/zerus-git/PKGBUILD'
        original = recipe_path.read_bytes()
        recipe = original.decode().replace('-DBUILD_TESTING=OFF', '-DBUILD_TESTING=ON')
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            rewritten = self.rewrite_recipe(root, recipe, 'false')
            self.assertIn('-DBUILD_TESTING=ON', rewritten)
            self.assertIn("source=('zerus::git+" + root.as_uri() + "')", rewritten)
            self.assertEqual(recipe_path.read_bytes(), original)
        self.assertIn('cp packaging/aur/zerus-git/PKGBUILD "$package_build/PKGBUILD"', ARCH)
        self.assertIn('python3 - "$package_build/PKGBUILD" "$repo_root" "$build_only"', ARCH)

    def test_build_only_copy_disables_qt_tests_and_keeps_recipe_check_function(self):
        recipe = (ROOT / 'packaging/aur/zerus-git/PKGBUILD').read_text()
        for setting in ('ON', 'OFF'):
            with self.subTest(setting=setting), tempfile.TemporaryDirectory() as scratch:
                candidate = recipe.replace('-DBUILD_TESTING=OFF', '-DBUILD_TESTING=' + setting)
                rewritten = self.rewrite_recipe(Path(scratch), candidate, 'true')
                self.assertIn('-DBUILD_TESTING=OFF', rewritten)
                self.assertNotIn('-DBUILD_TESTING=ON', rewritten)
                self.assertEqual(rewritten.split('\ncheck() {\n', 1)[1], recipe.split('\ncheck() {\n', 1)[1])

    def test_build_only_copy_refuses_missing_or_ambiguous_cmake_setting(self):
        recipe = (ROOT / 'packaging/aur/zerus-git/PKGBUILD').read_text()
        for candidate in (recipe.replace('-DBUILD_TESTING=OFF', ''), recipe + '\n# -DBUILD_TESTING=ON\n'):
            with self.subTest(candidate=candidate[-60:]), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                with self.assertRaisesRegex(AssertionError, 'one explicit package CMake testing setting'):
                    self.rewrite_recipe(root, candidate, 'true')
                self.assertEqual((root / 'disposable/PKGBUILD').read_text(), candidate)

    def test_arch_package_and_artifact_validation_remain_required(self):
        for required in ('makepkg --printsrcinfo | diff - .SRCINFO',
                         'namcap PKGBUILD ./*.pkg.tar.zst',
                         "grep -q ' E: '", 'python3 scripts/check-package.py',
                         'artifacts/desktop-build-info.json'):
            self.assertIn(required, ARCH)
        self.assertNotIn('--skipinteg', ARCH)

    def test_mac_keeps_bundle_dependencies_source_identity_and_selftest(self):
        self.assertIn('build_testing=ON', MAC)
        self.assertIn('if [ "$run_tests" = false ]; then build_testing=OFF; fi', MAC)
        self.assertIn('"-DBUILD_TESTING=$build_testing"', MAC)
        regression = re.search(r'(?s)if \[ "\$run_tests" = true \]; then\n(.*?)\nelse\n.*?\nfi', MAC)[1]
        self.assertIn('ctest --test-dir', regression)
        after = MAC.split(regression, 1)[1]
        for required in ('python3 scripts/package-macos.py', '--connector services/mobile/connector/target/release/zerus-mobile-connector',
                         'python3 scripts/check-macos-bundle.py', '--selftest --source-commit "$(git rev-parse HEAD)"',
                         'ditto -c -k --keepParent'):
            self.assertIn(required, after)
        self.assertIn('"$release" != "$(cat DEV_VERSION)-dev.$ZERUS_DEV_BUILD_NUMBER"', MAC)

    def test_workflow_boolean_defaults_true_and_passes_channel_and_test_mode(self):
        declaration = WORKFLOW.split('      run_tests:\n', 1)[1].split('    outputs:', 1)[0]
        self.assertIn('        type: boolean\n', declaration)
        self.assertIn('        default: true\n', declaration)
        for line in ('RUN_TESTS: ${{ inputs.run_tests }}', 'ZERUS_RUN_TESTS: ${{ inputs.run_tests }}',
                     'ZERUS_RELEASE_CHANNEL: ${{ inputs.channel }}',
                     "ZERUS_DEV_BUILD_NUMBER: ${{ inputs.channel == 'dev' && github.run_number || '' }}"):
            self.assertIn(line, WORKFLOW)
        self.assertNotIn('secrets.', WORKFLOW)
        self.assertNotIn('environment: macos-signing', WORKFLOW)

    def workflow_release(self, channel, run_tests):
        body = textwrap.dedent(re.search(r"(?s)python3 - <<'PY'\n(.*?)\n          PY", WORKFLOW)[1])
        ast.parse(body)
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'VERSION').write_text('0.1.0\n')
            (root / 'DEV_VERSION').write_text('0.2.0\n')
            environment = {'CHANNEL': channel, 'RUN_TESTS': run_tests, 'COMMIT': 'a' * 40,
                           'RUN_NUMBER': '17', 'GITHUB_OUTPUT': str(root / 'output')}
            modules = {'macos_release': SimpleNamespace(cask_for=lambda name: name),
                       'nightly_common': SimpleNamespace(nightly_version=lambda *args: '0.1.0.r9.gaaaaaaa.n17')}
            with patch.dict(os.environ, environment, clear=True), patch.dict(sys.modules, modules), \
                 patch.object(subprocess, 'check_output', return_value='9\n') as git, patch.object(sys, 'path', list(sys.path)), chdir(root):
                exec(compile(body, 'workflow-release-policy', 'exec'), {})
                self.assertEqual(git.call_count, 1 if channel == 'nightly' else 0)
            return (root / 'output').read_text()

    def test_workflow_allows_fast_dev_and_full_tests_for_every_channel(self):
        self.assertEqual(self.workflow_release('dev', 'false'), 'release=0.2.0-dev.17\n')
        for channel in ('stable', 'nightly', 'dev'):
            with self.subTest(channel=channel):
                self.assertIn('release=', self.workflow_release(channel, 'true'))

    def test_workflow_refuses_fast_stable_nightly_and_invalid_boolean(self):
        for channel, flag in (('stable', 'false'), ('nightly', 'false'), ('other', 'false'), ('dev', 'no')):
            with self.subTest(channel=channel, flag=flag):
                with self.assertRaisesRegex(SystemExit, 'Only Dev bundles'):
                    self.workflow_release(channel, flag)


if __name__ == '__main__':
    unittest.main()
