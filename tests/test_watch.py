"""`hgs [@host] watch --json`: the NDJSON event stream (watch-v1).

A private tmux socket, a scripted agent and a synthetic state folder only.
"""
import json
import os
import queue
import sqlite3
import subprocess
import threading
import time
import unittest
import uuid

import test_input as fixtures

HGS = fixtures.HGS
VOLATILE = ('goal_observed_at',)


def steady(row):
    """A row without its observation timestamps."""
    row = {key: value for key, value in row.items() if key not in VOLATILE}
    if isinstance(row.get('process_work'), dict):
        row['process_work'] = {k: v for k, v in row['process_work'].items() if k != 'sampled_at'}
    return row


class Stream:
    """One running `hgs watch --json` and the frames it has written."""

    def __init__(self, test, *args):
        self.test = test
        self.process = subprocess.Popen([str(HGS), *args, 'watch', '--json'], env=test.env,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True)
        self.frames = queue.Queue()
        self.seen = []
        self.raw = []
        threading.Thread(target=self.read, daemon=True).start()
        test.addCleanup(self.stop)

    def read(self):
        for line in self.process.stdout:
            self.raw.append(line)
            self.frames.put(json.loads(line))
        self.frames.put(None)

    def next(self, predicate=lambda frame: True, timeout=10):
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.test.fail(f'no matching frame within {timeout}s; seen {[f["t"] for f in self.seen]}')
            try:
                frame = self.frames.get(timeout=remaining)
            except queue.Empty:
                continue
            if frame is None:
                self.test.fail('the stream ended: ' + self.process.stderr.read())
            self.seen.append(frame)
            if predicate(frame):
                return frame

    def quiet(self, predicate, seconds):
        """Fails if a matching frame arrives within `seconds`."""
        deadline = time.monotonic() + seconds
        while (remaining := deadline - time.monotonic()) > 0:
            try:
                frame = self.frames.get(timeout=remaining)
            except queue.Empty:
                return
            if frame is None:
                return
            self.seen.append(frame)
            self.test.assertFalse(predicate(frame), frame)

    def send(self, **operation):
        self.process.stdin.write(json.dumps(operation) + '\n')
        self.process.stdin.flush()

    def close(self, timeout=5):
        self.process.stdin.close()
        return self.process.wait(timeout=timeout)

    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        for stream in (self.process.stdout, self.process.stderr, self.process.stdin):
            if stream and not stream.closed:
                stream.close()


@unittest.skipUnless(fixtures.TMUX, 'private tmux fixture requires tmux')
class WatchStream(unittest.TestCase):
    setUp = fixtures.InputTransport.setUp
    tearDown = fixtures.InputTransport.tearDown
    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    write_record = fixtures.InputTransport.write_record

    def key(self):
        return 'run:' + self.run_id

    def session(self):
        return dict(name=self.name, run_id=self.run_id, conversation_id=self.conversation_id)

    def listing(self):
        result = subprocess.run([str(HGS), 'ls', '--json', '--local', '--no-archives'], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def journal(self, kind, **fields):
        """Appends one Activity row as a hook would; returns its seq."""
        payload = dict(type=kind, at=time.time(), agent_id='', tool='', detail='')
        payload.update(fields)
        db = sqlite3.connect(self.state / 'events.sqlite3')
        with db:
            db.execute('CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT)')
            cursor = db.execute('INSERT INTO events(name, conversation, payload) VALUES (?, ?, ?)',
                                (self.name, self.conversation_id, json.dumps(payload)))
            seq = cursor.lastrowid
        db.close()
        return seq

    def inspect_events(self, after):
        result = subprocess.run([str(HGS), 'inspect', self.name, '--after', str(after), '--skip-processes'],
                                env=self.env, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)['events']

    def start(self):
        stream = Stream(self)
        hello = stream.next()
        snapshot = stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'catalog' and f['last'])
        meta = stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'catalog.meta')
        return stream, hello, snapshot, meta

    def test_hello_catalog_snapshot_and_meta_match_the_listing_then_eof_ends_the_stream(self):
        stream, hello, snapshot, meta = self.start()
        self.assertEqual((hello['v'], hello['t'], hello['seq']), (1, 'hello', 1))
        self.assertEqual(hello['host'], 'test')
        self.assertEqual((hello['heartbeat_ms'], hello['max_frame_bytes']), (10000, 1048576))
        self.assertEqual(hello['topics'], ['catalog', 'catalog.meta', 'activity'])
        self.assertEqual(hello['limits']['activity_subscriptions'], 16)
        uuid.UUID(hello['stream'])
        self.assertEqual([frame['seq'] for frame in stream.seen], list(range(1, len(stream.seen) + 1)))
        self.assertTrue(all(frame['v'] == 1 for frame in stream.seen))
        self.assertEqual(snapshot['page'], 0)
        listing = self.listing()
        rows = {item['key']: item['row'] for item in snapshot['items']}
        self.assertEqual(set(rows), {self.key()})
        expected = next(row for row in listing['sessions'] if row['name'] == self.name)
        self.assertEqual(steady(rows[self.key()]), steady(expected))
        self.assertNotIn('sessions', meta['value'])
        self.assertEqual(set(meta['value']), set(listing) - {'sessions'})
        self.assertEqual(meta['value']['host'], 'test')
        self.assertTrue(all(len(line.encode()) <= 1 << 20 for line in stream.raw))
        started = time.monotonic()
        self.assertEqual(stream.close(), 0)
        self.assertLess(time.monotonic() - started, 3)
        self.assertEqual(stream.process.stderr.read(), '')

    def test_binding_changes_and_a_rename_are_upserts_on_the_run_key(self):
        stream, *_ = self.start()
        self.record.update(phase='working', activity='busy', prompt='Synthetic request', last_event_at=time.time())
        self.write_record()
        delta = stream.next(lambda f: f['t'] == 'delta' and any(
            item['key'] == self.key() and item['row'].get('prompt') == 'Synthetic request' for item in f['upsert']), timeout=8)
        self.assertEqual(delta['topic'], 'catalog')
        self.assertEqual(delta['remove'], [])
        renamed = 'codex/input-test/renamed'
        result = subprocess.run([str(HGS), 'rename', self.name, renamed], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        delta = stream.next(lambda f: f['t'] == 'delta' and any(
            item['key'] == self.key() and item['row']['name'] == renamed for item in f['upsert']), timeout=8)
        self.assertEqual(delta['remove'], [])
        # Let the rename settle; then observation timestamps alone, which
        # change on every listing, produce no delta across a safety listing.
        stream.quiet(lambda f: False, 1.5)
        stream.quiet(lambda f: f['t'] == 'delta', 6)
        self.assertEqual(stream.close(), 0)

    def test_a_forgotten_session_is_removed_with_its_reason_and_ends_its_subscription(self):
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='activity', session=self.session())
        stream.next(lambda f: f['t'] == 'state')
        result = subprocess.run([str(HGS), 'kill', self.name], env=self.env, text=True,
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        delta = stream.next(lambda f: f['t'] == 'delta' and f['remove'], timeout=10)
        self.assertEqual(delta['remove'], [dict(key=self.key(), reason='forgotten')])
        ended = stream.next(lambda f: f['t'] == 'error')
        self.assertEqual((ended['code'], ended['topic'], ended['key'], ended['fatal']),
                         ('subscription_ended', 'activity', self.key(), False))
        self.assertIn('forgotten', ended['message'])
        self.assertEqual(stream.close(), 0)

    def test_activity_streams_new_journal_rows_and_summaries_until_unsubscribed(self):
        first = self.journal('UserPromptSubmit', detail='Synthetic prompt')
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='activity', session=self.session())
        initial = stream.next(lambda f: f['t'] == 'events')
        self.assertEqual((initial['topic'], initial['key'], initial['cursor']), ('activity', self.key(), first))
        self.assertEqual(initial['events'], self.inspect_events(0)[-len(initial['events']):])
        state = stream.next(lambda f: f['t'] == 'state')
        self.assertEqual((state['topic'], state['key'], state['rev']), ('activity', self.key(), 1))
        self.assertEqual(state['value']['name'], self.name)
        for section in ('events', 'processes', 'cursor'):
            self.assertNotIn(section, state['value'])
        reply = self.journal('Stop', detail='Synthetic reply')
        events = stream.next(lambda f: f['t'] == 'events' and f['cursor'] == reply, timeout=5)
        self.assertEqual(events['events'], self.inspect_events(first))
        self.assertEqual([event['seq'] for event in events['events']], [reply])
        stream.send(op='ping', id='after-reply')
        self.assertEqual(stream.next(lambda f: f['t'] == 'pong')['id'], 'after-reply')
        stream.send(op='unsubscribe', topic='activity', session=self.session())
        stream.send(op='ping', id='unsubscribed')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'unsubscribed')
        self.journal('Stop', detail='Unwatched reply')
        stream.quiet(lambda f: f['t'] in ('events', 'state'), 2)
        self.assertEqual(stream.close(), 0)

    def test_resume_after_a_cursor_and_gap_below_retention(self):
        rows = [self.journal('Stop', detail=f'reply {n}') for n in range(5)]
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='activity', session=self.session(), after=rows[2])
        resumed = stream.next(lambda f: f['t'] == 'events')
        self.assertEqual([event['seq'] for event in resumed['events']], rows[3:])
        self.assertEqual(resumed['cursor'], rows[-1])
        # Retention removed rows after the client's cursor.
        db = sqlite3.connect(self.state / 'events.sqlite3')
        with db:
            db.execute('DELETE FROM events WHERE seq <= ?', (rows[2],))
        db.close()
        stream.send(op='subscribe', topic='activity', session=self.session(), after=rows[0], tail=1)
        gap = stream.next(lambda f: f['t'] == 'gap')
        self.assertEqual((gap['topic'], gap['key'], gap['reason']), ('activity', self.key(), 'retention'))
        tail = stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'activity')
        self.assertEqual([event['seq'] for event in tail['items']], rows[-1:])
        self.assertEqual((tail['key'], tail['cursor'], tail['last']), (self.key(), rows[-1], True))
        # A cursor beyond the journal belongs to a replaced journal.
        stream.send(op='subscribe', topic='activity', session=self.session(), after=rows[-1] + 1000)
        self.assertEqual(stream.next(lambda f: f['t'] == 'gap')['reason'], 'reset')
        self.assertEqual(stream.close(), 0)

    def test_subscription_limit_unknown_topics_and_bad_requests_are_errors(self):
        stream, *_ = self.start()
        for index in range(16):
            stream.send(op='subscribe', topic='activity', session=dict(name=f'sh/limit-{index}'), tail=0)
        stream.send(op='subscribe', topic='activity', session=dict(name='sh/limit-16'))
        error = stream.next(lambda f: f['t'] == 'error', timeout=30)
        self.assertEqual((error['code'], error['key'], error['fatal']), ('too_many_subscriptions', 'name:sh/limit-16', False))
        self.assertEqual(sum(1 for f in stream.seen if f['t'] == 'events'), 16)
        stream.send(op='subscribe', topic='processes', session=self.session())
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'unsupported_topic')
        stream.process.stdin.write('{"op":"launch"}\nnot json\n')
        stream.process.stdin.flush()
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        stream.send(op='subscribe', topic='activity')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        self.assertEqual(stream.close(), 0)

    def test_closed_output_ends_the_stream_quietly(self):
        process = subprocess.Popen([str(HGS), 'watch', '--json'], env=self.env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        process.stdout.readline()
        process.stdout.close()
        # The next frame (snapshot, meta or a pong) meets a closed pipe.
        try:
            process.stdin.write(b'{"op":"ping"}\n' * 3)
            process.stdin.flush()
        except BrokenPipeError:
            pass
        self.assertEqual(process.wait(timeout=10), 0)
        self.assertEqual(process.stderr.read(), b'')
        process.stdin.close()
        process.stderr.close()

    def test_usage_marker_and_dry_run(self):
        result = subprocess.run([str(HGS), '-h'], env=self.env, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0)
        self.assertIn('\n       Event stream ABI: watch-v1\n', result.stdout)
        self.assertIn('hgs [@host] watch --json   long-lived NDJSON event stream (stdin control)', result.stdout)
        result = subprocess.run([str(HGS), 'watch'], env=self.env, text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn('usage: hgs [@host] watch --json', result.stderr)
        result = subprocess.run([str(HGS), '--dry-run', '@remote', 'watch', '--json'], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('-o ServerAliveInterval=10 -o ServerAliveCountMax=3', result.stdout)
        self.assertIn('grep -qF', result.stdout)
        self.assertIn('Event stream ABI: watch-v1', result.stdout)
        self.assertIn('then exec ~/.local/bin/hgs watch --json', result.stdout)

    def test_old_peer_is_never_launched_and_new_peer_streams_through_ssh(self):
        ssh_log = self.root / 'ssh-args'
        self.script('ssh', '#!/bin/sh\nprintf "%s\\n" "$@" > "' + str(ssh_log) + '"\n'
                    'for argument do last="$argument"; done\nexec /bin/sh -c "$last"\n')
        peer = self.root / '.local/bin/hgs'
        peer.unlink()
        launched = self.root / 'old-peer-launched'
        peer.write_text('#!/bin/sh\nif [ "$1" = --help ]; then echo "usage: hgs [@host] <cmd>"; exit 0; fi\n'
                        'touch "' + str(launched) + '"\n')
        peer.chmod(0o755)
        old = subprocess.run([str(HGS), '@remote', 'watch', '--json'], env=self.env, input='',
                             text=True, capture_output=True, timeout=10)
        self.assertEqual(old.returncode, 1)
        self.assertIn('update the selected computer to stream events', old.stderr)
        self.assertFalse(launched.exists())
        self.assertEqual(old.stdout, '')
        peer.unlink()
        peer.symlink_to(HGS)
        stream = Stream(self, '@remote')
        hello = stream.next()
        self.assertEqual(hello['t'], 'hello')
        stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'catalog' and f['last'])
        stream.send(op='ping', id='peer')
        self.assertEqual(stream.next(lambda f: f['t'] == 'pong')['id'], 'peer')
        self.assertEqual(stream.close(), 0)
        arguments = ssh_log.read_text().splitlines()
        for option in ('ServerAliveInterval=10', 'ServerAliveCountMax=3', 'BatchMode=yes'):
            self.assertIn(option, arguments)
        self.assertNotIn('-t', arguments)


if __name__ == '__main__':
    unittest.main()
