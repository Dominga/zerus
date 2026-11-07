"""All desktop bundles use one locked frontend runtime notice set."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("collect_notices", Path(__file__).with_name("collect-notices.py"))
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class CollectNotices(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zerus-notices-")
        self.addCleanup(self.temp.cleanup)
        self.client = Path(self.temp.name) / "client"
        self.client.mkdir()
        self.output = Path(self.temp.name) / "notices"
        self.packages = {
            "": {"version": "1.0.0"},
            "node_modules/@example/runtime": {"version": "1.2.3"},
            "node_modules/vite": {"version": "8.3.4", "dev": True},
            "node_modules/dev-tool": {"version": "4.0.0", "dev": True},
            "node_modules/optional-runtime": {"version": "1.0.0", "optional": True},
        }
        for name, version in (("@example/runtime", "1.2.3"), ("vite", "8.3.4")):
            folder = self.client / "node_modules" / name
            folder.mkdir(parents=True)
            (folder / "package.json").write_text(json.dumps(dict(name=name, version=version, license="MIT")))
            (folder / "LICENSE").write_text(f"exact {name} license\n")
        self.lock()

    def lock(self):
        (self.client / "package-lock.json").write_text(json.dumps({"packages": self.packages}))

    def test_vite_runtime_helper_notice_ships_beside_production_notices_on_every_platform(self):
        result = collector.collect(self.client, self.output)
        self.assertEqual([row["name"] for row in result], ["@example/runtime", "vite"])
        self.assertEqual(json.loads((self.output / "inventory.json").read_text()), {"packages": result})
        self.assertEqual((self.output / "vite-8.3.4/LICENSE").read_text(), "exact vite license\n")
        self.assertEqual((self.output / "@example-runtime-1.2.3/LICENSE").read_text(), "exact @example/runtime license\n")

    def test_stale_installed_versions_and_missing_runtime_notices_refuse(self):
        metadata = self.client / "node_modules/@example/runtime/package.json"
        original = metadata.read_bytes()
        metadata.write_text(json.dumps(dict(name="@example/runtime", version="1.2.2")))
        with self.assertRaisesRegex(ValueError, "differs from the lock"):
            collector.collect(self.client, self.output)
        metadata.write_bytes(original)
        (metadata.parent / "LICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "No license text"):
            collector.collect(self.client, self.output)

    def test_parent_and_absolute_dependency_paths_are_refused(self):
        for path in ("../outside", "/absolute", "node_modules/../../outside"):
            self.packages = {path: {"version": "1.0.0"}}
            self.lock()
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "Unexpected frontend dependency path"):
                collector.collect(self.client, self.output)

    def test_linked_workspaces_are_not_dependency_notices(self):
        self.packages["node_modules/workspace"] = {"link": True}
        self.lock()
        self.assertEqual(len(collector.collect(self.client, self.output)), 2)

    def test_bundled_logo_notice_ships_with_the_inventory_and_is_required(self):
        assets = (("lobe-icons", "agent-logos", "MIT", "src/assets/agent-icons/LICENSE"),)
        with self.assertRaisesRegex(ValueError, "No license text found for bundled lobe-icons"):
            collector.collect(self.client, self.output, assets)
        notice = self.client / assets[0][3]
        notice.parent.mkdir(parents=True)
        notice.write_text("MIT License\n\nCopyright (c) 2023 LobeHub\n")
        rows = collector.collect(self.client, self.output, assets)
        self.assertEqual(rows[-1], {"name": "lobe-icons", "version": "agent-logos", "license": "MIT", "notices": ["LICENSE"]})
        self.assertIn("LobeHub", (self.output / "lobe-icons-agent-logos/LICENSE").read_text())
        # The macOS bundle check looks for <name>-<version> beside the inventory.
        self.assertEqual(json.loads((self.output / "inventory.json").read_text())["packages"][-1]["version"], "agent-logos")

    def test_the_shipped_collector_includes_the_provider_logo_notice(self):
        self.assertEqual([row[0] for row in collector.BUNDLED_ASSETS], ["lobe-icons"])
        self.assertTrue((collector.ROOT / collector.BUNDLED_ASSETS[0][3]).is_file())


if __name__ == "__main__":
    unittest.main()
