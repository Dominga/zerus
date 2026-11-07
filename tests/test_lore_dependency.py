"""Task 256: lore has one source, github.com/WhiteWh/CodeLore, pinned by the wrapper package.

The package scripts ask scripts/lore_dependency.py which lore they ship; the pin in
crates/lore-bin/Cargo.toml and the one in its Cargo.lock must name the same commit, and
Zerus keeps no copy of the crate."""
import re
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import lore_dependency  # noqa: E402

URL = "https://github.com/WhiteWh/CodeLore"


class LoreDependency(unittest.TestCase):
    def read(self, relative):
        return (ROOT / relative).read_text(encoding="utf-8").replace("\r\n", "\n")

    def test_the_wrapper_pins_one_codelore_commit_in_its_manifest_and_its_lock(self):
        manifest = self.read(lore_dependency.MANIFEST)
        dependency = re.search(r'^zerus-lore = \{ package = "codelore", git = "([^"]+)", rev = "([0-9a-f]{40})" \}$', manifest, re.M)
        self.assertIsNotNone(dependency, "crates/lore-bin names zerus-lore as the codelore package at a full revision")
        self.assertEqual(dependency.group(1), URL)
        lock = self.read(lore_dependency.LOCK)
        source = re.search(r'name = "codelore"\nversion = "[^"]+"\nsource = "git\+([^?"]+)\?rev=([0-9a-f]{40})#([0-9a-f]{40})"', lock)
        self.assertIsNotNone(source, "the lock pins codelore from its git source")
        self.assertEqual((source.group(1), source.group(2), source.group(3)), (URL, dependency.group(2), dependency.group(2)))

    def test_the_shipped_version_is_the_pinned_codelore_version(self):
        lock = self.read(lore_dependency.LOCK)
        version = re.search(r'name = "codelore"\nversion = "([^"]+)"', lock).group(1)
        self.assertEqual(lore_dependency.lore_version(ROOT), version)

    def test_a_lock_without_codelore_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            lock = Path(root) / lore_dependency.LOCK
            lock.parent.mkdir(parents=True)
            lock.write_text('[[package]]\nname = "serde"\nversion = "1.0.0"\n', encoding="utf-8")
            with self.assertRaises(SystemExit):
                lore_dependency.lore_version(Path(root))
            lock.write_text('[[package]]\r\nname = "codelore"\r\nversion = "9.8.7"\r\n', encoding="utf-8", newline="")
            self.assertEqual(lore_dependency.lore_version(Path(root)), "9.8.7")

    def test_zerus_keeps_no_copy_of_the_crate_and_hgs_does_not_link_it(self):
        self.assertFalse((ROOT / "crates/lore").exists(), "crates/lore is gone: CodeLore is the one source")
        # Linking it would unify serde_json's preserve_order into hgs (docs/lore.md 2.1).
        self.assertNotIn("codelore", self.read("Cargo.toml"))
        self.assertNotIn("zerus-lore", self.read("Cargo.toml"))

    def test_no_script_or_workflow_builds_the_removed_crate(self):
        offenders = []
        for folder in ("scripts", "packaging", ".github", "tray"):
            for path in (ROOT / folder).rglob("*"):
                if path.is_file() and path.suffix in {".py", ".sh", ".yml", ".in", ""} and "crates/lore/" in path.read_text(encoding="utf-8", errors="ignore"):
                    offenders.append(str(path.relative_to(ROOT)))
        for name in ("install.sh", "mac-install.sh"):
            if "crates/lore/" in self.read(name):
                offenders.append(name)
        self.assertEqual(offenders, [])


if __name__ == "__main__":
    unittest.main()
