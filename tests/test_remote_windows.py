"""Windows-target command planning through the actual client and a private SSH peer.

The peer decodes the transport for assertions; this is not PowerShell runtime
qualification. The admitted Windows fixture tests the real native envelope.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', ROOT / 'target/debug/hgs'))
PROBE = 'echo "hgs-remote-v2|%OS%|%SystemRoot%|$env:OS|$env:SystemRoot"'
SSH = r'''#!/usr/bin/env python3
import base64,json,os,pathlib,sys
r=pathlib.Path(os.environ['REMOTE_WINDOWS_FIXTURE'])
command=sys.argv[-1]
with (r/'calls').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\n')
if command.startswith('echo "hgs-remote-v2|'):
    mode=(r/'mode').read_text()
    if mode=='windows':print('hgs-remote-v2|%OS%|%SystemRoot%|Windows_NT|C:\\Windows');sys.exit(0)
    if mode=='cmd':print('"hgs-remote-v2|Windows_NT|C:\\Windows|$env:OS|$env:SystemRoot"');sys.exit(0)
    if mode=='offline':sys.exit(255)
    if mode=='oversize':print('x'*300000);sys.exit(0)
    print('not a platform receipt');sys.exit(0)
if command.startswith('& ([ScriptBlock]::Create('):
    encoded=command.split("FromBase64String('",1)[1].split("'",1)[0]
else:
    assert command.startswith('"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" '),command
    encoded=command.rsplit(' -EncodedCommand ',1)[1]
script=base64.b64decode(encoded).decode('utf-16-le')
data=script.split("FromBase64String('",1)[1].split("'",1)[0]
words=json.loads(base64.b64decode(data))
with (r/'words').open('a') as f:f.write(json.dumps(words)+'\n')
if words==['--help']:
    print((r/'help').read_text());sys.exit(0)
body=sys.stdin.read()
with (r/'operations').open('a') as f:f.write(json.dumps({'argv':words,'stdin':body})+'\n')
if words==['--version']:print('hgs 9.9.9');sys.exit(0)
if words==['ls','--local','--json'] and (r/'listing').exists():print((r/'listing').read_text());sys.exit(0)
if words[0]=='watch':
    print(json.dumps({'t':'hello'}));print(json.dumps({'t':'snapshot'}))
    print(json.dumps({'t':'echo','stdin':body}));sys.exit(0)
print(json.dumps({'argv':words,'stdin':body}))
sys.exit(0)
'''


@unittest.skipIf(os.name == 'nt', 'Linux/macOS SSH client fixture; Windows native transport has a separate control')
class WindowsTarget(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zerus-windows-target-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / 'home'
        self.home.mkdir()
        self.env = {k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE','SSH_AUTH_SOCK')}
        self.env.update(HOME=str(self.home),HGS_SELF='client',HGS_PEERS='peer',HGS_CONFIG_DIR=str(self.root/'config'),
                        HGS_STATE_DIR=str(self.root/'state'),HGS_TAB='0',REMOTE_WINDOWS_FIXTURE=str(self.root))
        ssh = self.root/'ssh'
        ssh.write_text(SSH);ssh.chmod(0o700)
        self.env['HGS_SSH']=str(ssh)
        (self.root/'mode').write_text('windows')
        help_text=subprocess.check_output([str(HGS),'--help'],env=self.env,text=True)
        (self.root/'help').write_text(help_text)

    def hgs(self,*words,body=''):
        return subprocess.run([str(HGS),'@peer',*words],input=body,capture_output=True,text=True,env=self.env,timeout=15)

    def operations(self):
        p=self.root/'operations'
        return [json.loads(v) for v in p.read_text().splitlines()] if p.exists() else []

    def test_version_uses_installed_windows_cli(self):
        out=self.hgs('version')
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(out.stdout,'hgs 9.9.9\n')
        self.assertEqual(self.operations(),[{'argv':['--version'],'stdin':''}])

    def test_literal_arguments_and_json_input_keep_their_bytes(self):
        name="codex/it's\\a path/日本語\"tag"
        body=json.dumps({'text':'quotes \" and \\ and $()\nsecond line'},ensure_ascii=False)
        out=self.hgs('send',name,'--json',body=body)
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(json.loads(out.stdout),{'argv':['send',name,'--json'],'stdin':body})

    def test_watch_gates_then_passes_control_input_without_consuming_it(self):
        body='{"op":"subscribe","session":"codex/project/tag"}\n'
        out=self.hgs('watch','--json',body=body)
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual([json.loads(v)['t'] for v in out.stdout.splitlines()],['hello','snapshot','echo'])
        self.assertEqual(self.operations(),[{'argv':['watch','--json'],'stdin':body}])
        words=[json.loads(v) for v in (self.root/'words').read_text().splitlines()]
        self.assertEqual(words,[['--help'],['watch','--json']])

    def test_unknown_probe_is_closed_before_an_operation(self):
        for mode in ('unknown','offline','oversize'):
            with self.subTest(mode=mode):
                (self.root/'mode').write_text(mode)
                out=self.hgs('send','codex/project/tag','--json',body='{"text":"never send"}')
                self.assertNotEqual(out.returncode,0,out.stdout)
                self.assertEqual(self.operations(),[])

    def test_old_peer_cannot_interpret_capability_commands_as_launches(self):
        (self.root/'help').write_text('usage: hgs AGENT DIRECTORY\n')
        for command in ('watch','wait','notifications','hooks','analytics','guide','repository','subagent'):
            with self.subTest(command=command):
                out=self.hgs(command,'--json',body='{}')
                self.assertNotEqual(out.returncode,0,out.stdout)
                self.assertEqual(self.operations(),[])

    def test_exact_selector_marker_and_machine_guards_stay_required(self):
        ident='68da1c4c-1b69-49ec-a76a-fc52ac6f19de'
        machine='18141714-83b2-4c67-a7a1-b400864d9b1b'
        good=(self.root/'help').read_text()
        for words,marker in ((('inspect','--session-id',ident),'session-id-selector-v1'),
                             (('--expect-machine',machine,'session-action-status','--session-id',ident,'--json'),'lifecycle-receipt-status-v1')):
            with self.subTest(words=words):
                (self.root/'help').write_text(good.replace(marker,marker+'-unsupported'))
                out=self.hgs(*words,body='{}')
                self.assertNotEqual(out.returncode,0,out.stdout)
                self.assertEqual(self.operations(),[])
                (self.root/'help').write_text(good)
                out=self.hgs(*words,body='{}')
                self.assertEqual(out.returncode,0,out.stderr)
                expected=list(words)
                if words[0]=='--expect-machine': expected[2:2]=['--client','client']
                self.assertEqual(self.operations()[-1]['argv'],expected)
                (self.root/'operations').unlink()

    def test_reference_marker_and_analytics_read_only_are_preserved(self):
        uri='zerus://session?machine=18141714-83b2-4c67-a7a1-b400864d9b1b&host=peer&name=codex%2Fp%2Ft'
        good=(self.root/'help').read_text()
        (self.root/'help').write_text(good.replace('session-reference-selector-v1','not-a-capability'))
        out=self.hgs('send',uri,'--json',body='{}')
        self.assertNotEqual(out.returncode,0)
        self.assertEqual(self.operations(),[])
        (self.root/'help').write_text(good)
        out=self.hgs('analytics','--json')
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(self.operations()[-1]['argv'],['analytics','--json','--read-only'])

    def test_launch_and_ls_use_the_same_windows_transport(self):
        out=self.hgs('fake_agent','C:\\fixture folder\\日本語','--new','-n','fixture','-d','--','a"b','')
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(self.operations()[-1]['argv'],['--client','client','fake_agent','C:\\fixture folder\\日本語','--new','-n','fixture','-d','--','a"b',''])
        out=self.hgs('ls','--json')
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(self.operations()[-1]['argv'],['ls','--local','--json'])
        # remote() and its listing helper share process-local discovery.
        calls=[json.loads(v) for v in (self.root/'calls').read_text().splitlines()]
        self.assertEqual(sum(v[-1]==PROBE for v in calls),2)

    def test_machine_check_uses_the_windows_local_sessions_contract(self):
        for listing, ready in (({'ok':True,'sessions':[]},True),
                               ({'ok':True,'sessions':[],'local_sessions':True},True),
                               ({'ok':True,'sessions':[],'local_sessions':False},False),
                               ({'ok':True},False)):
            with self.subTest(listing=listing):
                (self.root/'listing').write_text(json.dumps(listing))
                out=subprocess.run([str(HGS),'machine','check','peer'],capture_output=True,text=True,env=self.env,timeout=15)
                self.assertEqual(out.returncode,0,out.stderr)
                value=json.loads(out.stdout)
                self.assertEqual(value['ok'],ready)
                self.assertEqual(value['os'],'Windows')
                self.assertIsNone(value['tmux'])

    def test_windows_setup_refuses_before_packaging_or_remote_install(self):
        out=subprocess.run([str(HGS),'machine','setup','peer'],capture_output=True,text=True,env=self.env,timeout=15)
        self.assertEqual(out.returncode,2,out.stderr)
        self.assertIn('Windows Zerus package',out.stderr)
        self.assertEqual(self.operations(),[])

    def test_cmd_uses_the_same_closed_transport(self):
        (self.root/'mode').write_text('cmd')
        out=self.hgs('send','codex/project/tag','--json',body='{"text":"synthetic"}')
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(self.operations(),[{'argv':['send','codex/project/tag','--json'],'stdin':'{"text":"synthetic"}'}])
