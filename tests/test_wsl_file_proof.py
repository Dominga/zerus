"""Opt-in scoped file proof across the actual Windows/WSL boundary.
All bindings, files and receiver wrappers belong to disposable test profiles.
No native agent service, owner conversation, GUI editor or SSH is used.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid
import test_wsl_local as transport

@unittest.skipUnless(os.name == "nt" and transport.DISTRO and transport.USER and transport.LINUX_HGS,
                     "explicit Windows/WSL target and receiver required")
class WslFileProof(unittest.TestCase):
    setUpTransport = transport.WslLocal.setUp
    hgs = transport.WslLocal.hgs
    profile = transport.WslLocal.profile
    save = transport.WslLocal.save

    def setUp(self):
        self.setUpTransport()
        self.project = self.root / 'project spaced 世界'
        self.project.mkdir()
        self.file = self.project / 'result %20.txt'
        self.file.write_bytes(b'fixture\x00bytes\n')
        self.session = 'opencode/fixture/files'
        self.run = str(uuid.uuid4())
        self.conversation = 'ses_fixture'
        self.state = self.root / 'native-state'
        bindings = self.state / 'opencode/bindings'; bindings.mkdir(parents=True)
        self.binding = bindings / (hashlib.sha256(self.session.encode()).hexdigest() + '.json')
        self.binding.write_text(json.dumps({'backend':'opencode','service':'opencode2','agent':'opencode',
            'name':self.session,'run_id':self.run,'conversation_id':self.conversation,
            'agent_home':self.linux+'/native-home','cwd':self.linux+'/project spaced 世界'}))
        wrapper=self.root/'scoped receiver.py'
        self.wrapper_text=("#!/usr/bin/python3\nimport os,sys,json\nfrom pathlib import Path\n"
            'root=Path('+repr(self.linux)+')\n'
            "with (root/'calls').open('a') as log:log.write(json.dumps(sys.argv[1:])+'\\n')\n"
            "if (root/'change-connection').exists() and 'file-read' in sys.argv:\n"
            " p=root/'config/machines.local.json';v=json.loads(p.read_text());v['machines']['linux-tools']['wsl']['user']='wrong-user';p.write_text(json.dumps(v))\n"
            "os.environ.update(HGS_STATE_DIR=str(root/'native-state'),HGS_CONFIG_DIR=str(root/'native-config'),"
            "HOME=str(root/'native-home'),HGS_SELF='fixture-linux',HGS_PEERS='')\n"
            'p='+repr(transport.LINUX_HGS)+'\nos.execv(p,[p]+sys.argv[1:])\n')
        wrapper.write_text(self.wrapper_text,encoding='utf-8',newline='\n')
        self.wrapper=wrapper
        connection=self.profile(self.linux+'/scoped receiver.py');self.save(connection)
        mount=self.linux.split('/'+self.root.drive[0].lower()+'/',1)[0] or '/'
        self.selection={'schema_version':1,'request_id':str(uuid.uuid4()),'machine_id':'test-controller',
            'connection':connection['wsl'],
            'mapping':{'mapping_id':'fixture-map','workspace_id':'fixture','machine_id':'test-controller',
                'windows_environment_id':'windows-native','wsl_environment_id':'wsl:'+transport.DISTRO+':'+transport.USER,
                'distribution':transport.DISTRO,'user':transport.USER,'mount_root':mount,'windows_root':str(self.project)},
            'session':self.session,'run_id':self.run,'conversation_id':self.conversation,
            'path':self.linux+'/project spaced 世界/result %20.txt'}

    def proof(self, selection=None, expected=None, success=True):
        payload={'selection':selection or self.selection}
        if expected is not None:payload['expected_preview']=expected
        result=self.hgs('machine','file-proof','linux-tools','--json',data=json.dumps(payload).encode(),timeout=35)
        if success:
            self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'))
            return json.loads(result.stdout)
        self.assertNotEqual(result.returncode,0)
        self.assertFalse(result.stdout)
        return result.stderr.decode(errors='replace')

    def test_bom_prefixed_proof_request(self):
        data = b'\xef\xbb\xbf' + json.dumps({'selection': self.selection}).encode()
        result = self.hgs('machine', 'file-proof', 'linux-tools', '--json', data=data, timeout=35)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual(json.loads(result.stdout)['sha256'], hashlib.sha256(self.file.read_bytes()).hexdigest())

    def test_exact_scope_content_and_recheck_preserve_originals(self):
        binding=self.binding.read_bytes();data=self.file.read_bytes()
        proof=self.proof()
        self.assertEqual(Path(proof['target']['native_path']),self.file)
        self.assertEqual(proof['bytes'],len(data));self.assertEqual(proof['sha256'],hashlib.sha256(data).hexdigest())
        self.assertNotIn('data_base64',proof);self.assertFalse(proof['native_changed']);self.assertFalse(proof['local_verified'])
        self.assertEqual(self.proof(expected=proof['preview_identity']),proof)
        self.assertEqual(self.binding.read_bytes(),binding);self.assertEqual(self.file.read_bytes(),data)
        self.assertFalse((self.root/'native-home/.local/state/opencode/service.json').exists())

    def test_changed_contents_refuse_old_preview_without_substitution(self):
        first=self.proof();self.file.write_bytes(b'new content')
        self.assertIn('changed',self.proof(expected=first['preview_identity'],success=False))
        fresh=self.proof();self.assertNotEqual(first['preview_identity'],fresh['preview_identity'])

    def test_stale_conversation_run_and_unknown_child_are_refused(self):
        for key,value in [('run_id','wrong'),('conversation_id','ses_wrong'),('agent_id','unknown-child'),('archive_id','unknown-archive')]:
            with self.subTest(field=key):self.proof({**self.selection,key:value},success=False)
        self.assertEqual(self.file.read_bytes(),b'fixture\x00bytes\n')

    def test_wrong_mapping_and_profile_fail_before_the_receiver_starts(self):
        for change in ['machine','user','receiver','root','outside']:
            s=copy.deepcopy(self.selection)
            if change=='machine':s['machine_id']='other'
            if change=='user':s['connection']['user']='wrong-user'
            if change=='receiver':s['connection']['hgs_path']='/missing-receiver'
            if change=='root':s['mapping']['windows_root']=str(self.root/'other')
            if change=='outside':s['path']=self.linux+'/outside.txt'
            with self.subTest(change=change):self.proof(s,success=False)
        self.assertFalse((self.root/'calls').exists())

    def test_disabled_and_changed_connections_fail_closed(self):
        (self.root/'change-connection').touch()
        self.assertIn('changed',self.proof(success=False))
        calls=(self.root/'calls').read_text();self.assertNotIn('wrong-user',calls)
        profile=self.profile(self.linux+'/scoped receiver.py');profile['enabled']=False
        result=self.hgs('machine','set','linux-tools','--json',json.dumps(profile));self.assertEqual(result.returncode,0,result.stderr)
        (self.root/'calls').unlink();self.proof(success=False);self.assertFalse((self.root/'calls').exists())

    def test_older_receiver_is_never_sent_an_unknown_file_command(self):
        self.wrapper.write_text("#!/usr/bin/python3\nfrom pathlib import Path\nimport sys\n"
            'root=Path('+repr(self.linux)+')\n'
            "if '--help' in sys.argv: print('hgs fixture old');sys.exit()\n"
            "(root/'unexpected-command').touch()\n",encoding='utf-8',newline='\n')
        self.assertIn('does not offer',self.proof(success=False));self.assertFalse((self.root/'unexpected-command').exists())

    def test_missing_oversized_and_nonregular_files_are_refused(self):
        self.file.unlink();self.proof(success=False)
        self.file.mkdir();self.proof(success=False);self.file.rmdir()
        with self.file.open('wb') as out:out.truncate(10*1024*1024+1)
        self.proof(success=False)

    def test_native_symlink_to_a_different_file_is_not_a_lexical_alias(self):
        self.file.unlink()
        other=self.project/'other.txt';other.write_bytes(b'fixture')
        # Windows cannot unlink a Linux LX_SYMLINK reparse point. Remove only
        # this fixture link through its creating namespace before TempDirectory.
        def remove_link():
            result=subprocess.run(['wsl.exe','-d',transport.DISTRO,'-u',transport.USER,'--exec','python3','-c',
                'import os,sys; p=sys.argv[1]; os.unlink(p) if os.path.lexists(p) else None',
                self.selection['path']],capture_output=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
        self.addCleanup(remove_link)
        result=subprocess.run(['wsl.exe','-d',transport.DISTRO,'-u',transport.USER,'--exec','ln','-s',
            self.linux+'/project spaced 世界/other.txt',self.selection['path']],capture_output=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('another path',self.proof(success=False))

    def test_unknown_fields_and_oversized_input_do_not_invoke_wsl(self):
        self.proof({**self.selection,'command':'run'},success=False)
        result=self.hgs('machine','file-proof','linux-tools','--json',data=b' '*(64*1024+1))
        self.assertNotEqual(result.returncode,0);self.assertFalse((self.root/'calls').exists())

if __name__=='__main__':unittest.main()
