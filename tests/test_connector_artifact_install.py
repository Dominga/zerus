"""The maintained connector-only install leaves the CLI and services alone."""
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ConnectorArtifactInstall(unittest.TestCase):
    def setUp(self):
        scratch = ROOT / ".ci-build"
        scratch.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="connector-install-", dir=scratch)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.prefix = self.base / "prefix"
        self.bin = self.prefix / "bin"
        self.bin.mkdir(parents=True)
        self.cli = self.bin / "hgs"
        self.cli.write_bytes(b"preserved CLI")
        self.artifact = self.base / "candidate"
        self.artifact.write_bytes(b"synthetic connector artifact; never executed")
        self.artifact.chmod(0o755)
        self.sha = hashlib.sha256(self.artifact.read_bytes()).hexdigest()
        self.destination = self.bin / "zerus-mobile-connector"
        self.destination.write_bytes(b"previous connector")
        self.destination.chmod(0o755)

    def run_install(self, sha=None):
        environment = {"HOME": str(self.base), "PATH": "/usr/bin:/bin",
                       "HGS_INSTALL_PREFIX": str(self.prefix)}
        return subprocess.run(["bash", str(ROOT / "install.sh"), "--connector-only",
                               str(self.artifact), sha or self.sha], env=environment,
                              capture_output=True, timeout=5)

    def test_verified_artifact_only_and_idempotence(self):
        cli_inode = self.cli.stat().st_ino
        self.assertEqual(self.run_install().returncode, 0)
        self.assertEqual(self.destination.read_bytes(), self.artifact.read_bytes())
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o755)
        inode = self.destination.stat().st_ino
        self.assertEqual(self.run_install().returncode, 0)
        self.assertEqual(self.destination.stat().st_ino, inode)
        self.assertEqual(self.cli.read_bytes(), b"preserved CLI")
        self.assertEqual(self.cli.stat().st_ino, cli_inode)
        self.assertEqual(set(self.prefix.iterdir()), {self.bin})
        self.assertEqual(set(self.bin.iterdir()), {self.cli, self.destination})

    def test_hash_refusal_keeps_installed_bytes(self):
        inode = self.destination.stat().st_ino
        self.assertNotEqual(self.run_install("0" * 64).returncode, 0)
        self.assertEqual(self.destination.read_bytes(), b"previous connector")
        self.assertEqual(self.destination.stat().st_ino, inode)
        self.assertEqual(set(self.bin.iterdir()), {self.cli, self.destination})

    def test_symlink_refusal_keeps_target(self):
        self.destination.unlink()
        self.destination.symlink_to(self.cli)
        self.assertNotEqual(self.run_install().returncode, 0)
        self.assertTrue(self.destination.is_symlink())
        self.assertEqual(self.cli.read_bytes(), b"preserved CLI")


if __name__ == "__main__":
    unittest.main()
