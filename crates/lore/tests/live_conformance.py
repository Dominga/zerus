"""A live conformance run: the Python CodeLore CLI and the Rust `lore` binary
on the same fixture lore, comparing what each prints. Run from the workspace
with `hgs run`; the Python tool is D:/Tools/CodeLore.
"""
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(r"D:\Tools\Zerus-team-33fbafeb-hgs")
RUST = HERE / "crates" / "lore" / "target" / "debug" / "lore.exe"
PY = Path(r"D:\Tools\CodeLore\src")
WORK = HERE / ".zerus-team" / "live"
PYTHON = sys.executable or "python"
ENV = dict(os.environ, PYTHONPATH=str(PY), LORE_CACHE=str(HERE / ".zerus-team" / "live" / "cache"))

FAULT = {
    "title": "the atlas checksum mismatch",
    "area": "general",
    "symptom": "the nightly staging step aborts with a checksum mismatch on a texture atlas",
    "cause": "a half-written atlas from an interrupted run is reused because its stamp file survived",
    "fix": "delete the stale stamp file and re-import the atlas whole",
    "check": "the staging step finishes and the atlas hash matches the manifest",
    "signals": ["E_ASSET_STAMP_STALE", "atlas checksum mismatch"],
}
DECISION = {
    "title": "keep the index disposable",
    "area": "general",
    "question": "may the index be deleted safely?",
    "choice": "yes: it is rebuilt from the JSONL on demand and never committed",
    "reason": "the JSONL files are the source of truth",
    "decided_by": "owner",
}
DOC = "## Setup\n\nRun the tools once per session.\n\n## Faults | a hang on exit\n\nIt hangs.\n"


def git(root, *args):
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


def run(side, root):
    if side == "rust":
        command = [str(RUST), "-C", str(root)]
    else:
        command = [PYTHON, "-m", "codelore", "-C", str(root)]
    return command


def call(side, root, argv, stdin=""):
    command = run(side, root) + argv
    result = subprocess.run(command, input=stdin.encode("utf-8"), capture_output=True, env=ENV)
    return result.returncode, result.stdout.decode("utf-8", "replace"), result.stderr.decode("utf-8", "replace")


def fixture(side):
    root = WORK / side
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True, exist_ok=True)
    git(root, "init", "-q", "-b", "main")
    git(root, "config", "user.name", "tester")
    git(root, "config", "user.email", "t@example.com")
    git(root, "config", "core.autocrlf", "false")
    (root / "README.md").write_text("# fixture\n", encoding="utf-8")
    (root / "docs").mkdir()
    (root / "docs" / "hand.md").write_text(DOC, encoding="utf-8")
    git(root, "add", "README.md", "docs/hand.md")
    git(root, "commit", "-qm", "fixture")
    return root


def clean(text):
    text = text.replace("\r\n", "\n")
    text = re.sub(r"\b[0-9a-f]{40}\b", "<sha>", text)
    text = re.sub(r"\b[0-9a-f]{10}\b", "<sha10>", text)
    return text


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    results = {}
    for side in ("python", "rust"):
        root = fixture(side)
        out = {}
        code, stdout, stderr = call(side, root, ["init", "--areas", "general,ops"])
        out["init"] = (code, clean(stdout), clean(stderr))
        out["lore.toml"] = (root / "lore.toml").read_text(encoding="utf-8")
        out["gitattributes"] = (root / ".gitattributes").read_text(encoding="utf-8")
        code, stdout, stderr = call(side, root, ["add", "fault", "--stdin", "--new", "--by", "tester"], json.dumps(FAULT))
        out["add"] = (code, clean(stdout), clean(stderr))
        code, stdout, stderr = call(side, root, ["add", "decision", "--stdin", "--new", "--by", "tester"], json.dumps(DECISION))
        out["add2"] = (code, clean(stdout), clean(stderr))
        for name, argv in [
            ("find", ["find", "atlas checksum mismatch", "--json", "--text-only"]),
            ("plans", ["plans", "--json"]),
            ("items", ["items", "--json"]),
            ("for", ["for", "docs/hand.md", "--json"]),
            ("stale", ["stale", "--json"]),
            ("check", ["check", "--json"]),
            ("cover", ["cover", "docs/hand.md", "--json"]),
            ("guide", ["guide"]),
        ]:
            code, stdout, stderr = call(side, root, argv)
            out[name] = (code, clean(stdout), clean(stderr))
        results[side] = out
    failures = 0
    for name in results["python"]:
        a = results["python"][name]
        b = results["rust"][name]
        if a != b:
            failures += 1
            print(f"DIFF {name}")
            for label, x, y in (("python", a, b),):
                print(f"  {label} code={x[0]}")
                if isinstance(x[1], str) and len(x[1]) < 4000:
                    print("   out:", x[1].replace("\n", "\\n")[:1000])
                    print("   err:", x[2].replace("\n", "\\n")[:300])
                x2, y2 = x, y
                print("   rust  code=", y[0])
                print("   out:", y[1].replace("\n", "\\n")[:1000])
                print("   err:", y[2].replace("\n", "\\n")[:300])
    print(f"{len(results['python']) - failures} of {len(results['python'])} outputs equal")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
