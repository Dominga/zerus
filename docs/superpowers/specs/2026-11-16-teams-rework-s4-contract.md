# Phase rework: slice 4 interface

Implementation contract for `zerus-63rru.161.4`, based on admitted main
`8e6e792ec9e0eef51cdf68fab0087e572bd1c1c3`. This pins the interface implemented by
the separate rework slice; its complete qualification remains in the slice
report, rather than being inferred from this contract. The governing state and authority contract is
[M2 slice 4](2026-11-15-teams-phases-m2.md).

## Actions

Every operation retains the ordinary `request_id`, `team` and
`expected_revision` envelope and common `ops::apply` writer. CLI operation
names use hyphens; typed `TeamAction.kind` and `Operation.op` use underscores.
An original request is checked through the existing unresolved-intent path;
an unknown result never authorizes replay with another request identity.

| Operation | Fields in addition to the envelope | Authority |
| --- | --- | --- |
| `send_back_phase` | `phase`, `publication`, `comment` | Person, or the exact bound lead only when the phase's acceptance is Lead and `accept_results` is delegated. |
| `submit_phase_corrections` | `phase`, `publication`, `rework`, `draft` | Person or exact bound lead. |
| `edit_phase_corrections` | `phase`, `publication`, `rework`, `generation`, `draft` | Person or exact bound lead. |
| `approve_phase_corrections` | `phase`, `publication`, `rework`, `generation` | Person; service only under the existing explicit Auto policy. |
| `reject_phase_corrections` | `phase`, `publication`, `rework`, `generation`, `comment` | Person. |
| `undo_phase_correction_rejection` | `phase`, `publication`, `rework`, `generation`, `rejection` | Person. |
| `reconcile_correction_review` | `phase`, `publication`, `rework`, `generation` | Service only. |
| `accept_phase` | Existing `phase`, `publication` shape, unchanged | Person, or exact bound lead only for Lead acceptance plus `accept_results`. No open rework may exist. |

`publication` on correction operations is the exact retained base publication,
not the new correction publication. `rework` is the SendBack request UUID.
The correction approval request UUID becomes the new publication UUID. All
UUIDs remain canonical and non-nil. Generation is monotonically allocated
from the phase's existing draft counter; it is never reset for another episode.
Comments are nonblank, bounded by the existing 16,384-byte reason limit and
use the existing forbidden-character rules. A comment never rewrites a task.

The correction `draft` uses the existing complete `DraftInput` shape. Its
barrier must match the existing phase. Approval preserves acceptance,
approval policy, barrier, pause/ownership, order, kind and mode; it grants no
S5 control change. Header reuse is checked again at publication, and retained
earlier facts remain part of the final completion predicate.
 New node
IDs come from `next_node` and are never aliases of accepted tasks. The
operations above are separate variants so ordinary saved S3 requests keep
their exact pre-change shape. No Person authority is inferred from payload
flags, and independently bound Session operations retain their own proof.

## States and publication

SendBack rechecks the exact waiting phase and publication under the writer
lock and creates a `drafting` rework episode. It keeps the real phase in
AwaitingAcceptance. Before the first nonempty submitted proposal, the episode
has no draft; it never exposes an empty editable plan. Submission/editing
creates a complete correction proposal and a fresh review generation. Its
review states are `review` and `rejected`; neither changes the real gate.

Explicit approval freezes only those correction nodes into a new durable
publication and changes the episode to `publishing`. The original named-task
writer, saved requests, read-only recovery and all-or-nothing binding path
remain authoritative. Only settled publication installs the correction phase
header, moves the real phase to Approved and marks the episode `published`.
SendBack, draft editing and rejection alone never open the gate. Acceptance
rechecks all live and archived task/stage/decision/integration/publication
facts; no task or integration receipt is manufactured.

The first SendBack fact atomically raises the private execution floor to the
registry's `CORE_REWORK=6`. Ordinary native migration stays at 4; publication
stays at 5; native 7 remains reserved and refused. New creates, drafts and
approvals cannot lower an existing floor. The real admitted old-5 engine must
refuse fresh and already-open writers after the rework fact, including both
admission race orders.

## Existing bounded reader and history membership

Use the same `team phase --phase N` read, selection epoch, poller, core model,
coalescer and batch. No new history poller, task estimate writer or cache is
introduced. The current phase DTO gains `rework_available` and optional
`rework`:

```text
rework = {
  id: UUID,
  base_publication: UUID,
  comment: string,
  state: drafting | review | rejected | publishing | published,
  draft: null | PhaseDraft
}
```

The nested draft is a bounded correction proposal for the same phase and
read revision. Its `generation` is the exact new proposal generation, its
`phase` is the proposed header, and its `publication` is null before approval
or the exact new reserved correction publication. It has no nested `rework`
or live snapshot. The core validates that single level explicitly. Draft and
node bounds remain the existing 200-node/262,144-byte limits. The full phase
including retained nodes must remain inside the existing live snapshot bound.

While an episode is open, the outer phase remains AwaitingAcceptance and the
outer `publication`/`generation`/`nodes` describe its retained base approved
snapshot. The proposal lives only in `rework.draft`; it never replaces those
base estimates. `accept_available` is false during open rework. After the new
publication settles, outer publication/generation/nodes switch to that exact
new approved snapshot, and `rework.state=published` retains the base UUID and
comment with `draft=null`.

`live` continues to cover all supplied live and archived nodes of the phase,
including earlier accepted work; it is not the membership list of one
publication. `live.complete` proves the bounded whole-phase scope, including
the publication facts. Match current publication nodes by their stable IDs
and exact task identities; never replace an earlier snapshot's estimates with
current/live estimates. Extra earlier nodes in the whole-phase scope do not
belong to the current publication's `draft.nodes`.

All accepted task documents, commits, publication snapshots and original
per-node requests remain stored unchanged. The current rework read exposes
its exact base snapshot while that episode is open; it does **not** add an
arbitrary historical-publication selector or enumerate every prior snapshot.
Once a prior snapshot is no longer returned by this read, projections must
show that historical version as unavailable, rather than scan, infer an
ordinal epoch or substitute current data. A future historical read is outside
this slice. Missing/invalid/incomplete live scopes retain their honest flags,
disabled acceptance and unavailable numeric metrics; the shared graph may
still show only supplied partial nodes with its explicit warning.

An older hgs without the capability leaves SendBack/correction UI unavailable.
An older core encountering these unknown draft fields follows its existing
read-only/unknown-field behavior. No unknown field grants an action or causes
a silent downgrade to S3 semantics. Exact old/new read controls will pin this.

## Canonical native row bounds

The exact current episode is `phase_reworks(team,phase)` selected by descending
`seq`, then bound to its `id` and `base_publication`. Relevant fields are
`state`, `generation`, `header`, `approval` and immutable `comment`. Header
JSON is limited to 4,096 bytes and comment to 16,384 bytes before loading.
The original actor/creation stamp are immutable attribution, not new authority.
The current draft counter remains `phase_drafts(team,phase).generation`.

Before approval, proposal rows are `phase_draft_nodes(team,phase,id).data`: at
most 200 rows and 262,144 aggregate data bytes, checked before loading. The
retained base is `phase_publications.id=base_publication`; the reserved new
approval is `phase_publications.id=approval`. Each exact `snapshot` is at most
262,144 bytes. `phase_publication_nodes(publication,node)` retains each
original request, with the existing 530,000-byte per-request read bound.
The base and proposal are distinct canonical sources; do not concatenate them
into an assumed old 262,144-byte single row or omit a source from consent.
Bind the exact row/reference identities before consent and under the writer.

The first-floor migration preserves raw snapshots/requests and rebuilds only
the publication parent to permit distinct generations. Its one-time scan
bounds at 65,536 publications and 256 MiB of snapshot data, returning over_cap
before changing a gate/floor beyond that bound. SQL storage errors roll the
parent rebuild and floor back together. Normal opens never perform this rewrite.
