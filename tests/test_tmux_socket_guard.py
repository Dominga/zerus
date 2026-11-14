"""The tmux socket guard for test and CI sources (zerus-63rru.89).

Every harness that starts tmux must name an explicit -S socket inside a
directory it creates and owns; tmux otherwise falls back to
/tmp/tmux-UID/default, the real server of every agent session on the
machine, and a kill-server there ends them all. This test scans the test
and CI sources and fails on any tmux invocation that names a subcommand or
a shim exec without -S, with the file and line.
"""
import os
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# An invocation of the real tmux whose argv the line builds: a shim's exec,
# a direct [TMUX, ...] / ("tmux", ...) argv, or the Rust Command::new.
INVOCATION = [
    re.compile(r"\bexec\s+(\S*/)?tmux\b"),
    re.compile(r"subprocess\.\w+\(\[TMUX\s*,"),
    re.compile(r"\[fixtures\.TMUX"),
    re.compile(r"run\(\"tmux\"\s*,"),
    re.compile(r"Command::new\(\"tmux\"\)"),
]

# Files that may invoke tmux without an explicit -S on the line: the
# product's own backend resolves its socket from TMUX_TMPDIR (never -S).
ALLOWED = {
    "src/state/backend/tmux.rs": "the product's tmux backend resolves its socket from TMUX_TMPDIR",
}


def scanned():
    files = list((ROOT / "tests").glob("*.py")) + list((ROOT / "scripts").glob("*.py"))
    files += [p for p in (ROOT / "scripts" / "ci").rglob("*") if p.is_file()]
    files += [p for p in (ROOT / "src").rglob("tests.rs")]
    files += [p for p in (ROOT / "src").rglob("tests") if p.is_dir() for p in p.glob("*.rs")]
    return sorted({f for f in files if "tmux" in f.read_text(errors="ignore")}, key=str)


def violations(files):
    found = []
    for file in files:
        if file == Path(__file__).resolve():
            continue  # the guard's own control snippet is the point
        rel = (
            file.relative_to(ROOT).as_posix()
            if file.is_relative_to(ROOT)
            else file.name
        )
        if rel in ALLOWED:
            continue
        lines = file.read_text(errors="ignore").splitlines()
        for number, line in enumerate(lines, 1):
            # A version probe names no server and no socket.
            if '"tmux", "-V"' in line or '"-V"' in line:
                continue
            if any(pattern.search(line) for pattern in INVOCATION) and "-S" not in line:
                # A Rust Command::new chains its args on the following lines.
                window = "\n".join(lines[number - 1 : number + 4])
                if 'Command::new("tmux")' in line and "-S" in window:
                    continue
                found.append((rel, number, line.strip()))
    return found


class TmuxSocketGuard(unittest.TestCase):
    def test_every_tmux_invocation_names_an_explicit_socket(self):
        found = violations(scanned())
        self.assertEqual(
            found,
            [],
            "tmux invocations without an explicit -S:\n"
            + "\n".join(f"{f}:{n}: {t}" for f, n, t in found),
        )

    def test_the_guard_fails_on_a_reverted_helper(self):
        # The control: a shim that execs tmux without -S is exactly the
        # reverted helper the guard exists to catch.
        import tempfile

        with tempfile.TemporaryDirectory(prefix="hgs-tmux-guard-control-") as temp:
            path = Path(temp) / "reverted.py"
            path.write_text('#!/bin/sh\nexec /usr/bin/tmux "$@"\n')
            self.assertEqual(
                [(number, text) for _, number, text in violations([path])],
                [(2, 'exec /usr/bin/tmux "$@"')],
                "the guard must fail on a reverted helper",
            )


if __name__ == "__main__":
    unittest.main()
