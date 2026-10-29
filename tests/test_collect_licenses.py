"""The reviewed monorepo license exception is exact-source and fail-closed."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("collect_licenses", ROOT / "scripts/collect-licenses.py")
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class ReviewedMonorepoLicense(unittest.TestCase):
    def package(self):
        return {"name": "canonicalpath-rust", "version": "2026.5.18-2",
                "source": collector.CANONICALPATH_SOURCE}

    def test_exact_revision_carries_the_complete_retained_license(self):
        text, entry = collector.reviewed_monorepo_notice(self.package())
        self.assertEqual(text, (ROOT / "docs/licenses/canonicalpath-LICENSE.txt").read_bytes())
        self.assertIn(b"Copyright (c) 2026 Roman Ilyin", text)
        self.assertEqual(entry["license"], "MIT")

    def test_unreviewed_version_or_source_cannot_reuse_the_exception(self):
        for key, value in [("version", "2027.1.1"), ("source", "registry+example.invalid"),
                           ("source", collector.CANONICALPATH_SOURCE.replace("50b390", "000000"))]:
            package = self.package()
            package[key] = value
            with self.assertRaises(SystemExit):
                collector.reviewed_monorepo_notice(package)

    def test_modified_or_missing_license_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(collector, "ROOT", root):
                with self.assertRaises(FileNotFoundError):
                    collector.reviewed_monorepo_notice(self.package())
                target = root / "docs/licenses/canonicalpath-LICENSE.txt"
                target.parent.mkdir(parents=True)
                target.write_text("MIT only is not the full license", encoding="utf-8")
                with self.assertRaises(SystemExit):
                    collector.reviewed_monorepo_notice(self.package())

    def test_other_dependencies_still_use_normal_notice_collection(self):
        self.assertIsNone(collector.reviewed_monorepo_notice({"name": "another-crate"}))


if __name__ == "__main__":
    unittest.main()
