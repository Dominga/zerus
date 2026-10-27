#!/usr/bin/env python3
"""Runs the client's pacing scenarios in WebKitGTK (webkit2gtk-4.1, Tauri's Linux engine).

Loads the built synthetic harness (client/bench/harness, built into
client/bench/build/harness) from a local cross-origin isolated HTTP server,
starts globalThis.zerusPerf.run() and writes the report, then samples the
WebKit process tree while the page is idle. Use run-webkitgtk.sh: by default it
starts a private Xvfb, so no window reaches the desktop; ZERUS_BENCH_DISPLAY=wayland
uses the real display (only with the owner warned first), where the window is
titled "Zerus WebKitGTK measurement (closes itself)" and closes when the run ends.
"""
import argparse
import functools
import http.server
import json
import os
import sys
import threading
import time

XVFB = os.environ.get("ZERUS_BENCH_XVFB") == "1" and not os.environ.get("WAYLAND_DISPLAY")
REAL = os.environ.get("ZERUS_BENCH_DISPLAY") == "wayland" and bool(os.environ.get("WAYLAND_DISPLAY"))
if not (XVFB or REAL):
    sys.exit("run this through client/bench/run-webkitgtk.sh")

import gi  # noqa: E402

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

CLOCK_TICKS = os.sysconf("SC_CLK_TCK")


class IsolatedHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        # Cross-origin isolation gives performance.now() its fine resolution (spec criterion setup).
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass


def proc_stat(pid):
    try:
        text = open(f"/proc/{pid}/stat", encoding="utf-8", errors="replace").read()
    except OSError:
        return None
    close = text.rfind(")")
    rest = text[close + 1:].split()
    return {"name": text[text.find("(") + 1:close], "ppid": int(rest[1]), "ticks": int(rest[11]) + int(rest[12])}


def kb(path, key):
    try:
        for line in open(path, encoding="utf-8"):
            if line.startswith(key + ":"):
                return int(line.split()[1]) * 1024
    except OSError:
        pass
    return None


def tree(root):
    parents = {int(p): s["ppid"] for p in os.listdir("/proc") if p.isdigit() and (s := proc_stat(int(p)))}
    members, queue = set(), [root]
    while queue:
        pid = queue.pop()
        if pid not in members:
            members.add(pid)
            queue.extend(c for c, parent in parents.items() if parent == pid and c != pid)
    return {pid: proc_stat(pid) for pid in members if proc_stat(pid)}


def role(pid, root, name):
    if pid == root:
        return "host"
    lowered = name.lower()
    if "webkitwebproces" in lowered:
        return "web"
    if "network" in lowered:
        return "network"
    if "gpu" in lowered:
        return "gpu"
    return "child"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    here = os.path.dirname(os.path.abspath(__file__))
    parser.add_argument("--dist", default=os.path.join(here, "build", "harness"))
    parser.add_argument("--out", required=True)
    parser.add_argument("--label", default="arch-xvfb-webkitgtk-harness")
    parser.add_argument("--hz", type=float, default=60.0)
    parser.add_argument("--warmup-ms", type=int, default=2000)
    parser.add_argument("--run-ms", type=int, default=10000)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--idle-seconds", type=float, default=10.0)
    parser.add_argument("--timeout", type=float, default=1200.0)
    args = parser.parse_args()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(IsolatedHandler, directory=args.dist))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    representative = "1" if REAL else "0"
    url = f"http://127.0.0.1:{server.server_address[1]}/index.html?label={args.label}&representative={representative}"

    window = Gtk.Window(title="Zerus WebKitGTK measurement (closes itself)")
    window.set_default_size(1440, 900)
    view = WebKit2.WebView()
    settings = view.get_settings()
    settings.set_enable_developer_extras(False)
    window.add(view)
    window.show_all()

    state = {"report": None, "error": None}
    plan = {"hz": args.hz, "hzSource": "manual", "warmupMs": args.warmup_ms, "runMs": args.run_ms, "runs": args.runs}
    body = (
        "while (!globalThis.zerusPerf) await new Promise((r) => setTimeout(r, 100));"
        f"const report = await globalThis.zerusPerf.run({json.dumps(plan)});"
        "return JSON.stringify(report);"
    )

    def finish_sample():
        root = os.getpid()
        first, started = tree(root), time.monotonic()

        def second():
            now, elapsed = tree(root), time.monotonic() - started
            processes = []
            for pid, s in sorted(now.items()):
                before = first.get(pid)
                cpu = round((s["ticks"] - before["ticks"]) / CLOCK_TICKS / elapsed * 100, 3) if before else None
                processes.append({
                    "role": role(pid, root, s["name"]), "name": s["name"], "cpu_percent": cpu,
                    "rss_bytes": kb(f"/proc/{pid}/status", "VmRSS"), "pss_bytes": kb(f"/proc/{pid}/smaps_rollup", "Pss"),
                })
            webkit = [p for p in processes if p["role"] != "host"]
            state["report"]["resources"] = {"idle": {
                "window_ms": round(elapsed * 1000),
                "note": "Python GTK host plus WebKit processes; the host is not the Tauri app",
                "webkit": {
                    "processes": len(webkit),
                    "cpu_percent": round(sum(p["cpu_percent"] or 0 for p in webkit), 3),
                    "rss_bytes": sum(p["rss_bytes"] or 0 for p in webkit),
                    "pss_bytes": sum(p["pss_bytes"] or 0 for p in webkit),
                },
                "processes": processes,
            }}
            Gtk.main_quit()
            return False

        GLib.timeout_add(int(args.idle_seconds * 1000), second)

    def done(webview, result, _data):
        try:
            value = webview.call_async_javascript_function_finish(result)
            state["report"] = json.loads(value.to_string())
            finish_sample()
        except Exception as error:  # the report or the page failed
            state["error"] = str(error)
            Gtk.main_quit()

    def loaded(webview, event):
        if event == WebKit2.LoadEvent.FINISHED:
            webview.call_async_javascript_function(body, -1, None, None, None, None, done, None)

    view.connect("load-changed", loaded)
    view.load_uri(url)
    GLib.timeout_add(int(args.timeout * 1000), lambda: (state.update(error="timeout"), Gtk.main_quit()))
    Gtk.main()
    server.shutdown()
    window.destroy()

    if state["error"] or not state["report"]:
        sys.exit(f"measurement failed: {state['error']}")
    report = state["report"]
    report["platform"]["engine"] = f"WebKitGTK {WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}"
    report["harness"] = "client/bench/harness (synthetic screen) in client/bench/run-webkitgtk.py"
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")


if __name__ == "__main__":
    main()
