#!/usr/bin/env python3
"""Restart a real private service under the GUI catalog adapter; no display or manager."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

TESTS = {
    'restart': 'desktop_service::tests::restart::real_service_restart_retains_catalog_and_recovers_without_retry',
    'resume': 'desktop_service::tests::restart::real_service_resume_delta_keeps_the_native_replaced_reason',
    'paged-fresh': 'desktop_service::tests::restart::real_service_paged_start_fresh_keeps_the_journaled_replaced_reason',
    'resync': 'desktop_service::tests::restart::real_service_native_resync_keeps_a_live_computer_live',
    'parity': 'desktop_service::tests::restart::real_service_matches_native_delivery_of_lifecycle_frames',
}
# Real native watch frames of rename, a stopped run and archive, recorded from
# an isolated hgs with a fake agent and sanitized to example paths.
PARITY_FRAMES = Path(__file__).resolve().parents[1] / 'src-tauri/src/desktop_service/tests/fixtures/lifecycle-parity.ndjson'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hgs', type=Path, required=True)
    parser.add_argument('--test-binary', type=Path, required=True)
    parser.add_argument('--scenario', choices=sorted(TESTS), default='restart',
                        help='restart: service loss and automatic reconnect; resume: a native resume delta; '
                             'paged-fresh: a Start fresh whose native delta pages the removal and successor apart; '
                             'resync: a native catalog overflow resync of an online source; '
                             'parity: recorded native lifecycle frames, compared with native delivery')
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
            child = subprocess.Popen([str(test_binary), '--exact', TESTS[args.scenario],
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

        def resume_rows(run, state, conversation='conversation-1'):
            return dict(key='run:' + run, row=dict(name='codex/example/review', run_id=run, conversation_id=conversation,
                                                   agent='codex', state=state))

        wait('client-connected')
        if args.scenario == 'parity':
            # The recorded native stream, in order. After each catalog frame the
            # GUI test compares the service rows with native delivery and
            # acknowledges that step before the next frame.
            feed, _, frames = fixture.feed()
            for index, line in enumerate(PARITY_FRAMES.read_text().splitlines()):
                frame = json.loads(line)['frame']
                fixture.send(feed, frames, dict(t='watch', computer='peer', generation=1, frame=frame))
                if frame.get('topic') == 'catalog' and (frame['t'] == 'delta' or frame.get('last')):
                    wait(f'parity-{index}')
            wait('client-finished')
        elif args.scenario == 'resync':
            # The native watch's overflow resync (gap, then a full snapshot
            # over two pages) while its source stays online: the service
            # hints reset_required, then a new projection (zerus-bkbb).
            feed, _, frames = fixture.feed()
            hello = dict(v=1, t='hello', seq=1, machine_id='example-machine', host='example', hgs_version='1.56.0',
                         stream='example-stream', heartbeat_ms=10000, max_frame_bytes=1 << 20, topics=['catalog'],
                         limits=dict(activity_subscriptions=16, page_bytes=512 << 10, queue_bytes=4 << 20,
                                     activity_tail=100, activity_read=200, control_line_bytes=64 << 10))

            def watch(frame):
                fixture.send(feed, frames, dict(t='watch', computer='peer', generation=1, frame=frame))

            watch(hello)
            watch(dict(v=1, t='snapshot', seq=2, topic='catalog', items=[fixture.item(n) for n in (1, 2, 3)],
                       page=0, last=True, cursor=1))
            wait('baseline-ready')
            watch(dict(v=1, t='gap', seq=3, topic='catalog', reason='overflow'))
            watch(dict(v=1, t='snapshot', seq=4, topic='catalog', items=[fixture.item(n) for n in (1, 3)],
                       page=0, last=False, cursor=2))
            wait('staging-walked')
            watch(dict(v=1, t='snapshot', seq=5, topic='catalog', items=[fixture.item(4)], page=1, last=True, cursor=2))
            wait('client-finished')
        elif args.scenario in ('resume', 'paged-fresh'):
            # Native watch frames for a replacement of the stopped run by a
            # new run of the same name. `resume` sends the one native resume
            # frame, whose journal revision orders the removal first by key.
            feed, _, frames = fixture.feed()
            hello = dict(v=1, t='hello', seq=1, machine_id='example-machine', host='example', hgs_version='1.56.0',
                         stream='example-stream', heartbeat_ms=10000, max_frame_bytes=1 << 20, topics=['catalog'],
                         limits=dict(activity_subscriptions=16, page_bytes=512 << 10, queue_bytes=4 << 20,
                                     activity_tail=100, activity_read=200, control_line_bytes=64 << 10))

            def watch(frame):
                fixture.send(feed, frames, dict(t='watch', computer='peer', generation=1, frame=frame))

            watch(hello)
            watch(dict(v=1, t='snapshot', seq=2, topic='catalog', items=[resume_rows('run-1', 'saved')],
                       page=0, last=True, cursor=1))
            wait('baseline-ready')
            if args.scenario == 'resume':
                watch(dict(v=1, t='delta', seq=3, topic='catalog', upsert=[resume_rows('run-2', 'running')],
                           remove=[dict(key='run:run-1', reason='replaced')], cursor=2))
            else:
                # The native watch pages a delta's removals before its upserts.
                watch(dict(v=1, t='delta', seq=3, topic='catalog', upsert=[],
                           remove=[dict(key='run:run-1', reason='replaced')], cursor=2))
                watch(dict(v=1, t='delta', seq=4, topic='catalog', upsert=[resume_rows('run-2', 'running', 'fresh')],
                           remove=[], cursor=3))
            wait('client-finished')
        else:
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
        print({'restart': 'PASS: real service restart; retained stale rows; automatic reconnect; complete membership recovery',
               'resume': 'PASS: real service resume delta; successor first; native replaced reason restored',
               'paged-fresh': 'PASS: real service paged Start fresh; journaled native replaced reason',
               'resync': 'PASS: real service native resync; a live computer stays live; new membership',
               'parity': 'PASS: real service delivers recorded native lifecycle frames as native mode does'}[args.scenario])
    finally:
        if child is not None and child.poll() is None:
            child.terminate()  # Only our original private libtest subprocess.
            child.wait(timeout=15)
        if log.exists() and child is not None and child.returncode:
            print(log.read_text(), file=sys.stderr)
        assert fixture.doCleanups(), 'private fixture cleanup was not confirmed'


if __name__ == '__main__':
    main()
