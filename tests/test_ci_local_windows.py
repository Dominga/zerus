"""Windows local planning must not turn unexecuted checks green."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('local_windows_ci', ROOT / 'scripts/ci/local-windows.py')
WINDOWS = importlib.util.module_from_spec(SPEC)
with patch.object(sys, 'path', [str(ROOT / 'scripts/ci'), *sys.path]):
    SPEC.loader.exec_module(WINDOWS)


def rows(steps, **fields):
    job = {'runs-on': 'windows-2025', 'steps': steps, **fields}
    return WINDOWS.plan({'jobs': {'windows-tests': job}})[0]['steps']


class WorkflowBodies(unittest.TestCase):
    def test_mixed_infrastructure_keeps_its_actual_body(self):
        body = WINDOWS.CI.RUST_CACHE.format(toolchain='') + '\nexit 19\n'
        result = rows([{'name': 'Rust cache identity', 'run': body}])[0]
        self.assertEqual(result['body'], body)
        self.assertEqual(result['status'], 'not run')
        mixed = rows([{'uses': 'actions/checkout@fixed', 'run': 'exit 19'}])[0]
        self.assertNotIn('body', mixed)
        self.assertNotEqual(mixed['status'], 'local')

    def test_owner_machine_exclusions_are_unexecuted(self):
        for body in ['python scripts/ci/outside-job.py --custody-only -- true',
                     'bash scripts/ci/windows-next-shell.sh',
                     'bash scripts/ci/client-native.sh windows core',
                     'python scripts/ci/ensure-webview2.py']:
            with self.subTest(body=body):
                result = rows([{'run': body}])[0]
                self.assertNotIn('body', result)
                self.assertEqual(result['status'], 'not run')

    def test_only_the_fixed_workspace_binding_is_accepted(self):
        step = {'run': 'cargo build', 'env': {'CARGO_TARGET_DIR': '${{ github.workspace }}/target/windows-next'}}
        self.assertIn('body', rows([step])[0])
        step['env']['CARGO_TARGET_DIR'] = '${{ secrets.other }}'
        self.assertNotIn('body', rows([step])[0])
        step['env'] = {'HGS_CONFIG_DIR': 'owner'}
        self.assertNotIn('body', rows([step])[0])

    def test_stale_or_replaced_executables_do_not_satisfy_a_producer(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory); binary = target / 'hgs.exe'; binary.write_bytes(b'original')
            record = {'commit': 'a' * 40, 'files': {'hgs.exe': WINDOWS.CI.file_digest(binary)}}
            self.assertTrue(WINDOWS.producer_ready(record, 'a' * 40, target))
            self.assertFalse(WINDOWS.producer_ready(record, 'b' * 40, target))
            binary.write_bytes(b'replaced')
            self.assertFalse(WINDOWS.producer_ready(record, 'a' * 40, target))
            binary.unlink()
            self.assertFalse(WINDOWS.producer_ready(record, 'a' * 40, target))
            self.assertFalse(WINDOWS.producer_ready(None, 'a' * 40, target))

    def test_conditions_and_tool_versions_never_guess(self):
        self.assertNotIn('body', rows([{'run': 'true', 'if': 'new_condition'}])[0])
        result = rows([{'uses': 'actions/setup-python@fixed', 'with': {'python-version': '3.14'}}])[0]
        self.assertEqual(result['status'], 'not run')
        jobs = WINDOWS.plan({'jobs': {'new-windows-job': {'runs-on': 'windows-2025', 'steps': [{'run': 'true'}]}}})
        self.assertEqual(jobs[0]['status'], 'not run')


if __name__ == '__main__':
    unittest.main()
