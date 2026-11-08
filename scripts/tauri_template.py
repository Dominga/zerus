#!/usr/bin/env python3
"""The Zerus Tauri templates: build, dev and launch with the development identity.

The build and dev commands run through the Tauri CLI and the default
`tauri.windows.conf.json` (273's dev identity). The launch command reads the
binary's `--build-info` and refuses a switch-identity build: only the
release-candidate/package step builds `com.hgdev.hgs-tray`, outside this
template. Run as `python scripts/tauri_template.py build|dev|launch`.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLIENT = ROOT / "client"
SWITCH_ID = "com.hgdev.hgs-tray"
DEV_ID_PREFIX = "dev.zerus.next"


def binary() -> Path:
    name = "zerus-client.exe" if sys.platform == "win32" else "zerus-client"
    return CLIENT / "src-tauri" / "target" / "release" / name


def identity_allows_launch(info: dict) -> bool:
    """273: only a development build may be launched outside the package."""
    identifier = str(info.get("CFBundleIdentifier", ""))
    return (
        info.get("buildKind") == "development"
        and identifier.startswith(DEV_ID_PREFIX)
        and identifier != SWITCH_ID
    )


def read_info(executable: Path) -> dict:
    result = subprocess.run(
        [str(executable), "--build-info"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"{executable} --build-info failed: {result.stderr.strip() or result.stdout.strip()}"
        )
    return json.loads(result.stdout)


def run(command: list[str], cwd: Path = ROOT) -> int:
    print("+ " + " ".join(command), flush=True)
    return subprocess.call(command, cwd=cwd)


def build() -> int:
    result = run(["npm", "--prefix", "client", "run", "tauri", "--", "build", "--no-bundle"])
    if result:
        return result
    return check_identity()


def check_identity() -> int:
    executable = binary()
    if not executable.is_file():
        print(f"No development build at {executable}: run template tauri-build first.", file=sys.stderr)
        return 1
    try:
        info = read_info(executable)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Could not read the built identity: {error}", file=sys.stderr)
        return 1
    if not identity_allows_launch(info):
        print(
            "Refusing a switch-identity build; only tauri-build/tauri-dev/tauri-launch use the "
            f"development identity, and the package builds {SWITCH_ID}: {info}",
            file=sys.stderr,
        )
        return 1
    print(f"development identity: {info.get('CFBundleIdentifier')}", flush=True)
    return 0


def dev() -> int:
    return run(["npm", "--prefix", "client", "run", "tauri", "--", "dev"])


def launch() -> int:
    executable = binary()
    if not executable.is_file():
        print(f"No development build at {executable}: run template tauri-build first.", file=sys.stderr)
        return 2
    try:
        info = read_info(executable)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Refusing to launch: {error}", file=sys.stderr)
        return 2
    if not identity_allows_launch(info):
        print(
            "Refusing to launch a switch-identity build outside the package "
            f"({info.get('CFBundleIdentifier')} / buildKind {info.get('buildKind')}). "
            "Build it with tauri-build and launch that development build.",
            file=sys.stderr,
        )
        return 2
    return subprocess.call([str(executable), *sys.argv[2:]], cwd=ROOT)


def main() -> int:
    actions = {"build": build, "dev": dev, "launch": launch}
    action = sys.argv[1] if len(sys.argv) > 1 else ""
    if action not in actions:
        print("usage: tauri_template.py build|dev|launch", file=sys.stderr)
        return 2
    expected = os.environ.get("ZERUS_TAURI_ACTION", "")
    if expected and expected != action:
        print(f"ZERUS_TAURI_ACTION is {expected}, not {action}", file=sys.stderr)
        return 2
    return actions[action]()


if __name__ == "__main__":
    raise SystemExit(main())
