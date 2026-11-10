#!/usr/bin/env python3
"""Fails when a dot-like separator appears in the new web UI's sources.

Owner rule (2026-10-27): the new web UI never separates values with dots
(middle dot, bullet or similar); values sit apart through layout or line
breaks. Scope: design/webui and client only. The Qt GUI and the mobile
clients are not changed.
"""
import pathlib
import re
import sys

CHARS = "\u00b7\u2022\u2219\u22c5\u2027\u2981\u30fb"
PATTERN = re.compile("[" + CHARS + "]")
TEXT = {".ts", ".tsx", ".js", ".jsx", ".mjs", ".css", ".html", ".json", ".md", ".svg", ".rs", ".toml"}
SKIP = {"node_modules", "licenses", "dist", "target", "icons", ".git"}


def scan(roots):
    hits = []
    for root in roots:
        base = pathlib.Path(root)
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file() or path.suffix not in TEXT or SKIP & set(path.parts):
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for number, line in enumerate(text.splitlines(), 1):
                for match in PATTERN.finditer(line):
                    hits.append(f"{path}:{number}: U+{ord(match.group()):04X}: {line.strip()[:100]}")
    return hits


def main(argv):
    found = scan(argv or ["design/webui", "client"])
    if found:
        print("\n".join(found))
        print(f"{len(found)} separator dot(s): separate values with layout or line breaks, never dots.")
        return 1
    print("No separator dots in the web UI sources.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
