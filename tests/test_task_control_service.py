"""Gate-off task control over the actual private passive desktop service.

No native agents, manager, owner profile, GUI or installed service is used.
"""
from contextlib import closing
import json
import os
import socket
import sqlite3
import unittest

import test_desktop as kernel


@unittest.skipIf(os.name == "nt", "Unix socket transport; Windows peer controls are native")
class InactiveBridge(unittest.TestCase):
    setUp = kernel.Desktop.setUp
    cli = kernel.Desktop.cli
    serve = kernel.Desktop.serve
    stop = kernel.Desktop.stop

    def request(self, configuration, operation, payload):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(4)
            client.connect(str(self.desktop / "service.sock"))
            hello = dict(abi="desktop-v1", configuration=configuration,
                         op=operation, request=payload)
            client.sendall(json.dumps(hello, separators=(",", ":")).encode() + b"\n")
            with client.makefile("rb") as reader:
                return reader.readline(65537)

    def test_task_control_keeps_the_exact_existing_unsupported_response_while_off(self):
        process, service = self.serve()
        self.assertNotIn("task_controls", service)
        control = self.request(service["configuration"], "not_an_operation", None)
        original = json.loads(control)
        self.assertEqual(original["value"], dict(code="unsupported_operation"))
        for payload in (None, dict(schema=1, operation="begin", input={}),
                        dict(schema=1, operation="execute", caller=dict(kind="person")),
                        dict(schema=99, surprise=True), dict(text="x" * 17000)):
            raw = self.request(service["configuration"], "task_control", payload)
            actual = json.loads(raw)
            expected = dict(original, seq=actual["seq"])
            self.assertEqual(actual, expected)
            # Sequence is the actor's current native cursor, not a new field.
            self.assertIsInstance(actual["seq"], int)
            self.assertEqual(raw, json.dumps(expected, sort_keys=True, separators=(",", ":")).encode() + b"\n")
        with closing(sqlite3.connect(self.desktop / "state.sqlite3")) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'desktop_task_control_%'").fetchone()[0], 0)
        self.assertFalse((self.state / "task-stop-facts").exists())
        self.assertFalse((self.config / "swarm").exists())
        self.assertFalse((self.root / "unexpected-tool").exists())
        self.assertIsNone(process.poll())
