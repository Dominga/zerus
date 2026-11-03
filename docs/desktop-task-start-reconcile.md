# Native Task Start reconciliation boundary

`desktop::task_start_reconcile::Intent` is the closed input for one explicitly
requested `TaskStartReconcile` kind. The profile name `task-start-reconcile-v1`
is definition data. It does not extend `BACKGROUND_V1`, confer general
Operations permission or turn a notification Grant into execution consent.

This first wire slice has no positive executor or advertised capability marker.
The ordinary CLI request lane refuses `frontend_authentication_required`; an
authenticated native request still receives `task_start_reconcile_unsupported`.
No operation, attempt, custody, task call or source-store write follows either
refusal. The actor and current lease/consent integration remain separate work.

## Original identity and bytes

The request contains a service `Scope`, a fresh `reconcile_id`, the existing
Task Start policy `Binding` and the exact producer checkpoint as `original`.
The reconciliation ID differs from the original task-delivery UUID. It cannot
replace that original, an earlier admission ID or an attempt already reserved.
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

## Required actor integration

The request has its own exhaustive ordering lane and a typed native `Validated`
accessor. Generic request consumption and renderer policy dispatch refuse it.
The native responder must retain its actual registered child and cached
Service/configuration evidence. None of the wire values can construct a permit.

The later actor must acquire `BEGIN IMMEDIATE`, then recheck the original opaque
two-second proof, actual serve Owner, configuration, writer, exclusion and live
local executor/routing binding. Admission, reservation and exact prepared custody
must commit together before spawning. Existing operation bytes, admission ID and
revision remain immutable; coalescing is observation only. Every later explicit
attempt requires fresh live Consent and previous physical drain. A whole SQL
transaction rollback requires abandoning that transaction before any further
write, including refusal bookkeeping.

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
lost-reply and post-BEGIN-expiry qualification belong to that executing slice.
