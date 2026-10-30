#!/usr/bin/env python3
"""Capture the Updates screen in WebKitGTK from bounded replay views, without network."""
import argparse
import copy
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess


CLIENT = Path(__file__).resolve().parent.parent
STATES = ("unchecked", "available", "cached-error", "restart-required")
THEMES = ("dark", "light")

BASE = {
    "busy": False,
    "available": False,
    "heading": "Updates have not been checked",
    "installed": "0.46.2 (abcdef0)",
    "latest": "Not checked",
    "comparison": "",
    "last_checked": "Never",
    "cached": False,
    "error": "",
    "channel": "stable",
    "automatic": True,
    "release_label": "View release",
    "release_enabled": False,
    "download_label": None,
    "details": {
        "ownership": "Local source checkout. Update it from the exact verified source.",
        "command": "yay -S zerus",
        "copy_name": "Copy the update command",
        "instructions": "Review the installation instructions before updating.",
    },
    "restart_required": False,
    "version": "",
    "quiet": False,
}


def view(state):
    result = copy.deepcopy(BASE)
    if state == "checking":
        result.update(busy=True, heading="Checking for updates…", comparison="A previous comparison")
    elif state in ("available", "cached-error", "restart-required"):
        result.update(
            available=True,
            heading="Update available",
            latest="0.47.0",
            comparison="Update available for this local build.",
            last_checked="2026-10-30T00:00:00Z",
            release_enabled=True,
            download_label="Download dev source",
            version="0.47.0",
        )
        if state == "cached-error":
            result.update(cached=True, error="Check failed. Try again later.")
        elif state == "restart-required":
            result["restart_required"] = True
    elif state != "unchecked":
        raise ValueError(f"unknown update replay state: {state}")
    return result


def run_case(app, replay, scratch, state, theme):
    case = scratch / f"{state}-{theme}"
    for name in ("home", "data", "cache"):
        (case / name).mkdir(parents=True)
    fixture = case / "view.json"
    fixture.write_text(json.dumps(view(state), separators=(",", ":")), encoding="utf-8")
    image = scratch / f"{state}-{theme}.png"
    log = case / "app.log"
    query = f"page=settings&section=updates&skin=zerus-{theme}&updates-smoke={state}"
    script = case / "run.sh"
    script.write_text(f"""set -eu
{shlex.quote(str(app))} 2> {shlex.quote(str(log))} &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
n=0
until grep -q {shlex.quote(f"updates-smoke {state}: ready")} {shlex.quote(str(log))} 2>/dev/null; do
  if grep -q {shlex.quote(f"updates-smoke {state}: failed")} {shlex.quote(str(log))} 2>/dev/null; then
    cat {shlex.quote(str(log))}
    exit 1
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    cat {shlex.quote(str(log))}
    exit 1
  fi
  n=$((n + 1))
  if [ "$n" -ge 300 ]; then
    cat {shlex.quote(str(log))}
    exit 1
  fi
  sleep 0.1
done
sleep 1
import -window root {shlex.quote(str(image))}
wait "$pid"
trap - EXIT
""", encoding="utf-8")
    env = os.environ.copy()
    env.pop("DISPLAY", None)
    env.pop("ZERUS_CLIENT_HGS", None)
    env.update(
        HOME=str(case / "home"),
        XDG_DATA_HOME=str(case / "data"),
        XDG_CACHE_HOME=str(case / "cache"),
        WAYLAND_DISPLAY="zerus-none",
        GDK_BACKEND="x11",
        WEBKIT_DISABLE_DMABUF_RENDERER="1",
        ZERUS_CLIENT_HEADLESS="1",
        ZERUS_CLIENT_REPLAY=str(replay),
        ZERUS_CLIENT_UPDATES_FIXTURE=str(fixture),
        ZERUS_CLIENT_QUERY=query,
        ZERUS_CLIENT_PERF="smoke",
    )
    command = ["xvfb-run", "-a", "-s", "-screen 0 1440x900x24", "sh", str(script)]
    result = subprocess.run(command, cwd=CLIENT, env=env, timeout=60, check=False)
    if result.returncode or not image.is_file() or image.stat().st_size == 0:
        raise RuntimeError(f"updates smoke failed for {state} {theme}; see {log}")
    print(f"updates-smoke {state} {theme}: {image}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=CLIENT / "src-tauri/target/release/zerus-client")
    parser.add_argument("--replay", type=Path, default=CLIENT / "fixtures/replay.ndjson")
    parser.add_argument("--scratch", type=Path, required=True, help="a new directory for logs and screenshots")
    args = parser.parse_args()
    app = args.app.resolve()
    replay = args.replay.resolve()
    scratch = args.scratch.resolve()
    if not app.is_file():
        parser.error(f"the app does not exist: {app}")
    if not replay.is_file():
        parser.error(f"the replay does not exist: {replay}")
    for tool in ("xvfb-run", "import"):
        if not shutil.which(tool):
            parser.error(f"required tool is unavailable: {tool}")
    scratch.mkdir(parents=True, exist_ok=False)
    for state in STATES:
        for theme in THEMES:
            run_case(app, replay, scratch, state, theme)
    print(f"eight offline WebKitGTK screenshots are in {scratch}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
