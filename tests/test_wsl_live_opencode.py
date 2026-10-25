"""Opt-in, read-only acceptance against an owner-started OpenCode service.
ZERUS_TEST_WSL_LIVE=1, ZERUS_TEST_WSL_DISTRO, ZERUS_TEST_WSL_USER,
ZERUS_TEST_WSL_HGS and HGS_TEST_BIN explicitly select the two executables.
This suite never creates a native session, sends a prompt or manages a service.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "nt" and os.environ.get("ZERUS_TEST_WSL_LIVE") == "1",
                     "explicit owner-managed Windows/WSL acceptance required")
class OwnerManagedOpenCode(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        required = ["ZERUS_TEST_WSL_DISTRO", "ZERUS_TEST_WSL_USER", "ZERUS_TEST_WSL_HGS", "HGS_TEST_BIN"]
        if any(not os.environ.get(key) for key in required):
            raise RuntimeError("The opted-in live suite requires an explicit distro, user and both binaries")
        cls.binary = os.environ["HGS_TEST_BIN"]
        cls.wsl = ["wsl.exe", "--distribution", os.environ[required[0]], "--user", os.environ[required[1]], "--exec"]
        cls.temp = tempfile.TemporaryDirectory(prefix="zerus-live-read-")
        cls.addClassCleanup(cls.temp.cleanup)
        root = Path(cls.temp.name)
        (root / "home").mkdir()
        cls.env = {k: v for k, v in os.environ.items() if not k.startswith("HGS_")}
        cls.env.update(HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                       HGS_CONFIG_DIR=str(root / "config"), HGS_STATE_DIR=str(root / "state"),
                       HGS_SELF="acceptance-controller", HGS_PEERS="",
                       HGS_SSH=str(root / "ssh-must-not-run.exe"))
        cls.before = cls.identity()
        cls.remote = cls.linux(r"""from pathlib import Path
import json,os,sys,tempfile
binary=json.load(sys.stdin)['binary']
r=Path(tempfile.mkdtemp(prefix='zerus-live-read-'));r.chmod(0o700)
for name in ['config','state','tmux']:(r/name).mkdir(mode=0o700)
w=r/'receiver'
env=dict(HGS_CONFIG_DIR=str(r/'config'),HGS_STATE_DIR=str(r/'state'),HGS_SELF='acceptance-linux',HGS_PEERS='',TMUX_TMPDIR=str(r/'tmux'))
w.write_text('#!/usr/bin/python3\nimport os,sys\nos.environ.update('+repr(env)+')\np='+repr(binary)+'\nos.execv(p,[p]+sys.argv[1:])\n')
w.chmod(0o700)
print(json.dumps({'root':str(r),'wrapper':str(w)}))
""", {"binary": os.environ[required[2]]})
        cls.addClassCleanup(cls.remove_owned_fixture)
        profile = {"transport": "wsl-local", "enabled": True, "wsl": {
            "distribution": os.environ[required[0]], "user": os.environ[required[1]],
            "hgs_path": cls.remote["wrapper"]}}
        cls.hgs("machine", "set", "live-wsl", "--json", json.dumps(profile), "--create")

    @classmethod
    def linux(cls, code, payload=None):
        out = subprocess.run(cls.wsl + ["python3", "-c", code], input=json.dumps(payload or {}).encode(),
                             capture_output=True, timeout=30)
        if out.returncode:
            raise RuntimeError("The selected WSL acceptance helper failed")
        return json.loads(out.stdout)

    @classmethod
    def identity(cls):
        return cls.linux(r"""from pathlib import Path
import hashlib,json,os
p=Path.home()/'.local/state/opencode/service.json'
s=p.lstat()
if p.is_symlink() or s.st_uid!=os.getuid() or s.st_mode&0o077 or s.st_size>16384:raise RuntimeError('Private registration required')
b=p.read_bytes();d=json.loads(b);os.kill(int(d['pid']),0)
print(json.dumps({'pid':d['pid'],'version':d['version'],'digest':hashlib.sha256(b).hexdigest()}))
""")

    @classmethod
    def hgs(cls, *args):
        out = subprocess.run([cls.binary, *args], capture_output=True, env=cls.env, timeout=90)
        if out.returncode:
            raise RuntimeError("hgs live acceptance command failed: " + args[0])
        return json.loads(out.stdout)

    @classmethod
    def remove_owned_fixture(cls):
        cls.linux(r"""from pathlib import Path
import json,shutil,sys
r=Path(json.load(sys.stdin)['root'])
assert r.parent==Path('/tmp') and r.name.startswith('zerus-live-read-') and (r/'receiver').is_file()
shutil.rmtree(r)
print('{}')
""", cls.remote)

    def test_live_account_and_models_are_available_without_native_changes(self):
        self.assertTrue(self.hgs("machine", "check", "live-wsl")["ok"])
        accounts = self.hgs("@live-wsl", "account", "ls")
        native = next(p for p in accounts["profiles"] if p["id"] == "native-opencode")
        self.assertTrue(native["installed"])
        self.assertEqual(native["catalog_state"], "ready")
        models = self.hgs("@live-wsl", "account", "models", "native-opencode")
        self.assertEqual(models["catalog_state"], "ready")
        self.assertIsInstance(models["model_options"], list)
        self.assertEqual(self.identity(), self.before)

    def test_explicit_start_only_reuses_the_owned_live_service(self):
        self.hgs("@live-wsl", "account", "start", "native-opencode")
        self.assertEqual(self.identity(), self.before)


if __name__ == "__main__":
    unittest.main()
