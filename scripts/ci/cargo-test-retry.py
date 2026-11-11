#!/usr/bin/env python3
"""Run a `cargo test` command and run the tests that failed once more.

WhiteWh fork (owner, 2026-11-11). Of the last thirty fork builds seventeen
failed, most of them on one test among about 4400: a millisecond bound that a
shared runner missed (#157), or a probe that read its fresh folder too early
(#158, #161). A whole build was lost each time, and the next dispatch met the
next one.

The command runs once with every test binary (`--no-fail-fast`), so one run
names every failure. Each test that failed is then run alone, by its exact
name. A test that fails again fails the step; one that passes on the second
run is reported as unstable - in the log, in the step summary and in a file
beside the log - and does not fail the step. A failure that names no test (a
compile error, a crashed test binary) is never retried.

Usage: cargo-test-retry.py --log PATH -- cargo test [arguments]
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

FAILED = re.compile(r"^test (\S+) \.\.\. FAILED$")
# A test that prints a child test run's own verdict (an isolated probe) puts
# that child's name into the log too; only the names cargo lists itself count.
LISTED = re.compile(r"^    ([A-Za-z0-9_:]+)$")
RESULT = re.compile(r"^test result: (\w+)\.")


def run(command: list[str], log: Path, mode: str) -> tuple[int, list[str]]:
    """Runs `command`, copying its output to the console and to `log`."""
    lines: list[str] = []
    with log.open(mode, encoding="utf-8", errors="replace", newline="\n") as kept:
        # The runner gives every step a synchronous pipe as stdin; hgs must not
        # inherit it.
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        assert process.stdout is not None
        for line in process.stdout:
            sys.stdout.write(line)
            sys.stdout.flush()
            kept.write(line)
            lines.append(line.rstrip("\r\n"))
        return process.wait(), lines


def failed_tests(lines: list[str]) -> list[str]:
    """The tests cargo reported as failed, in order, once each."""
    reported = [match[1] for line in lines if (match := FAILED.match(line))]
    listed = {match[1] for line in lines if (match := LISTED.match(line))}
    names = [name for name in reported if name in listed]
    return list(dict.fromkeys(names))


def retry_command(command: list[str], names: list[str]) -> list[str]:
    """`command` limited to `names` by their exact paths."""
    base = command[:command.index("--")] if "--" in command else list(command)
    return base + ["--", "--exact", *names]


def summarize(unstable: list[str], twice: list[str]) -> str:
    text = []
    if unstable:
        text.append(f"Unstable tests: {len(unstable)} failed and passed when run again alone. They did not fail this step; fix them.")
        text.extend(f"- {name}" for name in unstable)
    if twice:
        text.append(f"Failed twice: {len(twice)}")
        text.extend(f"- {name}" for name in twice)
    return "\n".join(text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    command = arguments.command[1:] if arguments.command[:1] == ["--"] else arguments.command
    if "test" not in command:
        parser.error("the command after -- is a cargo test command")
    if "--no-fail-fast" not in command:
        at = command.index("test") + 1
        command = command[:at] + ["--no-fail-fast"] + command[at:]
    arguments.log.parent.mkdir(parents=True, exist_ok=True)
    code, lines = run(command, arguments.log, "w")
    if code == 0:
        return 0
    names = failed_tests(lines)
    if not names:
        print("cargo-test-retry: the run failed without naming a test (a compile error or a crashed test binary); nothing is run again.")
        return code
    # A broken tree fails many tests; running hundreds again says nothing new.
    if len(names) > 40:
        print(f"cargo-test-retry: {len(names)} tests failed; that is no unstable test, nothing is run again.")
        return code
    print(f"\ncargo-test-retry: {len(names)} test(s) failed; running each again alone:")
    for name in names:
        print(f"  {name}")
    unstable, twice = [], []
    for name in names:
        again, _ = run(retry_command(command, [name]), arguments.log, "a")
        (unstable if again == 0 else twice).append(name)
    summary = summarize(unstable, twice)
    print("\ncargo-test-retry: " + summary.replace("\n", "\n  "))
    arguments.log.with_suffix(".unstable.txt").write_text(summary + "\n", encoding="utf-8", newline="\n")
    step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if step_summary:
        with open(step_summary, "a", encoding="utf-8", newline="\n") as kept:
            kept.write(summary + "\n")
    return 1 if twice else 0


if __name__ == "__main__":
    raise SystemExit(main())
