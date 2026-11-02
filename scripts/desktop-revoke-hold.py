#!/usr/bin/env python3
"""Retain one original native rollback exclusion through a manager transition."""
import json
import os
import selectors
import subprocess
import time
import uuid

ABI = "desktop-revoke-hold-v1"
MARKER = "Desktop revoke hold ABI: " + ABI
LINE = 8192
OUTPUT = 65536


class Refusal(Exception):
    pass


def require(condition, message):
    if not condition:
        raise Refusal(message)


class Hold:
    def __init__(self, hgs, env, configuration, request_id, receipt, help_text,
                 *, popen=subprocess.Popen, timeout=10):
        require(MARKER in (line.strip() for line in help_text.splitlines()),
                "Selected CLI lacks the native rollback hold; update it before changing mode.")
        require(isinstance(receipt, dict) and receipt.get("request_id") == request_id
                and receipt.get("configuration") == configuration
                and "writer_epoch" in receipt
                and all(receipt.get(k) is True for k in ("accepted", "durable", "revoked", "checkpointed")),
                "Original rollback receipt is not settled; retain its request UUID.")
        try:
            require(str(uuid.UUID(request_id)) == request_id, "Invalid original rollback UUID.")
            epoch = receipt["writer_epoch"]
            require(epoch is None or str(uuid.UUID(epoch)) == epoch, "Invalid original writer epoch.")
        except (TypeError, ValueError, AttributeError):
            raise Refusal("Invalid original rollback identity.") from None
        self.receipt = dict(receipt)
        self.timeout = timeout
        self.child = popen([str(hgs), "desktop", "revoke-hold", "--json", "--request-id", request_id],
                           env=dict(env), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, close_fds=True)
        self.selector = selectors.DefaultSelector()
        self.frames, self.pending = [], bytearray()
        self.total, self.error, self.ready, self.finished = 0, None, False, False
        for stream, name in ((self.child.stdout, "out"), (self.child.stderr, "err")):
            os.set_blocking(stream.fileno(), False)
            self.selector.register(stream, selectors.EVENT_READ, name)

    def pump(self, timeout):
        for key, _ in self.selector.select(timeout):
            try:
                raw = os.read(key.fd, 4096)
            except BlockingIOError:
                continue
            if not raw:
                self.selector.unregister(key.fileobj)
                if key.data == "out" and self.pending:
                    self.error = "Partial rollback hold frame; inspect the pending transition."
                continue
            self.total += len(raw)
            if self.total > OUTPUT:
                self.error = "Rollback hold output exceeded 64 KiB."
            if key.data != "out" or self.error:
                continue
            self.pending.extend(raw)
            while b"\n" in self.pending:
                line, _, rest = self.pending.partition(b"\n")
                self.pending = bytearray(rest)
                try:
                    require(len(line) <= LINE and len(self.frames) < 2, "Invalid rollback hold framing.")
                    frame = json.loads(line.decode("utf-8", "strict"))
                    require(isinstance(frame, dict), "Invalid rollback hold frame.")
                    self.frames.append(frame)
                except (Refusal, ValueError, UnicodeError, RecursionError):
                    self.error = "Invalid rollback hold frame; retain the original request UUID."
                    self.pending.clear()
                    break
            if len(self.pending) > LINE:
                self.error = "Rollback hold frame exceeded 8 KiB."
                self.pending.clear()

    def acquire(self):
        deadline = time.monotonic() + self.timeout
        try:
            while not self.frames and not self.error and self.selector.get_map():
                remaining = deadline - time.monotonic()
                require(remaining > 0, "Native rollback hold did not become ready; keep its original UUID.")
                self.pump(min(remaining, 0.1))
            require(not self.error, self.error)
            require(self.frames and self.frames[0] == {"abi": ABI, "held": True, "receipt": self.receipt},
                    "Original rollback hold refused or changed; do not alter the service manager.")
            self.ready = True
            self.check()
            return self
        except BaseException:
            self.close(validate=False)
            raise

    def check(self):
        require(not self.finished and self.ready, "Original rollback hold is unavailable.")
        # Root emits nothing between Ready and the original stdin EOF. There
        # is deliberately no post-ready deadline that can expire the hold.
        self.pump(0)
        require(not self.error, self.error)
        require(self.child.poll() is None and len(self.frames) == 1
                and self.child.stdout in self.selector.get_map(),
                "Original rollback hold exited or released; stop and inspect the pending transition.")

    def close(self, *, validate=True):
        if self.finished:
            return
        try:
            try:
                self.child.stdin.close()  # EOF only; no command bytes or inherited writer.
            except BrokenPipeError:
                pass
            deadline = time.monotonic() + self.timeout
            while self.selector.get_map():
                remaining = deadline - time.monotonic()
                require(remaining > 0, "Original rollback hold pipes did not drain; transition remains pending.")
                self.pump(min(remaining, 0.1))
            code = self.child.wait(timeout=max(0.01, deadline - time.monotonic()))
            if validate:
                require(not self.error, self.error)
                require(self.ready and code == 0 and self.frames == [
                    {"abi": ABI, "held": True, "receipt": self.receipt},
                    {"abi": ABI, "held": False, "released": True, "receipt": self.receipt}],
                    "Original rollback release/exit is unverified; transition remains pending.")
        finally:
            # On a protocol failure no further manager steps are admitted.
            # Cleanup targets this original CLI child alone, never a group.
            if self.child.poll() is None:
                self.child.kill()
                self.child.wait(timeout=2)
            self.selector.close()
            self.child.stdin.close()
            self.child.stdout.close()
            self.child.stderr.close()
            self.finished = True
