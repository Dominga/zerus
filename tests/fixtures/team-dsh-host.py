"""An isolated reviewed harness for the Rust Teams delivery contracts.

The binding was made by dsh's Rust writer. This helper starts only its own
keyless native host and closes it when its owning test closes stdin.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time

root = Path(sys.argv[1])
repo = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(repo / 'scripts/ci'))
import dsh_fixture

name = 'dsh/example/member'
binding = json.loads((root / 'dsh/bindings' / (hashlib.sha256(name.encode()).hexdigest() + '.json')).read_text())
home = Path(binding['agent_home'])
host = root / 'dsh/hosts' / hashlib.sha256(str(home).encode()).hexdigest()[:16]
host.mkdir(parents=True, mode=0o700, exist_ok=True)
workspace = root / 'native-workspace'
workspace.mkdir()
holds = root / 'holds'
holds.mkdir()
tmux = root / 'native-tmux'
tmux.mkdir(mode=0o700)
exe = shutil.which('dsh')
if not exe:
    raise RuntimeError('The reviewed official dsh must be first in PATH')
package = dsh_fixture.package_root(exe)
patch = root / 'native-patch.json'
patch.write_text(json.dumps([
    {'id': 'llm-deepseek', 'disabled': True},
    {'id': 'agent-default-model', 'config': {'provider': 'hgs-test', 'model': 'test-a'}},
    {'id': 'schedule', 'disabled': True},
    {'insert': [
        {'id': 'hgs-test-model', 'name': str(repo / 'tests/fixtures/dsh-model.mjs'), 'config': {'holds': str(holds)}},
        {'id': 'hgs-native-dsh', 'name': str(repo / 'src/dsh/bridge.mjs'), 'config': {'socket': str(host / 'host.sock')}},
        {'id': 'hgs-team-control', 'name': str(repo / 'tests/fixtures/team-dsh-control.mjs'), 'config': {'root': str(root), 'socket': str(host / 'host.sock')}},
    ]},
]))
env = {'PATH': os.environ['PATH'], 'LANG': 'C.UTF-8', 'HOME': str(root / 'native-user'),
       'DSH_HOME': str(home), 'DSH_TELEMETRY_DISABLED': '1', 'HGS_DSH_TEST_PACKAGE': str(package),
       'HGS_STATE_DIR': str(root), 'HGS_CONFIG_DIR': str(root / 'config'), 'HGS_SELF': 'fixture',
       'HGS_PEERS': '', 'TMUX_TMPDIR': str(tmux)}
Path(env['HOME']).mkdir()

def rpc(method, params=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(30)
        stream.connect(str(host / 'host.sock'))
        stream.sendall((json.dumps({'method': method, 'params': params or {}}) + '\n').encode())
        answer = json.loads(stream.makefile().readline())
    if not answer['ok']:
        raise RuntimeError(answer.get('error', str(answer)))
    return answer['value']

with (root / 'native-host.log').open('w') as log:
    proc = subprocess.Popen([exe, '--profile', 'web', '--patch', str(patch), '--no-open', '--host', '127.0.0.1', '--port', '0'],
                            env=env, cwd=workspace, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
    try:
        until = time.monotonic() + 40
        while not (host / 'host.sock').exists() and proc.poll() is None and time.monotonic() < until:
            time.sleep(.1)
        if not (root / 'native-control-ready').exists():
            raise RuntimeError('The fixture control plugin did not load')
        rpc('create', {'sessionId': binding['conversation_id'], 'cwd': binding['cwd'], 'title': name})
        (root / 'native-ready.json').write_text(json.dumps({'pid': proc.pid, 'host': str(host)}))
        for line in sys.stdin:
            request = json.loads(line)
            if request['method'] == 'fixture-stop-host':
                proc.kill()
                proc.wait(timeout=15)
                print(json.dumps({'stopped': True}), flush=True)
                continue
            answer = rpc(request['method'], request.get('params'))
            print(json.dumps(answer), flush=True)
    finally:
        if proc.poll() is None:
            proc.terminate()
        try:
            proc.wait(timeout=15)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
