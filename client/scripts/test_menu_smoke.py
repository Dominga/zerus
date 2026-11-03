"""The default smoke mode owns a private X11 display, even on Wayland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest


class VirtualDisplay(unittest.TestCase):
    @unittest.skipUnless(shutil.which("xvfb-run") and shutil.which("dbus-run-session"), "private GTK display tools required")
    def test_inherited_wayland_cannot_select_the_desktop(self):
        env = dict(os.environ, GDK_BACKEND="wayland", WAYLAND_DISPLAY="fixture-owner-wayland", DISPLAY=":199")
        result = subprocess.run(["dbus-run-session", "--", "python3", str(Path(__file__).with_name("menu-smoke.py")), "--probe-display"],
                                env=env, capture_output=True, text=True, timeout=40)
        self.assertEqual(result.returncode, 0, result.stderr)
        info = json.loads(result.stdout)
        self.assertEqual(info["display_type"], "X11Display")
        self.assertNotEqual(info["display_name"], ":199")
        self.assertFalse(info["wayland_inherited"])
        self.assertFalse(info["real_display"])
        self.assertEqual(info["title"], "Zerus menu regression (synthetic)")

    @unittest.skipUnless(shutil.which("xvfb-run") and shutil.which("dbus-run-session"), "private GTK display tools required")
    def test_a_named_regression_keeps_its_title_in_the_private_display_child(self):
        # Another page (the list flicker smoke) runs under its own title and deadline.
        env = dict(os.environ, GDK_BACKEND="wayland", WAYLAND_DISPLAY="fixture-owner-wayland", DISPLAY=":199")
        result = subprocess.run(["dbus-run-session", "--", "python3", str(Path(__file__).with_name("menu-smoke.py")),
                                 "--probe-display", "--title", "Zerus list flicker regression"],
                                env=env, capture_output=True, text=True, timeout=40)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["title"], "Zerus list flicker regression")


if __name__ == "__main__":
    unittest.main()
