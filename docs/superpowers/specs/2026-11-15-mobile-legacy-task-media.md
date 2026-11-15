# Legacy mobile task media presentation

Proposed compatibility slice for zerus-oqh7.7, following caller PR #2466.
No runtime changes are included in this design commit. Rebase shared phone
integration onto the admitted #2466 merge before implementation.

## Existing boundaries

The admitted schema-8 editor already supports description and comment files
through its qualified edit handle. Ordinary `TaskStore` writes deliberately
stop at schema 2 (`crates/replica-ffi/src/lib.rs`, `TaskInfo::try_from` and
`TaskStore::change`). Immutable comment media raises schema 3, and the existing
`comment_media_is_lazy_read_only_and_bound_to_a_stable_history_entry` control
pins its ordinary-handle read-only behavior. A picker must not silently widen
that writer limit or replace the qualified schema-8 lane.

Description editing needs no new FFI: `edit_content` checks the original
content revision and retained-file tokens. `add_with_media` supports a
new legacy task. `media` and `read_media` already expose at most eight files
for one selected immutable history entry. History rows carry stable entry IDs
and attachment counts; file bodies stay out of the row DTO.

## Proposed implementation

Complete both phones in one client PR, keeping the ordinary schema-2 writer
cap and the existing qualified schema-8 controls:

- Add description pickers, kept-file removal and previews to writable legacy
  task settings. Use revision-bound `edit_content`, including for text-only
  saves from the new form, so concurrent body/media changes keep the draft
  and require fresh review. Preserve additive session identities.
- Reuse the same media form for the new-task composer with
  `add_with_media`. A new draft has its own nonce; an existing draft
  additionally pins the exact task UUID and original content revision.
- Capture connection, project, task/draft, target computer, opaque session
  identity, form sequence and picker token before asynchronous import. A later
  selection, rename or callback cannot rebind them. Save and action completion
  refresh the original scope; native Send remains a separate explicit action.
- Check the original target host's `task_media_upload_v1` advertisement before
  opening a picker and again for the frozen save target. Unsupported hosts get
  an upfront localized explanation. Keep existing draft bytes on failure;
  a later network result never recreates or retries a native action.
- Use the existing private attachment store, file/type/byte limits and one
  active/latest pending checkpoint writer. Preserve new files and retained
  tokens until the original save settles; cleanup may remove only this draft's
  unreferenced private files.
- Add lazy comment-file viewing to both inspectors, including read-only tasks.
  A click selects one immutable entry ID and loads only its bounded metadata;
  a preview reads the exact token/position in that same entry. New history rows
  never reinterpret the selection as an array index. Retain no growing
  per-row metadata/body cache.
- Comment-file creation continues through the already reviewed schema-8 editor.
  Ordinary schema-3 tasks remain read-only in this slice. New visible strings
  use both existing English/Russian catalogs.

## Scope decision before code

The originally filed follow-up also mentioned legacy comment-file creation.
That requires an explicit new ordinary schema-3 writer contract, rather than
just UI wiring. The proposal above completes the compatible description and
history-media surfaces without that expansion. Confirm this boundary before
implementation; if legacy schema-3 writing is required now, review its FFI
method, precise writer limit, old-reader controls and failure semantics first.
Never change the general writer cap as an incidental picker fix.

## Local proof

Runtime RED controls cover stale content revision, concurrent unrelated
history, wrong connection/project/task, late picker results, retained-file
tokens, quota/import failure, immutable entry selection after arriving rows,
original-scope completion and unsupported target capability. Keep literal
older-reader/read-only controls and the qualified schema-8 lane unchanged.
Kill production guard mutants and restore exact source. Run Android unit tests,
lint and unsigned three-ABI assembly on Arch, plus ZerusKit and an unsigned
simulator build in the private Mac clone. No devices, signing, installs, relay
changes or hosted gates.
