#!/usr/bin/env python3
"""Opt-in hosted cost DATA, not unelevated or removal qualification."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "artifacts/membership-qualification-windows"
OUTPUT = ROOT / "artifacts/membership-hosted-timing"
CASE = "platform::windows_membership::native::qualification::hosted_measurement"
MARKER = "file-users-report-v1="


def record(text):
    lines = [line[len(MARKER):] for line in text.splitlines() if line.startswith(MARKER)]
    if len(lines) != 1:
        raise ValueError("Expected one exact hosted measurement record")
    value = json.loads(lines[0])
    if value.get("scope") != "hosted-cost-only":
        raise ValueError("Wrong measurement scope")
    return value


def environment(scope):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    path = ctypes.create_unicode_buffer(32768)
    filesystem = ctypes.create_unicode_buffer(64)
    get_path = kernel.GetVolumePathNameW
    get_path.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_ulong]
    get_path.restype = ctypes.c_int
    get_info = kernel.GetVolumeInformationW
    get_info.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_ulong,
                         ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                         ctypes.c_wchar_p, ctypes.c_ulong]
    get_info.restype = ctypes.c_int
    known = get_path(str(scope), path, len(path)) and get_info(
        path.value, None, 0, None, None, None, filesystem, len(filesystem))
    return {"osVersion": list(sys.getwindowsversion().platform_version),
            "pointerBits": ctypes.sizeof(ctypes.c_void_p) * 8,
            "filesystem": filesystem.value if known else "unknown"}


def main():
    if (os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"):
        raise SystemExit("This diagnostic runs on opted-in hosted Windows only")
    manifest = json.loads((FIXTURE / "manifest.json").read_text())
    source = os.environ["QUALIFICATION_SOURCE"]
    exe = FIXTURE / "membership-tests.exe"
    with exe.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    if (manifest["sourceCommit"] != source or manifest["buildProfile"] != "release"
            or manifest["executable"]["sha256"] != digest
            or manifest["executable"]["size"] != exe.stat().st_size
            or CASE not in manifest["ignoredExactCases"]):
        raise SystemExit("Hosted fixture source/profile/image mismatch")
    OUTPUT.mkdir(parents=True, exist_ok=False)
    tick = ctypes.WinDLL("kernel32", use_last_error=True).GetTickCount64
    tick.restype = ctypes.c_ulonglong
    tick.argtypes = []
    deadline = time.monotonic() + 600
    summary = {"source": source, "buildProfile": "release", "scope": "hosted-cost-only",
               "unelevatedQualified": False, "removalQualified": False, "completed": False, "cases": []}
    (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    nonce = str(uuid.uuid4())
    with tempfile.TemporaryDirectory(prefix="zerus-hosted-membership-") as temporary:
        scope = Path(temporary)
        summary["environment"] = environment(scope)
        for count in [128, 4096]:
            for phase in ["prepare", "first-pass", "warm"]:
                remaining = min(240, deadline - time.monotonic())
                if remaining <= 0:
                    raise SystemExit("Original hosted diagnostic bound exhausted")
                until_tick = tick() + int(remaining * 1000)
                env = os.environ | {"ZERUS_MEMBERSHIP_QUALIFICATION": "readonly-v1",
                      "ZERUS_MEMBERSHIP_SCOPE": str(scope), "ZERUS_MEMBERSHIP_NONCE": nonce,
                      "ZERUS_MEMBERSHIP_OBJECTS": str(count), "ZERUS_MEMBERSHIP_PASS": phase,
                      "ZERUS_MEMBERSHIP_UNTIL_TICK": str(until_tick), "HGS_HOME": str(scope / "hgs")}
                started = time.monotonic()
                # Output is file-backed: inherited writers cannot keep a read
                # open. outside-job retains the whole original owned tree.
                with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
                    child = subprocess.Popen([str(exe), CASE, "--exact", "--ignored", "--nocapture",
                                               "--test-threads=1"], env=env, cwd=scope,
                                              stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                              creationflags=subprocess.CREATE_NO_WINDOW)
                    try:
                        result = child.wait(timeout=max(0.01, remaining + 5 - (time.monotonic() - started)))
                    except subprocess.TimeoutExpired:
                        child.kill()  # This original Popen only, never a PID lookup.
                        child.wait(timeout=10)
                        raise SystemExit("Original hosted case timed out; no timing admission")
                    overflow = os.fstat(stdout.fileno()).st_size > 65536 or os.fstat(stderr.fileno()).st_size > 65536
                    stdout.seek(0)
                    stderr.seek(0)
                    text = stdout.read(65536).decode("utf-8", errors="strict")
                    errors = stderr.read(65536)
                (OUTPUT / f"case-{count}-{phase}.json").write_text(json.dumps({
                    "scope": "hosted-cost-only", "originalExit": result, "overflow": overflow,
                    "stdout": text, "stderr": errors.decode("utf-8", errors="replace")}) + "\n")
                if overflow:
                    raise SystemExit("Hosted output bound exceeded; bounded diagnostics retained")
                if result != 0 or errors:
                    raise SystemExit("Original hosted case refused; private job log retained")
                value = record(text)
                expected_kind = "private-fixture-prepared-v1" if phase == "prepare" else "usage-timing-data-only-v1"
                if (value.get("files") != count or value.get("kind") != expected_kind
                        or (phase != "prepare" and value.get("pass") != phase)):
                    raise SystemExit("Original hosted case binding differs")
                summary["cases"].append({"files": count, "pass": phase,
                                         "wholeProcess_ms": (time.monotonic() - started) * 1000,
                                         "originalExit": result, "record": value})
                (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(json.dumps({"files": count, "pass": phase, "scope": "hosted-cost-only",
                                  "record": value}), flush=True)
    summary["privateFixtureRemoved"] = True
    summary["completed"] = True
    (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
