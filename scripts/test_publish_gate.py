#!/usr/bin/env python3
"""Task 157: the publish gate refuses without both green legs."""
import json
import tempfile
import unittest
from pathlib import Path

from publish_whitewh_main import gate


class PublishGate(unittest.TestCase):
    def record(self, data):
        directory = tempfile.TemporaryDirectory()
        path = Path(directory.name) / "abc.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        self.addCleanup(directory.cleanup)
        return path

    def test_both_green_for_this_commit_publishes(self):
        ok, errors = gate(self.record({"commit": "abc", "local": {"status": "green"}, "experimental": {"status": "green"}}), "abc")
        self.assertTrue(ok, errors)

    def test_each_missing_or_failed_leg_refuses(self):
        for data in (
            {"commit": "abc", "local": {"status": "green"}},
            {"commit": "abc", "local": {"status": "failed"}, "experimental": {"status": "green"}},
            {"commit": "other", "local": {"status": "green"}, "experimental": {"status": "green"}},
        ):
            ok, errors = gate(self.record(data), "abc")
            self.assertFalse(ok, data)
            self.assertTrue(errors)

    def test_a_missing_evidence_file_refuses(self):
        ok, errors = gate(Path(tempfile.gettempdir()) / "definitely-not-a-release-record.json", "abc")
        self.assertFalse(ok)
        self.assertIn("no release-candidate evidence", errors[0])


if __name__ == "__main__":
    unittest.main()
