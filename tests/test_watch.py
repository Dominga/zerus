"""`hgs [@host] watch --json`: the NDJSON event stream (watch-v1).

A private tmux socket, a scripted agent and a synthetic state folder only.
"""
import hashlib
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

    def journal(self, kind, name=None, **fields):
        """Appends one Activity row as a hook would; returns its seq."""
        payload = dict(type=kind, at=time.time(), agent_id='', tool='', detail='')
        payload.update(fields)
        db = sqlite3.connect(self.state / 'events.sqlite3')
        with db:
            db.execute('CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT)')
            cursor = db.execute('INSERT INTO events(name, conversation, payload) VALUES (?, ?, ?)',
                                (name or self.name, self.conversation_id, json.dumps(payload)))
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
        self.assertEqual(hello['topics'], ['catalog', 'catalog.meta', 'activity', 'git', 'processes', 'tasks', 'organization'])
        self.assertEqual(hello['limits']['activity_subscriptions'], 16)
        self.assertEqual(hello['limits']['git_subscriptions'], 64)
        self.assertEqual(hello['limits']['processes_subscriptions'], 16)
        self.assertEqual(hello['limits']['tasks_subscriptions'], 8)
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

    def listed_row(self):
        return next(row for row in self.listing()['sessions'] if row['name'] == self.name)

    def upserted(self, predicate, timeout):
        """The row of the first delta that upserts this session as `predicate` wants."""
        delta = self.stream.next(lambda f: f['t'] == 'delta' and any(
            item['key'] == self.key() and predicate(item['row']) for item in f['upsert']), timeout=timeout)
        return next(item['row'] for item in delta['upsert'] if item['key'] == self.key())

    def test_a_changed_binding_or_journal_row_derives_the_row_the_listing_shows(self):
        # Between whole listings (every 5 s) only the changed session's row is
        # derived again, through the listing's own steps.
        self.stream, *_ = self.start()
        self.record.update(prompt='Derived request', last_event_at=time.time())
        self.write_record()
        row = self.upserted(lambda row: row.get('prompt') == 'Derived request', timeout=4)
        self.assertEqual(steady(row), steady(self.listed_row()))
        # A journal row alone, such as a reply, changes the reply identity.
        reply = self.journal('Stop', detail='Derived reply')
        row = self.upserted(lambda row: str(row.get('reply_id', '')).startswith(f'{reply}:'), timeout=4)
        self.assertEqual(steady(row), steady(self.listed_row()))
        self.assertEqual(self.stream.close(), 0)

    def test_a_session_killed_in_tmux_is_noticed_without_any_file_change(self):
        self.stream, *_ = self.start()
        self.tmux('kill-session', '-t', '=' + self.name)
        # The probe sees the terminal change within 5 s and lists it whole.
        row = self.upserted(lambda row: row.get('state') == 'stopped', timeout=8)
        self.assertEqual(steady(row), steady(self.listed_row()))
        self.assertEqual(self.stream.close(), 0)

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

    def transcript_line(self, text, phase='commentary', at='2026-10-27T12:00:00Z'):
        return json.dumps(dict(type='response_item', timestamp=at, payload=dict(
            type='message', role='assistant', phase=phase, content=[dict(type='output_text', text=text)]))) + '\n'

    def test_messages_between_tools_stream_as_event_frames_from_the_transcript(self):
        transcript = (self.root / f'rollout-{self.conversation_id}.jsonl').resolve()
        transcript.write_text(json.dumps(dict(type='session_meta', payload=dict(id=self.conversation_id))) + '\n'
                              + self.transcript_line('Before the stream.'))
        self.record.update(transcript=str(transcript))
        self.write_record()
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='activity', session=self.session())
        state = stream.next(lambda f: f['t'] == 'state')
        self.assertEqual([m['detail'] for m in state['value']['provider_messages']], ['Before the stream.'])
        with transcript.open('a') as file:
            file.write(self.transcript_line('Checking the deployment.', at='2026-10-27T12:00:05Z'))
        started = time.monotonic()
        event = stream.next(lambda f: f['t'] == 'event', timeout=5)
        self.assertLess(time.monotonic() - started, 2)
        self.assertEqual((event['topic'], event['key'], event['rev']), ('activity', self.key(), 1))
        inspected = subprocess.run([str(HGS), 'inspect', self.name, '--skip-processes'], env=self.env,
                                   text=True, capture_output=True, timeout=10)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        listed = json.loads(inspected.stdout)['provider_messages']
        self.assertEqual(event['event'], listed[-1])
        self.assertEqual(event['event']['detail'], 'Checking the deployment.')
        self.assertEqual(event['id'], f"AgentMessage@{event['event']['at']}#"
                         + hashlib.sha256(b'Checking the deployment.').hexdigest()[:16])
        # Private reasoning and a repeat of the same message send nothing.
        with transcript.open('a') as file:
            file.write(self.transcript_line('PRIVATE_REASONING', phase='analysis'))
            file.write(self.transcript_line('Checking the deployment.', at='2026-10-27T12:00:06Z'))
        stream.quiet(lambda f: f['t'] == 'event', 1.5)
        self.assertNotIn('PRIVATE_REASONING', ''.join(stream.raw))
        self.assertEqual(stream.close(), 0)

    def test_processes_state_matches_inspect_and_follows_the_binding(self):
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='processes', session=self.session())
        state = stream.next(lambda f: f['t'] == 'state' and f['topic'] == 'processes', timeout=10)
        self.assertEqual((state['key'], state['rev']), (self.key(), 1))
        inspected = subprocess.run([str(HGS), 'inspect', self.name], env=self.env, text=True, capture_output=True, timeout=10)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        inspection = json.loads(inspected.stdout)
        def steady_value(value):
            return {key: {k: v for k, v in value[key].items() if k != 'sampled_at'} for key in ('processes', 'process_work')}
        self.assertEqual(set(state['value']), {'processes', 'process_work'})
        self.assertEqual(steady_value(state['value']), steady_value(inspection))
        # A pre-extension client can still resubscribe to recover its first state.
        stream.send(op='subscribe', topic='processes', session=self.session())
        again = stream.next(lambda f: f['t'] == 'state' and f['topic'] == 'processes', timeout=10)
        self.assertEqual(again['rev'], 1)
        # Reconfigure and refresh this subscription without dropping the last
        # inventory or resetting its revision. Older servers reset and resend 1.
        stream.send(op='subscribe', topic='processes', session=self.session(),
                    interval_ms=2500, refresh=True)
        stream.send(op='ping', id='processes-configured')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'processes-configured')
        stream.quiet(lambda f: f['t'] == 'state' and f['topic'] == 'processes' and f['rev'] == 1, .25)
        # A recorded background job in the binding is read again and sent.
        self.record['shell_jobs'] = {'job-1': dict(id='job-1', status='running', background=True, observed=True,
                                                   run_id=self.run_id, command='sleep 600', started_at=time.time(), source='hooks')}
        self.record.update(activity='idle', phase='idle', last_event_at=time.time())
        self.write_record()
        changed = stream.next(lambda f: f['t'] == 'state' and f['topic'] == 'processes' and f['rev'] == 2, timeout=10)
        inspected = json.loads(subprocess.run([str(HGS), 'inspect', self.name], env=self.env, text=True,
                                              capture_output=True, timeout=10).stdout)
        self.assertEqual(steady_value(changed['value']), steady_value(inspected))
        stream.send(op='unsubscribe', topic='processes', session=self.session())
        stream.send(op='ping', id='unsubscribed')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'unsubscribed')
        self.record['shell_jobs'] = {}
        self.write_record()
        stream.quiet(lambda f: f['t'] == 'state' and f['topic'] == 'processes', 3)
        self.assertEqual(stream.close(), 0)

    def task(self, *args, text=None):
        result = subprocess.run([str(HGS), 'task', *args], input=text, env=self.env, text=True,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def tasks_snapshot(self, stream, predicate=lambda value: True, timeout=10):
        """The items and last-page value of the next whole tasks snapshot."""
        items = []
        while True:
            frame = stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'tasks', timeout=timeout)
            if frame['page'] == 0:
                items = []
            items += frame['items']
            if frame['last'] and predicate(frame['value']):
                return frame['key'], items, frame['value']

    def test_a_project_task_list_streams_as_whole_snapshots_with_a_digest(self):
        catalog = {"projects": [{"id": "project-1", "name": "orchard", "color": "#123456", "folders": [], "sessions": []}]}
        result = subprocess.run([str(HGS), 'swarm', 'initialize'], input=json.dumps(catalog), env=self.env,
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.task('add', '--project', 'orchard', text='First synthetic task')
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='tasks', project='project-1')
        key, items, value = self.tasks_snapshot(stream)
        listed = self.task('list', '--project', 'project-1')
        self.assertEqual(key, 'project-1')
        self.assertEqual(items, listed['tasks'])
        self.assertEqual((value['project'], value['damaged']), (listed['project'], listed['damaged']))
        self.assertRegex(value['digest'], '^[0-9a-f]{64}$')
        # A new task changes the store: the whole list again, with a new digest.
        self.task('add', '--project', 'orchard', text='Second synthetic task')
        _, items, changed = self.tasks_snapshot(stream, lambda v: v['digest'] != value['digest'], timeout=10)
        self.assertEqual(items, self.task('list', '--project', 'project-1')['tasks'])
        self.assertEqual(len(items), 2)
        # An unknown project is a non-fatal error of its key.
        stream.send(op='subscribe', topic='tasks', project='no-such-project')
        error = stream.next(lambda f: f['t'] == 'error' and f.get('topic') == 'tasks')
        self.assertEqual((error['code'], error['key'], error['fatal']), ('tasks_failed', 'no-such-project', False))
        stream.send(op='subscribe', topic='tasks')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error' and f['code'] == 'bad_request')['topic'], 'tasks')
        stream.send(op='unsubscribe', topic='tasks', project='project-1')
        stream.send(op='ping', id='unsubscribed')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'unsubscribed')
        self.task('add', '--project', 'orchard', text='Unwatched task')
        stream.quiet(lambda f: f['t'] == 'snapshot' and f['topic'] == 'tasks', 3)
        self.assertEqual(stream.close(), 0)

    @staticmethod
    def organization_rebuilt(rows):
        """The `swarm get` snapshot (without `written`) the organization rows encode."""
        snapshot = json.loads(json.dumps(rows['organization']))
        projects = []
        for project_id in snapshot['organization']['projects']:
            project = dict(rows['project:' + project_id])
            chunks = project.pop('session_chunks')
            project['sessions'] = [s for n in range(chunks) for s in rows[f'members:{project_id}:{n}']['sessions']]
            projects.append(project)
        snapshot['organization']['projects'] = projects
        snapshot['versions'] = {key[len('version:'):]: row for key, row in rows.items() if key.startswith('version:')}
        return snapshot

    def swarm(self, *args, payload=None):
        result = subprocess.run([str(HGS), 'swarm', *args], input=json.dumps(payload) if payload is not None else '',
                                env=self.env, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        value = json.loads(result.stdout)
        value.pop('written', None)
        return value

    def test_the_organization_streams_as_rows_with_deltas_of_what_changed(self):
        catalog = {"projects": [{"id": "project-1", "name": "orchard", "color": "#123456", "folders": [], "sessions": []}]}
        self.swarm('initialize', payload=catalog)
        before = self.swarm('get')
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='organization')
        rows = {}
        while True:
            frame = stream.next(lambda f: f['t'] == 'snapshot' and f['topic'] == 'organization', timeout=10)
            if frame['page'] == 0:
                rows = {}
            rows.update({item['key']: item['row'] for item in frame['items']})
            if frame['last']:
                break
        self.assertEqual(self.organization_rebuilt(rows), before)
        self.assertIn('project:project-1', rows)
        # A new project through the shared catalog's own edit: one delta of the
        # changed rows, after which the rows match `swarm get` again.
        desired = json.loads(json.dumps(before['organization']))
        desired['projects'].append({"id": "project-2", "name": "meadow", "color": "#654321", "folders": [], "sessions": []})
        self.swarm('apply', payload={'base': before['organization'], 'desired': desired, 'versions': before['versions']})
        delta = stream.next(lambda f: f['t'] == 'delta' and f['topic'] == 'organization', timeout=10)
        upserted = {item['key'] for item in delta['upsert']}
        self.assertIn('project:project-2', upserted)
        self.assertIn('organization', upserted)
        self.assertNotIn('project:project-1', upserted)
        self.assertGreater(delta['cursor'], frame['cursor'])
        rows.update({item['key']: item['row'] for item in delta['upsert']})
        for removal in delta['remove']:
            rows.pop(removal['key'], None)
        self.assertEqual(self.organization_rebuilt(rows), self.swarm('get'))
        # Unsubscribed, a change sends nothing.
        stream.send(op='unsubscribe', topic='organization')
        stream.send(op='ping', id='unsubscribed')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'unsubscribed')
        after = self.swarm('get')
        changed = json.loads(json.dumps(after['organization']))
        changed['projects'][0]['name'] = 'grove'
        self.swarm('apply', payload={'base': after['organization'], 'desired': changed, 'versions': after['versions']})
        stream.quiet(lambda f: f.get('topic') == 'organization', 3)
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
        stream.send(op='subscribe', topic='schedules', session=self.session())
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'unsupported_topic')
        stream.process.stdin.write('{"op":"launch"}\nnot json\n')
        stream.process.stdin.flush()
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        stream.send(op='subscribe', topic='activity')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        self.assertEqual(stream.close(), 0)

    # ---- git ----------------------------------------------------------------

    def git(self, *args):
        env = {key: value for key, value in self.env.items() if not key.startswith('GIT_')}
        env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        return subprocess.check_output(['git', *args], env=env, stderr=subprocess.PIPE, text=True).strip()

    def commit(self, repo, message):
        self.git('-C', str(repo), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test',
                 'commit', '-qam', message)

    def repository(self):
        """A temporary checkout with one commit; Git reads no user configuration."""
        self.env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        repo = (self.root / 'checkout').resolve()
        self.git('init', '-q', '-b', 'main', str(repo))
        (repo / 'tracked').write_text('initial\n')
        self.git('-C', str(repo), 'add', '.')
        self.commit(repo, 'initial')
        return repo

    def git_status(self, path):
        result = subprocess.run([str(HGS), 'git-status', '--path', str(path), '--json'], env=self.env,
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    @staticmethod
    def git_state(path, predicate=lambda value: True):
        return lambda f: f['t'] == 'state' and f['topic'] == 'git' and f['key'] == str(path) and predicate(f['value'])

    @staticmethod
    def git_states(stream, count, timeout):
        """The latest git state of each folder once `count` folders answered;
        checks finish in any order."""
        deadline = time.monotonic() + timeout
        while True:
            states = {f['key']: f['value'] for f in stream.seen if f['t'] == 'state' and f['topic'] == 'git'}
            if len(states) >= count:
                return states
            stream.next(lambda f: f['t'] == 'state' and f['topic'] == 'git',
                        timeout=max(0.1, deadline - time.monotonic()))

    def test_git_follows_commits_checkouts_and_session_edits_until_unsubscribed(self):
        repo = self.repository()
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='git', path=str(repo))
        first = stream.next(self.git_state(repo), timeout=10)
        volatile = ('sampled_at', 'remote_age_seconds')
        self.assertEqual({k: v for k, v in first['value'].items() if k not in volatile},
                         {k: v for k, v in self.git_status(repo).items() if k not in volatile})
        self.assertEqual((first['value']['state'], first['value']['branch'], first['value']['changed_files']), ('ok', 'main', 0))
        # A commit changes HEAD's branch ref and the index; a stat sees it.
        (repo / 'tracked').write_text('second\n')
        started = time.monotonic()
        self.commit(repo, 'second')
        committed = stream.next(self.git_state(repo, lambda v: v['head'] != first['value']['head']), timeout=5)
        self.assertEqual(committed['value']['head'], self.git('-C', str(repo), 'rev-parse', 'HEAD'))
        self.assertGreater(committed['rev'], first['rev'])
        self.git('-C', str(repo), 'checkout', '-qb', 'feature')
        stream.next(self.git_state(repo, lambda v: v['branch'] == 'feature'), timeout=5)
        # A check that a late metadata write asked for runs within a second.
        stream.quiet(lambda f: False, 1.5)
        # An edit in the working tree changes no Git metadata. A session working
        # in the checkout that finishes a file-changing tool makes the stream
        # check it, long before the 30 s safety tick. A saved session keeps
        # the folder its hooks reported (a live one shows its pane's folder).
        name = 'codex/git-fixture'
        saved = dict(version=1, name=name, agent='codex', run_id=str(uuid.uuid4()),
                     conversation_id=str(uuid.uuid4()), cwd=str(repo), activity='idle', phase='idle',
                     active_tools={}, subagents={}, last_event_at=time.time(), created=time.time())
        (self.state / (hashlib.sha256(name.encode()).hexdigest() + '.json')).write_text(json.dumps(saved))
        stream.next(lambda f: f['t'] == 'delta' and any(
            item['row'].get('name') == name and item['row'].get('cwd') == str(repo) for item in f['upsert']), timeout=8)
        (repo / 'tracked').write_text('edited by the agent\n')
        stream.quiet(self.git_state(repo, lambda v: v['changed_files'] == 1), 1.5)
        self.journal('PostToolUse', name=name, tool='Edit', detail=str(repo / 'tracked'))
        edited = stream.next(self.git_state(repo, lambda v: v['changed_files'] == 1), timeout=5)
        self.assertEqual(edited['value']['branch'], 'feature')
        self.assertLess(time.monotonic() - started, 25)
        # An unchanged checkout sends nothing more, and unsubscribed nothing at all.
        stream.quiet(self.git_state(repo), 2)
        stream.send(op='unsubscribe', topic='git', path=str(repo))
        stream.send(op='ping', id='unsubscribed')
        stream.next(lambda f: f['t'] == 'pong' and f['id'] == 'unsubscribed')
        self.git('-C', str(repo), 'checkout', '-q', 'main')
        stream.quiet(lambda f: f['t'] == 'state' and f['topic'] == 'git', 2.5)
        self.assertTrue(all(len(line.encode()) <= 1 << 20 for line in stream.raw))
        self.assertEqual(stream.close(), 0)

    def test_git_folders_answer_like_git_status_and_are_limited(self):
        self.env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        plain = self.root / 'plain'
        plain.mkdir()
        missing = self.root / 'missing'
        stream, *_ = self.start()
        stream.send(op='subscribe', topic='git', path=str(plain))
        stream.send(op='subscribe', topic='git', path=str(missing))
        states = self.git_states(stream, 2, timeout=10)
        self.assertEqual((states[str(plain)]['state'], states[str(missing)]['state']), ('not_repo', 'folder_unavailable'))
        for path in ('relative/folder', ''):
            stream.send(op='subscribe', topic='git', path=path)
            error = stream.next(lambda f: f['t'] == 'error')
            self.assertEqual((error['code'], error['topic'], error['fatal']), ('bad_request', 'git', False))
        stream.send(op='subscribe', topic='git')
        self.assertEqual(stream.next(lambda f: f['t'] == 'error')['code'], 'bad_request')
        for index in range(62):
            stream.send(op='subscribe', topic='git', path=str(self.root / f'limit-{index}'))
        stream.send(op='subscribe', topic='git', path=str(self.root / 'limit-62'))
        error = stream.next(lambda f: f['t'] == 'error', timeout=30)
        self.assertEqual((error['code'], error['topic'], error['key'], error['fatal']),
                         ('too_many_subscriptions', 'git', str(self.root / 'limit-62'), False))
        # Every accepted folder answers, and the stream stays up.
        states = self.git_states(stream, 64, timeout=30)
        self.assertNotIn(str(self.root / 'limit-62'), states)
        stream.send(op='ping', id='alive')
        self.assertEqual(stream.next(lambda f: f['t'] == 'pong')['id'], 'alive')
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
