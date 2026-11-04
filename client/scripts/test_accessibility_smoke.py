"""The native accessibility reader must never attach to an inherited bus."""
import os
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from accessibility_smoke import private_bus


class Isolation(unittest.TestCase):
    def test_missing_or_inherited_session_bus_refuses_before_starting_any_child(self):
        for environment in ({}, {"DBUS_SESSION_BUS_ADDRESS": "private-session"},
                            {"DBUS_SESSION_BUS_ADDRESS": "same", "ZERUS_MENU_A11Y_PARENT_BUS": "same"}):
            with self.subTest(environment=environment), tempfile.TemporaryDirectory() as scratch:
                with patch.dict(os.environ, environment, clear=True), patch("accessibility_smoke.subprocess.Popen") as spawn:
                    with self.assertRaisesRegex(RuntimeError, "own session bus"), private_bus(scratch):
                        self.fail("foreign session bus admitted")
                    spawn.assert_not_called()

    def test_foreign_accessibility_address_refuses_before_registry_or_client(self):
        child = MagicMock()
        child.poll.return_value = 0
        reply = MagicMock(stdout="('unix:path=/outside-fixture/bus',)")
        environment = {"DBUS_SESSION_BUS_ADDRESS": "private-session", "ZERUS_MENU_A11Y_PARENT_BUS": "parent-session"}
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, environment, clear=True):
            with patch("accessibility_smoke._executable", return_value="/synthetic/helper"), patch("accessibility_smoke._owns", return_value=True):
                with patch("accessibility_smoke.subprocess.Popen", return_value=child) as spawn, patch("accessibility_smoke.subprocess.run", return_value=reply) as call:
                    with self.assertRaisesRegex(RuntimeError, "foreign accessibility bus"), private_bus(scratch):
                        self.fail("foreign accessibility bus admitted")
                    self.assertEqual(spawn.call_count, 1, "registry must not start on the foreign bus")
                    self.assertEqual(call.call_count, 1, "foreign bus must not receive a status write")
                    child.wait.assert_called_once_with(timeout=3)


if __name__ == "__main__":
    unittest.main()
