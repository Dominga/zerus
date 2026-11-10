#!/usr/bin/env python3
"""Check GTK layout and display-rate pacing in an isolated native Wayland window.

Build src-tauri's resize_fixture example, then run on an unlocked KDE desktop:
flock -w 1800 /tmp/zerus-arch-display.lock python3 client/scripts/window-resize-smoke.py \
    --fixture PRIVATE_TARGET/debug/examples/resize_fixture --output FRESH_DIRECTORY \
    --webkit-prefix PRIVATE_PREFIX --min-fps 130 --timer-burst
Use --min-fps 0 to check the system-GTK fallback's allocation at 60 Hz.
No installed GUI entry, tray or service runs; every KWin script matches only
our retained child's exact title and captured window UUID. The fixture keeps
the canonical bundle ID; its GTK application name is a test harness detail.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import select
import statistics
import subprocess
import threading
import time
from unittest.mock import patch
import uuid

ROOT = Path(__file__).resolve().parents[2]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


safety = module('resize_display_safety', Path(__file__).with_name('window-theme-smoke.py'))
launcher = module('resize_linux_launcher', ROOT / 'packaging/desktop/linux-launcher.py')


def resize(child, command, output, burst):
    plugin = 'zerus-resize-test-' + uuid.uuid4().hex
    config = dict(command, pid=child.pid, caption='Zerus isolated resize fixture',
                  timerBurst=120 if burst and command['step'] in (0, 4) else 0)
    script = output / (plugin + '.js')
    script.write_text('const c=' + json.dumps(config) + ''';
const matches=workspace.windowList().filter(w=>w.pid===c.pid && w.normalWindow &&
 !w.deleted && !w.dialog && w.caption===c.caption);
if(matches.length===1){const w=matches[0],id=String(w.internalId);
 function size(width,height){const g=w.frameGeometry,b=w.clientGeometry;
  w.frameGeometry={x:g.x,y:g.y,width:width+g.width-b.width,height:height+g.height-b.height};}
 if(c.timerBurst){let step=0;const timer=new QTimer();timer.interval=8;
  timer.timeout.connect(function(){
   if(w.deleted || w.pid!==c.pid || String(w.internalId)!==id || step>=c.timerBurst){timer.stop();return;}
   const r=step++/Math.max(1,c.timerBurst-1),from=c.step===0?950:1700,fromHeight=c.step===0?600:1000;
   size(Math.round(from+(c.width-from)*r),Math.round(fromHeight+(c.height-fromHeight)*r));
  });timer.start();
 }else if(typeof c.maximize==='boolean')w.setMaximize(c.maximize,c.maximize);
 else size(c.width,c.height);
}
''')

    def call(*args):
        return subprocess.check_output(['qdbus6', 'org.kde.KWin', *args],
                                       timeout=3, text=True).strip()
    loaded = False
    try:
        number = call('/Scripting', 'org.kde.kwin.Scripting.loadScript', str(script), plugin)
        assert int(number) >= 0, 'KWin refused the private script'
        loaded = True
        call('/Scripting/Script' + number, 'org.kde.kwin.Script.run')
        if config['timerBurst']:
            until = time.monotonic() + 1.5
            while time.monotonic() < until:
                safety.unlocked()
                assert child.poll() is None, 'Fixture exited during resize'
                time.sleep(.02)
    finally:
        if loaded:
            call('/Scripting', 'org.kde.kwin.Scripting.unloadScript', plugin)
        script.unlink()


def validate(events, minimum):
    requests = [i for i, e in enumerate(events) if e['phase'] == 'request']
    assert len(requests) == 6, 'Not all resize/maximize requests ran'
    transitions, failures = [], []
    for n, start in enumerate(requests):
        end = requests[n + 1] if n + 1 < len(requests) else len(events)
        rows = events[start + 1:end]
        configured = [r for r in rows if r['phase'] == 'configure']
        allocated = [r for r in rows if r['phase'] == 'view_allocate']
        ticks = [r for r in rows if r['phase'] == 'tick']
        target = configured[-1] if configured else None
        size = lambda r: [r['width'], r['height']] if r else None
        good = target and allocated and ticks and size(target) == size(allocated[-1]) == size(ticks[-1])
        transitions.append({'step': n, 'configured': size(target),
                            'allocated': size(allocated[-1]) if allocated else None,
                            'live_page': size(ticks[-1]) if ticks else None})
        if not good:
            failures.append(f'Step {n}: native allocation or live page retained the old size')
    # Exclude startup and retain resize periods: timers, layout and after-paint
    # must keep progressing while the page continuously scrolls and animates.
    warm = events[requests[0]:]
    rates = lambda phase: [r['width'] for r in warm if r['phase'] == phase]
    raf, painted, after = rates('raf_rate'), rates('paint_rate'), rates('after_paint_rate')
    if not raf or statistics.median(raf) < minimum:
        failures.append('Page frame rate is below the requested display rate')
    if not painted or statistics.median(painted) < minimum:
        failures.append('GTK paint rate is below the requested display rate')
    if not after or statistics.median(after) <= 0:
        failures.append('GTK after-paint stopped progressing')
    return dict(transitions=transitions, raf_median=statistics.median(raf) if raf else 0,
                paint_median=statistics.median(painted) if painted else 0,
                after_paint_median=statistics.median(after) if after else 0, failures=failures)


def loaded_libraries(child, prefix):
    """Verify the retained child's mappings, including resolved prefix symlinks."""
    maps = Path('/proc', str(child.pid), 'maps').read_text()
    libraries = {}
    for name in ('libgtk-3.so.0', 'libgdk-3.so.0', 'libwebkit2gtk-4.1.so.0',
                 'libjavascriptcoregtk-4.1.so.0'):
        paths = {line.split(maxsplit=5)[-1] for line in maps.splitlines()
                 if name in line and len(line.split(maxsplit=5)) == 6}
        if len(paths) != 1:
            return None  # Wait until GTK and WebKit have both loaded.
        loaded = Path(paths.pop())
        directory = (prefix / 'gtk/lib' if (prefix / 'gtk').exists() else Path('/usr/lib')) \
            if name.startswith(('libgtk-', 'libgdk-')) else prefix / 'lib'
        assert loaded.resolve(strict=True) == (directory / name).resolve(strict=True), (
            f'Unexpected {name} mapping: {loaded}')
        libraries[name] = str(loaded)
    return libraries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--webkit-prefix', type=Path, required=True)
    parser.add_argument('--min-fps', type=float, default=130)
    parser.add_argument('--timer-burst', action='store_true')
    args = parser.parse_args()
    assert os.name == 'posix' and 0 <= args.min_fps <= 500
    safety.unlocked()
    fixture = args.fixture.resolve(strict=True)
    prefix = args.webkit_prefix.resolve(strict=True)
    assert launcher.available(prefix), 'Incomplete private GTK/WebKit stack'
    socket = Path(os.environ['WAYLAND_DISPLAY'])
    if not socket.is_absolute():
        socket = Path(os.environ['XDG_RUNTIME_DIR']) / socket
    assert socket.is_socket(), 'No native Wayland socket'
    output = args.output.absolute()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    env = {key: os.environ[key] for key in ('PATH', 'LANG', 'DISPLAY', 'XAUTHORITY') if key in os.environ}
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME', 'XDG_CACHE_HOME',
                'XDG_RUNTIME_DIR', 'TMPDIR', 'HGS_CONFIG_DIR', 'HGS_STATE_DIR'):
        directory = output / key.lower()
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    env.update(WAYLAND_DISPLAY=str(socket), GDK_BACKEND='wayland', GSETTINGS_BACKEND='memory',
               NO_AT_BRIDGE='1', ZERUS_RESIZE_EXTERNAL='1')
    install = output / 'install'
    setting = install / launcher.SETTING
    setting.parent.mkdir(parents=True)
    setting.write_text(json.dumps({'version': 1, 'prefix': str(prefix)}))
    # The fixture's --build-info is an inert pre-GTK answer. Use the exact
    # maintained launcher environment; restore the parent desktop environment
    # before screen checks, private-bus creation and reader threads.
    with patch.dict(os.environ, env, clear=True):
        env = launcher.environment(install, fixture)
    assert env.get('WEBKIT_EXEC_PATH') == str(prefix / 'libexec/webkit2gtk-4.1')
    bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'],
                           env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        assert select.select([bus.stdout], [], [], 3)[0], 'Private bus did not start'
        env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline(8193).decode().strip()
        with (output / 'fixture.log').open('wb') as log:
            child = subprocess.Popen([str(fixture), str(output / 'trace')], env=env,
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT)
            def diagnostics():
                retained = 0
                finished = None
                os.set_blocking(child.stdout.fileno(), False)
                while True:
                    if child.poll() is not None:
                        if finished is None:
                            finished = time.monotonic()
                        if time.monotonic() - finished > .5:
                            return
                    if not select.select([child.stdout], [], [], .1)[0]:
                        continue
                    chunk = os.read(child.stdout.fileno(), 4096)
                    if not chunk:
                        return
                    remaining = max(0, 512 * 1024 - retained)
                    log.write(chunk[:remaining])
                    log.flush()
                    retained += min(len(chunk), remaining)
            reader = threading.Thread(target=diagnostics, daemon=True)
            reader.start()
            try:
                deadline = time.monotonic() + 35
                step = -1
                libraries = None
                while child.poll() is None:
                    safety.unlocked()
                    assert time.monotonic() < deadline, 'Fixture exceeded its deadline'
                    if libraries is None:
                        libraries = loaded_libraries(child, prefix)
                        if libraries:
                            (output / 'loaded-libraries.json').write_text(json.dumps(libraries, indent=2))
                    command_file = output / 'trace/command.json'
                    if command_file.exists():
                        command = json.loads(command_file.read_text())
                        if command['step'] != step:
                            resize(child, command, output, args.timer_burst)
                            step = command['step']
                    time.sleep(.05)
            finally:
                safety.stop(child)
                reader.join(timeout=3)
                assert not reader.is_alive(), 'Fixture diagnostic pipe did not close'
                child.stdout.close()
            assert child.returncode == 0, f'Fixture exited {child.returncode}'
            assert libraries, 'Fixture exited before its GTK/WebKit mappings were verified'
        backend = (output / 'trace/backend.txt').read_text()
        assert backend == 'GdkWaylandDisplay', backend
        result = validate(json.loads((output / 'trace/trace.json').read_text()), args.min_fps)
        (output / 'result.json').write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
        return 1 if result['failures'] else 0
    finally:
        safety.stop(bus)


if __name__ == '__main__':
    raise SystemExit(main())
