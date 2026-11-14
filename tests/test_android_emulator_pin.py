"""Archive and provisioning controls only; never launch an emulator or use the SDK."""
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("android_emulator_driver", ROOT / "scripts/ci/android-project-ui.py")
UI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UI)


class PinnedEmulatorTests(unittest.TestCase):
    def archive(self, root, *, revision="37.2.12", extra=None):
        archive = root / "emulator.zip"
        with zipfile.ZipFile(archive, "w") as output:
            for name, data, mode in [
                ("emulator/source.properties", f"Pkg.Revision={revision}\nPkg.Path=emulator\nPkg.BuildId=16428233\n", stat.S_IFREG | 0o644),
                ("emulator/emulator", "synthetic binary; never execute", stat.S_IFREG | 0o755),
                *([extra] if extra else []),
            ]:
                entry = zipfile.ZipInfo(name)
                entry.external_attr = mode << 16
                output.writestr(entry, data)
        raw = archive.read_bytes()
        selected = {"revision": "37.2.12", "archive": {"bytes": len(raw), "checksum_type": "sha1",
            "checksum": hashlib.sha1(raw).hexdigest(), "sha256": hashlib.sha256(raw).hexdigest()}}
        return archive, selected

    def test_verified_archive_installs_its_actual_revision_and_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); archive, selected = self.archive(root)
            installed = UI.install_emulator(archive, root / "owned", selected)
            self.assertTrue((installed / "source.properties").is_file())
            self.assertEqual(UI.revision(installed / "source.properties"), "37.2.12")
            self.assertEqual((installed / "emulator").stat().st_mode & 0o777, 0o700)
            self.assertFalse((installed / "package.xml").exists(), "Do not synthesize SDK manager evidence")

    def test_size_and_both_checksums_are_verified_before_creating_destination(self):
        for change in ("bytes", "checksum", "sha256"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); archive, selected = self.archive(root)
                selected["archive"][change] = 1 if change == "bytes" else "0" * (40 if change == "checksum" else 64)
                destination = root / "owned"
                with self.assertRaises(AssertionError):
                    UI.install_emulator(archive, destination, selected)
                self.assertFalse(destination.exists())

    def test_wrong_archive_revision_cannot_be_claimed_as_the_selected_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); archive, selected = self.archive(root, revision="37.2.13")
            with self.assertRaises(AssertionError):
                UI.install_emulator(archive, root / "owned", selected)

    def test_archive_paths_types_duplicates_and_expansion_are_bounded(self):
        for name, mode in (("../escaped", stat.S_IFREG | 0o644), ("/escaped", stat.S_IFREG | 0o644),
                           ("other/file", stat.S_IFREG | 0o644), ("emulator/../escaped", stat.S_IFREG | 0o644),
                           ("emulator/link", stat.S_IFLNK | 0o777), ("emulator/source.properties", stat.S_IFREG | 0o644)):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); archive, selected = self.archive(root, extra=(name, "untrusted", mode))
                with self.assertRaises(AssertionError):
                    UI.install_emulator(archive, root / "owned", selected)
                self.assertFalse((root / "escaped").exists())

    def test_existing_destination_is_never_reused_or_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); archive, selected = self.archive(root)
            destination = root / "owned"; destination.mkdir(); marker = destination / "keep"; marker.write_text("retained")
            with self.assertRaises(AssertionError):
                UI.install_emulator(archive, destination, selected)
            self.assertEqual(marker.read_text(), "retained")

    def test_archive_expansion_and_entry_count_refuse_before_extraction(self):
        for entries in ([SimpleNamespace(file_size=1024 ** 3 + 1)], [SimpleNamespace(file_size=0)] * 1025):
            with self.subTest(count=len(entries)), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); archive, selected = self.archive(root)
                with patch.object(UI.zipfile.ZipFile, "infolist", return_value=entries), self.assertRaises(AssertionError):
                    UI.install_emulator(archive, root / "owned", selected)
                self.assertFalse((root / "owned").exists())

    def test_runtime_must_report_the_single_selected_revision_and_build(self):
        valid = "Android emulator version 37.2.12.0 (build_id 16428233) (CL:N/A)\nCopyright\n"
        UI.emulator_runtime_version(valid)
        for text in ("", valid.replace("37.2.12", "37.2.13"), valid.replace("16428233", "16428234"), valid * 2, "x" * 65537):
            with self.subTest(text=text[:100]), self.assertRaises(AssertionError):
                UI.emulator_runtime_version(text)

    def test_driver_selects_archive_before_runtime_and_never_requests_unversioned_emulator(self):
        source = (ROOT / "scripts/ci/android-project-ui.py").read_text()
        tree = ast.parse(source)
        request = next(node for node in ast.walk(tree) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            and node.func.id == "run" and node.args and isinstance(node.args[0], ast.Constant) and node.args[0].value == "sdk-packages")
        self.assertNotIn("emulator", [node.value for node in ast.walk(request) if isinstance(node, ast.Constant)])
        self.assertIn('assert revision(emulator_root / "source.properties") == os.environ["EXPECTED_EMULATOR_REVISION"]', source)
        self.assertIn('run("emulator-version", [emulator, "-version"]', source)
        self.assertIn('emulator_runtime_version(version)', source)
        main = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "main")
        calls = [node for node in ast.walk(main) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)]
        install = next(node for node in calls if node.func.id == "install_emulator")
        stages = {node.args[0].value: node.lineno for node in calls if node.func.id == "run" and node.args and isinstance(node.args[0], ast.Constant)}
        self.assertLess(stages["emulator-download"], install.lineno)
        self.assertLess(install.lineno, stages["emulator-version"])
        self.assertLess(stages["emulator-version"], stages["apk-build"])
        pins = json.loads((ROOT / "scripts/ci/android-project-ui-pins.json").read_text())
        self.assertEqual(pins["packages"]["emulator"]["revision"], "37.2.12")
        self.assertEqual(pins["packages"][UI.IMAGE]["revision"], "9.0.0")
        self.assertEqual(pins["packages"]["emulator"]["archive"]["sha256"],
            "b08fc43d8608d2955f607f1b287beb525041e730086bdca3af152accac3af9c1")
        self.assertEqual(pins["packages"]["emulator"]["archive"]["checksum"], "cd7362ea55dfb86a418958138dc396e74165dd01")
        self.assertEqual(pins["packages"]["emulator"]["archive"]["bytes"], 349654171)
        self.assertEqual(pins["packages"]["emulator"]["archive"]["url"],
            "https://dl.google.com/android/repository/emulator-linux_x64-16428233.zip")


if __name__ == "__main__":
    unittest.main()
