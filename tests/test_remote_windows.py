"""Windows-target command planning through the actual client and a private SSH peer.

The peer decodes the transport for assertions; this is not PowerShell runtime
qualification. The admitted Windows fixture tests the real native envelope.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import select
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get('HGS_TEST_BIN', ROOT / 'target/debug/hgs'))
PROBE = 'echo "hgs-remote-v2|%OS%|%SystemRoot%|$env:OS|$env:SystemRoot"'
SSH = r'''#!/usr/bin/env python3
import base64,json,os,pathlib,struct,sys,time
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
    encoded=command.split("FromBase64String('",1)[1].split("'",1)[0]
script=base64.b64decode(encoded).decode('utf-8')
data=script.split("FromBase64String('",1)[1].split("'",1)[0]
words=json.loads(base64.b64decode(data))
with (r/'words').open('a') as f:f.write(json.dumps(words)+'\n')
if words==['--help']:
    print((r/'help').read_text());sys.exit(0)
if words[0]=='watch':
    print(json.dumps({'t':'hello'}),flush=True);print(json.dumps({'t':'snapshot'}),flush=True)
if (r/'consumer-loss').exists() and words[0]=='watch':
    import fcntl
    with (r/'alive').open('w') as alive:
        fcntl.flock(alive,fcntl.LOCK_EX)
        (r/'ready').touch()
        while True:sys.stdout.write('x'*65536);sys.stdout.flush()
def exact(n):
    out=b''
    while len(out)<n:
        more=sys.stdin.buffer.read(n-len(out))
        assert more,'transport closed before explicit EOF'
        out+=more
    return out
body=bytearray()
while True:
    n=struct.unpack('<I',exact(4))[0]
    if n==0xffffffff:continue
    assert n<=65536,n
    if not n:break
    body+=exact(n)
if (r/'binary').exists():sys.stdout.buffer.write(bytes(body));sys.exit(7)
body=body.decode('utf-8')
with (r/'operations').open('a') as f:f.write(json.dumps({'argv':words,'stdin':body})+'\n')
if words==['--version']:print('hgs 9.9.9');sys.exit(0)
if words==['ls','--local','--json'] and (r/'listing').exists():print((r/'listing').read_text());sys.exit(0)
if words[0]=='watch':
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

    def test_explicit_eof_ends_watch_while_the_ssh_input_pipe_stays_open(self):
        child = subprocess.Popen([str(HGS),'@peer','watch','--json'],env=self.env,
                                 stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,bufsize=0)
        try:
            frames = []
            for _ in range(2):
                self.assertTrue(select.select([child.stdout],[],[],8)[0])
                frames.append(json.loads(child.stdout.readline())['t'])
            self.assertEqual(frames,['hello','snapshot'])
            child.stdin.write(b'{"op":"list"}\n');child.stdin.close()
            # The fake SSH decoder waits for the explicit frame, not OS EOF.
            child.wait(timeout=8)
            self.assertEqual(child.returncode,0,child.stderr.read())
            self.assertEqual(json.loads(child.stdout.readline())['stdin'],'{"op":"list"}\n')
        finally:
            if child.poll() is None:child.kill();child.wait()
            child.stdout.close();child.stderr.close()
            if not child.stdin.closed:child.stdin.close()

    def test_multiple_frames_preserve_binary_stdin_and_original_exit(self):
        (self.root/'binary').touch()
        body=bytes(range(256))*2048
        child=subprocess.run([str(HGS),'@peer','send','codex/p/t','--json'],input=body,
                             capture_output=True,env=self.env,timeout=15)
        self.assertEqual(child.returncode,7,child.stderr)
        self.assertEqual(child.stdout,body)

    def test_closed_output_settles_the_same_ssh_child(self):
        import fcntl
        (self.root/'consumer-loss').touch()
        child=subprocess.Popen([str(HGS),'@peer','watch','--json'],env=self.env,
                               stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            until=time.monotonic()+8
            while not (self.root/'ready').exists() and time.monotonic()<until:time.sleep(.01)
            self.assertTrue((self.root/'ready').exists())
            child.stdout.close()
            child.wait(timeout=8)
            self.assertNotEqual(child.returncode,0)
            # The original fake client's held lock proves it has settled.
            with (self.root/'alive').open() as alive:fcntl.flock(alive,fcntl.LOCK_EX|fcntl.LOCK_NB)
        finally:
            if child.poll() is None:child.kill();child.wait()
            child.stdin.close();child.stderr.close()

    @unittest.skipUnless(sys.platform.startswith('linux'), 'requires a controlled Linux pipe capacity')
    def test_original_exit_waits_for_a_healthy_slow_output_consumer(self):
        import fcntl
        script = self.root/'ssh'
        script.write_text("#!/usr/bin/env python3\nimport os,pathlib\nb=bytes(range(256))*32\nwhile b:\n n=os.write(1,b);b=b[n:]\npathlib.Path(os.environ['REMOTE_WINDOWS_FIXTURE'],'original-exited').touch()\nos._exit(0)\n")
        expected = bytes(range(256))*32
        for delay in (0, 3):
            with self.subTest(delay=delay):
                marker = self.root/'original-exited'
                marker.unlink(missing_ok=True)
                child = subprocess.Popen([str(HGS),'--ssh-bytes-client','peer','__hgs_ssh_bytes_v1__ fixed'],
                                         env=self.env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                try:
                    fcntl.fcntl(child.stdout, fcntl.F_SETPIPE_SZ, 4096)
                    if delay: time.sleep(delay)
                    out, err = child.communicate(timeout=8)
                    self.assertEqual(child.returncode, 0, err)
                    self.assertEqual(out, expected)
                    self.assertTrue(marker.exists())
                finally:
                    if child.poll() is None: child.terminate();child.wait(timeout=5)
                    child.stdout.close();child.stderr.close()

    @unittest.skipUnless(sys.platform.startswith('linux'), 'requires kernel exit observation through procfs')
    def test_cancel_settles_after_ssh_exits_with_inherited_output_still_open(self):
        ssh = self.root/'ssh'
        child_script = "import os,pathlib,time; r=pathlib.Path(os.environ['REMOTE_WINDOWS_FIXTURE']); (r/'holder-ready').touch(); deadline=time.monotonic()+12;\nwhile not (r/'holder-stop').exists() and time.monotonic()<deadline:time.sleep(.01)\n(r/'holder-ended').touch()\n"
        ssh.write_text("#!/usr/bin/env python3\nimport os,pathlib,subprocess\np=subprocess.Popen(['python3','-c',"+repr(child_script)+"],stdin=subprocess.DEVNULL)\npathlib.Path(os.environ['REMOTE_WINDOWS_FIXTURE'],'ssh-ended').write_text(str(os.getpid()))\nos._exit(0)\n")
        child = subprocess.Popen([str(HGS),'--ssh-bytes-client','peer','__hgs_ssh_bytes_v1__ fixed'],env=self.env,
                                 stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic()+5
            while not ((self.root/'ssh-ended').exists() and (self.root/'holder-ready').exists()):
                self.assertLess(time.monotonic(), deadline, 'fixture did not start')
                time.sleep(.01)
            original = int((self.root/'ssh-ended').read_text())
            while Path('/proc', str(original)).exists():
                self.assertLess(time.monotonic(), deadline, 'original SSH did not exit/reap')
                time.sleep(.01)
            # The original is physically gone, while its holder stays alive.
            # Cancel this SAME Popen only; the holder has a natural control exit.
            child.terminate()
            try: child.wait(timeout=1)
            except subprocess.TimeoutExpired: self.fail('cancellation did not settle the original helper after SSH exit')
            self.assertEqual(child.returncode,255)
        finally:
            (self.root/'holder-stop').touch()
            if child.poll() is None: child.kill(); child.wait(timeout=3)
            child.stdout.close();child.stderr.close()
            deadline=time.monotonic()+3
            while not (self.root/'holder-ended').exists() and time.monotonic()<deadline:time.sleep(.01)
            self.assertTrue((self.root/'holder-ended').exists(), 'fixture holder did not settle naturally')

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


def async_stdin_pipe():
    """Match Windows OpenSSH's inherited overlapped stdin, not Popen's sync pipe."""
    import ctypes
    from ctypes import wintypes
    import msvcrt
    import uuid
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    class Security(ctypes.Structure):
        _fields_ = [('size', wintypes.DWORD), ('descriptor', ctypes.c_void_p), ('inherit', wintypes.BOOL)]
    kernel.CreateNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                      wintypes.DWORD, wintypes.DWORD, wintypes.DWORD,
                                      wintypes.DWORD, ctypes.POINTER(Security)]
    kernel.CreateNamedPipeW.restype = wintypes.HANDLE
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                  ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    name = r'\\.\pipe\hgs-ssh-stdin-' + uuid.uuid4().hex
    security = Security(ctypes.sizeof(Security), None, True)
    # Inbound, first-instance, overlapped byte pipe; the sole writer is synchronous.
    read = kernel.CreateNamedPipeW(name, 1 | 0x80000 | 0x40000000, 0, 1, 4096, 4096, 0,
                                  ctypes.byref(security))
    invalid = ctypes.c_void_p(-1).value
    if read == invalid: raise ctypes.WinError(ctypes.get_last_error())
    try:
        write = kernel.CreateFileW(name, 0x40000000, 0, None, 3, 0, None)
        if write == invalid: raise ctypes.WinError(ctypes.get_last_error())
        try:
            reader = msvcrt.open_osfhandle(read, os.O_RDONLY | os.O_BINARY)
            read = None
            try:
                writer = os.fdopen(msvcrt.open_osfhandle(write, os.O_WRONLY | os.O_BINARY), 'wb', buffering=0)
                write = None
                return reader, writer
            except BaseException:
                os.close(reader)
                raise
        finally:
            if write is not None: kernel.CloseHandle(write)
    finally:
        if read is not None: kernel.CloseHandle(read)


@unittest.skipUnless(os.name == 'nt', 'native PowerShell 5.1/cmd decoder controls run on Windows')
class WindowsNativeTransport(unittest.TestCase):
    def run_decoder(self, shell, lease=False, code=0, slow=False, sustained=False):
        import base64
        import shutil
        import struct
        import threading
        fake = Path(os.environ.get('HGS_TEST_FAKE_AGENT', ROOT / 'target/debug/examples/fake_agent.exe'))
        self.assertTrue(fake.is_file(), 'build the fake_agent example first')
        with tempfile.TemporaryDirectory(prefix='hgs-remote-bytes-') as temporary:
            root = Path(temporary)
            profile = root / 'profile space 日本語'
            (profile / '.local/bin').mkdir(parents=True)
            shutil.copy2(fake, profile / '.local/bin/hgs.exe')
            for name in ('tmp', 'config', 'state', 'appdata', 'localappdata'):
                (root / name).mkdir()
            env = {k: os.environ[k] for k in ('SystemRoot', 'WINDIR', 'ComSpec', 'PATHEXT') if k in os.environ}
            system = Path(env['SystemRoot'])
            ps = system / 'System32/WindowsPowerShell/v1.0/powershell.exe'
            cmd = system / 'System32/cmd.exe'
            ready, record, control = (root / name for name in ('ready.json', 'record.jsonl', 'control'))
            env.update(HOME=str(profile), USERPROFILE=str(profile), HGS_CONFIG_DIR=str(root/'config'),
                       HGS_STATE_DIR=str(root/'state'), APPDATA=str(root/'appdata'), LOCALAPPDATA=str(root/'localappdata'),
                       TEMP=str(root/'tmp'), TMP=str(root/'tmp'), PATH=os.pathsep.join(map(str, (system/'System32', ps.parent))),
                       FAKE_AGENT_READY=str(ready), FAKE_AGENT_RECORD=str(record), FAKE_AGENT_CONTROL=str(control),
                       FAKE_AGENT_RAW='0', FAKE_AGENT_MODES='none', FAKE_AGENT_ON_EOF='exit:'+str(code))
            words = ['', 'a"b', 'back\\', 'two\\\\tail', '日本語', 'Пример', '$(literal);& data', "it's a word"]
            version = 'x'*8191
            if slow:
                words = ['--version']
                env['FAKE_AGENT_PROBE_TEXT'] = version
            if sustained:
                render = root/'render'
                render.write_bytes(b''.join(json.dumps({'i': n}).encode().ljust(63, b' ')+b'\n' for n in range(20)))
                env.update(FAKE_AGENT_RENDER=str(render), FAKE_AGENT_RENDER_CHUNK='64', FAKE_AGENT_RENDER_DELAY='1000')
            data = base64.b64encode(json.dumps(words, ensure_ascii=False, separators=(',', ':')).encode()).decode()
            script = '\n'.join(line.strip() for line in (ROOT/'src/remote_shell/windows.ps1').read_text().splitlines()
                               if line.strip() and not line.strip().startswith('#')).replace('__HGS_ARGUMENTS__', data)
            encoded = base64.b64encode(script.encode('utf-8')).decode()
            if shell == 'powershell':
                invocation = "& ([ScriptBlock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+encoded+"'))))"
                argv, executable = [str(ps), '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', invocation], None
            else:
                invocation = ('"'+str(ps)+'" -NoLogo -NoProfile -NonInteractive -Command "'
                              "& ([ScriptBlock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"
                              +encoded+"'))))"+'"')
                argv, executable = '"'+str(cmd)+'" /d /s /c "'+invocation+'"', str(cmd)
            self.assertLess(len(invocation), 8000)
            reader, writer = async_stdin_pipe() if sustained else (None, None)
            try:
                child = subprocess.Popen(argv, executable=executable, cwd=root, env=env,
                                         stdin=reader if sustained else subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                         creationflags=subprocess.CREATE_NO_WINDOW)
            except BaseException:
                if writer is not None: writer.close()
                raise
            finally:
                if reader is not None: os.close(reader)
            if sustained: child.stdin = writer
            readers = []
            def capture(pipe, path):
                with path.open('wb', buffering=0) as out:
                    if slow: time.sleep(3)  # keep the same reader open, without consuming
                    while True:
                        chunk = pipe.read(4096)
                        if not chunk: break
                        out.write(chunk)
                pipe.close()
            for pipe, name in ((child.stdout, 'stdout'), (child.stderr, 'stderr')):
                reader = threading.Thread(target=capture, args=(pipe, root/name))
                reader.start(); readers.append(reader)
            body = bytes(range(256)) + 'π 日本語\r\n'.encode()
            fallback = False
            try:
                child.stdin.write(struct.pack('<I', 0xffffffff)); child.stdin.flush()
                # The fake renders before writing ready; keep renewing while
                # its twenty-second output crosses OpenSSH's original lease.
                started = time.monotonic()
                deadline = started+(25 if sustained else 8)
                next_ping = started+2
                while not slow and not ready.exists():
                    if sustained and time.monotonic() >= next_ping:
                        child.stdin.write(struct.pack('<I', 0xffffffff)); child.stdin.flush()
                        next_ping = time.monotonic()+2
                    status = child.poll()
                    self.assertIsNone(status, (root/'stderr').read_bytes() if status is not None else None)
                    self.assertLess(time.monotonic(), deadline, 'original fake did not become ready')
                    time.sleep(.02)
                if not slow: self.assertEqual(json.loads(ready.read_text(encoding='utf-8'))['argv'], words)
                if sustained: self.assertGreaterEqual(time.monotonic()-started, 19)
                started = time.monotonic()
                if slow:
                    child.stdin.write(struct.pack('<I', 0)); child.stdin.flush()
                elif not lease:
                    frame = struct.pack('<I', len(body))+body
                    child.stdin.write(frame[:2]); child.stdin.flush()
                    child.stdin.write(frame[2:]+struct.pack('<I', 0)); child.stdin.flush()
                # Never close the underlying input pipe before the original
                # wrapper exits: its EOF/lease handling must do the work.
                child.wait(timeout=20 if lease else 8)
                self.assertFalse(child.stdin.closed)
                self.assertEqual(child.returncode, 127 if lease else code)
                if lease: self.assertGreaterEqual(time.monotonic()-started, 14)
                for reader in readers:
                    reader.join(timeout=3); self.assertFalse(reader.is_alive())
                if lease:
                    self.assertIn(b'remote Windows invocation failed', (root/'stderr').read_bytes())
                elif slow:
                    self.assertEqual((root/'stdout').read_bytes(), (version+'\n').encode())
                    self.assertEqual((root/'stderr').read_bytes(), b'')
                else:
                    rows = [json.loads(line) for line in record.read_text().splitlines()]
                    self.assertEqual(b''.join(bytes.fromhex(row['hex']) for row in rows if 'hex' in row), body)
                    self.assertEqual((root/'stderr').read_bytes(), b'')
                    if sustained:
                        self.assertEqual([json.loads(line) for line in (root/'stdout').read_bytes().splitlines()],
                                         [{'i': n} for n in range(20)])
            finally:
                if child.poll() is None:
                    fallback = True
                    control.write_text('exit 7\n')
                    try: child.wait(timeout=18)
                    except subprocess.TimeoutExpired:
                        child.terminate(); child.wait(timeout=5)  # same original wrapper only
                child.stdin.close()
                for reader in readers: reader.join(timeout=3)
                self.assertFalse(fallback, 'original decoder required fallback; this is a failed control')

    def test_native_decoder_preserves_argv_bytes_eof_and_exit_code(self):
        for shell in ('powershell', 'cmd'):
            for code in (0, 7):
                with self.subTest(shell=shell, code=code): self.run_decoder(shell, code=code)

    def test_lease_loss_ends_only_the_original_native_helper(self):
        for shell in ('powershell', 'cmd'):
            with self.subTest(shell=shell): self.run_decoder(shell, lease=True)

    def test_native_output_backpressure_preserves_every_byte_and_exit(self):
        for shell in ('powershell', 'cmd'):
            with self.subTest(shell=shell): self.run_decoder(shell, slow=True)

    def test_async_ssh_stdin_keeps_a_live_stream_through_repeated_lease_renewal(self):
        for shell in ('powershell', 'cmd'):
            with self.subTest(shell=shell): self.run_decoder(shell, sustained=True)
