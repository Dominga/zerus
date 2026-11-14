"""Private fixture hook producers whose hgs children retain real ancestry.

Synthetic events still pass the production hook command. The producer runs
under the scripted agent, or as a genuine async SessionStart child of a private
Claude process; the test controller never borrows another process's markers.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
import uuid


def run(test, event, env, timeout=12, *, command='hook'):
    """Return the actual child command/result from the fixture's agent tree."""
    selected = getattr(test, 'native_hook_socket', None) or scripted_socket(test.root, env)
    if selected:
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(timeout)
            until = time.monotonic() + min(timeout, 1)
            while True:
                try:
                    client.connect(str(selected)); break
                except (FileNotFoundError, ConnectionRefusedError):
                    if time.monotonic() >= until: raise
                    time.sleep(.01)
            client.sendall(json.dumps(dict(event=event, env=env, command=command)).encode() + b'\n')
            client.shutdown(socket.SHUT_WR)
            data = b''
            while chunk := client.recv(65536):
                data += chunk
            reply = json.loads(data)
    else:
        token = uuid.uuid4().hex
        request = test.root / (token + '-hook.json')
        result = test.root / (token + '-hook-result.json')
        request.write_text(json.dumps(dict(event=event, env=env, result=str(result), command=command)))
        program = Path(__file__).resolve()
        launch = dict(argv=[sys.executable, str(program), 'once', str(request)], env=env)
        temporary = test.root / 'hook-launch.tmp'
        temporary.write_text(json.dumps(launch)); temporary.replace(test.root / 'nested-launch.json')
        pid = int((test.root / 'agent-pid').read_text())
        os.kill(pid, signal.SIGUSR2)  # This fixture's recorded scripted agent only.
        until = time.monotonic() + timeout
        while not result.exists():
            if time.monotonic() >= until:
                raise TimeoutError('the owned fixture hook producer did not finish')
            time.sleep(.01)
        reply = json.loads(result.read_text())
        request.unlink(); result.unlink()
    return subprocess.CompletedProcess(reply['args'], reply['returncode'], reply['stdout'], reply['stderr'])


def invoke(request):
    import test_input
    selected = request['env'].get('HGS_TEST_BIN') or str(test_input.HGS)
    command = request.get('command', 'hook')
    if command not in ('hook', 'answer-channel'):
        return dict(args=[], returncode=1, stdout='', stderr='unsupported fixture hook operation')
    argv = [selected, '__state', command]
    try:
        result = subprocess.run(argv,
            input=json.dumps(request['event']), env=request['env'], text=True, capture_output=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired) as error:
        return dict(args=argv, returncode=1, stdout='', stderr='fixture hook invocation failed: ' + str(error))
    return dict(args=result.args, returncode=result.returncode, stdout=result.stdout, stderr=result.stderr)


def install_native(config, root):
    """Fixture-only async child, kept below the actual private Claude process."""
    import shlex
    path = root / 'hook-caller.sock'
    command = ' '.join(shlex.quote(x) for x in [sys.executable, str(Path(__file__).resolve()), 'serve', str(path)])
    config.setdefault('hooks', {}).setdefault('SessionStart', []).append(dict(hooks=[
        {'type':'command', 'command':command, 'async':True}]))
    return path



def scripted_path(root, run):
    token = hashlib.sha256(run.encode()).hexdigest()[:12]
    return root / ('hook-' + token + '.sock')


def scripted_socket(root, env):
    for run in (env.get('HGS_RUN_ID', ''), ''):
        path = scripted_path(root, run)
        if path.exists(): return path
    return None


def start_scripted(root):
    import threading
    path = scripted_path(root, os.environ.get('HGS_RUN_ID', ''))
    threading.Thread(target=serve, args=(path, False, 3600), daemon=True).start()
    until = time.monotonic() + 3
    while not path.exists():
        if time.monotonic() >= until: raise TimeoutError('scripted hook producer did not start')
        time.sleep(.01)


def scripted_source():
    return ('import sys, pathlib\n'
            + 'sys.path.insert(0, ' + repr(str(Path(__file__).resolve().parent)) + ')\n'
            + 'import hook_caller_fixtures\n'
            + 'hook_caller_fixtures.start_scripted(pathlib.Path.home())\n')


def lifecycle(test, event, env):
    import test_pause
    env = dict(env, HGS_TEST_BIN=str(test_pause.HGS))
    if env.get('HGS_SESSION') and env.get('HGS_RUN_ID'):
        record = test.binding(env['HGS_SESSION'])
        if (record.get('agent') == 'claude' and record.get('run_id') == env['HGS_RUN_ID']
                and record.get('pid') and record.get('process_start')):
            # A no-hook launch publishes its record before its producer starts.
            # Wait only for this current fixture run; stale/missing markers
            # must still exercise the direct, unadmitted CLI path below.
            test.wait(lambda: scripted_socket(test.root, env))
    # Missing or stale marker tests keep their direct, unadmitted CLI path.
    if scripted_socket(test.root, env):
        try: return run(test, event, env)
        except (FileNotFoundError, ConnectionRefusedError): pass  # Nothing was sent.
    return subprocess.run([str(test_pause.HGS), '__state', 'hook'],
        input=json.dumps(event), env=env, text=True, capture_output=True, timeout=15)


def stop(path):
    if not path.exists(): return
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(12)
        try: client.connect(str(path))
        except (FileNotFoundError, ConnectionRefusedError):
            path.unlink(missing_ok=True); return
        client.sendall(b'{"stop":true}\n'); client.shutdown(socket.SHUT_WR)
        while client.recv(4096): pass
    until = time.monotonic() + 3
    while path.exists() and time.monotonic() < until: time.sleep(.01)
    if path.exists(): raise TimeoutError("fixture hook producer did not close")


def serve(path, read_stdin=True, lifetime=180):
    # Consume Claude's authentic hook input. No decision or model context is emitted.
    from concurrent.futures import ThreadPoolExecutor
    import threading
    if read_stdin: json.load(sys.stdin)
    deadline = time.monotonic() + lifetime
    stopped = threading.Event()
    slots = threading.BoundedSemaphore(4)
    def handle(client):
        try:
            with client:
                client.settimeout(12)
                data = b''
                while chunk := client.recv(65536):
                    data += chunk
                    if len(data) > 256 * 1024:
                        raise ValueError('fixture hook request is too large')
                request = json.loads(data)
                if request.get('stop'):
                    client.sendall(b'{}'); stopped.set(); return
                client.sendall(json.dumps(invoke(request)).encode())
        finally:
            slots.release()
    with socket.socket(socket.AF_UNIX) as server, ThreadPoolExecutor(max_workers=4) as workers:
        server.bind(str(path)); os.chmod(path, 0o600); server.listen(16); server.settimeout(.1)
        try:
            while time.monotonic() < deadline and not stopped.is_set():
                try:
                    client, _ = server.accept()
                except TimeoutError:
                    continue
                slots.acquire()
                workers.submit(handle, client)
        finally:
            path.unlink(missing_ok=True)


if __name__ == '__main__':
    if sys.argv[1] == 'once':
        request = json.loads(Path(sys.argv[2]).read_text())
        result = Path(request['result'])
        temporary = result.with_suffix('.tmp')
        temporary.write_text(json.dumps(invoke(request))); temporary.replace(result)
    elif sys.argv[1] == 'serve':
        serve(Path(sys.argv[2]))
