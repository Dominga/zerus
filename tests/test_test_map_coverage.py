"""U62: the weekly coverage run of the test map (scripts/ci/test-map-coverage.py)."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts' / 'ci' / 'test-map-coverage.py'
WORKFLOW = ROOT / '.github' / 'workflows' / 'test-map-coverage.yml'


def load():
    spec = importlib.util.spec_from_file_location('test_map_coverage', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TestMapCoverage(unittest.TestCase):
    def test_every_measured_target_writes_its_own_report(self):
        coverage = load()
        test_map = json.loads((ROOT / '.zerus' / 'test-map.json').read_text(encoding='utf-8'))
        for provider in ('rust', 'tray'):
            targets = test_map['providers'][provider]['targets']
            names = [coverage.safe(target) for target in targets]
            with self.subTest(provider=provider):
                self.assertTrue(targets)
                self.assertEqual(len(names), len(set(names)))
                self.assertTrue(all(name and '/' not in name and ' ' not in name for name in names))

    def test_the_weekly_workflow_measures_both_providers_and_only_reports_gaps(self):
        text = WORKFLOW.read_text(encoding='utf-8')
        self.assertIn("cron: '23 2 * * 0'", text)
        # The monthly report of tests without unique coverage.
        self.assertIn("cron: '47 3 1 * *'", text)
        self.assertIn('without unique coverage', SCRIPT.read_text(encoding='utf-8'))
        self.assertIn('test-map-coverage.py rust --out artifacts/coverage', text)
        self.assertIn('test-map-coverage.py tray --build build-coverage --out artifacts/coverage', text)
        self.assertNotIn('--fail-on-gaps', text)
        self.assertNotIn('actions/cache', text)
        self.assertNotIn('secrets.', text)


if __name__ == '__main__':
    unittest.main()
