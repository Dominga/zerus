import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/ci/windows-membership-artifact.py"
SPEC = importlib.util.spec_from_file_location("membership_artifact", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class QualificationArtifact(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        (self.root / "Cargo.toml").write_text("private fixture")
        self.exe = self.root / "target/release/deps/hgs-1234.exe"
        self.exe.parent.mkdir(parents=True)
        self.exe.write_bytes(b"MZprivate test fixture")

    def record(self, **changes):
        record = {"reason": "compiler-artifact", "manifest_path": str(self.root / "Cargo.toml"),
                  "target": {"name": "hgs", "kind": ["bin"]},
                  "profile": {"test": True, "opt_level": "3", "debug_assertions": False},
                  "executable": str(self.exe)}
        record.update(changes)
        return json.dumps(record)

    def test_only_one_root_test_executable_is_selected(self):
        self.assertEqual(MODULE.select(self.record(), self.root), self.exe)
        for records in ["", self.record() + "\n" + self.record(),
                        self.record(profile={"test": False}),
                        self.record(profile={"test": True, "opt_level": "0", "debug_assertions": False}),
                        self.record(profile={"test": True, "opt_level": "3", "debug_assertions": True}),
                        self.record(profile={"test": True, "opt_level": "3"}),
                        self.record(target={"name": "other", "kind": ["bin"]}),
                        self.record(manifest_path=str(self.root / "other/Cargo.toml"))]:
            with self.subTest(records=records), self.assertRaises(ValueError):
                MODULE.select(records, self.root)

    def test_outside_wrong_type_and_non_pe_sources_refuse(self):
        outside = self.root / "other.exe"
        outside.write_bytes(b"MZother")
        with self.assertRaises(ValueError):
            MODULE.select(self.record(executable=str(outside)), self.root)
        self.exe.write_bytes(b"not Windows")
        with self.assertRaises(ValueError):
            MODULE.select(self.record(), self.root)
        self.exe.unlink()
        self.exe.mkdir()
        with self.assertRaises(ValueError):
            MODULE.select(self.record(), self.root)

    def test_linked_release_directory_refuses(self):
        release = self.root / "target/release"
        original = self.root / "target/actual-release"
        release.rename(original)
        try:
            release.symlink_to(original, target_is_directory=True)
        except OSError:
            self.skipTest("native directory links unavailable")
        with self.assertRaises(ValueError):
            MODULE.select(self.record(), self.root)

    def test_hard_linked_and_symbolic_executables_refuse(self):
        alias = self.exe.with_name("alias.exe")
        try:
            alias.hardlink_to(self.exe)
        except OSError:
            self.skipTest("native hard links unavailable")
        with self.assertRaises(ValueError):
            MODULE.select(self.record(), self.root)
        alias.unlink()
        original = self.exe.with_name("original.exe")
        self.exe.rename(original)
        try:
            self.exe.symlink_to(original)
        except OSError:
            return  # The genuine hard-link assertion above still ran.
        with self.assertRaises(ValueError):
            MODULE.select(self.record(), self.root)

    def test_manifest_binds_source_and_bytes_without_a_release_admission(self):
        output = self.root / "artifact"
        sha = "a" * 40
        MODULE.package(self.exe, sha, output)
        manifest = json.loads((output / "manifest.json").read_text())
        self.assertEqual(manifest["sourceCommit"], sha)
        self.assertEqual(manifest["buildProfile"], "release")
        self.assertIn("platform::windows_membership::native::qualification::private_measurement", manifest["ignoredExactCases"])
        self.assertIn("platform::windows_membership::native::qualification::hosted_measurement", manifest["ignoredExactCases"])
        self.assertEqual(manifest["executable"]["sha256"], MODULE.digest(self.exe))
        self.assertEqual(manifest["executable"]["size"], self.exe.stat().st_size)
        self.assertEqual(manifest["kind"], "read-only-membership-test-fixture")
        self.assertFalse(manifest["inspectionBudgetSelected"])
        self.assertFalse(manifest["removalQualified"])
        self.assertNotIn(str(self.root), json.dumps(manifest))

    def test_changed_copy_or_source_sha_cannot_receive_a_manifest(self):
        for number, sha in enumerate(["main", "A" * 40, "a" * 39]):
            with self.subTest(sha=sha), self.assertRaises(ValueError):
                MODULE.package(self.exe, sha, self.root / f"rejected-{number}")
        def changed(source, target):
            target.write_bytes(b"MZchanged")
        output = self.root / "changed"
        with patch.object(MODULE.shutil, "copyfile", side_effect=changed):
            with self.assertRaises(ValueError):
                MODULE.package(self.exe, "a" * 40, output)
        self.assertFalse((output / "manifest.json").exists())


if __name__ == "__main__":
    unittest.main()
