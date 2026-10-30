# Durable desktop read queue

The service's schema 4 includes indexed read intent and exact dispatch receipts.
This is an internal actor API in `zerus_desktop_ledger::reads`. The service still
refuses frontend read commands and starts no delivery workers or fleet source;
writer takeover remains fenced. Existing `hgs read` and client APIs are unchanged.

`zerus-desktop-read-policy` contains the existing client `ReadMark`, field merge,
snapshot settlement, receipt interpretation and retry delays. The client reexports
its previous names and uses the shared policy. The service does not instantiate
the client's in-memory map of all pending reads.

## Intent and dispatch

`enqueue` requires a canonical UUID request and a nonempty, bounded mark naming
computer, session name, run and conversation. Computer/name/run/conversation form
the queue identity. Marks merge only within that identity. Each changed mark gets
a new revision; an unchanged request returns its original receipt even after the
mark is retired. Reusing a request ID for changed input refuses. The actor must
commit with its existing FULL SQLite durability before acknowledging acceptance.
Passive imports can record intent but cannot authorize delivery.

`reserve` checks the current non-revoked writer and service epochs in the same
IMMEDIATE transaction as its reservation. There are four global slots. Each slot
goes to a due computer with the fewest calls in flight, then its oldest due mark.
Indexed host summaries and per-computer due indexes avoid scanning pending marks.
Each returned ticket pins the attempt UUID, both epochs, queue sequence, revision
and exact mark. An attempt UUID is never reused, including after a crash.

The future worker must independently verify current delegation and fresh source
identity before dispatch, pass the exact mark to the selected host's existing
`hgs read`, and honor cancellation and drain. A policy row or this reservation is
not runtime handoff proof. No subprocess is launched by the queue itself.

`finish` requires the exact ticket. A result for an older revision releases only
its owned slot and cannot delay the changed mark. Exact repeat completion is
read-only; a changed result refuses. Revocation permits the original worker to
settle, but prohibits new dispatch. A new service epoch cannot settle an old
reserved attempt. `NotSent` releases a proven undelivered call without counting
a failure. The worker must not release its slot until its owned call has ended.

Failure delays remain 10, 20, 40, 80, 160, then 300 seconds. Confirmed delivery
waits 120 seconds. Partial application waits 120, 240, 480, then 960 seconds.
Changed intent resets the counters and becomes immediately eligible after any
older owned call ends. Clock addition saturates rather than overflowing.

## Observation and restart

A successful delivery receipt never retires a mark. `observe` accepts a fresh
authoritative catalog row in source order and the exact pending revision. It
uses the shared client settlement policy: changed run/conversation, a superseded
part, or confirmation of every still-current part retires the intent. A stale
pending revision cannot clear a newer mark. If a worker is still running, its
slot remains occupied until completion even when the snapshot has settled it.

Startup stays revoked and recovers at most four owned dispatches. Each old
attempt becomes permanently `unknown`; its pending acknowledgement survives
with failure backoff. Only read acknowledgements may receive a new attempt after
unknown delivery, because the host's exact run/conversation read operation is
idempotent. This grants no replay permission for posting, approvals or other
actions. A recovery error pauses this component and is visible in `read_queue`
status while IPC remains available.

## Bounds and proof

Each encoded mark is at most 32 KiB; its identity and individual fields have
smaller limits. Internal pages contain at most 128 rows, bounded by a captured
sequence watermark. They are not desktop-v1 frames; future transport must also
respect that protocol's byte limits. Maintained pending/in-flight totals make
status a constant-size indexed read. Receipt history stays on disk under the
service's database admission limit. Safe retention and allocated database/WAL
pressure qualification remain separate work; unresolved marks and unknown
attempt identities must never be evicted to admit an effect.

Focused tests cover durable reopen, default revocation, exact request/attempt
replay, stale completion and snapshot refusal, changed intent during an owned
call, partial application, fair four-slot admission and SQLite-full rollback.
A 50,000-row fixture verifies indexed host, due and keyset query plans without
table scans or temporary sorting and checks bounded dispatch and pagination.
