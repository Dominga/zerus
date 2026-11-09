import importlib.util
import json
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/ci/windows-membership-hosted.py"
SPEC = importlib.util.spec_from_file_location("hosted_membership", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class HostedRecord(unittest.TestCase):
    def test_only_one_anchored_hosted_record_can_supply_cost_data(self):
        value = {"scope": "hosted-cost-only", "kind": "usage-timing-data-only-v1"}
        line = MODULE.MARKER + json.dumps(value)
        self.assertEqual(MODULE.record("libtest progress\n" + line + "\n"), value)
        for text in ["", "prefix " + line, line + "\n" + line,
                     MODULE.MARKER + json.dumps(value | {"scope": "console-user"})]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                MODULE.record(text)


if __name__ == "__main__":
    unittest.main()
