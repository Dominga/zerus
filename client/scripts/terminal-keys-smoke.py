#!/usr/bin/env python3
"""Terminal copy and paste keys in a PRIVATE Xvfb/D-Bus session.
Build bench/terminal-keys first, then pass its output directory. Synthetic text
only; never point this at an installed GUI. Requires GTK3/WebKitGTK4.1 and libXtst.
"""
import ctypes
import functools
import http.server
import json
import os
from pathlib import Path
import sys
import threading

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GdkX11", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, GdkX11, GLib, Gtk, WebKit2  # noqa: E402

assert os.environ.get("GDK_BACKEND") == "x11"
assert os.environ.get("WAYLAND_DISPLAY") == "zerus-none"
assert os.environ.get("DBUS_SESSION_BUS_ADDRESS")
root = Path(sys.argv[1]).resolve(strict=True)


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(root)))
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
window = Gtk.Window()
window.set_default_size(800, 600)
window.move(0, 0)
web = WebKit2.WebView.new_with_context(WebKit2.WebContext.new_ephemeral())
window.add(web)
window.show_all()
manager = web.get_user_content_manager()
manager.register_script_message_handler("result")
rows = []
observed = {}
finished = False

# Actual X11 events go only to this fixture's own XID on its private display.
x11 = ctypes.CDLL("libX11.so.6")
xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
display = x11.XOpenDisplay(None)
assert display

CTRL, SHIFT, INSERT = 0xFFE3, 0xFFE1, 0xFF63


def own_focus():
    x11.XSetInputFocus(display, GdkX11.X11Window.get_xid(window.get_window()), 1, 0)


def keys(symbols):
    own_focus()
    codes = [x11.XKeysymToKeycode(display, s) for s in symbols]
    for c in codes:
        xtst.XTestFakeKeyEvent(display, c, True, 0)
    for c in reversed(codes):
        xtst.XTestFakeKeyEvent(display, c, False, 0)
    x11.XFlush(display)


def right_click(x, y):
    own_focus()
    _, left, top = window.get_window().get_origin()
    xtst.XTestFakeMotionEvent(display, -1, left + x, top + y, 0)
    xtst.XTestFakeButtonEvent(display, 3, True, 0)
    xtst.XTestFakeButtonEvent(display, 3, False, 0)
    x11.XFlush(display)


def js(code):
    web.evaluate_javascript(code, -1, None, None, None, None, None)


def post(stage, extra=""):
    js("window.webkit.messageHandlers.result.postMessage(JSON.stringify({stage:" + json.dumps(stage)
       + ",snapshot:terminalFixture.snapshot()" + extra + "}))")
    return False


# Each step: which terminal has the focus, the keys, and what the "with"
# terminal must show afterwards (cumulative): the copy key copies and sends
# nothing, the paste keys reach the engine's paste event (Terminal's onPaste),
# Ctrl+C and Ctrl+V reach the agent. The baseline is recorded only.
STEPS = [
    ("with", "Ctrl+Shift+C", [CTRL, SHIFT, ord("c")], {"copies": 1, "data": [], "nativeCopies": 0, "nativePastes": 0}),
    ("with", "Ctrl+Shift+V", [CTRL, SHIFT, ord("v")], {"copies": 1, "data": [], "nativeCopies": 0, "nativePastes": 1}),
    ("with", "Shift+Insert", [SHIFT, INSERT], {"copies": 1, "data": [], "nativeCopies": 0, "nativePastes": 2}),
    ("with", "Ctrl+C", [CTRL, ord("c")], {"copies": 1, "data": ["\x03"], "nativeCopies": 0, "nativePastes": 2}),
    ("with", "Ctrl+V", [CTRL, ord("v")], {"copies": 1, "data": ["\x03", "\x16"], "nativeCopies": 0, "nativePastes": 2}),
    ("base", "Ctrl+Shift+C", [CTRL, SHIFT, ord("c")], None),
    ("base", "Ctrl+Shift+V", [CTRL, SHIFT, ord("v")], None),
    ("base", "Shift+Insert", [SHIFT, INSERT], None),
]
step = 0


def run_step():
    name, label, symbols, _ = STEPS[step]
    js(f"terminalFixture.focus({json.dumps(name)})")
    GLib.timeout_add(120, lambda: (keys(symbols), GLib.timeout_add(200, lambda: post("step")), False)[-1])
    return False


def receive(_manager, message):
    global finished, step
    data = json.loads(message.get_js_value().to_string())
    stage = data.get("stage")
    if stage == "ready":
        observed["platform"] = data["platform"]
        Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).set_text("synthetic clipboard text", -1)
        GLib.timeout_add(300, run_step)
    elif stage == "step":
        name, label, _, expected = STEPS[step]
        got = data["snapshot"][name]
        if expected is None:
            observed[f"xterm alone, {label}"] = {k: got[k] for k in ("data", "nativeCopies", "nativePastes")}
        else:
            actual = {k: got[k] for k in expected}
            rows.append({"name": f"actual {label} in Terminal", "pass": actual == expected, "detail": actual})
        step += 1
        if step < len(STEPS):
            run_step()
        else:
            # Typing cleared xterm's selection: select again so Copy selection is offered.
            js("terminalFixture.focus('with');window.webkit.messageHandlers.result.postMessage(JSON.stringify({stage:'point',point:terminalFixture.point('with'),base:terminalFixture.point('base')}))")
    elif stage == "point":
        right_click(data["point"]["x"], data["point"]["y"])
        GLib.timeout_add(250, lambda: (right_click(data["base"]["x"], data["base"]["y"]), GLib.timeout_add(250, lambda: post("menu")), False)[-1])
    elif stage == "menu":
        menus = data["snapshot"]["with"]["menus"]
        rows.append({"name": "actual right-click opens Copy selection and Paste", "pass": menus == ["Copy selection | Paste"], "detail": menus})
        observed["xterm alone, right-click"] = data["snapshot"]["base"]["menus"]
        finished = True
        Gtk.main_quit()


manager.connect("script-message-received::result", receive)
web.load_uri(f"http://127.0.0.1:{server.server_port}/bench/terminal-keys/index.html")
GLib.timeout_add_seconds(25, lambda: (rows.append({"name": "bounded fixture", "pass": False}), Gtk.main_quit(), False)[-1])
try:
    Gtk.main()
finally:
    window.destroy()
    x11.XCloseDisplay(display)
    server.shutdown()
    server.server_close()
    thread.join(2)
print(json.dumps({"engine": ".".join(str(v()) for v in (WebKit2.get_major_version, WebKit2.get_minor_version, WebKit2.get_micro_version)),
                  "results": rows, "observed": observed}, indent=2, ensure_ascii=True))
sys.exit(0 if finished and rows and all(r["pass"] for r in rows) else 1)
