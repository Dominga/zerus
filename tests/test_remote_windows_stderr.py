"""Portable call-site controls: no shell, process, transport or native agent."""
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import test_remote_windows as remote


class ReachedReadinessWait(Exception):
    pass


class DecoderStderr(unittest.TestCase):
    def exercise(self, *, exited=False):
        with tempfile.TemporaryDirectory(prefix='decoder-stderr-') as name:
            root = Path(name)
            fake = root/'fake-agent-data'
            fake.touch()  # Only copied; Popen below is always a model.
            child = mock.Mock()
            child.stdin, child.stdout, child.stderr = io.BytesIO(), io.BytesIO(), io.BytesIO()
            child.returncode = 7 if exited else None
            child.poll.side_effect = lambda: child.returncode
            child.wait.side_effect = lambda **_: setattr(child, 'returncode', 0)
            readers = []

            class Capture:
                def __init__(self, *, target, args):
                    self.pipe, self.path = args
                    readers.append(self)

                def start(self):
                    # Hold capture-file publication until after the first live
                    # poll. The exit case has a real diagnostic to preserve.
                    if exited:
                        self.path.write_bytes(b'original helper exited' if self.path.name == 'stderr' else b'')

                def join(self, **_):
                    self.pipe.close()

            def readiness_turn(_seconds):
                self.assertFalse(readers[1].path.exists())
                # End this control before any body/EOF is sent. The original
                # finally sees an exited model and must not use its fallback.
                child.returncode = 0
                raise ReachedReadinessWait

            fixture = remote.WindowsNativeTransport()
            with mock.patch.dict(os.environ, {'HGS_TEST_FAKE_AGENT': str(fake), 'SystemRoot': str(root)}), \
                    mock.patch.object(remote.subprocess, 'Popen', return_value=child) as launch, \
                    mock.patch.object(remote.subprocess, 'CREATE_NO_WINDOW', 0, create=True), \
                    mock.patch('threading.Thread', Capture), \
                    mock.patch.object(remote.time, 'sleep', side_effect=readiness_turn):
                try:
                    if exited:
                        with self.assertRaisesRegex(AssertionError, 'original helper exited'):
                            fixture.run_decoder('powershell')
                    else:
                        try:
                            fixture.run_decoder('powershell')
                        except ReachedReadinessWait:
                            pass
                        except (FileNotFoundError, AssertionError) as error:
                            self.fail('live-child observation eagerly read an unpublished stderr file: ' + str(error))
                        else:
                            self.fail('the maintained readiness wait was not reached')
                    self.assertEqual(launch.call_count, 1)
                    self.assertEqual(child.poll.call_count, 2)  # One observation and original finally.
                    child.wait.assert_not_called()
                finally:
                    for pipe in (child.stdin, child.stdout, child.stderr):
                        pipe.close()

    def test_live_original_does_not_read_stderr_before_capture_publishes_it(self):
        self.exercise()

    def test_original_exit_still_fails_with_its_captured_stderr(self):
        self.exercise(exited=True)


class DecoderReadinessFailure(unittest.TestCase):
    def test_readiness_deadline_keeps_its_primary_failure_and_reports_each_condition(self):
        import contextlib
        with tempfile.TemporaryDirectory(prefix='decoder-deadline-') as name:
            fake = Path(name) / 'fake-agent-data'
            fake.touch()
            child = mock.Mock()
            child.pid = 123
            child.returncode = None
            child.stdin, child.stdout, child.stderr = io.BytesIO(), io.BytesIO(), io.BytesIO()
            child.poll.side_effect = lambda: child.returncode
            child.wait.side_effect = lambda **_: setattr(child, 'returncode', 7)
            reader_fd, unused = os.pipe()
            os.close(unused)
            writer = io.BytesIO()
            captures = []

            class Capture:
                def __init__(self, *, target, args):
                    self.path = args[1]
                    captures.append(self)

                def start(self):
                    pass  # The original capture has not published its file.

                def join(self, **_):
                    pass

                def is_alive(self):
                    return True

            def launch(*args, **kwargs):
                root = kwargs['cwd']
                (root/'record.jsonl').write_bytes(b'x'*20000+b'\n{"kind":"start","pid":123}\n')
                return child

            ticks = iter([0.0])
            clock = lambda: next(ticks, 26.0)
            log = io.StringIO()
            fixture = remote.WindowsNativeTransport()
            with mock.patch.dict(os.environ, {'HGS_TEST_FAKE_AGENT':str(fake), 'SystemRoot':str(Path(name))}), \
                    mock.patch.object(remote.subprocess, 'Popen', side_effect=launch), \
                    mock.patch.object(remote.subprocess, 'CREATE_NO_WINDOW', 0, create=True), \
                    mock.patch.object(remote, 'async_stdin_pipe', return_value=(reader_fd, writer)), \
                    mock.patch('threading.Thread', Capture), \
                    mock.patch.object(remote.time, 'monotonic', side_effect=clock), \
                    contextlib.redirect_stderr(log):
                with self.assertRaisesRegex(AssertionError, 'original fake did not become ready') as failure:
                    fixture.run_decoder('powershell', sustained=True)
            self.assertIn('original decoder required fallback', '\n'.join(failure.exception.__notes__))
            lines = [line.removeprefix('WINDOWS_DECODER_FAILURE ') for line in log.getvalue().splitlines()
                     if line.startswith('WINDOWS_DECODER_FAILURE ')]
            self.assertEqual(len(lines), 1, log.getvalue())
            import json
            detail = json.loads(lines[0])
            self.assertEqual(detail['shell'], 'powershell')
            self.assertTrue(detail['sustained'])
            self.assertEqual(detail['budget_ms'], 8000)
            self.assertEqual(detail['stage'], 'startup')
            self.assertFalse(detail['marker_observed'])
            self.assertGreaterEqual(detail['elapsed_ms'], detail['budget_ms'])
            self.assertIsNone(detail['child_returncode'])
            self.assertFalse(detail['stdin_closed'])
            self.assertEqual(detail['renewals'], 0)
            self.assertEqual(detail['readers_alive'], [True, True])
            self.assertFalse(detail['files']['ready.json']['exists'])
            self.assertFalse(detail['files']['stderr']['exists'])
            self.assertEqual(detail['files']['record.jsonl']['size'], 20028)
            self.assertLessEqual(len(detail['files']['record.jsonl']['tail'].encode()), 4096)
            self.assertIn('"kind":"start"', detail['files']['record.jsonl']['tail'])
            self.assertTrue(writer.closed)
            child.wait.assert_called_once_with(timeout=18)


class DecoderStages(unittest.TestCase):
    def test_record_observation_refuses_an_oversized_fixture(self):
        with tempfile.TemporaryDirectory(prefix='decoder-record-') as name:
            path = Path(name)/'record.jsonl'
            path.write_bytes(b'x'*65537)
            with self.assertRaisesRegex(AssertionError, 'exceeded its bound'):
                remote.decoder_rows(path)

    def exercise(self, *, startup=7, partial_ready=False, partial_marker=False, renew=True):
        import contextlib
        import json
        import struct
        with tempfile.TemporaryDirectory(prefix='decoder-stages-') as name:
            fake = Path(name)/'fake-agent-data'
            fake.touch()
            child = mock.Mock()
            child.pid, child.returncode = 123, None
            child.stdout, child.stderr = io.BytesIO(), io.BytesIO()
            words = ['', 'a"b', 'back\\', 'two\\\\tail', '日本語', 'Пример', '$(literal);& data', "it's a word"]
            marker_delay = .5 if partial_marker is True else float(partial_marker)

            class Model:
                now, last_renew = 0.0, 0.0
                root = None
                render_at = None
                ready_complete = False
                marker_complete = False
                marker_at = None
                rendered = False
                eof = False
                frames = []
                events = []
                stdin_buffer = b''
                pipe_capacity = 4096

                def row(self, value):
                    with (self.root/'record.jsonl').open('a', encoding='utf-8') as file:
                        file.write(json.dumps(value)+'\n')

                def delivered(self):
                    if not self.stdin_buffer:
                        return
                    body, self.stdin_buffer = self.stdin_buffer, b''
                    self.frames.append(body)
                    row = json.dumps(dict(hex=body.hex()))
                    with (self.root/'record.jsonl').open('a', encoding='utf-8') as file:
                        file.write(row+('' if partial_marker and len(self.frames) == 1 else '\n'))
                    if len(self.frames) == 1:
                        self.events.append(('marker', self.now))
                        self.marker_at = self.now
                        self.marker_complete = not partial_marker

                def update(self):
                    if self.root is None or child.returncode is not None:
                        return
                    if self.now-self.last_renew > 15:
                        child.returncode = 127
                        (self.root/'stderr').write_bytes(b'hgs: remote Windows invocation failed\n')
                        return
                    ready_at = startup+(19 if self.preloaded else 0)
                    if self.now >= ready_at and not self.ready_complete:
                        if partial_ready and self.now < ready_at+.5:
                            (self.root/'ready.json').write_text('{"argv":', encoding='utf-8')
                        else:
                            (self.root/'ready.json').write_text(json.dumps(dict(argv=words)), encoding='utf-8')
                            self.ready_complete = True
                    if self.marker_at is not None and not self.marker_complete and self.now >= self.marker_at+marker_delay:
                        with (self.root/'record.jsonl').open('a', encoding='utf-8') as file:
                            file.write('\n')
                        self.marker_complete = True
                    control = self.root/'control'
                    if control.exists() and control.read_text() == 'render render\n':
                        assert self.frames and self.marker_complete, 'render preceded the complete input marker record'
                        self.render_at = self.now
                        self.events.append(('render', self.now))
                        control.unlink()
                    if self.render_at is not None and self.now-self.render_at >= 19 and not self.rendered:
                        self.row(dict(kind='render', bytes=1280, steps=20))
                        (self.root/'stdout').write_bytes((self.root/'render').read_bytes())
                        self.rendered = True

                def sleep(self, _seconds):
                    self.now += .25
                    self.update()

            model = Model()

            class Input(io.BytesIO):
                pending = b''

                def write(self, data):
                    if not model.frames and len(data) > 4:
                        assert len(data)+4 <= model.pipe_capacity, 'marker can block before startup deadline checks'
                    self.pending += data
                    while len(self.pending) >= 4:
                        length = struct.unpack('<I', self.pending[:4])[0]
                        if length == 0xffffffff:
                            self.pending = self.pending[4:]
                            model.last_renew = model.now
                            model.events.append(('ping', model.now))
                        elif length == 0:
                            self.pending = self.pending[4:]
                            model.eof = True
                            model.delivered()
                        elif len(self.pending) >= length+4:
                            body, self.pending = self.pending[4:4+length], self.pending[4+length:]
                            model.last_renew = model.now
                            # .NET Framework's synchronous redirected stdin
                            # buffers small writes until a full buffer or EOF.
                            model.stdin_buffer += body
                            if len(model.stdin_buffer) >= 4096:
                                model.delivered()
                        else:
                            break
                    return len(data)

            writer = Input()
            child.stdin = writer
            reader_fd, unused = os.pipe()
            os.close(unused)

            def pipe(buffer_size=4096):
                model.pipe_capacity = buffer_size
                return reader_fd, writer

            def launch(*args, **kwargs):
                model.root = kwargs['cwd']
                model.preloaded = 'FAKE_AGENT_RENDER' in kwargs['env']
                if model.preloaded:
                    model.render_at = startup
                (model.root/'record.jsonl').touch()
                return child

            def poll():
                model.update()
                return child.returncode

            def wait(**_):
                if child.returncode is not None:
                    return child.returncode
                if not model.eof:
                    self.assertEqual((model.root/'control').read_text(), 'exit 7\n')
                child.returncode = 0 if model.eof else 7

            child.poll.side_effect, child.wait.side_effect = poll, wait

            class Capture:
                def __init__(self, *, target, args):
                    self.pipe, self.path = args
                def start(self):
                    self.path.touch()
                def join(self, **_):
                    self.pipe.close()
                def is_alive(self):
                    return child.returncode is None

            log = io.StringIO()
            with mock.patch.dict(os.environ, {'HGS_TEST_FAKE_AGENT':str(fake), 'SystemRoot':name}), \
                    mock.patch.object(remote.subprocess, 'Popen', side_effect=launch), \
                    mock.patch.object(remote.subprocess, 'CREATE_NO_WINDOW', 0, create=True), \
                    mock.patch.object(remote, 'async_stdin_pipe', side_effect=pipe), \
                    mock.patch('threading.Thread', Capture), \
                    mock.patch.object(remote.time, 'monotonic', side_effect=lambda: model.now), \
                    mock.patch.object(remote.time, 'sleep', side_effect=model.sleep), \
                    contextlib.redirect_stderr(log):
                kwargs = {} if renew else dict(renew=False)
                result = remote.WindowsNativeTransport().run_decoder('powershell', sustained=True, **kwargs)
            self.assertTrue(writer.closed)
            self.assertEqual(child.returncode, 0 if renew else 127)
            return result, model

    def test_seven_second_startup_does_not_spend_the_sustained_render_allowance(self):
        result, model = self.exercise()
        self.assertGreaterEqual(result['startup_ms'], 7000)
        self.assertLess(result['startup_ms'], 8000)
        self.assertGreaterEqual(result['render_ms'], 19000)
        self.assertGreater(model.now, 25)
        marker = next(at for event, at in model.events if event == 'marker')
        render = next(at for event, at in model.events if event == 'render')
        self.assertFalse(any(marker < at < render for event, at in model.events if event == 'ping'))

    def test_partial_ready_publication_stays_inside_the_same_startup_deadline(self):
        result, _ = self.exercise(partial_ready=True)
        self.assertEqual(result['startup_ms'], 7500)

    def test_render_waits_for_the_complete_marker_record(self):
        result, _ = self.exercise(partial_marker=True)
        self.assertEqual(result['startup_ms'], 7500)

    def test_startup_still_refuses_after_eight_seconds(self):
        with self.assertRaisesRegex(AssertionError, 'original fake did not become ready'):
            self.exercise(startup=9)

    def test_no_renewal_mutant_expires_before_the_render_can_complete(self):
        result, model = self.exercise(renew=False)
        self.assertTrue(result['lease_expired'])
        self.assertEqual(result['returncode'], 127)
        self.assertGreaterEqual(result['lease_elapsed_ms'], 14000)
        self.assertEqual(result['renewals'], 0)
        self.assertFalse(model.rendered)
        self.assertFalse(model.eof)

    def test_marker_observation_delay_does_not_shorten_the_lease_measurement(self):
        result, _ = self.exercise(startup=0, partial_marker=2, renew=False)
        self.assertLess(result['render_ms'], 14000)
        self.assertGreaterEqual(result['lease_elapsed_ms'], 14000)
