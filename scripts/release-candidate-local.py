#!/usr/bin/env python3
"""Task 157: the Windows half of the release candidate.

Runs the fork-windows steps after the Qt and fork tests: the Tauri build with
the fork channel, the package and the portable self-test. The paths are the
workflow's; --dry-run prints them.
"""
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], cwd: Path = ROOT, env: dict[str, str] | None = None, dry: bool = False) -> None:
    print("+", " ".join(command), flush=True)
    if dry:
        return
    merged = dict(os.environ)
    merged.update(env or {})
    # On Windows npm is npm.cmd: CreateProcess finds only .exe by a bare
    # name, so the program is resolved on PATH first (task 169).
    program = shutil.which(command[0], path=merged.get("PATH")) or command[0]
    if subprocess.run([program, *command[1:]], cwd=cwd, env=merged).returncode != 0:
        raise SystemExit(f"release candidate step failed: {' '.join(command)}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--qt-root", default=os.environ.get("QT_ROOT_DIR", r"D:/Tools/zerus-deps/Qt/6.10.3/msvc2022_64"))
    args = parser.parse_args()
    run(["cargo", "build", "--locked", "--release", "--bin", "hgs"], dry=args.dry_run)
    run(["cargo", "build", "--locked", "--release", "--manifest-path", "services/mobile/connector/Cargo.toml"], dry=args.dry_run)
    run(["cargo", "build", "--locked", "--release", "--manifest-path", "services/mobile/relay/Cargo.toml"], dry=args.dry_run)
    conpty = Path(os.environ.get("RUNNER_TEMP", ROOT / ".zerus-team/tmp")) / "conpty"
    if not conpty.exists():
        run(["python", "scripts/fetch-conpty.py", "--output", str(conpty)], dry=args.dry_run)
    results = ROOT / "artifacts/test-results"
    results.mkdir(parents=True, exist_ok=True)
    client = ROOT / "client"
    target = ROOT / "target/windows-next"
    run(["npm", "ci"], cwd=client, dry=args.dry_run)
    run(
        ["npm", "run", "tauri", "--", "build", "--ci", "--no-bundle",
         "--config", "src-tauri/tauri.windows.conf.json", "--features", "video"],
        cwd=client,
        env={"CARGO_TARGET_DIR": str(target), "CARGO_BUILD_JOBS": "4",
             "ZERUS_FORK_BUILD": "1",
             "ZERUS_FORK_FEED_URL": os.environ.get("ZERUS_FORK_FEED_URL", "")},
        dry=args.dry_run,
    )
    run(["python", "client/scripts/collect-notices.py", "artifacts/windows-frontend-licenses"], dry=args.dry_run)
    run(["python", "scripts/package-windows.py", "--fork",
         "--cli", "target/release/hgs.exe",
         "--gui", "target/windows-next/release/hgs-tray.exe",
         "--connector", "services/mobile/connector/target/release/zerus-mobile-connector.exe",
         "--mobile-relay", "services/mobile/relay/target/release/zerus-relay.exe",
         "--qt-fallback", ".ci-build/gui/hgs-tray.exe", "--qt-root", args.qt_root,
         "--frontend-licenses", "artifacts/windows-frontend-licenses",
         "--conpty", str(Path(os.environ.get("RUNNER_TEMP", ROOT / ".zerus-team/tmp")) / "conpty"),
         "--output", "artifacts/zerus-windows-x86_64.zip"], dry=args.dry_run)
    run(["python", "scripts/check-windows-portable.py", "artifacts/zerus-windows-x86_64.zip",
         "--run", "--selftest"], dry=args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


