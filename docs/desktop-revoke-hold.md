# Holding the original rollback across a service-manager change

Linux and macOS mode helpers check the selected CLI's exact help line
`Desktop revoke hold ABI: desktop-revoke-hold-v1`. After persisting an original
rollback UUID and obtaining its settled `desktop revoke --json --request-id
UUID --reason rollback` receipt, they start this same selected CLI:

```text
hgs desktop revoke-hold --json --request-id UUID
```

Use the same canonical configuration/state environment. The hold accepts no
caller-supplied writer epoch, consent flag or receipt. It uses the native mode
caller check: non-agent ancestry plus the real foreground controlling terminal
or an actual selected installed frontend ancestor. The checking CLI itself is
not its own frontend ancestor. The helper does not issue another revoke, rotate
the UUID, acquire the service's owner lock or start a service.

Acquisition waits at most two seconds for the actual handoff flock. Under that
exclusion it also acquires the native-host lock when one exists, reads the
database with READ_ONLY/query_only/trusted_schema off, and checks the original
settled rollback fingerprint and receipt. Configuration, original writer epoch,
revoked marker, operation-resource drain and the latest append-only revoke row
must agree. Ownership must still be revoked with no current grant. A fresh
Grant/Rearm/native takeover or later rollback refuses; an old receipt cannot
adopt its runtime. The database is closed before the ready frame so a passive
service restart can write its own startup state.

The child emits one bounded JSON line and flushes it:

```json
{"abi":"desktop-revoke-hold-v1","held":true,"receipt":{"request_id":"original UUID","configuration":"original configuration","writer_epoch":"original writer epoch or null","accepted":true,"durable":true,"revoked":true,"checkpointed":true}}
```

The fields above describe the frame shape; identity values are supplied by the
original stored receipt. This JSON is not a transferable exclusion or authority.
The caller must retain this exact child and its original stdin/stdout/stderr.
Keep its stdin pipe **open** while unloading/stopping, rewriting and restarting
only the owned desktop service into passive mode, through its readiness check.
Check the original child before each manager step and after it returns. A
different process, a copied frame or a new `revoke_status` response cannot
replace it. If that child unexpectedly exits, stop admitting new manager steps
and preserve the pending transition; already begun manager outcomes remain
subject to their original result and inspection.

There is no timeout after ready: it must not release the fence in the middle of
a slow manager operation. Close the original stdin writer only after the
transition has settled or the helper has fenced further operations. EOF releases
the physical locks. The child then flushes a second JSON line with the same
ABI/receipt, `held:false,released:true`, and exits. Join that original child and
drain both original output pipes before releasing its resource charge. Caller
exit closes its pipe and releases the hold; the durable revoke remains. Do not
inherit the pipe's writer into manager children. Unexpected input is discarded
with constant memory but cannot release the fence before EOF; it makes the final
process result an error. No manager commands or input payloads are accepted.

An acquisition refusal emits one JSON line with `held:false`, the ABI,
configuration, original request UUID (null if invalid), and a bounded code such
as `handoff_unavailable`, `writer_authority_changed`, `writer_epoch_changed` or
`revocation_pending`, then exits nonzero. No manager change follows a refusal,
partial frame, timeout, unexpected exit or unknown result. Retain the original
rollback UUID. A missing/older CLI marker stays unavailable; do not replace this
physical hold with a status Boolean or clear a retained revoke.

The Root command is a primitive for maintained mode helpers. Their installer
lock, selected/running mode record, original pending UUID, exact owned unit/plist
checks and manager-result handling remain independently required. It changes no
notification, writer, approval or rearm policy. Windows mode switching is a
separate native integration.
