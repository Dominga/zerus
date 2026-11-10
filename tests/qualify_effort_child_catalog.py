#!/usr/bin/env python3
"""Actual-CLI Codex child-catalog refusal with a private scripted tmux agent.

The ordinary path positively reads a real SQLite catalog and native rollout.
The ID Effort path must refuse before opening that catalog, retain stale raw
child evidence, and deliver no input. This is not official-agent qualification.
"""
import argparse
import contextlib
import fcntl
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time

import native_effort_id
import test_effort
import test_input

CHILD = '22222222-2222-4222-8222-222222222222'
TURN = '33333333-3333-4333-8333-333333333333'
REFUSAL = 'ID Effort cannot verify this Codex child catalog; use Terminal'


def cleanup(fixture):
    until = time.monotonic() + 2
    while True:
        locks = [fixture.state / '.lock']
        sid = fixture.record.get('session_id')
        if sid:
            locks.append(fixture.state / 'native-input' / (sid + '.lock'))
        busy = False
        for path in locks:
            if not path.exists():
                continue
            with path.open('rb') as stream:
                try:
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    fcntl.flock(stream, fcntl.LOCK_UN)
                except BlockingIOError:
                    busy = True
        if not busy:
            fixture.tearDown()
            fixture.doCleanups()
            return
        if time.monotonic() >= until:
            fixture.temp._finalizer.detach()
            raise RuntimeError('Keeper cleanup unknown; retained private fixture at ' + str(fixture.root))
        time.sleep(.02)


def rollout(path, parent, event, stamp):
    header = dict(type='session_meta', payload=dict(id=CHILD, parent_thread_id=parent))
    row = dict(type='event_msg', timestamp=stamp,
               payload=dict(type=event, turn_id=TURN, last_agent_message='Complete'))
    path.write_text(json.dumps(header) + '\n' + json.dumps(row) + '\n')


def qualify(args):
    trace = shutil.which('strace')
    if not trace:
        raise RuntimeError('This Linux qualification requires strace; no skipped proof')
    hgs_sha256 = native_effort_id.image_digest(test_input.HGS)
    fixture = test_effort.EffortTransport(methodName='runTest')
    fixture.setUp()
    try:
        home = fixture.root / '.codex'
        database = home / 'state_5.sqlite'
        child = home / 'rollout-child.jsonl'
        with contextlib.closing(sqlite3.connect(database)) as db:
            db.execute('CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT)')
            db.execute('INSERT INTO threads VALUES(?,?)', (CHILD, str(child)))
            db.commit()
        fixture.record.update(main_done=True, agent_home=str(home), subagents={CHILD: dict(
            name='Catalog child', state='working', started=1.0, updated=2.0)})
        fixture.write_record()
        rollout(child, fixture.conversation_id, 'task_complete', '1970-01-01T00:00:03Z')
        # Without the native catalog/rollout, the recorded working child would
        # block this ordinary transaction. It must really change the terminal.
        ordinary = fixture.change(fixture.payload('high'))
        fixture.assertEqual(ordinary['status'], 'confirmed')
        fixture.assertEqual(fixture.received(), b'\x1b,')
        fixture.record = json.loads(fixture.record_path.read_bytes())
        fixture.assertEqual(fixture.record['subagents'][CHILD]['state'], 'finished')
        image = args.test_image.resolve(strict=True)
        os.environ['ZERUS_PICKER_EFFORT_SEED_IMAGE'] = str(image)
        os.environ['ZERUS_PICKER_EFFORT_SEED_SHA256'] = native_effort_id.image_digest(image)
        record, path, legacy = native_effort_id.moved_record(fixture, fixture.name, fixture.record)
        fixture.record, fixture.record_path = record, path
        fixture.assertEqual(record['subagents'][CHILD]['state'], 'finished')
        # Native work has restarted while the authoritative saved row still
        # looks settled. Silently omitting reconciliation would be unsafe.
        rollout(child, fixture.conversation_id, 'task_started', '1970-01-01T00:00:04Z')
        observed = native_effort_id.inspect(fixture, fixture.name, record['session_id'])
        fixture.assertEqual(observed['subagent_active_count'], 1)
        fixture.assertEqual(observed['phase'], 'working')
        fixture.assertEqual(json.loads(path.read_bytes())['subagents'][CHILD]['state'], 'finished')
        before = path.read_bytes(), database.read_bytes(), fixture.received()
        sidecars = {suffix: (home / ('state_5.sqlite' + suffix)).exists()
                    for suffix in ('-wal', '-shm', '-journal')}
        request = fixture.payload('low')
        log = fixture.root / 'native-catalog-opens.log'
        command = [trace, '-f', '-o', str(log), '-s', '2048',
                   '-e', 'trace=open,openat,openat2,creat', str(test_input.HGS),
                   'effort', fixture.name, '--session-id', record['session_id'], '--json']
        result = subprocess.run(command, input=json.dumps(request), env=fixture.env,
                                text=True, capture_output=True, timeout=25)
        fixture.assertNotEqual(result.returncode, 0, result.stdout)
        fixture.assertIn(REFUSAL, result.stderr)
        fixture.assertLess(log.stat().st_size, 4 * 1024**2)
        opens = [line for line in log.read_text().splitlines() if 'state_5.sqlite' in line]
        fixture.assertEqual(opens, [], 'the keeper opened the native SQLite catalog')
        fixture.assertEqual((path.read_bytes(), database.read_bytes(), fixture.received()), before)
        fixture.assertEqual({suffix: (home / ('state_5.sqlite' + suffix)).exists()
                             for suffix in sidecars}, sidecars)
        fixture.assertFalse((fixture.state / 'effort_receipts' / (request['request_id'] + '.json')).exists())
        fixture.assertFalse(legacy.exists())
        fixture.assertEqual(native_effort_id.image_digest(test_input.HGS), hgs_sha256)
        return dict(boundary='actual CLI; private scripted tmux/SQLite fixture; not official native',
                    hgs_sha256=hgs_sha256,
                    seed_sha256=native_effort_id.image_digest(image),
                    ordinary_native_child_reconciliation=True,
                    stale_finished_metadata_preserved=True, sticky_refusal=REFUSAL,
                    native_catalog_opens=0, new_keys=0, new_receipts=0,
                    source_bytes_and_sidecar_membership_unchanged=True)
    finally:
        cleanup(fixture)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-image', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = qualify(args)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
