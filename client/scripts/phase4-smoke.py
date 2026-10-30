#!/usr/bin/env python3
"""Run the Phase 4 Part A smoke against a fresh synthetic scratch directory."""
import argparse
from array import array
import base64
import copy
import io
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import wave


CLIENT = Path(__file__).resolve().parent.parent
PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="


def tone_wav():
    pcm = array("h", (int(1000 * math.sin(2 * math.pi * 440 * index / 8000)) for index in range(8000)))
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(8000)
        wav.writeframes(pcm.tobytes())
    return output.getvalue()


def fixture_config(replay_path):
    catalog = {}
    states = {}
    events = {}
    for line in replay_path.read_text(encoding="utf-8").splitlines():
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
            "req-fixture-echo-image": {"name": "pasted-image.png", "mime": "image/png", "data_base64": PNG},
        },
    }


def private_environment(scratch, replay, hgs, smoke_file):
    return {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(scratch / "home"),
        "XDG_CONFIG_HOME": str(scratch / "config"),
        "XDG_DATA_HOME": str(scratch / "data"),
        "XDG_CACHE_HOME": str(scratch / "cache"),
        "XDG_RUNTIME_DIR": str(scratch / "runtime"),
        "TMPDIR": str(scratch / "tmp"),
        "TMUX_TMPDIR": str(scratch / "tmp"),
        "DISPLAY": ":99",
        "WAYLAND_DISPLAY": "zerus-none",
        "GDK_BACKEND": "x11",
        "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
        "ZERUS_CLIENT_HEADLESS": "1",
        "ZERUS_CLIENT_REPLAY": str(replay),
        "ZERUS_CLIENT_HGS": str(hgs),
        "ZERUS_CLIENT_QUERY": "phase4-smoke=1",
        "ZERUS_CLIENT_PERF": "smoke",
        "ZERUS_CLIENT_SMOKE_FILE": str(smoke_file),
        "ZERUS_CLIENT_SMOKE_NOW": "1793000100",
    }


def assert_private_environment(env, scratch):
    expected = {
        "HOME": scratch / "home",
        "XDG_CONFIG_HOME": scratch / "config",
        "XDG_DATA_HOME": scratch / "data",
        "XDG_CACHE_HOME": scratch / "cache",
        "XDG_RUNTIME_DIR": scratch / "runtime",
        "TMPDIR": scratch / "tmp",
        "TMUX_TMPDIR": scratch / "tmp",
    }
    for key, value in expected.items():
        if env.get(key) != str(value):
            raise RuntimeError(f"private smoke refused inherited {key}")
    if env.get("DISPLAY") != ":99" or env.get("WAYLAND_DISPLAY") != "zerus-none":
        raise RuntimeError("private smoke refused an inherited display")
    address = env.get("DBUS_SESSION_BUS_ADDRESS", "")
    if not (address.startswith("unix:abstract=") or address.startswith("unix:path=/tmp/")):
        raise RuntimeError("private smoke refused an inherited or missing D-Bus")


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
    for name in ("home", "config", "data", "cache", "runtime", "tmp"):
        (scratch / name).mkdir(mode=0o700)
    hgs = scratch / "hgs"
    shutil.copy2(CLIENT / "src-tauri/tests/support/fake_hgs_phase4.py", hgs)
    hgs.chmod(0o755)
    frames = [json.loads(line) for line in (CLIENT / "fixtures/replay.ndjson").read_text(encoding="utf-8").splitlines()]
    activity_events = [event for frame in frames if frame.get("t") == "events" and frame.get("topic") == "activity"
                       for event in frame["events"]]
    media = {"seq": max(event["seq"] for event in activity_events) + 1, "type": "UserPromptSubmit", "source": "hgs_voice",
             "message_id": "m-media-smoke", "voice": {"id": "m-media-smoke", "state": "submitted",
             "audio": {"duration_ms": 60000, "waveform": [128] * 64, "request_id": "req-media-smoke", "index": 0}, "transcript": "media smoke transcript"},
             "at": max(event.get("at", 0) for event in activity_events) + 1}
    frames.append({"v": 1, "seq": max(frame["seq"] for frame in frames) + 1,
                   "t": "events", "topic": "activity", "key": "run:000001-run", "events": [media], "cursor": media["seq"]})
    replay = scratch / "replay.ndjson"
    replay.write_text("".join(json.dumps(frame, separators=(",", ":")) + "\n" for frame in frames), encoding="utf-8")
    config = fixture_config(replay)
    config["attachments"]["req-media-smoke"] = {"name": "media-smoke.wav", "mime": "audio/wav",
                                                "data_base64": base64.b64encode(tone_wav()).decode("ascii")}
    (scratch / "config.json").write_text(json.dumps(config, separators=(",", ":")), encoding="utf-8")
    smoke_file = scratch / "smoke-notes.txt"
    smoke_file.write_text('{"orders":[1,2,3]}\n', encoding="utf-8")

    env = private_environment(scratch, replay, hgs, smoke_file)
    command = ["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()), "--guard", str(app), str(scratch)]
    app_log = scratch / "app.log"
    print("GDK_BACKEND=x11 WAYLAND_DISPLAY=zerus-none DISPLAY=:99 (private Xvfb)", flush=True)
    with app_log.open("w", encoding="utf-8") as output, (scratch / "xvfb.log").open("w", encoding="utf-8") as xvfb_log:
        output.write("GDK_BACKEND=x11 WAYLAND_DISPLAY=zerus-none DISPLAY=:99 (private Xvfb)\n")
        output.write("command: " + " ".join(command) + "\n")
        output.flush()
        xvfb = subprocess.Popen(["Xvfb", ":99", "-screen", "0", "1440x900x24"], cwd=CLIENT, env=env,
                                stdout=xvfb_log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
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
        finally:
            try:
                xvfb.terminate()
            except ProcessLookupError:
                pass
            try:
                xvfb.wait(timeout=5)
            except subprocess.TimeoutExpired:
                xvfb.kill()
                xvfb.wait()
    print(app_log.read_text(encoding="utf-8"), end="")
    print(f"phase4 smoke exit {code}; logs kept in {scratch}")
    return code


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--guard":
        assert_private_environment(os.environ, Path(sys.argv[3]))
        os.execvpe(sys.argv[2], [sys.argv[2]], os.environ)
    raise SystemExit(main())
