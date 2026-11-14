"""Private native consumer/caller topology for the read-only mode check."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def identity(pid):
    if sys.platform == 'darwin':
        # The public BSD layout; this reads only fixture-owned processes.
        class Bsd(ctypes.Structure):
            _fields_ = ([(name, ctypes.c_uint32) for name in
                         ('flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid',
                          'ruid', 'rgid', 'svuid', 'svgid', 'reserved')]
                        + [('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32)]
                        + [(name, ctypes.c_uint32) for name in
                           ('nfiles', 'pgid', 'pjobc', 'tdev', 'tpgid')]
                        + [('nice', ctypes.c_int32), ('start_sec', ctypes.c_uint64),
                           ('start_usec', ctypes.c_uint64)])
        lib = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
        value = Bsd()
        assert ctypes.sizeof(value) == 136
        used = lib.proc_pidinfo(pid, 3, 0, ctypes.byref(value), ctypes.sizeof(value))
        assert used == ctypes.sizeof(value) and value.pid == pid and value.uid == os.geteuid()
        path = ctypes.create_string_buffer(4096)
        assert lib.proc_pidpath(pid, path, len(path)) > 0
        return dict(pid=pid, parent=value.ppid, group=value.pgid, session=os.getsid(pid),
                    foreground=value.tpgid, birth=f'{value.start_sec}:{value.start_usec}',
                    image=path.value.decode())
    proc = Path('/proc') / str(pid)
    text = (proc / 'stat').read_text()
    fields = text[text.rfind(')') + 2:].split()
    return dict(pid=pid, parent=int(fields[1]), group=int(fields[2]), session=int(fields[3]),
                foreground=int(fields[5]), birth=fields[19], image=os.readlink(proc / 'exe'))


def wait_json(path, seconds=15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        try:
            return json.loads(path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            time.sleep(.01)
    raise TimeoutError(path.name)


def environment_withheld(pid):
    """Native diagnostic only; it grants no authority to the caller."""
    lib = ctypes.CDLL(None, use_errno=True)
    mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
    data = ctypes.create_string_buffer(65536)
    size = ctypes.c_size_t(len(data))
    assert lib.sysctl(mib, 3, data, ctypes.byref(size), None, 0) == 0
    raw = data.raw[:size.value]
    count = int.from_bytes(raw[:4], sys.byteorder, signed=True)
    assert 1 <= count <= 4096
    offset = raw.index(b'\0', 4) + 1
    while raw[offset:offset + 1] == b'\0':
        offset += 1
    for _ in range(count):
        offset = raw.index(b'\0', offset) + 1
    return not any(raw[offset:])


def caller(hgs, folder):
    try:
        assert not any(key in os.environ for key in ('HGS_SESSION', 'HGS_SESSION_ID', 'HGS_RUN_ID', 'HGS_AGENT'))
        tty = os.open('/dev/tty', os.O_RDWR | os.O_NOCTTY)
        info = identity(os.getpid())
        assert info['group'] == os.tcgetpgrp(tty) == os.getpgrp()
        os.close(tty)
        (folder / 'caller.json').write_text(json.dumps(info))
        until = time.monotonic() + 15
        while not (folder / 'go').exists() and time.monotonic() < until:
            time.sleep(.01)
        assert (folder / 'go').exists()
        result = subprocess.run([hgs, 'desktop', 'registration', 'mode-check', '--json'],
                                capture_output=True, text=True, timeout=10)
        (folder / 'result.json').write_text(json.dumps(dict(
            code=result.returncode, out=result.stdout, error=result.stderr)))
    finally:
        (folder / 'finished').touch()


def run(hgs, folder, image, env, restricted=False):
    import pty
    folder.mkdir()
    master, slave = pty.openpty()
    process = None
    try:
        process = subprocess.Popen([str(image), os.ttyname(slave), sys.executable,
                                    str(Path(__file__).resolve()), str(hgs), str(folder),
                                    'restricted' if restricted else 'hold'],
                                   env=env, start_new_session=True, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child = wait_json(folder / 'caller.json')
        before = identity(process.pid)
        if restricted:
            until = time.monotonic() + 5
            while Path(before['image']).name != 'zsh' and time.monotonic() < until:
                time.sleep(.01)
                before = identity(process.pid)
            assert Path(before['image']).name == 'zsh', before
            while (not (folder / 'inherited').exists() or not (folder / 'inherited').stat().st_size) and time.monotonic() < until:
                time.sleep(.01)
            assert (folder / 'inherited').read_text().strip() == 'inherited'
            assert environment_withheld(process.pid), 'restricted-shell control needs a withheld tail'
        assert before['pid'] == before['group'] == before['foreground'] == child['group']
        assert child['parent'] != before['pid']
        (folder / 'go').touch()
        result = wait_json(folder / 'result.json')
        after = identity(process.pid)
        assert before == after, (before, after)
        result.update(leader=before, caller=child)
        return result
    finally:
        (folder / 'go').touch()
        until = time.monotonic() + 12
        while process is not None and not (folder / 'finished').exists() and time.monotonic() < until:
            time.sleep(.01)
        (folder / 'done').touch()
        if process is not None:
            try:
                stdout, stderr = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                # Only this original, unreaped fixture child can be signalled.
                process.kill()
                stdout, stderr = process.communicate(timeout=5)
            assert process.returncode == 0, (process.returncode, stdout, stderr)
        os.close(master)
        os.close(slave)


if __name__ == '__main__':
    assert sys.argv[1] == 'caller'
    caller(sys.argv[2], Path(sys.argv[3]))
