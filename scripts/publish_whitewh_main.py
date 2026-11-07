#!/usr/bin/env python3
"""Task 157: the only action that moves whitewh/main.

It refuses unless .zerus-release/<commit>.json says both legs are green for
exactly that commit: the local release candidate and the whitewh-exp-* runner
workflow. A green local run alone never publishes.

    python scripts/publish_whitewh_main.py --commit <sha> [--dry-run]
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / ".zerus-release"


def gate(path: Path, commit: str) -> tuple[bool, list[str]]:
    """True only for a record of this commit with both legs green."""
    errors = []
    if not path.is_file():
        return False, [f"no release-candidate evidence at {path}"]
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return False, [f"{path}: {error}"]
    if data.get("commit") != commit:
        errors.append(f"evidence names {data.get('commit')}, not {commit}")
    for leg in ("local", "experimental"):
        entry = data.get(leg)
        if not isinstance(entry, dict):
            errors.append(f"the {leg} leg did not run")
        elif entry.get("status") != "green":
            errors.append(f"the {leg} leg is {entry.get('status')!r}, not green")
    return not errors, errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    commit = subprocess.run(["git", "rev-parse", args.commit], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    ok, errors = gate(EVIDENCE / f"{commit}.json", commit)
    if not ok:
        print("refused: whitewh/main does not move", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 2
    if args.dry_run:
        print(f"would push {commit} to whitewh/main")
        return 0
    subprocess.run(["git", "push", "origin", f"{commit}:refs/heads/whitewh/main"], cwd=ROOT, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

