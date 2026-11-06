"""Private Linux own-helper integration; no installed GUI or owner processes.

Real canonical native parent, sealed original child, Root proof, live native
loan and guarded Git engine. Cached native evidence is supplied by a fixture;
this is not installed GUI confirmation or Mac/Windows acceptance.
"""
import hashlib
import fcntl
import json
import os
from pathlib import Path
import select
import sqlite3
import shutil
import subprocess
import sys
import time
import unittest
import uuid

import test_pause
import test_worktree_lifecycle as lifecycle


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(compact(value).encode()).hexdigest()


@unittest.skipUnless(sys.platform == 'linux' and test_pause.TMUX, 'Linux native private process scope')
class ProgramExecution(test_pause.Harness):
    git = lifecycle.WorktreeLifecycle.git
    commit = lifecycle.WorktreeLifecycle.commit
    program_capture = lifecycle.WorktreeLifecycle.program_capture
    settle_program = staticmethod(lifecycle.WorktreeLifecycle.settle_program)

    def setUp(self):
        super().setUp()
        # Own-helper observation uses the existing private receipt contract.
        (self.root / 'state').mkdir(mode=0o700)
        self.repo = self.project
        self.git('init', '-q', '-b', 'main', str(self.repo))
        (self.repo / 'tracked').write_text('original')
        self.git('-C', str(self.repo), 'add', 'tracked')
        self.commit(self.repo, 'original')
        self.project = self.root / 'managed'
        self.hgs('worktrees', 'create', '--path', str(self.repo), '--destination', str(self.project),
                 '--branch', 'session-work', '--json')
        (self.root / 'cfg/projects.local').write_text('p=' + str(self.project) + '\n')
        self.frontend = self.bin / 'hgs-tray'
        source = Path(__file__).parent / 'fixtures/desktop_frontend.c'
        subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(self.frontend)],
                       check=True, capture_output=True, timeout=30)
        self.frontend.chmod(0o700)
        self.image = self.root / 'selected-hgs'
        shutil.copyfile(test_pause.HGS, self.image)
        self.image.chmod(0o700)
        subprocess.run(['strip', '--strip-debug', str(self.image)], check=True, capture_output=True, timeout=30)
        self.assertLess(self.image.stat().st_size, 256 << 20)
        self.sealed = os.memfd_create('zerus-helper-image', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING | 0x10)
        self.addCleanup(os.close, self.sealed)
        with self.image.open('rb') as source:
            while block := source.read(65536):
                view = memoryview(block)
                while view:
                    view = view[os.write(self.sealed, view):]
        fcntl.fcntl(self.sealed, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        (self.root / 'tmp').mkdir(exist_ok=True)
        self.env['TMPDIR'] = str(self.root / 'tmp')
        self.holder = None
        self.permissions = []

    def tearDown(self):
        # Restore only permission changes introduced by this private fixture,
        # BEFORE Harness removes its own directory (unittest cleanups run later).
        for directory in self.permissions:
            if directory.exists():
                directory.chmod(0o700)
        super().tearDown()

    def frame(self):
        value = bytearray()
        until = time.monotonic() + 10
        while not value.endswith(b'\n'):
            self.assertLess(len(value), 128 * 1024 + 4096)
            self.assertTrue(select.select([self.holder.stdout], [], [], max(0, until - time.monotonic()))[0], 'original holder frame')
            part = os.read(self.holder.stdout.fileno(), 1)
            if not part:
                self.fail('original holder closed: ' + self.holder.stderr.read(4096).decode(errors='replace'))
            value.extend(part)
        return json.loads(value)

    def control(self, value):
        self.holder.stdin.write((compact(value) + '\n').encode())
        self.holder.stdin.flush()
        return self.frame()

    def original(self, occupant=True, resistant=False):
        name = self.start('claude')
        capture = self.program_capture(name)
        self.program = None
        if occupant:
            if resistant:
                self.program = subprocess.Popen([sys.executable, '-c', "import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print('ready',flush=True); time.sleep(300)"],
                                                cwd=self.project, env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, text=True)
                self.addCleanup(self.program.stdout.close)
                self.assertEqual(self.program.stdout.readline(), 'ready\n')
            else:
                self.program = subprocess.Popen(['/bin/sleep', '300'], cwd=self.project, env=self.env, stdin=subprocess.DEVNULL)
            self.addCleanup(self.settle_program, self.program)
        self.pipeline = str(uuid.uuid4())
        self.holder = subprocess.Popen([str(self.image), 'worktrees', 'program-holder', '--json'],
                                      env=self.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      executable=f'/proc/self/fd/{self.sealed}', pass_fds=(self.sealed,))
        self.addCleanup(self.close_holder)
        ready = self.control(dict(op='open', pipeline=self.pipeline, discovery=dict(capture=capture)))
        self.assertEqual(ready['op'], 'ready')
        self.ready = ready
        manifest = ready['manifest']
        if self.program is not None:
            self.assertIn(self.program.pid, [program['pid'] for program in manifest['programs']])
        original = compact(dict(request_id=str(uuid.uuid4()), action='terminate',
                                expected_run_id=capture['target']['run_id'], expected_conversation_id=capture['target']['conversation_id']))
        result = subprocess.run([str(self.image), 'session-action', name, '--json'], env=self.env,
                                input=original, text=True, capture_output=True, timeout=40)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        done = json.loads(result.stdout)
        self.assertEqual(done['status'], 'completed', done)
        self.assertTrue(self.project.exists(), 'the original live holder/occupant keeps ordinary cleanup conservative')
        archive_path = self.root / 'state/archives' / (done['result_target']['archive_id'] + '.json')
        archive = json.loads(archive_path.read_text())
        self.assertEqual(archive['version'], 1)
        self.assertEqual(archive['name'], capture['target']['name'])
        self.assertEqual(archive['run_id'], capture['target']['run_id'])
        self.assertEqual(archive['conversation_id'], capture['target']['conversation_id'])
        marker = Path(self.git('-C', str(self.project), 'rev-parse', '--absolute-git-dir')) / 'zerus-owner.json'
        self.assertEqual(archive['worktree_owner'], json.loads(marker.read_text()))
        for directory in [self.root / 'state', self.root / 'state/archives']:
            self.assertEqual(directory.stat().st_mode & 0o077, 0, str(directory))
        return dict(request_id=str(uuid.uuid4()), confirmation_id=str(uuid.uuid4()),
                    source=dict(mode='native', request_id=str(uuid.uuid4()), journal=None, revision=0),
                    manifest=manifest, original=original, archive_id=done['result_target']['archive_id'], stop=dict(kind='normal'))

    def close_holder(self):
        if self.holder is None:
            return
        child = self.holder
        self.holder = None
        if child.poll() is None:
            child.terminate()
        child.communicate(timeout=8)

    @staticmethod
    def finish_frontend(child):
        if child.poll() is None:
            child.terminate()
        if child.stdin is not None:
            child.stdin.close()
            child.stdin = None
        child.communicate(timeout=8)

    def run_original(self, command, input, *, wrong_birth=False, wrong_kind=False, cache_change=None):
        child = subprocess.Popen([str(self.frontend), str(self.root / 'tmp/hgs-tray'), str(self.image), f'held:{self.sealed}', command],
                                 env=self.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, pass_fds=(self.sealed,))
        self.addCleanup(self.finish_frontend, child)
        self.assertTrue(select.select([child.stdout], [], [], 5)[0], 'native original child readiness')
        native = json.loads(child.stdout.readline(512))
        until = time.monotonic() + 5
        while not os.readlink(f"/proc/{native['pid']}/exe").startswith('/memfd:zerus-helper-image'):
            self.assertLess(time.monotonic(), until)
            time.sleep(.002)
        native['parent'] = child.pid
        native['audit'] = None
        stop = input if command == 'program-stop' else input['stop']
        binding = dict(pipeline=self.pipeline, manifest=digest(stop['manifest']), request=input['request_id'], input=digest(input))
        armed = self.control(dict(op='arm' if command == 'program-stop' else 'arm_cleanup', pipeline=self.pipeline,
                                  input=input, borrower=native, deadline_millis=int(time.monotonic() * 1000) + 15000))
        self.assertEqual(armed, dict(op='armed', binding=binding))
        attempt = str(uuid.uuid4())
        intent = dict(input=input, pipeline=self.pipeline, holder=self.ready['holder'], socket=self.ready['socket'])
        request = dict(op='worktree_program_stop' if command == 'program-stop' else 'worktree_program_cleanup', intent=intent)
        if wrong_kind:
            request['op'] = 'worktree_program_cleanup' if command == 'program-stop' else 'worktree_program_stop'
        hello = dict(abi='desktop-delegation-v1', version=1, nonce=attempt, application='com.hgdev.hgs-tray', gui='tauri',
                     build=dict(version='1.0.0', source_commit='a' * 40, source_dirty=False), instance_epoch=str(uuid.uuid4()),
                     writer_epoch=None, configuration=stop['manifest']['configuration'], live=True, replay=False)
        cache = dict(attempt=attempt, request_id=input['request_id'], fingerprint=digest(request), child_pid=native['pid'],
                     child_birth='foreign' if wrong_birth else native['birth'], serial=1, ownership_mode='native')
        if cache_change is not None:
            cache.update(cache_change)
        for frame in [dict(abi='desktop-ui-request-v1', attempt=attempt, request=request), dict(hello=hello, request=cache)]:
            child.stdin.write(compact(frame) + '\n')
        child.stdin.write(f'desktop-ui-request-v1 {attempt}\n')
        child.stdin.close()
        child.stdin = None
        output, error = child.communicate(timeout=25)
        self.assertIn('original_reaped=1', error)
        retired = self.control(dict(op='joined', binding=binding))
        self.assertEqual(retired, dict(op='retired', binding=binding))
        return child.returncode, output, error

    def test_the_exact_original_stop_then_final_cleanup_preserves_branch_and_archive(self):
        stop = self.original()
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        report = json.loads(output)
        self.assertEqual(report['state'], 'complete', report)
        self.assertIn('proofs=1 ', error)
        self.program.wait(timeout=5)
        cleanup = dict(request_id=str(uuid.uuid4()), stop=stop)
        code, output, error = self.run_original('program-cleanup', cleanup)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'removed', output)
        self.assertIn('proofs=1 ', error)
        self.assertFalse(self.project.exists())
        self.assertTrue(self.git('-C', str(self.repo), 'show-ref', 'refs/heads/session-work'))
        archive = self.root / 'state/archives' / (stop['archive_id'] + '.json')
        self.assertEqual(json.loads(archive.read_text())['run_id'], stop['manifest']['capture']['target']['run_id'])
        self.assertEqual(self.control(dict(op='close', pipeline=self.pipeline)), dict(op='closed', pipeline=self.pipeline))
        self.holder.stdin.close()
        self.holder.stdin = None
        out, err = self.holder.communicate(timeout=8)
        self.assertEqual(self.holder.returncode, 0, out + err)

    def test_new_members_and_changed_files_after_complete_keep_the_checkout(self):
        for change in ('new-program', 'new-file'):
            with self.subTest(change=change):
                case = ProgramExecution()
                case.setUp()
                try:
                    stop = case.original()
                    code, output, error = case.run_original('program-stop', stop)
                    case.assertEqual(code, 0, output + error)
                    case.assertEqual(json.loads(output)['state'], 'complete')
                    new = None
                    if change == 'new-program':
                        new = subprocess.Popen(['/bin/sleep', '300'], cwd=case.project, env=case.env)
                        case.addCleanup(case.settle_program, new)
                    else:
                        (case.project / 'new-unsaved').write_text('keep this original data')
                    code, output, error = case.run_original('program-cleanup', dict(request_id=str(uuid.uuid4()), stop=stop))
                    case.assertEqual(code, 0, output + error)
                    case.assertEqual(json.loads(output)['state'], 'kept', output)
                    case.assertTrue(case.project.exists())
                    if new is not None:
                        case.assertIsNone(new.poll(), 'a Complete stop never authorizes newly discovered programs')
                    else:
                        case.assertEqual((case.project / 'new-unsaved').read_text(), 'keep this original data')
                finally:
                    try: case.tearDown()
                    finally: case.doCleanups()

    def test_ignored_content_changed_after_reservation_never_uses_the_old_fingerprint(self):
        (self.repo / 'Cargo.toml').write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
        self.git('-C', str(self.repo), 'add', 'Cargo.toml')
        self.commit(self.repo, 'tool-declared generated cache')
        self.git('-C', str(self.project), 'merge', '--ff-only', 'main')
        cache = self.project / 'target'
        cache.mkdir()
        (cache / 'CACHEDIR.TAG').write_text('Signature: 8a477f597d28d172789f06886806bc55\n')
        blob = cache / 'original-cache'
        blob.write_text('AAAA')
        (self.repo / '.git/info/exclude').write_text('target/\n')
        stop = self.original()
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete')
        removal = dict(request_id=str(uuid.uuid4()), stop=stop)
        # Change the SAME ignored inode after the real engine has durably
        # reserved its removal and captured the original review fingerprint.
        # The native inventory alone proves objects, not unchanged bytes.
        actual_git = shutil.which('git')
        self.assertIsNotNone(actual_git)
        script = """#!/usr/bin/python3
import json,os,pathlib,sqlite3,sys
root=pathlib.Path(ROOT)
db=root/'state/worktree-maintenance/queue.sqlite3'
if db.exists() and not (root/'injected').exists():
 with sqlite3.connect(db) as connection:
  pending=connection.execute('SELECT status FROM removals WHERE request_id=?',(REQUEST,)).fetchone()
 if pending and pending[0]=='pending':
  (root/'managed/target/original-cache').write_text('BBBB')
  (root/'injected').write_text('after reservation')
os.execv(GIT,[GIT,*sys.argv[1:]])
""".replace('ROOT', repr(str(self.root))).replace('REQUEST', repr(removal['request_id'])).replace('GIT', repr(actual_git))
        shim = self.bin / 'git'
        shim.write_text(script); shim.chmod(0o700)
        before = blob.stat().st_ino
        code, output, error = self.run_original('program-cleanup', removal)
        self.assertEqual(code, 0, output + error)
        self.assertTrue((self.root / 'injected').exists(), 'the actual reserved-window change ran')
        self.assertEqual(json.loads(output)['state'], 'kept', output)
        with sqlite3.connect(self.root / 'state/worktree-maintenance/queue.sqlite3') as connection:
            self.assertEqual(connection.execute('SELECT count(*) FROM removals WHERE request_id=?', (removal['request_id'],)).fetchone()[0], 0)
        self.assertEqual(blob.stat().st_ino, before)
        self.assertEqual(blob.read_text(), 'BBBB')

    def test_force_is_a_fresh_original_uuid_after_the_normal_mutation_retires(self):
        stop = self.original(resistant=True)
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        previous = json.loads(output)
        self.assertEqual(previous['state'], 'incomplete', previous)
        self.assertIsNone(self.program.poll(), 'normal stop never escalates automatically')
        observed = self.control(dict(op='observe', pipeline=self.pipeline, observation=str(uuid.uuid4())))
        self.assertEqual(observed['op'], 'observed')
        force = dict(stop, request_id=str(uuid.uuid4()), confirmation_id=str(uuid.uuid4()), manifest=observed['manifest'],
                     stop=dict(kind='force', previous_request_id=stop['request_id'], previous_binding=previous['binding']))
        code, output, error = self.run_original('program-stop', force)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete', output)
        self.assertEqual(self.program.wait(timeout=5), -9)
        self.assertNotEqual(force['request_id'], stop['request_id'])
        self.assertTrue(self.project.exists(), 'stopping is distinct from deleting')

    def test_partial_deletion_is_terminal_unknown_and_original_status_never_replays(self):
        folder = self.repo / 'protected'
        folder.mkdir()
        (folder / 'file').write_text('original')
        self.git('-C', str(self.repo), 'add', 'protected/file')
        self.commit(self.repo, 'protected content')
        self.git('-C', str(self.project), 'merge', '--ff-only', 'main')
        stop = self.original()
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete')
        protected = self.project / 'protected'
        protected.chmod(0o500)
        self.permissions.append(protected)
        cleanup = dict(request_id=str(uuid.uuid4()), stop=stop)
        code, output, error = self.run_original('program-cleanup', cleanup)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'unknown', output)
        before = (protected / 'file').read_bytes()
        request = [str(self.image), 'worktrees', 'program-cleanup-status', '--json']
        for _ in range(2):
            result = subprocess.run(request, input=compact(cleanup), env=self.env, text=True, capture_output=True, timeout=8)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)['report']['state'], 'unknown')
            self.assertEqual((protected / 'file').read_bytes(), before)
        # A closed native holder never reconnects/adopts a terminal cleanup,
        # even under a different removal UUID. No new helper is admitted.
        self.assertEqual(self.control(dict(op='close', pipeline=self.pipeline))['op'], 'closed')
        self.holder.stdin.close(); self.holder.stdin = None
        self.holder.communicate(timeout=8)
        before_receipts = sorted((self.root / 'state/worktree-program-stops').iterdir())
        for new_id in (cleanup['request_id'], str(uuid.uuid4())):
            replay = dict(cleanup, request_id=new_id)
            envelope = dict(abi='desktop-ui-request-v1', attempt=str(uuid.uuid4()), request=dict(op='worktree_program_cleanup',
                            intent=dict(input=replay, pipeline=self.pipeline, holder=self.ready['holder'], socket=self.ready['socket'])))
            result = subprocess.run([str(self.image), 'worktrees', 'program-cleanup', '--json'], input=compact(envelope),
                                    env=self.env, text=True, capture_output=True, timeout=8)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((protected / 'file').read_bytes(), before)
            self.assertEqual(sorted((self.root / 'state/worktree-program-stops').iterdir()), before_receipts)

    def test_the_closed_kind_and_current_cache_remain_independent_of_native_image_equality(self):
        for change in ('kind', 'serial', 'attempt', 'fingerprint', 'mode'):
            with self.subTest(change=change):
                case = ProgramExecution()
                case.setUp()
                try:
                    stop = case.original()
                    cache = {'serial': 0} if change == 'serial' else ({'attempt': str(uuid.uuid4())} if change == 'attempt'
                            else ({'fingerprint': 'f' * 64} if change == 'fingerprint' else ({'ownership_mode': 'service'} if change == 'mode' else None)))
                    code, output, error = case.run_original('program-stop', stop, wrong_kind=change == 'kind', cache_change=cache)
                    case.assertNotEqual(code, 0, output + error)
                    case.assertIsNone(case.program.poll())
                    case.assertTrue(case.project.exists())
                    case.assertFalse((case.root / 'state/worktree-program-stops' / (stop['request_id'] + '.original.json')).exists())
                finally:
                    try: case.tearDown()
                    finally: case.doCleanups()

    def test_wrong_original_registration_refuses_without_signalling_the_known_occupant(self):
        stop = self.original()
        code, output, error = self.run_original('program-stop', stop, wrong_birth=True)
        self.assertNotEqual(code, 0, output + error)
        self.assertIn('proofs=1 ', error)
        self.assertIsNone(self.program.poll())
        self.assertTrue(self.project.exists())
        receipt = self.root / 'state/worktree-program-stops' / (stop['request_id'] + '.result.json')
        self.assertFalse(receipt.exists())


if __name__ == '__main__':
    unittest.main()
