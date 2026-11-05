"""The same bounded Service payload is staged for local and public Mac apps."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("service_payload", ROOT / "scripts/macos_service_payload.py")
payload = importlib.util.module_from_spec(spec)
spec.loader.exec_module(payload)


@unittest.skipIf(os.name == "nt", "Mac payload staging uses Unix file semantics")
class ServicePayload(unittest.TestCase):
    def test_packaged_helper_imports_only_its_staged_dependencies(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Zerus.app"
            app.mkdir()
            payload.stage(ROOT, app)
            helper = app / payload.SERVICE_RESOURCES[0]
            # --help imports both sibling modules without starting any service.
            result = subprocess.run([sys.executable, "-I", "-B", str(helper), "--help"],
                                    cwd=temporary, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("--mode", result.stdout)
            self.assertEqual(payload.findings(app), [])

    def test_missing_empty_or_linked_source_cannot_produce_a_pair(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source/scripts"
            source.mkdir(parents=True)
            for name in payload.SERVICE_SCRIPTS:
                (source / name).write_bytes((ROOT / "scripts" / name).read_bytes())
            target = source / payload.SERVICE_SCRIPTS[0]
            for kind in ("missing", "empty", "link"):
                with self.subTest(kind=kind):
                    target.unlink(missing_ok=True)
                    if kind == "empty": target.write_bytes(b"")
                    elif kind == "link": target.symlink_to(ROOT / "scripts" / target.name)
                    app = root / kind
                    app.mkdir()
                    with self.assertRaises((ValueError, OSError)):
                        payload.stage(source.parent, app)
                    self.assertTrue(payload.findings(app))

    def test_staging_never_follows_a_setup_directory_link(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            app, external = root / "Zerus.app", root / "external"
            external.mkdir()
            resources = app / "Contents/Resources"
            resources.mkdir(parents=True)
            (resources / "setup").symlink_to(external, target_is_directory=True)
            with self.assertRaises(ValueError): payload.stage(ROOT, app)
            self.assertEqual(list(external.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
