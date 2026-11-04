"""Receipt identity controls; these tests establish no native caller/manager authority."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
import uuid

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


if __name__ == "__main__":
    unittest.main()
