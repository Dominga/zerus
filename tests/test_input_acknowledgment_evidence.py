"""Bounded input evidence reads, private passive actor and scripted terminal only."""
import json
import os
import subprocess
import sys
import time
import unittest
import uuid

import test_desktop
import test_acknowledge_input as recovery_fixture


@unittest.skipUnless(sys.platform.startswith(("linux", "darwin")), "Unix private evidence fixtures")
class EvidenceReads(unittest.TestCase):
    def test_fifo_audit_refuses_without_blocking_an_unrelated_desktop_request(self):
        f = test_desktop.Desktop("test_native_startup_preparation_repairs_only_the_owned_state_directory")
        f.setUp()
        self.addCleanup(f.doCleanups)
        service, _ = f.serve()  # Passive: no background runtime, native host or presence sampler.
        acknowledgment = dict(request_id=str(uuid.uuid4()), name="claude/example/private",
                              expected_run_id="private-run", expected_conversation_id="private-conversation",
                              original_request_id=str(uuid.uuid4()), pending_at=10.0,
                              terminal_binding_id="a" * 64, expires_at=time.time() + 5)
        directory = f.state / "input_acknowledgments"
        directory.mkdir(mode=0o700)
        fifo = directory / (acknowledgment["request_id"] + ".json")
        os.mkfifo(fifo, 0o600)
        children = []

        def start(value):
            child = subprocess.Popen([str(test_desktop.HGS), "desktop", "request", "--json"],
                                     env=f.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True)
            children.append(child)
            child.stdin.write(json.dumps(value))
            child.stdin.close()
            child.stdin = None
            return child

        try:
            query = start(dict(op="input_acknowledge_result", request_id=str(uuid.uuid4()),
                               acknowledgment=acknowledgment))
            time.sleep(.3)
            heartbeat = start(dict(op="read", request_id=str(uuid.uuid4())))
            until = time.monotonic() + 2
            while time.monotonic() < until and any(child.poll() is None for child in children):
                time.sleep(.02)
            finished = all(child.poll() is not None for child in children)
            self.assertIsNone(service.poll())
        finally:
            # Rescue only this fixture if a mutant restores a blocking open.
            fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
            os.write(fd, b"{}")
            os.close(fd)
            replies = [child.communicate(timeout=6) for child in children]
            fifo.unlink()
        self.assertTrue(finished, "A FIFO audit stalled its result lookup and unrelated desktop work")
        self.assertEqual(query.returncode, 0, replies[0])
        self.assertEqual(heartbeat.returncode, 0, replies[1])
        self.assertEqual(json.loads(replies[0][0])["code"], "input_acknowledgment_unavailable")
        self.assertFalse((f.root / "unexpected-tool").exists())

    def test_original_fifo_refuses_in_both_recent_proof_and_older_recovery_reads(self):
        recovery = recovery_fixture.AcknowledgeInput("test_person_releases_only_the_original_input_without_claiming_acceptance_or_replay")
        recovery.setUp()
        self.addCleanup(recovery.doCleanups)
        f = recovery.f
        path = f.state / "input_receipts" / (recovery.original["request_id"] + ".json")
        original = path.read_bytes()
        for age in (0, 120):
            with self.subTest(age=age):
                record = dict(recovery.pending)
                record["input_pending_at"] -= age
                f.record_path.write_text(json.dumps(record))
                path.unlink()
                os.mkfifo(path, 0o600)
                request = dict(request_id=str(uuid.uuid4()), action="composer",
                               expected_run_id=f.run_id, expected_conversation_id=f.conversation_id)
                child = subprocess.Popen([str(test_desktop.HGS), "terminal", f.name, "--json"],
                                         env=f.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.PIPE, text=True)
                finished = True
                try:
                    output, error = child.communicate(json.dumps(request), timeout=2)
                except subprocess.TimeoutExpired:
                    finished = False
                finally:
                    fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
                    os.write(fd, b"{}")
                    # Later reads use a regular original; only the held FIFO
                    # reader is rescued, including a repeated proof read.
                    path.unlink()
                    path.write_bytes(original)
                    os.close(fd)
                    if not finished:
                        output, error = child.communicate(timeout=6)
                    f.record_path.write_text(json.dumps(recovery.pending))
                self.assertTrue(finished, "A FIFO original input receipt blocked the composer read")
                self.assertEqual(child.returncode, 0, error)
                self.assertNotIn("recovery", json.loads(output)["composer"])
                self.assertEqual(f.received(), recovery.before)
                self.assertNotIn("input_acknowledged_unknown", json.loads(f.record_path.read_text()))


if __name__ == "__main__":
    unittest.main()
