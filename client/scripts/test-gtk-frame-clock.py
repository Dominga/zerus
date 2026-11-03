#!/usr/bin/env python3
"""Check a private GTK3 build's paint freeze/thaw without using the real display.

python3 client/scripts/test-gtk-frame-clock.py --gtk-source GTK_SOURCE \
    --lib-dir REPAIRED_PREFIX/gtk/lib [--control-lib-dir ORIGINAL_PREFIX/gtk/lib]
The optional control must reproduce layout/after-paint starvation. Both runs
use the same C program, exact test name, and a retained private Xvfb process.
"""
import argparse
import json
import os
from pathlib import Path
import select
import shlex
import subprocess
import tempfile
import uuid


def stop(child):
    if child.poll() is None:
        child.terminate()
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=3)


def library(value):
    path = Path(value).resolve(strict=True)
    if any(c in str(path) for c in ':\n\r\0') or not (path / 'libgdk-3.so.0').is_file():
        raise argparse.ArgumentTypeError('Expected a GTK3 library directory without colons or newlines')
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gtk-source', type=Path, required=True)
    parser.add_argument('--lib-dir', type=library, required=True)
    parser.add_argument('--control-lib-dir', type=library)
    args = parser.parse_args()
    header = args.gtk_source.resolve(strict=True) / 'gdk/gdkframeclockprivate.h'
    assert header.is_file(), 'Use the uninstalled header from this GTK3 build'
    source = Path(__file__).with_name('gtk-frame-clock-test.c')
    with tempfile.TemporaryDirectory(prefix='zerus-frame-clock-') as temporary:
        root = Path(temporary)
        binary = root / 'frame-clock-test'
        flags = shlex.split(subprocess.check_output(
            ['pkg-config', '--cflags', '--libs', 'gdk-3.0'], text=True, timeout=5))
        subprocess.run(['cc', '-O2', '-Wall', '-Wextra', '-Wno-unused-parameter',
                        '-I' + str(header.parent), str(source), *flags, '-ldl', '-o', str(binary)],
                       check=True, timeout=30)
        env = {'PATH': os.environ['PATH'], 'LANG': 'C.UTF-8', 'HOME': str(root),
               'XDG_CONFIG_HOME': str(root), 'DBUS_SESSION_BUS_ADDRESS':
               'unix:path=/nonexistent/zerus-frame-clock', 'GDK_BACKEND': 'x11',
               'WAYLAND_DISPLAY': 'zerus-frame-clock-no-wayland',
               'ZERUS_GTK_FRAME_CLOCK_TEST': str(uuid.uuid4()),
               'ZERUS_GTK_PARENT_DISPLAY': os.environ.get('DISPLAY', '')}
        read_fd, write_fd = os.pipe()
        display = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0',
                                    '640x480x24', '-nolisten', 'tcp'], env=env,
                                   pass_fds=(write_fd,), stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        os.close(write_fd)
        try:
            assert select.select([read_fd], [], [], 5)[0], 'Private Xvfb did not start'
            number = os.read(read_fd, 32).decode().strip()
            assert number.isdigit(), 'Invalid private Xvfb display'
            env['DISPLAY'] = ':' + number
            assert env['DISPLAY'] != env['ZERUS_GTK_PARENT_DISPLAY']
            results = {}
            variants = [('repaired', args.lib_dir)]
            if args.control_lib_dir:
                variants.insert(0, ('control', args.control_lib_dir))
            for name, directory in variants:
                result = subprocess.run([str(binary), '--exact',
                                         'paint_freeze_resumes_after_paint'],
                                        env=dict(env, LD_LIBRARY_PATH=str(directory)),
                                        capture_output=True, text=True, timeout=6)
                report = json.loads(result.stdout)
                results[name] = report
                loaded = Path(report['gdk_library'])
                assert loaded.is_absolute() and loaded.parent.resolve() == directory, (
                    f'{name}: libgdk loaded outside selected directory: {loaded}')
                assert loaded.resolve(strict=True) == (directory / 'libgdk-3.so.0').resolve(strict=True), (
                    f'{name}: unexpected libgdk: {loaded}')
                assert not report['expired'], report
                assert report['paints'] == 32, report
                if name == 'control':
                    assert result.returncode == 1 and not report['passed'], report
                    assert report['layouts'] == report['after_paints'] == 1, report
                else:
                    assert result.returncode == 0 and report['passed'], report
                    assert report['layouts'] == report['after_paints'] == 32, report
            print(json.dumps(results, indent=2))
        finally:
            os.close(read_fd)
            stop(display)


if __name__ == '__main__':
    main()
