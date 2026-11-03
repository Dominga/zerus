#!/usr/bin/env python3
"""Run a synthetic WebKitGTK component regression (default: the real menu components) against synthetic, delayed IPC.

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
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--url", default="http://127.0.0.1:1437/scripts/menu-smoke.html")
parser.add_argument("--real-display", action="store_true")
parser.add_argument("--title", default="Zerus menu regression (synthetic)")
parser.add_argument("--probe-display", action="store_true", help="verify the isolated display without opening a window")
parser.add_argument("--xvfb-child", action="store_true", help=argparse.SUPPRESS)
parser.add_argument("--paint-column", type=int, metavar="X",
                    help="check the presented frames of a page that reports `morphs`: a 4 px column at X "
                         "through a magenta collapsing card must never grow again (row-collapse-smoke)")
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
    # The child owns the window, its title and the title-named deadline.
    command = ["xvfb-run", "-a", sys.executable, __file__, "--xvfb-child", "--url", args.url, "--title", args.title]
    if args.paint_column is not None:
        command += ["--paint-column", str(args.paint_column)]
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

def painted_collapse(frames, morphs, height):
    """Each collapse's painted card (magenta) must only shrink and the next
    card (cyan) only rise, from the first smaller frame until the morph ends."""
    def column(data):
        stride = len(data) // height
        out = []
        for y in range(height):
            b, g, r = data[y * stride + 4], data[y * stride + 5], data[y * stride + 6]
            out.append("M" if r > 200 and g < 60 and b > 200 else "C" if r < 60 and g > 200 and b > 200 else ".")
        return out
    cases, failures = [], []
    for index, start in enumerate(m / 1000 for m in morphs):
        shown = [(t, column(data)) for t, data in frames if start - 0.03 <= t <= start + 0.85]
        lowest_card = lowest_next = None
        shrinking, taller = False, []
        for t, col in shown:
            card = [y for y, c in enumerate(col) if c == "M"]
            if not card:
                continue
            nxt = next((y for y in range(card[-1] + 1, height) if col[y] == "C"), None)
            if lowest_card is not None and len(card) < lowest_card:
                shrinking = True
            if shrinking and (len(card) > lowest_card + 1 or (nxt is not None and lowest_next is not None and nxt > lowest_next + 1)):
                taller.append({"ms": round((t - start) * 1000), "card": len(card), "next": nxt})
            lowest_card = len(card) if lowest_card is None else min(lowest_card, len(card))
            if nxt is not None:
                lowest_next = nxt if lowest_next is None else min(lowest_next, nxt)
        cases.append({"collapse": index, "painted_frames": len(shown), "taller": taller[:5]})
        if len(shown) < 5:
            failures.append(f"collapse {index}: only {len(shown)} painted frames were captured")
        if taller:
            failures.append(f"collapse {index}: {len(taller)} painted frames were taller after the collapse began")
    return {"paint": cases, "paint_failures": failures}


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
        print(json.dumps({**display_info, "real_display": False, "wayland_inherited": "WAYLAND_DISPLAY" in os.environ,
                          "title": args.title}))
        raise SystemExit(0)

    manager = WebKit2.UserContentManager()
    manager.register_script_message_handler("result")
    view = WebKit2.WebView(user_content_manager=manager, web_context=WebKit2.WebContext.new_ephemeral())
    window = Gtk.Window(title=args.title)
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
        finish({"failures": [f"{args.title} exceeded 30 seconds"]})
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

    # What WebKit hands GTK each frame, polled in-process: no screen capture.
    painted, PAINT_HEIGHT = [], 800
    if args.paint_column is not None:
        import cairo
        surface = cairo.ImageSurface(cairo.FORMAT_RGB24, 4, PAINT_HEIGHT)

        def poll_paint():
            cr = cairo.Context(surface)
            cr.set_source_rgb(0, 0, 0)
            cr.paint()
            cr.translate(-args.paint_column, 0)
            view.draw(cr)
            surface.flush()
            data = bytes(surface.get_data())
            if not painted or painted[-1][1] != data:
                painted.append((time.time(), data))
            return window.get_visible()
        GLib.timeout_add(2, poll_paint)

    manager.connect("script-message-received::result", received)
    window.connect("delete-event", lambda *_: finish({"failures": ["Regression window closed early"]}))
    GLib.timeout_add_seconds(30, deadline)
    if args.real_display:
        GLib.timeout_add(500, check_screen)
    view.load_uri(args.url)
    window.show_all()
    Gtk.main()
    if args.paint_column is not None:
        result = {**result, **painted_collapse(painted, result.get("morphs", []), PAINT_HEIGHT)}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    print(json.dumps({**result, **display_info, "real_display": args.real_display,
                      "webkit": f"{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}"}, indent=2))
    raise SystemExit(1 if result.get("failures") else 0)
