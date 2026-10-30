#!/usr/bin/python3
"""Built-app Terminate cleanup walk, private D-Bus/Xvfb and fake hgs only."""
import argparse, ctypes, json, os, signal, subprocess, sys, tempfile, time
from pathlib import Path
REPO = Path(__file__).resolve().parents[2]
MACHINE = '11111111-1111-4111-8111-111111111111'
if '--worker' not in sys.argv:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    args = parser.parse_args()
    app = args.app.resolve()
    assert app.is_file()
    root = Path(tempfile.mkdtemp(prefix='zerus-cleanup-walk-'))
    for name in ('home', 'config', 'data', 'cache', 'runtime', 'tmp', 'tmux', 'state', 'bin', 'repo'):
        (root / name).mkdir(mode=0o700)
    (root/'repo/.git').mkdir()
    tags = ('clean', 'kept', 'unknown', 'unknown-only', 'changed')
    rows = []
    for tag in tags:
        folder = root/'repo'/tag
        folder.mkdir()
        (folder/'changes.txt').write_text('Synthetic local changes only.\n')
        rows.append({'key': 'run:'+tag, 'row': {'name': 'codex/example/'+tag, 'cmd': 'codex', 'agent': 'codex', 'project': 'example', 'tag': tag, 'run_id': 'run-'+tag, 'conversation_id': 'conv-'+tag, 'tracked': True, 'resumable': True, 'state': 'running', 'runtime_state': 'live', 'process_state': 'running', 'phase': 'idle', 'activity': 'idle', 'cwd': str(folder), 'git_root': str(folder), 'git_common_dir': str(root/'repo/.git'), 'git_worktree': True, 'git_branch': tag, 'worktree_owner_id': 'owner-'+tag}})
    frames = [
        {'v':1,'seq':1,'t':'hello','machine_id':MACHINE,'host':'example-host','hgs_version':'1.56.0','stream':'synthetic-cleanup','heartbeat_ms':10000,'max_frame_bytes':1048576,'topics':['catalog','catalog.meta'],'limits':{'activity_subscriptions':16,'page_bytes':524288,'queue_bytes':4194304,'activity_tail':100,'activity_read':200,'control_line_bytes':65536,'git_subscriptions':64,'processes_subscriptions':16}},
        {'v':1,'seq':2,'t':'snapshot','topic':'catalog','items':rows,'page':0,'last':True,'cursor':1},
        {'v':1,'seq':3,'t':'snapshot','topic':'catalog.meta','value':{'host':'example-host','ok':True,'hgs_version':'1.56.0','projects':{},'peers':[]},'page':0,'last':True,'cursor':1},
        {'v':1,'seq':4,'t':'heartbeat','at':1793000000.0},
    ]
    (root/'replay.ndjson').write_text(''.join(json.dumps(f)+'\n' for f in frames))
    fake = root/'bin/fake-hgs'
    fake.write_text('''#!/usr/bin/python3
import json,sys,time
from pathlib import Path
root=Path(__file__).resolve().parent.parent
args=sys.argv[1:]
with (root/'calls.jsonl').open('a') as f:f.write(json.dumps(args)+'\\n')
if args==['--help']:
 print('guarded-create-v1\\n hgs [@host] session-action <session> --json scoped native lifecycle action');sys.exit(0)
assert args[:2]==['--expect-machine','11111111-1111-4111-8111-111111111111'],args
args=args[2:]
def arg(key):return args[args.index(key)+1]
if args[0]=='inspect':
 name=args[1];tag=name.rsplit('/',1)[-1];counter=root/('inspect-'+tag)
 n=int(counter.read_text())+1 if counter.exists() else 1;counter.write_text(str(n))
 print(json.dumps({'name':name,'run_id':'run-'+tag,'conversation_id':'conv-'+tag,'tracked':True,'state':'running','cmd':'codex','git_worktree':True,'git_root':str(root/'repo'/tag),'git_common_dir':str(root/'repo/.git'),'worktree_owner_id':'replacement' if tag=='changed' and n>1 else 'owner-'+tag}));sys.exit(0)
if args[0]=='session-action':
 name=args[1];tag=name.rsplit('/',1)[-1];data=json.load(sys.stdin)
 assert data['action']=='terminate' and data['expected_run_id']=='run-'+tag and data['expected_conversation_id']=='conv-'+tag
 assert tag!='changed','changed source must never reach mutation'
 print(json.dumps({'request_id':data['request_id'],'name':name,'run_id':'run-'+tag,'conversation_id':'conv-'+tag,'status':'completed','result_target':{'name':name,'run_id':'run-'+tag,'conversation_id':'conv-'+tag,'archive_id':'22222222-2222-4222-8222-222222222222'}}));sys.exit(0)
if args[:2]==['worktrees','review']:
 path=Path(arg('--worktree'));tag=path.name;common=arg('--path');assert path.is_relative_to(root)
 if tag=='clean':time.sleep(2)
 row={'path':str(path),'kind':'linked','branch':tag,'zerus_owned':True,'ownership_id':'owner-'+tag,'archive_restore_ready':True,'verdict':'ready','fingerprint':'fingerprint-'+tag,'reasons':[],'changes':0,'untracked':0,'disk_complete':True,'ignored_complete':True,'ignored_count':0,'integration':{'kind':'ancestor'},'disk_bytes':1200}
 if tag=='kept':row.update({'verdict':'blocked','changes':1,'discardable':True,'discard_fingerprint':'discard-kept','reasons':[{'code':'changes','message':'Uncommitted changes need review.'}]})
 print(json.dumps({'state':'ok','common_dir':common,'worktrees':[row]}));sys.exit(0)
if args[:2]==['worktrees','remove']:
 path=Path(arg('--path'));tag=path.name;assert path.is_relative_to(root)
 assert '--delete-branch' not in args
 assert '--preserve-archive-restore' in args
 assert '--process-fingerprint' not in args
 if tag=='kept':assert arg('--discard-fingerprint')=='discard-kept'
 if tag.startswith('unknown'):print('Synthetic unknown after issue',file=sys.stderr);print('{}');sys.exit(1)
 print(json.dumps({'request_id':arg('--request-id'),'path':str(path),'status':'removed'}));sys.exit(0)
if args[:2]==['worktrees','inspect']:
 path=arg('--path');common=arg('--common-dir');assert Path(path).is_relative_to(root)
 data={'state':'ok','path':path,'common_dir':common,'files':[{'path':'changes.txt','status':' M'}],'changes':{'text':'Synthetic read-only diff'},'commits':{'text':'No additional commits.'},'review':{'sessions':[]}}
 if '--file' in args:data['file']={'path':arg('--file'),'entries':[{'path':'changes.txt','directory':False,'symlink':False}],'offset':0}
 print(json.dumps(data));sys.exit(0)
raise RuntimeError(args)
''')
    fake.chmod(0o700)
    (root/'bus.conf').write_text('<busconfig><type>session</type><listen>unix:tmpdir='+str(root/'runtime')+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    env = {'PATH':str(root/'bin')+':/usr/bin:/bin','LANG':'C.UTF-8','HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_RUNTIME_HOME':str(root/'runtime'),'TMPDIR':str(root/'tmp'),'TMUX_TMPDIR':str(root/'tmux'),'HGS_CONFIG_DIR':str(root/'config'),'HGS_STATE_DIR':str(root/'state'),'GDK_BACKEND':'x11','WAYLAND_DISPLAY':'zerus-nonexistent','WEBKIT_DISABLE_DMABUF_RENDERER':'1','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','ZERUS_CLIENT_HEADLESS':'1','ZERUS_CLIENT_REPLAY':str(root/'replay.ndjson'),'ZERUS_CLIENT_HGS':str(fake),'ZERUS_CLIENT_QUERY':'page=sessions&skin=zerus-light&termination-cleanup-smoke=1','ZERUS_CLIENT_PERF':'smoke'}
    print('Private Terminate cleanup walk:', root, flush=True)
    child = subprocess.Popen(['/usr/bin/dbus-run-session','--config-file',str(root/'bus.conf'),'--','/usr/bin/xvfb-run','-a','-s','-screen 0 1440x900x24','/usr/bin/python3',str(Path(__file__).resolve()),'--worker',str(root),str(app)],env=env,start_new_session=True)
    try: result=child.wait(timeout=95)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid,signal.SIGTERM);child.wait(timeout=5);raise
    raise SystemExit(result)
root, app = Path(sys.argv[2]), Path(sys.argv[3])
assert os.environ['HOME']==str(root/'home')
# Private wrapper-generated X11 and D-Bus only; no owner environment inherited.
env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_RUNTIME_HOME','TMPDIR','TMUX_TMPDIR','HGS_CONFIG_DIR','HGS_STATE_DIR','GDK_BACKEND','WAYLAND_DISPLAY','WEBKIT_DISABLE_DMABUF_RENDERER','NO_AT_BRIDGE','GSETTINGS_BACKEND','DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY'} or k.startswith('ZERUS_CLIENT_')}
x=ctypes.CDLL('libX11.so.6');t=ctypes.CDLL('libXtst.so.6')
x.XOpenDisplay.argtypes=[ctypes.c_char_p];x.XOpenDisplay.restype=ctypes.c_void_p
x.XStringToKeysym.argtypes=[ctypes.c_char_p];x.XStringToKeysym.restype=ctypes.c_ulong
x.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong];x.XKeysymToKeycode.restype=ctypes.c_uint
x.XFlush.argtypes=[ctypes.c_void_p];t.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]
display=x.XOpenDisplay(None);assert display
with (root/'app.log').open('wb') as log:
    child=subprocess.Popen([str(app)],env=env,stdout=log,stderr=log,cwd=REPO/'client')
    stages=set();deadline=time.monotonic()+85
    try:
        while time.monotonic()<deadline:
            output=(root/'app.log').read_text()
            for stage in ('CLEANING','MIXED','REOPENED','UNKNOWN','INSPECTION'):
                if 'cleanup-smoke: '+stage in output and stage not in stages:
                    stages.add(stage);time.sleep(.15)
                    subprocess.run(['/usr/bin/import','-window','root',str(root/(stage.lower()+'.png'))],check=True,env=env)
            if 'cleanup-smoke: ESCAPE_CLEANING' in output and 'escape' not in stages:
                code=x.XKeysymToKeycode(display,x.XStringToKeysym(b'Escape'))
                t.XTestFakeKeyEvent(display,code,1,0);t.XTestFakeKeyEvent(display,code,0,0);x.XFlush(display);stages.add('escape')
            if child.poll() is not None: break
            time.sleep(.03)
        else: raise RuntimeError('private built-app walk timed out')
        print('\n'.join(l for l in output.splitlines() if 'cleanup-smoke:' in l))
        assert child.returncode==0, output
        calls=[json.loads(l) for l in (root/'calls.jsonl').read_text().splitlines()]
        effects=[c for c in calls if 'session-action' in c]
        removals=[c for c in calls if 'remove' in c]
        assert len(effects)==4,effects
        assert len(removals)==4,removals
        assert len([c for c in removals if c[c.index('--path')+1].endswith('/unknown')])==1,'unknown was repeated'
        assert all('--expect-machine' in c for c in effects+removals)
        assert all((root/'repo'/tag/'changes.txt').exists() for tag in ('clean','kept','unknown','unknown-only','changed')),'fake hgs must not delete even fixture files'
        assert {'CLEANING','MIXED','REOPENED','UNKNOWN','INSPECTION'}<=stages
        print('PASS exact scoped fake termination; generated cleanup; explicit discard; unknown non-replay; source change refusal; chip, review, inspection, Escape and Keep; no real removal')
    finally:
        if child.poll() is None: child.terminate();child.wait(timeout=5)
