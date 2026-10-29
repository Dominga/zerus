#!/usr/bin/env python3
"""Run the Phase 4 Part A smoke against a fresh synthetic scratch directory."""
import argparse
import base64
import copy
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess


CLIENT = Path(__file__).resolve().parent.parent
PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="


def fixture_config():
    catalog = {}
    states = {}
    events = {}
    for line in (CLIENT / "fixtures/replay.ndjson").read_text(encoding="utf-8").splitlines():
        frame = json.loads(line)
        if frame.get("t") == "snapshot" and frame.get("topic") == "catalog":
            for item in frame["items"]:
                catalog[item["key"]] = item["row"]
        elif frame.get("t") == "events" and frame.get("topic") == "activity":
            events.setdefault(frame["key"], []).extend(frame["events"])
        elif frame.get("t") == "state" and frame.get("topic") == "activity":
            states[frame["key"]] = frame["value"]
    inspect = {}
    for key, state in states.items():
        row = catalog[key]
        detail = copy.deepcopy(state)
        detail["tracked"] = True
        detail["events"] = events.get(key, [])
        detail["cursor"] = detail["events"][-1]["seq"] if detail["events"] else 0
        inspect[row["name"]] = detail
    review_key = next(key for key, row in catalog.items() if row["name"] == "claude/sample-project/review")
    row = catalog[review_key]
    review_events = events.get(review_key, [])
    inspect[row["name"]] = {
        "name": row["name"], "phase": row["phase"], "activity": row["activity"], "current_tool": row["current_tool"],
        "activity_summary": row["activity_summary"], "turn_started": row["turn_started"], "pending_questions": [],
        "history_truncated": False, "tracked": True, "run_id": row["run_id"], "conversation_id": row["conversation_id"],
        "state": row["state"], "runtime_state": row["runtime_state"], "process_state": row["process_state"],
        "resumable": True, "compact_context_supported": True, "clear_context_supported": True,
        "events": review_events, "cursor": review_events[-1]["seq"] if review_events else 0,
    }
    resumed = copy.deepcopy(inspect["claude/sample-project/review"])
    resumed.update(run_id=resumed["run_id"] + "-resumed", state="running", runtime_state="live", process_state="running",
                   phase="idle", activity="idle")
    orders = base64.b64encode(b'{"orders":[1,2,3]}\n').decode("ascii")
    return {
        "attach": str(CLIENT / "core/tests/support/fake_attach.py"),
        "inspect": inspect,
        "resume_ready": {"claude/sample-project/review": resumed},
        "delays": {"clear-context": 0.5, "resume": 0.3},
        "attachments": {
            "req-fixture-1": {"name": "orders.json", "mime": "application/json", "data_base64": orders},
            "req-fixture-image": {"name": "pixel.png", "mime": "image/png", "data_base64": PNG},
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=CLIENT / "src-tauri/target/release/zerus-client")
    parser.add_argument("--scratch", type=Path, required=True, help="a new directory; logs are kept here")
    args = parser.parse_args()
    app = args.app.resolve()
    scratch = args.scratch.resolve()
    if not app.is_file():
        parser.error(f"the app does not exist: {app}")
    scratch.mkdir(parents=True, exist_ok=False)
    (scratch / "home").mkdir()
    (scratch / "data").mkdir()
    (scratch / "cache").mkdir()
    hgs = scratch / "hgs"
    shutil.copy2(CLIENT / "src-tauri/tests/support/fake_hgs_phase4.py", hgs)
    hgs.chmod(0o755)
    (scratch / "config.json").write_text(json.dumps(fixture_config(), separators=(",", ":")), encoding="utf-8")
    smoke_file = scratch / "smoke-notes.txt"
    smoke_file.write_text('{"orders":[1,2,3]}\n', encoding="utf-8")

    env = os.environ.copy()
    env.pop("DISPLAY", None)
    env.update(HOME=str(scratch / "home"), XDG_DATA_HOME=str(scratch / "data"), XDG_CACHE_HOME=str(scratch / "cache"),
               WAYLAND_DISPLAY="zerus-none", GDK_BACKEND="x11", WEBKIT_DISABLE_DMABUF_RENDERER="1", ZERUS_CLIENT_HEADLESS="1",
               ZERUS_CLIENT_REPLAY=str(CLIENT / "fixtures/replay.ndjson"), ZERUS_CLIENT_HGS=str(hgs),
               ZERUS_CLIENT_QUERY="phase4-smoke=1", ZERUS_CLIENT_PERF="smoke", ZERUS_CLIENT_SMOKE_FILE=str(smoke_file),
               ZERUS_CLIENT_SMOKE_NOW="1793000100")
    command = ["xvfb-run", "-a", "-s", "-screen 0 1440x900x24", str(app)]
    app_log = scratch / "app.log"
    print("GDK_BACKEND=x11 WAYLAND_DISPLAY=zerus-none DISPLAY=<unset>", flush=True)
    with app_log.open("w", encoding="utf-8") as output:
        output.write("GDK_BACKEND=x11 WAYLAND_DISPLAY=zerus-none DISPLAY=<unset>\n")
        output.write("command: " + " ".join(command) + "\n")
        output.flush()
        process = subprocess.Popen(command, cwd=CLIENT, env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=180)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            code = 124
    print(app_log.read_text(encoding="utf-8"), end="")
    print(f"phase4 smoke exit {code}; logs kept in {scratch}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
