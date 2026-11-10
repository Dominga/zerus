"""The web UI separator rule excludes only its shared XML generator input."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FORBIDDEN = "\u00b7\u2022\u2219\u22c5\u2027\u2981\u30fb"


class UISeparators(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="zerus-ui-separators-")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.scanner = self.root / "scripts/check-ui-separators.py"
        self.scanner.parent.mkdir()
        shutil.copyfile(ROOT / "scripts/check-ui-separators.py", self.scanner)

    def write(self, relative, value):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value, encoding="utf-8")

    def check(self):
        return subprocess.run([sys.executable, str(self.scanner)], cwd=self.root,
                              env=dict(os.environ, PYTHONIOENCODING="utf-8"),
                              stdin=subprocess.DEVNULL, capture_output=True,
                              text=True, encoding="utf-8", timeout=10)

    def test_the_shared_catalog_is_scanned_like_every_ui_source(self):
        # Task #28 (team ede724c4): the retired Qt XML catalog had its own
        # exemption; the shared JSON catalog has none.
        self.write("client/i18n/shared/zerus_ru.json", json.dumps({"Example": "left · right"}, ensure_ascii=False))
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("client/i18n/shared/zerus_ru.json", result.stdout.replace("\\", "/"))

    def test_generated_runtime_catalog_rejects_every_forbidden_character(self):
        for char in FORBIDDEN:
            with self.subTest(char=hex(ord(char))):
                self.write("client/src/i18n.ru.json", json.dumps({"Example": "left " + char + " right"}, ensure_ascii=False))
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("client/src/i18n.ru.json", result.stdout.replace("\\", "/"))
                self.assertIn(f"U+{ord(char):04X}", result.stdout)

    def test_ui_sources_and_design_sources_remain_covered(self):
        for path in ("client/src/Example.ts", "client/src/Example.tsx", "client/core/src/layout.rs", "design/webui/src/Example.tsx"):
            with self.subTest(path=path):
                self.write(path, 'const row = "left · right";')
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(path, result.stdout.replace("\\", "/"))

    def test_no_basename_directory_or_suffix_exemption(self):
        for path in ("client/src/zerus_ru.ts", "client/i18n/shared/copied_ru.ts",
                     "client/other/i18n/shared/zerus_ru.ts"):
            with self.subTest(path=path):
                self.write(path, '<TS><source>left · right</source></TS>')
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(path, result.stdout.replace("\\", "/"))


if __name__ == "__main__":
    unittest.main()
