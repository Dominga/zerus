# Tasks as work tracked through Activity

Status: approved for incremental implementation and live Mac evaluation,
2026-10-26. Design work is tracked in Beads `zerus-oo9v`; implementation is
tracked in `zerus-cg5h`. Individual slices must distinguish implemented behavior
from the remaining design. Public distribution follows the normal release flow.

## Concept

Activity remains the primary work surface. Add a **Tasks** tab to the existing
right inspector beside **Plan & agents**, **Details**, and **Worktrees**. It
contains the task board, with the current task, actual executor, queue, expected
result and verified delivery milestones. Owner live review, 2026-10-26: reuse the
board's stage filters and cards; show stage labels when width permits and icons
with counts on narrow panels. Owner layout correction, 2026-10-26: the upper pane
contains the board and the permanent lower pane contains the selected task's
details. Selection never inserts content into the list, moves cards, changes its
viewport or changes the divider position. Keep the divider adjustable and saved.
Remove the Project board shortcut; move the board's existing scope and settings
buttons into the inspector toolbar. Detail tab captions must remain complete on
macOS and narrow panels. Reports and comments use Activity's Markdown, author,
timestamp and message-card presentation. Opening a task's conversation uses the
central Activity renderer and composer, with an explicit task context. Do not put another full
conversation inside a narrow inspector or require a modal task window to work.

Owner density correction, 2026-10-26: collapsed cards must remain short regardless
of report length. Show a two-line title and one worker/identity line, with actions
in a compact menu. Full reports are not list previews. The selected card reveals
Description, Result or History individually; do not concatenate them into one
long document or tint every card broadly because one session owns them.

The project-wide task list remains useful for planning and review. It becomes
an optional overview; it must not replace Activity during execution.

The central model is **Task -> Execution attempts -> Evidence and results**.
A task records intent; an execution records who is doing which revision of that
task. A submitted message, a running native turn and a completed task are three
different facts. The interface must show the differences and automate transitions
when the corresponding facts are available.

```text
Sessions       Activity                           Inspector
               Task T-a1b2c3 / all session work    Tasks | Plan & agents | Details
               discussion, tools, approvals       Board + actual executor
               progress and results               Next tasks, in explicit order
               shared Activity composer           Result, milestones, history
               [Task T-a1b2c3 -> executor]         Open full project overview
```

Owner inspector review, 2026-10-26: tabs form the lower pane's first row, with
Activity/details icons at the right. Assignment is an explicit field with its
own chooser; it must never open the whole editor. Both description editing and
comments reuse MessageComposer, including attachments, previews and durable
per-task drafts. Footer workflow actions carry text, and a blocked Start explains
its reason in the pane. Switching tasks retains the presented controls until the
replacement snapshot and comment draft are loaded; handlers retain their exact
binding while loading, and unchanged icons are reused.

Owner executor decision, 2026-10-26: selecting New session or Subagent saves the
routing intent. Create the executor only on explicit Start. The first slice uses
the existing NewSessionDialog and exact launch correlation for new sessions, and
a coordinator request plus verified native-child registration for subagents.
Unknown delivery is reserved durably and requires explicit review before another
launch. This slice does not implement the execution/revision scheduler, true
per-task pause, automatic native comment delivery, or verified result artifacts.


## Central Task View after inspector preview

Status: implemented on the feature branch after `815571f9`, 2026-10-26;
qualification and signed Mac preview are in progress. The central Task View
replaces the lower inspector discussion/editor. The owner is reviewing branch
previews; this does not authorize a main merge.

Keep the board, its filters, ordering, selection and compact task actions in the
right inspector. A card opens one reusable central **Task View**, identified by
project and task UUID. It appears beside ordinary Activity with a task ID/title,
actual executor and stage. Selecting another card updates that one view instead
of opening an unbounded set of task tabs. Ordinary session Activity retains its
selection, scroll and draft and remains directly reachable. Opening a task must
not implicitly switch the selected session or send input to an executor.

The central body contains the description, discussion, execution evidence and
reported result using Activity's existing presentation. Keep a result reachable
without scrolling a long journal. Native session events enter this view only
when their task/execution identity is known; filtering a whole conversation by
its name, current task or timestamp cannot establish attribution.

Use one shared input surface with explicit task context, attachments, speech,
previews and durable drafts. Description editing, new-task creation and task
comments are distinct destinations using the same input system. Recording or
recognizing speech must not submit native session input. New-task speech fills
a reviewable task draft; task comments retain their task UUID throughout capture,
recognition and submission. A context change saves/ends recording safely rather
than redirecting it. Ordinary session drafts remain separate.


### Title-first creation from the right board

Owner steering, 2026-10-26 supersedes the earlier private-card publication
proposal: add a **New task** plus control to the right board. It opens a one-line
name entry. Confirming the name creates a shared title-only task in To do (or
Backlog when that column is selected) and immediately opens its central Task View.
There is no extra publish step after naming. Creation never starts an executor.

The task title is an explicit persisted field. **Rename** is a separate action;
subsequent description text, voice recognition and comments never overwrite it.
The existing single task text field becomes the description, which can be empty
when a named task is created. Older text-only tasks keep their complete text and
derive a display title until explicitly renamed. Names never establish identity.

The central Task View uses the familiar full input for description content,
attachments, reviewed speech and later comments. Its destination is explicit:
**Description** on a new card's Description tab until the first description is saved, then **Comment**. History selects comment input. Description
editing selects that destination explicitly. Retain unsent input privately and
durably across navigation/restart, including a failed new-name submission. Voice
capture/recognition must not submit native input or start an executor.

Only an unconfirmed new name and unsent content are drafts. Once the name is
confirmed, the real card remains in the shared board even if it has no description
yet. Preserve selection and the same card UUID through content edits and rename.
Reserve an idempotent creation identity before transport, so duplicate clicks or
an uncertain reply cannot create another card. Stage and executor selection stay
separate from title/content entry; only explicit Start dispatches work.

Proposed submission flow:

| Task state | Default input destination | Executor delivery |
| --- | --- | --- |
| New name | Enter creates a title-only task and opens Task View | Only explicit Start creates/dispatches an executor. |
| Backlog / To do | Durable task comment | Included with the next explicit dispatch. |
| In progress | Durable task comment | Also deliver to the verified current worker; show that destination before submission. |
| In review / Done | Durable task comment | Requesting more work uses an explicit reopen-and-send action. |

A comment's history identity and delivery receipt remain distinct. Show saved,
queued, submitted or delivery-unconfirmed evidence without claiming model
consumption. A failed or uncertain delivery keeps the comment and never permits
a hidden retry or fallback to another executor.

Put compact stage actions in the task context/input toolbar: Back to do, Accept,
Start, and a send icon for comments. Keep text for less obvious stage transitions;
use existing familiar attachment, microphone and send icons. Place Jump to latest
in that toolbar, following the existing desktop Activity pattern. Avoid a floating
control bundle that obscures the journal or another full composer in the inspector.
The task view solves the space/input duplication; distributed execution tracking,
real task Pause and verified artifacts still require the open execution work.

## Verified starting point

The inspected task implementation is the PR348 tree, merged as main
`68c8fde40a840199ff6bf3bb1f744c4b1c80515e` and published in Dev.1353.
Later main changes inspected during this design do not change the task model.

* `crates/replica/src/task.rs::record_send` sets Active after native submission.
  It records a request receipt, not proof that the agent consumed or started
  this particular task. `record_report` moves an Active task to Review; it has
  no execution generation or task revision against which to validate readiness.
* `src/tasks.rs::send` submits native input and then saves the task. These are
  separate durable operations, not one transaction with the native agent.
* `tray/src/TaskDialogs.cpp` builds a 640-by-640 modal form. Its comment input
  explicitly says that comments are not sent to the session. The history is a
  separate text browser, not the Activity timeline.
* `SessionsWindow.cpp` puts project Tasks in a main tab. The inspector renders
  native plan lists and subagents independently, and its default width is 290.
  A working-session badge is not attribution of that work to a specific task.
* Desktop/CLI session links already retain immutable identity through PR348.
  `SessionKey` uses machine plus retained launch/run identity; names are labels.
  However, `replica-ffi::SessionRef` still drops that identity. This is an
  incomplete cross-client contract, not evidence that every current link is
  still name-based. Mobile presentation is already tracked in `zerus-oqh7`.
* `ProjectStore::tasks` reads all project task documents, and the CLI listing
  is not a bounded page API. Paged replication does not make local listing or
  task history bounded. Repeated media-history qualification is `zerus-w0xh`.

These gaps require a common execution model. A larger dialog, more agent
instructions, or changing a badge whenever a session becomes busy will not
provide reliable task tracking.

## Separate the objects

| Object | Meaning |
| --- | --- |
| Task | Stable task UUID, current requirement revision, description, expected outcome, routing preference, responsible human/coordinator and conversation thread. |
| Execution | One attempt at a specific task revision: execution UUID, assignment generation, executor reference, role, delivery/acceptance/start evidence, runtime condition and outcome. |
| Executor | A verified session, native subagent, or separate child session. Parent/coordinator and actual worker are distinct references. |
| Event | An immutable identified fact: created, queued, submitted, accepted, began, yielded, comment added, transferred, result proposed, verified, reopened or accepted. |
| Artifact | A versioned result: document, diff, commit, PR, check run, build, publication or installation receipt. It includes its source and verification state. |
| Queue | An explicit ordered list for an executor or explicitly configured worker pool. Project backlog priority and executor queue position are different fields. |

A task may have several historical executions and several explicit concurrent
participants. Show **responsible coordinator**, **currently executing**, and
**last executor** separately. Do not display the originally requested target
as if it were still the worker.

Native plans remain plans. Link a plan or step to a durable task when identity
is known, but do not automatically create project tasks from every checklist
item or use a native plan's completion percentage as project-task completion.

## Identity and attribution

Task and executor links use immutable IDs throughout CLI, desktop, FFI, Android,
iOS, replication and history. A machine name, SSH alias, session caption or
subagent nickname is never an ownership key.

Retain existing immutable session identities when migrating; do not invent a
new identity from a current name. An execution additionally pins the exact native
conversation, native child identity when applicable, run/generation and originating
request. A native child ID is scoped by its provider and parent conversation.
Renaming changes a caption. Clearing or replacing a conversation creates a new
execution context; it cannot silently continue an old attempt's authority.

Every task-associated native input carries a durable task/execution context.
Progress events link to exact native event identities, not matching text or
timestamps. A tool call affecting multiple tasks can have several explicit task
links. Do not tag all events of a busy session with whichever task is selected
in the GUI.

A child's inherited parent environment is not proof that the child is the
parent, nor that it owns every parent task. Bind native child identity through
the adapter or a scoped delegation capability. Where the provider cannot prove
the child identity, show the confirmed coordinator and unconfirmed worker;
do not manufacture a named subagent.

## Lifecycle and what the user sees

Keep the project workflow small. Backlog, Queued, In progress, Review, Done and
Cancelled describe the task. Show execution details such as Running, Waiting
for input, Waiting for CI, Paused or Unavailable alongside that workflow.
They must not become a single enum with a state for every combination.

| Evidence or action | Visible consequence |
| --- | --- |
| Filed for later | Backlog or Queued, with creator, routing and reason it has not started. |
| Message submitted | Queued / Delivered, awaiting acceptance; not Running. |
| Executor acknowledges | Accepted, with a confirmed executor and queue position. |
| Exact task execution begins | In progress / Running. |
| Confirmed question or dependency | In progress / Waiting for the specific person, child, CI run or permission. |
| Execution explicitly yields | Show why it yielded and whether its foreground slot is free. |
| Normal turn ends without a result | Awaiting agent result; do not manufacture completion. |
| Executor disappears or observation is stale | Unavailable / last confirmed state; do not assign it elsewhere automatically. |
| Progress report arrives | Append progress and update evidence; keep working state. |
| Structured ready result passes its gates | Review, with the result for the exact task revision. |
| Person accepts that result | Done. |
| Implementation resumes or requirements change | Invalidate readiness of the old revision; Queued or In progress according to the actual new execution. |
| Cancellation requested | Cancelling until confirmed; then Cancelled, with partial result and history. |

One deterministic reducer owns these transitions. UI clients and agent commands
submit events; they do not independently guess a stage from session activity.
Store enough causal/revision identity to reject late events from old executions.

Show the observation source and freshness of runtime details. Losing the GUI's
connection changes what is currently confirmed; it does not by itself rewrite
the task's durable workflow or make a formerly running worker safely available.

The session's native Working, Background, Needs attention and Scheduled rules
remain intact. A native Stop ends a turn, not necessarily its owned background
jobs. Neither a Stop, an idle session, an elapsed timeout nor an optimistic
agent sentence proves task completion. User Stop must not trigger automatic
resubmission, queued dispatch or recovery until an explicit resume.

### Automation without pretending to know the answer

The execution controller creates the task context before dispatch and observes
delivery, verified consumption/start and native work for that context. Agent
hooks/commands add semantic progress and results. A task already being worked
on must be created and begun in one idempotent operation; do not first leave it
in To do and depend on a later remembered command.

Creation must state intent: current work, queued work, or proposed follow-up.
An agent-created follow-up legitimately stays queued if it has not started.
Show that explicitly; do not run all automatically created tasks merely because
an agent filed them. System-created tasks retain a controller/coordinator and
their originating request even after the creating agent exits.

The controller runs on the owning computer independently of whether an inspector
is open. UI polling cannot be the execution engine. Recover pending operations
from durable receipts on restart, preserving existing native processes and
explicit Stop/cancel decisions.

Provider capabilities differ. Start can be automatic only with a proven native
request-to-execution link. Otherwise require the scoped execution acknowledgement
and label the state honestly until it exists. A reconciliation worker repairs
projections from durable facts and flags missing lifecycle events; it must not
infer them from prose. A normal completed turn can request a missing structured
result under a bounded continuation policy, but Stop, cancellation and uncertain
delivery never authorize such a continuation or a replay.

### Review is a deliverable state

`report` currently conflates a progress update, an inability to continue and
readiness for review. Separate those outcomes. Readiness requires the current
revision, a result, the required evidence, and no unresolved implementation
work in that execution tree. A deliberately separate reviewer can work while
the task is in Review; new implementation work invalidates the ready result.

Discussing a result does not always mean reopening implementation. The composer
must distinguish **Discuss result** from **Continue/change the task**. The latter
records a revision/reopen event and resumes or queues work automatically. Do not
classify free text such as a thank-you as a request to reopen.

An intentionally delivered long-lived job can itself be a result when its
ownership and health are explicitly handed over. Do not require every background
process to exit before any task can finish; require that outstanding work has
an explicit disposition consistent with the task's expected result.

The recommended default keeps final acceptance with the person. Automatic
acceptance, if wanted for a bounded class of internal tasks, is an explicit
project policy with declared checks; it is not the meaning of an agent's Stop.

## Activity, inspector and comments

The inspector has three compact sections:

1. **Current work:** task title, workflow/runtime condition, actual worker and
   role, responsible coordinator, current action and the next expected event.
2. **Next:** a numbered executor queue, with reorder and explicit routing actions.
3. **Result:** expected outcome, achieved milestones and links to artifacts/history.

Use normal workspace typography, content scaling and standard icon/action sizes.
Make the inspector resizable, initially around 420-480 logical pixels where
space permits, and collapsible. On narrow windows use an expandable drawer or
full-width task detail view. A full-width project overview remains available.
Do not compress the normal Activity conversation to fit every task control.
Persist inspector tabs by stable keys, migrating existing numeric preferences;
polling must not switch the user's tab or steal focus.

Selecting **Task Activity** uses the central renderer, with a clear breadcrumb
and a return to the complete session. It can aggregate linked executions across
the main session, children and handoffs, showing each source. Original session
messages remain where they were; task filtering must not silently move them.
Retain native order within each execution and causal links between assignments,
comments and results. Wall-clock timestamps across computers do not establish
one authoritative global execution order.

Use the existing Activity composer for text, multiline editing, attachments,
images, voice, previews, drafts and supported controls. Add a visible context
chip: **Task T-a1b2c3 -> current executor**. Provide **Add to task** on an existing
Activity message or selection, retaining the original event reference and the
explicitly selected content. Never silently attach an entire conversation.

The normal task message action is **Clarify task**: persist the comment/revision
and durably deliver it to the task's current executor. The separate **Save note**
action records a note without delivery. History shows Pending delivery,
Submitted and Acknowledged/considered distinctly; native submission alone must
not be labelled read or considered.

An actionable clarification advances the input revision. The result declares
which input revision it considered. An unacknowledged actionable revision blocks
readiness, or reopens a previously ready result. A passive note does neither.
When no executor is available, retain the clarification with the task and
include it in the next assignment package. Uncertain delivery stays visible
and cannot be retried as a new uncorrelated message.

Drafts are bound to task, recipient/mode and attachment identity. Changing
selection or receiving a handoff must not redirect a half-written draft. Before
Send, resolve and pin the displayed exact recipient/generation; if it changed,
refresh the recipient without losing the draft. Give the person a visible
choice of where a queued clarification will go.

## Explicit queues and parallel work

Default: one foreground implementation execution per executor. Six tasks
assigned to a session appear as **one current, five next**, not six Running
cards. Zerus holds the task queue and dispatches its next item when permitted;
it must not dump all task prompts into the native input queue and call that a
serial task scheduler. Ordinary session messages remain available as messages.

The project backlog answers what is important. An executor queue answers what
this worker will do next. Show both orders explicitly and distinguish tasks
accepted by a worker from merely preferred routing.

A worker normally releases its slot when its implementation is ready for
review. Waiting for the person to accept Done should not freeze the next task.
Advancing while a previous task waits for CI or other external work requires
an explicit yield and queue policy. Show **T-a1b2c3 waiting for CI; T-d4e5f6
running**, not that the agent is actively doing both. Unknown live jobs cannot
silently release a slot or be declared finished.

Parallel work is explicit: separate sessions or confirmed children own separate
executions and queues. A coordinator may supervise several while running only
one foreground implementation itself. Show the execution tree and distinguish
the coordinator, implementer, reviewer and verifier.

Reordering changes tasks that have not started. **Do now** is an explicit
preemption request, not a side effect of dragging a card above the current one.
Hold dispatch while the current worker needs native permission, has unknown
input state, is paused/stopped, or has an unresolved transfer. Requirement
changes attached to the current task can continue through Activity without
becoming an unrelated new item in its task queue.

If task A is reopened while its executor is implementing task B, queue A and
show that position. Do not inject A into B's context and silently call both
Running. The explicit preemption action can request a safe switch; it does not
prove that B's native work or background jobs have stopped.

Do not assume that parallel sessions have isolated files. Default to one writer
per checkout, or use a separately chosen worktree for independent writing.
Role captions describe work; they do not create filesystem permissions. Existing
permission, new-session and worktree-creation rules still apply.

## Routing, delegation and the orchestration boundary

The routing action offers:

* This session's queue.
* Another existing session's queue.
* A new session, created through the normal explicit Start flow.
* A confirmed native child of a selected parent, when supported.

Optional roles are implementer, investigator, reviewer and verifier, with
documented instructions and capability requirements. Display the selected role
and exact worker. Do not pretend a provider can create or address independent
children when its adapter cannot prove that capability.

Keep requested routing separate from actual execution. A handoff is a durable
offer, acknowledgement and ownership transition with a new assignment generation.
Until acceptance, show the pending destination and retained responsibility;
do not label a submitted prompt as a confirmed new worker. Preserve the old
attempt and all later replies as history. Stale attempts cannot move the current
task to Review. Never retry an uncertain native side effect automatically.

Reuse the existing scoped acceptance/result protocol in
`docs/agent-coordination.md` for independent-agent waits; do not invent textual
acknowledgement from a comment or an idle producer. Native child lifecycle links
need their own verified adapter identity, rather than being faked as SSH sessions.

This is orchestration in the narrow sense of dispatch, queueing, delegation and
tracking. Zerus owns deterministic coordination and evidence. Agents own the
reasoning and implementation. The person owns priorities, permissions and
acceptance. The first version does not need an invisible LLM supervisor,
unbounded agent spawning, automatic model selection or a general dependency
graph scheduler. An optional existing-worker pool can follow after exact
ownership and queues are reliable.

## Results and delivery are first-class

Declare what counts as the result when work is assigned, with a project default
that can be refined. Suggested outcome types are **Proposal**, **Local change**,
**Commit**, **PR**, **Main**, **Published build**, and **Installed build**. These
are result requirements, not seven extra project board columns. A proposal can
finish with a document; a feature intended for the installed app cannot be
described as published merely because its source was edited.

| Milestone | Evidence shown to the person |
| --- | --- |
| Proposal / local work | Versioned document or retained diff and a readable summary. |
| Committed | Repository identity and exact commit, with task association. |
| Pushed / PR opened | Verified remote/ref or PR URL and exact head. |
| Merged | Target branch and verified merge/inclusion identity. |
| Checks passed | The checks for the exact source and selected required coverage, not a different green run. |
| Published | Version, candidate/run, artifact hash and public catalog/source match. |
| Installed | Target computer and verified installed/running build identity. |

Each milestone distinguishes **reported by agent** from **verified by Zerus**.
Show missing requirements plainly: for example, **Changes ready; not committed**
or **Merged; build running**. Evidence can support several tasks in one commit
or release; do not claim that the entire shared worktree belongs to one task.
Validation can establish that a commit or build exists, not that every product
requirement is semantically correct. Final acceptance remains meaningful.

Publication or installation requirements do not themselves grant permission to
push, merge, change a service or replace a live application. When such a step
needs the person's decision, the task waits for that decision with the prepared
result, rather than pretending the whole requirement is ready for review.

A task's result page contains its latest deliverable, evidence, checks, remaining
work and earlier results. Retain review/reopen cycles and the revisions they
addressed. The previous task-media incident should read visibly as local source,
request to deliver, merged, build verified and published; it must not look like
one unqualified completion comment.

The result envelope pins the task/input revision, execution UUID, assignment
generation, considered clarification revision, summary, artifacts, check evidence
and outstanding work. A progress event does not need a ready envelope. A ready
envelope cannot override a newer requirement or a later accepted execution.

## Storage, reconciliation and compatibility

Keep Loro ownership inside `crates/replica`. Evolve the task schema/capabilities
for execution, revision and result semantics; readers that cannot preserve these
semantics cannot write or dispatch the affected tasks. Extend the CLI/desktop
and generated mobile contracts together. Preserve existing command names,
durable request identities, API v1 and connector journal behavior.

Keep existing `start` and `handoff` entry points. Proposed additions distinguish
progress from readiness, attach structured result evidence, and bind delegation
to an exact child. For example, `report --progress`, `report --ready
--result-json`, and `delegate --child ID --role reviewer`. These are API design
sketches, not commands implemented by this proposal. Existing `--json` remains
the output-format flag. New behavior must not silently reinterpret old clients'
commands as proof of a different execution or result.

Task comments, results and selected shared artifacts belong to the task.
Native Activity journals remain authoritative for native execution events;
store stable references and bounded summaries, not full duplicated transcripts
in a replicated task document. Offline or unauthorized history links say that
the source is unavailable. Sharing a task does not share private conversation
history, credentials or native control authority.

Large media needs immutable content-addressed storage, deduplication and a
retention/collection contract compatible with offline replicas. Do not repeatedly
append base64 copies of entire attachment sets to an ever-growing operation
history when adding a rich task conversation.

Use indexed materialized projections for task headers, executor queues, events
and artifacts, while the replicated documents/journals remain authoritative.
All list/history queries need bounded pages and stable cursors as new records
arrive. Load attachment bytes only on demand, outside the GUI thread. Do not
load every project's complete task documents on a sidebar poll or grow a cache
for every session indefinitely.

Loro convergence alone does not guarantee exclusive execution. Assignments need
an authoritative durable dispatcher for the queue, compare-and-set generation
checks and confirmed transfers. Across offline computers, allow comments and
planning, but refuse a new conflicting execution claim that cannot contact the
current authority. Do not use last-writer-wins ownership, heartbeat expiry or
textual claims as permission to run a second worker. Native agents can perform
external effects outside task storage; exactly-once external work under network
partitions cannot be promised by a task CRDT. Recovery must expose uncertain
work and preserve both attempts rather than replaying it.

Recommended initial authority: each task names its coordination computer at
creation, normally the selected project's computer; each executor queue is
owned by its executor's computer. Moving coordination authority is a confirmed
transfer, not a replicated last-writer-wins field. A newly filed offline task
can be planned and edited while its configured authority is unreachable, but
cannot acquire a competing executor through another copy. This deliberately
trades automatic failover for reliable ownership. An always-available shared
coordinator is an optional later deployment choice, not a prerequisite for
single-computer work and not something the CRDT supplies for free.

## Migration and delivery order

1. **Execution truth:** shared execution/revision/event model, exact identities
   through every typed client, progress versus ready results, native context
   binding, stale-event rejection and reconciliation. Prototype the central
   Activity task context and inspector against these same contracts.
2. **The usable vertical slice:** a real serial session queue and controller,
   inspector current work/next/result, task-aware Activity/composer, delivered
   clarifications, durable media and paged history, and result milestones.
   Include bounded read/index APIs and native Stop qualification in this slice.
3. **Parallel execution:** explicit yield, native child delegation, accepted
   cross-session handoff and multiple executor queues. Native adapters qualify
   their actual task attribution before these modes are exposed for them.
4. **Optional policies:** existing-worker pools, narrowly defined automatic
   acceptance and dependency scheduling only if separately wanted.

## Worked example

A person queues two tasks for one session: A should reach a published Dev build,
and B should produce a proposal. A is accepted and begins; B remains visibly
second in that executor's queue.

The parent delegates implementation of A to a confirmed child. The inspector
shows the parent as coordinator and the child as implementer, with links to both
in Task Activity. A pasted screenshot is saved as an actionable clarification
at revision two and delivered to the child. A result considering only revision
one cannot make A ready for review.

The child reports a retained diff. A shows **Local changes ready; publication
still required**, with its actual worker/coordinator and any pending decision.
It does not claim to be published or installed. If the person authorizes the
next delivery steps, commit, merge, exact-source checks and catalog verification
add distinct milestones. An explicit yield can let the coordinator begin B
while A is waiting for CI, under the queue's declared policy.

Only the verified Dev result for the current revision makes A ready for
acceptance. A request to change implementation opens a new revision and resumes
or queues A; a discussion of that result stays a review discussion. The old
result and every handoff remain in history. Accepting A records Done without
discarding the earlier local-only result.

Do not infer the actual current worker of six legacy Active tasks solely from
their stage or timestamps. Preserve submitted receipts and reports; display
**Assigned, execution not confirmed** where current identity/work evidence is
missing. Keep old history without inventing start times, child identities or
verified artifacts. Migrate old name-only links only from unambiguous verified
identity evidence, never by assigning a task to whoever reused its old name.

## Acceptance scenarios

The redesigned system must demonstrate these behaviors with isolated real-agent
contract coverage where native behavior is involved. Each delivery slice must
qualify its applicable scenarios before exposing the corresponding mode:

* Six tasks queued to one session show one current and five ordered next tasks.
  Two independent sessions or confirmed children can run tasks concurrently.
* Work created during an active task context appears In progress without a
  remembered follow-up command; a deliberately filed future task stays queued.
* A progress report cannot move active implementation to Review. An exact ready
  result can. A stale executor or old input revision cannot do so.
* Continuing implementation from task Activity reopens/starts the right task;
  discussing a frozen result does not silently alter it.
* A clarification with an image survives restart, is delivered to the confirmed
  worker, and is either acknowledged at the result revision or visibly pending.
* Renaming a session or child changes captions without changing any task link.
  Clearing, replacing or reusing a name does not inherit an old execution.
* Accepted handoff shows the new worker and preserves the previous attempt;
  an uncertain send is not retried and does not falsely complete the transfer.
* Parent and child progress are attributed separately; a finished child turn
  with confirmed background jobs cannot complete the task.
* Stop, cancellation, crashes, delayed acknowledgements, duplicate receipts and
  offline conflicting assignments retain correct states without automatic replay.
* A task requiring a published build shows local, commit, merge, build and
  publication evidence separately, including the actual public artifact identity.
* Large projects and long task histories remain paged and responsive; offline
  task history does not require copying private native journals into the task.
* Task selection, reordering, remote updates, handoff and font scaling preserve
  drafts, attachments, focus, readable layouts and stable pagination.

The proposal succeeds when the person can answer four questions from one work
surface: **what is happening, who is doing it, what happens next, and where the
result is**. A cosmetically updated board with the old lifecycle does not meet
that criterion.

## Task editing interaction review, 2026-10-26

Reviewed the branch at `84898fc1` against the Activity-centered workflow after
owner feedback on missing reordering, the ambiguous lower-pane ellipsis and task
context menus. The old `TaskDialog` is a combined task console, not a focused
editor: it mixes description/attachment changes, routing, reports, history,
immediate notes and workflow actions. It cannot be the main work surface.

| Finding | Consequence | Disposition |
| --- | --- | --- |
| `TaskInspector` connected card clicks and actions but not `TaskRow::dragStarted`; it had no drop targets. | The sidebar looked like the board but lost queue ordering and session/stage drops. | Restore both queue surfaces through shared drag lifetime and guarded mutations, insertion feedback and edge scrolling. Backlog ordering is project priority; neither it nor To do implies parallel execution. |
| The lower ellipsis opened `TaskDialog` even when the task was read-only. | An apparent editing action opened a window in which description and routing could not change. | Use an explicit pencil plus Edit label for editable tasks; use Details for other stages so notes and inspection remain accessible. Explain the backend edit restriction. |
| The dialog's Session field chose between target and recorded worker. | Routing preference appeared to be the actual executor. | Show Assigned to separately from Executor / Last executor; a send without a start remains unconfirmed. These fields do not implement an active handoff. |
| The Review report was an unbounded wrapping label. | A long result could force the dialog beyond the screen and obscure controls. | Use a bounded Activity report view with Markdown, author and known timestamp. A report remains an agent statement, not verified delivery evidence. |
| The note control was a single line, persisted independently, and never delivered to the session. Duplicate submits were possible, while a refusal used the board notice instead of the open dialog. | A clarification could fail silently or be saved twice, and a saved note could be mistaken for communication with the worker. | Label Save note explicitly, reserve one pending UI submission, retain the draft and show refusal locally. Return saves only the note and does not trigger the dialog default Save/Close. Task-linked delivery and rich drafts remain `zerus-cg5h.3`. |
| Workflow buttons applied unsaved edits before changing task state. The modal also duplicated result/history and had no durable discussion draft. | Editing, workflow and discussion had different commit boundaries inside one form; closing it could discard an unsent discussion draft. | Make the explicit editor contain only description, attachments and next-send routing with Save/Cancel. Keep workflow actions in card menus, read results/history in the inspector and discuss through Activity. This separation is `zerus-cg5h.9`, after `zerus-cg5h.3`. |
| `TaskDoc::edit_text`, attachment edits and routing require Backlog or To do; rank changes also require these stages. | Enabling an active-task editor or dragging running tasks would misrepresent the current execution model. | Keep these guards. Revision-scoped edits, executor queues, handoff and verified artifacts belong to `zerus-cg5h.4`. Legacy full-journal reads remain `zerus-cg5h.2`. |

The target interaction is: select to inspect; drag to prioritize queued work;
right-click for explicit stage actions; Edit for requirement/routing changes;
Activity for discussion with the current executor. Saving a task note is not
message delivery, and changing routing is not changing recorded execution
ownership. The current UI slice improves these boundaries without claiming the
remaining execution/composer model is implemented.


### Owner follow-up: one fixed task work surface, 2026-10-26

The owner superseded the Edit/Details text and duplicate-modal dispositions
above: use one stable icon-only Task details action and put the form directly
inside Description in the fixed lower pane. Description shows the assignment
and its change control; description, attachments and routing use the shared
form and explicit Save/Cancel. History and result remain Activity-rendered tabs.
Task workflow commands stay in a fixed bottom bar on every tab. Task notes are
saved to history with a local durable draft and an explicit non-delivery label;
task-linked rich Activity communication remains `zerus-cg5h.3`.

Every writable stage supports persisted board ordering, not just the two queues.
This supersedes the old rank guard while preserving description/attachment/target
edit guards. Reordering active work is a presentation change, never preemption,
stop, resume or an implicit executor queue mutation. Invalid cross-stage anchors,
foreign projects and future-schema writes remain refused.

Compact cards expose their stage's primary action, with other actions in the
context menu. The owner explicitly chose task-scoped pause and execution
accounting first. A whole-session interrupt cannot implement this action.
Task pause therefore remains unavailable in this UI slice until
`zerus-cg5h.4` provides the execution identity and controller.

Selection updates must be atomic from the person's perspective: retain the
previous document while fetching the exact new task, disable commands during
the read and keep permanent command widgets and pane geometry. Do not clear the
pane or rebuild its buttons on each card click.

### Central Task View implementation boundary (2026-10-26)

The branch now has a title field (task schema 4), separately audited Rename,
caller-reserved idempotent creation UUIDs, and title-plus-description worker
prompts. Current mobile ABI readers remain read-only for schemas above 2 and
show named cards through their existing text field. The right board creates a
shared named card and opens one reusable central Task View; ordinary Activity
keeps its own draft and selection. Task View uses MessageComposer for description
and comments, including media, previews and draft speech. Workflow controls share
the bottom attachment/send row, and latest navigation uses the toolbar icon.

Draft speech uses verified speech-computer routing without a native session send.
Recording is saved before transcription; job status reads never replay Submit.
Completed text is checkpointed before insertion, bound to the original task input
key and unchanged draft revision. An edited draft requires explicit insertion.
Insertion UUID receipts prevent repeated transcript insertion after an uncertain
local clear. Recordings remain recoverable across task navigation.

Task comments still persist in task history only. Verified delivery to an active
executor, execution-scoped Activity attribution, revision guards, distributed
claims and true per-task Pause remain the execution workstream; this UI iteration
must not imply that those contracts have shipped.


## Task View UI/UX review after be54a544 (2026-10-26)

Status: approved by the owner; implemented in the next branch iteration below. The owner supplied two screenshots
of the live Mac preview: the oversized detached-looking composer and the dense
header with a disabled Run in field and a description rendered as a user message.
This review supersedes the automatic Description/Comment destination switch in
the first central-view iteration. A shared widget alone does not establish the
same input presentation or the same interaction contract as Activity.

### Verified behavior and defects

* The Task View uses MessageComposer directly in a zero-inset detail layout;
  Activity places its input in ActivityWidth::ColumnLayout. Task-specific rules
  style every descendant QPushButton, and the composer root is renamed while
  its transparent-root skin still selects QWidget#messageComposer. These are
  concrete styling/composition differences; the screenshot's entire input panel
  cannot be treated as a correctly reused Activity input. Empty-height behavior
  also needs comparison at the same window size, scale and saved resize setting.
* Rename, ID, stage and a long title compete in one row. The next rows introduce
  three content tabs, an executor link, a description-edit glyph, Last executor
  text and a full-width Run in pseudo-field. The hierarchy does not make the
  selected task, its current worker or the primary action immediately legible.
* Run in is a QPushButton that opens an assignment menu. It is disabled outside
  To do/Backlog, yet still resembles an available dropdown in a review task.
  Planned assignment and actual/last execution are displayed simultaneously.
* Description is rendered through the person-message TaskEntry adapter, including
  You, a timestamp and a green message background. It reads as a chat post. For
  legacy tasks the header derives its title from the first description line, so
  a one-line task repeats the same text as both heading and supposed message.
* A successful comment explicitly selects History. The default task page does
  not show a continuing discussion, so the person loses the place they wrote in.
* saveTaskNote invokes hgs task comment, which appends a comment record only.
  It has no native delivery or scheduled outbox. task_prompt renders title and
  description; it does not include comment bodies. Only a previously sent task
  gets an instruction to read history. A saved pre-start comment is therefore
  not guaranteed to reach the agent even at first dispatch.
* The input destination changes according to empty description and selected tab.
  A small Comment label and a generic send arrow do not explain that first input
  edits a document, later input saves a journal entry, and neither means delivery
  to an executor. State-transition buttons placed beside that arrow further
  compete with its meaning.

### Approved information architecture

The right board owns task selection, ordering and filters. One central Task View
owns the selected task's document, discussion, result and input. Remove the
secondary Description/Result/History tab barrier from the normal conversation
flow: the default task view must contain the description and the discussion.
Keep result and audit access as contextual navigation rather than destinations
where comments disappear.

1. A task heading may wrap naturally, with Rename in its context menu. A quieter
   metadata row holds the task ID, a real stage badge and a compact executor chip.
   The chip states Planned executor, Working in, or Last executor according to
   actual evidence. Assignment opens a focused picker, and navigation to the
   worker's Activity has its own unambiguous affordance. Session names are labels;
   mutations and delivery retain immutable execution/session identity.
2. The canonical Description is a document: a clear heading, ordinary Markdown,
   attachment previews and an Edit control. It has no You bubble or chat timestamp.
   New cards enter an explicit Edit description state. Save/Cancel have document
   meanings; navigating a tab must not silently repurpose the field. Later comments
   never rewrite the description automatically.
3. Discussion sits directly below the document and uses Activity message styling.
   A newly submitted comment appears here immediately with its persistence and
   delivery evidence. Agent replies appear only with verified task attribution;
   neighboring session events cannot be guessed into the task thread.
4. An available Result is a distinct, reachable block with reported changes and
   actual commit/build/deployment artifacts when known. Review tasks foreground
   that result and offer Accept or Request changes. Service history is secondary:
   stage changes, assignment, handoff and receipts remain accessible in an audit
   surface without being the only home for conversation.
5. Input uses Activity's complete layout/width/skin, spacing, resize behavior,
   attachments, voice preview and send affordance. Task metadata does not apply
   broad styles to that subtree. Lifecycle actions occupy a separate task action
   group, not an indistinguishable extension of the message field. The send
   destination and its effect are readable before pressing Enter.

### Approved message and document contract

| Intent / task state | Person-facing action | Durable effect and executor behavior |
| --- | --- | --- |
| Create card | Confirm title | Create one shared card and open explicit description editing; do not launch work. |
| Edit description | Save description | Save the canonical document; display the changed version in the same task. Active revision delivery needs its own guarded execution contract. |
| Message in To do/Backlog | Add to task, to be sent on Start | Save discussion and include its bounded original identities in the next explicit dispatch snapshot. |
| Message in progress | Send to the displayed worker | Persist the task message and reserve its delivery identity, then submit to the verified current execution. Keep failures/uncertain delivery visible; no fallback to another session. |
| Feedback in review | Request changes and send | Save feedback, reopen through a deliberate action, and record new work only with the appropriate delivery/start evidence. An optional note-only mode leaves the review status intact. |
| Message on Done | Reopen and send, or Save note | Make the distinction explicit. A thank-you or internal note must not silently start another execution. |

A note-only action is available intentionally. Normal task conversation must not
silently behave as an internal note. After submission show Saved to task, Waiting
for Start, Submitted to worker, or Delivery unconfirmed as warranted. Native
submission does not establish that a model read or incorporated the message.

Delivery is a backend requirement, not a visual label change. Use a durable task
message identity and guarded outbox, capture the exact target execution, and keep
its receipt visible in both task discussion and session Activity. Handoff,
renames, offline targets and unknown native replies must not authorize replay.
Do not reuse hgs task send for active-task comments or infer ownership from a
last-session name. The comment CLI remains note-only; the distinct `task message` operation below
establishes the discussion/delivery contract.

### Acceptance criteria for this iteration

At the same width, theme, content scale and user resize setting, Activity and
Task View inputs should share alignment, typography and controls. A task should
show its document, stage and actual/planned executor without competing full-width
routing fields. Sending must keep the new message visible in the same discussion
and explain where it went. A first dispatch must include queued clarifications;
active delivery and review feedback must retain exact identities and honest
receipts. Saving a note must never look like confirmed agent delivery. Review the
new document and delivery contract together before another cosmetic-only preview.

### Implemented follow-up

The next branch iteration puts the canonical description, reported result and
recent discussion on the same central page. Description uses document Markdown,
without a user bubble or timestamp, with explicit Edit and Save/Cancel. Task
navigation never silently changes the input destination. A task menu owns Rename,
Result and History. Header metadata uses a compact executor chip; lifecycle
controls stay outside message submission. The composer uses ActivityWidth and
its component skin, including a shared toolbar chip for delivery intent. The
side-panel toggle uses a padded corner host and neutral checked styling.

Discussion uses a distinct schema-five `task message` operation. Queued context
and message-scoped media are included once in the next explicit dispatch; image
labels are flattened without changing the stored document or messages. Notes
remain note-only. Active feedback reserves an immutable owner/run/conversation
and input UUID before guarded submission. Reopen moves Review/Done to Active only
after a confirmed submission. A coordinator receipt never proves child delivery
or model consumption. Native dispatch receipts refuse a changed run/conversation;
legacy direct-work acknowledgements retain an owner and explicit start anchor.
This does not implement distributed leases, revision scheduling, stale-report
protection or per-task Pause (`zerus-cg5h.4`).

An interrupted desktop request retains its original identity and payload in one
private file per task, with only a small marker in settings. Content I/O runs on
a worker. Check uses `--recover-only`: even a missing reservation cannot authorize
an initial send. A reserved request is never automatically replayed. Releasing a
local recovery record requires explicit confirmation and neither cancels nor
resends native input. Existing full-journal CLI reads remain `zerus-cg5h.2`;
Task View retains only its bounded selected-task window.
