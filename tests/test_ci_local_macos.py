"""Mac planning must preserve checks and the local producer's exact boundary."""
import importlib.util
from pathlib import Path
import sys
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

    def test_signal_and_file_scope_cannot_expand_to_the_owner_home(self):
        profile = MAC.profile('/Users/example/zerus-dev/private-ci', ['/public-git/objects'], '/private/tmp/owned-ci')
        self.assertIn('(allow signal (target same-sandbox))', profile)
        self.assertNotIn('(allow signal)', profile)
        self.assertNotIn('(subpath "/Users/example")', profile)
        self.assertIn('(literal "/private/tmp")', profile)
        self.assertNotIn('(subpath "/private/tmp")', profile)

    def test_xcode_cache_exception_is_only_the_public_cache_leaf(self):
        profile = MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci',
                              '/private/var/folders/ab/synthetic-user/T')
        self.assertIn('xcrun_db', profile)
        self.assertNotIn('(subpath "/private/var/folders', profile)
        with self.assertRaises(RuntimeError):
            MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci', '/Users/example')


if __name__ == '__main__':
    unittest.main()
