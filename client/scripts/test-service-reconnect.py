#!/usr/bin/env python3
"""Restart a real private service under the GUI catalog adapter; no display or manager."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hgs', type=Path, required=True)
    parser.add_argument('--test-binary', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    sys.path.insert(0, str(repo / 'tests'))
    import test_desktop as kernel
    import test_desktop_catalog as catalog
    kernel.HGS = args.hgs.resolve(strict=True)
    test_binary = args.test_binary.resolve(strict=True)
    fixture = catalog.Catalog()
    fixture.setUp()
    child = None
    log = fixture.root / 'gui-fixture.log'
    try:
        # Helpers always use this exact isolated configuration. Manager/SSH/
        # tmux tools are denied by the standard passive desktop test fixture.
        env = dict(fixture.env)
        env['PATH'] = str(fixture.root / '.local/bin') + os.pathsep + env['PATH']
        fixture.env = env
        relay = fixture.root / 'relay'
        private = {key: env[key] for key in ('HOME', 'USERPROFILE', 'TMPDIR', 'TMUX_TMPDIR',
                   'HGS_CONFIG_DIR', 'HGS_STATE_DIR', 'HGS_SELF', 'HGS_PEERS', 'PATH')}
        relay.write_text('#!' + sys.executable + '\nimport os,sys,json\n'
                         + 'env={k:v for k,v in os.environ.items() if not k.startswith("HGS_") and k not in ("TMUX","TMUX_PANE")}\n'
                         + 'env.update(' + repr(private) + ')\n'
                         + 'with open(' + repr(str(fixture.root / 'gui-calls')) + ',"a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n'
                         + 'os.execve(' + repr(str(kernel.HGS)) + ', [' + repr(str(kernel.HGS)) + '] + sys.argv[1:], env)\n')
        relay.chmod(0o700)
        first, status = fixture.serve()
        test_env = dict(fixture.env)
        test_env['ZERUS_TEST_SERVICE_RESTART'] = str(fixture.root)
        with log.open('w') as output:
            child = subprocess.Popen([str(test_binary), '--exact',
                'desktop_service::tests::restart::real_service_restart_retains_catalog_and_recovers_without_retry',
                '--ignored', '--nocapture', '--test-threads=1'], env=test_env,
                stdin=subprocess.DEVNULL, stdout=output, stderr=output)

        def wait(name):
            deadline = time.monotonic() + 45
            while not (fixture.root / name).exists():
                if child.poll() is not None:
                    raise AssertionError(log.read_text())
                if time.monotonic() >= deadline:
                    raise AssertionError('private fixture marker deadline: ' + name)
                time.sleep(.01)

        def seed_begin():
            feed, _, frames = fixture.feed()
            fixture.send(feed, frames, fixture.begin(rows=2, pages=1))
            return feed, frames

        def seed_finish(feed, frames, numbers):
            fixture.send(feed, frames, dict(t='legacy_page', computer='peer', generation=1,
                                           index=0, items=[fixture.item(n) for n in numbers]))
            fixture.send(feed, frames, dict(t='legacy_complete', computer='peer', generation=1))

        wait('client-connected')
        feed, frames = seed_begin()
        seed_finish(feed, frames, [1, 2])
        wait('baseline-ready')
        fixture.stop(first)  # Only the original private service child.
        wait('client-offline')
        time.sleep(.7)  # Exercise more than one unavailable reconnect attempt.
        second, next_status = fixture.serve()
        assert next_status['epoch'] != status['epoch']
        wait('client-reconnected')
        feed, frames = seed_begin()
        (fixture.root / 'staging-started').touch()
        wait('staging-retained')
        seed_finish(feed, frames, [1, 3])
        wait('client-finished')
        assert child.wait(timeout=15) == 0, log.read_text()
        assert not (fixture.root / 'unexpected-tool').exists()
        calls = [json.loads(line) for line in (fixture.root / 'gui-calls').read_text().splitlines()]
        assert all(call == ['--help'] or call[:2] in (
            ['desktop', 'status'], ['desktop', 'watch'], ['desktop', 'request'],
            ['desktop', 'catalog-relay']) for call in calls), calls
        print('PASS: real service restart; retained stale rows; automatic reconnect; complete membership recovery')
    finally:
        if child is not None and child.poll() is None:
            child.terminate()  # Only our original private libtest subprocess.
            child.wait(timeout=15)
        if log.exists() and child is not None and child.returncode:
            print(log.read_text(), file=sys.stderr)
        assert fixture.doCleanups(), 'private fixture cleanup was not confirmed'


if __name__ == '__main__':
    main()
