#!/usr/bin/env python3
"""Task 157: a machine-dependent test must probe and skip, never assume.

Scans Rust test functions that run Visual Studio, an Unreal engine, the
registry, a GPU tool or the network. A function that names one of those and has
no `skip`, `ignore`, `available` or `probe` nearby is reported; the release
candidate runs this before it builds.

    python scripts/check-machine-dependent-tests.py [--root DIR]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

MACHINE = re.compile(
    r"vswhere|reg(?:\.exe)?\s+query|UnrealEditor|Build\.bat|msbuild|cl\.exe|"
    r"nvidia-smi|vulkaninfo|/dev/dri",
    re.IGNORECASE,
)
PROBE = re.compile(r"\b(skip|ignored?|available|probe|probed)\b", re.IGNORECASE)
TEST = re.compile(r"#\[(?:tokio::)?test\]")


def functions(text: str):
    """(name, body) for every #[test] fn, by brace counting."""
    for found in TEST.finditer(text):
        start = text.find("fn ", found.end())
        if start < 0:
            continue
        name = re.match(r"fn\s+([A-Za-z0-9_]+)", text[start:])
        brace = text.find("{", start)
        if not name or brace < 0:
            continue
        depth = 0
        for index in range(brace, len(text)):
            depth += text[index] == "{"
            depth -= text[index] == "}"
            if depth == 0:
                yield name.group(1), text[brace : index + 1]
                break


def violations(root: Path) -> list[str]:
    out = []
    for path in sorted(root.rglob("*.rs")):
        if any(part in {"target", "node_modules", ".git"} for part in path.parts):
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for name, body in functions(text):
            if "Command::new" in body and MACHINE.search(body) and not PROBE.search(body):
                out.append(f"{path.relative_to(root)}: {name} uses a machine-dependent tool without a probe/skip")
    return out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=str(Path(__file__).resolve().parents[1]))
    args = parser.parse_args()
    found = violations(Path(args.root))
    for line in found:
        print(line)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())


