# Native Task Start reconciliation boundary

`desktop::task_start_reconcile::Intent` is the closed input for one explicitly
requested `TaskStartReconcile` kind. The profile name `task-start-reconcile-v1`
is definition data. It does not extend `BACKGROUND_V1`, confer general
Operations permission or turn a notification Grant into execution consent.

Linux and macOS have a guarded, fixed recover-only actor. The ordinary CLI
request lane still refuses `frontend_authentication_required`, and a passive
service retains `desktop_background_unavailable`. Windows remains
`task_start_reconcile_unsupported` until it has equivalent physical custody.
The native renderer policy route cannot submit this kind. The dedicated native
Task Start command below supplies the actual registered helper and original
checkpoint; it adds no general Operations permission.

## Original identity and bytes

The request contains a service `Scope`, a fresh `reconcile_id`, the existing
Task Start policy `Binding` and the exact producer checkpoint as `original`.
The reconciliation ID differs from the original task-delivery UUID. It cannot
replace that original, an earlier admission ID or an attempt already reserved.
Binding project identity is the canonical `intent.plan.project`, not its display
caption or a newly resolved alias.
Scope configuration must match Binding configuration; native admission also
checks it against the actual cached Service-mode frontend.

The verifier takes input only from the checkpoint's `intent.retained_input`
string. It never serializes `intent.plan` to reconstruct a missing input. It
retains the separate complete-source, UI-snapshot and input fingerprints and
uses the existing pure Task Start policy to check the full original plan and
executor identities. A historical terminal record is not a current reconcile
request. Duplicate fields anywhere in the embedded original or input refuse
this wire route; generic outbox and G3 decoders and opaque bytes are unchanged.
Debug output omits the original body and input.

The embedded original is at most 32 KiB, as in the creation wire routes, inside
the existing 64 KiB control framing. No frame limit increases. The complete
encoded native request must still fit that existing framing; the
field limit never waives it. The producer's
1 MiB reconciliation-record/input bounds, 31 MiB native plans and 32 MiB native
outboxes remain distinct. Larger originals need bounded native-managed custody
with exact object/hash binding before a service can consume them; no caller
path or truncated original substitutes for that mechanism. Legacy missing input
and unsupported sizes stay available for ordinary native review.

## Current admission and custody

The request has its own exhaustive ordering lane and a typed native `Validated`
accessor. Generic request consumption and renderer policy dispatch refuse it.
The native responder must retain its actual registered child and cached
Service/configuration evidence. None of the wire values can construct a permit.

The actor acquires `BEGIN IMMEDIATE`, then rechecks the original opaque
two-second proof, actual serve Owner, configuration, writer, exclusion and live
local executor/routing binding. Admission, reservation and exact prepared custody
must commit together before spawning. Existing operation bytes, admission ID and
revision remain immutable; coalescing is observation only. Every later explicit
attempt requires fresh live Consent and previous physical drain. A whole SQL
transaction rollback requires abandoning that transaction before any further
write, including refusal bookkeeping. Preparation of the exact custody file
runs off actor while the original permit and bounded reply queue remain held.
Preparation does not extend the permit. An expired proof, failed current guard,
failed admission or failed custody insert commits no operation or attempt and
starts no helper. The response is `task_start_reconcile_unavailable`.

The live lease uses the actual locally owned Fleet source, original serve-owner
`Arc<FileLock>` and original handoff exclusion. It checks the current durable
service and writer binding. The task consent remains kind-specific, process-local
and absent after restart. An existing operation keeps its original admission ID,
raw/input/provenance bytes and binding. A new explicit reconciliation ID can
reserve a separate attempt only after previous physical drain. Repeating an
existing attempt ID returns its original state read-only and starts no worker.

Executor preparation reads only the exact selected name's private saved binding,
at most 1 MiB, through retained directory handles and nonblocking no-follow opens.
It does not enumerate sessions, follow replacement names or call the binding
reader which repairs provider state. The local route, machine, session, run and
conversation must match the original. A tracked live agent and its original hgs
supervisor are pinned by native birth, parent edge and image identity; the agent's
native environment must contain exactly the original run marker. Both processes
and the selected record are checked again after the writer lock and before input
release. Missing, paused, archived, external, changed or unverifiable executor
layouts remain unavailable. A historical sent receipt proves no current process.

The supervisor check compares the exact executable file object with the running
service's image. Agents launched by an earlier hgs image can therefore remain
ineligible for a new reconciliation after an update. They keep running; an
update never restarts them to regain eligibility. Manual review remains
available until their normal user-directed lifecycle changes the supervisor.
Historical result reads below do not require or establish a live executor.

The only permitted worker is the unchanged original
`hgs task start-deliver TASK --project PROJECT --json --recover-only` input.
It is a metadata reconciliation effect: existing receipt checks may record task
send/queue metadata and exchange task metadata. It does not authorize fresh
Start, typing, plan regeneration, release or a new task-delivery UUID. Historical
send proof supplies neither current executor liveness nor physical drain.

The agreed worker bounds remain one per configuration, 32 unsettled operations,
30 seconds, 1 MiB stdout and 16 KiB stderr. The original child must be registered
before its retained input is released to stdin. Original stdin, child, output
pipes and inherited custody retain the service/exclusion resources until actual
joins. Revoke, restart or a returned outcome cannot reconstruct Consent, replay
an unknown attempt or clear its charge from an absent PID. Kill/revoke/restart,
lost replies and proof expiry after BEGIN are separate controls from ordinary
successful metadata reconciliation. Background stop, revoke and executable
replacement use the same `drain`/`poll_drain` path. A returned outcome never drops
the original serve-owner/handoff loan before original child, input and output
resources finish and the exact existing custody lock can be acquired. A failed
settlement keeps the original loan and charge for recovery.

## Private qualification

The actor controls use synthetic native admission/source fixtures with actual
kernel process identities, SQLite, children, pipes and inherited flock custody.
They cover commit failure, current writer/executor changes, original-attempt
coalescing, a later explicit attempt without re-admission, revoke, a retained
descendant pipe, terminal Unknown and expiry while waiting for BEGIN IMMEDIATE.
They are not an installed frontend or native-agent compatibility positive.

After building the production CLI, `scripts/ci/cli.sh` runs three exact opt-in
tests. Two run on Linux/macOS: the fixed real `task start-deliver --recover-only`
reconciles an existing private native-receipt fixture, while a missing receipt
stays uncertain without touching a terminal; exact child kill and revoke before
input keep Unknown across a real Store reopen. The Linux-only case kills one
original private actor process, retains its exact CLI helper birth/custody, then
opens a new service epoch. The new actor never reconstructs consent or launches
another helper. It retires the charge only after actual original exit and exact
kernel-lock recovery. Its marked leaf is bounded and cannot spawn itself; its
original agent and helper are reaped by the original private parent. These tests
use fixture receipts and no native terminal submission, installed service, GUI,
platform manager or owner's agent.

The current pure Task Start policy still excludes task-reserved new-session
creation. Its original launch-result boundary and native caller integration
remain separate. Larger originals and Windows custody also remain unavailable;
no missing evidence becomes a launcher or an automatic retry.

## Original-attempt result reads

`task_start_reconcile_result` is a distinct bounded indexed read, including on
a passive service. Its closed `Query` carries the fresh selected service scope,
an independent read UUID, the original reconciliation UUID and its full binding.
The service reads its current scope, the immutable stored ticket, frozen original
and outcome under one SQLite snapshot. It verifies the original raw/UI/input
digests and exact admission/revision/binding before returning metadata.

`desktop-task-start-result-v1` returns at most 16 KiB. It contains only the exact
stored binding, ticket, phase, closed outcome and durable drain flag; source,
input, provenance and private task/error views are absent. An old ticket's
service/writer epochs and admission ID stay original even when the query uses a
new service epoch. Unknown cannot become submitted because of a later read.
Only exact submitted metadata together with actual durable drain is eligible
for the native caller's CAS of its still-unchanged outbox. The redacted
five fields are never expanded into a canonical CLI reply.

Missing is only an observation. It proves no absence of an earlier effect,
current executor, physical drain or permission to offer again. This path cannot
admit/reserve, start a worker, release a record or mint a reconciliation ID. A
changed/malformed query or stored binding returns
`task_start_result_unavailable`. The native caller retains its offered-unknown journal and original selected-service
resources through the exchange. The per-kind marker below describes this caller
and result route together; it supplies no admission or execution authority.

## Native checkpoint capture

On Unix, `task_start::capture::Captured` reads the native selected draft store's
fixed task-start record and reservation marker. Project/task identity derives
the basenames; a renderer cannot select a source path or supply retained input.
The managed data alias resolves once to its retained canonical directory;
retargeting the selected alias refuses instead of adopting another source.
Private task-start directories and both single-link regular files are opened
relative to retained handles with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` together.
FIFO, symlink, shared, missing and oversized sources refuse before reading.

The complete exact record is limited to 32 KiB. Capture separately binds raw
bytes, the existing UI snapshot, unchanged retained input and original delivery
ID. It rejects duplicate JSON fields, including in retained input. It preserves
opaque fields and whitespace instead of serializing a plan into new call bytes.
The retained file objects, sizes, ownership, permissions and modification/change
versions are checked again before constructing the fixed data Intent. An
identical-byte replacement is still a different source. A frozen `Version` is
metadata for the future native journal; decoding it grants no file authority.

Capture creates or repairs nothing, submits nothing and cannot settle or release
an outbox. Its Scope is data: the eventual native caller must separately retain
and recheck the actual selected Service and the complete unchanged control-frame
limit. The separate native caller below owns the durable offered-unknown journal,
actual native offer and exact result-to-outbox CAS. Capture itself advertises
no capability and authorizes none of those actions.

## Native original-input journal

`capture::journal` stores one bounded private `.reconcile` record beside the
native task-start record, with a separate CLOEXEC metadata lock. It retains the
exact original bytes, binding and native version plus one reconciliation UUID;
it creates no Root operation or worker. The complete encoded journal is limited
to 96 KiB and uses one fixed hash-derived slot per task, without discovery or
history scans. An invalid preparation writes nothing. A missing journal beside
an existing lock is uncertainty and cannot reserve another UUID.

`Prepared` is durable before preparation returns. Before returning the fixed
data Intent, the writer must persist `OfferedUnknown` with a file sync, atomic
rename and parent-directory sync. A failed sync returns no Intent. The original
native source, journal object and metadata lock are checked around publication.
The native caller still needs its actual selected-Service/current admission;
neither this phase nor an Intent is execution permission.

After an offer, GUI exit, a missing lookup, a changed executor or service restart
can only lead to bounded queries with the same original reconciliation UUID and
binding. Saved data is readable without a live executor or extant outbox. It
cannot infer no effect, change authority, replay the offer or reconstruct a task
reply. A private-process kill control verifies the offered record survives the
original writer's actual exit.

Only a separate explicitly confirmed discard can retire a `Prepared` CHECK. It
writes `CancelledBeforeOffer`, retains the original identity, and touches neither
the task/outbox nor a Root receipt. `OfferedUnknown` cannot be discarded. A later
explicit Check must match that cancellation tombstone before reserving a new
reconciliation ID. Stale snapshots and missing metadata never authorize this
transition; one previous cancellation ID stays in the bounded record.

The journal has no renderer operation or independent execution authority. The
native caller owns the offer and connection checks; completion below allows a
genuinely new delivery without clearing an unsettled original.


## Explicit native receipt check and settlement

In desktop-service mode on Linux and macOS, the Task Start review offers
**Check delivery receipt**. Opening a task only reads its local view and journal;
it does not submit a check. The renderer passes the expected task, original
native delivery ID and UI snapshot, never a path, input body or reconciliation
ID. The command selects the managed draft store and actual native Service itself.
Windows remains unavailable.

Before changing the journal, the native command probes the exact retained helper
for `Desktop task start reconcile ABI: desktop-task-start-reconcile-v1` and the
existing native frontend-request ABI. An older helper refuses with update guidance.
Both the source field and the complete encoded request retain their existing
32 KiB and 64 KiB bounds. The complete-envelope preflight is defence in depth:
with today's fixed fields the reviewed quote-filled source (32,722 bytes)
produces a 64,938-byte envelope, below the control bound. It remains enforced if
fields grow; direct oversized helper input is also refused before spawn.
The actual cached Service configuration, epoch, generation, cancellation object,
relay and helper image are retained and checked around every call. The helper's
original Child is registered before its input is released. Refused registration
withholds stdin while that same Child remains owned for stop/reap and pipe joins.
The native task-write permit and original registration resources stay charged
through those physical joins even after a logical failure.

A prepared check keeps its reconciliation ID. The native command durably marks
it offered before the native request and follows a lost reply only with a bounded
query of that same ID. GUI exit or restart, missing results and terminal Unknown
never offer it again. Pending or submitted-but-undrained results may be queried
within the fixed polling bound; they cannot settle the task. A fresh connection
can read the original stored result without rebinding its original admission,
writer, operation or delivery identities.

An exact Submitted result with actual stored drain first persists
`SettlementPrepared` and that exact result metadata. Under the native metadata
lock and the existing in-process outbox writer lane, settlement rechecks the
original managed directories, reservation marker and exact source file/version.
It atomically changes only `phase` to `started` and clears `error`; all original
input, plan and opaque fields are preserved. It never reconstructs a canonical
CLI reply from redacted result fields. A changed same-ID source, alias, marker,
file object or native connection refuses. After the source and directory sync,
`Completed` is persisted. A crash after the source CAS can finish this metadata
from the exact deterministic settled source without writing that source twice.
A genuinely new native delivery is preserved and needs a separately explicit
new Check after the previous completion; an unsettled offer cannot be replaced.

Only an explicit confirmation can discard a prepared, unsent check. The
confirmation binds the current task's delivery ID/snapshot and the exact native
unsent check ID. A Prepared check for an earlier manually released delivery is
shown with discard guidance even when a newer Start is pending. Check stays
disabled until that confirmed metadata-only discard; it never silently rotates
or offers the earlier check. A replaced check ID or changed current task view
invalidates confirmation. Offered, settling and completed checks cannot be discarded.
A lost
return from its subsequent explicit preparation reopens the same new ID; invalid
next records refuse before publication. Offered checks cannot be discarded.
If the first prepare crashed after creating its metadata lock, or retained
metadata is missing/malformed, the UI disables checking and directs manual review
of the original task history before the existing separately confirmed manual
release. It does not delete the slot, repair it or infer that no offer occurred.
Orphan publication files and retirement of older cancellation IDs remain bounded
follow-up work; neither authorizes replay or source cleanup.

Private Core controls exercise exact result/drain matching, changed sources,
late connection loss, publication validation and restart on either side of the
source CAS. Native-cache controls use actual original child evidence but synthetic
result DTOs; they cover lost replies, missing/Unknown results, refused registration,
older helpers, image replacement and retained pipe resources. These controls are
not installed frontend, macOS native or native-agent compatibility acceptance.
