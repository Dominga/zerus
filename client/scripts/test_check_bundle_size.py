import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("bundle_size", Path(__file__).with_name("check-bundle-size.py"))
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class BundleSizeTest(unittest.TestCase):
    def test_decimal_budget_includes_boundary_and_rejects_overflow(self):
        with tempfile.TemporaryDirectory() as root:
            dist = Path(root)
            (dist / "assets").mkdir()
            (dist / "index.html").write_text('<script type="module" crossorigin src="./assets/index-built.js"></script>')
            entry = dist / "assets/index-built.js"
            entry.write_bytes(b"x" * bundle.BUDGET)
            self.assertEqual(bundle.check(dist), (entry.name, 500_000))
            entry.write_bytes(b"x" * (bundle.BUDGET + 1))
            with self.assertRaisesRegex(ValueError, "500,001 bytes"):
                bundle.check(dist)

    def test_missing_empty_external_or_ambiguous_entries_fail_closed(self):
        with tempfile.TemporaryDirectory() as root:
            dist = Path(root)
            (dist / "empty.js").write_text("")
            for src in ("", "empty.js", "missing.js", "../outside.js", "https://example.com/index.js"):
                (dist / "index.html").write_text(f'<script type="module" src="{src}"></script>')
                with self.subTest(src=src), self.assertRaises(ValueError):
                    bundle.check(dist)
            (dist / "index.html").write_text('<script type="module" src="one.js"></script><script type="module" src="two.js"></script>')
            with self.assertRaisesRegex(ValueError, "exactly one"):
                bundle.check(dist)


if __name__ == "__main__":
    unittest.main()
