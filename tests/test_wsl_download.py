"""Real Windows/WSL copy transport with isolated profiles and no SSH or agents."""
import base64
import json
import os
from pathlib import Path
import subprocess
import unittest
import test_wsl_local as transport

@unittest.skipUnless(os.name=='nt' and transport.DISTRO and transport.USER and transport.LINUX_HGS,
                     'explicit Windows/WSL target and receiver required')
class WslDownload(unittest.TestCase):
    setUpTransport=transport.WslLocal.setUp
    hgs=transport.WslLocal.hgs
    profile=transport.WslLocal.profile
    save=transport.WslLocal.save
    def setUp(self):
        self.setUpTransport()
        self.source=self.root/'source spaced 世界 %20 ; $literal.txt'
        self.content=bytes(range(256))*37
        self.source.write_bytes(self.content)
        self.destination=self.root/'target Windows copy.txt';self.destination.write_bytes(b'keep-previous')
        self.home=self.root/'native-home';self.home.mkdir();(self.home/'empty.txt').write_bytes(b'')
        self.wrapper=self.root/'copy receiver.py'
        self.wrapper.write_text("#!/usr/bin/python3\nimport os,sys,json\nfrom pathlib import Path\n"
            'r=Path('+repr(self.linux)+')\n'
            "with (r/'calls').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')\n"
            "if (r/'change-connection').exists() and '--snapshot' in sys.argv:\n"
            " p=r/'config/machines.local.json';v=json.loads(p.read_text());v['machines']['linux-tools']['wsl']['user']='wrong-user';p.write_text(json.dumps(v))\n"
            "os.environ.update(HGS_CONFIG_DIR=str(r/'native-config'),HGS_STATE_DIR=str(r/'native-state'),HOME=str(r/'native-home'),HGS_SELF='fixture-linux',HGS_PEERS='')\n"
            'p='+repr(transport.LINUX_HGS)+'\nos.execv(p,[p]+sys.argv[1:])\n',encoding='utf-8',newline='\n')
        self.save(self.profile(self.linux+'/copy receiver.py'))
    def download(self,path=None,success=True):
        path=path or self.linux+'/'+self.source.name
        result=self.hgs('@linux-tools','download','--path',path,'--output',str(self.destination),timeout=35)
        if success:
            self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'));return json.loads(result.stdout)
        self.assertNotEqual(result.returncode,0);self.assertFalse(result.stdout)
        self.assertEqual(self.destination.read_bytes(),b'keep-previous')
        return result.stderr.decode(errors='replace')
    def test_literal_binary_copy_is_published_on_windows_only(self):
        result=self.download();self.assertEqual(self.destination.read_bytes(),self.content)
        self.assertEqual(result,{'host':'linux-tools','path':self.linux+'/'+self.source.name,'local_path':str(self.destination),'bytes':len(self.content)})
        calls=[json.loads(line) for line in (self.root/'calls').read_text().splitlines()]
        self.assertEqual(calls[-1],['swarm','__wsl-local-v1','download','--snapshot',self.linux+'/'+self.source.name])
        self.assertNotIn(str(self.destination),json.dumps(calls))
        self.assertEqual(self.source.read_bytes(),self.content)
        self.assertFalse((self.home/'.local/state/opencode/service.json').exists())
    def test_home_and_empty_files_are_resolved_by_selected_linux_user(self):
        result=self.download('~/empty.txt');self.assertEqual(result['bytes'],0);self.assertEqual(self.destination.read_bytes(),b'')
    def test_missing_directory_and_too_large_files_preserve_destination(self):
        self.download(self.linux+'/missing.txt',success=False)
        self.download(self.linux+'/native-home',success=False)
        self.source.write_bytes(b'')
        with self.source.open('wb') as f:f.truncate(10*1024*1024+1)
        self.assertIn('10 MiB',self.download(success=False))
    def test_connection_change_during_read_does_not_publish(self):
        (self.root/'change-connection').touch();self.assertIn('changed',self.download(success=False))
    def test_old_receiver_never_receives_a_new_snapshot_command(self):
        self.wrapper.write_text("#!/usr/bin/python3\nimport sys\nfrom pathlib import Path\n"
            'r=Path('+repr(self.linux)+')\n'
            "if '--help' in sys.argv:print('hgs old fixture');sys.exit()\n"
            "(r/'unexpected-command').touch()\n",encoding='utf-8',newline='\n')
        self.assertIn('SSH fallback is disabled',self.download(success=False));self.assertFalse((self.root/'unexpected-command').exists())
    def test_malformed_or_mismatched_snapshot_never_replaces_output(self):
        path=self.linux+'/'+self.source.name
        values=[{'schema_version':1,'requested_path':path,'path':path,'bytes':2,'sha256':'0'*64,'data_base64':base64.b64encode(b'no').decode()},
                {'schema_version':1,'requested_path':'/another-file','path':path,'bytes':0,'sha256':'0'*64,'data_base64':''},
                {'schema_version':1,'requested_path':path,'path':path,'bytes':10*1024*1024+1,'data_base64':''}]
        for value in values:
            self.wrapper.write_text("#!/usr/bin/python3\nimport sys,json\n"
                "if '--help' in sys.argv:print('WSL file snapshot ABI: wsl-file-snapshot-v1');sys.exit()\n"
                'print('+repr(json.dumps(value))+')\n',encoding='utf-8',newline='\n')
            self.download(success=False)
    def test_disabled_connection_and_invalid_arguments_do_not_use_wsl(self):
        profile=self.profile(self.linux+'/copy receiver.py');profile['enabled']=False
        changed=self.hgs('machine','set','linux-tools','--json',json.dumps(profile));self.assertEqual(changed.returncode,0,changed.stderr)
        self.download(success=False);self.assertFalse((self.root/'calls').exists())
    def test_snapshot_path_never_becomes_a_windows_destination(self):
        result=self.hgs('@linux-tools','download','--snapshot',self.linux+'/'+self.source.name)
        self.assertNotEqual(result.returncode,0);self.assertFalse(result.stdout);self.assertFalse((self.root/'calls').exists())

if __name__=='__main__':unittest.main()
