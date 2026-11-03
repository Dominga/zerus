"""Qt rollback orchestration with private files and a modelled native boundary.
No display, real service manager, installed app or native agent is accessed.
"""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("cutover_updater", ROOT / "tray/update-installed.py")
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)
CONFIG = "a" * 64


class QtRollback(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="zerus-qt-cutover-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.cli = self.root / "cli"
        self.installed = self.root / "Example.app"
        self.installed.mkdir()
        for name, value in (("HOME", self.root), ("SNAPSHOTS", self.root / "snapshots"),
                            ("CLI", self.cli), ("MAC", False)):
            p = patch.object(updater, name, value)
            p.start()
            self.addCleanup(p.stop)
        p = patch.object(updater.subprocess, "run", self.command)
        p.start()
        self.addCleanup(p.stop)
        self.calls = []
        self.phase = "settled"
        self.helper_ok = True

    def command(self, args, **kwargs):
        args = list(map(str, args))
        self.calls.append((args, kwargs.get("input")))
        if args == [str(self.cli), "--help"]:
            return subprocess.CompletedProcess(args, 0, updater.REVOKE_MARKER, "")
        if args == [str(self.cli), "desktop", "status", "--json"]:
            return subprocess.CompletedProcess(args, 0, json.dumps({"configuration": CONFIG,
                "lifecycle": {"abi": "desktop-lifecycle-v1", "mode": "background", "ipc_ready": True}}), "")
        if args[:2] == [sys.executable, str(ROOT / "scripts/install-linux-desktop-service.py")] or args[:2] == [sys.executable, str(ROOT / "scripts/install-macos-desktop-service.py")]:
            receipt = {"schema": 1, "configuration": CONFIG, "ok": self.helper_ok,
                       "selected_mode": "passive", "running_mode": "passive", "ipc_ready": True,
                       "pending_restart": False, "transition": None}
            return subprocess.CompletedProcess(args, 0 if self.helper_ok else 1, json.dumps(receipt), "")
        if args[1:3] == ["desktop", "revoke"]:
            request = args[args.index("--request-id") + 1]
        elif args[1:3] == ["desktop", "request"]:
            value = json.loads(kwargs["input"])
            self.assertEqual(value["op"], "revoke_status")
            request = value["request_id"]
        else:
            raise AssertionError(args)
        receipt = {"request_id": request, "configuration": CONFIG, "accepted": True, "durable": True,
                   "revoked": self.phase == "settled", "checkpointed": self.phase == "settled"}
        return subprocess.CompletedProcess(args, 0, json.dumps(receipt), "")

    def command_calls(self, command):
        return [a for a, _ in self.calls if len(a) > 2 and a[1:3] == ["desktop", command]]

    def helper_calls(self):
        return [a for a, _ in self.calls if a[0] == sys.executable]

    def test_passive_helper_precedes_revoke_on_linux_and_mac_without_raw_manager_calls(self):
        for mac in (False, True):
            with self.subTest(mac=mac), patch.object(updater, "MAC", mac):
                self.calls.clear()
                updater.revoke_before_qt(self.installed)
                helper = self.helper_calls()[0]
                self.assertEqual(helper[-3:], ["--mode", "passive", "--json"])
                index = next(i for i, (a, _) in enumerate(self.calls) if a == helper)
                revoke = next(i for i, (a, _) in enumerate(self.calls) if a[1:3] == ["desktop", "revoke"])
                self.assertLess(index, revoke)
                self.assertFalse(any(a[0] in ("systemctl", "launchctl", "kill", "tmux") for a, _ in self.calls))

    def test_unknown_survives_updater_restart_and_never_reoffers_or_runs_the_helper(self):
        self.phase = "pending"
        with self.assertRaises(updater.UpdateError):
            updater.revoke_before_qt(self.installed)
        original = json.loads((self.root / "snapshots/.qt-revoke/original.json").read_bytes())
        self.calls.clear()
        with self.assertRaises(updater.UpdateError):
            updater.revoke_before_qt(self.installed)
        self.assertEqual(self.helper_calls(), [])
        self.assertEqual(self.command_calls("revoke"), [])
        self.assertEqual(len(self.command_calls("request")), 1)
        self.assertEqual(json.loads(self.calls[-1][1])["request_id"], original["request_id"])
        self.assertEqual(json.loads((self.root / "snapshots/.qt-revoke/original.json").read_bytes()), original)
        self.phase = "settled"
        self.calls.clear()
        updater.revoke_before_qt(self.installed)
        self.assertEqual(self.helper_calls(), [])
        self.assertEqual(self.command_calls("revoke"), [])
        settled = json.loads((self.root / "snapshots/.qt-revoke/original.json").read_bytes())
        self.assertEqual(settled, dict(original, phase="settled"))

    def test_helper_refusal_keeps_qt_stopped_and_never_offers_a_revoke(self):
        self.helper_ok = False
        with self.assertRaisesRegex(updater.UpdateError, "passive rollback is not settled"):
            updater.revoke_before_qt(self.installed)
        self.assertEqual(self.command_calls("revoke"), [])
        self.assertFalse((self.root / "snapshots/.qt-revoke/original.json").exists())

    def test_changed_scope_and_symlink_journal_refuse_without_mutations(self):
        self.phase = "pending"
        with self.assertRaises(updater.UpdateError):
            updater.revoke_before_qt(self.installed)
        path = self.root / "snapshots/.qt-revoke/original.json"
        record = json.loads(path.read_bytes())
        record["configuration"] = "b" * 64
        path.write_text(json.dumps(record))
        original = path.read_bytes()
        self.calls.clear()
        with self.assertRaisesRegex(updater.UpdateError, "scope changed"):
            updater.revoke_before_qt(self.installed)
        self.assertEqual(path.read_bytes(), original)
        self.assertEqual(self.helper_calls(), [])
        self.assertEqual(self.command_calls("revoke"), [])
        other = self.root / "kept.json"
        path.rename(other)
        path.symlink_to(other)
        with self.assertRaises(updater.UpdateError):
            updater.revoke_before_qt(self.installed)
        self.assertEqual(other.read_bytes(), original)
