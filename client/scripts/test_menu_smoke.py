"""The default smoke mode owns a private X11 display, even on Wayland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SMOKE = Path(__file__).with_name("menu-smoke.py")
# What the private display itself needs: xvfb-run starts Xvfb with an xauth
# cookie, and the regression runs under its own session bus.
DISPLAY_TOOLS = ("xvfb-run", "Xvfb", "xauth", "dbus-run-session")
# What menu-smoke.py imports before it can probe that display.
GTK_STACK = (("Gtk", "3.0"), ("Gdk", "3.0"), ("GdkX11", "3.0"), ("WebKit2", "4.1"))


def missing_tools():
    """The first display tool this computer lacks, or None."""
    return next((f"{tool} is not installed" for tool in DISPLAY_TOOLS if not shutil.which(tool)), None)


def missing_stack():
    """Why `python3` here cannot import the GTK/WebKitGTK stack, or None."""
    probe = subprocess.run(["python3", "-c", "import gi\n" + "".join(
        f"gi.require_version({name!r}, {version!r})\n" for name, version in GTK_STACK)],
        capture_output=True, text=True, timeout=30)
    if probe.returncode == 0:
        return None
    reason = (probe.stderr.strip().splitlines() or ["unknown error"])[-1]
    return "the GTK/WebKitGTK Python stack is missing: " + reason


TOOLS = missing_tools()
# A missing stack skips the display probes (hosted Quick runners have no
# python3-gi); a desktop that has it always runs them.
STACK = TOOLS or missing_stack()


def probe(*extra, env=None):
    return subprocess.run(["dbus-run-session", "--", "python3", str(SMOKE), "--probe-display", *extra],
                          env=env, capture_output=True, text=True, timeout=40)


def report(result):
    return f"exit {result.returncode}\nstderr:\n{result.stderr}\nstdout:\n{result.stdout}"


class VirtualDisplay(unittest.TestCase):
    @unittest.skipIf(STACK, STACK)
    def test_inherited_wayland_cannot_select_the_desktop(self):
        env = dict(os.environ, GDK_BACKEND="wayland", WAYLAND_DISPLAY="fixture-owner-wayland", DISPLAY=":199")
        result = probe(env=env)
        self.assertEqual(result.returncode, 0, report(result))
        info = json.loads(result.stdout)
        self.assertEqual(info["display_type"], "X11Display")
        self.assertNotEqual(info["display_name"], ":199")
        self.assertFalse(info["wayland_inherited"])
        self.assertFalse(info["real_display"])
        self.assertEqual(info["title"], "Zerus menu regression (synthetic)")

    @unittest.skipIf(STACK, STACK)
    def test_a_named_regression_keeps_its_title_in_the_private_display_child(self):
        # Another page (the list flicker smoke) runs under its own title and deadline.
        env = dict(os.environ, GDK_BACKEND="wayland", WAYLAND_DISPLAY="fixture-owner-wayland", DISPLAY=":199")
        result = probe("--title", "Zerus list flicker regression", env=env)
        self.assertEqual(result.returncode, 0, report(result))
        self.assertEqual(json.loads(result.stdout)["title"], "Zerus list flicker regression")

    @unittest.skipIf(TOOLS, TOOLS)
    def test_a_failing_private_display_child_explains_itself_on_stderr(self):
        # zerus-mpdy: Debian/Ubuntu xvfb-run merges its command's stderr into
        # stdout, so a missing GTK stack failed with an empty stderr. A stub
        # `gi` that refuses to import stands in for it; no GTK is needed.
        with tempfile.TemporaryDirectory(prefix="zerus-menu-stub-") as stub:
            Path(stub, "gi.py").write_text('raise ImportError("synthetic: no GTK stack in this fixture")\n')
            env = dict(os.environ, PYTHONPATH=stub)
            result = probe(env=env)
        self.assertNotEqual(result.returncode, 0, report(result))
        self.assertIn("synthetic: no GTK stack in this fixture", result.stderr, report(result))
        self.assertNotIn("synthetic", result.stdout, report(result))


if __name__ == "__main__":
    unittest.main()
