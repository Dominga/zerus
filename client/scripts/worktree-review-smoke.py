#!/usr/bin/python3
"""Native Worktrees walk in private D-Bus/Xvfb/state, with fake hgs only."""
import argparse, ctypes, json, os, signal, subprocess, sys, tempfile, time
from pathlib import Path
REPO = Path(__file__).resolve().parents[2]
MACHINE = '11111111-1111-4111-8111-111111111111'
if '--worker' not in sys.argv:
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--app',type=Path,required=True);parser.add_argument('--shell-badge',action='store_true');options=parser.parse_args();app=options.app.resolve();assert app.is_file()
    root=Path(tempfile.mkdtemp(prefix='zerus-worktree-review-'))
    for name in ('home','config','data','cache','runtime','tmp','tmux','state','bin','repo'):(root/name).mkdir(mode=0o700)
    (root/'repo/.git').mkdir()
    for tag in ('clean','ignored','unknown','changed','blocked'):
        (root/'repo'/tag).mkdir();(root/'repo'/tag/'fixture.txt').write_text('Synthetic fixture; fake hgs never deletes it.\n')
    frames=[
        {'v':1,'seq':1,'t':'hello','machine_id':MACHINE,'host':'example-host','hgs_version':'1.56.0','stream':'synthetic-review','heartbeat_ms':10000,'max_frame_bytes':1048576,'topics':['catalog','catalog.meta'],'limits':{'activity_subscriptions':16,'page_bytes':524288,'queue_bytes':4194304,'activity_tail':100,'activity_read':200,'control_line_bytes':65536,'git_subscriptions':64,'processes_subscriptions':16}},
        {'v':1,'seq':2,'t':'snapshot','topic':'catalog','items':[{'key':'run:probe','row':{'name':'codex/example/probe','cmd':'codex','agent':'codex','project':'example','tag':'probe','run_id':'run-probe','conversation_id':'conv-probe','tracked':True,'resumable':True,'state':'running','runtime_state':'live','phase':'idle','activity':'idle','cwd':str(root/'repo'),'cwd_canonical':str(root/'repo'),'git_root':str(root/'repo'),'git_branch':'main'}}],'page':0,'last':True,'cursor':1},
        {'v':1,'seq':3,'t':'snapshot','topic':'catalog.meta','value':{'host':'example-host','ok':True,'hgs_version':'1.56.0','projects':{},'peers':[]},'page':0,'last':True,'cursor':1},
        {'v':1,'seq':4,'t':'heartbeat','at':1793000000.0},
    ]
    if options.shell_badge:
        first=frames[1]['items'][0]
        comparison=json.loads(json.dumps(first));comparison['key']='run:comparison';comparison['row'].update({'name':'codex/example/compare','run_id':'run-comparison','conversation_id':'conv-comparison','tag':'compare'})
        first['row'].update({'name':'sh/example/probe','cmd':'sh','agent':'sh'})
        frames[1]['items'].append(comparison)
    (root/'replay.ndjson').write_text(''.join(json.dumps(f)+'\n' for f in frames))
    fake=root/'bin/fake-hgs';fake.write_text('''#!/usr/bin/python3
import json,sys,time
from pathlib import Path
root=Path(__file__).resolve().parent.parent;args=sys.argv[1:]
with (root/'calls.jsonl').open('a') as f:f.write(json.dumps(args)+'\\n')
if args==['--help']:print('guarded-create-v1');sys.exit(0)
guarded=args[:2]==['--expect-machine','11111111-1111-4111-8111-111111111111']
if guarded:args=args[2:]
def arg(key):return args[args.index(key)+1]
def row(tag):
 value={'path':str(root/'repo'/tag),'kind':'linked','available':tag!='missing','branch':tag,'head':'head-'+tag,'merged':tag in ('clean','ignored','changed'),'verdict':'missing' if tag=='missing' else 'ready','fingerprint':'fp-'+tag,'reasons':[],'ignored_count':0,'changes':0,'untracked':0,'integration':{'kind':'ancestor','base':'main'}}
 if tag=='ignored':value.update({'verdict':'review','ignored_count':1,'ignored_bytes':12288,'ignored_fingerprint':'ignored-proof','ignored':[{'path':'target/','bytes':12288,'complete':True}]})
 if tag=='blocked':value.update({'verdict':'blocked','reasons':[{'code':'processes','message':'A running program uses this folder.'}]})
 return value
if args and args[0]=='worktrees' and (len(args)<2 or args[1].startswith('--')):
 path=arg('--path');assert Path(path).is_relative_to(root)
 tags=[t for t in ('clean','ignored','missing','unknown') if not (root/('removed-'+t)).exists()]
 value={'state':'ok','path':path,'selected_root':str(root/'repo'),'common_dir':str(root/'repo/.git'),'sampled_at':time.time(),'worktrees':[{'path':str(root/'repo'),'kind':'main','available':True,'branch':'main'},*[row(t) for t in tags]]}
 print(json.dumps(value));sys.exit(0)
if args[:2]==['worktrees','review']:
 assert guarded and arg('--path')==str(root/'repo/.git')
 tags=[t for t in ('clean','ignored','unknown','changed','missing','blocked') if not (root/('removed-'+t)).exists()]
 if '--worktree' in args:tags=[t for t in tags if str(root/'repo'/t)==arg('--worktree')]
 print(json.dumps({'state':'ok','common_dir':str(root/'repo/.git'),'worktrees':[row(t) for t in tags]}));sys.exit(0)
if args[:2]==['worktrees','remove']:
 assert guarded and arg('--common-dir')==str(root/'repo/.git')
 path=Path(arg('--path'));assert path.is_relative_to(root);tag=path.name
 assert tag!='blocked' and '--discard-fingerprint' not in args and '--force' not in args
 assert '--preserve-archive-restore' not in args and '--process-fingerprint' not in args
 assert arg('--fingerprint')=='fp-'+tag
 assert ('--delete-branch' in args)==(tag in ('clean','ignored','changed'))
 if tag=='ignored':assert arg('--ignored-fingerprint')=='ignored-proof'
 if tag=='clean':time.sleep(1)
 if tag=='unknown':print('{}');print('Synthetic unknown after issue',file=sys.stderr);sys.exit(1)
 if tag=='changed':print(json.dumps({'request_id':arg('--request-id'),'path':str(path),'common_dir':arg('--common-dir'),'operation':'worktree_remove','outcome_version':1,'status':'kept','detail':'Worktree changed since the review. Review it again.'}));sys.exit(1)
 (root/('removed-'+tag)).touch()
 print(json.dumps({'request_id':arg('--request-id'),'path':str(path),'common_dir':arg('--common-dir'),'status':'forgotten' if tag=='missing' else 'removed'}));sys.exit(0)
if 'create' in args or 'start' in args or 'session-action' in args:raise RuntimeError('inspection attempted creation/mutation: '+repr(args))
print('{}')
''');fake.chmod(0o700)
    (root/'bus.conf').write_text('<busconfig><type>session</type><listen>unix:tmpdir='+str(root/'runtime')+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    env={'PATH':str(root/'bin')+':/usr/bin:/bin','LANG':'C.UTF-8','HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_RUNTIME_HOME':str(root/'runtime'),'TMPDIR':str(root/'tmp'),'TMUX_TMPDIR':str(root/'tmux'),'HGS_CONFIG_DIR':str(root/'config'),'HGS_STATE_DIR':str(root/'state'),'GDK_BACKEND':'x11','WAYLAND_DISPLAY':'zerus-nonexistent','WEBKIT_DISABLE_DMABUF_RENDERER':'1','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','ZERUS_CLIENT_HEADLESS':'1','ZERUS_CLIENT_REPLAY':str(root/'replay.ndjson'),'ZERUS_CLIENT_HGS':str(fake),'ZERUS_CLIENT_QUERY':'page=sessions&skin=zerus-light&worktree-review-smoke=1','ZERUS_CLIENT_PERF':'smoke'}
    if options.shell_badge:env['ZERUS_CLIENT_QUERY']='page=sessions&skin=zerus-light&shell-badge-smoke=1'
    print('Private Worktrees walk:',root,flush=True)
    child=subprocess.Popen(['/usr/bin/dbus-run-session','--config-file',str(root/'bus.conf'),'--','/usr/bin/xvfb-run','-a','-s','-screen 0 1440x900x24','/usr/bin/python3',str(Path(__file__).resolve()),'--worker',str(root),str(app),*(['--shell-badge'] if options.shell_badge else [])],env=env,start_new_session=True)
    try:result=child.wait(timeout=110)
    except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGTERM);child.wait(timeout=5);raise
    raise SystemExit(result)
root,app=Path(sys.argv[2]),Path(sys.argv[3]);assert os.environ['HOME']==str(root/'home')
shell_badge='--shell-badge' in sys.argv
prefix='shell-badge-smoke: ' if shell_badge else 'worktree-review-smoke: '
env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_RUNTIME_HOME','TMPDIR','TMUX_TMPDIR','HGS_CONFIG_DIR','HGS_STATE_DIR','GDK_BACKEND','WAYLAND_DISPLAY','WEBKIT_DISABLE_DMABUF_RENDERER','NO_AT_BRIDGE','GSETTINGS_BACKEND','DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY'} or k.startswith('ZERUS_CLIENT_')}
x=ctypes.CDLL('libX11.so.6');t=ctypes.CDLL('libXtst.so.6');x.XOpenDisplay.argtypes=[ctypes.c_char_p];x.XOpenDisplay.restype=ctypes.c_void_p;x.XStringToKeysym.argtypes=[ctypes.c_char_p];x.XStringToKeysym.restype=ctypes.c_ulong;x.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong];x.XKeysymToKeycode.restype=ctypes.c_uint;x.XFlush.argtypes=[ctypes.c_void_p];t.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong];display=x.XOpenDisplay(None);assert display
with (root/'app.log').open('wb') as log:
    child=subprocess.Popen([str(app)],cwd=REPO/'client',env=env,stdout=log,stderr=log);stages=set();deadline=time.monotonic()+100
    try:
        while time.monotonic()<deadline:
            output=(root/'app.log').read_text()
            for stage in (('SHELL',) if shell_badge else ('PANEL','REVIEW','CONFIRMATION','REMOVING','OUTCOMES','UNKNOWN_REOPEN')):
                if prefix+stage in output and stage not in stages:
                    stages.add(stage);time.sleep(.1);subprocess.run(['/usr/bin/import','-window','root',str(root/(stage.lower()+'.png'))],check=True,env=env)
            for stage in ('ESCAPE_CONFIRM','ESCAPE_REVIEW'):
                if 'worktree-review-smoke: '+stage in output and stage not in stages:
                    stages.add(stage);key=x.XKeysymToKeycode(display,x.XStringToKeysym(b'Escape'));t.XTestFakeKeyEvent(display,key,1,0);t.XTestFakeKeyEvent(display,key,0,0);x.XFlush(display)
            if child.poll() is not None:break
            time.sleep(.03)
        else:raise RuntimeError('private Worktrees walk timed out')
        print('\n'.join(line for line in output.splitlines() if prefix in line));assert child.returncode==0,output
        calls=[json.loads(line) for line in (root/'calls.jsonl').read_text().splitlines()];effects=[a for a in calls if 'remove' in a]
        if shell_badge:
            assert not effects and not any('create' in a or 'start' in a or 'session-action' in a for a in calls)
            assert 'SHELL' in stages
            print('PASS synthetic sh list/header Shell label, terminal glyph and existing default tint; Codex unchanged; no agent or operation started')
            raise SystemExit(0)
        assert len(effects)==5,effects
        assert all('--expect-machine' in a for a in effects)
        assert len([a for a in effects if a[a.index('--path')+1].endswith('/unknown')])==1,'unknown replayed'
        assert all((root/'repo'/tag/'fixture.txt').is_file() for tag in ('clean','ignored','unknown','changed','blocked'))
        assert not any('create' in a or 'start' in a or 'session-action' in a for a in calls),'opening/inspection created work'
        print('PASS Worktrees menu/copy/New session without creation; focused missing review; explicit ignored/branch consent; exact guarded fake removal; kept and unknown outcomes; no replay; real Escape/focus; fixture files unchanged')
    finally:
        if child.poll() is None:child.terminate();child.wait(timeout=5)
