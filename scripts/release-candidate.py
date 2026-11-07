#!/usr/bin/env python3
"""Task 157: the fork release candidate, run on the Windows worker.

It runs the same steps as .github/workflows/fork-windows.yml after the Rust
suite, records their result, and with --exp-tag also pushes a whitewh-exp-*
tag for the exact commit and waits for that workflow. Both legs must be green
before scripts/publish-whitewh-main.py may move whitewh/main.

    python scripts/release-candidate.py --commit <sha> [--exp-tag <name>]
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / ".zerus-release"


def run(command: list[str], env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    merged = dict(os.environ)
    merged.update(env or {})
    completed = subprocess.run(command, cwd=ROOT, env=merged)
    if completed.returncode != 0:
        raise SystemExit(f"release candidate step failed: {' '.join(command)}")


def record(commit: str, leg: str, status: str, detail: str = "") -> None:
    EVIDENCE.mkdir(exist_ok=True)
    path = EVIDENCE / f"{commit}.json"
    data = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {"commit": commit}
    data[leg] = {"status": status, "detail": detail, "at": int(time.time())}
    path.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")


def local_steps(commit: str) -> None:
    run([sys.executable, "scripts/check-machine-dependent-tests.py"])
    # The same order as fork-windows.yml after "Rust tests and release build".
    run(["bash", "scripts/ci/gui.sh", "test"], {"ZERUS_QT_RETRY": "2", "ZERUS_TEST_HGS": str(ROOT / "target/release/hgs.exe")})
    run(["cargo", "test", "--locked", "--manifest-path", "client/core/Cargo.toml", "--lib", "fork_update"])
    run(["cargo", "test", "--locked", "--manifest-path", "client/core/Cargo.toml", "--test", "fork_handoff"])
    # Tauri build and the portable zip are run through their scripts so this
    # file has no second copy of the flags.
    run([sys.executable, "scripts/release-candidate-local.py", "--commit", commit])


def push_exp_tag(commit: str, tag: str) -> None:
    run(["git", "tag", "-f", tag, commit])
    run(["git", "push", "origin", f"refs/tags/{tag}"])


def wait_exp(tag: str, commit: str, timeout_minutes: int = 150) -> None:
    deadline = time.time() + timeout_minutes * 60
    print(f"waiting for the {tag} workflow on {commit}...", flush=True)
    while time.time() < deadline:
        output = subprocess.run(
            ["gh", "run", "list", "--workflow", "fork-windows.yml", "--commit", commit,
             "--json", "databaseId,headSha,status,conclusion,url", "--limit", "20"],
            cwd=ROOT, capture_output=True, text=True)
        if output.returncode == 0:
            for run in json.loads(output.stdout or "[]"):
                if run.get("headSha") == commit and run.get("status") == "completed":
                    if run.get("conclusion") != "success":
                        raise SystemExit(f"experimental run failed: {run.get('url')}")
                    return
        time.sleep(30)
    raise SystemExit("timed out waiting for the experimental workflow")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--exp-tag", default="")
    args = parser.parse_args()
    commit = subprocess.run(["git", "rev-parse", args.commit], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    local_steps(commit)
    record(commit, "local", "green")
    if args.exp_tag:
        push_exp_tag(commit, args.exp_tag)
        wait_exp(args.exp_tag, commit)
        record(commit, "experimental", "green", args.exp_tag)
    else:
        print("no --exp-tag: the runner leg is not green yet; publishing stays refused")
    print(f"release candidate: {commit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())


