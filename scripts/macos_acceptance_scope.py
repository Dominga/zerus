"""Read-only identity checks for the external installed-Mac acceptance hook.

These observations grant no authority. Root still validates the original
command, writer, service incarnation and physical resource drain itself.
"""
from dataclasses import dataclass
import json
import re
import uuid


def require(value, code):
    if not value:
        raise ValueError(code)


def canonical_uuid(value):
    try:
        return isinstance(value, str) and str(uuid.UUID(value)) == value
    except (ValueError, TypeError, AttributeError):
        return False


def settled(value):
    return bool(value) and all(value.get(key) is True for key in
                               ("accepted", "durable", "revoked", "checkpointed"))


@dataclass(frozen=True)
class RevokeScope:
    configuration: str
    service_epoch: str
    writer_epoch: str
    grant_request_id: str

    def __post_init__(self):
        require(isinstance(self.configuration, str)
                and re.fullmatch(r"[0-9a-f]{64}", self.configuration), "revoke_configuration")
        require(all(canonical_uuid(value) for value in
                    (self.service_epoch, self.writer_epoch, self.grant_request_id)), "revoke_epoch")

    def same_service(self, status):
        return (isinstance(status, dict)
                and (status.get("configuration"), status.get("epoch"))
                == (self.configuration, self.service_epoch))

    def record(self):
        return dict(configuration=self.configuration, service_epoch=self.service_epoch,
                    writer_epoch=self.writer_epoch, grant_request_id=self.grant_request_id)

    def parse_reply(self, raw, original):
        require(canonical_uuid(original), "revoke_request_identity")
        require(isinstance(raw, bytes) and len(raw) <= 4096, "revoke_reply_bound")
        def unique(pairs):
            value = {}
            for key, item in pairs:
                require(key not in value, "duplicate_revoke_field")
                value[key] = item
            return value
        value = json.loads(raw, object_pairs_hook=unique)
        fields = {"request_id", "configuration", "accepted", "durable", "revoked",
                  "checkpointed", "writer_epoch", "code"}
        require(isinstance(value, dict) and set(value) <= fields
                and value.get("request_id") == original, "revoke_reply_identity")
        for field in ("accepted", "durable", "revoked", "checkpointed"):
            require(field not in value or type(value[field]) is bool, "revoke_reply_schema")
        require("code" not in value or isinstance(value["code"], str), "revoke_reply_schema")
        if settled(value):
            require(value.get("configuration") == self.configuration
                    and value.get("writer_epoch") == self.writer_epoch, "revoke_scope_mismatch")
        return value


def capture(status, original_service):
    """Pin the current Grant from one status snapshot before Quit/revoke.

    The original Popen and transport remain the caller's responsibility. Never
    call this to rebind a spent command to a later Grant or replacement service.
    """
    require(isinstance(status, dict) and isinstance(original_service, tuple)
            and len(original_service) == 2, "revoke_service_scope")
    configuration, service_epoch = original_service
    require((status.get("configuration"), status.get("epoch")) == original_service,
            "revoke_service_changed")
    ownership = status.get("desktop_ownership")
    require(isinstance(ownership, dict)
            and ownership.get("abi") == "desktop-ownership-v1"
            and ownership.get("configuration") == configuration
            and ownership.get("state") == "service"
            and ownership.get("identity_current") is True
            and ownership.get("rearm_required") is False
            and type(ownership.get("revision")) is int
            and 0 < ownership["revision"] < 2 ** 53
            and canonical_uuid(ownership.get("journal")), "revoke_original_grant")
    return RevokeScope(configuration, service_epoch, ownership.get("owner_epoch"),
                       ownership.get("request_id"))
