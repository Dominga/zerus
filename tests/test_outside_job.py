"""Bounded CI cleanup policy and real private utility-tree controls.

Cleanup policy tests replace subprocesses and the clock. RealUtility starts only
bounded synthetic children, on Linux and natively in the Windows client gate.
"""
import importlib.util
import io
import json
import os
import sys
import tempfile
import time
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "outside_job_control", Path(__file__).resolve().parents[1] / "scripts/ci/outside-job.py")
outside_job = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(outside_job)


class Cleanup(unittest.TestCase):
    def setUp(self):
        self.now = 0.0
        self.calls = []
        self.messages = []
        self.clock = mock.patch.object(outside_job.time, "monotonic", side_effect=lambda: self.now)
        self.clock.start()
        self.addCleanup(self.clock.stop)
        self.runner = mock.patch.object(outside_job, "bounded_run", side_effect=self.unexpected)
        self.run = self.runner.start()
        self.addCleanup(self.runner.stop)

    def unexpected(self, *args, **kwargs):
        raise AssertionError("unexpected real subprocess attempt")

    def completed(self, argv, **kwargs):
        self.calls.append((argv, kwargs))
        return subprocess.CompletedProcess(argv, 0, stdout="", stderr="")

    def stop(self):
        return outside_job.stop_copy(4321, [r"C:\fixture\hgs.exe", r"C:\fixture\host\hgs.exe"], self.messages.append)

    def test_timeout_cleanup_keeps_the_exact_pid_and_patterns(self):
        self.run.side_effect = self.completed
        cleanup = self.stop()
        self.assertEqual(self.calls[0][0], ["taskkill", "/T", "/F", "/PID", "4321"])
        self.assertEqual([call[1]["env"]["OUTSIDE_JOB_EXE"] for call in self.calls[1:]],
                         [r"C:\fixture\hgs.exe", r"C:\fixture\host\hgs.exe"])
        self.assertEqual([call[1]["timeout"] for call in self.calls], [30, 30, 30])
        self.assertEqual(cleanup.problems, [])
        self.assertEqual(cleanup.exit_code(124), 124)
        self.assertEqual(cleanup.exit_code(0), 0)

    def test_taskkill_timeout_does_not_skip_the_two_remaining_cleanups(self):
        def run(argv, **kwargs):
            if argv[0] == "taskkill":
                self.calls.append((argv, kwargs))
                self.now += kwargs["timeout"]
                raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            return self.completed(argv, **kwargs)
        self.run.side_effect = run
        cleanup = self.stop()
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(len(cleanup.problems), 1)
        self.assertIn("taskkill: outcome unknown (TimeoutExpired)", cleanup.problems[0])
        self.assertEqual(cleanup.exit_code(124), 124)
        self.assertEqual(cleanup.exit_code(0), 1)

    def test_taskkill_os_error_does_not_skip_the_two_remaining_cleanups(self):
        def run(argv, **kwargs):
            if argv[0] == "taskkill":
                self.calls.append((argv, kwargs))
                raise OSError("synthetic denied")
            return self.completed(argv, **kwargs)
        self.run.side_effect = run
        cleanup = self.stop()
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(len(cleanup.problems), 1)
        self.assertIn("OSError", cleanup.problems[0])
        self.assertEqual(cleanup.exit_code(0), 1)

    def test_nonzero_and_unknown_outcomes_all_survive_later_attempts(self):
        outcomes = iter([5, 7, None])
        def run(argv, **kwargs):
            self.calls.append((argv, kwargs))
            return SimpleNamespace(returncode=next(outcomes))
        self.run.side_effect = run
        cleanup = self.stop()
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(cleanup.problems, ["taskkill: exit status 5", "executable cleanup 1: exit status 7",
                                            "executable cleanup 2: exit status unknown"])
        self.assertEqual(cleanup.exit_code(0), 1)
        self.assertEqual(cleanup.exit_code(17), 17)

    def test_each_action_uses_the_remaining_shared_allowance(self):
        durations = iter([29.5, 30, 30, 30, 10])
        def run(argv, **kwargs):
            self.calls.append((argv, kwargs))
            duration = next(durations)
            self.now += min(duration, kwargs["timeout"])
            if duration > kwargs["timeout"]:
                raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            return subprocess.CompletedProcess(argv, 0)
        self.run.side_effect = run
        cleanup = outside_job.stop_copy(4321, ["one", "two", "three", "four"], self.messages.append)
        self.assertEqual([call[1]["timeout"] for call in self.calls], [30, 30, 30, 30, 0.5])
        self.assertEqual(self.now, 120)
        self.assertEqual(len(cleanup.problems), 1)
        self.assertIn("executable cleanup 4", cleanup.problems[0])

    def test_exhausted_allowance_records_each_unattempted_cleanup(self):
        cleanup = outside_job.Cleanup(self.messages.append)
        self.now = 120
        outside_job.end_executables(["one", "two"], cleanup)
        self.run.assert_not_called()
        self.assertEqual(cleanup.problems, [
            "executable cleanup 1: not attempted; cleanup allowance exhausted",
            "executable cleanup 2: not attempted; cleanup allowance exhausted"])
        self.assertEqual(cleanup.exit_code(0), 1)

    def test_reporting_failure_does_not_skip_remaining_cleanup(self):
        def report(message):
            raise OSError("synthetic log failure")
        def run(argv, **kwargs):
            self.calls.append((argv, kwargs))
            return subprocess.CompletedProcess(argv, 1 if argv[0] == "taskkill" else 0)
        self.run.side_effect = run
        cleanup = outside_job.stop_copy(4321, ["one", "two"], report)
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(len(cleanup.problems), 2)
        self.assertIn("cannot report cleanup outcome", cleanup.problems[1])
        self.assertEqual(cleanup.exit_code(0), 1)

    def test_unknown_command_outcome_cannot_be_success(self):
        cleanup = outside_job.Cleanup(self.messages.append)
        self.assertEqual(cleanup.exit_code(None), 1)
        self.assertEqual(cleanup.problems, ["command exit status unknown"])

    def test_failed_survivor_report_never_claims_no_process_is_left(self):
        def run(argv, **kwargs):
            raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
        self.run.side_effect = run
        log = io.BytesIO()
        cleanup = outside_job.Cleanup(lambda message: log.write(message.encode()))
        outside_job.report_survivors(1234, log, cleanup)
        self.assertEqual(len(cleanup.problems), 1)
        self.assertNotIn(b"no process of the suites is left", log.getvalue())
        self.assertEqual(cleanup.exit_code(0), 1)

    def test_custody_timeout_never_attempts_pid_tree_or_image_cleanup(self):
        outside_job.timeout_cleanup(4321, [], self.messages.append, custody_only=True)
        self.run.assert_not_called()
        self.assertIn("process outcome unknown", "".join(self.messages))

    def test_command_failure_is_kept_when_cleanup_also_fails(self):
        self.run.side_effect = lambda argv, **kwargs: subprocess.CompletedProcess(argv, 7)
        cleanup = self.stop()
        self.assertEqual(len(cleanup.problems), 3)
        self.assertEqual(cleanup.exit_code(19), 19)

    def test_main_timeout_bounds_taskkill_and_retains_unknown_cleanup(self):
        self.main_timeout(False)

    def test_custody_main_timeout_retains_unknown_without_any_kill(self):
        self.main_timeout(True)

    def test_custody_parser_rejects_executable_cleanup(self):
        with mock.patch.object(outside_job.sys, "argv", ["outside-job", "--custody-only", "--end", "fixture"]), \
                mock.patch.object(outside_job.sys, "stderr", io.StringIO()):
            with self.assertRaises(SystemExit) as error: outside_job.main()
        self.assertEqual(error.exception.code, 2)
        self.run.assert_not_called()

    def main_timeout(self, custody_only):
        # This control also reaches the original main's actual taskkill call:
        # its missing timeout fails the same assertion, without invoking it.
        case = self
        files = {}
        removals = []
        class File:
            def __init__(self, value):
                self.value = str(value)
            def __str__(self):
                return self.value
            def __truediv__(self, name):
                return File(self.value + "/" + name)
            @property
            def parent(self):
                return File("fixture-parent")
            def resolve(self):
                return self
            def mkdir(self, **kwargs):
                pass
            def write_bytes(self, data):
                files[self.value] = data
            def write_text(self, text, **kwargs):
                files[self.value] = text.encode()
            def exists(self):
                return self.value in files
            def unlink(self, **kwargs):
                removals.append(self.value)
                files.pop(self.value, None)
            def rmdir(self):
                removals.append(self.value)
        class Follow(io.BytesIO):
            def read(self, *args):
                case.assertEqual(args, (outside_job.LOG_CHUNK,))
                case.now = max(case.now, 631)
                return super().read(*args)
        def open_log(path, mode, **kwargs):
            self.assertEqual(str(path), "fixture-log")
            self.assertIn(mode, ("rb", "ab"))
            return Follow() if mode == "rb" else io.BytesIO()
        def run(argv, **kwargs):
            self.calls.append((argv, kwargs))
            if argv[0] == "powershell" and "Invoke-CimMethod" in argv[-1]:
                return subprocess.CompletedProcess(argv, 0, "0 4321", "")
            # Every cleanup, including the original main's taskkill, must be finite.
            self.assertIn("timeout", kwargs)
            self.assertGreater(kwargs["timeout"], 0)
            self.assertLessEqual(kwargs["timeout"], 30)
            if argv[0] == "taskkill":
                self.now += kwargs["timeout"]
                raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            return subprocess.CompletedProcess(argv, 0, stdout="")
        self.run.side_effect = run
        args = SimpleNamespace(child=None, command=["fixture-command"], log="fixture-log",
                               timeout=600, end=[] if custody_only else ["one", "two"], custody_only=custody_only)
        error = io.StringIO()
        with mock.patch.object(outside_job.argparse.ArgumentParser, "parse_args", return_value=args), \
                mock.patch.object(outside_job.os, "name", "nt"), \
                mock.patch.object(outside_job.os, "getcwd", return_value="fixture-cwd"), \
                mock.patch.dict(outside_job.os.environ, {}, clear=True), \
                mock.patch.object(outside_job, "Path", File), \
                mock.patch.object(outside_job.tempfile, "mkdtemp", return_value="fixture-work"), \
                mock.patch.object(outside_job, "exited", return_value=False), \
                mock.patch.object(outside_job.time, "sleep", side_effect=self.unexpected), \
                mock.patch("builtins.open", side_effect=open_log), \
                mock.patch.object(outside_job.sys, "stdout", io.StringIO()), \
                mock.patch.object(outside_job.sys, "stderr", error):
            code = outside_job.main()
        self.assertEqual(code, 124)
        if custody_only:
            self.assertEqual(len(self.calls), 1)  # Only WMI launch; no cleanup process.
            self.assertIn("custody-only timeout", error.getvalue())
            self.assertFalse(any(call[0][0] == "taskkill" for call in self.calls))
        else:
            self.assertEqual(len(self.calls), 5)  # WMI, diagnostic snapshot, taskkill, both patterns.
            self.assertEqual(sum(call[0][0] == "taskkill" for call in self.calls), 1)
            self.assertIn("taskkill: outcome unknown (TimeoutExpired)", error.getvalue())
        self.assertIn("retained status/work directory fixture-work", error.getvalue())
        self.assertNotIn("ended pid", error.getvalue())
        self.assertEqual(removals, [])
        self.assertIn("fixture-work/spec.json", files)


class ChildDeadline(unittest.TestCase):
    def control(self, custody_only):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = root / "spec.json"
            spec.write_text(json.dumps({"argv": ["controlled-test"], "cwd": directory, "env": {},
                                        "log": str(root / "log"), "status": str(root / "status"),
                                        "end": [], "deadline": 20, "custody_only": custody_only}))
            process = mock.Mock()
            process.wait.return_value = 7
            if not custody_only:
                process.wait.side_effect = subprocess.TimeoutExpired("controlled-test", 0)
            order = []
            def snapshot(started, log, cleanup):
                order.append("snapshot")
                log.write(b"fixed process diagnostic\n")
            with mock.patch.object(outside_job, "in_job", return_value=False), \
                    mock.patch.object(outside_job, "join_kill_on_close_job", return_value=42), \
                    mock.patch.object(outside_job.time, "monotonic", return_value=21), \
                    mock.patch.object(outside_job.subprocess, "Popen", return_value=process), \
                    mock.patch.object(outside_job, "report_survivors", side_effect=snapshot), \
                    mock.patch.object(outside_job, "end_executables", side_effect=lambda *a: order.append("cleanup")):
                outside_job.child(str(spec))
            process.wait.assert_called_once_with(timeout=None if custody_only else 0)
            self.assertEqual((root / "status").read_text(), "7" if custody_only else "124")
            self.assertEqual(order, ["snapshot", "cleanup", "snapshot"])
            if not custody_only:
                self.assertIn("deadline expired", (root / "log").read_text())

    def test_outside_copy_has_its_own_command_deadline_and_diagnostic(self):
        self.control(False)

    def test_custody_copy_retains_its_unbounded_original_wait(self):
        self.control(True)


class EarlyCleanupAllowance(unittest.TestCase):
    def control(self, command_code, now=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = root / "spec.json"
            spec.write_text(json.dumps({"argv": ["controlled-test"], "cwd": directory, "env": {},
                                        "log": str(root / "log"), "status": str(root / "status"),
                                        "end": ["one", "two", "three", "four", "five"],
                                        "deadline": 600, "custody_only": False}))
            clock = [now]
            limits = []
            def utility(argv, **kwargs):
                limits.append(kwargs["timeout"])
                clock[0] += kwargs["timeout"]
                return subprocess.CompletedProcess(argv, 0, stdout="")
            process = mock.Mock()
            process.wait.return_value = command_code
            with mock.patch.object(outside_job, "in_job", return_value=False), \
                    mock.patch.object(outside_job, "join_kill_on_close_job", return_value=42), \
                    mock.patch.object(outside_job.time, "monotonic", side_effect=lambda: clock[0]), \
                    mock.patch.object(outside_job.subprocess, "Popen", return_value=process), \
                    mock.patch.object(outside_job, "bounded_run", side_effect=utility):
                outside_job.child(str(spec))
            expected = min(now + 120, 720)
            self.assertEqual(clock[0], expected)
            self.assertEqual(sum(limits), expected - now)
            self.assertEqual((root / "status").read_text(), str(command_code or 1))
            self.assertIn("cleanup allowance exhausted", (root / "log").read_text())
            return limits

    def test_early_success_does_not_lend_unused_command_time_to_cleanup(self):
        self.assertEqual(self.control(0), [30, 30, 30, 30])

    def test_early_failure_keeps_its_status_and_clips_the_last_cleanup(self):
        self.assertEqual(self.control(17), [5, 30, 30, 30, 25])

    def test_late_observation_never_renews_the_original_hard_deadline(self):
        self.assertEqual(self.control(0, now=719), [1])


class RealUtility(unittest.TestCase):
    """Real private children, including a grandchild retaining BOTH outputs.

    All fixture children have their own 30-second safety exit even under a bad
    harness mutation; no recursion, shell, install, account or host service.
    These run natively in the existing Windows client step as well as on Unix.
    """
    def wait_gone(self, pid, seconds=35):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if os.name == "nt":
                gone = outside_job.exited(pid)
            elif sys.platform == "linux":
                path = Path(f"/proc/{pid}/stat")
                try:
                    gone = path.read_text().split(") ", 1)[1].startswith("Z ")
                except FileNotFoundError:
                    gone = True
            else:
                try:
                    os.kill(pid, 0)
                    gone = False
                except ProcessLookupError:
                    gone = True
            if gone:
                return
            time.sleep(0.05)
        self.fail("private utility grandchild did not exit within its safety bound")

    def control(self, root_exits):
        with tempfile.TemporaryDirectory(prefix="outside-utility-control-") as directory:
            pid_file = Path(directory) / "grandchild.json"
            grandchild = "import os,sys,time;print('grandchild stdout',flush=True);print('grandchild stderr',file=sys.stderr,flush=True);time.sleep(30)"
            root = ("import subprocess,sys,time,json,pathlib;"
                    "p=subprocess.Popen([sys.executable,'-u','-c',sys.argv[2]]);"
                    "pathlib.Path(sys.argv[1]).write_text(json.dumps(p.pid));"
                    "print('root started',flush=True);"
                    + ("sys.exit(0)" if root_exits else "time.sleep(30)"))
            before = time.monotonic()
            limit = 8 if os.name == "nt" else 2
            try:
                if root_exits:
                    result = outside_job.bounded_run([sys.executable, "-u", "-c", root, str(pid_file), grandchild],
                                                     timeout=limit, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("root started", result.stdout)
                else:
                    with self.assertRaises(subprocess.TimeoutExpired):
                        outside_job.bounded_run([sys.executable, "-u", "-c", root, str(pid_file), grandchild],
                                                timeout=limit, capture_output=True)
                self.assertLess(time.monotonic() - before, limit + outside_job.KILL_WAIT + 3)
                self.assertTrue(pid_file.is_file(), "utility did not reach the real grandchild control")
                pid = json.loads(pid_file.read_text())
                self.wait_gone(pid, 3)
            finally:
                if pid_file.exists():
                    # The deliberate mutant's child still has its safety exit.
                    self.wait_gone(json.loads(pid_file.read_text()))

    def test_success_does_not_wait_for_descendant_output_eof(self):
        self.control(True)

    def test_timeout_ends_the_whole_utility_tree_without_pipe_drain(self):
        self.control(False)

    def test_action_eof_never_dispatches_the_utility(self):
        with tempfile.TemporaryDirectory() as directory:
            spec = Path(directory) / "action.json"
            marker = Path(directory) / "must-not-exist"
            spec.write_text(json.dumps({"argv": [sys.executable, "-c", "import pathlib,sys;pathlib.Path(sys.argv[1]).touch()", str(marker)], "env": {}}))
            result = subprocess.run([sys.executable, "-I", outside_job.__file__, "--action", str(spec)],
                                    stdin=subprocess.DEVNULL, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 125, result.stderr)
            self.assertFalse(marker.exists())


class Workflow(unittest.TestCase):
    def test_packages_precede_the_risky_gate_and_diagnostics_are_always_retained(self):
        text = (Path(__file__).resolve().parents[1] / ".github/workflows/checks.yml").read_text()
        start = text.index("  windows-desktop:")
        end = text.index("\n  windows-host:", start)
        windows = text[start:end]
        package = windows.index("- name: Retain Windows portable zip and test logs")
        test = windows.index("- name: Installer upgrade keeps the session host")
        logs = windows.index("- name: Retain installer survival diagnostics")
        self.assertLess(package, test)
        self.assertLess(test, logs)
        self.assertIn("if: always()", windows[logs:])
        self.assertIn("name: windows-x86_64-${{ needs.source.outputs.commit }}", windows[package:test])
        self.assertIn("name: windows-installer-logs-${{ needs.source.outputs.commit }}", windows[logs:])
        self.assertIn("path: artifacts/test-results/installer*", windows[logs:])
        self.assertNotIn("continue-on-error", windows[test:logs])
        self.assertIn("test_outside_job.py", windows)
        self.assertIn("HGS_TEST_INSTALLER_LOG_DIR", windows[test:logs])


if __name__ == "__main__":
    unittest.main()
