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

Pause/Resume/Stop-hold activation is one owner-visible step with `.4.8`'s queue,
after coordinator-owned Arch/Mac preview and isolated contract qualification
against the latest supported native agents. No GUI scheduler is introduced.
This source work authorizes no live connector, relay, gateway or phone update.

## Owning computer and foreground slot

The executor's computer owns one foreground slot across projects. The key is
computer identity, stable run identity, native conversation and, only when
independently addressable, a verified native child. Names and folder paths are
labels. An unaddressable child never borrows its parent's interrupt authority.

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
its legacy meaning and never binds a managed attempt. A managed direct start
may bind only the attempt that exact call created. Unfloored starts remain
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
| Phone Send | Android/iOS TaskSync sends `task_send`; Rust connector `execute.rs::task_action` calls its **configured** CLI's `task mobile-send --json`; `src/tasks/mobile.rs::send` calls the same `change_within(...,"send",...)`. A maintained selected CLI can enforce here after wiring. A stale configured CLI remains a known gap; no live config is changed. |
| Phone reopen | There is no dedicated `task_reopen` connector action today. Native mobile local-stage edits arrive through task exchange; any subsequent `task_send` follows the preceding row. Local exchange/import itself grants no native dispatch. This is not claimed as a new phone control writer. |
| Task exchange and legacy Python connector | Exchange imports replica bytes and answers fetches; it is not native delivery. The retained Python connector's selected-CLI route must be re-inventoried before activation; this leaf claims no live gateway enforcement. |
| Remote owning CLI / selected helper / resident adapter | Every executing helper needs the marker and final admission; a caller-side check is insufficient. Older or unverified loaded paths remain unsupported for task controls. |

This inventory describes source routes, not an inspection of deployed gateway
configuration. Before activation, re-inventory every dispatch writer, prove the
executing path and explicitly list any remaining bypass in the PR. No phone
writer, relay or connector source/wire change is authorized by this leaf.

## Qualification

Record controls cover lazy/no-op storage, exact identities and slot CAS,
cross-project/child ownership, concurrent claims, immutable request no-replay,
observed versus unseen Pause/Stop facts, person Stop crossing versus agent or
automatic dispatch, non-crossable holds, restart, strict bounds, stable pages
and atomic capacity refusal. Gate-off native-fixture admission must retain its
existing bytes and allow another managed task for a working executor.

Later leaves add actor/attempt/generation/native-turn substitution, real CLI
legacy/managed acknowledgement output, prompt hashing and the 2400-byte guide
bound, every durable/native crash boundary, Stop versus normal turn-end,
confirmed versus submitted/unknown interruption, original receipt recovery,
loaded adapter capability and no replay. Activation adds the isolated latest
native contract campaign and coordinator-owned Arch/Mac preview.
