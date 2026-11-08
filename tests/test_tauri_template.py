"""Task #280: the Tauri launch template refuses the switch identity."""
import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "tauri_template.py"
spec = importlib.util.spec_from_file_location("tauri_template", SCRIPT)
tauri_template = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tauri_template)


class LaunchIdentityTest(unittest.TestCase):
    def test_development_build_may_launch(self):
        self.assertTrue(tauri_template.identity_allows_launch({
            "buildKind": "development", "CFBundleIdentifier": "dev.zerus.next.d593d5a40f84",
        }))

    def test_switch_build_is_refused(self):
        self.assertFalse(tauri_template.identity_allows_launch({
            "buildKind": "switch", "CFBundleIdentifier": "com.hgdev.hgs-tray",
        }))

    def test_switch_identifier_is_refused_even_with_a_development_kind(self):
        self.assertFalse(tauri_template.identity_allows_launch({
            "buildKind": "development", "CFBundleIdentifier": "com.hgdev.hgs-tray",
        }))

    def test_old_build_without_a_kind_is_refused(self):
        self.assertFalse(tauri_template.identity_allows_launch({
            "CFBundleIdentifier": "dev.zerus.next.d593d5a40f84",
        }))

    def test_package_step_names_the_switch_identity(self):
        package = (SCRIPT.parent / "release-candidate-local.py").read_text(encoding="utf-8")
        self.assertIn("tauri.windows.switch.conf.json", package)


class RunResolvesTheProgramTest(unittest.TestCase):
    """Task 205: npm is npm.cmd on Windows; CreateProcess needs the full name."""

    def test_the_program_is_resolved_like_the_shell_does(self):
        calls = []
        which, call = tauri_template.shutil.which, tauri_template.subprocess.call
        tauri_template.shutil.which = lambda name: "C:/Program Files/nodejs/npm.cmd" if name == "npm" else None
        tauri_template.subprocess.call = lambda argv, cwd=None: calls.append(argv) or 0
        try:
            self.assertEqual(tauri_template.run(["npm", "--prefix", "client", "run", "tauri"]), 0)
            self.assertEqual(tauri_template.run(["not-on-path", "x"]), 0)
        finally:
            tauri_template.shutil.which, tauri_template.subprocess.call = which, call
        self.assertEqual(calls, [["C:/Program Files/nodejs/npm.cmd", "--prefix", "client", "run", "tauri"], ["not-on-path", "x"]])


if __name__ == "__main__":
    unittest.main()
