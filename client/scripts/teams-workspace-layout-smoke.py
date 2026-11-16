#!/usr/bin/python3
"""Private WebKitGTK geometry with read-only synthetic UI callbacks.

Requires Python GI, GTK3/WebKit2 4.1 and Xvfb. Build the sibling smoke HTML
with Vite as the entry point. Run this driver under a private xvfb-run with
DISPLAY/DBUS_SESSION_BUS_ADDRESS cleared and GDK_BACKEND=x11; never use the
person's display. --build and --output are caller-owned directories.
"""
import argparse
import functools
import hashlib
import http.server
import json
import pathlib
import sys
import threading
from urllib.parse import urlsplit

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--build", type=pathlib.Path, required=True, help="Vite build containing scripts/teams-workspace-layout-smoke.html")
parser.add_argument("--output", type=pathlib.Path, required=True, help="Private evidence directory")
parser.add_argument("--name", default="webkit")
parser.add_argument("--scope", choices=["full", "baseline"], default="full")
args = parser.parse_args()
root = args.output.resolve()
root.mkdir(parents=True, exist_ok=True)
name, scope = args.name, args.scope
resources = {}
class Quiet(http.server.SimpleHTTPRequestHandler):
    def send_head(self):
        stream = super().send_head()
        if stream:
            data = stream.read()
            stream.seek(0)
            resources[urlsplit(self.path).path] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
        return stream

    def log_message(self, *_args):
        pass
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(args.build.resolve())))
threading.Thread(target=server.serve_forever, daemon=True).start()
manager = WebKit2.UserContentManager()
manager.register_script_message_handler("proof")
view = WebKit2.WebView.new_with_user_content_manager(manager)
window = Gtk.Window()
window.set_default_size(1440, 1000)
window.add(view)
outcome = {"ok": False, "failures": ["Browser timed out before result"]}
def message(_manager, result):
    global outcome
    value = json.loads(result.get_js_value().to_string())
    if value["type"] == "snapshot":
        def saved(source, result, _data):
            surface = source.get_snapshot_finish(result)
            path = root / f"{name}-{value['name']}.png"
            surface.write_to_png(str(path))
            source.evaluate_javascript("window.teamsLayoutProof.resume(" + json.dumps(value["name"]) + ")", -1, None, None, None, None, None)
        view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, saved, None)
    elif value["type"] == "result":
        outcome = value
        Gtk.main_quit()
manager.connect("script-message-received::proof", message)
view.load_uri(f"http://127.0.0.1:{server.server_port}/scripts/teams-workspace-layout-smoke.html?scope={scope}")
window.show_all()
GLib.timeout_add_seconds(100, lambda: (Gtk.main_quit(), False)[1])
try:
    Gtk.main()
finally:
    window.destroy()
    server.shutdown()
outcome["loadedResources"] = resources
(root / f"{name}-result.json").write_text(json.dumps(outcome, indent=2) + "\n")
print(json.dumps({"ok": outcome["ok"], "checks": len(outcome.get("checks", [])), "failures": outcome["failures"]}))
sys.exit(0 if outcome["ok"] else 1)
