#!/usr/bin/env python3
"""Run the real menu components in WebKitGTK against synthetic, delayed IPC.

Start Vite first. Default: run under dbus-run-session; a private Xvfb is started.
For the Arch desktop, use flock -w 1800 /tmp/zerus-arch-display.lock and
--real-display; that mode refuses a locked screen and stops if it locks.
No CLI, native agent, installed app or owner data is opened.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--url", default="http://127.0.0.1:1437/scripts/menu-smoke.html")
parser.add_argument("--real-display", action="store_true")
parser.add_argument("--probe-display", action="store_true", help="verify the isolated display without opening a window")
parser.add_argument("--xvfb-child", action="store_true", help=argparse.SUPPRESS)
args = parser.parse_args()
if not args.url.startswith("http://127.0.0.1:"):
    parser.error("use the local Vite server")
if args.real_display and (args.xvfb_child or args.probe_display):
    parser.error("display probes are private virtual-display checks")

if not args.real_display and not args.xvfb_child:
    # Own this server even when invoked from a Wayland desktop or an existing
    # xvfb-run. The child marker prevents re-execution; no owner display is used.
    env = dict(os.environ, GDK_BACKEND="x11", ZERUS_MENU_PARENT_DISPLAY=os.environ.get("DISPLAY", ""))
    env.pop("WAYLAND_DISPLAY", None)
    command = ["xvfb-run", "-a", sys.executable, __file__, "--xvfb-child", "--url", args.url]
    if args.probe_display:
        command.append("--probe-display")
    child = subprocess.Popen(command, env=env, start_new_session=True)
    try:
        raise SystemExit(child.wait(timeout=45))
    except subprocess.TimeoutExpired:
        # Only this original private fixture group, including its Xvfb server.
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
        raise SystemExit("Private display regression exceeded its deadline")

if not args.real_display:
    os.environ["GDK_BACKEND"] = "x11"
    os.environ.pop("WAYLAND_DISPLAY", None)

# The screen query uses the actual desktop bus even under dbus-run-session.
screen_bus = os.environ.get("ZERUS_MENU_SCREEN_BUS", os.environ.get("DBUS_SESSION_BUS_ADDRESS", ""))


def unlocked():
    reply = subprocess.run(["qdbus6", "--bus", screen_bus, "org.freedesktop.ScreenSaver",
                            "/ScreenSaver", "org.freedesktop.ScreenSaver.GetActive"],
                           text=True, capture_output=True, timeout=3)
    return reply.returncode == 0 and reply.stdout.strip() == "false"


if args.real_display and not unlocked():
    raise SystemExit("Screen is locked or its state is unknown; no window opened")

with tempfile.TemporaryDirectory(prefix="zerus-menu-") as scratch:
    for variable, directory in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"),
                                ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")]:
        path = Path(scratch, directory)
        path.mkdir()
        os.environ[variable] = str(path)
    os.environ["GSETTINGS_BACKEND"] = "memory"
    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("Gdk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import Gdk, GLib, Gtk, WebKit2
    Gtk.init_check()
    display = Gdk.Display.get_default()
    if display is None:
        raise SystemExit("No display available; no window opened")
    if not args.real_display:
        gi.require_version("GdkX11", "3.0")
        from gi.repository import GdkX11
        if (not isinstance(display, GdkX11.X11Display)
                or display.get_name() != os.environ.get("DISPLAY")
                or display.get_name() == os.environ.get("ZERUS_MENU_PARENT_DISPLAY")):
            raise SystemExit("Private X11 display was not selected; no window opened")
    display_info = {"display_type": type(display).__name__, "display_name": display.get_name()}
    if args.probe_display:
        print(json.dumps({**display_info, "real_display": False, "wayland_inherited": "WAYLAND_DISPLAY" in os.environ}))
        raise SystemExit(0)

    manager = WebKit2.UserContentManager()
    manager.register_script_message_handler("result")
    view = WebKit2.WebView(user_content_manager=manager, web_context=WebKit2.WebContext.new_ephemeral())
    window = Gtk.Window(title="Zerus menu regression (synthetic)")
    window.set_default_size(1000, 800)
    window.add(view)
    result = {"failures": ["No regression result received"]}

    def finish(report):
        global result
        result = report
        window.destroy()
        Gtk.main_quit()

    def received(_manager, value):
        finish(json.loads(value.get_js_value().to_string()))

    def deadline():
        finish({"failures": ["Menu regression exceeded 30 seconds"]})
        return False

    def check_screen():
        try:
            if not unlocked():
                finish({"failures": ["Screen locked; stopped regression and closed its window"]})
                return False
        except (OSError, subprocess.TimeoutExpired):
            finish({"failures": ["Screen state unavailable; stopped regression and closed its window"]})
            return False
        return True

    manager.connect("script-message-received::result", received)
    window.connect("delete-event", lambda *_: finish({"failures": ["Regression window closed early"]}))
    GLib.timeout_add_seconds(30, deadline)
    if args.real_display:
        GLib.timeout_add(500, check_screen)
    view.load_uri(args.url)
    window.show_all()
    Gtk.main()
    print(json.dumps({**result, **display_info, "real_display": args.real_display,
                      "webkit": f"{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}"}, indent=2))
    raise SystemExit(1 if result.get("failures") else 0)
