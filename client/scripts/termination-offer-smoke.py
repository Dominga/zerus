#!/usr/bin/python3
"""Browser-only actual dialog check. Native calls are recorded mock ports only."""
import argparse, os, signal, socket, subprocess, tempfile, time, urllib.request
from pathlib import Path
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--browser', default='/usr/bin/google-chrome-stable')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='zerus-offer-dialog-') as folder:
    private = Path(folder)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0)); port = listener.getsockname()[1]
    log = (private/'vite.log').open('wb')
    server = subprocess.Popen(['node', str(root/'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', str(port), '--strictPort'], cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    browser = None
    try:
        url = f'http://127.0.0.1:{port}/scripts/termination-offer-smoke.html'
        until = time.monotonic()+20
        while True:
            if server.poll() is not None: raise RuntimeError('Private Vite server exited')
            try:
                with urllib.request.urlopen(url, timeout=1) as reply:
                    assert b'Private Terminate offer fixture' in reply.read(65536)
                break
            except OSError:
                if time.monotonic() >= until: raise
                time.sleep(.05)
        with (private/'browser.out').open('wb') as out, (private/'browser.err').open('wb') as err:
            browser = subprocess.Popen([args.browser,'--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-extensions','--disable-component-update',f'--user-data-dir={private/"profile"}','--dump-dom','--virtual-time-budget=10000',url], stdout=out, stderr=err, start_new_session=True)
            code = browser.wait(timeout=40)
        data = (private/'browser.out').read_bytes()
        assert len(data) <= 1 << 20
        assert code == 0 and b'<pre id="result">PASS Linux/macOS' in data, data[-10000:].decode(errors='replace')
        print('PASS actual Linux/macOS dialog: visible explanation, ordinary Terminate, guarded manual review, zero fallback commands')
    finally:
        for owned in (browser, server):
            if owned is not None and owned.poll() is None:
                os.killpg(owned.pid, signal.SIGTERM)
                try: owned.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(owned.pid, signal.SIGKILL); owned.wait(timeout=5)
        log.close()
