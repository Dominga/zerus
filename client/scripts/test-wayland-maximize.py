#!/usr/bin/env python3
"""Check a selected private GTK build in its own compositing KWin instance.

No owner display, GUI, configuration or session bus is used. Build the production
resize_fixture example first. --control-lib-dir must reproduce the regression;
--lib-dir must maximize on one output and restore the original normal rectangle.
Requires Linux, KWin, qdbus6 and a working EGL render device. This is an explicit
native check, not a fallback to XWayland or a non-compositing geometry model.
"""
import argparse
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time
import uuid


LIMIT = 1024 * 1024
TITLE = 'Zerus isolated resize fixture'


def stop(child):
    if child.poll() is None:
        child.terminate()
    try:
        child.wait(timeout=4)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=4)


def read(path):
    with path.open('rb') as stream:
        value = stream.read(LIMIT + 1)
    assert len(value) <= LIMIT, f'Native evidence exceeds {LIMIT} bytes: {path.name}'
    return value.decode('utf-8', 'replace')


def call(env, path, method, *args):
    return subprocess.check_output(['qdbus6', 'org.kde.KWin', path, method, *args],
                                   env=env, stdin=subprocess.DEVNULL,
                                   stderr=subprocess.PIPE, text=True, timeout=2).strip()


def check(rows):
    normal = [r for r in rows if r['client']['width'] == 900 and r['client']['height'] == 600]
    assert normal, 'No original normal geometry was observed'
    area = normal[0]['output']
    maximized = [r for r in rows if r['tick'] > normal[0]['tick']
                 and r['frame']['width'] == area['width']
                 and r['frame']['height'] == area['height']]
    assert len(maximized) >= 3, 'No settled maximized frame was observed'
    prior = [r for r in normal if r['tick'] < maximized[0]['tick']]
    assert len(prior) >= 3, 'No settled original normal frame was observed'
    before = prior[-1]
    after = [r for r in normal if r['tick'] > maximized[-1]['tick']]
    assert len(after) >= 3, 'No settled normal restore was observed'
    failures = []
    if maximized[-1]['frame'] != area:
        failures.append('maximized frame moved outside its original output')
    if after[-1]['frame'] != before['frame']:
        failures.append('normal restore changed the original rectangle')
    return dict(normal=before['frame'], maximized=maximized[-1]['frame'],
                restored=after[-1]['frame'], output=area, failures=failures)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--webkit-prefix', type=Path, required=True)
    parser.add_argument('--lib-dir', type=Path, required=True)
    parser.add_argument('--control-lib-dir', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    fixture = args.fixture.resolve(strict=True)
    prefix = args.webkit_prefix.resolve(strict=True)
    selected = [('repaired', args.lib_dir.resolve(strict=True))]
    if args.control_lib_dir:
        selected.insert(0, ('control', args.control_lib_dir.resolve(strict=True)))
    for _, directory in selected:
        assert (directory / 'libgdk-3.so.0').is_file(), directory
    assert (prefix / 'gtk/lib/libgtk-3.so.0').is_file(), prefix
    assert (prefix / 'lib/libwebkit2gtk-4.1.so.0').is_file(), prefix
    output = args.output.resolve()
    output.mkdir(mode=0o700)  # Never reuse a previous result directory.
    children = []
    with tempfile.TemporaryDirectory(prefix='zerus-maximize-') as temporary:
        root = Path(temporary)
        # Deliberately omit the owner's DISPLAY, Wayland socket, bus and homes.
        env = dict(PATH=os.environ['PATH'], LANG='C.UTF-8',
                   QT_QPA_PLATFORM='offscreen', KWIN_COMPOSE='O2',
                   QT_LOGGING_RULES='kwin_scripting.debug=true',
                   QT_FORCE_STDERR_LOGGING='1', NO_AT_BRIDGE='1',
                   GSETTINGS_BACKEND='memory', XDG_SESSION_TYPE='wayland')
        for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME',
                    'XDG_STATE_HOME', 'XDG_RUNTIME_DIR', 'HGS_CONFIG_DIR', 'HGS_STATE_DIR'):
            directory = root / key.lower()
            directory.mkdir(mode=0o700)
            env[key] = str(directory)
        try:
            bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'],
                                   env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL)
            children.append(bus)
            assert select.select([bus.stdout], [], [], 3)[0], 'Private bus did not start'
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline(8193).decode().strip()
            assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:'), 'Invalid private bus'
            with (output / 'kwin.log').open('wb') as log:
                compositor = subprocess.Popen([
                    'kwin_wayland', '--virtual', '--width', '3440', '--height', '1440',
                    '--output-count', '2', '--socket', 'maximize-test', '--no-lockscreen',
                    '--no-global-shortcuts', '--no-kactivities'], env=env,
                    stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                children.append(compositor)
                deadline = time.monotonic() + 10
                while True:
                    assert compositor.poll() is None, 'Private compositor exited'
                    try:
                        support = call(env, '/KWin', 'org.kde.KWin.supportInformation')
                        scripting = call(env, '/Scripting', 'org.freedesktop.DBus.Introspectable.Introspect')
                        if 'loadScript' in scripting:
                            break
                    except (subprocess.SubprocessError, OSError):
                        pass
                    assert time.monotonic() < deadline, 'Private compositor did not become ready'
                    time.sleep(.1)
                assert 'KWin::VirtualBackend' in support, 'Refusing a physical compositor'
                assert 'Compositing is active' in support, 'A non-compositing control is invalid'
                socket = Path(env['XDG_RUNTIME_DIR']) / 'maximize-test'
                assert socket.is_socket(), 'Private Wayland socket is absent'
                env.update(WAYLAND_DISPLAY=str(socket), GDK_BACKEND='wayland')
                results = {}
                for name, directory in selected:
                    child_env = dict(env, LD_LIBRARY_PATH=':'.join(map(str, (
                        directory, prefix / 'gtk/lib', prefix / 'lib'))),
                        WEBKIT_EXEC_PATH=str(prefix / 'libexec/webkit2gtk-4.1'),
                        ZERUS_RESIZE_STEPS='4', ZERUS_RESIZE_DELAY_MS='1500')
                    trace = output / (name + '-trace')
                    plugin = 'zerus-private-maximize-' + uuid.uuid4().hex
                    with (output / (name + '.log')).open('wb') as diagnostics:
                        child = subprocess.Popen([str(fixture), str(trace)], env=child_env,
                                                 stdin=subprocess.DEVNULL,
                                                 stdout=diagnostics, stderr=subprocess.STDOUT)
                        children.append(child)
                        script = output / (name + '.js')
                        config = dict(pid=child.pid, title=TITLE, name=name)
                        script.write_text('const c=' + json.dumps(config) + ''';
let n=0,id=null;const t=new QTimer();t.interval=100;
function rect(g){return {x:g.x,y:g.y,width:g.width,height:g.height};}
t.timeout.connect(function(){
 if(n++>=110){t.stop();return;}
 const a=workspace.windowList().filter(w=>w.pid===c.pid && w.normalWindow && !w.deleted && w.caption===c.title);
 if(a.length!==1)return;const w=a[0];if(id===null)id=String(w.internalId);
 if(String(w.internalId)!==id){t.stop();return;}
 print('ZERUS_MAXIMIZE '+JSON.stringify({name:c.name,tick:n,frame:rect(w.frameGeometry),
  client:rect(w.clientGeometry),output:rect(w.output.geometry)}));
});t.start();
''')
                        number = call(env, '/Scripting', 'org.kde.kwin.Scripting.loadScript', str(script), plugin)
                        assert number.isdigit(), 'Invalid private observer'
                        try:
                            call(env, '/Scripting/Script' + number, 'org.kde.kwin.Script.run')
                            deadline = time.monotonic() + 15
                            loaded = False
                            while child.poll() is None:
                                assert time.monotonic() < deadline, 'Fixture exceeded its deadline'
                                if not loaded:
                                    maps = read(Path('/proc') / str(child.pid) / 'maps')
                                    paths = {Path(l.split(maxsplit=5)[5]).resolve() for l in maps.splitlines()
                                             if 'libgdk-3.so' in l}
                                    if paths:
                                        assert paths == {(directory / 'libgdk-3.so.0').resolve()}, paths
                                        loaded = True
                                time.sleep(.05)
                            assert child.returncode == 0 and loaded, 'Fixture failed or selected GDK was not loaded'
                        finally:
                            stop(child)
                            call(env, '/Scripting', 'org.kde.kwin.Scripting.unloadScript', plugin)
                    assert read(trace / 'backend.txt') == 'GdkWaylandDisplay', 'Not a native Wayland control'
                    lines = read(output / 'kwin.log').splitlines()
                    rows = [json.loads(line.split('ZERUS_MAXIMIZE ', 1)[1])
                            for line in lines if 'ZERUS_MAXIMIZE ' in line]
                    rows = [r for r in rows if r['name'] == name]
                    results[name] = check(rows)
                    (output / 'result.json').write_text(json.dumps(results, indent=2))
                    if name == 'control':
                        assert results[name]['failures'], 'Control did not reproduce the regression'
                    else:
                        assert not results[name]['failures'], results[name]
                print(json.dumps(results, indent=2))
                return 0
        finally:
            for child in reversed(children):
                stop(child)
            for child in children:
                if child.stdout is not None:
                    child.stdout.close()


if __name__ == '__main__':
    raise SystemExit(main())
