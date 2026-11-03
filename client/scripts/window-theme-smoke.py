#!/usr/bin/env python3
"""Capture only a synthetic fixture's first visible client pixels on Arch.

Build the window_theme_fixture example with tauri/custom-protocol, then run:
flock -w 1800 /tmp/zerus-arch-display.lock python3 client/scripts/window-theme-smoke.py \
    --fixture PRIVATE_TARGET/debug/examples/window_theme_fixture --output FRESH_DIRECTORY
The fixture uses the production theme/gate, a private bus and HOME, and the
canonical bundle ID. It never runs the Zerus entry point or any background role.
Stops on a locked screen; every created window exits with its retained child.
"""
import argparse
import json
import os
from pathlib import Path
import select
import subprocess
import time


def unlocked():
    result = subprocess.run(['dbus-send', '--session', '--print-reply',
                             '--dest=org.freedesktop.ScreenSaver', '/ScreenSaver',
                             'org.freedesktop.ScreenSaver.GetActive'],
                            capture_output=True, timeout=2, check=True)
    if b'boolean false' not in result.stdout or b'boolean true' in result.stdout:
        raise RuntimeError('Screen locked or lock state unavailable; stopping display run')


def stop(child):
    if child.poll() is None:
        child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()  # This exact harness-owned original only.
        child.wait(timeout=3)
        raise RuntimeError('Fixture did not close its window normally')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--webkit-prefix', type=Path)
    args = parser.parse_args()
    assert os.name == 'posix' and os.geteuid() != 0
    assert os.environ.get('DISPLAY'), 'A real Arch Xwayland display is required'
    fixture = args.fixture.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    unlocked()
    env = {k: os.environ[k] for k in ('PATH', 'LANG', 'DISPLAY', 'XAUTHORITY') if k in os.environ}
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME',
                'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'TMPDIR', 'HGS_CONFIG_DIR', 'HGS_STATE_DIR'):
        path = output / key.lower()
        path.mkdir(mode=0o700)
        env[key] = str(path)
    env.update(GDK_BACKEND='x11', GSETTINGS_BACKEND='memory', NO_AT_BRIDGE='1')
    if args.webkit_prefix:
        prefix = args.webkit_prefix.resolve(strict=True)
        assert not any(c in str(prefix) for c in ':\n\r')
        assert all((prefix / name).is_file() for name in (
            'gtk/lib/libgtk-3.so.0', 'lib/libwebkit2gtk-4.1.so.0',
            'lib/libjavascriptcoregtk-4.1.so.0',
            'libexec/webkit2gtk-4.1/WebKitWebProcess',
            'libexec/webkit2gtk-4.1/WebKitNetworkProcess',
            'libexec/webkit2gtk-4.1/WebKitGPUProcess'))
        env['LD_LIBRARY_PATH'] = f'{prefix}/gtk/lib:{prefix}/lib'
        env['WEBKIT_EXEC_PATH'] = str(prefix / 'libexec/webkit2gtk-4.1')
    # GTK application registration cannot address the owner's canonical app.
    bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'],
                           env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        assert select.select([bus.stdout], [], [], 3)[0], 'Private bus did not start'
        address = bus.stdout.readline(8193)
        assert 0 < len(address) < 8193 and address.endswith(b'\n')
        env['DBUS_SESSION_BUS_ADDRESS'] = address.decode().strip()
        for route, theme in ((route, theme) for route in ('lifecycle', 'window-layer')
                             for theme in ('dark', 'light', 'system-dark', 'system-light')):
            unlocked()
            case = route + '-' + theme
            target = output / case
            with (output / (case + '.log')).open('wb') as log:
                child = subprocess.Popen([str(fixture), route, theme, str(target)], env=env,
                                         stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 20
                    while child.poll() is None:
                        unlocked()
                        assert time.monotonic() < deadline, 'Fixture deadline exceeded'
                        time.sleep(.1)
                finally:
                    stop(child)
            assert child.returncode == 0, f'{case}: fixture failed, see {case}.log'
            result = json.loads((target / 'result.json').read_text())
            assert len(result['frames']) == 30
            assert result['frames'][0]['ms'] >= 600, 'Window mapped before delayed theme readiness'
            assert all(all(abs(actual - expected) <= 1 for actual, expected in zip(frame['rgb'], result['expected']))
                       for frame in result['frames']), f'{case}: wrong color in first visible frames'
            print(json.dumps({'route': route, 'theme': theme, 'frames': 30, 'first_visible_ms': result['frames'][0]['ms'],
                              'expected': result['expected'], 'first_rgb': result['frames'][0]['rgb']}), flush=True)
    finally:
        stop(bus)


if __name__ == '__main__':
    main()
