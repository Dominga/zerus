"""Opt-in Windows -> real local WSL contract using isolated profiles and helpers.
Set ZERUS_TEST_WSL_DISTRO, ZERUS_TEST_WSL_USER and ZERUS_TEST_WSL_HGS (Linux
path to this fork's test binary). Never starts or stops a real agent service.
"""
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "target/debug/hgs.exe"))
DISTRO = os.environ.get("ZERUS_TEST_WSL_DISTRO")
USER = os.environ.get("ZERUS_TEST_WSL_USER")
LINUX_HGS = os.environ.get("ZERUS_TEST_WSL_HGS")

@unittest.skipUnless(os.name == "nt" and DISTRO and USER and LINUX_HGS,
                     "explicit Windows/WSL target and test binary required")
class WslLocal(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zerus-wsl-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.linux = subprocess.check_output(
            ["wsl.exe", "--distribution", DISTRO, "--user", USER, "--exec", "wslpath", "-a", "-u", str(self.root)],
            text=True, encoding="utf-8", timeout=15).strip()
        self.env = {k:v for k,v in os.environ.items() if not k.startswith("HGS_")}
        self.env.update(HOME=str(self.root/"win-home"), USERPROFILE=str(self.root/"win-home"),
                        HGS_CONFIG_DIR=str(self.root/"config"), HGS_STATE_DIR=str(self.root/"state"),
                        HGS_SELF="test-controller", HGS_SSH=str(self.root/"must-not-use-ssh.exe"))
        (self.root/"win-home").mkdir()
        self.helper = self.root/"echo helper.py"
        self.helper.write_text("#!/usr/bin/python3\nimport base64,json,os,pwd,sys\n"
            "args=sys.argv[1:]\n"
            "if args[2:]==['--version']:\n print('hgs fixture');sys.exit(0)\n"
            "print(json.dumps({'argv':args,'stdin':base64.b64encode(sys.stdin.buffer.read()).decode(),"
            "'user':pwd.getpwuid(os.getuid()).pw_name,'cwd':os.getcwd()}))\n"
            "sys.exit(23 if 'exit-23' in args else 0)\n", encoding="utf-8", newline="\n")

    def hgs(self, *args, data=b"", timeout=25):
        return subprocess.run([str(HGS), *args], input=data, capture_output=True,
                              env=self.env, timeout=timeout)

    def profile(self, path=None):
        return {"transport":"wsl-local", "enabled":True, "wsl":{
            "distribution":DISTRO,"user":USER,"hgs_path":path or self.linux+"/echo helper.py"}}

    def save(self, profile):
        result=self.hgs("machine","set","linux-tools","--json",json.dumps(profile),"--create")
        self.assertEqual(result.returncode,0,result.stderr.decode(errors="replace"))

    def test_argv_stdin_unicode_and_exit_status_cross_the_real_boundary(self):
        self.save(self.profile())
        args=["inspect", "a ' \" ; $HOME & 世界\\leaf", "", "exit-23"]
        data=b"synthetic input\x00\xff\n"
        result=self.hgs("@linux-tools",*args,data=data)
        self.assertEqual(result.returncode,23,result.stderr.decode(errors="replace"))
        value=json.loads(result.stdout)
        self.assertEqual(value["argv"],["swarm","__wsl-local-v1",*args])
        self.assertEqual(base64.b64decode(value["stdin"]),data)
        self.assertEqual(value["user"],USER)
        self.assertEqual(value["cwd"],"/")

    def test_check_and_resolve_use_wsl_without_ssh(self):
        self.save(self.profile())
        result=self.hgs("machine","check","linux-tools")
        self.assertEqual(result.returncode,0,result.stderr.decode(errors="replace"))
        self.assertTrue(json.loads(result.stdout)["ok"])
        result=self.hgs("machine","resolve","linux-tools")
        self.assertEqual(json.loads(result.stdout)["effective"]["transport"],"wsl-local")
        result=self.hgs("@linux-tools","--dry-run","inspect","test")
        self.assertEqual(result.returncode,0)
        self.assertEqual(json.loads(result.stdout)["service_policy"],"existing-only")

    def test_incomplete_or_mixed_profiles_fail_before_launch(self):
        for value in [{"transport":"wsl-local"}, {"transport":"unknown"},
                      {**self.profile(),"hostname":"example.test"},
                      {"transport":"ssh","wsl":self.profile()["wsl"]}]:
            result=self.hgs("machine","set","linux-tools","--json",json.dumps(value))
            self.assertNotEqual(result.returncode,0)
        self.assertFalse((self.root/"config/machines.local.json").exists())

    def test_unavailable_distro_does_not_fall_back(self):
        value=self.profile()
        value["wsl"]["distribution"]="Zerus-Nonexistent-Fixture-Distribution"
        self.save(value)
        result=self.hgs("@linux-tools","--version")
        self.assertNotEqual(result.returncode,0)
        self.assertNotIn(b"hgs fixture",result.stdout)

    def test_explicit_mapping_reaches_the_selected_native_directory(self):
        # The mount root is measured for this disposable fixture, not inferred
        # by the product from a user's arbitrary POSIX spelling.
        project = self.root / 'project spaced 世界'
        project.mkdir()
        (project / 'child').mkdir()
        drive = self.root.drive[0].lower()
        separator = '/' + drive + '/'
        self.assertIn(separator, self.linux)
        mount = self.linux.split(separator, 1)[0] or '/'
        mapping = {'mapping_id': 'fixture-drive', 'workspace_id': 'fixture-workspace',
                   'machine_id': 'test-controller', 'windows_environment_id': 'windows-native',
                   'wsl_environment_id': 'wsl:' + DISTRO + ':' + USER, 'distribution': DISTRO,
                   'user': USER, 'mount_root': mount, 'windows_root': str(self.root)}
        source = {'machine_id': 'test-controller', 'environment_id': 'windows-native',
                  'path_flavor': 'windows', 'native_path': str(project), 'workspace_id': 'fixture-workspace'}
        described = self.hgs('path-ref', 'describe', data=json.dumps({'path': source, 'mapping': mapping}).encode())
        self.assertEqual(described.returncode, 0, described.stderr.decode(errors='replace'))
        mapped = self.hgs('path-ref', 'map', data=json.dumps({'path': json.loads(described.stdout), 'mapping': mapping}).encode())
        self.assertEqual(mapped.returncode, 0, mapped.stderr.decode(errors='replace'))
        native = json.loads(mapped.stdout)
        self.assertEqual(native['native_path'], self.linux + '/project spaced 世界')
        self.assertEqual(native['environment_id'], mapping['wsl_environment_id'])
        wrapper = self.root / 'directory receiver.py'
        wrapper.write_text('#!/usr/bin/python3\nimport os,sys\nr=' + repr(self.linux) + '\n'
            "os.environ.update(HGS_CONFIG_DIR=r+'/directory-config',HGS_STATE_DIR=r+'/directory-state',HGS_SELF='fixture',HGS_PEERS='')\n"
            'p=' + repr(LINUX_HGS) + '\nos.execv(p,[p]+sys.argv[1:])\n', encoding='utf-8', newline='\n')
        self.save(self.profile(self.linux + '/directory receiver.py'))
        result = self.hgs('@linux-tools', 'dirs', native['native_path'])
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        listed = json.loads(result.stdout)
        self.assertEqual(listed['path'], native['native_path'])
        self.assertIn('child', [row['name'] for row in listed['directories']])
        outside = {**source, 'native_path': str(self.root.parent / 'outside-fixture')}
        refused = self.hgs('path-ref', 'describe', data=json.dumps({'path': outside, 'mapping': mapping}).encode())
        self.assertNotEqual(refused.returncode, 0)
        self.assertFalse((self.root.parent / 'outside-fixture').exists())

    def test_real_receiver_never_starts_opencode_even_for_explicit_start(self):
        home=self.linux+"/linux-home"
        (self.root/"linux-home/bin").mkdir(parents=True)
        marker=self.root/"started-agent"
        fake=self.root/"linux-home/bin/opencode"
        fake.write_text("#!/usr/bin/python3\nfrom pathlib import Path\n"
            +"Path("+repr(self.linux+"/started-agent")+").write_text('unexpected')\n",
            encoding="utf-8",newline="\n")
        wrapper=self.root/"real receiver.py"
        wrapper.write_text("#!/usr/bin/python3\nimport os,sys\n"
            +"root="+repr(home)+"\n"
            +"os.environ.update(HOME=root,XDG_CONFIG_HOME=root+'/config',XDG_STATE_HOME=root+'/state',"
              "XDG_DATA_HOME=root+'/data',XDG_CACHE_HOME=root+'/cache',"
              "HGS_CONFIG_DIR=root+'/hgs-config',HGS_STATE_DIR=root+'/hgs-state',"
              "HGS_SELF='fixture-linux',PATH=root+'/bin:/usr/bin:/bin')\n"
            +"program="+repr(LINUX_HGS)+"\n"
            +"os.execv(program,[program]+sys.argv[1:])\n", encoding="utf-8",newline="\n")
        self.save(self.profile(self.linux+"/real receiver.py"))
        result=self.hgs("@linux-tools","account","start","native-opencode")
        self.assertNotEqual(result.returncode,0)
        self.assertIn(b"existing-only",result.stderr)
        self.assertFalse(marker.exists(),"existing-only called the agent executable")
        self.assertFalse((self.root/"linux-home/state/opencode/service.json").exists())
        self.assertFalse(list((self.root/"linux-home").rglob("service-start.lock")))

if __name__ == "__main__":
    unittest.main()
