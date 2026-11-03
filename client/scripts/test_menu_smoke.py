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


if __name__ == "__main__":
    unittest.main()
