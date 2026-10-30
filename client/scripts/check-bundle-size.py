#!/usr/bin/env python3
"""Enforce the production HTML entry chunk's 500 kB (decimal) budget."""
import argparse
from html.parser import HTMLParser
from pathlib import Path
import sys
from urllib.parse import urlsplit

BUDGET = 500_000


class Entry(HTMLParser):
    def __init__(self):
        super().__init__()
        self.scripts = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "script" and attrs.get("type") == "module" and attrs.get("src"):
            self.scripts.append(attrs["src"])


def check(dist: Path) -> tuple[str, int]:
    entry = Entry()
    entry.feed((dist / "index.html").read_text(encoding="utf-8"))
    if len(entry.scripts) != 1:
        raise ValueError("Expected exactly one production module entry in index.html")
    url = urlsplit(entry.scripts[0])
    if url.scheme or url.netloc or url.query or url.fragment:
        raise ValueError("Expected a local production module entry")
    path = (dist / url.path).resolve()
    if not path.is_relative_to(dist.resolve()) or not path.is_file():
        raise ValueError("Production module entry is missing or outside dist")
    size = path.stat().st_size
    if not size or size > BUDGET:
        raise ValueError(f"{path.name}: {size:,} bytes; main chunk budget is {BUDGET:,} bytes")
    return path.name, size


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, default=Path(__file__).resolve().parent.parent / "dist")
    args = parser.parse_args()
    try:
        name, size = check(args.dist)
    except (OSError, ValueError) as error:
        print(f"Bundle size check failed: {error}", file=sys.stderr)
        return 1
    print(f"{name}: {size:,} / {BUDGET:,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
