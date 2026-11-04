"""Receipt identity controls; these tests establish no native caller/manager authority."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import unittest
import uuid
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("service_activation", ROOT / "scripts/desktop-service-activation.py")
activation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(activation)


class ActivationReceipts(unittest.TestCase):
    def setUp(self):
        self.record = {"schema": 1, "configuration": "a" * 64}
        self.writes = []
        self.fail_before = False
        self.fail_after = False
        self.grant = {"configuration": "a" * 64, "revision": 1,
                      **{key: str(uuid.uuid4()) for key in ("journal", "request_id", "owner_epoch")}}
        self.bound = activation.binding("a" * 64, "b" * 64, "c" * 64, self.grant)
        self.journal = self.reopen()

    def save(self, record):
        if self.fail_before:
            raise OSError("controlled pre-write refusal")
        self.record = json.loads(json.dumps(record))
        self.writes.append(copy.deepcopy(self.record))
        if self.fail_after:
            raise OSError("controlled lost write acknowledgement")

    def reopen(self):
        return activation.Journal(lambda: self.record, self.save)

    def test_a_restart_only_looks_up_the_exact_original_and_cannot_complete_it(self):
        request = str(uuid.uuid4())
        original = self.journal.reserve(request, self.bound)
        self.journal.enter_effect(original)
        reopened = self.reopen()
        before = copy.deepcopy(self.record)
        self.assertIsNone(reopened.reserve(request, self.bound))
        self.assertEqual(reopened.lookup(request, "a" * 64)["phase"], "unknown")
        with self.assertRaisesRegex(activation.Refusal, "original_unavailable"):
            reopened.finish(original, ready=True)
        self.assertEqual(self.record, before)
        with self.assertRaisesRegex(activation.Refusal, "original_pending"):
            reopened.reserve(str(uuid.uuid4()), self.bound)

    def test_only_the_original_can_commit_and_a_terminal_receipt_is_immutable(self):
        request = str(uuid.uuid4())
        original = self.journal.reserve(request, self.bound)
        self.journal.enter_effect(original)
        self.journal.finish(original, ready=True)
        result = self.journal.lookup(request, "a" * 64)
        self.assertEqual((result["phase"], result["result"]), ("settled", {"ok": True, "code": "ready"}))
        before = copy.deepcopy(self.record)
        with self.assertRaisesRegex(activation.Refusal, "original_unavailable"):
            self.journal.finish(original, ready=False)
        self.assertIsNone(self.reopen().reserve(request, self.bound))
        self.assertEqual(self.record, before)

    def test_a_known_pre_effect_refusal_never_labels_an_entered_effect_unsent(self):
        request = str(uuid.uuid4())
        original = self.journal.reserve(request, self.bound)
        self.journal.finish(original, ready=False)
        self.assertEqual(self.journal.lookup(request, "a" * 64)["phase"], "refused")
        later = str(uuid.uuid4())
        original = self.journal.reserve(later, self.bound)
        self.journal.enter_effect(original)
        with self.assertRaisesRegex(activation.Refusal, "effect_uncertain"):
            self.journal.finish(original, ready=False)
        self.assertEqual(self.journal.lookup(later, "a" * 64)["phase"], "unknown")
        with self.assertRaisesRegex(activation.Refusal, "already_entered"):
            self.journal.enter_effect(original)

    def test_failed_or_lost_persistence_never_returns_a_new_original(self):
        request = str(uuid.uuid4())
        self.fail_before = True
        with self.assertRaises(OSError):
            self.journal.reserve(request, self.bound)
        self.assertEqual(self.record, {"schema": 1, "configuration": "a" * 64})
        self.fail_before, self.fail_after = False, True
        with self.assertRaises(OSError):
            self.journal.reserve(request, self.bound)
        self.fail_after = False
        self.assertIsNone(self.reopen().reserve(request, self.bound))
        self.assertEqual(self.reopen().lookup(request, "a" * 64)["phase"], "unknown")

    def test_changed_original_scope_and_malformed_grants_refuse_without_writing(self):
        request = str(uuid.uuid4())
        self.journal.reserve(request, self.bound)
        before = copy.deepcopy(self.record)
        changed = copy.deepcopy(self.bound)
        changed["definition"] = "d" * 64
        with self.assertRaisesRegex(activation.Refusal, "original_changed"):
            self.journal.reserve(request, changed)
        with self.assertRaisesRegex(activation.Refusal, "scope_changed"):
            self.journal.lookup(request, "d" * 64)
        with self.assertRaisesRegex(activation.Refusal, "grant_unavailable"):
            activation.binding("a" * 64, "b" * 64, "c" * 64, dict(self.grant, revision=True))
        self.assertEqual(self.record, before)

    def test_bounded_history_preserves_originals_and_missing_lookup_never_reserves(self):
        for _ in range(activation.RECORDS):
            original = self.journal.reserve(str(uuid.uuid4()), self.bound)
            self.journal.finish(original, ready=True)
        before = copy.deepcopy(self.record)
        missing = str(uuid.uuid4())
        self.assertEqual(self.journal.lookup(missing, "a" * 64)["phase"], "missing")
        with self.assertRaisesRegex(activation.Refusal, "history_full"):
            self.journal.reserve(missing, self.bound)
        self.assertEqual(self.record, before)

    def test_conflicting_terminal_fields_and_foreign_configuration_refuse(self):
        request = str(uuid.uuid4())
        original = self.journal.reserve(request, self.bound)
        self.journal.finish(original, ready=True)
        self.record["activations"][request]["result"]["code"] = "activation_not_started"
        with self.assertRaisesRegex(activation.Refusal, "history_unavailable"):
            self.reopen()
        self.record["activations"][request]["result"]["code"] = "ready"
        self.record["configuration"] = "d" * 64
        with self.assertRaisesRegex(activation.Refusal, "history_unavailable"):
            self.reopen()

    def native(self):
        return {"machine": "11111111-1111-4111-8111-111111111111",
                "boot": "22222222-2222-4222-8222-222222222222", "pid": os.getpid(), "birth": "native-birth"}

    def test_legacy_review_requires_an_immutable_reboot_barrier_then_another_explicit_review(self):
        request = str(uuid.uuid4())
        original = self.journal.reserve(request, self.bound)
        original_entry = copy.deepcopy(self.record["activations"][request])
        before = copy.deepcopy(self.record)
        self.assertEqual(self.reopen().lookup(request, "a" * 64)["phase"], "unknown")
        self.assertEqual(self.record, before, "status must not establish a barrier")
        first = self.reopen().review(request, self.native())
        self.assertEqual(first["disposition"]["state"], "reboot_required")
        before = copy.deepcopy(self.record)
        with self.assertRaisesRegex(activation.Refusal, "reboot_required"):
            self.reopen().review(request, self.native())
        self.assertEqual(self.record, before)
        with self.assertRaisesRegex(activation.Refusal, "original_changed"):
            self.journal.finish(original, ready=True)
        after_boot = dict(self.native(), boot=str(uuid.uuid4()))
        acknowledged = self.reopen().review(request, after_boot)
        self.assertEqual(acknowledged["disposition"]["state"], "reviewed_unknown")
        entry = self.record["activations"][request]
        self.assertEqual({key: entry[key] for key in original_entry}, original_entry)
        self.assertIsNone(activation.pending(self.record))
        self.assertTrue(activation.automatic_blocked(self.record))
        self.assertIsNone(self.reopen().reserve(request, self.bound))
        # A new Attempt is possible only when the host explicitly offers a new
        # request under its complete fresh native admission, not from review.
        self.assertEqual(len(self.record["activations"]), 1)
        self.reopen().reserve(str(uuid.uuid4()), self.bound, after_boot)

    def test_unavailable_boot_foreign_machine_and_replaced_barrier_refuse_without_writes(self):
        request = str(uuid.uuid4())
        self.journal.reserve(request, self.bound)
        self.journal.review(request, self.native())
        before = copy.deepcopy(self.record)
        for changed in (dict(self.native(), boot=None), dict(self.native(), machine=str(uuid.uuid4()))):
            with self.assertRaises(activation.Refusal):
                self.reopen().review(request, changed)
            self.assertEqual(self.record, before)
        for field, value in (("request_id", str(uuid.uuid4())),
                             ("binding", dict(self.bound, binary="d" * 64)), ("boot", "")):
            self.record = copy.deepcopy(before)
            self.record["activations"][request]["disposition"][field] = value
            with self.assertRaisesRegex(activation.Refusal, "review_unavailable"):
                self.reopen()

    def test_same_boot_new_consumer_also_requires_reboot_without_any_pid_probe(self):
        request = str(uuid.uuid4())
        self.journal.reserve(request, self.bound, self.native())
        with patch.object(activation.os, "kill", side_effect=AssertionError("no PID-derived acknowledgement")):
            result = self.reopen().review(request, self.native())
            before = copy.deepcopy(self.record)
            with self.assertRaisesRegex(activation.Refusal, "reboot_required"):
                self.reopen().review(request, self.native())
        self.assertEqual(self.record, before)
        self.assertEqual((result["phase"], result["result"]), ("unknown", None))
        self.assertEqual(result["disposition"]["state"], "reboot_required")

    def test_missing_legacy_barrier_cannot_claim_an_unrecorded_boot_boundary(self):
        request = str(uuid.uuid4())
        self.journal.reserve(request, self.bound)
        self.journal.review(request, self.native())
        del self.record["activations"][request]["disposition"]
        after_boot = dict(self.native(), boot=str(uuid.uuid4()))
        result = self.reopen().review(request, after_boot)
        self.assertEqual(result["disposition"]["state"], "reboot_required")
        self.assertEqual(result["disposition"]["boot"], after_boot["boot"])

    def test_captured_native_boot_can_prove_death_without_any_process_lookup(self):
        request = str(uuid.uuid4())
        self.journal.reserve(request, self.bound, self.native())
        with patch.object(activation.os, "kill", side_effect=AssertionError("no PID adoption")):
            result = self.reopen().review(request, dict(self.native(), boot=str(uuid.uuid4())))
        self.assertEqual(result["disposition"]["state"], "reviewed_unknown")


    def test_new_choices_keep_native_scope_linkage_and_reserve_future_review_bytes(self):
        first = str(uuid.uuid4())
        self.journal.reserve(first,self.bound)
        self.journal.review(first,self.native())
        after = dict(self.native(),boot=str(uuid.uuid4()))
        self.journal.review(first,after)
        # An outer registration close to its bound, with real retained entries.
        self.record["padding"] = "x" * 48000
        accepted = 0
        while accepted < activation.RECORDS:
            request = str(uuid.uuid4())
            before = json.dumps(self.record,indent=2) + "\n"
            try:
                original = self.journal.reserve(request,self.bound,after,after_review=first)
            except activation.Refusal as error:
                self.assertIn("history_full",str(error))
                self.assertEqual(json.dumps(self.record,indent=2) + "\n",before)
                break
            retained = copy.deepcopy(self.record)
            # Every admitted original has enough space even if it interrupts.
            reviewed = self.reopen().review(request,dict(after,boot=str(uuid.uuid4())))
            self.assertEqual(reviewed["disposition"]["state"],"reviewed_unknown")
            self.record = retained
            lifecycle = dict(configuration="a"*64,mode="background",ipc_ready=True,
                             epoch=str(uuid.uuid4()),generation=2**53-1)
            self.journal.finish(original,ready=True,lifecycle=lifecycle)
            result = self.journal.lookup(request,"a"*64)
            self.assertEqual(result["after_review"],first)
            self.assertEqual(result["scope"]["owner_epoch"],self.grant["owner_epoch"])
            self.assertEqual(activation.choice_status(self.record)["request"],request)
            accepted += 1
        self.assertGreater(accepted,0)
        self.assertLess(accepted,activation.RECORDS)
        # Scope is observation only, and forged current-Grant linkage refuses.
        broken = copy.deepcopy(self.record)
        last = list(broken["activations"])[-1]
        broken["activations"][last]["scope"]["owner_epoch"] = str(uuid.uuid4())
        with self.assertRaisesRegex(activation.Refusal,"choice_scope_unavailable"):
            activation.checked_entries(broken)


if __name__ == "__main__":
    unittest.main()
