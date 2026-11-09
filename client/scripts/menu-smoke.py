#!/usr/bin/env python3
"""Run a synthetic WebKitGTK component regression (default: the real menu components) against synthetic, delayed IPC.

Start Vite first. Default: run under dbus-run-session; a private Xvfb is started.
For the Arch desktop, use flock -w 1800 /tmp/zerus-arch-display.lock and
--real-display; that mode refuses a locked screen and stops if it locks.
No CLI, native agent, installed app or owner data is opened.
"""
import argparse
from contextlib import ExitStack
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
parser.add_argument("--accessibility", action="store_true", help="check reported semantics through AT-SPI on private buses")
parser.add_argument("--xvfb-child", action="store_true", help=argparse.SUPPRESS)
parser.add_argument("--paint-column", type=int, metavar="X",
                    help="check the presented frames of a page that reports `morphs` (a 4 px column at X through "
                         "a magenta collapsing card must never grow again, row-collapse-smoke), `landings` (a magenta "
                         "reading anchor must keep its top while an older page lands, activity-older-smoke) or `opens` "
                         "(the magenta loading heading and the cyan placed heading never in one frame, activity-open-smoke) "
                         "or `shows` (a magenta card, once fully painted, is never drawn dimmer again, task-view-flicker-smoke "
                         "and entrance-survey-smoke)")
parser.add_argument("--paint-row", type=int, metavar="Y",
                    help="check the presented frames of a page that reports `slides`: the right edge of a "
                         "magenta panel on a 4 px row at Y must only move with each slide (panel-slide-smoke)")
args = parser.parse_args()
if args.paint_column is not None and args.paint_row is not None:
    parser.error("choose one painted sample")
if not args.url.startswith("http://127.0.0.1:"):
    parser.error("use the local Vite server")
if args.real_display and (args.xvfb_child or args.probe_display):
    parser.error("display probes are private virtual-display checks")
if args.real_display and args.accessibility:
    parser.error("accessibility checks require the private virtual display")

if not args.real_display and not args.xvfb_child:
    # Own this server even when invoked from a Wayland desktop or an existing
    # xvfb-run. The child marker prevents re-execution; no owner display is used.
    env = dict(os.environ, GDK_BACKEND="x11", ZERUS_MENU_PARENT_DISPLAY=os.environ.get("DISPLAY", ""))
    env.pop("WAYLAND_DISPLAY", None)
    # The child owns the window, its title and the title-named deadline.
    command = ["xvfb-run", "-a", sys.executable, __file__, "--xvfb-child", "--url", args.url, "--title", args.title]
    if args.paint_column is not None:
        command += ["--paint-column", str(args.paint_column)]
    if args.paint_row is not None:
        command += ["--paint-row", str(args.paint_row)]
    if args.probe_display:
        command.append("--probe-display")
    if args.accessibility:
        env["ZERUS_MENU_A11Y_PARENT_BUS"] = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
        command = ["dbus-run-session", "--", *command, "--accessibility"]
    # Debian/Ubuntu xvfb-run runs its command with 2>&1. Hand the child this
    # stderr as an inherited descriptor, so its reasons (a missing GTK stack,
    # a refused display) stay on stderr and stdout carries only the result.
    stderr_fd = os.dup(2)
    env["ZERUS_MENU_STDERR_FD"] = str(stderr_fd)
    child = subprocess.Popen(command, env=env, start_new_session=True, pass_fds=(stderr_fd,))
    os.close(stderr_fd)
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

if args.xvfb_child and os.environ.get("ZERUS_MENU_STDERR_FD", "").isdigit():
    # Restore the parent's stderr that xvfb-run merged into stdout (see above).
    inherited = int(os.environ.pop("ZERUS_MENU_STDERR_FD"))
    os.dup2(inherited, 2)
    os.close(inherited)

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


def painted_slide(frames, slides, width, stride):
    """Each slide's painted panel (magenta at any opacity, on black) moves one
    way only: a closing slide's right edge never grows back, an opening one's
    never shrinks back, from its first move until the slide's window ends."""
    def edge(data):
        row = data[:stride]
        right = 0
        for x in range(width):
            b, g, r = row[x * 4], row[x * 4 + 1], row[x * 4 + 2]
            if r > 20 and b > 20 and g < 20:
                right = x + 1
        return right
    cases, failures = [], []
    for slide in slides:
        start = slide["at"] / 1000
        shown = [(t, edge(data)) for t, data in frames if start - 0.03 <= t <= start + 0.8]
        opening, moved, extreme, back = slide["opening"], False, None, []
        for t, right in shown:
            if extreme is not None and right != extreme and (right > extreme) == opening:
                moved = True
            if moved and extreme is not None and ((right < extreme - 2) if opening else (right > extreme + 2)):
                back.append({"ms": round((t - start) * 1000), "edge": right, "extreme": extreme})
            extreme = right if extreme is None else (max(extreme, right) if opening else min(extreme, right))
        edges = [right for _, right in shown]
        cases.append({"slide": slide["name"], "painted_frames": len(shown), "from": edges[:1], "to": edges[-1:],
                      "back": back[:5]})
        if len(shown) < 5:
            failures.append(f"{slide['name']}: only {len(shown)} painted frames were captured")
        if not moved:
            failures.append(f"{slide['name']}: the painted panel did not slide")
        if back:
            failures.append(f"{slide['name']}: {len(back)} painted frames went back against the slide")
    return {"paint": cases, "paint_failures": failures}


def painted_anchor(frames, landings, height):
    """While each older page lands, the painted reading anchor (magenta) keeps
    the top it had in the last frame before the landing, in every frame."""
    def column(data):
        stride = len(data) // height
        out = []
        for y in range(height):
            b, g, r = data[y * stride + 4], data[y * stride + 5], data[y * stride + 6]
            out.append("M" if r > 200 and g < 60 and b > 200 else "C" if r < 60 and g > 200 and b > 200 else ".")
        return "".join(out)

    def runs(col):
        out, start = [], 0
        for y in range(1, height + 1):
            if y == height or col[y] != col[start]:
                out.append(f"{col[start]}{start}+{y - start}")
                start = y
        return out[:8]
    cases, failures = [], []
    for index, start in enumerate(m / 1000 for m in landings):
        # Frames are recorded only when the paint changes: the last one before
        # the landing is what the screen showed then, however long ago.
        before = [column(data) for t, data in frames if t < start]
        place = before[-1].find("M") if before and "M" in before[-1] else None
        shown = [(t, column(data)) for t, data in frames if start <= t <= start + 0.9]
        away = [{"ms": round((t - start) * 1000), "top": col.find("M") if "M" in col else None, "column": runs(col)}
                for t, col in shown if place is None or "M" not in col or abs(col.find("M") - place) > 2]
        cases.append({"landing": index, "top": place, "painted_frames": len(shown), "away": away[:5]})
        # No new frame after the landing means the screen kept showing the
        # anchor where it was: the best outcome, not a missing sample.
        if place is None:
            failures.append(f"landing {index}: no painted anchor before the page landed")
        elif away:
            failures.append(f"landing {index}: {len(away)} painted frames showed the reading anchor away from its place")
    return {"paint": cases, "paint_failures": failures}


def painted_once(frames, opens, height):
    """While sessions open, the Activity heading is drawn once: no presented
    frame shows the loading heading (magenta) and the placed one (cyan)."""
    def kinds(data):
        stride = len(data) // height
        found = set()
        for y in range(height):
            b, g, r = data[y * stride + 4], data[y * stride + 5], data[y * stride + 6]
            if r > 200 and g < 60 and b > 200:
                found.add("M")
            elif r < 60 and g > 200 and b > 200:
                found.add("C")
        return found
    start = opens[0] / 1000 if opens else 0
    seen = [(t, kinds(data)) for t, data in frames if t >= start]
    both = [round((t - start) * 1000) for t, found in seen if found == {"M", "C"}]
    failures = [f"{len(both)} painted frames showed both Activity headings (ms after the first open: {both[:5]})"] if both else []
    return {"paint": {"frames": len(seen), "loading": sum("M" in f for _, f in seen),
                      "placed": sum("C" in f for _, f in seen), "both": len(both)},
            "paint_failures": failures}


def painted_steady(frames, shows, ends, height):
    """Once an opened magenta card (a Task View result, an entrance) is fully painted, no later
    presented frame of that open draws it dimmer: no blink after it appeared."""
    def level(data):
        stride = len(data) // height
        best = 0
        for y in range(height):
            b, g, r = data[y * stride + 4], data[y * stride + 5], data[y * stride + 6]
            if g < 60:
                best = max(best, min(r, b))
        return best
    cases, failures = [], []
    for index, start in enumerate(shows):
        # Each open's window ends before the page closes or switches it.
        end = ends[index] if index < len(ends) else shows[index + 1] if index + 1 < len(shows) else float("inf")
        seen = [(round((t - start / 1000) * 1000), level(data)) for t, data in frames if start / 1000 <= t < end / 1000]
        full = next((i for i, (_, value) in enumerate(seen) if value >= 230), None)
        dimmer = [(ms, value) for ms, value in seen[full + 1:] if value < 200] if full is not None else []
        # A fade-in starts near black; its first painted card frame says whether the entrance shows at all.
        first = next(((ms, value) for ms, value in seen if value > 20), None)
        cases.append({"open": index, "painted_frames": len(seen), "first_card": first,
                      "full_at": seen[full][0] if full is not None else None, "dimmer": dimmer[:5]})
        if full is None:
            failures.append(f"open {index}: the magenta card was never fully painted")
        elif dimmer:
            failures.append(f"open {index}: {len(dimmer)} painted frames dimmed the magenta card after it was fully shown "
                            f"(ms after the open, level: {dimmer[:5]})")
    return {"paint": cases, "paint_failures": failures}


with tempfile.TemporaryDirectory(prefix="zerus-menu-") as scratch, ExitStack() as cleanup:
    for variable, directory in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"),
                                ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")]:
        path = Path(scratch, directory)
        path.mkdir()
        os.environ[variable] = str(path)
    os.environ["GSETTINGS_BACKEND"] = "memory"
    if args.accessibility:
        from accessibility_smoke import check_lists, private_bus
        import threading
        cleanup.enter_context(private_bus(scratch))
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
        report = json.loads(value.get_js_value().to_string())
        if not args.accessibility:
            finish(report)
            return

        def inspect():
            try:
                native = check_lists(report.get("accessibility_lists"), os.getpid(), report.get("accessibility_descriptions"), report.get("accessibility_tree_items"))
                report.update(native)
                if native.get("accessibility_failure"):
                    report.setdefault("failures", []).append(native["accessibility_failure"])
            except Exception as error:
                report.setdefault("failures", []).append(f"Accessibility read failed: {error}")
            GLib.idle_add(lambda: finish(report))

        threading.Thread(target=inspect, daemon=True).start()

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
    painted, PAINT_HEIGHT, PAINT_WIDTH = [], 800, 1000
    if args.paint_column is not None or args.paint_row is not None:
        import cairo
        if args.paint_column is not None:
            surface = cairo.ImageSurface(cairo.FORMAT_RGB24, 4, PAINT_HEIGHT)
        else:
            surface = cairo.ImageSurface(cairo.FORMAT_RGB24, PAINT_WIDTH, 4)

        def poll_paint():
            cr = cairo.Context(surface)
            cr.set_source_rgb(0, 0, 0)
            cr.paint()
            if args.paint_column is not None:
                cr.translate(-args.paint_column, 0)
            else:
                cr.translate(0, -args.paint_row)
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
    if args.paint_column is not None and "opens" in result:
        result = {**result, **painted_once(painted, result["opens"], PAINT_HEIGHT)}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    elif args.paint_column is not None and "shows" in result:
        result = {**result, **painted_steady(painted, result["shows"], result.get("ends", []), PAINT_HEIGHT)}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    elif args.paint_column is not None and "landings" in result:
        result = {**result, **painted_anchor(painted, result["landings"], PAINT_HEIGHT)}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    elif args.paint_column is not None:
        result = {**result, **painted_collapse(painted, result.get("morphs", []), PAINT_HEIGHT)}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    if args.paint_row is not None:
        result = {**result, **painted_slide(painted, result.get("slides", []), PAINT_WIDTH, surface.get_stride())}
        result["failures"] = result.get("failures", []) + result.pop("paint_failures")
    if args.paint_column is None and "shows" in result:
        # Its pass is the painted check; without the column it would pass unseen.
        result["failures"] = result.get("failures", []) + ["this page reports `shows`: run it with --paint-column"]
    print(json.dumps({**result, **display_info, "real_display": args.real_display,
                      "webkit": f"{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}"}, indent=2))
    raise SystemExit(1 if result.get("failures") else 0)
