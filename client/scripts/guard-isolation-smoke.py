#!/usr/bin/env python3
"""The built app's single-instance rules against a fake canonical owner (zerus-5a99f).

Build a switch-identity app first:
TAURI_CONFIG='{"identifier":"com.hgdev.hgs-tray"}' cargo build --locked --manifest-path client/src-tauri/Cargo.toml
Then: python3 client/scripts/guard-isolation-smoke.py --app TARGET/debug/zerus-client --work NEW_DIRECTORY

Everything runs inside gui_sandbox (private /tmp, /run/user, network and IPC):
the fake owner there holds the canonical lock and listens at its socket. A
measurement start (ZERUS_CLIENT_PERF) must exit 3 and send it nothing; an
ordinary start must still forward, ignoring ZERUS_CLIENT_GUARD_ROOT; a
measurement start with ZERUS_CLIENT_GUARD_ROOT owns that private root instead.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gui_sandbox  # noqa: E402  (shared GUI isolation, zerus-5a99f)


def private_env(work):
    env = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'GDK_BACKEND': 'x11', 'WAYLAND_DISPLAY': 'zerus-none',
           'ZERUS_CLIENT_HEADLESS': '1', 'WEBKIT_DISABLE_DMABUF_RENDERER': '1', 'NO_AT_BRIDGE': '1',
           'GSETTINGS_BACKEND': 'memory', 'ZERUS_CLIENT_HGS': '/bin/false'}
    for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('TMPDIR', 'tmp'), ('HGS_CONFIG_DIR', 'hgs-config'), ('HGS_STATE_DIR', 'hgs-state')]:
        path = work / name
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
        env[key] = str(path)
    return env


def inner(app, work):
    gui_sandbox.refuse_reachable_guard()
    guard = gui_sandbox.canonical_guard()
    guard.mkdir(mode=0o700)
    lock = os.open(guard / 'gui.lock', os.O_CREAT | os.O_RDWR, 0o600)
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    server = socket.socket(socket.AF_UNIX)
    server.bind(str(guard / 'gui.sock'))
    server.listen(8)
    received = []

    def serve():
        while True:
            connection, _ = server.accept()
            with connection:
                connection.settimeout(2)
                data = b''
                try:
                    while not data.endswith(b'\n'):
                        chunk = connection.recv(256)
                        if not chunk:
                            break
                        data += chunk
                except OSError:
                    pass
                received.append(data.decode(errors='replace'))
    threading.Thread(target=serve, daemon=True).start()

    def start(name, **extra):
        env = dict(private_env(work / name), **extra)
        result = subprocess.run([str(app), '--sessions'], env=env, capture_output=True, text=True, timeout=60)
        time.sleep(0.5)
        print(f'{name}: exit {result.returncode}; owner received {received}', flush=True)
        print('  stderr tail: ' + ' | '.join(result.stderr.strip().splitlines()[-3:]), flush=True)
        return result

    results = {}
    measured = start('measured', ZERUS_CLIENT_PERF='auto')
    assert measured.returncode == 3, measured.stderr[-2000:]
    assert 'never forwards' in measured.stderr and 'is held' in measured.stderr, measured.stderr[-2000:]
    assert received == [], received
    results['measured'] = 'refused, nothing sent'

    root = work / 'private-guard'
    root.mkdir(mode=0o700)
    ordinary = start('ordinary-with-root', ZERUS_CLIENT_GUARD_ROOT=str(root))
    assert ordinary.returncode == 0, ordinary.stderr[-2000:]
    assert received == ['sessions\n'], received
    assert not (root / 'gui.lock').exists(), 'an ordinary start used the private root'
    results['ordinary'] = 'forwarded to the canonical owner; private root ignored'

    private = start('measured-with-root', ZERUS_CLIENT_PERF='auto', ZERUS_CLIENT_GUARD_ROOT=str(root))
    assert 'never forwards' not in private.stderr, private.stderr[-2000:]
    assert (root / 'gui.lock').exists(), 'the measurement start did not use its private root'
    assert received == ['sessions\n'], received
    results['measured-with-root'] = f'owned the private root (exit {private.returncode}), nothing sent'

    plain = start('ordinary')
    assert plain.returncode == 0 and received == ['sessions\n', 'sessions\n'], (plain.stderr[-2000:], received)
    results['ordinary-again'] = 'forwarded'
    (work / 'result.json').write_text(json.dumps(results, indent=1))
    print('guard-isolation-smoke: PASS', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--inner', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inner:
        inner(args.app, args.work)
        return
    app = args.app.resolve(strict=True)
    args.work.mkdir(mode=0o700, parents=True, exist_ok=False)
    work = args.work.resolve()
    command = gui_sandbox.sandboxed([sys.executable, str(Path(__file__).resolve()), '--inner',
                                     '--app', str(app), '--work', str(work)], keep=[work])
    raise SystemExit(subprocess.run(command, timeout=300).returncode)


if __name__ == '__main__':
    main()
