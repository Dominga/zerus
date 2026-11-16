"""The native4 local entry point audits real Git sources before any build."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/ci/check-teams-base-compatibility.py"
PREVIOUS = "5216b5b290a67e59b65665fa19d545bb3eb2ce5d"
CURRENT = "64eeb5a316f1bca1601b5c732c1d9a2a98d04a52"


def module():
    if not SCRIPT.is_file():
        raise AssertionError("the maintained native4 local entry point is missing")
    spec = importlib.util.spec_from_file_location("teams_base_compatibility", SCRIPT)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


class Native4EntryPoint(unittest.TestCase):
    def test_inspection_reports_both_actual_native4_sources_without_building(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "unused-target"
            env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--inspect"],
                cwd=ROOT, env=env, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            data = json.loads(result.stdout)
            self.assertEqual(data["contract"], "native4-capable-interoperability")
            self.assertEqual(data["sources"]["previous"]["commit"], PREVIOUS)
            self.assertEqual(data["sources"]["current"]["commit"], CURRENT)
            for source in data["sources"].values():
                self.assertEqual(source["core"], 4)
                self.assertTrue(source["base_pin_capability"])
                self.assertTrue(source["cached_execution_fence"])
            self.assertFalse(target.exists(), "inspection must not prepare or build an image")

    def test_actual_historical_core2_is_refused_as_a_native4_substitute(self):
        entry = module()
        with self.assertRaisesRegex(entry.ContractError, "native4"):
            entry.audit_source(ROOT, "34c231f26399229828f869dfdfbac1cb4ab0cd4e")

    def test_actual_admitted_predecessor_has_the_capability_not_a_missing_function(self):
        info = module().audit_source(ROOT, PREVIOUS)
        self.assertEqual(info["core"], 4)
        self.assertTrue(info["base_pin_capability"])
        self.assertTrue(info["cached_execution_fence"])

    @unittest.skipUnless(os.name == "posix", "executable shared wrappers use POSIX argv")
    def test_explicit_steps_honor_the_shared_wrapper_before_running_a_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            marker = root / "called.json"
            wrapper = root / "wrapper"
            wrapper.write_text(
                f"#!{sys.executable}\nimport json,sys\n"
                f"open({str(marker)!r}, 'w').write(json.dumps(sys.argv[1:]))\n"
                "sys.exit(73)\n"
            )
            wrapper.chmod(0o700)
            cargo = root / "cargo"
            cargo.write_text(f"#!{sys.executable}\nimport sys\nsys.exit(74)\n")
            cargo.chmod(0o700)
            env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"]}
            for step in ["build-previous", "build-current", "run", "lint-current"]:
                with self.subTest(step=step):
                    marker.unlink(missing_ok=True)
                    result = subprocess.run(
                        [sys.executable, str(SCRIPT), "--step", step,
                         "--heavy-wrapper", str(wrapper), "--evidence", str(root / "evidence")],
                        cwd=ROOT, env=env, capture_output=True, text=True,
                    )
                    self.assertTrue(marker.exists(), "an explicit step must acquire its shared wrapper")
                    self.assertNotEqual(result.returncode, 0)
                    command = json.loads(marker.read_text())
                    self.assertEqual(command[command.index("--step") + 1], step)
                    self.assertNotIn("--heavy-wrapper", command, "one lease per worker; no nested wrapper")

    def test_run_refuses_absent_image_seals_without_a_fallback_build(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / "evidence"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--step", "run", "--evidence", str(evidence)],
                cwd=ROOT, capture_output=True, text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Build both native4 images before run", result.stderr)
            self.assertNotIn("Compiling", result.stderr)


if __name__ == "__main__":
    unittest.main()
