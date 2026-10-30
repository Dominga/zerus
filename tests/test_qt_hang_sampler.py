import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "ci" / "qt-hang-sampler.py"


class SamplerTerminationTests(unittest.TestCase):
    def children(self, pid):
        result = subprocess.run(["ps", "-o", "pid=,comm=", "--ppid", str(pid)], capture_output=True, text=True)
        return [(int(line.split()[0]), line.split()[1]) for line in result.stdout.splitlines() if line.split()]

    def assertGone(self, pid):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        status = subprocess.check_output(["ps", "-p", str(pid), "-o", "stat="], text=True)
        self.assertTrue(status.startswith("Z"), f"PID {pid} is still running: {status.strip()}")

    def wrapper(self, command, after="30", log=None):
        arguments = [sys.executable, str(SCRIPT), "--name", "fixture", "--after", after]
        if log is not None:
            arguments += ["--log", log]
        process = subprocess.Popen(
            [*arguments, "--", *command],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        time.sleep(0.15)
        expected = Path(command[0]).name
        children = [pid for pid, name in self.children(process.pid)
                    if name == expected or (expected.startswith("python") and name.startswith("python"))]
        self.assertLessEqual(len(children), 1)
        return process, children[0] if children else None

    def finish(self, process, child, sent=None):
        if sent is not None:
            os.kill(process.pid, sent)
        output, _ = process.communicate(timeout=8)
        if child is not None:
            self.assertGone(child)
        return process.returncode, output

    def test_sigterm_and_sigint_reap_a_graceful_child_and_reraise_the_signal(self):
        script = "import signal,time\nsignal.signal(signal.SIGTERM,lambda *_: exit(0))\nsignal.signal(signal.SIGINT,lambda *_: exit(0))\ntime.sleep(30)"
        for sent in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signal=sent):
                process, child = self.wrapper([sys.executable, "-c", script])
                code, _ = self.finish(process, child, sent)
                self.assertEqual(code, -sent)

    def test_a_stubborn_child_is_killed_and_sigterm_is_reraised(self):
        script = "import signal,time\nsignal.signal(signal.SIGTERM,signal.SIG_IGN)\nsignal.signal(signal.SIGINT,signal.SIG_IGN)\ntime.sleep(30)"
        process, child = self.wrapper([sys.executable, "-c", script])
        code, _ = self.finish(process, child, signal.SIGTERM)
        self.assertEqual(code, -signal.SIGTERM)

    def test_a_childs_own_signal_is_reraised_without_an_outer_signal(self):
        process, child = self.wrapper(["bash", "-c", "kill -TERM $$"])
        code, _ = self.finish(process, child)
        self.assertEqual(code, -signal.SIGTERM)

    def test_an_exception_kills_the_child(self):
        process, child = self.wrapper(["sleep", "30"], after="0.05", log="/proc/zerus-hang-test.log")
        code, _ = self.finish(process, child)
        self.assertNotEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
