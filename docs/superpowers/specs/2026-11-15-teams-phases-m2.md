# Teams M2: planned phases on the existing engine

Design note for `zerus-63rru.161`, 2026-11-15. The Teams coordinator accepted
the design at `54178e5a9` and authorized slices 1 through 5 in order, each in
its own PR and review. Slices 6 and 7 are deferred.
The governing design is [Agent Teams](2026-11-11-agent-teams-design.md), M2;
the authority and request contract is [the reference](../../reference.md#agent-teams).

## Ground and boundaries

Read against main `21301c3c2fef02e57c72befe482f9645453c1108`. WhiteWh's UI
reference is `refs/remotes/whitewh-https/main` at
`ad0ad8d34181f270f4e890f853a8cbe9f0c3879a`, inspected with Git only. Its
backend, commands and whole-team view are not our implementation contract.

Our `crates/team-plan/src/model.rs` already defines `Phase` and draft `Node`s.
`src/teams/ops/team.rs::create` currently writes only the approved Free queue
at phase 0. `ops::Operation`, CLI commands and client actions have no phase
mutations. `ready.rs::phase_open`, `after_awaited_phase`, `behind_barrier` and
`position` already enforce approval, pause, acceptance gates, barriers and
display order. They do not branch on `mode`. Reuse this kernel, including its
dependency, integration, staffing and write-set rules; the UI never decides
readiness.

Coordinator decision, 2026-11-15: early slices keep phase 0 as the only intake
queue. `queue.rs::node_metadata` keeps `QUEUE_PHASE`. Dynamic Free-queue
conversion is a separate, deferred last slice, with one authoritative intake
target and a person-only operation to change it.

## Records and publication

| Existing field | Meaning in M2 |
|---|---|
| `n` | Stable identity. Reserve 0 for intake; allocate planned phases from 1. Never renumber identities to move a phase. |
| `kind` | `Plan` for planned work, `Queue` for intake. The scheduler's queue exemption stays intact. |
| `mode` | Planned phases start `Standard`; phase 0 stays `Free` in the early slices. A label change alone never converts intake. |
| `approval` | `Review` or the person's `Auto` policy, captured from settings. The lead cannot grant itself auto-approval. |
| `state` | `Draft`, `Rejected`, `Approved`, `AwaitingAcceptance`, `Done`; no fork-only `queued` or `running` values stored. |
| `acceptance` | `Lead` or `Person`; reject `Pipeline` as the existing validator does. A lead's soft acceptance needs `accept_results`. |
| `barrier`, `paused`, `order` | Feed the existing kernel unchanged. Ordering is `(order or n, n)`. A pause stops new hand-outs and sends no session command. |

Keep the existing serialized `Phase` and `Node` shapes. Add companion tables
in new store modules: draft nodes keyed by `(team, phase, node)`, and phase
control keyed by `(team, phase)`. Control records hold the draft generation,
proposal/rejection identity, bounded comment, pause owner/reason, publication
identity and pending review/acceptance episode. Link cards to the exact phase
and draft generation in companion storage; do not invent a task or parse a
card's sentence to recover that reference.

Drafts are **not** live task rows. `nodes.node_state` is generated from task
observations and `node_identity` requires task identity columns; putting a
draft there would misrepresent its state. Reserve node IDs through
`teams.next_node`, store draft `Node`s with `task=None`, and install live
identity/binding rows only from a confirmed task-creation outcome. IDs are
never reused. Draft criteria need a bounded companion value: `Node` has no
criteria field today. Require nonempty criteria and include them in the task
body produced at approval, without adding an unknown field to old row JSON.

Every new mutation goes through `ops::apply`: exact actor binding, strict
request, `request_id`, `expected_revision`, saved success/refusal, one revision
and journal event in the same immediate transaction. Keep replay before the
stale-revision check; replay grants no new side effect. Preserve `store_busy`
for a writer that never began and `storage_failed` for an unknown storage
result. Refresh after `stale_revision`; never silently rebuild and resend an
old intent against a new draft.

New storage follows the existing schema lock/fence/savepoint convention,
re-reading the schema under the writer lock. Because an older actor does not
advance M2 phase gates, executable planned work needs a private Teams schema
fence, not merely extra columns that an older actor ignores. Pin old/new
opens, an already-open older handle and spawn-admission races before enabling
publication. Slice 1's additive draft companions have no executable effect;
the fence belongs to the first executable slice, not the draft-only slice.
A newer Teams store must be refused by an older engine before
it can cause a planned effect; no desktop/task-document schema bump is needed.

Slice 2 uses private `user_version` 3 for planned startup, raised in the same
transaction as `StartPlanning`; queue-only creation stays at 2. Its pause flag
is a companion value and the team retains `Planning`, so an already-open old
writer's legacy `Paused -> Running` resume cannot activate the roster. The new
selector and final admission permit only the exact unpaused lead. Initial goal
context is a team-wide inbox notice, using the existing non-task `(0,0)` notice
sentinel and the original guarded doorbell; it never becomes a fake task row.
`current` also exposes the bound lead's goal. Ordinary planning notices retain
their own authority and listed decisions; task-backed and integration decisions
retain their existing active-team rules. No saved launch Plan shape changes.

The native Person-evidence migration from .31 owns core layout 4. Ordinary
opens preserve that native allocation and its validated nonce and receipts;
publication alone raises the private execution floor to 5 in its operation
transaction. Layout 6 is reserved for rework in slice 4 and remains refused
by slice 3. Legacy queue 2 and planning 3 remain migration inputs. These core
allocations live in `src/teams/store/layout.rs`, with the allocation uniqueness
control; timer layouts 8 through 12 and raw pinned mark 2147483660 are separate.
The supported ceiling is not the ordinary-open migration target.

## Operations and authority

These names are proposed additions, not commands that exist today. Each takes
the ordinary envelope. Member and unrelated-session mutations are refused.
The person can read/edit through the same ops as the lead, within the approved
goal; changing that goal or delegations remains person-only.

| Action | Proposed operation and authority | State and effect |
|---|---|---|
| Lead's plan | `SubmitDraftPhase`; person or exact lead | Add a bounded phase and its draft nodes; no task, branch, member launch or hand-out. Initial plans can be submitted phase by phase. |
| Edit a draft | `EditDraftPhase`; person or lead | Change title/text/criteria, routing, dependencies, write sets, estimate and order of draft nodes; validate the complete resulting phase atomically. Editing a rejected draft creates a new review generation. |
| Start planning | `StartPlanning`; person | Separate creation op, status `Planning`; launch the lead only. Keep ordinary `Create` semantics and saved request shape unchanged. |
| Approve | `ApprovePhase`; person, or a service fact under the person's Auto policy | Freeze the exact reviewed generation and reserve publication. No task is runnable until all publication facts are settled. For the initial plan, the person's confirmation also authorizes the shown roster/folders. |
| Reject with comment | `RejectPhase`; person | `Draft -> Rejected`; required bounded nonblank comment, one lead notice, close only that generation's review card. No task creation. |
| Undo rejection | `UndoRejectPhase`; person | `Rejected -> Draft` only while that rejection is still current and no rework/publication intervened. Undo does not approve. |
| Completion fact | `ReconcilePhase`; service only | Recompute from current live **and archived** nodes, stages, decisions and publication facts. Every task decided, integration settled, no open Crit/effect: `Approved -> AwaitingAcceptance`, once per episode. Never finish an open queue because it is empty. |
| Accept phase | `AcceptPhase`; person, or delegated lead for a soft gate | Recheck the completion facts under the lock; `AwaitingAcceptance -> Done`. The lead cannot accept a hard gate, and phase acceptance supplies no task/integration receipt. |
| Send back | `SendBackPhase`; person, or delegated lead for its soft gate | Required comment and a rework draft; retain `AwaitingAcceptance` while the lead drafts corrections. Existing accepted tasks/commits stay history. Approval of explicit correction nodes returns to `Approved`; do not mass-reopen Done tasks or rewind integration. |
| Barrier | `SetPhaseBarrier`; person or lead | Set the flag on a nonterminal planned phase, log who changed it. Removing it never bypasses an awaiting-acceptance gate. |
| Pause/resume phase | `PausePhase`, `ResumePhase`; person or lead | Set the flag and owner/reason. A lead cannot remove the person's pause. Existing work and its reports continue. |
| Move phase | `MovePhase {phase, relative_to, before/after}`; person or lead | Change order only. Initially allow drafts/rejected and approved-but-unstarted phases. Validate the resulting gate/barrier order; refuse started, publishing or terminal phases. |
| Acceptance policy | `SetPhaseAcceptance`; person | Choose lead/person before execution, with the team default for new phases. Never turn a waiting hard gate into a lead decision as a side effect. |

"Unstarted" is a store fact, not a count on the visible page: no current or
historical attempt, reservation, pipeline work, publication in flight or
unsettled decision. Editing an approved phase is a later logged change with
its own before/after generation, and undo is allowed only before the touched
work starts. It never rewrites task titles/text solely in SQLite.

Approval spans SQLite and replica documents, so it is not one cross-store
transaction. Use a new phase-publication ledger and the existing named-task
writer pattern (`actor/tasks.rs`, `actor/native/tasks.rs`,
`tasks/team_deliver/stage.rs`), **not** the stage-only command unchanged:

1. Save the reviewed snapshot, stable per-node creation UUIDs and their
   original requests before any child. The approval request UUID identifies
   the publication; a versioned domain-separated SHA-256 UUIDv8 names each
   node's task. Recovery still reads the saved request and never reconstructs
   it from current state. Keep the phase closed to hand-outs.
2. A service-only planned-task writer checks the exact publication under the
   caller's task-store admission and Teams lock, then uses `new_named_task`,
   `Target::Any`, To do, the lead attribution and the ordinary team key/save.
   Set the key before the first save. Extend the existing pending-creation
   guard to these reserved UUIDs so the key guards the task before live binding.
3. Record each creation/binding fact. After a lost answer, read that exact
   named task and original publication, verify the key/content and record the
   fact; never allocate a replacement UUID or treat unreadable as absent.
4. Only after all nodes are confirmed, publish `Approved` and, for initial
   planning, `Running`. The ordinary launcher then creates the integration
   branch and members. Refusal, conflict, cutting-over or unknown is visible;
   stop/pause/edit racing publication cannot authorize another write.

Auto approval uses the same publication path and logs the person's policy;
it is not authority for the lead to send `ApprovePhase`. Initial activation
and its roster/folder confirmation remain person-only; Auto applies to later
phases of an activated team. Rejection/undo is
available only before publication is reserved. A mid-publication failure keeps
the snapshot frozen and reports progress; it is not silently rolled back by
deleting already-created tasks.

A published task can depend on a node that is still in another draft. Keep
that edge in a companion table whose foreign keys name the confirmed source
and the actual draft prerequisite. Publish the prerequisite by promoting its
incoming edges to ordinary dependencies in the same transaction, before
removing its draft rows. Draft edits preserve referenced IDs and validate
cycles through live, archived and draft nodes. The hand-out kernel still
treats the unpublished prerequisite as missing; no task identity is invented
for it.

Creation answers and Stop cleanup answers are distinct facts with separate
three-read unknown budgets. A late creation answer after Stop cannot close
cleanup. A known pause before a document write can resume with the saved
request; an active command refusal ends creation. Auto-approval failures use
the existing bounded retry gate, and changed settings or draft generations
trigger a fresh policy check.

Project cutover holds defer creation and Stop cleanup through the same task-store
admission, preserving the original request for a later pass.

Completion keeps the pipeline's own lifecycle facts: an archived uncreated
stage is ended only with its positive superseded record and no task binding.
Archive alone proves nothing. Holds prevent new work; a hold left on an
already accepted or cancelled task does not prevent phase acceptance.

## Projection and WhiteWh UI mapping

Add bounded `team phases` and `team phase` reads in new modules: phase summaries
and one selected phase's draft/live nodes, including explicit cursors,
revision, completion/blocked facts and available actions. They have the same
person/lead read authority as today's queue; members keep their own context
read. Respect the current
200-phase readiness ceiling, configured phase/task caps, 262,144-byte request
limit and bounded text; never assemble `TeamFixture` in production. Drafts
count toward the planned-work cap. Whole-phase validation is bounded; page
counts never prove that a phase is complete or unstarted.
The same admitted-task capacity accounting includes saved drafts at queue
admission, pipeline stage creation and settings changes, under the writer lock.

Slice 1 validation also bounds write-set comparison work under the writer
lock. Barriers and dependencies are checked first, each unordered pair once;
the sum of each pair's path bytes (plus one per path) times the other set's path count is capped
at 64 million. Exceeding it refuses atomically with `over_cap`, asking for
simpler write sets or a smaller phase. The comparison itself remains the
existing kernel's conservative overlap check.

Project through `client/core/src/teams/read`, the single Teams poller/model,
view DTOs, coalescer and batch into a separate client phase store. Preserve
unknown/malformed/missing state as unavailable; an older local or remote hgs
without the new read shows an explanatory unavailable view. It must not show
an empty plan or enabled actions. Stop following when hidden or on a team
change; keep only selected bounded pages. Do not add frontend CLI parsing or
a second polling loop. New read keys need explicit routing through the Tauri
page hook; new writes use `TeamAction`/`TeamCall`, the existing intent table
and guarded `hgs team` invocation.

| Fork UI source | Adaptation onto our contracts |
|---|---|
| `phaseHeader.ts` | Keep state buttons, counts and notes; map draft Accept to **Approve**, awaiting acceptance to **Accept phase**, reject to the comment form, undo to the exact rejection generation. Derive queued/running display from our facts; never store fork states or infer waiting from `n - 1`. |
| `TeamActions.tsx::PhaseActions` / `EditPhaseDialog` | Port the compact menu, reason dialog and editable fields into new `PhaseActions.tsx` / `PhaseDraftEditor.tsx`. Dispatch our typed operations. A page's missing tasks do not make a phase editable; stale responses keep the draft and require reread. |
| `plan-panels.tsx::LayersView`, `planModes.ts::layerRows` | Present dependency layers inside ordered phase sections, with one shared header/action component. Use `team-plan::graph::layers` output from a bounded complete phase; missing inputs show an incomplete view, not a fabricated first layer. Preserve task selection, stage detail, cancelled filtering and explicit paging. |

The Layers layout is a view, not phase order or a scheduler. Graph work in
another package consumes the same phase/node projection and stable IDs.
Do not port heat/profile/testing modes with this slice. All labels and dialogs
get English/Russian catalog entries and regeneration; no separator dots.
Adapted fork code commits carry the exact author trailer obtained separately
for each copied file from `git log -1 --format='%an <%ae>'` on the pinned ref.

## Smallest safe slices, proposed order

Each row is independently reviewable. The chosen sequence is slices 1 through
5, one PR per slice; slices 6 and 7 remain deferred follow-up work.

| Slice | Ops and authority | Tests through product writers | UI delivered |
|---|---|---|---|
| 1. Drafts and phase reads | `SubmitDraftPhase`, `EditDraftPhase` for person/lead; paged phase reads. New storage/validation modules, no task effects. | Replay/changed-ID content, stale revision, exact lead/member refusal, invalid/cyclic/cross-phase hard deps, missing criteria, role/assignee/pipeline, overlap, row/byte caps; draft edits create no replica tasks or launches. Writer-produced phase-read fixtures through real core projection. | Read draft phase headers and Layers; edit a draft title, tasks and criteria. Approval controls remain explicitly unavailable until slice 3. |
| 2. Planned startup | `StartPlanning` person-only; service lead launch. | Real creation and launch writers prove one lead, zero member worktrees/branch while Planning; repeated intent, pause/stop and launch refusal/unknown retain existing protections. | New team's **Start planning** and goal; planning state in the phase pane. Ordinary queue creation stays usable. |
| 3. Approval, publication and a complete minimal gate | `ApprovePhase`, `RejectPhase`, `UndoRejectPhase`; service publication facts and Auto path, plus service-only `ReconcilePhase` and person-only `AcceptPhase`. | Real named-task writer, crash before/after every reserve/save/bind, lost reply, duplicate recovery, reject/undo/rework races, stale approvals, project limits/cutover; no partial hand-out and no duplicate task UUID. Initial approval releases members only after publication. Completion uses live and archived nodes and publication facts; fresh hand-out admission uses the same kernel gate; acceptance rechecks under the lock. | Review diff/counts/criteria and member confirmation, Approve, Reject with comment, Undo, publication progress, Waiting for you links and a plain Accept phase button. The existing phase read also carries an explicitly complete bounded live-and-archive snapshot. |
| 4. Full acceptance and rework | `SendBackPhase`, delegated lead soft acceptance and the full authority/rework matrix. | Archived accepted nodes, unfinished stages/merge, Crit, cancellation, empty queue, repeated passes, lost reply and hard/soft authority matrix. Send-back alone never releases an acceptance gate. | Awaiting acceptance header, Accept phase, Send back with comment; corrections visible separately from already accepted work. |
| 5. Phase controls | Barrier, pause/resume, move and acceptance-policy ops with the authorities above. | Kernel + real hand-out admission after reorder/pause/barrier races; no hand-out after pause wins; started work untouched; lead cannot remove person's pause or hard gate; stable IDs and stale moves. | Header More menu, pause owner/reason, real blocked explanation, Move earlier/later and gate choice. |
| 6. Logged approved-plan edits — **DEFERRED** | Person/lead propose bounded unstarted edits; person/authorized lead commits within goal, reversible until start. Any task-document edit goes through its product writer and recorded effect. | Writer-produced before/after generations, task edits, start/reservation versus edit/undo races, lost reply and supersession. No SQLite-only task rewrite or edit of work already started. | Extend draft editor to explicit change review/undo for eligible approved work. Keep busy/started fields read-only. |
| 7. Dynamic mode and intake — **DEFERRED** | Person-only `SetIntakePhase` changes one authoritative intake target; `SetPhaseMode` shares that transaction. Lead may propose a card, never switch it. Requires a separately approved mapping of `kind`, identity and queue routing. | Two phases can never be open intake simultaneously; all CLI/GUI/remote intake writers use the same target, replay/reorder/concurrent switches/old readers and empty-Free-phase lifetime covered. | Standard/Free control with confirmation and truthful capability/blocked reason. Until this slice, planned phases are Standard and phase 0 is the Free queue. |

Coordinator decision, 2026-11-15: publication ships with the minimal completion
and acceptance gate, so an activated multi-phase team has a way through it.
The service records Approved to AwaitingAcceptance from complete live/archive
facts once per episode; hand-out admission recomputes that gate through the
same kernel. The person's Accept phase rechecks completion under the lock.
Slice 4 adds send-back, delegated soft acceptance and the full rework UI.

For each implementation slice, run the scoped root/store/actor and team-plan
tests; client/core and touched-screen Vitest coverage; catalog/coverage and
separator checks; strict clippy for every touched Rust package, Windows lint
and privacy. All heavy steps use the shared wrapper, four jobs/threads and
private state/TMUX/targets. Use real producer fixtures and name intentional
fault-injection SQL. Kill production mutations of authority, stale-revision,
draft/task separation, stored publication identity, phase gate and intake
uniqueness in the slice that introduces each invariant. No build or execution
of fork code, live session manipulation, installed application update or
hosted CI is part of design validation.
