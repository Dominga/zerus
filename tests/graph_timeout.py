"""Failure-only diagnostics for the graph fixture's original subprocess.

This is observation, never a cleanup/drain proof. Linux reads only the pinned
original child and currently verifiable descendants. Exited/reparented children
and uncaptured output remain explicitly unavailable; no host-wide ps is used.
"""
import json
import os
from pathlib import Path
import queue
import select
import subprocess
import sys
import threading
import time

DIAGNOSTIC_SECONDS = 0.2
MAX_PROCESSES = 16
MAX_THREADS = 16
MAX_FDS = 32
MAX_READ = 4096
MAX_FRAME = 65536
TAIL_LINES = 20
TAIL_BYTES = 2048


def _tail(value):
    if isinstance(value, str):
        value = value[-TAIL_BYTES:].encode('utf-8', errors='replace')
    text = (value or b'')[-TAIL_BYTES:].decode('utf-8', errors='ignore')
    return '\n'.join(text.splitlines()[-TAIL_LINES:])


def _read(path):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        data = os.read(fd, MAX_READ + 1)
    finally:
        os.close(fd)
    if len(data) > MAX_READ:
        raise ValueError('native read limit')
    return data.decode('ascii')


def _stat(pid):
    # comm can contain spaces or parentheses; only numeric/state fields leave
    # the collector, never argv, environment, cwd or arbitrary descriptor paths.
    fields = _read(f'/proc/{pid}/stat').rsplit(') ', 1)[1].split()
    return {'pid': pid, 'parent': int(fields[1]), 'birth': int(fields[19]),
            'state': fields[0], 'cpu_ticks': int(fields[11]) + int(fields[12])}


def _live(pin):
    poll = select.poll()
    poll.register(pin, select.POLLIN)
    return not poll.poll(0)


def _native(root_pid, root_pin, deadline, outputs):
    pins = [root_pin]
    result = {'scope': 'original-child/current-descendants-only', 'processes': [],
              'unavailable': ['exited/reparented descendants are not reconstructed']}
    pending = [(root_pid, root_pin, None)]
    seen = {root_pid}
    try:
        result['clock_ticks_per_second'] = os.sysconf('SC_CLK_TCK')
        while pending and time.monotonic() < deadline:
            pid, pin, expected_parent = pending.pop(0)
            try:
                if not _live(pin):
                    raise ValueError('original process exited')
                before = _stat(pid)
                if expected_parent is not None and before['parent'] != expected_parent:
                    raise ValueError('parent changed')
                pipes = []
                streams = {'1': 'uncaptured-output-unavailable',
                           '2': 'uncaptured-output-unavailable'}
                with os.scandir(f'/proc/{pid}/fd') as entries:
                    for index, entry in enumerate(entries):
                        if index == MAX_FDS:
                            result['unavailable'].append(f'{pid}: descriptor limit')
                            break
                        try:
                            target = os.readlink(entry.path)
                        except OSError:
                            continue
                        if target.startswith('pipe:[') and target.endswith(']'):
                            pipes.append({'fd': int(entry.name), 'pipe': target})
                            if entry.name in ('1', '2'):
                                streams[entry.name] = next(
                                    (key for key, value in outputs.items() if value == target),
                                    'uncaptured-output-unavailable')
                children = set()
                with os.scandir(f'/proc/{pid}/task') as threads:
                    for index, thread in enumerate(threads):
                        if index == MAX_THREADS:
                            result['unavailable'].append(f'{pid}: thread limit')
                            break
                        children.update(int(p) for p in _read(Path(thread.path) / 'children').split())
                after = _stat(pid)
                if not _live(pin) or before['birth'] != after['birth'] or before['parent'] != after['parent']:
                    raise ValueError('process changed during observation')
                before.update(pipes=pipes, output_tails=streams)
                result['processes'].append(before)
                for child in sorted(children):
                    if child in seen:
                        continue
                    if len(seen) == MAX_PROCESSES:
                        result['unavailable'].append('process limit')
                        break
                    seen.add(child)
                    old = _stat(child)
                    child_pin = os.pidfd_open(child)
                    pins.append(child_pin)
                    current = _stat(child)
                    if (not _live(pin) or not _live(child_pin) or old['birth'] != current['birth']
                            or current['parent'] != pid or _stat(pid)['birth'] != before['birth']):
                        result['unavailable'].append(f'{child}: original ancestry unavailable')
                        continue
                    pending.append((child, child_pin, pid))
            except (OSError, ValueError, IndexError) as error:
                # Fixed type/errno diagnostics, never raw filesystem paths.
                result['unavailable'].append(f'{pid}: {type(error).__name__}/{getattr(error, "errno", None)}')
        if pending:
            result['unavailable'].append('diagnostic deadline')
        return result
    finally:
        for pin in pins:
            os.close(pin)


def snapshot(child, error):
    report = {'kind': 'graph-command-timeout', 'timeout': error.timeout,
              'tails': {'stdout': _tail(error.stdout), 'stderr': _tail(error.stderr)},
              'tail_note': 'shared captured pipes; no per-writer attribution'}
    if not sys.platform.startswith('linux') or not hasattr(os, 'pidfd_open'):
        report['native'] = {'unavailable': 'native own-custody observer unavailable'}
        return report
    if child.returncode is not None:
        report['native'] = {'unavailable': 'original child already reaped; no PID adoption'}
        return report
    try:
        # Popen exclusively owns this unreaped original child here. No poll or
        # wait runs until after capture; transfer the pin to the read-only worker.
        pin = os.pidfd_open(child.pid)
    except OSError:
        report['native'] = {'unavailable': 'original child pin unavailable'}
        return report
    outputs = {}
    for name in ('stdout', 'stderr'):
        try:
            outputs[name] = os.readlink(f'/proc/self/fd/{getattr(child, name).fileno()}')
        except (OSError, ValueError):
            pass
    completed = queue.Queue(maxsize=1)
    deadline = time.monotonic() + DIAGNOSTIC_SECONDS

    def collect():
        try:
            completed.put(_native(child.pid, pin, deadline, outputs))
        except Exception:
            completed.put({'unavailable': 'native observer failed'})

    worker = threading.Thread(target=collect, daemon=True)
    try:
        worker.start()
    except Exception:
        os.close(pin)
        raise
    worker.join(max(0, deadline - time.monotonic()))
    try:
        report['native'] = completed.get_nowait()
    except queue.Empty:
        report['native'] = {'unavailable': 'native diagnostic deadline'}
    return report


def run(args, *, env, timeout):
    """Keep subprocess.run's result/timeout/cleanup; observe before its kill."""
    with subprocess.Popen(args, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            try:
                report = snapshot(child, error)
                frame = json.dumps(report, ensure_ascii=True)
                if len(frame) > MAX_FRAME:
                    report['native'] = {'unavailable': 'diagnostic frame limit'}
                    frame = json.dumps(report, ensure_ascii=True)
                print('GRAPH_TIMEOUT ' + frame, file=sys.stderr, flush=True)
            except Exception:
                try:
                    print('GRAPH_TIMEOUT {"unavailable":"diagnostic failed"}', file=sys.stderr, flush=True)
                except Exception:
                    pass  # An unavailable CI log must not mask the original timeout.
            child.kill()
            if os.name == 'nt':
                error.stdout, error.stderr = child.communicate()
            else:
                child.wait()
            raise
        except BaseException:
            child.kill()
            child.wait()
            raise
        return subprocess.CompletedProcess(args, child.returncode, stdout, stderr)
