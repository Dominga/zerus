#!/usr/bin/env python3
"""Opt-in Linux search regression bench, with disposable synthetic state only.

Build hgs in release mode first. Full mode writes about 18 GB; choose an
explicit disk-backed scratch parent. No native agent or tmux server is started.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid


MARKER = '.zerus-scale-fixture'


def generate(root, current, archives):
    state = root / 'state'
    (state / 'archives').mkdir(parents=True, mode=0o700)
    (root / MARKER).write_text('Disposable synthetic search fixture\n')
    transcript = root / 'transcript.jsonl'
    transcript.write_text(json.dumps(dict(type='event_msg', timestamp='2026-11-14T10:00:00Z',
        payload=dict(type='agent_message', message='Synthetic scale benchmark result'))) + '\n')
    jobs = {}
    for n in range(24):
        command = f'printf synthetic-result-{n}'
        jobs[str(n)] = dict(call_id=str(n), command=command,
            command_digest=hashlib.sha256(command.encode()).hexdigest(), command_truncated=False,
            status='unknown' if n < 8 else 'completed', updated_at=1794650400+n,
            output=('synthetic bounded command output\n' * 1000)[:28*1024 if n < 8 else 1024])
    total = 0
    for n in range(current + archives):
        name = f'codex/project-{n%100:03}/session-{n:05}'
        record = dict(version=1, name=name, agent='codex', created=1794650400,
            updated=1794650400+n, last_event_at=1794650400+n, phase='interrupted', activity='idle',
            pid=None, process_start=None, cwd='/srv/example/project', launch_dir='/srv/example/project',
            run_id=str(uuid.UUID(int=n+1)), conversation_id=str(uuid.UUID(int=100000+n)),
            shell_jobs=jobs, active_tools={}, subagents={}, base=['codex'], transcript=str(transcript))
        if n < current:
            path = state / (hashlib.sha256(name.encode()).hexdigest() + '.json')
        else:
            ident = str(uuid.UUID(int=200000+n))
            record.update(state='archived', archive_id=ident, archived_at=1794650400+n)
            path = state / 'archives' / (ident + '.json')
        data = json.dumps(record, separators=(',', ':')).encode()
        path.write_bytes(data)
        total += len(data)
    metadata = dict(current=current, archives=archives, total_bytes=total, jobs_per_record=24,
                    fixture_limit='Saved/interrupted records; no real agents; shared synthetic transcript.')
    (root / 'fixture.json').write_text(json.dumps(metadata))
    return metadata


def environment(root):
    env = {key:value for key,value in os.environ.items()
           if not key.startswith('HGS_') and key not in ('TMUX', 'TMUX_PANE',
               'CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'KIMI_CODE_HOME', 'DEEPSEEK_HOME')}
    for directory in ('config', 'tmp', 'tmux'):
        (root / directory).mkdir(mode=0o700, exist_ok=True)
    env.update(HOME=str(root), USERPROFILE=str(root), HGS_STATE_DIR=str(root / 'state'),
        HGS_CONFIG_DIR=str(root / 'config'), HGS_SELF='scale-fixture', HGS_PEERS='',
        TMPDIR=str(root / 'tmp'), TMUX_TMPDIR=str(root / 'tmux'), LC_ALL='C', TZ='UTC')
    return env


def measure(hgs, root, indexed, timeout, rss_limit_mib):
    args = [str(hgs), 'search', '--query', 'synthetic', '--limit', '20']
    if indexed:
        args.append('--indexed')
    started = time.monotonic()
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        process = subprocess.Popen(args, env=environment(root), stdin=subprocess.DEVNULL,
                                   stdout=out, stderr=err, start_new_session=True)
        guard = None
        try:
            while not os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
                try:
                    status = Path(f'/proc/{process.pid}/status').read_text()
                    rss = next((int(line.split()[1]) for line in status.splitlines() if line.startswith('VmRSS:')), 0)
                except FileNotFoundError:
                    rss = 0
                if time.monotonic() - started >= timeout or rss > rss_limit_mib * 1024:
                    guard = 'timeout' if rss <= rss_limit_mib * 1024 else 'memory guard'
                    os.killpg(process.pid, signal.SIGKILL)
                    os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOWAIT)
                    break
                time.sleep(.01)
            io = {key:int(value) for key,value in (line.split(':') for line in
                  Path(f'/proc/{process.pid}/io').read_text().splitlines())}
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        finally:
            if process.returncode is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        out.seek(0); err.seek(0)
        reply = json.load(out) if process.returncode == 0 else None
        result = dict(indexed=indexed, seconds=time.monotonic()-started, exit=process.returncode,
            guard=guard, logical_read_bytes=io['rchar'], physical_read_bytes=io['read_bytes'],
            peak_rss_kib=usage.ru_maxrss, filesystem_input_blocks=usage.ru_inblock)
        if reply is not None:
            fallback = reply.get('fallback') if indexed else reply
            result.update(results=len(reply.get('results', [])), truncated=reply.get('truncated'),
                          fallback_used=fallback is not None,
                          fallback_results=len((fallback or {}).get('results', [])),
                          fallback_discovery=(fallback or {}).get('discovery'))
        else:
            result['error'] = err.read(4096).decode(errors='replace')
        return result


def run(args, root, metadata):
    results = [measure(args.hgs, root, indexed, args.timeout, args.rss_limit_mib) for indexed in (False, True)]
    print(json.dumps(dict(fixture=metadata, measurements=results), indent=2), flush=True)
    if args.check and any(result['exit'] != 0 or result['logical_read_bytes'] > 16 << 20
                          or result['peak_rss_kib'] > 96 << 10
                          or (result.get('fallback_used') and not result.get('fallback_discovery'))
                          for result in results):
        raise SystemExit('bounded search regression: expected <=16 MiB reads, <=96 MiB RSS and discovery coverage')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hgs', type=lambda value: Path(value).resolve(), default=Path('target/release/hgs').resolve())
    parser.add_argument('--scratch-parent', type=Path)
    parser.add_argument('--existing-fixture', type=Path, help='explicitly marked private audit fixture')
    parser.add_argument('--full', action='store_true', help='20,000 current records and 50,000 archives')
    parser.add_argument('--check', action='store_true', help='fail on an unbounded read/RSS regression')
    parser.add_argument('--timeout', type=float, default=300)
    parser.add_argument('--rss-limit-mib', type=int, default=1024, help='private-child memory guard')
    args = parser.parse_args()
    if not sys.platform.startswith('linux'):
        parser.error('final child /proc I/O accounting requires Linux')
    if args.existing_fixture:
        root = args.existing_fixture.resolve(strict=True)
        if not (root / MARKER).is_file():
            parser.error('existing state must carry the private synthetic fixture marker')
        run(args, root, json.loads((root / 'fixture.json').read_text()))
        return
    if args.full and args.scratch_parent is None:
        parser.error('--full needs an explicit disk-backed --scratch-parent')
    counts = (20000, 50000) if args.full else (2000, 5000)
    parent = args.scratch_parent or Path(tempfile.gettempdir())
    if shutil.disk_usage(parent).free < sum(counts) * 270000:
        parser.error('insufficient free space for the private fixture')
    with tempfile.TemporaryDirectory(prefix='zerus-search-scale-', dir=parent) as temporary:
        root = Path(temporary)
        run(args, root, generate(root, *counts))


if __name__ == '__main__':
    main()
