#!/usr/bin/python3
"""Two built-app launches, private state/bus/Xvfb and one fake color write."""
import argparse, ctypes, json, os, signal, subprocess, sys, tempfile, time
from pathlib import Path
REPO = Path(__file__).resolve().parents[2]
if '--worker' not in sys.argv:
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--app',type=Path,required=True);args=parser.parse_args();app=args.app.resolve();assert app.is_file()
    root=Path(tempfile.mkdtemp(prefix='zerus-project-vivid-'))
    for name in ('home','config','data','cache','runtime','tmp','tmux','state','bin'):(root/name).mkdir(mode=0o700)
    frames=[json.loads(line) for line in (REPO/'client/fixtures/replay.ndjson').read_text().splitlines()]
    parts={i['key']:i['row'] for f in frames if f.get('topic')=='organization' for i in f.get('items',[])}
    snapshot=parts['organization'];ids=snapshot['organization']['projects'];projects=[]
    for key in ids:
        project=dict(parts['project:'+key]);chunks=project.pop('session_chunks',0)
        project['sessions']=[s for n in range(chunks) for s in parts[f'members:{key}:{n}']['sessions']];projects.append(project)
    snapshot['organization']['projects']=projects;snapshot['versions']={key[len('version:'):]:value for key,value in parts.items() if key.startswith('version:')}
    (root/'snapshot.json').write_text(json.dumps(snapshot))
    fake=root/'bin/fake-hgs';fake.write_text('''#!/usr/bin/python3
import json,sys
from pathlib import Path
root=Path(__file__).resolve().parent.parent
args=sys.argv[1:]
with (root/'calls.jsonl').open('a') as f:f.write(json.dumps(args)+'\\n')
if args==['swarm','apply']:
 req=json.load(sys.stdin)
 with (root/'applies.jsonl').open('a') as f:f.write(json.dumps(req)+'\\n')
 value=json.loads((root/'snapshot.json').read_text());value['organization']=req['desired'];value['digest']='color-1'
 token=json.dumps({'kind':'project','id':'p-sample-project','field':'color'},separators=(',',':'))
 value['versions'][token]=['color-op'];value['written']={token:['color-op']};(root/'snapshot.json').write_text(json.dumps(value));print(json.dumps(value));sys.exit(0)
if args==['swarm','get']:print((root/'snapshot.json').read_text());sys.exit(0)
print('{}');sys.exit(1)
''');fake.chmod(0o700)
    (root/'bus.conf').write_text('<busconfig><type>session</type><listen>unix:tmpdir='+str(root/'runtime')+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    env={'PATH':str(root/'bin')+':/usr/bin:/bin','LANG':'C.UTF-8','HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_RUNTIME_HOME':str(root/'runtime'),'TMPDIR':str(root/'tmp'),'TMUX_TMPDIR':str(root/'tmux'),'HGS_CONFIG_DIR':str(root/'config'),'HGS_STATE_DIR':str(root/'state'),'GDK_BACKEND':'x11','WAYLAND_DISPLAY':'zerus-nonexistent','WEBKIT_DISABLE_DMABUF_RENDERER':'1','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','ZERUS_CLIENT_HEADLESS':'1','ZERUS_CLIENT_REPLAY':str(REPO/'client/fixtures/replay.ndjson'),'ZERUS_CLIENT_HGS':str(fake),'ZERUS_CLIENT_PERF':'smoke'}
    print('Private project vivid walk:',root,flush=True)
    child=subprocess.Popen(['/usr/bin/dbus-run-session','--config-file',str(root/'bus.conf'),'--','/usr/bin/xvfb-run','-a','-s','-screen 0 1440x900x24','/usr/bin/python3',str(Path(__file__).resolve()),'--worker',str(root),str(app)],env=env,start_new_session=True)
    try:result=child.wait(timeout=100)
    except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGTERM);child.wait(timeout=5);raise
    raise SystemExit(result)
root,app=Path(sys.argv[2]),Path(sys.argv[3]);assert os.environ['HOME']==str(root/'home')
env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_RUNTIME_HOME','TMPDIR','TMUX_TMPDIR','HGS_CONFIG_DIR','HGS_STATE_DIR','GDK_BACKEND','WAYLAND_DISPLAY','WEBKIT_DISABLE_DMABUF_RENDERER','NO_AT_BRIDGE','GSETTINGS_BACKEND','DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY'} or k.startswith('ZERUS_CLIENT_')}
x=ctypes.CDLL('libX11.so.6');t=ctypes.CDLL('libXtst.so.6');x.XOpenDisplay.argtypes=[ctypes.c_char_p];x.XOpenDisplay.restype=ctypes.c_void_p;x.XStringToKeysym.argtypes=[ctypes.c_char_p];x.XStringToKeysym.restype=ctypes.c_ulong;x.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong];x.XKeysymToKeycode.restype=ctypes.c_uint;x.XFlush.argtypes=[ctypes.c_void_p];t.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong];display=x.XOpenDisplay(None);assert display
for mode in ('first','restored'):
    case_env=dict(env);case_env['ZERUS_CLIENT_QUERY']='page=projects&skin=zerus-light&project-vivid-smoke='+mode
    path=root/(mode+'.log')
    with path.open('wb') as log:
        child=subprocess.Popen([str(app)],cwd=REPO/'client',env=case_env,stdout=log,stderr=log);stages=set();deadline=time.monotonic()+40
        try:
            while time.monotonic()<deadline:
                output=path.read_text()
                for stage in ('SOFT','BRIGHT','SAVED','RESTORED'):
                    if 'project-vivid-smoke: '+stage in output and stage not in stages:
                        stages.add(stage);time.sleep(.15);subprocess.run(['/usr/bin/import','-window','root',str(root/(stage.lower()+'.png'))],check=True,env=case_env)
                if 'project-vivid-smoke: ESCAPE' in output and 'escape' not in stages:
                    key=x.XKeysymToKeycode(display,x.XStringToKeysym(b'Escape'));t.XTestFakeKeyEvent(display,key,1,0);t.XTestFakeKeyEvent(display,key,0,0);x.XFlush(display);stages.add('escape')
                if child.poll() is not None:break
                time.sleep(.03)
            else:raise RuntimeError('project vivid walk timed out')
            print('\n'.join(l for l in output.splitlines() if 'project-vivid-smoke:' in l));assert child.returncode==0,output
        finally:
            if child.poll() is None:child.terminate();child.wait(timeout=5)
    saved=json.loads((root/'data/dev.zerus.next/settings-replay.json').read_text())
    assert saved['values']['projects/vivid/p-sample-project'] is False
applies=[json.loads(line) for line in (root/'applies.jsonl').read_text().splitlines()];assert len(applies)==1,applies
base,desired=applies[0]['base'],applies[0]['desired'];expected=json.loads(json.dumps(base))
next(p for p in expected['projects'] if p['id']=='p-sample-project')['color']='#ffcf33'
assert expected==desired,'color changed other shared fields'
assert not any('vivid' in key for key in desired), 'local style leaked into shared request'
print('PASS popup preview/Cancel/invalid-color, local exact-ID Apply, one existing shared color edit, false persisted across restart, Escape/focus; private state and fake hgs only')
