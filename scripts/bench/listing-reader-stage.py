#!/usr/bin/env python3
"""One Linux ls measurement stage in a caller-owned synthetic fixture only."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--hgs',type=Path,required=True)
    parser.add_argument('--clock',type=Path,required=True)
    parser.add_argument('--count',type=int,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();root=args.root.resolve();directory=root/'listing-metadata'
    assert (root/'.listing-metadata-scale-fixture').is_file()
    assert 1<=args.count<=100000
    env={k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX','TMUX_PANE','CLAUDE_CONFIG_DIR','CODEX_HOME','KIMI_CODE_HOME','DEEPSEEK_HOME')}
    home=root/'benchmark-home';home.mkdir(exist_ok=True,mode=0o700)
    for name in ['config','tmp','tmux','xdg-config','xdg-state','xdg-data']:(home/name).mkdir(exist_ok=True,mode=0o700)
    (home/'config/projects.local').write_text(''.join(f'project-{n:03}=/srv/example/project-{n:03}\n' for n in range(100)))
    env.update(HOME=str(home),HGS_STATE_DIR=str(root),HGS_CONFIG_DIR=str(home/'config'),HGS_SELF='scale-fixture',HGS_PEERS='',TMPDIR=str(home/'tmp'),TMUX_TMPDIR=str(home/'tmux'),XDG_CONFIG_HOME=str(home/'xdg-config'),XDG_STATE_HOME=str(home/'xdg-state'),XDG_DATA_HOME=str(home/'xdg-data'))
    def call(*words):
        return subprocess.run([str(args.hgs),*words],env=env,capture_output=True,text=True,timeout=15)
    def disabled():
        result=call('listing-index','disable','--json');assert result.returncode==0,result.stderr
        until=time.monotonic()+8
        while time.monotonic()<until:
            try:
                with (directory/'worker.lock').open('rb') as handle:fcntl.flock(handle,fcntl.LOCK_EX|fcntl.LOCK_NB)
                return
            except FileNotFoundError:return
            except BlockingIOError:time.sleep(.02)
        raise AssertionError('private worker did not release its lease')
    def measure(label,*extra):
        with tempfile.TemporaryDirectory(prefix='clock-',dir=home/'tmp') as temporary:
            metrics=Path(temporary)/'metrics';out=Path(temporary)/'stdout';err=Path(temporary)/'stderr'
            started=time.monotonic()
            with out.open('wb') as stdout,err.open('wb') as stderr:
                process=subprocess.run([str(args.clock),str(metrics),str(args.hgs),'ls','--json','--local','--no-archives',*extra],env=env,stdout=stdout,stderr=stderr,timeout=15)
            seconds=time.monotonic()-started
            rss,rchar,disk,sampled=map(int,metrics.read_text().split())
            result=dict(label=label,seconds=seconds,stdout_bytes=out.stat().st_size,foreground_rchar=rchar,foreground_read_bytes=disk,peak_rss_bytes=rss*1024,rss_sampled=bool(sampled),exit=process.returncode)
            if process.returncode==0:
                body=json.loads(out.read_bytes());rows=body['sessions'];page=body.get('listing_page')
                assert len({row['name'] for row in rows})==len(rows)
                if page is None:assert len(rows)==args.count
                else:assert len(rows)<=128 and page['coverage']=='ready'
                result.update(rows=len(rows),complete=not page or page['complete'],next_cursor=page.get('next_cursor') if page else None)
            else:
                assert out.stat().st_size==0,'partial body published as failure'
                result.update(complete=False,refusal=err.read_text()[:1024])
            return result
    def stamps():
        paths=[p for p in root.glob('*.json') if len(p.stem)==64 and all(char in '0123456789abcdef' for char in p.stem)]
        return {p.name:(p.stat().st_dev,p.stat().st_ino,p.stat().st_size,p.stat().st_mtime_ns,p.stat().st_ctime_ns) for p in paths}
    before=stamps();assert len(before)==args.count
    sample=sorted(before)[::max(1,args.count//16)][:16]
    digests={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sample}
    results=[];worker=None
    def evict(paths):
        for path in paths:
            descriptor=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
            try:os.posix_fadvise(descriptor,0,0,os.POSIX_FADV_DONTNEED)
            finally:os.close(descriptor)
    def worker_sample():
        io={}
        for line in Path(f'/proc/{worker.pid}/task/{worker.pid}/io').read_text().splitlines():
            key,value=line.split(':',1);io[key]=int(value)
        rss=next(int(line.split()[1])*1024 for line in Path(f'/proc/{worker.pid}/status').read_text().splitlines() if line.startswith('VmHWM:'))
        return dict(logical_read_bytes=io['rchar'],physical_read_bytes=io['read_bytes'],peak_rss_bytes=rss)
    try:
        disabled()
        # Index cold is explicit and private. No global OS cache flush.
        for name in ['index.sqlite3','index.sqlite3-wal','index.sqlite3-shm']:(directory/name).unlink(missing_ok=True)
        evict(root/name for name in before)
        results.append(measure('disabled-canonical-files-evicted'))
        results.append(measure('disabled-warm'))
        # Own the real worker as a subprocess so its resource counters remain
        # separate from the foreground. CLI startup/coalescing has runtime controls.
        evict(root/name for name in before)
        (directory/'disabled').unlink(missing_ok=True)
        bootstrap_started=time.monotonic()
        worker=subprocess.Popen([str(args.hgs),'__state','listing-metadata-worker'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        until=time.monotonic()+8
        while time.monotonic()<until:
            assert worker.poll() is None,'private worker exited before acquiring its lease'
            try:
                with (directory/'worker.lock').open('rb') as lease:fcntl.flock(lease,fcntl.LOCK_EX|fcntl.LOCK_NB)
            except FileNotFoundError:pass
            except BlockingIOError:break
            time.sleep(.001)
        else:raise AssertionError('private worker never acquired its lease')
        results.append(measure('enabled-index-cold-canonical-files-evicted'))
        started=time.monotonic();deadline=started+600
        while time.monotonic()<deadline:
            status=call('listing-index','status','--json')
            if status.returncode==0 and json.loads(status.stdout)['state']=='ready':break
            time.sleep(.1)
        else:raise AssertionError('bounded private worker never finished the cold census')
        readiness=time.monotonic()-bootstrap_started
        bootstrap=worker_sample()
        evict(directory/name for name in ['index.sqlite3','index.sqlite3-wal'] if (directory/name).exists())
        results.append(measure('enabled-index-files-evicted'))
        results.append(measure('enabled-warm'))
        results.append(measure('enabled-paged-warm','--paged','--page-size','128'))
        # Observe foreground/native resources; file bodies remain unchanged.
        assert stamps()==before
        assert {p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sample}==digests
        sizes={name:(directory/name).stat().st_size for name in ['index.sqlite3','index.sqlite3-wal','index.sqlite3-shm'] if (directory/name).exists()}
        args.output.write_text(json.dumps(dict(sessions=args.count,cold='canonical data files fadvise DONTNEED after fsync; derived index absent; namespace metadata warm',worker_bootstrap=bootstrap,readiness_seconds=readiness,index_files_bytes=sizes,canonical_native_versions_unchanged=len(before),canonical_sha256_samples=len(sample),results=results),indent=2)+'\n')
    finally:
        disabled()
        if worker is not None:worker.wait(timeout=5)
        # The benchmark owner continues its private direct-write measurements.
        (directory/'disabled').unlink(missing_ok=True)


if __name__=='__main__':main()
