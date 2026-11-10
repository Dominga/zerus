"""Private step-4 trace: real updater/helper, shared launchd model, no native GUI."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

repo, root = map(Path, sys.argv[1:3])
action, args = sys.argv[3], sys.argv[4:]
sys.path.insert(0, str(repo / 'tests'))
from test_macos_desktop_service import Commands, installer

spec = importlib.util.spec_from_file_location('step4_updater', repo / 'packaging/desktop/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)
home = root / 'home'
installed = home / 'Applications/hgs-tray.app'
backend, runner = installed / 'Contents/MacOS/hgs', root / 'bin/hgs'
state_path = root / 'launchd-model.json'
state = json.loads(state_path.read_text()) if state_path.exists() else {}
model = Commands(home, backend)
fields = ('loaded', 'printed_path', 'printed_argv', 'pid', 'status')
for key in fields:
    if key in state:
        setattr(model, key, state[key])
events = state.get('events', [])
gui_loaded = state.get('gui_loaded', False)


def command(argv, **kwargs):
    if argv[0] == 'launchctl' and argv[1] in ('bootout', 'bootstrap'):
        events.append('desktop:' + argv[1])
    return model(argv, **kwargs)


def desktop(argv):
    selected = Path(argv[0])
    assert selected == backend, argv
    passed_runner = Path(argv[argv.index('--runner-hgs') + 1]) if '--runner-hgs' in argv else None
    assert passed_runner == runner, argv
    return installer.install(selected, home, {'HOME': str(home)}, command=command,
                             restart='--restart' in argv, runner_hgs=passed_runner,
                             hold_factory=model.hold)


def gui(argv, **kwargs):
    global gui_loaded
    argv = list(map(str, argv))
    code, output = 0, ''
    if argv[1] == 'print':
        code = 0 if gui_loaded else 113
    elif argv[1] == 'list':
        output = '9876\t0\tcom.hgdev.hgs-tray\n' if gui_loaded else ''
    elif argv[1] in ('bootout', 'unload'):
        gui_loaded = False
    elif argv[1] in ('bootstrap', 'load'):
        events.append('gui:bootstrap')
        gui_loaded = True
    else:
        raise AssertionError(argv)
    return subprocess.CompletedProcess(argv, code, output, '')


def invoke(argv, **kwargs):
    argv = list(map(str, argv))
    if argv[0] == 'launchctl':
        return gui(argv, **kwargs).stdout
    assert Path(argv[1]).name == 'install-macos-desktop-service.py', argv
    return str(desktop(argv[2:]))


try:
    if action == 'init':
        desktop([str(backend), '--runner-hgs', str(runner)])
        events.clear()
    elif action == 'python':
        script, passed = Path(args[0]), args[1:]
        if script.name == 'update-installed.py':
            assert passed[:2] == ['--refresh-macos-desktop', str(runner)], passed
            assert Path(passed[3]) == installed, passed
            updater.MAC, updater.REFRESH_MAC_DESKTOP = True, runner
            updater.PLIST = home / 'Library/LaunchAgents/com.hgdev.hgs-tray.plist'
            updater.subprocess = SimpleNamespace(run=gui)
            updater.run = invoke
            updater.start(installed)
        else:
            assert script.name == 'install-macos-desktop-service.py', script
            desktop(passed)
    elif action == 'launchctl':
        sys.stdout.write(gui(['launchctl', *args]).stdout)
    else:
        raise AssertionError(action)
finally:
    state.update({key: getattr(model, key) for key in fields})
    state.update(events=events, gui_loaded=gui_loaded)
    state_path.write_text(json.dumps(state))
