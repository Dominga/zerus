# M2 S5: phase control contract

Status: implementation preparation on admitted S4 `32e88ec9c94f36c9de4f03aa128bafeaa9cc85df`.
The initial interface pin `443f5e6a3` added shared identity types only; current
implementation and local controls remain unadmitted pending native7 integration.
The governing slice is section 5 of
[the M2 note](2026-11-15-teams-phases-m2.md). Engineering task:
`zerus-63rru.161.5`. Final integration follows actual admitted native S2/core7.
Native S2 does not depend on this unadmitted interface.

## Actions and exact identity

All five operations enter the common `ops::apply` transaction with the existing
`team`, `request_id` and `expected_revision`. `scope` uses the shared
`zerus_team_plan::phase_control::PhaseControlScope`:

```text
Draft { kind: "draft", phase, generation }
Published { kind: "published", phase, generation, publication, rework }
CorrectionScope { id, generation: null|u64, approval: null|UUID }
```

`generation` in Published is the returned frozen publication's generation;
`publication` is its exact UUID. While a correction is open, this is its retained
BASE, and `rework` names that exact episode, proposal generation (if installed)
and reserved NEW approval (if any). When no correction is open, `rework` is null.
A completed historical rework is not the current episode. Every identity is
compared with current storage under custody, including explicit absence. A
missing current row, partial scope, invalid UUID, zero identity or unknown kind
never adopts a newer publication, generation, relative target or pause.
Generations are positive exact JSON integers (at most `2^53-1`); open proposal
generation exceeds its BASE, and episode/BASE/NEW UUIDs are distinct.

| Rust operation / typed TeamAction | CLI command | Additional fields | Authority |
| --- | --- | --- | --- |
| `SetPhaseBarrier` | `set-phase-barrier` | `scope, barrier: bool` | Person or exact current lead |
| `PausePhase` | `pause-phase` | `scope, pause: null\|UUID, reason: String` | Person or exact current lead |
| `ResumePhase` | `resume-phase` | `scope, pause: null\|UUID` | Person or eligible exact current lead |
| `MovePhase` | `move-phase` | `scope, relative_to: PhaseControlScope, placement: "before"\|"after"` | Person or exact current lead |
| `SetPhaseAcceptance` | `set-phase-acceptance` | `scope, acceptance: "person"\|"lead"` | Person only |

The operation names serialize to snake_case in the common saved Request. The
CLI accepts the existing JSON envelope on stdin and supplies no Actor. Every
Person variant must be explicitly enrolled in actual native7's typed
`TeamCall`/`team_action`, plan/reference/original-only path, including its bounded
canonical facts before consent and under the writer. No flag, serialized proof,
CLI Person fallback, or service authority replaces that admission. A lead remains
an independently bound Session; member, other session, Service and rebound lead
requests are refused. Phase0 and non-Plan phases are refused by every operation.

Replay retains common actor verification and replay-before-stale behavior. A
saved exact success/refusal answers its original UUID without executing again;
changed content conflicts. An unknown result is checked using the original
request only. UI Check never creates a new UUID or adopts fresh scope. A new
decision receives exactly one team revision and journal event; storage errors
roll back, and a busy writer that never began returns `store_busy`.

## Pause ownership and continuation

A new pause episode uses the PausePhase request UUID as its identity. Its owner
is derived exclusively from the verified Actor: Person, or the exact lead's
person/member/SessionKey. Owner, reason and creation time are immutable.
Reason is required, at most 2048 UTF-8 bytes, and passes the common forbidden
character rules. The caller never supplies an owner. Resume closes only the
named current episode. Replacing a reason/owner is a new explicit PausePhase
request naming the current episode; it closes that episode and creates a new
one in the same transaction.

Person may resume or replace any known pause. Lead may resume/replace only its
own exact current Session pause; it cannot remove a Person, predecessor-session
or unknown-owner pause. A legacy `Phase.paused=true` without an episode is shown
as paused with unknown ownership; only a fresh native Person decision may
resume or adopt it with `pause:null`. An episode/boolean disagreement or malformed
owner is unavailable, never interpreted as running or as legacy absence.

The existing `Phase.paused` bit remains the readiness fact. Its update and the
episode update commit together. New ordinary HandoutIntent and final native
hand-out admission recheck the same kernel under the Teams writer. If pause
wins that lock, no new hand-out starts; if admission wins, that already admitted
work may continue. Reports, outcome facts, completion, integration and existing
in-hands Return/Reassign/reopen continuation remain possible. Pause is not a
session command and does not stop a session or cancel an original delivery.

## Barrier, acceptance and stable movement

Barrier and pause operate on nonterminal planned phases. Barrier does not remove
AwaitingAcceptance or an open correction gate. Removing a barrier and moving a
phase revalidate write-set/dependency/barrier constraints through the existing
draft validation and scheduler rules; no second scheduling engine is added.

An open S4 correction retains the actual phase controls. A permitted S5 barrier
or pause change must update that field in both actual Phase and the one open
proposal header atomically (when a header exists), after verifying all retained
controls matched before the change. This includes a reserved correction
publication. It changes no proposal nodes, title, acceptance, approval policy,
order, BASE/NEW UUID, generation, frozen snapshot, original task request or
receipt. Closed historical headers remain unchanged. Corruption is refused,
not repaired. The team revision invalidates earlier unexecuted reviews.

MovePhase changes order only, preserving stable `n`. The source, exact relative
target and every crossed phase must positively satisfy the same unstarted
predicate and be Draft, Rejected or Approved; publishing, AwaitingAcceptance,
Done, open rework and unknown states are refused. It cannot move across a
started or waiting gate. The existing distinct effective order slots are
permuted within that finite interval; no renumbering of phase IDs, rewriting of
other phases or historical snapshots occurs. Ambiguous/colliding slots refuse.
The complete resulting order is validated before any write. Earlier/later UI
actions use supplied stable target scope, never `phase +/- 1` or a page offset.

SetPhaseAcceptance requires the same positive unstarted predicate, no open
rework and Person authority. It changes only the selected stored phase policy,
never the team default. AwaitingAcceptance cannot be softened even when all its
tasks are complete. No controls change approval Review/Auto, kind, mode, intake,
title, task membership, accepted tasks/commits, publication or integration history.

## Positive prestart proof and bounds

There is one store predicate shared by control reads and writers. It is not
derived from the visible queue, an empty queue, a holder, an effect's current
state or only the newest publication. Indexed Teams history and fresh task
documents must agree under retained custody:

* Phase/draft/publication/rework identities must be coherent and bounded. Draft
  or Rejected has a complete nonempty current draft and no publication/runtime
  identity. Approved has only settled complete publication facts and exact
  current/archived membership; reserved/refused/uncertain task creation refuses.
* Live and archived nodes are selected by the existing indexed phase prefix,
  with the phase snapshot's 4096-node/7-MiB bound. For a published task,
  unobserved/missing/conflicting task state is unknown, never proof of unstarted.
  Generation must be 1, actual observed stage Todo/Backlog, no attempt/current or
  last attempt identity, no cancellation, integration receipt or started stage.
* The supported Person task-send writer can reserve and save an attempt before
  the next service observation. A cached Todo/attempt0 row therefore cannot
  establish that a published task is still unstarted. The selected, relative
  and crossed phase scopes first select exact task IDs through the indexed
  live/archive membership, within one short Teams read snapshot. That snapshot
  is released **before** existing-only project custody is acquired. No missing
  project, directory or task is created, repaired or discovered through a
  catalog scan. All tasks must belong to the exact stored project/home/team;
  the finite cohort is capped at 4096 nodes across the selection.
* The private project lock is retained through the final Teams read or writer
  commit. Complete task documents are loaded within a five-second total
  budget (checked before and after each synchronous read), with a 300-ms
  project-lock wait. Optional control proof caps each retained file at 16 MiB
  and the whole selected cohort at 64 MiB before invoking the ordinary Loro
  reader. Exceeding this bound makes only the control proof unavailable; normal
  task readers retain their existing unlimited-history contract. The existing
  replica/store identity, project marker, directory and lock objects are bound
  across acquisition and final custody; missing identities are never created. Missing, damaged, newer, partial,
  differently bound or busy documents are unavailable. Every document must
  still be writable Todo/Backlog, with no retained attempt, execution or
  unexplained execution entry, and its revision must match the Teams
  observation. The final Teams snapshot rechecks the same team home/project,
  revision, tagged phase scopes and exact node/task membership. No second
  ProjectStore may be opened under a caller's existing project custody.
  Native S5 enrollment must bind this same resource set before consent and
  recheck it under writer custody after actual core7 admission.
* Per-node indexed existence probes include effects in **all** states and
  generations, reports, decisions (including settled history), integration
  facts, task observations, pipeline runs, stage tasks, verdicts and retained
  implementers. A reservation, prior attempt, pipeline/stage history or unsettled
  effect is a refusal. These facts are not erased by an archive/restore.
* Add the bounded `(team,target,node)` stage-task index if the existing target
  lookup lacks one. Do not scan or materialize journals/history; indexed
  point/prefix existence tests retain history without loading it. Existing
  draft graph byte/row/work bounds apply to order/barrier revalidation.

Receipt detection before external acquisition only avoids opening resources
for a known request. Actor/content checks, native original verification and
replay-before-stale remain in the common writer. It never returns a Person
receipt before that writer. A winning Person reservation refuses a later
Move/acceptance change; a winning retained project lease excludes reservations
until the phase-control commit or rollback. Current documents supplement,
rather than erase, all earlier indexed attempts/effects/decisions.

The service's Auto-approval loop uses an internal projection of this same
phase reader for generation/publication metadata. It does not acquire the
neighboring task documents needed only by window Move/acceptance affordances;
those unused prestart facts remain unavailable. The public selected-phase
read and every Move/acceptance writer still acquire the exact fresh resources.
This internal projection adds no route, poller or operation authority.

## Storage, legacy custody and native compatibility

The no-new-floor design remains **unproved** during source preparation. A real
private-child control found another required prerequisite: an ordinary child
spawned before PausePhase could reserve and deliver after the pause committed,
because its final `permit_for_reservation` checked the effect and task marker
but not phase readiness. The native7 candidate still had those identical
production bytes when inspected. This is a supported execution race, not an
injected companion disagreement. The S5 source rechecks the existing ordinary
hand-out kernel under the final task/Teams reservation locks; in-hands reopen
remains separate. The coordinator's source/landing decision of 2026-11-16 puts
this minimal existing-kernel recheck into native S2 **before actual7 admission**,
owned and independently qualified by that author. It adds no S5 variant,
companion or registry number. Final S5 takes that actual admitted source; the
local proposed guard remains earlier-source evidence, not a competing final
implementation. Genuine old7 compatibility still requires its real final
images and both writer orders. No new mark is allocated or compatibility
result inferred from an unadmitted candidate. Retained first-failure evidence is in
`artifacts/test-results/wp161-s5/final-pause-reservation-red.log`.

The intended layout is an additive `phase_pause_episodes` companion: immutable
episode identity/owner/reason/time, a closed flag, and a unique partial current
`(team,phase)` index. It has no executable authority independently of the
existing Phase.paused bit. Extension creation takes the writer lock/savepoint,
rechecks the core/timer fences and schema under it, and commits/rolls back
atomically. Fixed shape/row-size checks refuse malformed or unknown layouts;
absence alone means no S5 episode. Current reads fetch at most one bounded
episode (owner <=4096 bytes, reason <=2048); old episode history is not drained.

No additional core allocation is requested by this interface. Read-through of
admitted core6 and native7 candidate `a2439a09c95c0d9bc643eb36900f314dd3d9dd59`
finds the same exhaustive production Phase writers:

| Writer followed to its SQL | Relevant older behavior |
| --- | --- |
| `ops/team.rs::create` | Inserts phase0 only; cannot overwrite a planned pause/gate. |
| `ops/drafts.rs::write` | Existing-phase edit clones actual Phase, changes state/title/barrier; preserves pause, acceptance, order and all other controls. It cannot edit a publication/open rework. Its barrier authority is already Person/exact lead on a Draft/Rejected phase. |
| `ops/publication.rs::save_phase` and reject/undo/reconcile/accept/publish callers | Load actual Phase inside common custody and change state (or explicitly approved correction title); preserve pause, acceptance and order. |
| `ops/rework.rs` proposal/approval/promotion | Clone actual controls and compare retained header controls under custody; no default reconstruction or alternate policy authority. |

No earlier ResumePhase or pause companion writer exists in those sources.
Team/member resume do not change Phase.paused. Old readiness honors that bit;
the earlier final ordinary reservation omission is the prerequisite described
above, not a passing older-engine control. The corrected admitted7 must recheck
readiness at that final point under custody. The enumerated older phase writers
preserve the pause bit when editing a phase. Core6's common writer and native
admission recheck their execution fence inside custody; final native7
qualification must prove older Person writers remain fenced. Existing cached
Phase data is not an input to these writes. The new companion must be preserved
byte-for-byte by real older opens/writes, not merely ignored in a fabricated
connection test.

This source audit is not a claim that candidate7 is admitted or qualified.
Coordinator clarification (2026-11-16): old7 compatibility covers states real
supported writers, migrations and crash/rollback/replay paths can produce. An
injected active episode with `paused=false` is a separate new-engine fault
control. Old7 cannot detect a companion it does not read; no such detection is
claimed. New readiness/final ordinary admission validates companion shape,
current owner and bit consistency. A textual layout tag validates this current
format only; it is not an old-engine execution fence. No new registry number
is allocated solely for injected corruption. A genuine supported old-writer
counterexample requires reporting before any allocation.
Final S5 must re-audit the **actual admitted7** sources, use its genuine image
for fresh/cached/writer/admission orders, and bind new pause/relative/control
facts in its native original lane. Any contrary legacy writer or new required
execution fence is reported before consuming a registry mark. Reserved7,
timer8..12/bit31 are not repurposed; no alternate floor/downgrade is permitted.

## Single read and window projection

The existing `team phase --phase N` adds optional `draft.controls`, with fixed
`version:1`, exact `scope`, and these shared `PhaseControls` fields:

```text
pause: {state:"running"} | {state:"paused", episode:null|PauseEpisode}
PauseEpisode {id, owner, reason, created_ms}
owner: {kind:"person",person} | {kind:"lead",person,member,session:SessionKey}
unstarted, barrier, pause_change, resume, acceptance: {allowed:bool,reason:null|String}
earlier, later: {target:null|PhaseControlScope,availability:{allowed,reason}}
```

Reasons are at most 2048 UTF-8 bytes and owner encoding at most 4096. The shared
types define the exact read shape; `allowed` describes factual prerequisites,
never Actor authority. Unknown-owner legacy pause has `episode:null`. A missing
neighbor has `target:null` and a disabled explanation. The selected read obtains
neighboring headers itself within the existing <=200-phase cap; it does not infer neighbors
from a partial visible page. No history selector, new poller, authority or cache
is introduced. Current BASE/proposal/NEW selection and whole-phase live history
remain S4's; missing historical versions stay unavailable.

Core validates identities, flags, bounds and forbidden characters before
enabling controls. Missing capability on an older hgs, malformed/partial/stale
read, unknown control version or conflicting scope disables S5 controls. The
control writer proves complete frozen membership and every retained earlier
publication. An open correction may exclude only its exact reserved NEW from
that structural completeness test, never its BASE or earlier task/stage
history. This does not mark `live.complete` true or enable acceptance. Outside
that explicit pending-NEW case, an incomplete/missing published live frame
derives disabled control flags in Core while retaining the shared partial
Layers display and validated owner/reason. Malformed control fields still
refuse; no invalid value or historical identity is replaced.

A stale response or an unknown original locks the surrounding controls as well
as the active form, including after the form is closed. Closing only releases
the immutable modal snapshot; it cannot create another request or discard the
original Check route. The form snapshot is captured before reactive teardown,
so closing it does not read an invalidated conditional accessor.

The unchanged serialized Phase and frozen requests/snapshots remain readable by
old engines. Real old Core reads of the additive reply must be tested: unknown
fields may make actions read-only; no discard-and-enable fallback is allowed.

The header More menu exposes barrier, pause/resume with owner/reason, explicit
acceptance choice and Move earlier/later. It sends the same typed TeamCall and
retains exact original Unknown/Check behavior. Catalog English/Russian labels
explain blocked actions without dot separators. Existing Layers/metrics consume
the same projected phase/live scope without a scheduling or historical fallback.

## Qualification required before READY

Product writers create each normal row. Fault tests label deliberate malformed
rows. Both writer orders exercise pause/barrier/move against genuine hand-out
reservation/final native admission; no hand-out follows a winning pause. Tests
cover archived/prior attempts, all effect states, stages/pipeline/decision
history, stale targets, exact replay/refusal/Unknown original, lead/member/
rebound authority, hard gates, immutable S4 history/header synchronization and
extension races/rollback. Compiled production mutants must reach assertions and
be restored, with executed counts and original failure logs retained.

Final gates use scoped root/team-plan/Core/touched UI, strict touched Cargo and
Windows lint, clean-crate fmt and nonincreasing root fmt counts, privacy,
source-case, i18n and separator checks. A bounded private Mac UI/Core run seals
the exact loaded bytes and source/fixture/lock identities; mocked dispatch is
not native Person authorization. No unchanged full native/platform matrix is
repeated. Freeze one separate S5 PR/head/report for independent cross-family G1.
