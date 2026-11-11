"""scripts/ci/cargo-test-retry.py: a test that fails is run again alone; only one
that fails twice fails the step (WhiteWh fork, owner 2026-11-11)."""

import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/ci/cargo-test-retry.py"

# A stand-in for cargo: `plan` maps a test to the runs in which it fails; a run
# with `-- --exact NAME` runs that test only. `crash` fails without naming one.
FAKE = textwrap.dedent('''
    import json, sys
    from pathlib import Path
    here = Path(__file__).parent
    plan = json.loads((here / "plan.json").read_text())
    calls = here / "calls.txt"
    arguments = sys.argv[1:]
    with calls.open("a") as kept:
        kept.write(" ".join(arguments) + "\\n")
    if plan.get("crash"):
        print("error[E0425]: cannot find value `x` in this scope")
        sys.exit(101)
    names = arguments[arguments.index("--exact") + 1:] if "--exact" in arguments else sorted(plan["tests"])
    counts = here / "counts.json"
    seen = json.loads(counts.read_text()) if counts.exists() else {}
    failed = []
    print(f"running {len(names)} tests")
    for name in names:
        seen[name] = seen.get(name, 0) + 1
        fails = seen[name] in plan["tests"].get(name, [])
        print(f"test {name} ... {'FAILED' if fails else 'ok'}")
        if fails:
            failed.append(name)
    counts.write_text(json.dumps(seen))
    if failed:
        print("\\nfailures:\\n")
        for name in failed:
            print(f"---- {name} stdout ----")
            # A probe's own verdict inside a failing test's output is no test of this run.
            print("test nested::child_probe ... FAILED")
        print("\\nfailures:")
        for name in failed:
            print(f"    {name}")
        print(f"\\ntest result: FAILED. {len(names) - len(failed)} passed; {len(failed)} failed")
        sys.exit(101)
    print(f"\\ntest result: ok. {len(names)} passed; 0 failed")
''')


class CargoTestRetry(unittest.TestCase):
    def run_with(self, plan):
        import json
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        folder = Path(directory.name)
        (folder / "cargo.py").write_text(FAKE, encoding="utf-8")
        (folder / "plan.json").write_text(json.dumps(plan), encoding="utf-8")
        log = folder / "results" / "rust.log"
        done = subprocess.run(
            [sys.executable, str(SCRIPT), "--log", str(log), "--", sys.executable, str(folder / "cargo.py"), "test", "--locked"],
            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120, env={"PATH": "", "SYSTEMROOT": __import__("os").environ.get("SYSTEMROOT", "")},
        )
        calls = (folder / "calls.txt").read_text().splitlines()
        return done, calls, log

    def test_a_clean_run_is_one_run(self):
        done, calls, log = self.run_with({"tests": {"a::one": [], "a::two": []}})
        self.assertEqual(done.returncode, 0, done.stdout)
        # Every test binary runs, so one run names every failure.
        self.assertEqual(calls, ["test --no-fail-fast --locked"])
        self.assertIn("test result: ok. 2 passed", log.read_text())

    def test_a_test_that_passes_alone_is_reported_and_does_not_fail_the_step(self):
        done, calls, log = self.run_with({"tests": {"a::flaky": [1], "a::steady": []}})
        self.assertEqual(done.returncode, 0, done.stdout)
        self.assertEqual(calls, ["test --no-fail-fast --locked", "test --no-fail-fast --locked -- --exact a::flaky"])
        self.assertIn("Unstable tests: 1 failed and passed when run again alone", done.stdout)
        self.assertEqual(log.with_suffix(".unstable.txt").read_text().splitlines()[1], "- a::flaky")
        # The probe's verdict printed inside the failure is not run again.
        self.assertNotIn("nested::child_probe", "\n".join(calls))
        # The second run's output is kept after the first.
        self.assertEqual(log.read_text().count("running "), 2)

    def test_a_test_that_fails_twice_fails_the_step(self):
        done, calls, _ = self.run_with({"tests": {"a::broken": [1, 2], "a::flaky": [1], "a::steady": []}})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertEqual(len(calls), 3)
        self.assertIn("Failed twice: 1", done.stdout)
        self.assertIn("- a::broken", done.stdout)
        self.assertIn("Unstable tests: 1", done.stdout)

    def test_a_failure_that_names_no_test_is_not_run_again(self):
        done, calls, _ = self.run_with({"crash": True, "tests": {}})
        self.assertEqual(done.returncode, 101, done.stdout)
        self.assertEqual(len(calls), 1)
        self.assertIn("without naming a test", done.stdout)

    def test_a_broken_tree_is_not_run_again(self):
        done, calls, _ = self.run_with({"tests": {f"a::t{n}": [1, 2] for n in range(41)}})
        self.assertEqual(done.returncode, 101, done.stdout)
        self.assertEqual(len(calls), 1)
        self.assertIn("41 tests failed", done.stdout)


if __name__ == "__main__":
    unittest.main()
