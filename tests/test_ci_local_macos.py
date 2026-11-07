"""Mac planning must preserve checks and the local producer's exact boundary."""
import importlib.util
from pathlib import Path
import sys
import os
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('local_mac_ci', ROOT / 'scripts/ci/local-macos.py')
MAC = importlib.util.module_from_spec(SPEC)
with patch.object(sys, 'path', [str(ROOT / 'scripts/ci'), *sys.path]):
    SPEC.loader.exec_module(MAC)


class MacPlan(unittest.TestCase):
    def workflow(self, steps):
        return {'jobs': {'cli-macos-debug': {'runs-on': 'macos-15', 'steps': steps}}}

    def test_compound_or_renamed_infrastructure_keeps_its_real_body(self):
        bodies = ['echo cache; exit 19', 'brew install tmux python; exit 19',
                  'rustup toolchain install stable --profile minimal && rustup default stable; exit 19',
                  next(iter(MAC.TRANSPORT)) + '\necho MISSED; exit 19']
        rows = MAC.plan(self.workflow([dict(name='Rust cache identity', run=b) for b in bodies]), '')[0]['steps']
        self.assertEqual([row.get('body') for row in rows], bodies)
        for body in bodies:
            row = MAC.plan(self.workflow([dict(name='Seal the tested debug executables and their provenance', run=body)]), '')[0]['steps'][0]
            self.assertEqual(row.get('body'), body)

    def test_transport_substitution_requires_the_whole_known_body(self):
        for body in MAC.TRANSPORT:
            with self.subTest(body=body):
                row = MAC.plan(self.workflow([dict(run=body)]), '')[0]['steps'][0]
                self.assertEqual(row['status'], 'local')
                self.assertNotIn('body', row)
        row = MAC.plan(self.workflow([dict(uses='actions/checkout@123', run='exit 19')]), '')[0]['steps'][0]
        self.assertEqual(row['status'], 'not run')

    def test_independent_remaining_suites_keep_their_original_source(self):
        steps = [dict(run='bash scripts/ci/cli.sh remaining')]
        workflow = {'jobs': {'cli-macos-core': {'runs-on': 'macos-15', 'steps': steps}}}
        rows = MAC.plan(workflow, (ROOT / 'scripts/ci/cli.sh').read_text())[0]['steps']
        self.assertEqual([row['name'] for row in rows], ['CLI shell smoke', 'CLI Python integration', 'CLI release build'])
        self.assertIn('python3 scripts/ci/python-tests.py', rows[1]['body'])
        self.assertIn('voice_recovery_fixture', rows[1]['body'])

    def test_exact_binary_builds_do_not_depend_on_a_passing_unit_suite(self):
        rows = MAC.plan(self.workflow([dict(run='bash scripts/ci/cli.sh debug')]),
                        (ROOT / 'scripts/ci/cli.sh').read_text())[0]['steps']
        self.assertEqual([row['body'] for row in rows], ['cargo build --locked\n',
                         'cargo build --locked --example fake_agent\n', 'bash scripts/ci/cli.sh debug'])

    def test_unknown_node_major_leaves_a_gap(self):
        step = {'uses': 'actions/setup-node@123', 'with': {'node-version': '26'}}
        row = MAC.plan(self.workflow([step]), '')[0]['steps'][0]
        self.assertEqual(row['status'], 'not run')

    def test_xcode_substitution_binds_the_complete_selector_script(self):
        MAC.require_xcode('Xcode 26.2\nBuild version example')
        for output in ('Xcode 25.4', 'Command Line Tools', ''):
            with self.assertRaises(RuntimeError):
                MAC.require_xcode(output)
        body = 'bash mobile/ios/scripts/select-xcode.sh'
        workflow = {'jobs': {'ios': {'runs-on': 'macos-15', 'steps': [{'run': body}]}}}
        original = (ROOT / 'mobile/ios/scripts/select-xcode.sh').read_bytes()
        row = MAC.plan(workflow, '', xcode_selector=original)[0]['steps'][0]
        self.assertEqual(row['status'], 'local')
        for script in (None, original + b'\nexit 19\n'):
            row = MAC.plan(workflow, '', xcode_selector=script)[0]['steps'][0]
            self.assertEqual(row['status'], 'not run')
            self.assertNotIn('body', row)
        workflow['jobs']['ios']['steps'][0]['run'] += '\nexit 19'
        row = MAC.plan(workflow, '', xcode_selector=original)[0]['steps'][0]
        self.assertEqual(row['body'], body + '\nexit 19')

    def test_signal_and_file_scope_cannot_expand_to_the_owner_home(self):
        profile = MAC.profile('/Users/example/zerus-dev/private-ci', ['/public-git/objects'], '/private/tmp/owned-ci')
        self.assertIn('(allow signal (target same-sandbox))', profile)
        self.assertNotIn('(allow signal)', profile)
        self.assertNotIn('(subpath "/Users/example")', profile)
        self.assertIn('(literal "/private/tmp")', profile)
        self.assertNotIn('(subpath "/private/tmp")', profile)
        self.assertNotIn('com.hgdev.hgs-tray', profile)
        self.assertIn('(allow file-ioctl (subpath "/dev"))', profile)
        self.assertIn('(deny mach-lookup (global-name "com.apple.SecurityServer")', profile)
        self.assertIn('securityd|secd', profile)

    def test_xcode_cache_exception_is_only_the_public_cache_leaf(self):
        profile = MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci',
                              '/private/var/folders/ab/synthetic-user/T')
        self.assertIn('xcrun_db', profile)
        self.assertNotIn('(subpath "/private/var/folders', profile)
        with self.assertRaises(RuntimeError):
            MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci', '/Users/example')


class CanaryIdentity(unittest.TestCase):
    def test_exclusive_private_canary_and_original_cleanup(self):
        with tempfile.TemporaryDirectory() as parent:
            canary = MAC.Canary(parent)
            self.assertEqual(canary.directory.stat().st_mode & 0o777, 0o700)
            self.assertEqual(canary.path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(canary.path.read_text(), 'synthetic out-of-sandbox canary')
            self.assertTrue(canary.close())
            self.assertFalse(canary.directory.exists())

    def test_precreated_symlink_never_truncates_the_other_file(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent)
            other = root / 'other'; other.write_text('keep exactly')
            precreated = root / 'precreated'; precreated.mkdir(mode=0o700)
            alias = precreated / 'canary'; alias.symlink_to(other)
            with patch.object(MAC.tempfile, 'mkdtemp', return_value=str(precreated)):
                with self.assertRaises(FileExistsError):
                    MAC.Canary(root)
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertTrue(alias.is_symlink())

    def test_replaced_leaf_is_retained_and_not_followed_during_cleanup(self):
        with tempfile.TemporaryDirectory() as parent:
            other = Path(parent) / 'other'; other.write_text('keep exactly')
            canary = MAC.Canary(parent)
            canary.path.unlink(); canary.path.symlink_to(other)
            self.assertFalse(canary.close())
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertTrue(canary.path.is_symlink())

    def test_replaced_parent_is_not_cleaned_up(self):
        with tempfile.TemporaryDirectory() as parent:
            canary = MAC.Canary(parent)
            original = Path(parent) / 'original'
            canary.directory.rename(original)
            canary.directory.mkdir(mode=0o700)
            other = canary.directory / 'other'; other.write_text('keep exactly')
            self.assertFalse(canary.close())
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertEqual((original / 'canary').read_text(), 'synthetic out-of-sandbox canary')


if __name__ == '__main__':
    unittest.main()
