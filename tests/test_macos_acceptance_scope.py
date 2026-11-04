"""Portable regressions for the external Mac acceptance receipt boundary."""
from dataclasses import FrozenInstanceError
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("macos_acceptance_scope",
                                            ROOT / "scripts/macos_acceptance_scope.py")
scope = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = scope
spec.loader.exec_module(scope)
CONFIG = "c" * 64
SERVICE = "11111111-1111-4111-8111-111111111111"
WRITER = "22222222-2222-4222-8222-22222222222a"
GRANT = "33333333-3333-4333-8333-333333333333"
REQUEST = "44444444-4444-4444-8444-444444444444"


def status():
    return dict(configuration=CONFIG, epoch=SERVICE, desktop_ownership=dict(
        abi="desktop-ownership-v1", configuration=CONFIG, state="service",
        identity_current=True, rearm_required=False, revision=1,
        journal="55555555-5555-4555-8555-555555555555", owner_epoch=WRITER,
        request_id=GRANT))


def receipt(**changes):
    value = dict(request_id=REQUEST, configuration=CONFIG, writer_epoch=WRITER,
                 accepted=True, durable=True, revoked=True, checkpointed=True)
    value.update(changes)
    return json.dumps(value).encode()


class MacAcceptanceScope(unittest.TestCase):
    def capture(self, value=None):
        return scope.capture(status() if value is None else value, (CONFIG, SERVICE))

    def test_original_writer_receipt_settles_when_service_epoch_is_different(self):
        binding = self.capture()
        self.assertNotEqual(SERVICE, WRITER)
        try:
            result = binding.parse_reply(receipt(), REQUEST)
        except ValueError as error:
            self.fail(f"The original settled writer receipt was refused: {error}")
        self.assertTrue(scope.settled(result))
        self.assertEqual(result["writer_epoch"], WRITER)
        self.assertTrue(binding.same_service(status()))
        self.assertEqual(binding.record(), dict(configuration=CONFIG, service_epoch=SERVICE,
                                                writer_epoch=WRITER, grant_request_id=GRANT))

    def test_service_epoch_is_never_substituted_for_the_original_writer(self):
        with self.assertRaisesRegex(ValueError, "revoke_scope_mismatch"):
            self.capture().parse_reply(receipt(writer_epoch=SERVICE), REQUEST)

    def test_replacement_service_refuses_even_with_the_same_writer(self):
        replacement = status()
        replacement["epoch"] = GRANT
        self.assertFalse(self.capture().same_service(replacement))
        with self.assertRaisesRegex(ValueError, "revoke_service_changed"):
            self.capture(replacement)

    def test_wrong_configuration_writer_or_request_refuses_settled_receipt(self):
        for changed in (dict(configuration="d" * 64), dict(writer_epoch=GRANT),
                        dict(writer_epoch=None), dict(request_id=GRANT)):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                self.capture().parse_reply(receipt(**changed), REQUEST)

    def test_missing_revoked_or_uncurrent_grant_cannot_establish_a_writer(self):
        cases = [dict(abi="unknown"), dict(configuration="d" * 64), dict(state="revoked"),
                 dict(identity_current=False), dict(identity_current=1), dict(rearm_required=True),
                 dict(owner_epoch=None), dict(owner_epoch=WRITER.upper()), dict(request_id=None),
                 dict(journal=None), dict(revision=True), dict(revision=0), dict(revision=2 ** 53)]
        for change in cases:
            value = status(); value["desktop_ownership"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.capture(value)
        value = status(); del value["desktop_ownership"]
        with self.assertRaises(ValueError):
            self.capture(value)

    def test_later_grant_does_not_change_the_retained_original(self):
        value = status(); binding = self.capture(value)
        value["desktop_ownership"].update(owner_epoch=GRANT, request_id=REQUEST)
        self.assertTrue(scope.settled(binding.parse_reply(receipt(), REQUEST)))
        with self.assertRaises(ValueError):
            binding.parse_reply(receipt(writer_epoch=GRANT), REQUEST)
        with self.assertRaises(FrozenInstanceError):
            binding.writer_epoch = GRANT

    def test_pending_or_refused_receipt_never_claims_settlement(self):
        for values in (dict(accepted=True, durable=True, revoked=False, checkpointed=False),
                       dict(accepted=False, durable=False, revoked=False, checkpointed=False)):
            raw = json.dumps(dict(request_id=REQUEST, code="revocation_pending", **values)).encode()
            self.assertFalse(scope.settled(self.capture().parse_reply(raw, REQUEST)))

    def test_closed_bounded_stdout_does_not_accept_child_text_or_coercion(self):
        duplicate = ('{"request_id":"' + REQUEST
                     + '","accepted":false,"accepted":true}').encode()
        for raw in (receipt() + b"\nchild guidance", b"x" * 4097, b"[]", duplicate,
                    receipt(extra=True), receipt(accepted=1), receipt(code={}),
                    receipt().decode()):
            with self.subTest(kind=type(raw).__name__), self.assertRaises((ValueError, TypeError)):
                self.capture().parse_reply(raw, REQUEST)


if __name__ == "__main__":
    unittest.main()
