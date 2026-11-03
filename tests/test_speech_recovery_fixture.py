"""Speech fixture preparation keeps Cargo out of parallel recovery workers."""
import importlib.util
import json
import os
import shutil
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import test_speech_recovery as recovery

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('speech_fixture_selector', ROOT / 'scripts/ci/speech-recovery-executable.py')
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


@unittest.skipUnless(os.name == "posix", "speech recovery fixture preparation is Unix-only")
class PreparedSpeechFixture(unittest.TestCase):
    def test_prepared_executable_does_not_start_cargo_in_the_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / 'voice-fixture'
            executable.write_text('#!/bin/sh\nexit 99\n')
            executable.chmod(0o700)
            with patch.dict(os.environ, HGS_TEST_SPEECH_RECOVERY_BIN=str(executable)), \
                 patch.object(recovery.subprocess, 'run', side_effect=AssertionError('worker invoked Cargo')):
                recovery.SpeechRecovery.setUpClass()
            self.assertEqual(recovery.SpeechRecovery.fixture, executable.resolve())

    def test_missing_or_nonexecutable_override_refuses_without_build_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            leaf = Path(directory) / 'voice-fixture'
            leaf.write_text('not executable')
            for candidate in ('', str(leaf), str(leaf.with_name('absent'))):
                with self.subTest(candidate=candidate), \
                     patch.dict(os.environ, HGS_TEST_SPEECH_RECOVERY_BIN=candidate), \
                     patch.object(recovery.subprocess, 'run', side_effect=AssertionError('fallback Cargo invoked')):
                    with self.assertRaisesRegex(AssertionError, 'not executable'):
                        recovery.SpeechRecovery.setUpClass()

    def test_cli_prepares_once_before_workers_and_stops_on_preparation_failure(self):
        bash = shutil.which('bash')
        self.assertIsNotNone(bash)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'scripts/ci/cli.sh'
            script.parent.mkdir(parents=True)
            script.write_bytes((ROOT / 'scripts/ci/cli.sh').read_bytes())
            binaries = root / 'bin'
            binaries.mkdir()
            programs = {
                'cargo': '#!/bin/sh\nprintf "cargo:%s\\n" "$*" >> "$CALL_LOG"\n'
                         'case "$*" in *voice_recovery_fixture*) [ "$FAIL_KIND" != build ] || exit 17;; esac\n',
                'python3': '#!/bin/sh\nprintf "python:%s\\n" "$*" >> "$CALL_LOG"\n'
                           'case "$1" in\n'
                           'scripts/ci/speech-recovery-executable.py) [ "$FAIL_KIND" != selector ] || exit 19; printf "/fixture/prepared-voice\\n";;\n'
                           'scripts/ci/python-tests.py) [ "$HGS_TEST_SPEECH_RECOVERY_BIN" = /fixture/prepared-voice ] || exit 21;;\n'
                           'esac\n',
                'bash': '#!/bin/sh\nprintf "smoke:%s\\n" "$*" >> "$CALL_LOG"\n',
            }
            for name, body in programs.items():
                path = binaries / name
                path.write_text(body)
                path.chmod(0o700)
            for failure, expected in (('none', 0), ('build', 17), ('selector', 19)):
                with self.subTest(failure=failure):
                    log = root / (failure + '.log')
                    result = subprocess.run([bash, str(script), 'remaining'],
                        env=dict(os.environ, PATH=str(binaries) + ':/usr/bin:/bin',
                                 CALL_LOG=str(log), FAIL_KIND=failure),
                        capture_output=True, text=True, timeout=5)
                    self.assertEqual(result.returncode, expected, result.stderr)
                    calls = log.read_text().splitlines()
                    build = next(i for i, line in enumerate(calls) if 'voice_recovery_fixture' in line)
                    workers = [i for i, line in enumerate(calls) if line == 'python:scripts/ci/python-tests.py']
                    if expected == 0:
                        self.assertEqual(len(workers), 1)
                        self.assertLess(build, workers[0])
                        self.assertIn('--target-dir target/speech-recovery', calls[build])
                    else:
                        self.assertEqual(workers, [])
                        self.assertNotIn('cargo:build --release --locked', calls)

    def fixture(self, directory):
        root = Path(directory)
        source = root / 'tests/voice_recovery_fixture.rs'
        source.parent.mkdir()
        source.write_text('// synthetic source identity')
        executable = root / 'target/speech-recovery/debug/deps/voice_recovery_fixture-fixture'
        executable.parent.mkdir(parents=True)
        executable.write_text('#!/bin/sh\nexit 99\n')
        executable.chmod(0o700)
        record = dict(reason='compiler-artifact', executable=str(executable),
                      target=dict(name='voice_recovery_fixture', src_path=str(source)))
        manifest = root / 'build.jsonl'
        return root, executable, record, manifest

    def test_exact_current_source_and_dedicated_target_are_selected_without_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root, executable, record, manifest = self.fixture(directory)
            manifest.write_text(json.dumps({'reason': 'build-finished', 'success': True}) + '\n' + json.dumps(record) + '\n')
            self.assertEqual(selector.executable(manifest, root), executable.resolve())

    def test_missing_duplicate_foreign_source_or_nonfixture_target_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, executable, record, manifest = self.fixture(directory)
            foreign = dict(record, target=dict(record['target'], src_path=str(root / 'foreign.rs')))
            nonfixture = dict(record, target=dict(record['target'], name='other-test'))
            for rows in ([], [record, record], [foreign], [nonfixture]):
                with self.subTest(rows=rows):
                    manifest.write_text(''.join(json.dumps(row) + '\n' for row in rows))
                    with self.assertRaises(ValueError):
                        selector.executable(manifest, root)

    def test_main_debug_path_and_linked_or_nonexecutable_output_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root, executable, record, manifest = self.fixture(directory)
            main_debug = root / 'target/debug/hgs'
            main_debug.parent.mkdir()
            main_debug.write_text('#!/bin/sh\nexit 99\n')
            main_debug.chmod(0o700)
            manifest.write_text(json.dumps(dict(record, executable=str(main_debug))) + '\n')
            with self.assertRaises(ValueError):
                selector.executable(manifest, root)
            link = executable.with_name('linked-fixture')
            link.symlink_to(executable)
            manifest.write_text(json.dumps(dict(record, executable=str(link))) + '\n')
            with self.assertRaises(ValueError):
                selector.executable(manifest, root)
            executable.chmod(0o600)
            manifest.write_text(json.dumps(record) + '\n')
            with self.assertRaises(ValueError):
                selector.executable(manifest, root)
