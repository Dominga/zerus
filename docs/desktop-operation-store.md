# Desktop operation storage contract

G1 owns `zerus-desktop-ledger::operations` and the service's schema transaction,
separately from native notification `Kind` and attempt policy. G2 owns the pure
`operation_recovery::task_start` verifier and proposals. The storage APIs are
internal actor calls. There is no positive frontend operation IPC, worker,
scheduler or execution admission in this slice. The service stays passive.
Imported state and a Complete marker cannot grant execution authority.

The actor must first verify G2's complete original record, exact frozen input,
executor identity, current producer drain, policy and current writer/service
lease. A future executing actor must repeat these checks before reservation and
spawn. Database epoch checks are an additional fence, not a substitute for this
runtime proof. Missing old input remains review-only: this API never recreates
wire bytes from `intent.plan`.

## Frozen original and attempt identities

Schema 7 adds two private data tables and maintained totals:

| Table | Durable data |
| --- | --- |
| `desktop_operations` | Insertion sequence; unique operation and admission IDs; kind/schema/configuration/project/task/original request; closed Key and Binding; exact source bytes and retained input; distinct source, UI, input, plan and consent hashes; stable executor machine/session/run/conversation; provenance; policy state/revision and bounded outcome |
| `desktop_operation_attempts` | Unique attempt UUID; original operation/revision; complete Ticket and its fingerprint, admission/binding IDs, writer/service epochs and lease; reserved/submitted/review/unknown outcome; independent drained flag |
| `desktop_operation_totals` | Transactionally maintained unsettled count and globally undrained count |

The only kind is `task_start_reconcile`, schema 1. Key and Binding mirror G2's
closed data schema without a dependency from the ledger back into the client.
The root actor revalidates G2 policy before translating them. Native notification
kinds and retry rules remain separate.

The operation ID is SHA-256 of `desktop-operation-key-v1` followed by NUL and
these UTF-8 fields in order: `task_start_reconcile`, `1`, configuration, project,
task, original request UUID. Each field has its u32 big-endian byte length
before its exact bytes. The input/plan hash is SHA-256 of the retained bytes.
Consent conservatively binds the complete preserved plan: SHA-256 of
`task-start-reconcile-consent-v1`, NUL, then those same input bytes. Raw source
and existing UI snapshot hashes remain distinct. Shared UTF-8 vectors live in
`client/fixtures/desktop-operation-port-v1.json`.

Frozen columns and tickets cannot be updated through SQL triggers. Terminal
attempt outcomes cannot change; resource drain cannot be undone. There is no
retention/deletion API, and deletion is refused by triggers. Pressure never
removes unknown or pending work. The existing private desktop database page
limit still applies; this is not a hard WAL-size or physical-disk qualification.

## Internal calls and bounds

`admit(Admission)` freezes one original and returns its Admitted identity.
An exact duplicate returns the original metadata; changing an admission UUID's
identity, bodies, Binding or provenance refuses. It grants no execution.
`load(operation_id)` returns one bounded private Frozen original for policy
revalidation. Neither Admission nor Frozen implements Debug or Serialize.

`reserve(Reserve)` runs in an IMMEDIATE transaction and checks the exact current
nonrevoked writer/service epoch and original revision. The actor may call it
only after a new explicit lookup decision for the unchanged original. A new
reservation commits before execution. An exact duplicate or coalesced original
returns `fresh:false` with the complete existing Ticket and must never spawn.
An unknown result permits no timer retry. A later independently admitted
explicit lookup can reserve a new attempt for the same original only after the
previous resources actually drain and all current guards pass again.

`finish(Ticket, Outcome)` verifies the exact original attempt, lease, epochs and
revision. Submitted receipts use a closed allowlist and must match schema,
original request, project and task. They may settle only the original still
reserved attempt after revocation. Unknown is immutable; a late submitted reply
cannot replace it. Finish does not release execution admission.

`drained(Ticket, VerifiedDrain)` is separate. Only the actor's actual owned-child,
input writer, stdout and stderr lifetime accounting may construct that proof. No frontend,
adapter or import deserializes it. A bounded stop returning is insufficient. An inherited stdin reader can also
keep an input writer alive after the owned child exits; that lifetime stays charged.
One partial unique index holds the globally executing-or-undrained slot until
this exact terminal attempt is drained. Revoke refuses marker/checkpoint success
while any operation holds that slot. Startup recovers old reserved attempts to
Unknown in bounded batches and preserves the undrained flag.

At most 32 unsettled originals and one undrained attempt exist per configuration.
Each raw record and input is at most 1 MiB; Binding, provenance and allowlisted
outcome are each at most 16 KiB. Oversized originals remain in their existing
stores for review. Indexed status uses a fixed insertion watermark, at most
128 rows and 512 KiB per call, without loading private bodies or retaining a
per-operation cache. Service status reports admissions paused. This slice's
SQL/reopen and simulated-drain tests do not claim real process/pipe-drain proof;
that belongs to the executing actor/worker integration in G1 .66.3.22.

See [G2 policy](desktop-operations.md) and
[read-only delegation identity](desktop-delegation-hello.md).
