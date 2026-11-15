# Session listing metadata design

Status: design approved for zerus-63r5s, based on main940d45423. The coordinator
explicitly released the #2405 heavy hold before the first RED build. Land foundations,
publishers and repair first; switch ls and then watch in separate reviewed slices
only after coverage/parity proof. Reader activation is not part of slice1.

Use a separate advisory SQLite display index, not one sidecar file opened for
each session and not the process/caller index. Sidecars alone leave whole-directory
polling and N-file startup reads. Canonical record JSON and existing action
validation remain authoritative; no machine-identity API is needed by this cache.

## What the readers need

| Consumer | Bounded persisted input |
| --- | --- |
| Identity/routing | canonical source key/path/layout and file version; session/run/conversation/launch IDs; name, journal_name, agent/backend, terminal name; rename-shadow/route binding; account ID/home; recorded cwd/launch dir and public worktree owner ID |
| Card status | activity/phase/conversation state; created/progress/attention/turn/compaction timestamps; error/completion/paused/starting/clear/resume/recovery flags; PID/birth and supervisor identity; goal, effort/model, cache and scheduling display inputs |
| Attention/work | exact attention/reply identities; public prompt/reply previews including keep_warm_quiet; bounded active-tool identity/name/detail and child status/count/preview inputs; parent relation and input/approval completeness. No shell output, full tool arguments, transcript/event arrays, reasoning, custody or confirmation tokens |
| Watch routing | source path to name/journal_name/identity and revision; indexed reverse journal-name lookup. Watch's current binding_identity needs only name, journal_name and rename-shadow exclusion |

Use one explicit, versioned pure ListingSeed projector from the writer's already
available record. Preserve existing field semantics and opaque attention IDs;
do not call journal::summary on the write path: it does journal/pane/native I/O.
Initially cap a metadata row at256KiB. An oversized/unsupported row is explicitly
unknown/last-known and queues repair; never silently truncate identity/attention
or treat it as an absent session.

Native liveness and background work stay fresh observations, with PID/birth
verification and one lazy process sample per read batch. Pending questions,
latest reply, read marks, notifications, goals/schedules, waits, account marks and
folder/project/parent placement are bounded indexed overlays for requested rows.
They must not trigger every-record or every-journal scans. Activity, native tool
output and action preparation continue exact selected-record reads. Provider
runtime data uses bounded requested-session RPCs/cache, preserving old resident
DeepSeek compatibility; no host restart or hot reload.

## Publication and coverage

Audit all seven binding namespaces: root/bindings/sessions, dsh bindings/sessions
and opencode bindings/sessions. Cover terminal launch/hook/supervisor/lifecycle
through storage::atomic_record/remove, provider save/remove, and the bypasses:
guarded recovery, fresh-ID/owner transfer, rename/mirror/route publication,
clear/archive/restore and Windows session-host publication. Teams-created members
use their real terminal/provider writer, with a test for any extra bypass.
Non-record overlays have their own revision/watermark, not an assumed record write.
A writer matrix/control must remain incomplete until every intended family is covered.

Reserve an immutable change intent before canonical rename/delete, then publish
metadata/identity/tombstone and settle that intent in one SQLite transaction after
checking the resulting canonical file version. Multi-source renames publish as
one group; shadows never become duplicate sessions. Canonical writes must still
succeed if the advisory store is unavailable: leave durable coverage-lost/pending
state when possible; mismatched directory witnesses and row file stamps otherwise
force incomplete coverage/repair. Never block a hook/launch on index durability.
Use canonical-writer to index lock order; repair never acquires a canonical writer
lock while holding an index transaction.

## Reads, paging and migration

Store ordered rows plus bounded revision/tombstone pages. Read at most128 rows
and512KiB per page; opaque cursors bind root/store epoch, generation and high-water.
Watch consumes a snapshot walk and revision pages instead of retaining every
record identity/row/digest in memory or running stamps::scan on every pulse.
A page verifies its canonical file stamps; a mismatch is stale/repair, never
permission to infer a removal or switch to a different run.

Existing watch-v1 page framing remains compatible: do not send last=true for an
incomplete replacement. Retain the prior complete generation and expose loading/
last-known coverage while rebuilding; a cold reader must not publish a fabricated
empty complete list. A paged display read may expose explicit partial coverage
without sweeping unvisited rows. Legacy ls --json keeps its complete wire shape,
streamed from bounded pages; if complete observed coverage is unavailable within
its bounded wait, refuse truthfully rather than return partial success as absence.
All action/caller/cleanup paths still re-read canonical state independently.
The reader kill-switch must handle absent, corrupt, disabled, stale or unready
index state: reuse the existing authoritative path only within explicit entry,
byte and time bounds, otherwise refuse truthfully. It must never return wrong
metadata, fabricate completeness or silently run the old unbounded fallback.
Test the kill-switch before either reader is activated.

Old records migrate lazily without rewriting their JSON or minting identities.
A background worker queues filenames and builds a new staged generation using
bounded slices: initially256 entries,16MiB raw bytes (the existing per-record
ceiling) and50ms checks. Keep the directory iterator across slices and persist
the bounded discovery queue; do not reopen and skip a growing prefix each poll.
After a crash restart discovery safely, retaining staged/last-known rows; publish
complete coverage only after a full stable pass plus outstanding intents/deltas.
Old/unknown writers invalidate directory witnesses; periodic bounded verification
also catches in-place edits. Incomplete or stale coverage never proves absence.

Move automatic archive reconciliation out of the listing request: queue only
candidates and apply the existing exact canonical checks under their lock after
re-reading that record. Unready archive/reference coverage defers reconciliation.
Keep archive catalog rebuild work in zerus-scbyk; this change must not call its
unbounded rebuild inline as a fallback. Display cache evidence authorizes no archive,
restore, signal, removal, ownership change or consent reconstruction.

## Implementation and evidence after approval

Land foundations/publishers/repair before switching readers, with explicit
incomplete coverage until the writer audit passes. Then switch listing/watch to
paged index reads, retaining existing wire compatibility. RED parity controls
compare the same public listing from full fixtures and ListingSeed, especially
Background/Unknown, attention, unread reply, quiet snapshots, clear/resume and
rename/ID layouts. Crash cuts cover pre/post-file/pre-settlement and outage;
mutants remove bounds, route/version checks or incomplete-coverage protections.

Reuse the20k/50k audit fixture. Measure cold bootstrap separately from warm ls,
first watch catalog, repeat pulses and one-record updates, including bytes/RSS
and write overhead. Warm queries must avoid raw whole-record scans and watch
identity preload; do not claim cold migration or full legacy output is O(1).
Measure incremental canonical-write overhead with and without publication,
reporting p50/p95 at20k and50k rows (target a couple of milliseconds p50,
comparable to the process index). Report database plus WAL/index size at both
scales, including pending/repair state, and prove advisory failure still preserves
the original canonical outcome. No heavy measurement/build/test begins until
the coordinator explicitly releases the fence hold.

## Slice 1 implementation boundary

The store is local display evidence with no machine-ID dependency. Canonical
records keep their existing schema and bytes. Outstanding intents and the
coverage-lost marker never grant authority, and do not authorize replay. A
canonical error is returned unchanged even when metadata settlement fails.

| Writer family | Canonical seam to cover |
| --- | --- |
| Terminal launches, supervisor, hooks and lifecycle (including Teams terminal members and Windows session host) | storage::atomic_record / remove_record_file |
| DeepSeek resident-host and hosted member bindings, OpenCode | Provider::save / remove (separate from atomic_record) |
| Fresh ID launch | create_id_record's original Directory::write_new |
| Guarded recovery and rename mirrors | View::atomic_record / remove, retaining its original Effects fences |
| Dormant legacy-to-ID owner transfer | original legacy replacement, ID publication and legacy retention |
| Legacy rename shadows | atomic_record / rename::remove |
| Older binaries, in-place/uninstrumented writers, original picker keeper | bounded periodic canonical repair; never asserted as current publisher coverage |

The original picker keeper continues to perform only its lifeline-bound effects.
Its existing file/version witnesses and subsequent repair, rather than unrelated
SQLite effects inside that keeper, detect display coverage changes. Completion of
a display scan describes an observed generation, never proof of canonical absence.
A later reader review must check overlays, freshness and all unsupported fields;
this first slice cannot activate any reader as a shortcut.

Repair does not acquire a canonical writer lock, run canonical recovery or replay
an operation. A held writer alone is not proof of inconsistent source inputs.
The rename transaction directory is enumerated within the same entry budget;
its pending JSON journals prevent sealing, and its directory witness is rechecked.
Identity-layout transition markers remain explicitly unsupported inputs until
canonical reconciliation can supply a qualified display seed. Display repair may
observe bytes left by an unknown canonical call, but it never changes that call's
receipt or promotes its delivery outcome.

Cached seeds carry an integrity checksum and a canonical source key; both are
verified before cache reuse and paged exposure. A store epoch change invalidates
retained directory cursors, including when a recreated store reuses scan numbers.
A changed selected-root alias prevents metadata settlement while the canonical
call still returns its original result. None of these data checks grants authority.
