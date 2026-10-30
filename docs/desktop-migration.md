# Desktop import evidence and writer handoff

G1 (`hgs desktop`) and G3 (read-only migration) agreed this v1 contract on
2026-10-30. The shared Rust crate is `crates/desktop-migration`, package
`zerus-desktop-migration`. It contains structural types and pure validation,
not an importer, effect worker or ownership proof. Existing Qt/interim readers
remain the import stack. See the [migration inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md#migration-contract)
and [service design](superpowers/specs/2026-10-29-hgs-desktop-service-design.md#handoff-coexistence-and-upgrade).

## Marker

`desktop/handoff-v1.json` below the existing `HGS_STATE_DIR` is at most 64 KiB:

```json
{
  "version": 1,
  "configuration": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "source": "com.hgdev.hgs-tray",
  "import_id": "11111111-1111-4111-8111-111111111111",
  "writer_epoch": "22222222-2222-4222-8222-222222222222",
  "phase": "imported",
  "import_record": {"version": 1, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
  "automatic_approvals": false
}
```

`Handoff::parse` rejects missing, unknown and malformed fields, unsupported
versions, wrong configuration/source, noncanonical UUIDs, non-lowercase hashes,
and migrated approval consent. Development/local identities and replay never
participate. `writer_epoch` is the durable cutover generation, distinct from a
service process/watch epoch, and authorizes no delivery replay.

The closed v1 phase enum is `imported`, `quiesced`, `complete`, `revoked`.
`revoked` durably withdraws the service's migrated writer authority before a
rollback or Qt start. G1 alone commits it. Reason/history stay in G1's private
journal, without extending the marker payload. Older consumers fail closed on
the unknown phase; none may interpret it as a new import or reseed.

The import record always resolves to
`desktop/imports/<import_id>/record-v1.json`; JSON cannot supply another path.
`Handoff::verify_import_record` checks the exact byte digest, version and matching
configuration/import identity. Missing or mismatched evidence leaves G1 passive
and is never repaired by rewriting or replacing it with defaults.

## Immutable import evidence

`ImportRecord` is a metadata-only manifest, at most 256 KiB. Its exact fields are
`version`, `configuration`, `source`, `import_id`, `evidence` (`staged` or
`drained`), `created_at` (Unix seconds), `automatic_approvals` (false), `sources`
and `pages`. Each source records a family, reader schema version, format, exact
source-byte SHA-256 and optional verified build identity. There are at most 256
source records with distinct family names and 1024 distinct page references.

Each page reference has only `sha256` and `bytes` (1 to 1 MiB) and resolves to
`desktop/imports/<import_id>/pages/<sha256>.json`. `verify_page` checks exact
length/digest. Private pages carry bounded family metadata, schema
and snapshot/quarantine references. The manifest and handoff marker contain no
message, question, token or session arrays. No credentials are copied from their
privileged stores. Snapshots and quarantine details are never repository data.

G3 assembles read-only snapshots through the existing importers. Its evidence is
always explicitly `staged` and ineligible for takeover. G1 owns the executable
publication boundary and immutable `drained` publication after a fresh drain.
The producer must run after that drain and recheck source identity/hash; a stale
pre-drain bundle cannot simply be relabelled. Neither owner changes a committed
import. These fields describe a past import, not current process ownership.
Automatic-approval reset evidence must inspect both Qt's former choice and
Next's interim policy. Neither carries consent to the service.

Previously unhandled families may import once under the versioned first-G3 rule
in the inventory. Next's existing values win. A family record includes schema,
source path, time, key names and the exact source-byte hash; a changed Qt source
since the original switch is recorded and remains eligible for a new family.
Failed/partial families remain unhandled. Already handled settings/layout/drafts
never re-seed after rollback.

## Locks and live proof

G1 retains `desktop/owner.lock` for its process lifetime. G3 never takes it.
`desktop/handoff.lock` is a separate OS-backed exclusive lock. G1's publication
helper holds it briefly to commit immutable evidence and refuses/retries while
an active writer holds it. G3 has no filesystem committer. G1 rereads
the exact import UUID and digest under that lock, owns all marker transitions
(`imported`, `quiesced`, `complete`, `revoked`), and retains the lock while any migrated
writer runs. Commits use atomic replacement with durable file/directory flushes.

Readers/committers require private directories and private regular single-link
files, no-follow opens and byte limits. Pure validation does not perform these
filesystem checks. A complete marker never replaces fresh runtime proof:
versioned build/configuration/writer-epoch hello, stopped old admissions, durable
drain, exact shared effect identities and both service locks. The revised
coordinator ruling of 2026-10-30 uses explicit durable revocation before rollback
or Qt start on the normal path, with live legacy-GUI detection revoking the
abnormal path. It does not require the service to own Qt's Unix socket; the
temporary instance start lock alone cannot prove long-lived exclusion. Takeover
remains fenced until revocation and exact effect guards exist. Windows permanent
lock exclusion is separately unqualified. Arch remains passive until the explicit
Linux handoff even if a marker is copied there.

`configuration(state, config)` extracts the service's existing algorithm:
canonicalize existing path ancestors, then SHA-256 the state and config paths in
that order, each native encoded byte string prefixed by its little-endian u64
length. Missing descendants are resolved without creating directories. G1 and
G3 must call this same helper; lexical paths or independently approximated hashes
cannot establish configuration identity.

## Producer API

`zerus_client_core::migration::Bundle` is a Rust-side bounded iterator over
existing source decoders. It accepts validated `SourceStream` metadata and lazy
chunks, the production identity, configuration fingerprint, canonical import
UUID, time and an explicit total byte budget. Development identities, replay and
duplicate source families are refused before a decoder is read.

Each decoder checks its supported source schema and exact identity/hash and
yields bounded JSON values. It must refuse an oversized individual source item
before allocating it; it must not load a complete unbounded journal. Unknown,
partial, changed or unreadable data ends the bundle without a final record.
The source remains unchanged and its family stays unhandled for retry.

`BundleItem` yields a private `Snapshot { reference, reader }`, then a body-free
`Page { reference, reader }`, followed by exactly one final `Record` after all
sources succeed. Consume each `Read` before requesting another item. At most
one current body and one small pending metadata page are retained by assembly;
each serialized body is at most 1 MiB. The total budget includes unique bodies,
pages and the final manifest. The manifest retains at most 1024 page references,
not source values. Repeated snapshot bodies are emitted once by digest.

The shared `SnapshotRef` contains only `sha256` and `bytes` (1 to 1 MiB) and
resolves only to `desktop/imports/<import_id>/snapshots/<sha256>.json`.
`SnapshotPage` v1 contains `family`, `schema`, `source_sha256`, `index` and that
reference. `ImportRecord::snapshot_page` verifies the page's membership, exact
bytes and matching source family/schema/hash. `SnapshotRef::verify` checks the
private body's exact bytes. No caller supplies a final filesystem path.

This iterator cannot itself cross the core-to-hgs process boundary. G1 owns the
bounded authenticated transport and secure publisher, reusing its existing
platform file/Location/lock code: no-follow private single-link regular files,
create-new UUID directory, fixed digest names, hash/length rechecks, durable
file/directory flushes and atomic final record publication. Existing UUIDs,
held locks and partial/newer/mismatched bundles refuse publication. G3 writes no
service files and has no marker, drain or effect authority.

`migration::sources` supplies the first existing-decoder adapters:

- `creations(Origin::Qt | Origin::Next, bytes)` reuses
  `creation::decode_journal` (128 operations, 8 MiB). It preserves original
  dispatched/unknown states, request IDs and unknown header/operation fields;
  it does not call the live creation state machine. The same decoder now
  validates the entire journal before the client loads any operation.
- `qt_pending_reads`, `qt_shared_read_migrated` and `next_pending_reads` retain
  exact host/run/conversation/reply/attention identities. Qt's compound key must
  match its value. Unknown private fields are retained alongside the existing
  `ReadMark` decoder's normalized shape. Queues are bounded at 16 MiB and 50,000
  entries, in private pages of at most 64 entries/1 MiB. Nothing queues a read,
  retries delivery or changes a host's read state.

Attach `SourceStream::recheck` using the same existing source reader after
decoding. It runs once on exhaustion and refuses changed hashes or read errors
before a final manifest. G1 still checks source identity and its current drain;
this callback creates no runtime authority. Unsupported/oversized sources stay
unchanged and unhandled. These APIs do not imply a complete production import.
Source metadata is available through `SourceStream::identity()`; chunks and
recheck state cannot be replaced after construction.

`migration::transport::Publisher::probe(selected_hgs, environment)` connects
this producer to G1's [desktop-import-v1 transport](desktop-import-transport.md)
using the existing core child-process runner. It bounds the exact help-marker
probe to five seconds/256 KiB, then obtains the configuration from supported
read-only desktop status. It keeps the selected executable/environment and
rechecks capability/configuration before publication or fixed-ID inspection.
Call every probe, source decoder, stream and wait off the GUI thread.

Build the bundle with `publisher.configuration()` and a durably retained import
UUID. `publisher.publish(bundle)` accepts only an untouched bundle with a fresh
recheck attached to every source, within the 64 MiB wire budget. It writes one
bounded body at a time and closes stdin after the final Staged record. Both
publisher output pipes have a hard 16 KiB capture limit; overflow or the
130-second child deadline stops only that owned child. A `StagedReceipt` is
accepted only when its configuration, UUID, manifest digest, evidence, byte and
snapshot/page counts match exactly what was sent. No report contains bodies or
source paths. This API has no startup hook and does not select source families.

Failures retain `PublishFailure.import_id`. They never generate another UUID,
retry publication or overwrite evidence. Explicit `publisher.status(id)` checks
only that retained manifest and reports absent or its body-free digest/evidence.
Absent also covers incomplete publication and never authorizes reuse. Present
does not establish current drain, sync durability, delegation or effect authority.
The opt-in `migration_publisher_native` integration test exercises the real
selected CLI with synthetic sources and isolated HGS directories; run it with
`HGS_IMPORT_TEST_BIN` and `--ignored`. It starts no service or native agent.

`qt_task_forms(&QtSnapshot)` preserves `tasks/newCard/*/title|request`,
`titleDraft`, `noteDraft` and `messageRequest` in private entries
`{key, kind, project, task?, value}`. Reserved-card request scope/project,
request UUID, target and stage must match the existing Qt schema. Incomplete
text drafts and unknown request fields are preserved; neither request nor task
IDs are regenerated. Existing DraftStore import still owns task description,
comment and attachment drafts. Form input is bounded at 16 MiB/50,000 entries,
individual items at 1 MiB, and pages at 64 entries/1 MiB.

`task_delivery(Origin, project, task, marker_id, bytes)` reuses the existing
`tasks_write::outbox::decode`. Its source must be the existing
`outbox::path(root, project, task)`; the caller verifies/rechecks that exact file
and marker under the current drain. The original marker must equal the record's
request UUID. Private evidence preserves the exact intent and unknown fields,
with distinct project/task/request identities. A delivery is one indivisible
item: over 1 MiB is refused before JSON allocation, never truncated. Missing or
mismatched markers, unknown versions and orphan/unidentified files stay for
review; this adapter does not discover or authorize them. It never reserves,
releases, submits or reconciles a message. Source discovery and remaining
families are separate slices.

`task_start_delivery(project, task, marker_id, bytes)` covers Next's separate
existing-executor Start checkpoint at
`outbox::path(draft_root.join("task-start"), project, task)`. It reuses the
bounded outbox decoder and the live task-start writer's pure structural check.
The exact canonical reservation UUID, plan UUID, original request UUID and
project/task scope must agree. All four saved phases (`delivering`, `review`,
`started`, `released`), the complete original guarded CLI plan and opaque
unknown fields remain private evidence without mapping or settlement. The
distinct `next-task-start/<scope hash>` family cannot replace discussion
delivery evidence. The indivisible source and evidence envelope must each fit
1 MiB; future, partial or mismatched records refuse. The caller verifies and
freshly rechecks both the source file and its exact marker under G1's drain.
This adapter never calls task open/start/recover/release, reserves an identity
or writes a source. Startup publication and G1 consumption remain separate;
neither an imported phase nor an original plan authorizes execution.

`qt_pending_launches(&QtSnapshot)` and `qt_executor_launches(&QtSnapshot)`
preserve the separate legacy Qt maps `workspace/pendingLaunches` and
`tasks/pendingExecutorLaunches`. They use the existing QtStore/Qt JSON reader,
not the live creation or task state machines. Their distinct families are
`qt-pending-launches` and `qt-executor-launches`, schema 1, with format
`qt-ini`, `qt-plist` or `qt-defaults` and the raw Qt snapshot hash.
Private chunks start with `{kind:"pending-header",key,present}` and continue
with `{entries:[{launch_id,record}]}` pages. An absent key is distinct from an
explicitly empty map. Original keys, local empty host, task/project/parent,
saved `waiting`/`reserved`/`review` phase, session, timestamp, hidden status and
unknown fields remain unchanged. No age-based expiry, state conversion,
name-based matching or ID generation occurs. Creation-schema records in the
legacy map refuse; Qt already saves those only in its separate creation journal.

Executor input keeps Qt's 64 KiB/16-record caps, checked before JSON allocation;
legacy placeholders use the bounded 16 MiB/50,000-entry reader. Private pages
keep the existing 64-entry/1 MiB limit; a single oversized record refuses, never
truncates. Malformed identities, unsupported phases/shapes and partial sources
cannot complete a family manifest. The caller still discovers only the existing
source and freshly rechecks it under G1's current drain. These are Staged
evidence adapters with no startup publication, status service, placement,
launch, task delivery, release, settlement or replay authority. Deferred,
draft operation, download and cleanup sources remain uncovered.

`qt_repository_clones(&QtSnapshot)` preserves the separate Qt
`workspace/repositoryClone/<scope>/pending|previous|parent` namespace as
`qt-repository-clones`, schema 1, using the existing Qt source format/hash.
Qt's scope is lower-case hex of UTF-8 compact JSON `[hgs executable,host]`;
the adapter decodes that original identity without choosing an executable or
host, or claiming it belongs to the current service configuration. Private
chunks have `{kind:"clone-header",present}` followed by
`{entries:[{key,executable,host,kind,record}]}`. Pending/previous records reuse
`repository::Pending` serde and the existing pure UUID, name and path checks;
the original object and unknown fields survive. The remembered parent remains
text. A previous record proves neither settlement nor that a clone stopped.

The existing 16 MiB/50,000-entry source budget and 64-entry/1 MiB pages apply;
each source item is bounded at 1 MiB before JSON allocation, and scope JSON at
16 KiB. Unsupported scopes/suffixes, invalid/mismatched original identities or
destinations, unsafe credential-bearing repository URLs and oversized records
remain in the unchanged source without a final family record. Attach the fresh
source recheck before publication. No live Clone, status, retirement, clone,
checkout or launch call runs. Next keeps no separate pending-clone store: its
actual clone operations are already in the covered creation journal. These
adapters add no discovery, startup publication, consumer or execution authority.

`approval_policies(&QtSnapshot, next_settings_bytes)` inspects both policy
stores before returning either `qt-approval-policy` or `next-approval-policy`
stream. It reuses the existing Qt flag conversion and bounded Next settings
decoder without starting either settings writer. Private snapshots distinguish
an absent key, the former enabled choice and Next's old Qt-at-switch notice;
unknown Next policy fields survive privately. The raw source hashes remain in
the body-free manifest. Malformed flags, unsupported schemas or oversized
policy bodies refuse the pair and retain both originals. Attach each source's
fresh identity/hash recheck before publication. Every snapshot/manifest keeps
`automatic_approvals:false`; evidence grants no opt-in, writes no setting or
approval reservation and never delivers a request. Source discovery and G1's
policy/UI integration remain separate.

## Retained Qt shared organization

On the production switch only, the existing local organization topic queues a
one-time recovery on its organization worker. No GUI/pump/writer lock is held
across Qt, settings or CLI I/O. `swarm::qt_import` uses `QtStore::snapshot` to
read the current `workspace/organization` only. Backups, `swarm/observed*`,
pending edits, local layout/default/collapse/order and run memory are not
initialization sources. Supported current project fields are validated within
the existing project/folder/member limits, then the existing shared projection
filters implicit membership and this computer's Windows folders. Source
extensions and local fields remain untouched in Qt for their separate families.
Malformed, ambiguous, legacy pinned/groups, future and over-limit input refuses
without marking the family handled. An existing Qt store missing the current
organization is not treated as empty.

Recovery uses the existing `hgs swarm initialize` / `import_missing` writer:

- `initialize --g3-status --json` takes `{version:1,expected_node_id}` and
  returns `{version:1,node_id,initialized,gui_import_pending}` without saving
  or supervising a service. `initialized:true` alone is insufficient: the
  earlier CLI bootstrap may still await its GUI import. A missing catalog
  establishes its local node through the existing metadata-only `swarm
  inventory`, which does not initialize projects or read Tasks.
- `initialize --g3 --json` takes
  `{version:1,expected_node_id,source_sha256,organization}`. It verifies the
  exact node under the existing catalog lock and calls the original
  missing-field initializer. Existing values, concurrent heads and tombstones
  win. The normal snapshot carries `g3_import:{version:1,source_sha256}`.
  Legacy `initialize` input/output and the closed catalog file schema remain
  unchanged. Old CLIs cannot mistake the wrapper for a project organization;
  a missing protocol response never completes a G3 record.

Before initialization, the existing Settings writer durably saves metadata at
`g3_shared_organization_pending`: version, exact node and the normal G3 family
record (family/schema/source path/time/key names/raw source-byte hash and the
post-switch mtime observation). It contains no operation or source body. Qt's
path/source/hash is freshly rechecked before the command. An exact successful
receipt completes `g3.families.shared-organization` in that same writer. If a
reply is lost, a subsequent same-node status with no pending GUI import can
complete the original metadata without reading Qt again. Unknown/foreign
metadata and failed barriers remain pending; no completed family re-seeds after
rollback. A catalog already initialized by an earlier Qt import is kept as-is.
This is shared project metadata recovery, not G1 evidence publication, a drain
claim, a marker phase or effect authority. Local presentation families remain
separate.

## Retained Qt local project presentation

The first-G3 `organization-presentation` family imports only local `collapsed`,
`order` and `default_project` from the current Qt `workspace/organization`.
The coordinator's explicit 2026-10-30 exception permits these three fields to
fill absent or empty/null Next defaults (`[]`, `{}`, `null`): old Next writers
always serialized those defaults. A nonempty current Next field wins,
independently of the other two, including changes made during the Qt read.
Every other family retains strict key-presence precedence. This exception may
cause one cosmetic change; it never repeats after the family is handled.

The existing production-switch recovery hook queues the read on the existing
organization worker, even without an organization-topic update. No GUI, hub,
pump or writer lock spans Qt I/O. The existing QtStore and G3 family metadata
builder validate the bounded current source and freshly recheck its canonical
path/raw hash; overlapping source/target, malformed/future/legacy groups or
pinned shapes and duplicate/oversized identities refuse. Unsupported local
layout files still pause all layout seeds and writes for that startup.

Values and `g3_organization_presentation` commit together in the existing local
`organization-layout.json` writer. The record uses the same family/schema/
source path/time/key names/raw hash/mtime-after-switch shape as Settings G3
records. Unknown layout extensions survive. A failed read, oversized target or
failed file commit cannot leave a completed family on disk; the existing writer
retries pending bytes, and an unhandled family retries at the next startup.
A handled family skips Qt before reading it, across rollback and a second
switch. This never seeds `run_groups`, run tokens, shared membership, project
vivid state or local Windows folders; those retain their existing owners or
separate unhandled families. No G1 evidence/marker or effect authority is added.

## Delivery boundary

The shared crate validates no running process and offers no `ReadyForWrites`
boolean. It writes no marker, lock, source store or registration. G1 composes
structural results with fresh runtime evidence before taking writer authority;
the G3 importer and G1 integration are separate implementation slices. Client
settings, local layout and drafts retain their existing owners. G1 takes no
connector, draft or run-layout writer and never restarts agents or other services.

The existing organization-layout owner uses `Layout::try_from_json` before
enabling saves. It accepts the legacy unversioned/v1 shape within its existing
byte/member/run limits and preserves unknown extension fields on later saves.
Malformed/newer/over-limit layouts and repeated member/run identities refuse;
an unreadable original also pauses layout saves and the Qt run seed for that
startup. In-memory presentation changes never authorize replacing the retained
file with defaults. A fresh start must validate it again before writes resume.
`Layout::from_json` remains a compatibility projection for read-only callers;
file owners must not use its fallback to authorize a replacement.
