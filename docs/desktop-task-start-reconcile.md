# Native Task Start reconciliation boundary

`desktop::task_start_reconcile::Intent` is the closed input for one explicitly
requested `TaskStartReconcile` kind. The profile name `task-start-reconcile-v1`
is definition data. It does not extend `BACKGROUND_V1`, confer general
Operations permission or turn a notification Grant into execution consent.

Linux and macOS have a guarded, fixed recover-only actor. The ordinary CLI
request lane still refuses `frontend_authentication_required`, and a passive
service retains `desktop_background_unavailable`. Windows remains
`task_start_reconcile_unsupported` until it has equivalent physical custody.
The native renderer policy route cannot submit this kind. This actor slice
adds no general Operations permission or advertised helper capability marker;
the native Task Start caller is a separate integration.

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
for the future native caller's CAS of its still-unchanged outbox. The redacted
five fields are never expanded into a canonical CLI reply.

Missing is only an observation. It proves no absence of an earlier effect,
current executor, physical drain or permission to offer again. This path cannot
admit/reserve, start a worker, release a record or mint a reconciliation ID. A
changed/malformed query or stored binding returns
`task_start_result_unavailable`. The native caller still needs its offered-unknown
journal and retained selected-service resources. No per-kind capability marker
is advertised until both that caller and this query are callable.
