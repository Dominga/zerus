#!/usr/bin/env python3
"""Linux built-app File reference walk using only private state and fake effects.

Run from the repository root: --app /path/to/the/built/zerus-client.
Needs Xvfb, dbus-run-session, Python GObject/Gio, libX11, libXtst and ImageMagick.
The app must include the media/custom-protocol build and production frontend.
"""
import argparse,ctypes as C,json,os,signal,subprocess,sys,tempfile,time,threading
from pathlib import Path
repo=Path(__file__).resolve().parents[2]
worker=len(sys.argv)>1 and sys.argv[1]=='--worker'
if not worker:
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--app',type=Path,required=True)
 args=parser.parse_args();app_path=args.app.resolve()
 if not app_path.is_file():parser.error('build the selected app first')
 root=Path(tempfile.mkdtemp(prefix='zerus-file-dialog-walk-'))
 for name in ('home','config','data','cache','runtime','temp','tmux','state','bin','Downloads'):(root/name).mkdir(mode=0o700)
 settings=root/'data/dev.zerus.next/settings-replay.json';settings.parent.mkdir()
 settings.write_text(json.dumps({'version':1,'values':{'downloads/folder':str(root/'configured-cache'),'downloads/cacheBytes':4096}}))
 (root/'config/user-dirs.dirs').write_text('XDG_DOWNLOAD_DIR="'+str(root/'Downloads')+'"\n')
 fake=root/'bin/fake-hgs';fake.write_text('''#!/usr/bin/python3
import json,os,sys,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parent.parent
args=sys.argv[1:]
if len(args)==6 and args[:3]==['@example-peer','download','--path'] and args[4]=='--output':
 remote,out=args[3],Path(args[5]);assert out.is_relative_to(root)
 with (root/'downloads.jsonl').open('a') as f:f.write(json.dumps({'remote':remote,'output':str(out)})+'\\n')
 if 'slow' in remote:time.sleep(.8)
 if 'failure' in remote:time.sleep(.2);print('hgs: Synthetic remote failure.',file=sys.stderr);sys.exit(1)
 payload=('Synthetic content for '+remote).encode();out.parent.mkdir(parents=True,exist_ok=True)
 fd,tmp=tempfile.mkstemp(dir=out.parent)
 with os.fdopen(fd,'wb') as f:f.write(payload)
 os.replace(tmp,out)
 print(json.dumps({'host':'example-peer','path':remote,'local_path':str(out),'bytes':len(payload)}));sys.exit(0)
print('{}');sys.exit(1)
''');fake.chmod(0o700)
 opener=root/'bin/xdg-open';opener.write_text('''#!/usr/bin/python3
import json,sys
from pathlib import Path
root=Path(__file__).resolve().parent.parent
with (root/'opened.jsonl').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')
''');opener.chmod(0o700)
 (root/'bus.conf').write_text('<busconfig><type>session</type><listen>unix:tmpdir='+str(root/'runtime')+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
 env={'PATH':str(root/'bin')+':/usr/bin:/bin','LANG':'C.UTF-8','HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_RUNTIME_HOME':str(root/'runtime'),'TMPDIR':str(root/'temp'),'TMUX_TMPDIR':str(root/'tmux'),'HGS_CONFIG_DIR':str(root/'config'),'HGS_STATE_DIR':str(root/'state'),'GDK_BACKEND':'x11','WAYLAND_DISPLAY':'zerus-nonexistent','WEBKIT_DISABLE_DMABUF_RENDERER':'1','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','ZERUS_CLIENT_HEADLESS':'1','ZERUS_CLIENT_REPLAY':str(repo/'client/fixtures/replay.ndjson'),'ZERUS_CLIENT_HGS':str(fake),'ZERUS_CLIENT_QUERY':'page=sessions&skin=zerus-light&file-reference-smoke=1','ZERUS_CLIENT_PERF':'smoke'}
 print('Private file dialog walk:',root,flush=True)
 command=['/usr/bin/dbus-run-session','--config-file',str(root/'bus.conf'),'--','/usr/bin/xvfb-run','-a','-s','-screen 0 1440x900x24','/usr/bin/python3',str(Path(__file__).resolve()),'--worker',str(root),str(app_path)]
 process=subprocess.Popen(command,env=env,start_new_session=True)
 try:code=process.wait(timeout=90)
 except subprocess.TimeoutExpired:
  # The process group was created here and contains only this private test.
  os.killpg(process.pid,signal.SIGTERM);process.wait(timeout=5);raise
 print('Private file dialog walk exit:',code);raise SystemExit(code)
root=Path(sys.argv[2]);app_path=Path(sys.argv[3]);assert os.environ['HOME']==str(root/'home')
# Mock only the native OS effects, on this private bus. No real file manager opens.
import gi
gi.require_version('Gio','2.0')
from gi.repository import Gio,GLib
conn=Gio.bus_get_sync(Gio.BusType.SESSION,None)
conn.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',('org.freedesktop.FileManager1',0)),None,Gio.DBusCallFlags.NONE,5000,None)
xml='<node><interface name="org.freedesktop.FileManager1"><method name="ShowItems"><arg type="as" direction="in"/><arg type="s" direction="in"/></method></interface></node>'
def show(connection,sender,path,interface,method,parameters,invocation):
 with (root/'shown.jsonl').open('a') as f:f.write(json.dumps(parameters.unpack())+'\n')
 invocation.return_value(GLib.Variant('()',()))
conn.register_object('/org/freedesktop/FileManager1',Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0],show,None,None)
loop=GLib.MainLoop();threading.Thread(target=loop.run,daemon=True).start()
x=C.CDLL('libX11.so.6');t=C.CDLL('libXtst.so.6');W=C.c_ulong
x.XOpenDisplay.argtypes=[C.c_char_p];x.XOpenDisplay.restype=C.c_void_p
x.XDefaultRootWindow.argtypes=[C.c_void_p];x.XDefaultRootWindow.restype=W
x.XQueryTree.argtypes=[C.c_void_p,W,C.POINTER(W),C.POINTER(W),C.POINTER(C.POINTER(W)),C.POINTER(C.c_uint)]
x.XFetchName.argtypes=[C.c_void_p,W,C.POINTER(C.c_void_p)];x.XFree.argtypes=[C.c_void_p]
x.XStringToKeysym.argtypes=[C.c_char_p];x.XStringToKeysym.restype=W
x.XKeysymToKeycode.argtypes=[C.c_void_p,W];x.XKeysymToKeycode.restype=C.c_uint
x.XSetInputFocus.argtypes=[C.c_void_p,W,C.c_int,W];x.XFlush.argtypes=[C.c_void_p]
t.XTestFakeKeyEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,W]
display=x.XOpenDisplay(None);assert display
main=x.XDefaultRootWindow(display)
def windows(parent):
 r,p=W(),W();children=C.POINTER(W)();count=C.c_uint()
 if not x.XQueryTree(display,parent,C.byref(r),C.byref(p),C.byref(children),C.byref(count)):return []
 ids=[children[i] for i in range(count.value)];x.XFree(children)
 result=[]
 for wid in ids:
  name=C.c_void_p();x.XFetchName(display,wid,C.byref(name))
  title=C.string_at(name.value).decode(errors='replace') if name.value else ''
  if name.value:x.XFree(name)
  result.append((wid,title));result.extend(windows(wid))
 return result
def key(name,down=True):t.XTestFakeKeyEvent(display,x.XKeysymToKeycode(display,x.XStringToKeysym(name.encode())),int(down),0);x.XFlush(display)
def press(name):key(name);key(name,False)
def chord(mod,name):key(mod);press(name);key(mod,False)
def text(value):
 for char in value:
  name={'/':'slash','.':'period','-':'minus','_':'underscore'}.get(char,char)
  if char=='_':key('Shift_L');press('minus');key('Shift_L',False)
  elif char.isupper():key('Shift_L');press(char.lower());key('Shift_L',False)
  else:press(name)
  time.sleep(.003)
# Fresh whitelist: only private wrapper-generated X11/D-Bus values join the
# already private task environment. No inherited owner session values exist.
env={k:v for k,v in os.environ.items() if k in {'PATH','LANG','HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_RUNTIME_HOME','TMPDIR','TMUX_TMPDIR','HGS_CONFIG_DIR','HGS_STATE_DIR','GDK_BACKEND','WAYLAND_DISPLAY','WEBKIT_DISABLE_DMABUF_RENDERER','NO_AT_BRIDGE','GSETTINGS_BACKEND','DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY'} or k.startswith('ZERUS_CLIENT_')}
with (root/'app.log').open('wb') as log:
 app=subprocess.Popen([str(app_path)],env=env,stdout=log,stderr=log,cwd=repo/'client')
 stages=set();deadline=time.monotonic()+75
 try:
  while time.monotonic()<deadline:
   output=(root/'app.log').read_text()
   for stage in ('INITIAL','SAVED','FAILED'):
    if 'file-reference-smoke: '+stage in output and stage not in stages:
     stages.add(stage);time.sleep(.2);subprocess.run(['/usr/bin/import','-window','root',str(root/(stage.lower()+'.png'))],check=True,env=env)
   if 'file-reference-smoke: SAVE_DIALOG' in output and 'save' not in stages:
    candidates=[(wid,title) for wid,title in windows(main) if 'Save local copy' in title]
    if candidates:
     wid,title=candidates[-1];x.XSetInputFocus(display,wid,1,0);x.XFlush(display);time.sleep(.25)
     chord('Control_L','l');time.sleep(.15);chord('Control_L','a');text(str(root/'Downloads/saved.md'));press('Return');stages.add('save')
   if 'file-reference-smoke: ESCAPE' in output and 'escape' not in stages:
    press('Escape');stages.add('escape')
   if app.poll() is not None:break
   time.sleep(.05)
  else:raise RuntimeError('walk timeout')
  print('\n'.join(line for line in output.splitlines() if 'file-reference-smoke:' in line))
  assert app.returncode==0,output
  downloads=[json.loads(line) for line in (root/'downloads.jsonl').read_text().splitlines()]
  assert len(downloads)==5,downloads
  assert all(Path(d['output']).is_relative_to(root) for d in downloads)
  assert (root/'Downloads/saved.md').read_text()=='Synthetic content for /srv/example/save.md'
  opened=[json.loads(line) for line in (root/'opened.jsonl').read_text().splitlines()]
  assert len(opened)==3,opened
  shown=[json.loads(line) for line in (root/'shown.jsonl').read_text().splitlines()]
  assert len(shown)==1,shown
  assert not list((root/'configured-cache').glob('*.part'))
  print('PASS native save dialog, cache, open/copy/show, failure, Escape/focus, closed completion; only private fake transfers and OS effects')
 finally:
  if app.poll() is None:app.terminate();app.wait(timeout=5)
 loop.quit()
