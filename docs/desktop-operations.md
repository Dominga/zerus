# Desktop operation ownership and recovery evidence

G2 (`zerus-5wmh.66.4`) moves window-independent operation lifecycles into
`hgs`, preserving original identities and reviewable uncertain outcomes.
The ownership split agreed with G1, G3 and the task executor owner on
2026-10-30 is:

| Owner | Responsibility |
| --- | --- |
| G1 | One service actor and database connection; registration and locks; writer/service epochs; shared effect fences; admission, revoke and drain |
| G2 | Typed operation policy, identity and consent checks, bounded steps and reconciliation through that actor; operation-specific tables once admitted |
| G3 | Existing source decoders, versioned read-only adapters and immutable Staged evidence |
| Existing recovery and voice services | Their current authoritative work and receipts; desktop retirement creates no replacement worker |

G1's native `desktop-ledger::Kind` stays limited to native desktop effects.
There is currently no admitted positive job/effect IPC or generic scheduler.
Those requests still refuse `handoff_required`. Display coverage, a parsed
Complete marker, Staged or Drained evidence and old successful records grant
no current execution, release or takeover authority. G2 adds no second ledger,
daemon, fleet reader or source writer. Core sharing with the root executable
and any executing actor integration require their own packaging/API agreement.

## Passive recovery projection

`client/core/src/operation_recovery` is a pure consumer of one exact immutable
import. This first slice has no command, frontend, startup hook, source discovery
or executable filesystem reader. It opens no stores, makes no task/native call,
reserves no receipt and never invokes live creation state transitions. Its
reader trait is a boundary for the existing G1 secure reader, not a second file
or security implementation.

`Projection::new` accepts the selected import's trusted `ImportRecordRef`, exact
manifest bytes, configuration fingerprint and import UUID. It uses the shared
`ImportRecord` validation, then checks the exact manifest digest and identities.
The caller obtains that reference from G1's selected import, not an untrusted
snapshot. No JSON body chooses a path or another import.

`Projection::page` takes an optional opaque cursor, explicit limits, a caller
reader and cancellation check. For each selected metadata page it uses shared
`ImportRecord::snapshot_page` membership, length, digest and source binding.
Recognized operation bodies also pass `SnapshotRef::verify` before decoding.
Missing or changed bytes fail with a body-free unavailable/invalid error;
they never become an empty successful operation set.

The currently decoded families are:

| Families | Schema / format | Existing validation |
| --- | --- | --- |
| `qt-creations`, `next-creations` | 1 / `creation-json-v1` | `creation::decode_journal` for the header and `valid_operation` for each original operation |
| `qt-task-delivery/<scope hash>`, `next-task-delivery/<scope hash>` | 1 / `task-outbox-json-v1` | The pure G3 task-delivery adapter, reusing the existing outbox decoder and exact project/task/marker scope |
| `next-task-start/<scope hash>` | 1 / `task-start-outbox-json-v1` | The pure G3 Start adapter, reusing the existing outbox and original guarded plan checks |

The scope hash remains SHA-256 of project, LF, task UUID. The snapshot reference
retains the complete original private record, including unknown fields; the
projection never rewrites it. Qt and Next observations retain separate family
provenance even when their operation IDs agree. They are evidence observations,
not duplicate live operation admissions.

Summaries contain only bounded original operation/request IDs, source/page
identity, scope and recorded phase. Creation summaries omit checkout paths,
commands and diagnostics. Task summaries omit prompts, plans, attachments and
native output. A summary cannot reconstruct a live request: a future receipt
lookup must reverify the complete private original plan and current owner.

`RecordedState::Terminal` means only that the original writer recorded bound,
failed, stopped, started or released. It is not an exact live receipt or proof
that an effect is settled now. Every pending/interrupted/unknown creation and
task Start is `NeedsReview`. A task-message outbox is always `NeedsReview`;
opaque future fields claiming success cannot settle it. Nothing in this API
can retry, resume, release, clear or grant consent.

## Bounds, coverage and cancellation

- A manifest has at most 256 sources, 1024 page references and 256 KiB, using
  the shared import contract. Each metadata page and snapshot is at most 1 MiB.
  The existing publisher's total selected import budget stays at 64 MiB.
- A projection call scans at most 64 metadata pages and reads at most 8 MiB.
  Defaults are 16 pages and 4 MiB. The minimum 2 MiB allowance permits progress
  through one maximum-sized metadata page and body. An unfinished page may
  reread its metadata on the next call; each call charges those bytes.
- The caller's existing private no-follow, regular-file, single-link and
  identity checks remain mandatory. A reader must enforce the reference size
  before allocation, use only G1's fixed digest paths and perform reads off the
  GUI thread under its own deadline. The pure consumer checks returned bytes
  again. It cannot make a blocking external reader cancellable.
- Cancellation is checked before metadata/body reads and after a body read.
  An unfinished page stays at the same cursor position. There is no operation
  cache: only bounded manifest metadata, one current private body and at most
  one call's bounded summaries are retained.
- The cursor binds configuration, import UUID, exact manifest digest and page
  position. A different manifest refuses it, including a replacement with the
  same import UUID. A new import starts a new traversal.

`coverage()` distinguishes decodable source metadata, unsupported families and
evidence belonging to another responsibility (reads/approval policies). It also
lists domains without a decodable source. Decodable metadata is not a claim of
verified bodies, current discovery or completed migration. Absent and future
families remain explicitly unavailable. Unsupported pages carry their exact
manifest positions and their private bodies remain untouched.

Separate pending launch/executor maps, deferred input, draft transcription,
provider operations, cache maintenance and termination cleanup are unavailable
in this consumer until their individual schemas and policy slices are admitted.
This remains true if G3 has added an adapter that this consumer does not yet
understand. `ManifestEnd` means only the end of this immutable manifest; it
never claims complete source discovery, drain or G2 coverage.

## Executing kinds and the exit boundary

The first pure policy is `operation_recovery::task_start::Original`. G1 and G2
agreed its contract on 2026-10-30. It has no actor integration, process runner,
SQL, positive job IPC or executing permission. The dedicated operation
table/API remains G1's lane, separate from native `desktop-ledger::Kind`.

`Key` separates kind (`task_start_reconcile`), schema 1, configuration,
project/task and the original request UUID from every reconciliation-attempt
UUID. `Binding` adds the exact marker, original executor's stable machine,
session, run and conversation, and three distinct fingerprints:

| Field | Exact meaning |
| --- | --- |
| `source_record_sha256` | SHA-256 of complete original source record bytes |
| `ui_snapshot_sha256` | Existing `outbox::snapshot` fingerprint of the serialized record Value, as used by the UI |
| `task_start_reconcile_input_v1_sha256` | SHA-256 of the exact retained `TaskCall.input` bytes; this versioned field is its digest domain, without a hash prefix |

G1 supplies the immutable expected binding and retained input. Verification
checks the existing outbox/Start identity validators, all original native scope
bindings and parsed equality between the input and the complete saved
`intent.plan`. Opaque fields stay intact. Missing retained input stays reviewable;
this policy never regenerates request bytes from a plan or another build graph.
An actor must not replace bytes, consent or fingerprints under the same key.

Each explicit `step` returns NeedsReview, one private Proposed call, or
SettledSubmitted evidence. Default review, cancellation and unknown outcomes
produce no proposal. Historical started/released phases never prove current
settlement. An explicit request can propose only:

```text
hgs task start-deliver TASK --project PROJECT --json --recover-only
```

The unchanged retained input accompanies it. This existing command cannot
submit native input, but can settle original canonical task metadata from an
existing native receipt and exchange task metadata remotely. It is therefore
a fenced reconciliation effect, not a generic read-only command. It never
uses automatic `task_start::release`, a new Start/plan or a new delivery UUID.

Only a reply matching schema 1, original request/project/task, nested task
membership and `submitted` can propose settlement. The returned receipt is a
closed allowlist of schema/id/project/uuid/state, below 16 KiB; host task views
and errors are discarded. A missing, malformed, refused, uncertain, oversized
or timed-out reply remains reviewable with the original identity. Debug output
cannot expose private proposed input. This evidence still grants no authority:
G1 must match its exact reserved original operation, attempt and epochs.

The pure component accepts at most 1 MiB each for the record, retained input
and host reply, 512 bytes per executor identity and existing project bounds.
Larger legacy outboxes (up to 32 MiB) remain in their existing stores for review;
they are never truncated. There is at most one proposal per step. G1's agreed
execution admission is 32 unsettled operations, **one executing reconciliation
globally per configuration**, and one coalesced current lookup per original
scope, with a 30-second owned-child deadline, 1 MiB stdout and 16 KiB stderr.
All owned pipes/readers must actually drain before execution admission releases;
returning from a bounded stop does not establish that fact. These are actor
requirements, not guarantees implemented by this pure component.

G1 owns durable pre-execution reservation, current writer/service epoch and
lease/policy checks, fresh live writer drain, revoke and drain accounting.
Source/import/body provenance stays separate from that current ownership
proof. A remapped/missing computer identity refuses; a current display alias
never replaces the original executor. Immutable unknown attempts stay unknown.
A later separately admitted reconciliation may use only the unchanged original
call under its command's receipt semantics; notification retry rules do not
apply. No unknown result permits native input, a new delivery identity, changed
plan, automatic release or broader action. Pending/unknown work is never evicted
under pressure; admission refuses instead, and status uses bounded keyset pages.

The existing merged task Start/message outboxes remain their owners' records.
Only admitted producer schemas are recognized; a provisional new-session task
executor journal is not consumed by this slice. Creation migration preserves original durable effect
barriers. Further slices must coordinate their writer drain and exact receipts
before moving any execution into the actor, with kill/revoke/restart proof for
each kind. Pure tests cover frozen-input reconstruction, unknown/cancelled
decisions, identity and consent changes, exact receipts and historical records.
They do not claim a real process-kill, pipe-drain, late-epoch or worker restart
proof: those remain required at G1's executing reserve/finish boundary.

All slices preserve `.9.30`: Dock Quit, logout/shutdown, other Cocoa
`terminate:`, Windows `WM_ENDSESSION`, SIGKILL and Tauri restart retain the
Exit/`exit_work` fallback without requiring a page flush. The updater restart
explicitly calls `quit_with_flush`. This status slice changes neither path.

See [import evidence and handoff](desktop-migration.md),
[immutable publication](desktop-import-transport.md) and the
[G2 retirement inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md#g2-durable-operation-and-recovery-owners).
