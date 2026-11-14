# Agent Teams on Zerus tasks

Draft for owner review, 2026-11-11. Engineering epic: `zerus-63rru`.

## Origin and credit

Agent Teams were designed and first built by WhiteWh in the `WhiteWh/zerus`
fork. Pull request #322 is its first version; the fork's `whitewh/main` at
`ca4cc4481` is the reference this design reads from. This specification keeps
the concepts, the working process and the desktop views of that work, and
builds the runtime on Zerus tasks and the desktop service. Imported files keep
WhiteWh as their Git author; the rules are in
[Authorship](#authorship-and-credit).

## Outcome

A person describes a goal. A lead agent turns it into a plan: phases of tasks
with dependencies, each task for a member with a role. The person approves the
plan. Zerus launches the member sessions, hands every ready task to a free
member, brings reports back to the lead and merges accepted work into one
integration branch. The person is asked only for what they kept for
themselves.

A team can also be standing. The person creates it once from a template and
keeps sending it tasks; each task runs through the team's pipeline, for
example implement, review, accept, with no plan drawn up first.

The first milestone is done when all of this holds on one computer:

- a team created from a template, with a lead, two developers and a reviewer,
  takes twenty tasks sent to it one at a time and finishes each without the
  person, apart from items the person reserved;
- no task prompt or message is delivered twice, including across a restart of
  the desktop service;
- every decision has a journal entry naming who made it;
- a session that is in no team behaves exactly as it does today.

## Owner decisions

Recorded 2026-11-11.

| # | Decision |
|---|---|
| D1 | Teams come into upstream. For teams, phases and the task graph this replaces the "Orchestration" row of the owner constraints in `2026-10-27-tauri-web-ui-design.md` |
| D2 | The main points are taken as they are in the fork now. Work then continues on Zerus's own code, and later contributions by the author are made against that code |
| D3 | The author's credit is kept |
| D4 | Teams are built on Zerus tasks |
| D5 | The lead agent may accept results, answer members' permission requests, restart members and merge accepted work into the integration branch |
| D6 | Work happens on a branch. Subagents implement it under one coordinating session |
| D7 | Credit covers the imported base. Work after the import is Zerus's own and carries no co-author trailer |
| D8 | Team templates, and teams that keep taking tasks through a pipeline, are designed in from the start |
| D9 | The lead confirms whatever its member sessions need to keep working: folder trust, permission requests, questions, continuing after an interruption. Prompts of a fixed kind are confirmed at once under that delegation |
| D10 | The role catalog and the visual designer are part of Teams, made for Zerus |
| D11 | The queue team comes first and planning from a goal second |
| D12 | Until Teams are finished they sit behind a setting on each computer, off by default |
| D13 | A team is shown only on its home computer until viewing from other computers is built |
| D14 | A role names its agent and either one account or "the freest account of that agent", in the designer and in the New team form |

The same row of that specification also excludes iron rules and cache hold.
They were not discussed and stay excluded.

## What is carried over

These points are taken as the fork designed them.

- A team is a lead and members with roles. Each member has its own agent,
  account and computer.
- The lead plans alone before any member exists, proposes the members, and
  the person creates the team as a separate step (see Life cycle).
- A plan is phases of tasks. A task has hard or soft dependencies, an assignee
  or a role, a write set (the files it may change), acceptance criteria and an
  estimate (see Data model).
- A phase is a draft until it is approved. A barrier phase waits for every
  earlier phase. A phase is accepted by the lead (soft gate) or by the person
  (hard gate). Phases run in Standard or Free mode; the one open Free phase
  is the team's queue (see Queues).
- A team is created from a template: a roster of role seats, pipelines,
  planning rules and approval defaults. Roles, pipelines and templates live in
  a catalog and are edited in a visual designer (see Team templates).
- The scheduling rule, the order of hand-outs and the caps (see Scheduling
  and hand-out).
- An intent is recorded before anything is sent, and an unconfirmed delivery
  is never sent again.
- A report reaches the lead only after its pipeline stages (review, test) have
  passed, and a pipeline may accept on its own after the last stage (see
  Pipelines).
- Findings are tasks with a severity. Crit joins the running phase and blocks
  its acceptance; Minor goes to a later phase and blocks nothing.
- Everything the person must decide is a card in one list, "Waiting for you".
  The person answers in place and is never asked to type a phrase.
- The lead's own changes to an approved plan are logged and can be reverted
  until the work they touch has started.
- The Team tab shows one plan as Graph, Board, Layers, Lanes and Profile.
- The planning rules given to the lead, and the default timers.

## What changes, and why

| In the fork | Here | Reason |
|---|---|---|
| Its own task, attempt and report (`hgs team report`) | A Zerus task, a Zerus execution attempt, `hgs task report-attempt` | One task protocol for agents, one board, the delivery guarantees that already exist |
| `hgs team worker`, a detached process with a 3 s tick | A teams actor in the desktop service, driven by the watch stream | The service already owns fleet state, delivery, resume and answers |
| The owner is a caller without session markers | The desktop UI authority, and the verified Person authority of `zerus-cg5h.4.11` when it lands | Environment variables do not authenticate |
| A member is matched by computer and session name | By session identity | Session names are labels |
| The merge queue works in the team folder with hooks off and rolls back with `reset --hard` | The integration branch advances by a compare-and-swap ref update; no checkout is written | The primary checkout and the person's branches are never touched |
| Zerus writes folder trust into the agent's configuration before launching a member | Zerus answers the agent's own trust prompt when it appears, for a folder it created for the team (D9) | No agent configuration is written; the answer has an identity and a journal entry |
| An unanswered prompt is denied or given its recommended option after 10 minutes | A fixed list of prompts is confirmed at once under the lead's delegation; the rest go to the lead, and one the lead leaves unanswered goes to the person | A timer never decides |
| A crashed member is restarted automatically, three times an hour | The same, under the lead's delegation (D9), and never for a member a person stopped | A person's stop is not undone |
| Urgent messages interrupt a turn; context is compacted or cleared at usage thresholds | Not taken. A role may start each task in a fresh conversation when the person chose that in the template | A session is changed only by an explicit act or standing choice of the person or the lead |
| Session states that replace `Background` | Not taken | The status contract is unchanged |
| Zerus runs shell lines declared in the repository (test and run templates) | A stage is a task that a member agent runs | Zerus executes nothing from the repository itself |
| CodeLore memory by default | A plain digest of the team's own state | A project memory is a separate product decision |
| Shared folder and build broker | Later (see Folders, branches and integration) | Every session already defaults to its own worktree |
| A task enters a running team through the lead or an intake key | A Zerus task is sent to the team from the board or with `hgs task` | Tasks already have a board, an order and a way to be filed |
| Team state in one store, a whole team decoded per call | Rows, indexes, bounded pages | Session and task counts reach tens of thousands |

## Vocabulary

| Term | Meaning |
|---|---|
| Team | A lead, its members, their work (a plan, a queue or both) and the settings that say what the lead may decide |
| Template | A reusable description of a team: roles, roster, pipelines and settings |
| Role | What a member is for, with its instructions, agent kind, account choice and default pipeline |
| Pipeline | The stages a task passes between a member's report and its acceptance |
| Stage | A review or a test done by a member of a named role, as a task of its own |
| Queue | The team's open phase, which takes tasks sent one by one |
| Standing team | A team with only a queue. It has no end |
| Lead | The session that plans and decides. It writes no feature code |
| Member | A child session of the lead with a role |
| Plan | Phases, and the plan nodes inside them |
| Plan node | One task of the team with its dependencies, role, pipeline and write set. It points at a Zerus task: after approval for planned work, at once for a task sent to the queue |
| Task | A Zerus task (`hgs task`). A team has no task type of its own |
| Attempt | A Zerus execution attempt of that task |
| Write set | Files or folders a task may change |
| Integration branch | The branch that collects accepted work |
| Delegation | A decision the person lets the lead make |
| Waiting for you | The list of cards only the person can answer |
| Home | The computer where the team's lead runs. The team store and the teams actor live there |

In the desktop the word "Team" already names a parent session with its child
sessions. An agent team is the same thing with a plan: the lead is the parent
and the members are its children. No second meaning is introduced.

## Architecture

```
 person ── desktop UI ── (UI authority) ──┐
                                          ▼
 lead agent ── hgs team ──────────► team store ◄──── teams actor (desktop service)
     ▲                                  │                  │ reads the watch stream
     │ doorbell                         │ plan nodes       │ (catalog, tasks)
     │ (guarded send)                   ▼ point at         │
 member agents ── hgs task ───────► Zerus tasks ◄──────────┤ effects, each through an
     (start, report-attempt)        (replicated)           │ existing owner:
                                                           │   session creation
                                                           │   task attempt writer
                                                           │   deferred send
                                                           │   deferred answer + approval journal
                                                           │   session resume
                                                           └─► integration branch (ref update)
```

Each unit has one purpose and a narrow interface.

| Unit | Location | Purpose | Depends on |
|---|---|---|---|
| Plan kernel | `crates/team-plan` (new) | Plan types, validation, readiness, hand-out order, layers, critical path, write-set overlap. Pure: no I/O, time is an argument | nothing |
| Team store | `src/teams/store` | One SQLite file with versioned schema: teams, members, phases, nodes, dependencies, decisions, lead inbox, owner cards, journal | SQLite helpers |
| Operations | `src/teams/ops` | One function per operation: authority check, validation, one transaction with a revision check, one journal entry. An operation never touches a session | kernel, store |
| Teams actor | `src/desktop/teams` | Reacts to store revisions and watch events, asks the kernel what is due, performs effects through existing actors, records intent before and outcome after | kernel, store, service actors |
| Integration | `src/teams/integrate` | Reads reported commits, merges with Git plumbing, advances the integration ref | Git |
| CLI | `src/teams/cli` | `hgs team ...`, sharing actor resolution with `hgs task` | operations |
| Guides | `src/agent_guide/team.md`, `team-lead.md` | What a member and a lead are told | none |
| Client core | `client/core/src/teams` | Typed projections (list, team, graph, waiting, new team) and actions | store reads |
| Frontend | `client/src/ui/teams`, `client/src/store/teams*.ts` | New team, Team tab, Waiting for you | client core |

The kernel is the only place that decides what may run. The actor is the only
place that causes effects in sessions. Operations and the kernel can be tested
without a session; the actor is tested with fake effect ports and then with
real agents in isolation.

## Teams and Zerus tasks

A team task is a Zerus task. The plan adds structure around it and stores that
structure in the team store, keyed by project ID and task UUID. The replicated
task schema is not raised.

| Concern | Where it lives |
|---|---|
| Title, prompt, acceptance criteria, attachments | The task |
| History, attempts, reports, discussion | The task |
| Stage | The task |
| Phase or queue, dependencies, role or assignee, pipeline, write set, estimate, severity | The plan node |
| Who decided, integration commit, holds | The plan node |

**A task can be sent to a team.** An existing task in Backlog or To do, or a
new one, joins the team's queue: a plan node is created that points at it.
Nothing about the task changes except its optional `team` key (below). Its
replicated target stays `any`; no new target kind is added to the task schema.

**Drafts are not tasks.** While a phase is a draft, its nodes hold their own
title and text. Approving the phase creates one task per node in To do, in
plan order, and stores the UUID in the node. The project board therefore shows
approved work only.

**Stages keep their meaning.**

| Plan view | Task stage | Moved by |
|---|---|---|
| Draft | no task yet | phase approval |
| Pending | To do | hand-out |
| In progress | In progress | the attempt writer |
| Reported | In review | the member's `report-attempt` |
| Accepted | Done | the lead (soft gate) or the person |
| Returned | In progress again | reopen to the same member |
| Cancelled | cancelled | the lead or the person |

**Existing commands do the work.** A hand-out is the attempt writer that
`hgs task send` uses, addressed to the member's session. A return is the
existing reopen message. A reassignment is cancel, back to To do, and a new
hand-out; the attempt generation rejects a late report from the old member.

**The link.** At creation a team task gets one optional key, `team`, in its
field map: the team ID and the home computer. Readers that do not know the key
ignore it and sync carries it, so older builds and phones are unaffected. It
is also how a task is sent to a team from another computer: the home
computer's teams actor sees the key arrive and adds the task to the queue.
With it, `hgs task` on any computer can show the team and refuse to an agent
every write that would bypass the plan: sending a team task or handing it
off, changing its stage, target, title or text, starting, reopening or
cancelling it. The refusal applies to every agent on every computer, the
team's lead included: a lead decides through `hgs team` and the service
acts (decided during milestone 1, "No agent drives a team's task" in the
plan; the first draft excepted the lead on the home computer). A person can
still do all of them from the task board; the teams actor records the override and
re-plans. This key changes `crates/replica` and needs a check-in with that
crate's owner before it is written.

**Limits already in the project apply.** Hand-outs are recorded with the lead
as their actor, so the project's `agents_other` and `agent_limit` settings
admit or refuse them as they do for any agent. A refusal becomes a card in
Waiting for you; it is never bypassed.

## Data model

Only load-bearing fields are listed. Every record has a revision; every
mutation carries an idempotent request ID.

- **Team**: ID, project, name, goal, home computer, status, lead member,
  settings, integration branch, base commit, and its copy of the template it
  was created from (roles, roster, pipelines).
- **Role**: name, instructions, agent kind, account choice (named or
  freest), model and effort, default pipeline, context between tasks (keep or
  fresh).
- **Pipeline**: name, stages (kind, role), maximum rounds, acceptance (lead,
  person, automatic), and whether it ends in a land stage.
- **Settings**: delegations (see Authority and delegation), caps, timers, phase approval
  (review or auto), default phase acceptance (lead or person).
- **Member**: ID, role, agent kind, account, computer, state (not created,
  active, paused, removed), session identity (computer UUID, session ID,
  launch ID, last run and conversation), worktree path and ownership nonce,
  restart history.
- **Phase**: number, order, title, mode (standard, free), kind (plan, queue),
  approval, state
  (draft, approved, awaiting acceptance, done, rejected), acceptance (lead,
  person), barrier flag.
- **Plan node**: ID, phase, task UUID (empty while a draft), draft title and
  text, source (planned, sent), dependencies (node, hard or soft), assignee or
  role, pipeline and the state of its stages, write set, estimate in minutes,
  kind (task, check, stage), severity (none, crit, minor), origin node, hold,
  decided by (lead, person, pipeline), integration commit.
- **Decision**: a binding answer attached to nodes, shown in their hand-outs.
- **Lead inbox item**: something the lead must decide (report, question,
  permission request, stopped member, conflict), with its state.
- **Owner card**: something the person must decide (plan or phase approval,
  phase acceptance, proposal, escalation, refused limit).
- **Journal event**: sequence number, time, actor, kind, references, bounded
  text.

Bounds, taken from the fork unless noted: 6 members besides the lead, 50 tasks
in a phase (the planning rule aims at 20 to 25), 500 undecided tasks in a
team, 3 open questions per attempt. Decided queue tasks leave the live set for
an archive table, so a standing team can take tasks indefinitely. The journal is read in pages by sequence number; when a
team is finished its journal moves to an archive table, and a live team's
journal has a hard cap with the oldest entries archived first. A team is never
decoded as a whole: views read the rows they show.

The store is a private file, created when the first team is created, with its
own `user_version`. It adds no leaf to the desktop store, so it does not bind
anyone's ability to run an older `hgs`.

## Team templates

A template is a reusable description of a team: who is in it and how its work
flows. A team is always created from one, so the template is the single input
of the New team form.

A template holds:

- **Roles.** For each: name, the role's instructions, agent kind, the account
  choice (one named account, or the freest account of that agent), model and
  effort where the agent accepts them, the pipeline its tasks use by default,
  and whether a member keeps its conversation between tasks or starts each
  task fresh.
- **Roster.** Seats: a role, how many members, and whether the seat is
  optional.
- **Lead.** Agent kind, the same account choice, and rules added to the lead's
  guide.
- **Pipelines.** Named lists of stages with the way a result is accepted (see
  Pipelines), and which one is the default.
- **Work.** Whether the team has a queue, how tasks enter it (directly or
  through the lead), and how planned phases are approved and accepted.
- **Settings.** Delegations, caps, timers, integration (a branch or none) and
  whether landing is allowed.

A template never holds a project, a folder, a goal, a session or anything
secret. Accounts belong to a computer, so a named account is resolved on the
home computer by the New team form, which asks for the ones it cannot find.

**Choosing an account.** "The freest account" is decided when a member is
launched, from the usage windows that `hgs account inspect` already reports
for each account (used percent and reset time per window).

- Only signed-in accounts of the role's agent on the member's computer count.
- An account with any window fully used is out until that window resets.
- Among the rest, the one whose tightest window has the most left wins.
- When several members are placed at once, an account already given to a
  member of this team loses a tie, so the load spreads.
- An account that reports no windows (an API key with a balance, for
  example) is used when no account with known room exists.
- If nothing is available, the member is not launched and the card says when
  the nearest window resets.

The New team form shows which account each seat got before the person
confirms, and the person can change it. The choice is recorded in the
journal. It is made at launch only: a running conversation is never moved to
another account.

**The catalog.** Roles, pipelines and templates live in a catalog, as in the
fork: a template's seats name roles from it. The built-in catalog ships with
Zerus. A person's own entries are JSON in the `hgs` configuration directory of
that computer. An exported template carries the roles and pipelines it uses,
so the file is complete on another computer. Catalogs kept in a project's
repository or shared between computers are later steps.

**The designer.** A desktop page edits the catalog: roles with their
instructions and agent, pipelines with their stages and acceptance, templates
with their roster and settings, and the lead's planning rules. It is the
fork's designer, imported and reshaped for Zerus. **Save as template** on a
team, and editing a JSON file by hand, write the same entries.

**A team is a copy.** Creating a team copies the template into the team.
Changing the template later does not change a running team, and a team changed
by hand can be saved as a new template.

**A template is data.** Nothing in it is executed. Everything that widens what
agents may decide is shown on the New team form and confirmed there by the
person, whatever the template says: the five delegations, automatic
acceptance, tasks entering without the lead, and landing. A template file from
elsewhere cannot turn any of them on silently.

Built-in templates at the start:

| Template | Roster | Default pipeline | Meant for |
|---|---|---|---|
| Feature delivery | Lead, two developers, a reviewer, a tester | `code` | Planned work on one goal |
| Bugfix | Lead, a developer, a reviewer | `reviewed` | A queue of independent fixes |
| Small | Lead, one implementer, an independent checker | `reviewed` | A small task or a tight budget |

The first two names come from the fork. The New team form edits the team's
copy; the designer edits the catalog.

## Life cycle

Team status: Draft, Planning, Running, Paused, Finished, Archived, and Stopped.

1. **New team.** The person picks a template and a project folder, fills the
   accounts the template leaves open and may change the roster. For planned
   work they also give a goal and may attach a plan file. Two ways lead on
   from here.
2. **A queue team: Create team.** The person confirms members and folders.
   Zerus launches the lead and every member now, each member as a child
   session of the lead in its own generated worktree, and creates the
   integration branch. The team is Running with an empty queue.
3. **Planned work: Start planning.** Only the lead is launched, through the
   same creation path as New session (its own worktree by default). No member,
   worktree or branch for members exists yet. The lead reads the project, asks
   what it needs, may propose changes to the template's roster and submits the
   plan as a draft. Validation answers at once with exact errors: unknown
   member or role, dependency cycle, missing criteria, overlapping write sets
   without a dependency, a phase over its cap. The person then confirms
   **Create team**, which launches the members as in step 2, and approves the
   first phase. Its tasks are created and the team is Running.
4. **Trust.** The startup trust prompts of the team's sessions, for folders
   Zerus created for the team, are confirmed under the lead's delegation (D9)
   and recorded. The grant each agent records is wider than Claude's card
   names: answering a member's folder card trusts the repository's main
   checkout and its subfolders for that member's account, which the setting
   and the audit row say. Codex's own card names the repository root it would
   grant, and the same rule answers it. Nobody has to sit through them.
5. **Running and Paused.** The lead or the person pauses and resumes the team
   or one member. A pause stops hand-outs; it sends nothing to a session.
6. **Finish.** A team finishes only by an explicit `finish`, when every task
   is decided and no Crit is open. The integration branch holds the result.
   A standing team is not finished; it is stopped or archived.
   Landing it on the base branch or pushing it is the person's own step.
7. **Stop** ends the team's work: a running task is cancelled, a task with
   nothing running leaves the team as an ordinary task, and a task that a
   person took themselves after the stop is left to them. **Archive** follows
   Finish or Stop.
   Both are the person's.

A further phase is a draft from the lead. It is approved by the person, or
automatically when the person turned on **Auto-accept drafts** for the team.

## Queues: a team that keeps taking tasks

A team gets work in two ways, and may use both.

- **A plan.** The lead turns a goal into phases of tasks and the person
  approves them.
- **A queue.** Tasks are sent to the team one by one and each runs through a
  pipeline. No plan is drawn up first.

A team that has only a queue is a standing team: it is created once from a
template, has no end, and waits when its queue is empty. In the fork's terms
the queue is the one open Free phase.

**Sending a task.**

- A new task: `hgs task add --team TEAM`, the New task form with a team
  chosen, or **Add task** on the Team tab.
- An existing task in Backlog or To do: **Send to team** on its card, or
  `hgs task target REF team TEAM`.

Either way the task is an ordinary Zerus task, and a plan node in the team's
queue points at it.

**Admission.** A task sent by the person is in at once. A task the lead adds
is in and logged. A task sent by a member or by any other agent waits for the
lead to admit it, and the project's agent limits apply to it.

**Routing.** The team's template says how a queued task starts.

- *Directly.* The task takes the default pipeline and goes to the first free
  member of the implementing role. The lead is not involved until something
  needs a decision.
- *Through the lead.* The lead first sets the role, the pipeline, any
  dependencies and write set, may split the task, and then releases it.

The person can set the role or the pipeline while sending, which skips routing
for that task.

**Order.** The queue is taken in the order of the project board's To do
column. The person reorders the queue by dragging cards there. There is no
second priority.

**Capacity.** A member has one task at a time, so the roster is the limit: two
developers work on two tasks, and reviews wait for a free reviewer.

**Context between tasks.** By default a member's conversation continues from
task to task. A role can instead start each task in a fresh conversation,
through the existing Clear session and `resume --fresh` paths, and only while
the member is idle with no open attempt. The person chooses this in the
template, and each use is recorded in the journal.

**Results.** A standing team keeps one long-lived integration branch. So that
it does not drift, the lead can ask Zerus to bring in the base branch's new
commits: the merge uses the same plumbing as integration, and a conflict
becomes a task for a member. A team for independent fixes can run without an
integration branch; each task then branches from the base and its branch is
the result.

**Idle and end.** An empty queue sends nothing to anyone. A standing team is
paused, stopped and archived by the person; `finish` does not apply to it.

## Scheduling and hand-out

The kernel says a task is ready when all of these hold:

1. its phase is approved or it is in the queue and admitted; no earlier
   phase awaits the person's acceptance; a barrier phase has no unfinished
   earlier phase;
2. the task is in To do and has no hold;
3. every hard dependency is Done and merged into the integration branch; every
   soft dependency has started; a cancelled dependency releases;
4. no open task has an overlapping write set;
5. a member is free for it.

A member is free when its session is live and idle (not working, not
`Background`, no pending request), it has no open attempt, the task index
shows no other task in progress for that session, and it is not paused. The
plan's assignee comes first. For a role task the first free member with that
role takes it; an assignee that cannot take a task that is not pinned gives it
to a free member of the same role.

Order: stage tasks first, because they unblock work that is already done;
then planned tasks, those with the most transitive dependants first and then
in plan order; then queue tasks in the order of the project board.

The hand-out, performed by the teams actor:

1. record the intent (node, member, request ID) in the journal;
2. call the attempt writer once with that request ID, as the lead;
3. settle from the writer's own outcome: submitted, refused, cutting over or
   uncertain.

A refusal returns the task to Pending and the member is tried again later. An
uncertain outcome is never retried: it becomes a lead inbox item, and the lead
may settle it by reassigning, which opens a new attempt generation.

The task prompt carries, after the task's own text: team, role and goal;
criteria; the reports of its dependencies, bounded (2000 characters each, 6000
in total); binding decisions; the base commit and the branch to create; the
write set; and the exact acknowledgement and report commands of the task
protocol.

Until `zerus-cg5h.4.8` delivers per-executor queues, the actor guarantees one
open task per member itself. When the queue exists, step 2 enqueues instead.
This is the only seam between the two, and the actor adds no second way to
deliver a task into a session.

## Reports and decisions

1. The member reports with the `report-attempt` command pinned in its prompt.
   The task moves to In review by the existing rules.
2. The actor sees the change, reads the member's branch head without writing
   (the reported commit), notes uncommitted changes if any, and files a lead
   inbox item.
3. The actor rings the lead: one guarded send saying how many items wait.
   Doorbells are coalesced (at most one every 120 s). The inbox is the durable
   state; a doorbell only points at it, so an uncertain doorbell is not sent
   again and the next change rings a new one.
4. The lead reads the inbox and decides: **accept**, **return** with a reason,
   **reassign**, **hold** or **cancel**.
5. Accept runs integration (see Folders, branches and integration). When the merge is clean the task is
   Done. When it conflicts, the task returns to its member with the conflict
   list and the new base.
6. Return reopens the task to the same member with the reason.

A member whose turn ended without a report is noticed after 120 s and becomes
an inbox item ("ended without a report"). A hand-out that has not started
after 180 s is another. An undecided report reminds the lead after 15 minutes
and reaches the person after 60. A lead whose inbox has waited 20 minutes
while it does nothing produces an owner card.

When the task's pipeline has stages, they run between steps 1 and 2: the lead
gets the inbox item after the last stage, or not at all when the pipeline
accepts on its own.

## Pipelines

A pipeline says what happens between a member's report and the end of a task.

- A **stage** is a review or a test done by a member of a named role. It is a
  task of kind `stage`, created when the implementer reports and handed out
  like any other task. Its verdict is pass or return.
- **Return** reopens the task to the implementer with the findings, without
  the lead. After three rounds (a pipeline may set another number) the task
  goes to the lead.
- **Acceptance** after the last stage is one of three: the lead decides, the
  person decides, or the task is accepted because every stage passed.
- **Land** is an optional last stage: a member opens a pull request or merges,
  as an ordinary task in its own session with its own permissions. It exists
  only in a team where the person allowed landing. Zerus itself never pushes.

| Pipeline | Stages | Accepted by |
|---|---|---|
| `code` | review by a reviewer, test by a tester | the lead |
| `reviewed` | review by a reviewer | the lead |
| `docs` | review by a reviewer | the pipeline, when the review passes |
| `check` | none | the lead |

`code`, `docs` and `check` are the fork's built-in pipelines; `reviewed` is
added here.

A pipeline that names a role with no seat in the team is refused when the team
is created. A stage is never handed to the member who implemented the task.

## Authority and delegation

| Actor | How it is known | May do |
|---|---|---|
| Person | The desktop UI authority (`src/desktop/ui_authority`); for the CLI, the verified Person authority of `zerus-cg5h.4.11` when it exists | Everything; alone: create, approve, change settings and delegations, allow landing, accept a hard-gate phase, stop, archive, land or push by hand, sign in, change permissions beyond a member's own session or folder, remove a member or a worktree |
| Lead | The team's lead session, verified the way `hgs task` verifies an agent (live run and process ancestry) | Plan inside the approved goal, hand-out decisions, accept, return, reassign, hold, cancel, answer members' questions and permission requests, pause and resume, propose cards, request finish |
| Member | A member session of the team, verified the same way | Acknowledge and report its own attempt, give the verdict of its own stage task, ask the lead, read its own context |
| Any other session | none | Read |

Until `zerus-cg5h.4.11` lands, a person-only operation from the CLI is
accepted only when the call has no session markers and no live session agent
among its process ancestors, and its journal entry records that origin. This
is an interim rule and is named as such in the output.

**Delegations (D5, D9).** The person sets each one when the team is created
(the built-in templates propose all of them on) and can turn each off for a
team at any time.

- **Accept results.** The lead moves a team task to Done. In a hard-gate phase
  the phase then waits for the person.
- **Keep members working.** Whatever a member session needs in order to go on
  is confirmed inside the team.
  - *Confirmed at once*, with no turn of the lead, and recorded as confirmed
    under the lead's delegation: the startup folder trust and Codex hooks
    trust of a folder Zerus created for this team; resuming a member whose
    agent exited by itself, at most three times an hour, with a short brief of
    where its task stands; continuing after a provider failure, a rate limit
    or a usage-limit reset through the existing Automatic recovery, which is
    on for a team's members; and the permission categories the team took from
    the existing automatic approvals.
  - *Decided by the lead*: every other permission request and every question
    or chooser of a member.
  - *To the person*: what the lead leaves unanswered for ten minutes, as a
    card.
- **Merge accepted work.** Accept triggers integration.

Every confirmation, whoever makes it, follows the rules that automatic
approvals follow today. The answer goes to the agent's own prompt. The actor
checks the exact run, conversation, question and hash, reserves the entry in
the approval journal before delivery, delivers through the deferred-answer
actor and finishes the entry from that actor's receipt. An unknown delivery is
never repeated, and the original task prompt is never replayed. A folder
counts as created for the team only with its verified ownership record.
Providers the approval journal does not admit today need that admission
extended before their prompts can be confirmed; until then they go to the
person.

With accounts in bypass mode, which is the default for Claude, Codex and Kimi,
members rarely ask for permission at all; the delegation matters for accounts
in provider or default mode.

Not delegated: signing in and anything else that needs credentials; billing
and quota; a permission change that reaches beyond the member's own session
or folder; a member that a person stopped; the destructive actions that are
the person's everywhere; and landing outside the integration branch.

Attention follows the existing supervised-child contract: a member is a
supervised child of the lead, so its ordinary questions are quiet and show as
**Asking** on the lead's Team toggle. A member's permission request and trust
prompt are person-only today. Under the delegation they become the team's,
which needs one new host-reported flag and the matching change in the
desktop, Android, iOS and the shared fixture, made as one aligned change. A
prompt confirmed at once is gone before anyone is notified.

## Folders, branches and integration

- Every member works in its own generated worktree, created by the existing
  path with its ownership record. Worktree maintenance needs no new rule: a
  live or paused member session protects its folder, and after the team is
  archived the ordinary rules apply.
- At Create team Zerus creates the integration branch from the base commit. It
  is a branch only. It is never checked out, so advancing it cannot disturb a
  working tree.
- Each task gets a branch. The hand-out names the base commit (the integration
  tip) and the branch; the member's agent creates it. Zerus writes nothing in
  a member's folder.
- On accept, integration: verify the reported commit descends from the stated
  base; compute the merge with `git merge-tree --write-tree`; when it is
  clean, create the merge commit and advance the integration ref with a
  compare-and-swap on its old value. If the ref moved, compute again. If the
  merge conflicts, no ref and no reachable object changes, and the task
  returns to its member. A commit the branch already contains is a no-op.
- Integration never runs `reset`, never writes the primary checkout or any
  other branch, and never pushes.
- Because these merges are made with plumbing, commit hooks do not run on
  them; the member's own commits ran the hooks in its worktree. Hooks defined
  in Git configuration never run either: one defined by the repository makes
  integration unavailable, and the person's own are switched off for these
  calls. A merge driver defined by the repository also makes integration
  unavailable; the person's own driver and signer are honoured. A repository
  that requires signed commits gets signed merges, and a signing step that
  needs the person stops integration with an owner card.
- Integration is unavailable, and the team runs without an integration
  branch and says so, when Git is older than 2.38, when the repository has
  submodules, or when changed files need an external content filter.

**Shared folder.** The fork also lets all members work in one folder, for
projects where a second checkout is too expensive or the folder is not a Git
repository. That mode needs a build broker and test isolation. It is not in
the first four milestones. The write set is in the plan from the start, so the
mode can be added without changing the plan model.

## Runtime

- The teams actor is part of the desktop service on the team's home computer.
  It follows the service's actor pattern: admission, a fixed plan, the
  original CLI child and its custody, and "restart never continues an
  operation". After a restart it reads the journal: an intent with a recorded
  outcome is settled; an intent without one is unknown and becomes an inbox
  item.
- It is driven by events: the store's revision and the local watch stream
  (catalog rows for member sessions, the tasks topic for team projects). The
  tasks topic allows eight projects per stream; beyond that the actor reads
  the affected team's tasks in one bounded page when a catalog row changes. A
  slow safety check runs once a minute. Nothing polls every few seconds.
- With the desktop service not running, `hgs team` still records the lead's
  operations and answers reads; nothing is handed out, and status says so.
- **Preview switch.** Teams are off by default behind one per-computer
  setting. With it off, `hgs team` answers `teams_inactive`, the desktop shows
  no entry point, nothing of teams runs and no code path of an ordinary session
  changes. A store written while the preview was on stays where it is:
  switching the preview off deletes nothing.
- First milestones are single-computer: the team, its lead and its members are
  on the home computer, and the Team tab is shown by that computer's desktop.
  Viewing from another computer and remote members come in the fourth
  milestone through a `teams` watch topic and the existing peer command path.
- Phones are unchanged. Team tasks appear as ordinary tasks.

## CLI and agent guidance

`hgs team` uses subcommands, prose on stdin and `--json` for structured
output, like `hgs task`. Plans go in as JSON on stdin and are answered with
validation results.

| Who | Commands |
|---|---|
| Person (the desktop uses the same operations) | `catalog show`, `catalog import`, `catalog export`, `template list`, `template show`, `template save`, `template import`, `template export`, `template remove`, `create --template`, `start-planning`, `members`, `approve`, `reject`, `accept-phase`, `send-back-phase`, `settings`, `pause`, `resume`, `stop`, `finish`, `archive`, `decide` (a card) |
| Lead | `propose-members`, `plan draft`, `plan edit`, `admit`, `route`, `status`, `status --digest`, `inbox`, `accept`, `return`, `reassign`, `hold`, `release`, `cancel`, `spawn` (a finding), `reply`, `answer`, `restart`, `pause`, `resume`, `sync-base`, `propose`, `finish` |
| Member | `current`, `ask`, `verdict` (pass or return, for a stage task) |

Tasks are sent to a team with the task commands, not with `hgs team`:
`hgs task add --team TEAM` files a new one, and `hgs task target REF team TEAM`
sends an existing one.

Members report with `hgs task`; a stage member closes its stage with the
`verdict` command pinned in that task's prompt. The start-of-session text
about Zerus tasks stays true for them, so the two guides never contradict each
other.

Guide pages: `hgs guide team` for a member and `hgs guide team-lead` for the
lead, each short enough to read in one call. The lead's planning rules are the
fork's, carried over: 20 to 25 tasks in a phase; hard dependencies only where
needed and none across phases (use a barrier); shared foundations first; a
reviewer never writes what it reviews; QA never implements; task text is
self-contained; the lead never builds, tests or merges a member's work by
hand.

## Desktop

**Data.** Team views arrive the way task views do: a keyed list in the
per-frame batch (`list`, `waiting`, `team:<id>`, `graph:<id>`, `new:<id>`),
latest value per key. The desktop follows one team at a time and stops
following it when the tab is hidden. Actions return done, failed, uncertain or
confirm, and an uncertain action is never repeated automatically.

**Screens.**

- **New team**: a lazy dialog opened from the rail and the command table. It
  starts from a template and shows its roster, pipelines and the settings that
  need the person's confirmation; **Save as template** keeps a changed one.
- **Team tab** on the lead's session detail: team bar, member cards, and the
  work as a Board with the queue as its own group, the task card with its
  stages, and **Add task**. The Graph and the phase header come with planned
  work in the second milestone; Layers, Lanes and Profile in the fourth.
- **Task board**: a card in Backlog or To do gets **Send to team**, and the
  New task form gets a team chooser.
- **Designer**: a rail page that edits the catalog: roles, pipelines,
  templates and the lead's planning rules. It arrives in the second milestone.
- **Waiting for you**: a rail entry with a count, listing owner cards and the
  person-only requests of team sessions.
- **Sessions list**: members nest under their lead through the existing
  parent and child grouping, with its Asking and Needs you counters. No new
  grouping is added.

**Reuse of the fork's views.** The plan views do not depend on the fork's
store, so they move with little change. The table covers the first two
milestones; line counts are estimates from reading, not from a build.

| Treatment | Files | About |
|---|---|---|
| Imported unchanged | `planLayout.ts`, `plan.ts`, `planTasks.ts`, `phaseHeader.ts`, `team-model.ts`, `members.ts`, `roles.ts`, `sideLists.ts`, `NewTeamFooter.tsx`, `NewTeamGuide.tsx`, `newTeamFollow.ts`, `teamConfirm.ts`, `TeamConfirmDialog.tsx`, `teamLayout.ts`, `newteam.css`, with their tests | 2,900 lines |
| Imported, then adapted | `PlanView.tsx`, `plan-panels.tsx`, `planModes.ts`, `TeamActions.tsx`, `actions.ts`, `decisions.ts`, `Waiting.tsx`, `ProposalCards.tsx`, `team-bar.tsx`, `MemberCard.tsx`, `NewTeam.tsx`, the Teams stores, `plan.css`, `teams.css`, the Teams types; for the designer `TeamDesigner.tsx`, `TeamCatalog.tsx`, `TeamCatalogEditor.tsx`, `catalog.ts`, `catalogControls.ts`, `catalog.css` and the core catalog editor | 7,000 lines kept |
| Written new | Team tab shell, New team page, client core projections and actions, Tauri commands | 2,000 lines |
| Not taken in these milestones | Lanes, heat, Profile, feed, swap, the 3 s poller and its readers | — |

The Graph draws cards as positioned elements and edges as inline SVG, with the
fork's own layout and no new dependency.

**Conventions.** Strings go through the i18n catalogs with Russian in the
supplement file (about 950 strings for the first two milestones, counted in
the fork's views). Teams views
load lazily and the entry bundle stays under its 500,000 byte limit. The fork's
`.team-badge` class collides with an existing one and is renamed on import.
Eleven skin tokens the plan views need are added to the skins. The scroll map
that never shrinks and the per-team local storage key are replaced by bounded
settings.

## Project rules

| Rule | Effect of this design |
|---|---|
| Git is never initialized automatically | Unchanged. A team needs an existing repository for worktrees and integration |
| Worktrees appear only after an explicit start | Kept. The lead's at Start planning, the members' at Create team |
| Destructive actions need confirmation | Kept. Stop, archive and member removal are the person's, confirmed in the desktop core |
| Automatic tool approvals are a separate opt-in, off by default | Unchanged for ordinary sessions. For a team's members the delegation (D5, D9) confirms what they need, with the same journal, identity and no-replay rules |
| Startup trust and persistent permission changes stay manual | Changed for a team's members by D9: Zerus answers the trust prompt of a folder it created for the team. On the member's account the grant applies to the repository's main checkout and its subfolders, which the setting and the audit row say. Claude's card names only the folder and the grant is wider; Codex's card names the repository root. A change beyond a member's own session or the team repository stays manual |
| "Only the person accepts Done" (agent guide) | Unchanged for `hgs task`: no agent moves a team task to Done. Under D5 the lead accepts through `hgs team`, and the service's own command finishes the task |
| An unknown delivery is never replayed | Kept for hand-outs, doorbells, answers and resumes |
| `Background` status contract | Unchanged. A member in `Background` is busy |
| Native agents are never restarted or killed as part of an update | Unchanged. A member whose agent exited by itself is resumed under the delegation, at most three times an hour; one that a person stopped is not |
| Bounded pages, indexed queries, background I/O | Kept: rows and pages, no whole-team reads, bounded batches |
| Session names are labels | Kept: identity only |
| The UI never separates values with dots | Kept; the imported views already pass the check |
| Configuration is data and is never executed | Kept. A template is data; settings that widen what agents may decide are confirmed by the person on the New team form |

## Authorship and credit

- The reference is pinned: `WhiteWh/zerus` at `ca4cc4481`.
- A file taken unchanged is committed with WhiteWh as its Git author, the
  importing session as committer, and a message naming the source commit and
  paths. Where the fork's history of that file includes another author, that
  author is added as co-author.
- A file that is imported and then adapted is two commits: the import as
  above, then the adaptation.
- Credit ends with the imported base (D7). Adaptations and all new code are
  Zerus's own commits and carry no co-author trailer.
- From the Rust side the candidates for import are the plan graph kernel
  (cycles, layers, dependants, hand-out order), the catalog model (roles,
  seats, templates, pipelines), the critical-path function, the estimate
  medians and the write-set overlap helpers, with their tests.
  The dispatcher, the operations, the commands and the store are not imported:
  they cannot be separated from the fork's own modules.
- The specification, the user guide and the reference name the origin.
- When the first milestone is merged, PR #322 gets the `partially-adopted`
  label and a comment saying exactly what was adopted, as `CONTRIBUTING.md`
  asks. That step is the owner's to approve.

## Delivery

**Branch.** Work is collected on `integrate/teams`. Each work package has its
own branch and worktree from it. A milestone goes to `main` as one pull
request when it works end to end and passes the local gate, with the preview
switch off by default.

**Milestones.**

| # | Name | Contains | Done when |
|---|---|---|---|
| M1 | Queue team | Templates (built-in and a person's own), team creation from a template, member launch, the queue and sending tasks to it, teams actor, hand-out, reports, pipelines with review and test stages, acceptance by the lead or by the pipeline, integration branch with base sync, confirmation of folder trust and resume of an exited member under the lead's delegation, pause, Board, Waiting for you | The outcome conditions at the top hold; with the switch off, nothing differs from today |
| M2 | Planned work and designer | Planning by the lead from a goal, phases, dependencies, write sets, approval, barrier, hard and soft gates, the lead's logged plan changes, Graph; the designer and catalog editing | A lead plans 10 to 20 tasks in phases, the person approves, and the team finishes them |
| M3 | Autonomy and flow | The lead's answers to members' permission requests and Automatic recovery for members, with the attention change on all clients, findings, Auto-accept drafts, stalled lead, digest, the land stage, fresh context per task, teams without an integration branch | A standing team runs for days and a planned team overnight, and every decision is in the journal |
| M4 | Insight and reach | Layers, Lanes, Profile, estimates; `teams` watch topic, viewing from other computers, remote members; phone attention; `hgs test affected` as a standalone command | A team with a member on a second computer is watched from a third |
| M5 | Later | Shared folder with build broker, catalogs in a repository or shared between computers, skeleton plans in templates, sub-teams, budgets, memory extensions | Decided separately |

This specification is detailed enough to plan M1 and M2. Each milestone gets
its own implementation plan, and M3 and M4 get a short design addendum first.

**Work packages of M1.** Contracts come first so the others can run in
parallel.

| # | Package | Owns | After |
|---|---|---|---|
| 1 | Contracts: team, template and pipeline types, wire shapes, shared fixtures, error codes, preview switch | `crates/team-plan` types, `fixtures/teams` | — |
| 2 | Plan kernel: import and adapt the graph kernel; readiness, hand-out order, validation | `crates/team-plan` logic | 1 |
| 3 | Team store and operations: teams, members, queue, nodes, lead inbox, owner cards, journal | `src/teams/store`, `src/teams/ops` | 1 |
| 4 | Catalog and templates: built-ins, a person's catalog, validation, save, import and export | `src/teams/catalog` | 1 |
| 5 | CLI, actor resolution, member and lead guide pages | `src/teams/cli`, `src/agent_guide` | 3, 4 |
| 6 | Tasks: sending a task to a team, the `team` key, refusals in `hgs task` | `crates/replica` (small), `src/tasks` (small) | 3, replica check-in |
| 7 | Member launch and identity: lead and members as parent and children in worktrees | `src/desktop/teams` (creation) | 3 |
| 8 | Teams actor: events, intents, hand-out through the attempt writer, doorbells | `src/desktop/teams` | 2, 3, 7, seam agreed |
| 9 | Pipelines: stage tasks, verdicts, rounds, acceptance by the lead or the pipeline | `src/teams/ops`, `crates/team-plan` | 8 |
| 10 | Lead inbox and decisions, timers, escalation to owner cards, restart and pause | `src/teams/ops`, `src/desktop/teams` | 8 |
| 11 | Integration branch: commit checks, plumbing merge, ref update, base sync | `src/teams/integrate` | 3 |
| 12 | Client core: projections, actions, batch key, commands | `client/core/src/teams`, `client/src-tauri` | 1 |
| 13 | Frontend: New team from a template; Team tab with team bar, members, Board and task card | `client/src/ui/teams` | 12 |
| 14 | Frontend: Waiting for you; Send to team on the task board | `client/src/ui/teams`, rail, task board | 12 |
| 15 | Isolated real-agent coverage of hand-out, report, verdict and resume | `tests` | 8, 9, 10 |
| 16 | Guide, reference, credit, the Tauri specification row | `docs` | 5 |
| 17 | Standing confirmations (D9): folder trust for team worktrees, resume of an exited member | `src/desktop/teams`, `src/approval_log` (small) | 7, 8 |

**Work packages of M2** in outline: planning operations and validation; drafts
and approval; tasks created at approval; gates and barrier; the lead's logged
plan changes; import of the Graph and the phase header; `hgs team` planning
commands and the lead's planning guide; the designer, imported and reshaped,
with its core catalog editor.

**How subagents work.** The coordinating session owns this specification, the
contracts, `integrate/teams`, reviews and merges. For each package: one
implementer in its own worktree, tests first; then an independent reviewer
that is given the diff and the relevant section, not the implementer's
summary; then fixes; then the coordinator merges. Subagents commit locally and
never push. They do not use the Mac or the Windows computer unless granted for
that task, keep build output inside their worktree, and at most three heavy
builds run at once on the shared computer. Every report includes the raw test
log.

**Checks.** Unit tests for the kernel and operations; fake-port tests for the
actor, including a restart at every step of a hand-out; fixtures shared by
client core and frontend; the local gate that mirrors CI before a milestone
pull request; and, for the behaviours that touch a native agent (hand-out,
delegated answer, resume), isolated real-agent contract tests on the latest
release of each supported agent.

**Coordination.** The per-executor queue, task controls and Person authority
(`zerus-cg5h.4.7`, `.4.8`, `.4.9`, `.4.11`) belong to another stream. Before
package 8 of M1 starts, the seam described under Scheduling and hand-out is
agreed with that stream's coordinator. Teams do not change those modules.

## Several people

Sharing teams, projects, tasks and sessions between different developers is a
later feature of its own, and it is not designed here. Three things are fixed
now so that it can be added without reworking Teams.

- **Who, not "the owner".** Every decision, card and journal entry records the
  person who made it as an identifier, and a team records the person who owns
  it. Today there is one person, so the value is always the same. Authority is
  decided in one place, a single function of actor, operation and team, so
  that roles for other people can be added there.
- **Two doors between computers.** Work reaches a team from another computer
  only as a Zerus task carrying the `team` key, and a team is seen from
  another computer only through the `teams` watch topic of M4. Nothing else in
  Teams reaches across computers. Sharing between people will use the same two
  doors.
- **The word.** "Team" means a team of agents everywhere in Zerus. People
  working together get a different name, chosen when that feature is designed.

Not decided here: how people are identified to each other, what one person may
see or do in another's sessions, and where shared state lives.

## Risks

| Risk | Handling |
|---|---|
| The task queue stream changes the attempt writer while M1 builds on it | One seam, agreed before package 8; packages 1 to 7 and 11 to 14 do not depend on it |
| A member's conversation grows over many unrelated tasks | A role can start each task fresh (M3); until then the agent's own compaction applies |
| A standing team's integration branch drifts from the base | Base sync on the lead's request, by the same plumbing; a conflict is a task |
| An agent's sandbox blocks `hgs team` writes | Operations write the way `hgs task` does; verified with each supported agent in M1 |
| The `team` key in the task field map | Check-in with the replica owner; without it a team still takes tasks sent on its home computer, but not from another computer |
| The attention change for delegated answers spans four clients | One aligned change in M3 with the shared fixture first |
| Trust is confirmed without a person | Only for a folder with Zerus's verified ownership record, created for that team; each agent's trust prompt has an isolated contract test |
| Entry bundle size | Lazy views; the size check runs on every client package |
| Plumbing merges skip hooks | Stated in the guide; signing honoured; integration can be turned off per team |
| The fork keeps moving | The reference is pinned; later changes are reviewed as contributions against this code |

## Open questions for the owner

1. **Limits of the lead's delegation.** This design keeps with the person:
   signing in, billing and quota, a permission change beyond a member's own
   session or folder, a member that a person stopped, and destructive
   actions. Is anything missing from that list, or wrongly on it?
2. **Several people.** The design fixes three seams and nothing more (see
   Several people). Is that the right amount for now?

## Documents to update as milestones land

- `CLAUDE.md` and `AGENTS.md`: where the team code lives, and the team's
  standing confirmations next to the automatic-approval rule. No exception
  to person-only acceptance is needed, and `src/agent_guide/tasks.md` stays
  as it is: see the project-rules table.
- `docs/reference.md`: a Teams section; the supervised-child attention text in
  M3.
- `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`: the
  "Orchestration" row, pointing here.
- A user guide for teams, based on the fork's guide, in English and Russian.
  The English guide is `docs/agent-teams.md`; the Russian one waits for the
  owner's decision, because the repository's documentation is English.
