#!/usr/bin/env python3
"""Private reproducible search campaign; all state is synthetic and removed."""
import hashlib,json,os,re,subprocess,sys,tempfile,time,uuid
from pathlib import Path
repo=Path(__file__).resolve().parents[1];out=repo/'artifacts/test-results/search-recovery';out.mkdir(parents=True,exist_ok=True)
binary=Path(sys.argv[1]).resolve();count=int(sys.argv[2]);size=int(sys.argv[3]) if len(sys.argv)>3 else 270_000
order=sys.argv[4] if len(sys.argv)>4 else 'legacy-first'
if order not in ['legacy-first','indexed-first']:raise SystemExit('Order must be legacy-first or indexed-first.')
if not 1<=count<=50_000 or not 0<=size<=64<<20:raise SystemExit('Use 1..50000 sessions and 0..64 MiB private payloads.')
if not (sys.platform.startswith('linux') or sys.platform=='darwin'):raise SystemExit('This manual benchmark needs Linux or macOS wait4 accounting.')
scratch=Path(os.environ.get('SEARCH_SCRATCH',tempfile.gettempdir()));scratch.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='search-p1-',dir=scratch) as raw:
 root=Path(raw).resolve();state=root/'sessions';state.mkdir(mode=0o700)
 for d in ['config','tmp','tmux','home']:(root/d).mkdir(mode=0o700)
 env={k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ['TMUX','TMUX_PANE']}
 env.update(HOME=str(root/'home'),HGS_STATE_DIR=str(state),HGS_CONFIG_DIR=str(root/'config'),TMPDIR=str(root/'tmp'),TMUX_TMPDIR=str(root/'tmux'))
 if sys.platform.startswith('linux'):
  timer=root/'search-time'
  subprocess.run(['cc','-O2','-std=c11','-Wall','-Wextra','-Werror',str(repo/'bench/search-time.c'),'-o',str(timer)],check=True,timeout=15)
 for n in range(count):
  conv=str(uuid.UUID(int=n+1));transcript=root/(conv+'.jsonl')
  transcript.write_text(json.dumps(dict(type='event_msg',timestamp='2026-11-14T10:00:00Z',payload=dict(type='agent_message',id='result',message='public benchmark needle' if n==count-1 else 'ordinary public message',phase='final')))+'\n')
  record=dict(version=1,agent='codex',name=f'codex/example/session-{n:05}',run_id=f'run-{n}',conversation_id=conv,transcript=str(transcript),updated=n,created=n,cwd='/srv/example',prompt='p'*4096,last_message='a'*4096,shell_jobs=dict(job=dict(output='x'*size)))
  (state/(hashlib.sha256(record['name'].encode()).hexdigest()+'.json')).write_text(json.dumps(record,separators=(',',':'),sort_keys=True))
 # Model existing durable records, not an unflushed multi-GB bulk import.
 def settle(path):
  with path.open('rb+') as handle:os.fsync(handle.fileno())
 from concurrent.futures import ThreadPoolExecutor
 with ThreadPoolExecutor(max_workers=4) as pool:
  list(pool.map(settle,[*state.glob('*.json'),*root.glob('*.jsonl')]))
 for directory in [state,root]:
  descriptor=os.open(directory,os.O_RDONLY|os.O_DIRECTORY)
  try:os.fsync(descriptor)
  finally:os.close(descriptor)
 results=[]
 def call(label,args,timeout=30):
  start=time.monotonic();io={};usage=None;usage_path=root/(label+'.usage')
  command=([str(timer),str(usage_path)] if sys.platform.startswith('linux') else ['/usr/bin/time','-l'])+[str(binary),*args]
  with tempfile.TemporaryFile() as stdout,tempfile.TemporaryFile() as stderr:
   p=subprocess.Popen(command,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
   if sys.platform.startswith('linux'):
    while not os.waitid(os.P_PID,p.pid,os.WEXITED|os.WNOHANG|os.WNOWAIT):
     if time.monotonic()-start>timeout:
      import signal
      os.killpg(p.pid,signal.SIGKILL)
     time.sleep(.005)
    _,status,usage=os.wait4(p.pid,0);p.returncode=os.waitstatus_to_exitcode(status)
   else:
    _,status,usage=os.wait4(p.pid,0);p.returncode=os.waitstatus_to_exitcode(status)
   stdout.seek(0);stderr.seek(0)
   errors=stderr.read();(out/f"bench-stderr-{binary.name}-{count}-{label}.log").write_bytes(errors);stderr.seek(0)
   if sys.platform.startswith('linux'):
    measured=usage_path.read_text().strip().split();peak=int(measured[0])
    io={key:None if int(value)==2**64-1 else int(value) for key,value in zip(['rchar','read_bytes'],measured[1:3])}
    peak_source='proc-vmhwm-1ms' if int(measured[3]) else 'wait4-upper-bound'
   else:
    peak=int(re.search(rb'(\d+)\s+maximum resident set size',errors).group(1))/1024;peak_source='time-child'
   try:value=json.load(stdout)
   except Exception:value=dict(error=stderr.read().decode(errors='replace')[:500])
  try:
   with (state/'content-search-v1/worker.json').open() as handle:worker=json.loads(handle.read(4096))
  except (OSError,ValueError):worker=None
  item=dict(fixture_durable=True,worker_status=worker,label=label,count=count,record_payload_bytes=size,seconds=time.monotonic()-start,exit=p.returncode,logical_read_bytes=io.get('rchar'),physical_read_bytes=io.get('read_bytes'),peak_rss_kib=peak,peak_rss_source=peak_source,reply=value)
  results.append(item);print(json.dumps(item),flush=True);return value
 try:
  if order=='legacy-first':
   call('legacy-cold',['search','--query','needle','--limit','20'])
   call('indexed-followup',['search','--query','needle','--limit','20','--indexed'])
  else:
   call('indexed-cold',['search','--query','needle','--limit','20','--indexed'])
   call('legacy-followup',['search','--query','needle','--limit','20'])
  time.sleep(2)
  call('indexed-after-2s',['search','--query','needle','--limit','20','--indexed'])
  call('legacy-warm',['search','--query','needle','--limit','20'])
 finally:
  # Helpers are bounded and scoped to this private root; never touch other processes.
  # Production worker must exit when its selected root disappears. Keep this directory
  # until workers have stopped or their bounded run lease expires.
  (out/f'bench-{binary.name}-{count}-{size}-{order}-sessions.json').write_text(json.dumps(results,indent=2))
