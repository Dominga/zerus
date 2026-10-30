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

The first proposed executing policy is existing task Start's original
`--recover-only` reconciliation. Before integration, G2 and G1 must agree exact
types, durable fields, limits and the bounded step interface. A step may return
Settled, NeedsReview or one proposed original receipt lookup/existing request.
G1 owns durable pre-execution reservation, current writer/service epoch and
lease checks, revoke and drain accounting. Unknown execution only reconciles
by its original identity. No new request may be manufactured to repeat it;
each command's existing receipt semantics decide what can be checked.

The existing merged task Start/message outboxes remain their owners' records.
Only admitted producer schemas are recognized; a provisional new-session task
executor journal is not consumed by this slice. Creation migration preserves original durable effect
barriers. Further slices must coordinate their writer drain and exact receipts
before moving any execution into the actor, with kill/revoke/restart proof for
each kind.

All slices preserve `.9.30`: Dock Quit, logout/shutdown, other Cocoa
`terminate:`, Windows `WM_ENDSESSION`, SIGKILL and Tauri restart retain the
Exit/`exit_work` fallback without requiring a page flush. The updater restart
explicitly calls `quit_with_flush`. This status slice changes neither path.

See [import evidence and handoff](desktop-migration.md),
[immutable publication](desktop-import-transport.md) and the
[G2 retirement inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md#g2-durable-operation-and-recovery-owners).
