"""Original activation receipts inside the maintained registration record.

This is metadata, never caller, Grant, process, manager or drain authority.
The platform helper retains its registration exclusion and performs its native
checks. A restarted helper can only look up an existing attempt; it cannot
reconstruct the original in-memory completion object or repeat that attempt.
"""
import copy
import hashlib
import json
import os
import re
import stat
import uuid

ABI = "desktop-service-activation-v1"
RECORDS = 64
BYTES = 56 * 1024
RECORD_BYTES = 64 * 1024


class Refusal(RuntimeError):
    pass


def require(condition, reason):
    if not condition:
        raise Refusal(reason)


def canonical_uuid(value):
    try:
        return isinstance(value, str) and uuid.UUID(value).int != 0 and str(uuid.UUID(value)) == value
    except (ValueError, AttributeError):
        return False


def digest(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def consumer(value):
    require(isinstance(value, dict) and set(value) == {"machine", "boot", "pid", "birth"}
            and canonical_uuid(value["machine"]) and canonical_uuid(value["boot"])
            and type(value["pid"]) is int and 0 < value["pid"] < 2 ** 31
            and isinstance(value["birth"], str) and 0 < len(value["birth"]) <= 128
            and not any(ord(c) < 32 for c in value["birth"]), "activation_consumer_unavailable")
    return copy.deepcopy(value)


def reviewed(entry):
    return entry.get("disposition", {}).get("state") == "reviewed_unknown"


def automatic_blocked(record):
    return any("disposition" in entry for entry in checked_entries(record).values())


def review_status(record):
    return next(({"request_id": request, "phase": entry["phase"], "result": entry["result"],
                  "disposition": copy.deepcopy(entry["disposition"])}
                 for request, entry in reversed(list(checked_entries(record).items()))
                 if "disposition" in entry), None)


class Image:
    """Bounded digest plus the same retained file, not caller or Grant proof."""
    def __init__(self, path):
        self.path = path
        self.file = open(path, "rb")
        try:
            before = os.fstat(self.file.fileno())
            require(stat.S_ISREG(before.st_mode) and before.st_size <= 512 * 1024 * 1024,
                    "activation_image_unavailable")
            self.identity = self.stamp(before)
            checksum = hashlib.sha256()
            remaining = before.st_size
            while remaining:
                chunk = self.file.read(min(1024 * 1024, remaining))
                require(chunk, "activation_image_changed")
                checksum.update(chunk)
                remaining -= len(chunk)
            require(not self.file.read(1), "activation_image_changed")
            self.check()
            self.digest = checksum.hexdigest()
        except BaseException:
            self.file.close()
            raise

    @staticmethod
    def stamp(info):
        return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)

    def check(self):
        require(self.stamp(os.fstat(self.file.fileno())) == self.identity
                == self.stamp(os.stat(self.path)), "activation_image_changed")

    def close(self):
        self.file.close()


def binding(configuration, binary, definition, grant):
    """Copy only the fixed native helper's checked routing/current Grant data."""
    require(all(digest(value) for value in (configuration, binary, definition)),
            "activation_scope_unavailable")
    require(isinstance(grant, dict) and grant.get("configuration") == configuration
            and all(canonical_uuid(grant.get(key)) for key in ("journal", "request_id", "owner_epoch"))
            and type(grant.get("revision")) is int and 0 < grant["revision"] < 2 ** 53,
            "activation_grant_unavailable")
    return {"configuration": configuration, "binary": binary, "definition": definition,
            "grant": {key: grant[key] for key in ("journal", "request_id", "owner_epoch", "revision")}}


def checked_entries(record):
    require(isinstance(record, dict) and digest(record.get("configuration")),
            "activation_scope_unavailable")
    entries = record.get("activations", {})
    require(isinstance(entries, dict) and len(entries) <= RECORDS,
            "activation_history_unavailable")
    for request, entry in entries.items():
        require(canonical_uuid(request) and isinstance(entry, dict)
                and {"binding", "phase", "result"} <= set(entry)
                and set(entry) <= {"binding", "phase", "result", "consumer", "disposition"}
                and entry["phase"] in ("unknown", "settled", "refused"),
                "activation_history_unavailable")
        bound = entry["binding"]
        require(isinstance(bound, dict) and set(bound) == {"configuration", "binary", "definition", "grant"}
                and isinstance(bound["grant"], dict), "activation_history_unavailable")
        copied = binding(bound["configuration"], bound["binary"], bound["definition"],
                         dict(bound["grant"], configuration=bound["configuration"]))
        require(bound == copied and bound["configuration"] == record["configuration"]
                and (entry["result"] is None) == (entry["phase"] == "unknown"),
                "activation_history_unavailable")
        if "consumer" in entry:
            consumer(entry["consumer"])
        if "disposition" in entry:
            review = entry["disposition"]
            require(entry["phase"] == "unknown" and isinstance(review, dict)
                    and set(review) == {"state", "request_id", "binding", "machine", "boot"}
                    and review["state"] in ("reboot_required", "reviewed_unknown")
                    and review["request_id"] == request and review["binding"] == bound
                    and canonical_uuid(review["machine"]) and canonical_uuid(review["boot"])
                    and ("consumer" not in entry
                         or review["machine"] == entry["consumer"]["machine"]),
                    "activation_review_unavailable")
        if entry["result"] is not None:
            require(isinstance(entry["result"], dict)
                    and set(entry["result"]) == {"ok", "code"}
                    and type(entry["result"]["ok"]) is bool
                    and entry["result"]["ok"] == (entry["phase"] == "settled")
                    and entry["result"]["code"] == ("ready" if entry["phase"] == "settled"
                                                    else "activation_not_started"),
                    "activation_history_unavailable")
    require(len(json.dumps(entries, indent=2).encode()) <= BYTES, "activation_history_unavailable")
    return entries


def pending(record):
    """Recover only an original ID for lookup, never its live execution object."""
    return next((request for request, entry in reversed(list(checked_entries(record).items()))
                 if entry["phase"] == "unknown" and not reviewed(entry)), None)


class Attempt:
    """Process-local completion identity, not an execution permission."""
    def __init__(self, journal, request, bound):
        self.journal, self.request, self.bound = journal, request, copy.deepcopy(bound)
        self.completed = False
        self.effect_started = False


class Journal:
    def __init__(self, current_record, persist):
        checked_entries(current_record())
        self.current_record, self.persist = current_record, persist
        self.originals = {}

    @property
    def record(self):
        return self.current_record()

    def save(self, record):
        # Keep the stricter Linux record bound, including the actual pretty
        # serialization, so this metadata cannot make its own host unreadable.
        require(len((json.dumps(record, indent=2) + "\n").encode()) <= RECORD_BYTES,
                "activation_history_full")
        self.persist(record)
        require(self.record == record, "activation_record_changed")

    def lookup(self, request, configuration):
        require(canonical_uuid(request) and digest(configuration), "activation_scope_unavailable")
        require(self.record.get("configuration") == configuration, "activation_scope_changed")
        entry = checked_entries(self.record).get(request)
        require(entry is None or entry["binding"]["configuration"] == configuration,
                "activation_scope_changed")
        result = {"abi": ABI, "request_id": request, "configuration": configuration,
                "read_only": True, "phase": entry["phase"] if entry else "missing",
                "result": copy.deepcopy(entry["result"]) if entry else None}
        if entry and "disposition" in entry:
            result["disposition"] = copy.deepcopy(entry["disposition"])
        return result

    def reserve(self, request, bound, origin=None):
        require(canonical_uuid(request), "activation_request_unavailable")
        # Validate the complete binding before touching the retained record.
        probe = {"configuration": self.record["configuration"],
                 "activations": {request: {"binding": bound, "phase": "unknown", "result": None}}}
        if origin is not None:
            probe["activations"][request]["consumer"] = consumer(origin)
        checked_entries(probe)
        entries = checked_entries(self.record)
        if request in entries:
            require(entries[request]["binding"] == bound, "activation_original_changed")
            return None  # Exact original-ID lookup only, including after restart.
        require(len(entries) < RECORDS, "activation_history_full")
        require(not any(entry["phase"] == "unknown"
                        and not reviewed(entry)
                        and entry["binding"]["grant"]["owner_epoch"] == bound["grant"]["owner_epoch"]
                        for entry in entries.values()), "activation_original_pending")
        next_record = copy.deepcopy(self.record)
        next_record.setdefault("activations", {})[request] = probe["activations"][request]
        checked_entries(next_record)
        self.save(next_record)  # Host performs original native checks and durable write.
        original = Attempt(self, request, bound)
        self.originals[request] = original
        return original

    def review(self, request, current):
        """Explicit metadata only; host MUST hold the whole-effect installer
        lock and recheck its actual native caller through the durable save.

        Legacy entries have no process age. The first Review creates a reboot
        barrier, and only a later different native boot can acknowledge it.
        This cannot reconstruct an Attempt or release any resource charge.
        """
        current = consumer(current)
        entry = checked_entries(self.record).get(request)
        require(canonical_uuid(request) and entry is not None and entry["phase"] == "unknown",
                "activation_original_unavailable")
        review = entry.get("disposition")
        origin = entry.get("consumer")
        require((origin is None or origin["machine"] == current["machine"])
                and (review is None or review["machine"] == current["machine"]),
                "activation_machine_changed")
        if reviewed(entry):
            return self.lookup(request, self.record["configuration"])
        if origin is not None and origin["boot"] != current["boot"]:
            next_review = {"state": "reviewed_unknown", "request_id": request,
                           "binding": copy.deepcopy(entry["binding"]),
                           "machine": origin["machine"], "boot": origin["boot"]}
        elif review is None:
            # A released helper lock does not prove that an orphaned manager
            # child or its pipes have joined after abrupt helper death. This
            # slice deliberately exposes NO same-boot acknowledgement.
            next_review = {"state": "reboot_required", "request_id": request,
                           "binding": copy.deepcopy(entry["binding"]),
                           "machine": current["machine"], "boot": current["boot"]}
        else:
            require(review["boot"] != current["boot"],
                    "activation_reboot_required: restart the computer, then explicitly Review again")
            next_review = dict(review, state="reviewed_unknown")
        next_record = copy.deepcopy(self.record)
        next_record["activations"][request]["disposition"] = next_review
        checked_entries(next_record)
        self.save(next_record)
        return self.lookup(request, self.record["configuration"])

    def original(self, original):
        require(isinstance(original, Attempt) and original.journal is self
                and self.originals.get(original.request) is original and not original.completed,
                "activation_original_unavailable")
        entries = checked_entries(self.record)
        entry = entries.get(original.request)
        require(entry is not None and entry["phase"] == "unknown"
                and "disposition" not in entry
                and entry["binding"] == original.bound, "activation_original_changed")
        return entry

    def enter_effect(self, original):
        """Mark locally BEFORE the first definition/manager mutation, never after it."""
        self.original(original)
        require(not original.effect_started, "activation_effect_already_entered")
        original.effect_started = True

    def finish(self, original, *, ready):
        self.original(original)
        require(type(ready) is bool and (ready or not original.effect_started),
                "activation_effect_uncertain")
        next_record = copy.deepcopy(self.record)
        next_record["activations"][original.request].update(
            phase="settled" if ready else "refused",
            result={"ok": bool(ready), "code": "ready" if ready else "activation_not_started"})
        self.save(next_record)
        original.completed = True
