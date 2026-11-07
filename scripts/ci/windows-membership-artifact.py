#!/usr/bin/env python3
"""Opt-in test fixture only. No publisher consumes this artifact or manifest."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "artifacts/membership-qualification-windows"
EXE = "membership-tests.exe"
CASES = [
    "platform::windows_membership::native::file_users::tests::native_disk_query_reports_positive_usage_without_copying_a_foreign_handle",
    "platform::windows_membership::native::file_users::tests::private_native_observations_do_not_turn_coverage_gaps_into_clear",
    "platform::windows_membership::native::resident::tests::native_original_exit_without_eof_cannot_release_admission_or_adopt_old_data",
    "platform::windows_membership::native::qualification::private_measurement",
    "platform::windows_membership::native::qualification::hosted_measurement",
]


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(65536), b""):
            result.update(block)
    return result.hexdigest()


def linked(path):
    return path.is_symlink() or (hasattr(path, "is_junction") and path.is_junction())


def select(messages, root):
    candidates = []
    for line in messages.splitlines():
        message = json.loads(line)
        if (message.get("reason") == "compiler-artifact"
                and message.get("target", {}).get("name") == "hgs"
                and message.get("target", {}).get("kind") == ["bin"]
                and message.get("profile", {}).get("test") is True
                and message.get("profile", {}).get("opt_level") == "3"
                and message.get("profile", {}).get("debug_assertions") is False
                and Path(message["manifest_path"]).resolve() == root / "Cargo.toml"
                and message.get("executable")):
            candidates.append(Path(message["executable"]))
    if len(candidates) != 1:
        raise ValueError("Expected one exact root test executable")
    path = candidates[0]
    parent = root / "target/release/deps"
    if (linked(path) or path.parent != parent
            or not re.fullmatch(r"hgs-[0-9a-f]+\.exe", path.name)
            or any(linked(part) for part in [root / "target", root / "target/release", parent])):
        raise ValueError("Unexpected test executable location")
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or not 2 <= info.st_size <= 1 << 30:
        raise ValueError("Invalid test executable")
    with path.open("rb") as file:
        if file.read(2) != b"MZ":
            raise ValueError("Expected a Windows executable")
    return path


def package(source, sha, output):
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Expected exact source SHA")
    if any(linked(parent) for parent in output.parents):
        raise ValueError("Qualification output parent is linked")
    output.mkdir(parents=True, exist_ok=False)
    before = digest(source)
    target = output / EXE
    shutil.copyfile(source, target)
    if digest(source) != before or digest(target) != before:
        raise ValueError("Qualification executable changed during copy")
    manifest = {
        "schema": 1,
        "kind": "read-only-membership-test-fixture",
        "sourceCommit": sha,
        "buildProfile": "release",
        "executable": {"name": EXE, "sha256": before, "size": target.stat().st_size},
        "allowedExactCases": CASES,
        "ignoredExactCases": CASES[-2:],
        "arguments": ["--exact", "--nocapture", "--test-threads=1"],
        "inspectionBudgetSelected": False,
        "removalQualified": False,
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main():
    if os.name != "nt":
        raise SystemExit("Build the qualification fixture on native Windows only")
    sha = os.environ["QUALIFICATION_SOURCE"]
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if sha != actual or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise SystemExit("Qualification source does not match checkout")
    def require_clean():
        current = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True)
        if current != sha or dirty.strip():
            raise SystemExit("Qualification source changed or is dirty")
    require_clean()
    # File-backed compiler output avoids inherited-pipe waits after a timeout.
    # The workflow additionally keeps the whole owned build tree in outside-job.
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        built = subprocess.run(
            ["cargo", "+1.99.1", "test", "--release", "--locked", "--bin", "hgs", "--no-run", "--message-format=json"],
            cwd=ROOT, stdout=output, stderr=errors, timeout=1800, check=False,
        )
        if built.returncode:
            errors.seek(0, 2)
            errors.seek(max(0, errors.tell() - 8192))
            print(errors.read(8192).decode("utf-8", errors="replace"))
            raise SystemExit("Qualification compilation failed")
        if output.tell() > 64 << 20:
            raise SystemExit("Compiler record bound exceeded")
        output.seek(0)
        messages = output.read(64 << 20).decode("utf-8")
    require_clean()
    package(select(messages, ROOT), sha, OUTPUT)


if __name__ == "__main__":
    main()
