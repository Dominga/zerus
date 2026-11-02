"""Original-child protocol custody; all subprocesses are private Python fixtures."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("hold_protocol", ROOT / "scripts/desktop-revoke-hold.py")
protocol = importlib.util.module_from_spec(spec)
spec.loader.exec_module(protocol)

CHILD = r'''
import json,os,sys
receipt=json.loads(os.environ['RECEIPT'])
mode=os.environ['MODE']
def send(value):
    print(json.dumps(value),flush=True)
if mode=='refuse':
    send(dict(abi='desktop-revoke-hold-v1',held=False,code='writer_authority_changed'))
    sys.exit(1)
if mode=='partial':
    sys.stdout.write('{"abi":');sys.stdout.flush();sys.exit(0)
if mode=='overflow':
    sys.stdout.write('x'*10000);sys.stdout.flush();sys.exit(0)
if mode=='wrong_epoch':
    receipt['writer_epoch']='00000000-0000-4000-8000-000000000000'
if mode!='silent':
    send(dict(abi='desktop-revoke-hold-v1',held=True,receipt=receipt))
if mode=='early_release':
    send(dict(abi='desktop-revoke-hold-v1',held=False,released=True,receipt=receipt))
sys.stdin.buffer.read()
if mode=='stderr':
    sys.stderr.write('bounded private fixture stderr\n')
if mode=='wrong_release':
    receipt['request_id']='00000000-0000-4000-8000-000000000000'
send(dict(abi='desktop-revoke-hold-v1',held=False,released=True,receipt=receipt))
sys.exit(1 if mode=='bad_exit' else 0)
'''


@unittest.skipUnless(os.name == "posix", "Unix pipe/selector helper")
class DesktopRevokeHold(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zerus-hold-protocol-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.receipt = dict(request_id=str(uuid.uuid4()), configuration="a" * 64,
                            writer_epoch=str(uuid.uuid4()), accepted=True, durable=True,
                            revoked=True, checkpointed=True)
        self.children, self.calls = [], []

    def hold(self, mode="normal", **overrides):
        def spawn(argv, **kwargs):
            self.calls.append((argv, kwargs))
            child = subprocess.Popen([sys.executable, "-c", CHILD], **kwargs)
            self.children.append(child)
            return child
        values = dict(hgs=self.root / "selected hgs", env={"PATH": "/usr/bin:/bin", "HOME": str(self.root),
                      "RECEIPT": json.dumps(self.receipt), "MODE": mode}, configuration="a" * 64,
                      request_id=self.receipt["request_id"], receipt=self.receipt,
                      help_text="  " + protocol.MARKER, popen=spawn)
        values.update(overrides)
        value = protocol.Hold(**values)
        self.addCleanup(value.close, validate=False)
        return value

    def test_original_child_is_alive_until_eof_then_same_receipt_exit_and_both_pipes_drain(self):
        hold = self.hold("stderr").acquire()
        hold.check()
        self.assertIsNone(hold.child.poll())
        self.assertFalse(hold.child.stdin.closed)
        argv, kwargs = self.calls[0]
        self.assertEqual(argv, [str(self.root / "selected hgs"), "desktop", "revoke-hold",
                               "--json", "--request-id", self.receipt["request_id"]])
        self.assertIs(kwargs["stdin"], subprocess.PIPE)
        self.assertTrue(kwargs["close_fds"])
        hold.close()
        self.assertTrue(hold.finished)
        self.assertEqual(hold.child.returncode, 0)
        self.assertTrue(all(p.closed for p in (hold.child.stdin, hold.child.stdout, hold.child.stderr)))
        self.assertEqual(hold.frames[-1]["receipt"], self.receipt)
        with self.assertRaises(protocol.Refusal):
            hold.check()

    def test_missing_marker_and_invalid_original_receipts_refuse_without_spawning(self):
        for change in ({"help_text": "Desktop revoke hold ABI: desktop-revoke-hold-v0"},
                       {"receipt": dict(self.receipt, checkpointed=False)},
                       {"receipt": dict(self.receipt, writer_epoch="unknown")},
                       {"receipt": dict(self.receipt, request_id=str(uuid.uuid4()))}):
            with self.subTest(change=change), self.assertRaises(protocol.Refusal):
                self.hold(**change)
        self.assertEqual(self.children, [])

    def test_refused_partial_oversize_and_changed_epoch_frames_never_acquire_a_hold(self):
        for mode in ("refuse", "partial", "overflow", "wrong_epoch", "early_release"):
            with self.subTest(mode=mode), self.assertRaises(protocol.Refusal):
                self.hold(mode).acquire()
            self.assertIsNotNone(self.children[-1].returncode)

    def test_silent_initial_handshake_is_bounded_and_original_child_is_joined(self):
        hold = self.hold("silent", timeout=0.2)
        with self.assertRaises(protocol.Refusal):
            hold.acquire()
        self.assertTrue(hold.finished)
        self.assertIsNotNone(hold.child.returncode)

    def test_wrong_release_or_nonzero_exit_is_not_a_settled_transition(self):
        for mode in ("wrong_release", "bad_exit"):
            hold = self.hold(mode).acquire()
            with self.subTest(mode=mode), self.assertRaisesRegex(protocol.Refusal, "unverified"):
                hold.close()
            self.assertTrue(hold.finished)
            self.assertIsNotNone(hold.child.returncode)

    def test_exited_original_child_cannot_be_replaced_by_copied_ready_metadata(self):
        hold = self.hold().acquire()
        original = hold.child
        original.kill()  # Private protocol fixture only.
        original.wait(timeout=2)
        with self.assertRaises(protocol.Refusal):
            hold.check()
        self.assertIs(hold.child, original)
        self.assertEqual(hold.frames[0]["receipt"], self.receipt)


if __name__ == "__main__":
    unittest.main()
