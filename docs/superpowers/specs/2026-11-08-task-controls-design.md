# Task-scoped controls and executor holds

Design-tima2 approved the pre-code design on 2026-11-08, with adjustments
A–G. Engineering owner: `zerus-cg5h.4.7`, followed by `.4.8` (queues/controller)
and `.4.9` (typed client views). Considered-revision policy remains `.29`.

## Inactive delivery and activation

Every `.4.7` leaf lands **inactive**. There is one const gate,
`zerus_desktop_ledger::task_controls::TASK_CONTROLS = false`. There is no runtime
switch. Gate-off must preserve existing send, handoff, reopen, direct-start and
report admission and prompt bytes. In particular, sending another managed task
to an already-working executor continues to work until a queue is available.

The three leaves are:

1. Records, lazy store, bounds and status reads. No production caller.
2. CLI plan/execute/status and `begin-attempt`, plus the gated prompt pin.
3. Task Pause/Resume/yield and native Stop-hold wiring, still gated off.

The second leaf is a closed CLI wire/data surface: `task control
plan|execute|status --json` and strict `begin-attempt REF --attempt UUID`.
Until the third leaf supplies a verified runtime, even a premature gate change
must refuse with `task_control_native_path_unverified`. Parsed guards and
capability text are never a substitute for native ownership proof. The dormant
CLI emits `task_controls_inactive`/`attempted: false`; no caller can use it to
write records or task history. The local record action `release_stop` names
only explicit observed executor Stop facts and performs no continuation input.

The native-wiring check-in was approved by design-tima2 on 2026-11-08.
Slice 3 stays inactive and requires independent review followed by a
coordinator-owned Arch/Mac preview. Its record/plan boundaries are below;
activation and latest-native qualification remain coupled to `.4.8`.

The native support boundary is reviewed separately from its actor/effect
connections: v2 identity/guard records, bounded Stop-fact publication, UUID
record observation and native peer/resident proof first. These modules alone
enable no route, marker, dispatch admission or control effect. The following
inactive runtime leaf owns service actor/worker wiring, Start acknowledgement
mode, final C3 refusals and native control/import boundaries. Neither support
review nor its preview completes `.4.7` or qualifies activation.

The desktop Start plan captures a gated acknowledgement mode before hashing.
The field is omitted byte-for-byte while off and grants no floor authority.
The final C3 result must agree: managed uses one pinned begin; positively
legacy uses plain start; unknown never authorizes legacy. Either mismatch
direction returns a durable original-request-bound refusal before native
input, visible to the person. Retry creates a fresh request. The frozen payload
is never rewritten or resent, and a missing receipt is not no-delivery proof.

The owning Start-mode leaf records a mismatch in the existing native
`input_receipts/<request_id>.json` journal as schema-1
`refused_before_input`. It binds the complete frozen plan with sorted compact
UTF-8, integer-only JSON and SHA-256 under `zerus-task-start-refusal-v1\0`.
The native body digest remains the existing native-input digest. Neither digest
authorizes Person, a floor, an attempt or a retry. The original native input
exclusion protects a positively absent, bounded no-follow receipt path; file
and namespace durability must complete after atomic no-replace publication
before a definite refusal is returned. Any write/sync uncertainty is Unknown;
an existing pending, submitted, other, partial or unreadable record is never
overwritten. Matching retained refusals are lookup-only; a changed plan/body
under the same ID still refuses permanently. No receipt-age pruning is added.
Old readers reject the unsupported status as present/Unknown, never absent.
The exact-main CLI reader is qualified before adding this writer. Windows
namespace durability is not yet qualified (`zerus-cg5h.4.8.2`), so its new
writer returns Unknown; successful ordinary previews do not qualify that path.

Only a complete synced retained read-back can construct the local Start reply's
`refusal` proof: exactly `schema: 1`, `request_id`, `plan_digest`, and the closed
reason `acknowledgement_mode_changed`. Core recomputes the digest from its own
frozen outbox plan and checks the original ID; a reply cannot supply a plan or
authorize release with its digest alone. Missing/extra/malformed fields,
another plan/body, a missing outbox entry, or Unknown keep the pending Start.
A proven recovery checkpoints the original Start as terminal `released` with
the visible refusal reason; the native UUID tombstone remains permanent. Only
a new explicit Start with a fresh UUID proceeds. No proof reaches phone,
connector or relay operations, and Windows emits none while durability is
unqualified. The single const-false gate lives in the already-shared pure
read-policy crate and is re-exported by the ledger, keeping Core free of SQLite.
It gates both proof emission and recovery; there is no runtime override.

This inactive Start leaf does not connect the task-control service route,
actor/effect workers, begin binding, native Pause or Stop-fact import. Those
remain subsequent `.4.7` work before the `.4.8` activation decision.

Pause/Resume/Stop-hold activation is one owner-visible step with `.4.8`'s queue,
after coordinator-owned Arch/Mac preview and isolated contract qualification
against the latest supported native agents. No GUI scheduler is introduced.
This source work authorizes no live connector, relay, gateway or phone update.

## Owning computer and foreground slot

The executor's computer owns one foreground slot across projects. The key is
computer identity, stable run/birth identity and, only when independently
addressable, a verified native child. Conversation is an exact native-effect
guard, not a slot/hold index key. Names and folder paths are labels. An
unaddressable child never borrows its parent's interrupt authority.

The native key's birth is the precise held supervisor birth: Linux start
ticks, macOS seconds/microseconds, Windows creation ticks. Existing Unix run
records instead retain their compatible second-resolution UTC `lstart` text.
That text is an additional guard, never the precise key or a handle. Native
proof brackets its formatting lookup with the original pidfd/process pin (or
macOS unique-ID/version checks), and independently checks the precise key.
An unavailable formatting read refuses without a `ps` or unpinned fallback.

A startup reservation explicitly has no conversation. Only its original
request plus the scoped begin proof may CAS-bind the first confirmed
conversation. A later conversation change never retargets the slot or clears
holds. Resume creates a new run and therefore a new executor key: it never
inherits, satisfies or clears the old run's slot/holds. Old attempts stay held
or Unknown until an explicit person action; re-dispatch to the new run creates
a fresh attempt. The native leaf's lazy ledger version is 2. Any version-1
prototype refuses for review without migration or reset; the never-enabled
gate means no such database is expected in the field.

A slot pins a random slot UUID/revision, project/task identity, immutable
attempt UUID/generation/fingerprint, input provenance and an optional native
turn binding. Dispatched provenance pins the original native request UUID and
digest. Direct-start provenance is a distinct admitted start, not a fabricated
input receipt. A slot is initially unbound; a matching acknowledgement and the
original native request establish the current-turn proof. Text equality, idle,
the most recent attempt or the existence of a submitted receipt alone do not.

For managed dispatch, the prompt's pinned command becomes
`hgs task begin-attempt REF --attempt UUID`, **replacing** `hgs task start REF`.
There is one acknowledgement. It requires the original attempt, current
generation, exact executor and current native-turn proof. Plain `start` retains
its legacy meaning and never binds a managed native turn. A managed direct
start exposes only the UUID that exact call created; native binding requires
that original scoped acknowledgement, never a latest-attempt lookup. Unfloored starts remain
legacy; no floor is activated. Legacy/unbound/pre-upgrade work gets an honest
unsupported transition reason, never an invented binding.

After activation, slot acquisition/replacement is compare-and-set. Ready
releases the slot without waiting for Done; an unfinished paused task retains
it. Yield is separate and requires its exact settled native boundary and an
explicit disposition of retained work. Unknown live jobs do not prove yield.
Reopen queues behind current work in `.4.8`. Existing exact-origin report
authority, Ready-over-Paused precedence and `.29` semantics are unchanged.

## Pause, Resume and Stop

Pause first persists its attempt-bound Paused dispatch hold and original
control intent, then may interrupt only its proven foreground native turn.
Generic session interrupt is insufficient. Native jobs/processes and unrelated
tasks are not stopped. Logical dispatch-held and physical native outcome are
separate: confirmed, submitted, refused or unknown. A transport receipt is not
proof all work stopped. Working/Background/attention retain their native rules.

Resume is explicit. Its plan displays observed task pause and executor Stop
IDs; execute names only those the person chose. New/unseen holds survive. A
continuation has a fresh request UUID, current task context and the original
report origin. Its one-shot admission may cross only the chosen pause/Stop
facts. They are settled only after its exact successful submission receipt.
Unknown keeps them; status, restart and receipt recovery never send or replay.
Yielded work must reacquire an empty slot (later, queue) before continuation.

An explicit native Stop persists an executor-wide hold before its interrupt,
even without a task. A verified external interruption also holds dispatch.
A normal provider `Stop` hook is merely turn-end evidence. Stop does not finish
a task, release a slot or prove background jobs exited. Its hold survives
restart. Task-Pause interrupts are correlated to their original control and
are not accidentally classified as an additional person Stop.

Verified external stop facts are bounded to 64 exact IDs per executor, with
durable Unknown/held on overflow. Publication is atomic under the native state
writer; idempotent import occurs only after releasing that lock. No desktop
database lock is acquired under a native state lock. Admission checks pending
facts before a native effect, including after a publication/import crash cut.
Run-record-only storage is permitted only if every current and rollback writer
preserves the field. Otherwise the facts live in a separate durable file under
the same atomic discipline (or admission holds a detected discontinuity), never
solely in a record an older writer can rebuild. Qualification must rewrite the
run record between publication and import and still preserve the Stop hold.

Stop holds block automatic dispatch and agent-initiated handoff. A verified
person's explicit managed send/reopen/direct-start may cross **observed Stop
IDs it explicitly names**, by the same plan/execute mechanism as Resume. It
cannot cross new/unseen Stops. Permission, uncertain-input/transfer,
unknown-delivery and unresolved-custody holds remain refusing for everyone;
Resume cannot clear them. Unowned new native input invalidates task-turn proof
and holds task dispatch, never retargets Pause.

## Local records, bounds and status

Authority records live in the existing durable desktop database, not the
disposable task index. The public store entry returns inactive before any
SQLite access while the gate is false. There is no call from service startup,
no desktop Store schema bump and no migration in the dormant leaf. Behind the
gate, tables are created lazily in one transaction and carry their own version;
partial/future state refuses without repair or reset. Activation must review
the desktop schema/rollback boundary before enabling this lazy extension.

The ledger stores slots/tombstones, immutable request bodies with exact
settlement, and identified holds with explicit settlement references. New
reservation versus an existing request is a distinct result: existing always
means status-only. Changed payload under the same UUID refuses. No age-based
expiry frees a live slot, unknown outcome, hold or request identity.

The local v2 Request requires `admission`: SHA-256 over
`zerus-task-control-admission-v1` followed by NUL, then the complete validated
schema-1 Plan except `request.admission`. Encoding is compact JSON with sorted
object keys, integer numbers only (no float coercion), and fixed UTF-8 strings
without Unicode normalization. Caller, configuration, store identity, selected
CLI image and actually loaded adapter guards are all included. The service
computes the binding when planning; execute recomputes it from the received
Plan. A changed outer guard under the same request ID changes the immutable
Request and refuses without overwrite or replay; the identical Plan is lookup
only. A matching digest never supplies caller, adapter or native authority;
those proofs are independently revalidated at execute. A future canonical
encoding needs a new schema/domain version.

Server-only native Stop import uses the existing `Action::Stop` records, with
no public execute operation or invented caller. Its admission digest instead
covers the closed `{schema:1, fact, configuration, store_identity}` guard under
`zerus-task-control-stop-import-v1` followed by NUL, using the same canonical
encoding. Service image and epoch are excluded: an unchanged retained fact
survives service replacement. The exact fact ID and guard are immutable;
changed fact/configuration/store bindings refuse. Import only adds or retains
holds, and never reactivates a settled identity or grants input authority.

The native writer captures a sealed bounded file identity, size and SHA-256
digest, retaining unlocked original fact and independently opened writer-file
handles so their numeric identities cannot recycle through replacement. The
original native exclusion is not retained. After that writer drops, the actor
imports the captured page in one immediate transaction and rechecks the
retained no-follow file inside that transaction before commit. It takes no
native lock there. Missing, changed,
oversized or overflowed evidence and any failed import refuse admission; a
caller cannot skip reconciliation and treat the absence of imported holds as
permission. A failure rolls back the page, including request reservations and
holds. Already-retained reservations can be completed by exact import after
restart; this does not replay native input. Native fact files remain retained.

Scoped begin resolves its exact original attempt with a partial active-attempt
index and at most two bounded rows; ambiguity refuses. The index is part of
canonical v2 creation only. **No DDL runs against an existing task-control
store in this leaf:** an earlier prototype missing it refuses without repair,
migration or a full-scan fallback. A future existing-store migration requires
its own reviewed backup and one-way handling. These helpers remain inactive;
the service route, actor/effect wiring and activation are subsequent work.

Requests are at most 16 KiB; records reserve 32 KiB to retain outcome details.
Status pages contain at most 128 rows and 64 KiB of stored payload. Sequence
cursors use indexes; no complete journal is loaded. There are at most 64 active
holds per executor, and 65,536 retained records/64 MiB of payload globally.
Capacity refuses atomically without pruning unresolved evidence. Future bounded
maintenance needs its own reviewed tombstone/retention policy. Admission must
examine every active hold, even when status requires several byte-bound pages.

These records grant no actor, task-write, native interrupt or cleanup authority.
Future callers must keep their original bounded admission/custody alive and
recheck task version, attempt, slot revision, exact executor/turn, loaded
adapter, observed facts and existing conflict/permission/delivery guards at the
effect boundary. Cross-store partial commits remain held/unknown. No reverse
lock acquisition is introduced from native callbacks.

## Writer boundary and known gaps

Enforcement is the updated **executing CLI/service on the owning computer**,
not schema 7 alone. Active task-control will require a `task-control-v1`
help/capability marker and refuse older selected helpers. A resident
input-producing adapter must demonstrate a compatible actually-loaded path;
a newer executable on disk is insufficient. Otherwise Task Control refuses
with a reason. Never hot-reload an adapter or restart its agents to qualify it.

**An older CLI on the owning computer can still dispatch.** No new C3,
exchange schema or minimum-writer floor is introduced here. Schema 7 already
understands pause event shapes but does not force an older writer to enforce
local slots/holds. A minimum-writer boundary, if required, is a separate
reviewed change before or with activation. Generic native messages, direct
provider input and old resident adapters are not claimed as task admission.

Source inventory (main `988a30e18`, dormant leaf):

| Path | Executing path and activation work |
| --- | --- |
| CLI send/handoff | `src/tasks.rs::change_within` selects `attempts::send`; `src/tasks/attempts.rs` reserves under the final fence, then delivers on the owning computer. Add slot/hold checks there when activated. |
| Desktop Start, including a newly created executor | `src/tasks/dispatch.rs` start-plan/start-deliver uses its immutable plan and final `attempts::admit`. Recovery is lookup-only. Both ordinary and fresh-executor paths need admission. |
| Continue/reopen and feedback | `src/tasks/messages.rs` selects `attempts::reopen_request/reopen/reopen_again`; pin context before hashing and check original execution. Ordinary task clarification is not unrelated queue work. |
| Direct start/add --start/worker acknowledgement | `src/tasks.rs` records start; no native dispatch receipt exists. Preserve legacy paths; bind only an exact newly admitted managed start, and audit the worker/child registration path. |
| Phone Send | Android/iOS TaskSync sends `task_send`; Rust connector `execute.rs::task_action` calls its **configured** CLI's `task mobile-send --json`; `src/tasks/mobile.rs::send` calls the same `change_within(...,"send",...)`. A maintained selected CLI reaches the owning admission, but this CLI bridge cannot prove phone Person authority. Before activation, `.4.8` must resolve or explicitly classify that authority route; no silent bypass or fabricated Person proof is permitted. A stale configured CLI remains a known gap; no live config is changed. |
| Phone reopen | There is no dedicated `task_reopen` connector action today. Native mobile local-stage edits arrive through task exchange; any subsequent `task_send` follows the preceding row. Local exchange/import itself grants no native dispatch. This is not claimed as a new phone control writer. |
| Task exchange and legacy Python connector | Exchange imports replica bytes and answers fetches; it is not native delivery. The retained Python connector's selected-CLI route must be re-inventoried before activation; this leaf claims no live gateway enforcement. |
| Remote owning CLI / selected helper / resident adapter | Every executing helper needs the marker and final admission; a caller-side check is insufficient. Older or unverified loaded paths remain unsupported for task controls. |

This inventory describes source routes, not an inspection of deployed gateway
configuration. Before activation, re-inventory every dispatch writer, prove the
executing path and explicitly list any remaining bypass in the PR. No phone
writer, relay or connector source/wire change is authorized by this leaf.

## Local service ownership and CLI peer

Design-tima2 approved the local bridge boundary on 2026-11-08. The service
actor alone owns the connection and schema; CLI code never opens another
desktop database writer. Reuse the existing private same-UID Unix socket,
authenticated Windows pipe and Hello ABI for a gated `task_control` operation:
schema 1, at most 16 KiB, closed plan/execute/status/begin envelope. With the
gate off the operation receives today's exact `unsupported_operation` error
frame, with no marker, new route, record access or schema work.

Same UID is the transport trust boundary, not actor proof. The bridge proves
only Executor by the actual peer's birth-pinned ancestry to the native agent
recorded for that executor, plus the loaded trusted hgs image and exact command
path. `HGS_SESSION` and other environment variables never authenticate it.
**Person callers refuse on this CLI bridge.** The GUI's Person path is a later
separately reviewed surface; JSON cannot substitute for it.

The v2 local Executor and CLI envelope require the canonical native
`session_id` as a non-key lookup selector and exact-match guard. The key stays
exactly computer/run/birth/addressable-child. Resolve one bounded hit in the
existing UUID index; never scan, select by a name or substitute a latest run.
Independently verify the resulting record's run, birth and conversation,
actually loaded adapter and held-peer ancestry to the native agent PID/birth.
Archived, saved, mismatched and not-live records refuse or stay held.
The startup CAS fixes the session UUID with its first confirmed conversation.
A later change never retargets the slot. Rename preserves that UUID; resume
also preserves it but creates a new run/key, which cannot satisfy, clear or
inherit the original run's slots or holds.

The bounded native-record observer can read ordinary supervised session
records in both existing layouts, including the existing routed label-holder
path. Shared DeepSeek/OpenCode host entries and independently addressable
children do not yet have a qualified scoped adapter on this bridge; they
refuse explicitly. This is a known activation prerequisite, not permission to
borrow a shared host or parent process identity.

Native peer evidence stays held: Linux socket credentials plus a pidfd bound
to that peer before birth/image reads; macOS peer audit token; Windows named
pipe client PID plus a retained process handle, creation time and image path.
Bare PID-then-image lookup is unknown and refuses. Native workers run outside
the actor and return bounded original-request results. There is one in-flight
operation per request UUID; repeat execute/status is lookup-only, never replay.
No database transaction is held while acquiring the native state writer.

An old or missing selected service refuses explicitly and names the owning
computer to update. Neither version direction falls back to a direct database
write, another writer or native input. This bridge is not the queue/controller
and introduces no automatic dispatch, Resume or scheduler.

## Qualification

Record controls cover lazy/no-op storage, exact identities and slot CAS,
cross-project/child ownership, concurrent claims, immutable request no-replay,
observed versus unseen Pause/Stop facts, person Stop crossing versus agent or
automatic dispatch, non-crossable holds, restart, strict bounds, stable pages
and atomic capacity refusal. Gate-off native-fixture admission must retain its
existing bytes and allow another managed task for a working executor.

Gate-enabled private fixture checkouts use a separate Cargo target directory.
They never share artifacts with gate-off builds. Before any debug CLI is used
as evidence or installed, rebuild it, freeze the candidate, and check that its
capability marker and actual inactive response agree with the intended gate.
Source text alone does not qualify a previously built shared-cache artifact.

Later leaves add actor/attempt/generation/native-turn substitution, real CLI
legacy/managed acknowledgement output, prompt hashing and the 2400-byte guide
bound, every durable/native crash boundary, Stop versus normal turn-end,
confirmed versus submitted/unknown interruption, original receipt recovery,
loaded adapter capability and no replay. Activation adds the isolated latest
native contract campaign and coordinator-owned Arch/Mac preview.

The macOS audit-token/csops/argv path has only been cross-typechecked so far.
Before `.4.8` activation it requires actual non-display private tests on a Mac
(`zerus-cg5h.4.8.1`), including wrong/reused identities, loaded-image changes,
ancestry and both compatibility directions. Neither a cross-check nor the
ordinary GUI preview qualifies that native path. No live agent or owner
display is needed for that prerequisite.
