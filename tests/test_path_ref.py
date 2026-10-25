"""Black-box PathRef JSON/CLI boundaries; never touches described paths."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / ("target/debug/hgs.exe" if os.name == "nt" else "target/debug/hgs")))

@unittest.skipUnless(HGS.is_file(), "built hgs required")
class PathRefCli(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zerus-path-ref-")
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.env = {k:v for k,v in os.environ.items() if not k.startswith("HGS_")}
        self.env.update(HOME=str(root), USERPROFILE=str(root), HGS_CONFIG_DIR=str(root/"config"),
                        HGS_STATE_DIR=str(root/"state"), HGS_SELF="fixture", HGS_PEERS="")
        self.path = {"machine_id":"workstation-a","environment_id":"windows-native",
                     "path_flavor":"windows","native_path":r"C:\Projects\Orbit\世界 %20",
                     "workspace_id":"orbit"}
        self.mapping = {"mapping_id":"orbit-drive","workspace_id":"orbit","machine_id":"workstation-a",
                        "windows_environment_id":"windows-native","wsl_environment_id":"wsl:Ubuntu-24.04:alice",
                        "distribution":"Ubuntu-24.04","user":"alice","mount_root":"/mnt",
                        "windows_root":r"C:\Projects\Orbit"}

    def run_cli(self, action, value):
        data = value if isinstance(value, bytes) else json.dumps(value, ensure_ascii=False).encode("utf-8")
        return subprocess.run([str(HGS), "path-ref", action], input=data, capture_output=True,
                              env=self.env, timeout=10)

    def ok(self, action, value):
        result = self.run_cli(action, value)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        return json.loads(result.stdout)

    def test_describe_and_validate_recompute_identity(self):
        ref = self.ok("describe", {"path":self.path})
        self.assertEqual(ref["canonical_path"],"c:/Projects/Orbit/世界 %20")
        self.assertEqual(ref["native_path"],self.path["native_path"])
        self.assertEqual(self.ok("validate", {"path":ref}), ref)
        ref["canonical_path"] = "c:/Other"
        self.assertNotEqual(self.run_cli("validate", {"path":ref}).returncode,0)

    def test_explicit_mapping_round_trip_and_wrong_namespace_denial(self):
        ref = self.ok("describe", {"path":self.path})
        wsl = self.ok("map", {"path":ref,"mapping":self.mapping})
        self.assertEqual(wsl["native_path"],"/mnt/c/Projects/Orbit/世界 %20")
        back = self.ok("map", {"path":wsl,"mapping":self.mapping})
        self.assertEqual(back["native_path"],ref["native_path"])
        wsl["environment_id"] = "wsl:Ubuntu-24.04:bob"
        self.assertNotEqual(self.run_cli("map", {"path":wsl,"mapping":self.mapping}).returncode,0)

    def test_native_posix_drive_mount_is_not_implicitly_windows(self):
        value = dict(self.path, environment_id="linux-a", path_flavor="posix", native_path="/mnt/c/Projects/Orbit")
        ref = self.ok("describe", {"path":value})
        self.assertEqual(ref["canonical_path"],value["native_path"])
        self.assertIsNone(ref["mapping_id"])

    def test_unknown_fields_bad_json_and_oversized_requests_fail(self):
        for request in [b"{", {"path":self.path,"unknown":True}, b" " * (64*1024+1)]:
            self.assertNotEqual(self.run_cli("describe", request).returncode,0)
        self.assertNotEqual(self.run_cli("unknown", {"path":self.path}).returncode,0)

    def test_nul_relative_path_and_future_schema_fail(self):
        for raw in ["C:relative", "relative", "C:\\bad\0path"]:
            self.assertNotEqual(self.run_cli("describe", {"path":dict(self.path,native_path=raw)}).returncode,0)
        ref = self.ok("describe", {"path":self.path})
        ref["schema_version"] = 2
        self.assertNotEqual(self.run_cli("validate", {"path":ref}).returncode,0)

if __name__ == "__main__":
    unittest.main()
