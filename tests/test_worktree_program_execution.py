"""Private Linux own-helper integration; no installed GUI or owner processes.

Real canonical native parent, sealed original child, Root proof, live native
loan and guarded Git engine. Cached native evidence is supplied by a fixture;
this is not installed GUI confirmation or Mac/Windows acceptance.
"""
import hashlib
from contextlib import closing
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
        self.assert_original_stop_and_cleanup(self.original())

    def assert_original_stop_and_cleanup(self, stop):
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

    def unrelated_archive_history(self, stop):
        folder = self.root / 'state/archives'
        template = json.loads((folder / (stop['archive_id'] + '.json')).read_text())
        total = 0
        for index in range(48):
            archive = dict(template)
            archive.update(archive_id=str(uuid.uuid4()),
                           name='claude/unrelated-history/archive-' + str(index),
                           session_id=str(uuid.uuid4()), run_id=str(uuid.uuid4()),
                           conversation_id=str(uuid.uuid4()),
                           cwd=str(self.root / ('old-folder-' + str(index))),
                           launch_dir=str(self.root / ('old-folder-' + str(index))),
                           worktree_owner=None, unrelated_fixture_padding='x' * (768 << 10))
            path = folder / (archive['archive_id'] + '.json')
            data = compact(archive).encode()
            self.assertLess(len(data), 1 << 20)
            path.write_bytes(data)
            path.chmod(0o600)
            total += len(data)
        self.assertGreater(total, 32 << 20)
        return total

    def warm_archive_references(self):
        # Ordinary archive browsing performs migration/reconciliation. The
        # mutation guard itself must never rebuild unknown reference coverage.
        result = subprocess.run([str(test_pause.HGS), 'archives', '--json'], env=self.env,
                                input='{"limit":1}', text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def historical_archives(self, stop):
        folder = self.root / 'state/archives'
        original = json.loads((folder / (stop['archive_id'] + '.json')).read_text())
        archives = []
        for tag in ('older', 'legacy'):
            record = dict(original, archive_id=str(uuid.uuid4()), session_id=str(uuid.uuid4()),
                          run_id=str(uuid.uuid4()), conversation_id=str(uuid.uuid4()),
                          name='claude/p/' + tag, archived_at=1)
            if tag == 'legacy':
                record.pop('worktree_owner')
            path = folder / (record['archive_id'] + '.json')
            path.write_text(compact(record))
            path.chmod(0o600)
            archives.append(record)
        return archives

    def assert_restore_metadata(self, stop, historical):
        folder = self.root / 'state/archives'
        original = json.loads((folder / (stop['archive_id'] + '.json')).read_text())
        expected = original['worktree_cleanup']
        self.assertEqual(expected['status'], 'removal_pending')
        self.assertEqual(expected['owner']['id'], stop['manifest']['capture']['owner_id'])
        self.assertEqual(expected['branch'], 'session-work')
        self.assertEqual(expected['head'], self.git('-C', str(self.repo), 'rev-parse', 'refs/heads/session-work'))
        for record in historical:
            saved = json.loads((folder / (record['archive_id'] + '.json')).read_text())
            self.assertEqual(saved, dict(record, worktree_cleanup=expected), 'every compatible historical reference prepared durably')

    def restore_historical(self, record):
        self.hgs('resume', record['name'], '--archive', record['archive_id'], '--dry-run')
        self.assertFalse(self.project.exists(), 'inspection cannot recreate the folder')
        self.hgs('resume', record['name'], '--archive', record['archive_id'], '-d')
        self.wait(lambda: self.binding(record['name']).get('conversation_id') == record['conversation_id']
                  and not self.binding(record['name']).get('expected_id'))
        self.assertEqual((self.project / 'tracked').read_text(), 'original')
        self.assertEqual(self.git('-C', str(self.project), 'branch', '--show-current'), 'session-work')

    def test_unrelated_archive_bodies_over_the_global_budget_do_not_block_original_cleanup(self):
        stop = self.original()
        self.warm_archive_references()
        self.unrelated_archive_history(stop)
        historical = self.historical_archives(stop)
        unrelated = {p: hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in (self.root / 'state/archives').glob('*.json')
                     if json.loads(p.read_text())['name'].startswith('claude/unrelated-history/')}
        # An older CLI knows only the search stamp. Its ordinary refresh must
        # not hide these new historical Restore references from the next CLI.
        stat = (self.root / 'state/archives').stat()
        seconds, nanos = divmod(stat.st_mtime_ns, 1_000_000_000)
        stamp = f'{stat.st_size}:SystemTime {{ tv_sec: {seconds}, tv_nsec: {nanos} }}'
        database = self.root / 'state/archive-catalog-v1.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute("UPDATE catalog_meta SET value=? WHERE key='stamp'", (stamp,))
            self.assertNotEqual(db.execute("SELECT value FROM catalog_meta WHERE key='archive_references_stamp'").fetchone()[0], stamp)
            self.assertEqual(db.execute('SELECT count(*) FROM archive_references WHERE id=?',
                                       (historical[0]['archive_id'],)).fetchone()[0], 0)
        self.warm_archive_references()
        # The original native continuation still has to prove its exact
        # termination/archive/owner. Unrelated body volume grants no bypass.
        self.assert_original_stop_and_cleanup(stop)
        self.assert_restore_metadata(stop, historical)
        self.assertEqual({p: hashlib.sha256(p.read_bytes()).hexdigest() for p in unrelated}, unrelated)
        self.restore_historical(historical[0])

    def test_legacy_missing_owner_archive_remains_restorable_after_original_cleanup(self):
        stop = self.original()
        historical = self.historical_archives(stop)
        self.warm_archive_references()
        self.assert_original_stop_and_cleanup(stop)
        self.assert_restore_metadata(stop, historical)
        self.restore_historical(historical[1])

    def test_incomplete_relevant_reference_index_keeps_the_checkout_without_rebuilding_it(self):
        stop = self.original()
        historical = self.historical_archives(stop)
        self.warm_archive_references()
        database = self.root / 'state/archive-catalog-v1.sqlite3'
        with closing(sqlite3.connect(database)) as db, db:
            db.execute('DELETE FROM archive_reference_paths WHERE id=?', (historical[0]['archive_id'],))
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete')
        self.program.wait(timeout=5)
        code, output, error = self.run_original('program-cleanup', dict(request_id=str(uuid.uuid4()), stop=stop))
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'kept')
        self.assertTrue(self.project.exists())
        self.assertTrue(self.git('-C', str(self.repo), 'show-ref', 'refs/heads/session-work'))
        with closing(sqlite3.connect(database)) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM archive_reference_paths WHERE id=?',
                                        (historical[0]['archive_id'],)).fetchone()[0], 0)

    def test_large_unrelated_history_never_substitutes_for_the_exact_original_archive(self):
        stop = self.original()
        self.unrelated_archive_history(stop)
        self.warm_archive_references()
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete')
        self.program.wait(timeout=5)
        path = self.root / 'state/archives' / (stop['archive_id'] + '.json')
        original = json.loads(path.read_text())
        changed = dict(original)
        changed['worktree_owner'] = dict(original['worktree_owner'], id=str(uuid.uuid4()))
        path.write_text(compact(changed))
        cleanup = dict(request_id=str(uuid.uuid4()), stop=stop)
        code, output, error = self.run_original('program-cleanup', cleanup)
        self.assertNotEqual(code, 0, output + error)
        self.assertIn('proofs=0 ', error)
        self.assertTrue(self.project.exists())
        self.assertEqual(json.loads(path.read_text()), changed, 'no rewrite or archive adoption')
        self.assertTrue(self.git('-C', str(self.repo), 'show-ref', 'refs/heads/session-work'))

    def refuse_unavailable_original_archive(self, missing):
        stop = self.original()
        historical = self.historical_archives(stop)
        self.warm_archive_references()
        folder = self.root / 'state/archives'
        retained = {folder / (row['archive_id'] + '.json'):
                    (folder / (row['archive_id'] + '.json')).read_bytes() for row in historical}
        code, output, error = self.run_original('program-stop', stop)
        self.assertEqual(code, 0, output + error)
        self.assertEqual(json.loads(output)['state'], 'complete')
        self.program.wait(timeout=5)
        original = folder / (stop['archive_id'] + '.json')
        if missing:
            original.unlink()
        else:
            original.write_text('{corrupt original')
        code, output, error = self.run_original('program-cleanup', dict(request_id=str(uuid.uuid4()), stop=stop))
        self.assertNotEqual(code, 0, output + error)
        self.assertIn('proofs=0 ', error)
        self.assertTrue(self.project.exists())
        self.assertTrue(self.git('-C', str(self.repo), 'show-ref', 'refs/heads/session-work'))
        self.assertEqual({path: path.read_bytes() for path in retained}, retained)
        self.assertEqual(original.exists(), not missing)
        if not missing:
            self.assertEqual(original.read_text(), '{corrupt original')

    def test_missing_original_archive_cannot_be_replaced_by_other_valid_references(self):
        self.refuse_unavailable_original_archive(missing=True)

    def test_corrupt_original_archive_cannot_be_replaced_by_other_valid_references(self):
        self.refuse_unavailable_original_archive(missing=False)

    def test_a_configured_state_alias_keeps_the_same_native_scope_and_receipts(self):
        alias = self.root / 'selected-state'
        alias.symlink_to(self.root / 'state', target_is_directory=True)
        self.env['HGS_STATE_DIR'] = str(alias)
        # Use the actual original helper/proof/loan path. Canonical state
        # selection must neither split its configuration nor lose its receipts.
        self.test_the_exact_original_stop_then_final_cleanup_preserves_branch_and_archive()
        self.assertTrue(alias.is_symlink())
        self.assertEqual(alias.resolve(), self.root / 'state')
        originals = list((alias / 'worktree-program-stops').glob('*.original.json'))
        self.assertEqual(len(originals), 2)
        for record in originals:
            removal = record.name.startswith('cleanup-')
            command = 'program-cleanup-status' if removal else 'program-stop-status'
            saved = json.loads(record.read_text())['input']
            status = subprocess.run([str(self.image), 'worktrees', command, '--json'],
                                    input=compact(saved), env=self.env, text=True, capture_output=True, timeout=8)
            self.assertEqual(status.returncode, 0, status.stderr)
            receipt = json.loads(status.stdout)
            self.assertTrue(receipt['found'])
            self.assertEqual(receipt['report']['state'], 'removed' if removal else 'complete')

    def test_a_journal_symlink_below_the_selected_state_refuses_without_a_signal(self):
        alias = self.root / 'selected-state'
        alias.symlink_to(self.root / 'state', target_is_directory=True)
        self.env['HGS_STATE_DIR'] = str(alias)
        stop = self.original()
        outside = self.root / 'foreign-journal'
        outside.mkdir(mode=0o700)
        sentinel = outside / 'retained'
        sentinel.write_text('original foreign data')
        journal = self.root / 'state/worktree-program-stops'
        journal.symlink_to(outside, target_is_directory=True)
        code, output, error = self.run_original('program-stop', stop)
        self.assertNotEqual(code, 0, output + error)
        self.assertIn('Cannot pin clone directory', error)
        self.assertIsNone(self.program.poll())
        self.assertTrue(self.project.exists())
        self.assertTrue(journal.is_symlink())
        self.assertEqual(list(outside.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_text(), 'original foreign data')

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
