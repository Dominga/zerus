"""Actual installer call sites with private files and modelled native authority.

The death control owns real consumer/helper processes on Linux. The macOS
installer runs against a launchd model here; it is not native Mac acceptance.
"""
import copy
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import subprocess
import sys
import unittest
import uuid


class ReviewControls:
    def review_manager(self):
        return self.manager if hasattr(self, "manager") else self.command

    def review_install(self, **kwargs):
        if hasattr(self, "manager"):
            return self.instance(kwargs.pop("mode", None)).install(**kwargs)
        return self.install(**kwargs)

    def review_original(self, request):
        if hasattr(self, "manager"):
            return self.instance().activation_result(request)
        return self.install(activation_id=request, activation_status=True)

    def review(self, request):
        if hasattr(self, "manager"):
            return self.instance().review_activation(request)
        return self.install(review_activation=request)

    def review_path(self):
        return self.instance().record_path if hasattr(self, "manager") else self.mode_record()

    def assert_review_has_no_effects(self, *, allow_reads=False, allow_grant_read=False):
        calls = self.review_manager().calls
        self.assertFalse(any(args[0] in ("systemctl", "launchctl") and (not allow_reads
                             or args[2 if args[0] == "systemctl" else 1] not in ("show", "print"))
                             for args, _ in calls), calls)
        self.assertFalse(any("revoke" in args or (not allow_grant_read and "ownership-status" in args)
                             for args, _ in calls), calls)

    def review_unknown(self, *, legacy):
        self.review_install()
        request = str(uuid.uuid4())
        def lost():
            raise RuntimeError("controlled lost manager reply")
        self.review_manager().after_action = ("restart" if hasattr(self, "manager") else "bootstrap", lost)
        with self.assertRaisesRegex(RuntimeError, "controlled lost"):
            self.review_install(mode="background", activation_id=request)
        if legacy:
            path = self.review_path()
            record = json.loads(path.read_bytes())
            del record["activations"][request]["consumer"]
            path.write_text(json.dumps(record))
        self.review_manager().calls.clear()
        return request

    @unittest.skipUnless(os.name == "posix", "Unix installer lock")
    def test_review_legacy_reboot_barrier_and_second_confirmation_use_actual_helper(self):
        request = self.review_unknown(legacy=True)
        manager, path = self.review_manager(), self.review_path()
        original = copy.deepcopy(json.loads(path.read_bytes())["activations"][request])
        grant = copy.deepcopy(manager.grant)
        first = self.review(request)
        self.assertEqual(first["disposition"]["state"], "reboot_required")
        before = path.read_bytes()
        with self.assertRaisesRegex(Exception, "reboot_required"):
            self.review(request)
        self.assertEqual(path.read_bytes(), before)
        self.assert_review_has_no_effects()
        manager.consumer["boot"] = str(uuid.uuid4())  # Native boot model, no wall-clock mutation.
        second = self.review(request)
        self.assertEqual(second["disposition"]["state"], "reviewed_unknown")
        self.assertEqual(manager.grant, grant)
        entry = json.loads(path.read_bytes())["activations"][request]
        self.assertEqual({key: entry[key] for key in original}, original)
        self.assertEqual(len(json.loads(path.read_bytes())["activations"]), 1,
                         "acknowledgement must not reserve a new attempt")
        self.assert_review_has_no_effects()
        # Even deliberately restored default provenance cannot auto-activate.
        record = json.loads(path.read_bytes())
        record["mode_selection"] = "default"
        if hasattr(self, "manager"):
            definition = self.preview["definition"].encode()
            record["mode"] = "passive"
            manager.lifecycle["mode"] = "passive"
            self.destination.write_bytes(definition)
        else:
            definition = manager.plan["definition"].encode()
            record["selected_mode"] = "passive"
            manager.status["lifecycle"]["mode"] = "passive"
            manager.printed_argv = manager.plan["argv"]
            manager.destination.write_bytes(definition)
        record["definition_sha256"] = hashlib.sha256(definition).hexdigest()
        path.write_text(json.dumps(record))
        with self.assertRaisesRegex(Exception, "Resume"):
            self.review_install(mode="background", activation_id=str(uuid.uuid4()), default_only=True)
        self.assertFalse(any(args[0] in ("systemctl", "launchctl") and args[2 if args[0] == "systemctl" else 1]
                             not in ("show", "print") for args, _ in manager.calls))
        manager.calls.clear()
        same = self.review_install(mode="background", activation_id=request)
        self.assertEqual((same["phase"], same["result"]), ("unknown", None))
        self.assert_review_has_no_effects(allow_reads=True)
        with self.assertRaisesRegex(Exception, "NEW --activation-id"):
            self.review_install(mode="background")
        # Review never lends the old Grant to a later request. A fresh native
        # preflight refuses a revoked incarnation without reserving a new ID.
        manager.grant.update(state="revoked", rearm_required=True)
        with self.assertRaisesRegex(Exception, "Grant|Rearm"):
            self.review_install(mode="background", activation_id=str(uuid.uuid4()))
        self.assert_review_has_no_effects(allow_reads=True, allow_grant_read=True)
        self.assertEqual(len(json.loads(path.read_bytes())["activations"]), 1)
        manager.grant = grant  # Native Grant model, not a production Rearm call.
        fresh = str(uuid.uuid4())
        result = self.review_install(mode="background", activation_id=fresh)
        self.assertEqual(result["phase"], "settled")
        self.assertNotEqual(fresh, request)
        self.assertEqual(self.review_original(request)["phase"], "unknown")

    @unittest.skipUnless(os.name == "posix", "Unix installer lock")
    def test_review_refuses_unavailable_boot_and_changed_native_caller_without_writes(self):
        request = self.review_unknown(legacy=False)
        manager, path = self.review_manager(), self.review_path()
        before = path.read_bytes()
        boot = manager.consumer["boot"]
        manager.consumer["boot"] = None
        with self.assertRaisesRegex(Exception, "consumer_unavailable"):
            self.review(request)
        self.assertEqual(path.read_bytes(), before)
        manager.consumer["boot"] = boot
        if hasattr(self, "manager"):
            manager.caller_refused = True
        else:
            manager.refuse_caller = True
        with self.assertRaises(Exception):
            self.review(request)
        self.assertEqual(path.read_bytes(), before)
        self.assert_review_has_no_effects()

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux process-death model")
    def test_killed_original_consumer_and_helper_join_precede_metadata_acknowledgement(self):
        manager = self.review_manager()
        consumer = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"],
                                    stdin=subprocess.DEVNULL)
        helper = None
        ctx = multiprocessing.get_context("fork")
        ready, release = ctx.Event(), ctx.Event()
        parent, child = ctx.Pipe(duplex=False)
        request = str(uuid.uuid4())
        try:
            birth = Path(f"/proc/{consumer.pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
            manager.consumer.update(pid=consumer.pid, birth=birth)
            self.review_install()
            def worker():
                try:
                    def pause():
                        ready.set()
                        if not release.wait(5):
                            raise RuntimeError("fixture release missing")
                        if hasattr(self, "manager"):
                            manager.caller_refused = not Path(f"/proc/{consumer.pid}").exists()
                        else:
                            manager.refuse_caller = not Path(f"/proc/{consumer.pid}").exists()
                    manager.after_action = ("daemon-reload" if hasattr(self, "manager") else "bootout", pause)
                    self.review_install(mode="background", activation_id=request)
                    child.send("unexpected completion")
                except Exception as error:
                    child.send(str(error))
            helper = ctx.Process(target=worker)
            helper.start()
            self.assertTrue(ready.wait(5), "original helper did not reach its effect boundary")
            # A different admitted caller cannot use a reboot model to bypass
            # the original helper's still-held whole-effect registration lock.
            manager.consumer.update(pid=os.getpid(), birth="review-native-birth")
            manager.calls.clear()
            self.assertEqual(Path(f"/proc/{consumer.pid}/stat").read_text().rsplit(")", 1)[1].split()[19], birth)
            consumer.kill()  # SAME owned fixture child; never an agent or adopted PID.
            consumer.wait(timeout=3)
            self.assertFalse(Path(f"/proc/{consumer.pid}").exists())
            with self.assertRaises(BlockingIOError):
                self.review(request)
            self.assert_review_has_no_effects()
            release.set()
            self.assertTrue(parent.poll(5), "original helper did not settle")
            self.assertNotEqual(parent.recv(), "unexpected completion")
            helper.join(timeout=3)
            self.assertFalse(helper.is_alive(), "original helper cleanup unconfirmed")
            original = self.review_original(request)
            self.assertEqual((original["phase"], original["result"]), ("unknown", None))
            manager.calls.clear()
            first = self.review(request)
            self.assertEqual(first["disposition"]["state"], "reboot_required",
                             "normal helper join must not qualify abrupt-death cleanup or same-boot acknowledgement")
            self.assert_review_has_no_effects()
            manager.consumer["boot"] = str(uuid.uuid4())  # Different native boot model only.
            acknowledged = self.review(request)
            self.assertEqual((acknowledged["phase"], acknowledged["result"]), ("unknown", None))
            self.assertEqual(acknowledged["disposition"]["state"], "reviewed_unknown")
            self.assert_review_has_no_effects()
        finally:
            release.set()
            if helper is not None:
                helper.join(timeout=1)
                if helper.is_alive():
                    helper.kill()
                    helper.join(timeout=3)
                self.assertFalse(helper.is_alive(), "owned helper was not reaped")
            if consumer.poll() is None:
                consumer.kill()
                consumer.wait(timeout=3)
            parent.close()
            child.close()
