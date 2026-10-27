# Zerus teams: the next phase of work

Status: discussion draft, 2026-10-25. Nothing here is approved for implementation
until the owner marks it so. It builds on the phases work (branch
`team/f3d22e71/lead` on the WhiteWh fork, spec
`docs/superpowers/specs/2026-10-25-team-phases.md`).

Sources: feedback from the leads of earlier teams (Claude lead of the phases
team, a documentation lead, the Codex lead), the owner's wishes, and the
bootstrap notes of another agent system. Each item below carries the owner's decision from the
review of 2026-10-25.

## 0. Ground rules this plan keeps

- The lead plans and decides; members execute; Zerus dispatches; the owner's
  word is final and the owner's stop is final.
- An accepted task's text, criteria and history are never edited. New work is
  appended as a phase at the end. Changed by D2: a critical finding may reopen
  an accepted task as a new attempt; the earlier acceptance stays in its
  history.
- Every mutation is idempotent by `request_id`; histories and lists are bounded.
- Permissions stay per request. Nothing here grants blanket automatic
  approvals (CLAUDE.md: automatic tool approvals are a separate desktop opt-in).
  Where an item comes close to that line it says so.
- The next team that builds this runs on the currently installed Zerus
  (no phases, no extend). Its plan must be complete up front, as in the phases
  run; fixes and retests are planned tasks that are cancelled when empty.
- Every phase ends with a local build ready to install (portable zip plus
  notes: changes, install and rollback, known issues). The owner installs it
  and brings back the problems found; they feed the next phase.

## 1. Already closed by the phases work

| Complaint | Closed by | State |
|---|---|---|
| No tasks can be added to a running team | `hgs team extend`, a phase appended at the end | closed |
| An accepted task cannot be retried; new bugs had to ride on a final task | a fix task in a new phase, pinned to the same member, depending on the accepted one | closed; reopening the task itself is D2 |
| A tester cannot find the coder's work; a fixer cannot see the findings | "From dependencies" section with reports and folders | closed |
| A coder's task may go to another coder of the same role | `pinned` in plan and extend | closed |
| The team closes by itself while the lead still works | waiting for the lead, `finish` with a summary | closed |
| A finished report waited ~50 min for the lead | only the all-decided wait exists; no per-report timer | partial, see C4 |
| The GUI does not show which task a member works on | member card shows "Working on #N" | partial, see G2 |
| Hierarchical teams | REQUIREMENTS.md "Prompt 2" | not started |

## A. Task delivery, attempt and report

Problem: "handed out" is shown as in progress while the text may still sit in
the agent's input; a progress line or a question is captured as the report and
closes the attempt; a rejected task is handed straight back even to a busy
member; work continues outside any attempt.

- **A1. Real start states.** An attempt moves through `sent` -> `input
  accepted` -> `started` (the agent's first turn after the hand-off began) ->
  `reported`. Without `started` after N minutes the task shows a warning.
  Owner: yes; show every state as an icon with a tooltip, not as text.
- **A2. No report capture while work is open.** The last reply becomes the
  report only when the turn ended, no question of that member is open and the
  session has no Background jobs. Owner: yes.
- **A3. A question is not a report.** `hgs team ask` (member -> lead): the task
  shows "waiting for an answer", the attempt stays open, `hgs team answer`
  closes the question. Owner: yes.
- **A4. Current attempt on demand.** `hgs team current` returns the member's
  task, attempt id and the report command, so a member never digs ids out of
  messages. Owner: yes.
- **A5. Return with a hold.** `reject` with `"hold": true` reopens the task but
  does not hand it out until the lead releases it (`release`). Owner: yes, and
  the team UI shows each member's queue: the current task and tasks returned to
  it (held or waiting for it). Other pending tasks are not listed there.
- **A6. Replace a captured report.** While a captured report is undecided, the
  member may send the real report for the same attempt; it replaces the
  captured text and is marked as such. Owner: yes.

## B. Sandbox, permissions and permission requests

Problem: a member in a worktree cannot commit (git metadata lives in the shared
`.git`), build or run tests inside its sandbox; every step becomes a
danger-full-access request; the request card hides the command and who
answered; a denial cannot carry an explanation.

- **B1. Show the exact command** in the permission request (lead and GUI), and
  after the answer who answered and how: owner, lead, rule, timeout. Owner: yes.
  Gap (seen 2026-10-26): `hgs team answer` with the allow option refuses a
  non-empty text ("Choose an approval outcome"); an allow with a note should
  work, the note reaching the member as a deny's reason does (B2).
- **B2. Deny with a reason.** `hgs team answer` takes a text that reaches the
  member as its reason (and a recommendation what to do instead). Owner: yes.
- **B3. Preflight at team creation.** Warn when a member's agent runs in a
  sandbox that cannot write the worktree's git metadata, build directories or
  temp files. Owner: yes.
- **B4. Commit through Zerus.** `hgs team commit --json
  {request_id, team, task, attempt, message, paths?}`: Zerus (outside the
  sandbox) stages and commits in the member's own worktree on the member's team
  branch only, never pushes, records the sha on the attempt. Refused for any
  other folder or branch. Owner: yes.
  Later (re-review, normal): `refused_repository_config` refuses a list of
  risky keys; replace that denylist with an allowlist of safe local and
  worktree keys, so a key git adds later is refused by default.
- **B5. Permission profiles, decided by the lead.** Owner's model:
  1. The team designer sets base permissions per role in the template
     (for example: read only / write own folder / build and test / commit via
     Zerus / network).
  2. At team creation the lead may adjust them; any widening shows a warning
     and needs the owner's confirmation.
  3. During work, when a member asks for more, the lead rates the command:
     low risk inside the member's folder -> allow; high risk -> ask the owner;
     wrong or harmful -> deny with an explanation and a better way (B2).
  The rating rules live in the built-in lead protocol (E). The lead's own
  session must not block its answers: the Claude Code permission check once
  blocked an approval the owner had granted. Owner: the lead's session gets a
  permission rule that always allows `hgs team answer`; the lead's rating is
  the guard, and high-risk requests go to the owner by the protocol anyway.
  Zerus writes that rule when it starts a lead session, for each agent that
  supports such rules.
- **B6. Workspace policy.** The template sets `folders: auto | shared`.
  `shared` forbids worktrees. `auto` (default) lets the lead choose per member,
  from the tasks, between a worktree and the shared folder. The owner may
  forbid worktrees for a project. Replaces the always-checked "Worktree per
  member" box. Owner: yes, auto by default.

## C. The lead as a bottleneck

- **C1. The lead's own tasks are not handed out as messages.** A ready lead
  task appears in the lead's decision list (C2) and the lead reports it when it
  suits; no extra "report to yourself, accept yourself" step. Owner: yes.
- **C2. One inbox for the lead.** `hgs team inbox`: reports to decide, open
  questions, permission requests, ready lead tasks, each with its age. Zerus
  notifies the lead of new items (coalesced, at most one message per interval,
  only when the lead is idle). Owner: yes, with the notifier.
- **C3. Who waits for whom.** `hgs team status` gains, per task, what blocks it
  and which action unblocks it; agents ask for it when needed rather than
  receiving a stream of updates. Owner: yes, on demand via hgs.
- **C4. Unhandled report reminder.** A report undecided for N minutes reminds
  the lead; after M minutes it is shown to the owner. Owner: yes.
- **C5. Check tasks.** A task of kind `check` has a command and an expected
  exit code. When its deps are done, Zerus hands the command to the assigned
  member, which runs it in its own folder, inside its own sandbox and with its
  own permissions, and returns the exit code and output tail; Zerus accepts or
  rejects the task by the exit code, without the lead. Owner: yes, run in the
  member's session. The plan shows each check command to the owner before
  approval.
- **C7. Plan health feedback (owner, from team 58fd186a).** A lead turned
  owner rounds into whole-round barriers (a runtime install waited for the
  acceptance of model installs; one task waited for three blocks' acceptance)
  and the team ran one task at a time for hours while members idled. Zerus
  must say so to the planning lead, explicitly and with numbers, at plan,
  extend and revision time and while the team runs:
  - critical path length versus total work, expected parallelism per member,
    the widest ready set; hard dependencies on review/QA/acceptance tasks that
    gate unrelated work (fan-in barriers); members with no ready work for N
    minutes while tasks are open ("starved: 3 members idle, 0 ready, chain
    #401 <- #507 <- ...");
  - shown as plan warnings in validation, in status (a plan-health block) and
    as one message to the lead; the owner sees the same numbers in the GUI.
  Owner: yes, first, into the nearest phase (Permissions, with F10 once the
  fix-phase build is installed).
  Refinement (owner, 2026-10-26): health is computed per phase and for the
  plan separately. Today one block covers every task of the team, history
  included (79 tasks: "critical path 14" says nothing about the draft).
  - `phases[].health` in status and in each extend/edit-draft validation: only
    that phase's tasks (dependencies on earlier phases count as met): task
    count, dependency chain, critical path with each member doing one task
    at a time, parallelism, widest ready set, load per member, fan-in and
    the phase's warnings;
  - the plan's `health`: the same over every task not yet decided, across
    phases; decided work counts only in totals;
  - "starved" stays a live signal of the phase in work, not a draft's
    health (U9).
- **C8. Running-plan revisions, a last resort.** A good plan never needs it:
  C7 must catch the problem while planning. If a running plan is stuck anyway,
  the lead may propose a revision of not-started tasks of an approved phase
  (deps, text, assignee) with a preview diff, a cycle check and kept reports;
  it applies only after the owner explicitly approves it, every time; accepted
  work never changes. Zerus shows it to the owner as an exceptional step.
- **C9. Owner constraints in the graph.** An ordering rule of the owner
  ("publish only after local acceptance") is written as a dependency or gate,
  never only as text, so the cycle check catches it (the incident's cycle
  appeared only when the owner's rule was added).
- **C10. Why a hard dependency.** A hard dependency may name what the consumer
  needs (an artifact, a contract, a commit); one on a review/QA/acceptance task
  without it is a plan warning (C7).
- **C11. Not needed is not cancelled.** A conditional fix or retest can be
  closed as "not needed" with a reason; that satisfies hard dependencies
  without a fake Accepted, unlike a plain cancel.
- **C12. Time budgets for background work (owner).** A member (or its
  subagents) that starts a long background job - an install, a build, a full
  test run - declares how long it should take: `hgs team expect --json
  {request_id, team, label, minutes, red_minutes?}` (red defaults to twice the
  estimate). Zerus ties the budget to the member's live Background job and
  shows it on the member card and the task: normal until the estimate, amber
  after it, red after the red time ("install: 25 min, expected 15 - check it").
  At red the lead is told once, with the command and its last output lines,
  so it checks whether the job hangs or died. A Background job without a
  declared budget gets a default red time (the blocked timeout of #272, 30
  min). Budgets end when the job ends; they never stop anything themselves.
- **C6. Unblocking order.** When a member has several ready tasks, the
  dispatcher hands out first the one with the most dependants. Owner: yes, in
  Zerus's dispatcher, no model involved.

## D. Reports and acceptance

- **D1. A structured but flexible report.** Fields:
  - `summary` (text);
  - `commit` (optional sha the work was checked on);
  - `criteria`: per acceptance criterion `{n, result: pass|fail|partial|n/a,
    evidence, note}`;
  - `evidence` items of any kind: `command` (with exit code), `file`, `image`,
    `html`, `url`, `log`, `text`, each with a caption;
  - `limits`: what was not checked, what was mocked or a fixture.
  A plain text report is still accepted and marked "unstructured". The GUI
  shows images and HTML as previews. Owner: yes; must cover pictures and HTML
  designs, not only tests.
- **D2. Findings by severity, without blocking autonomous work.** Review and
  QA reports list findings as `critical`, `normal` or `later`.
  - `critical`: handled now, in one of three ways the lead chooses per
    finding:
    1. **Return the task to its member** (`reopen`): the accepted task that
       caused the defect gets a new attempt for the same member (pinned), with
       the finding as its message. Its earlier acceptance stays in the
       history. Its dependants that have not started wait again; dependants
       already decided are not reopened, so a retest is added explicitly.
    2. **Urgent phase**: `extend` with `"urgent": true`. It is appended at the
       end like any phase but runs at once (no owner approval even under
       `review`) and its tasks are handed out before other pending tasks.
    3. **Stop and replan**: the team goes to Paused (not Stopped, nothing is
       deleted): no new hand-outs, running attempts may finish. The lead sends
       the owner a replan proposal (cancellations, new phases, reopenings);
       the owner approves or changes it and resumes the team.
    Options 1 and 2 need no owner, so autonomous work continues; option 3 is
    the lead's way to say the plan itself is wrong.
  - `normal`: the lead decides: into the fix phase or into the deferred list.
  - `later`: goes to the team's deferred list. The owner is notified, but the
    team does not wait: the list is shown in the team header and in the finish
    summary, and the owner can, at any time, turn an item into a new phase or
    send it back to the member who did the work.
  - A critical finding cannot be deferred by the lead; only the owner can.
  - Acceptance of a review/QA task means "the report is good"; the team header
    shows open findings by severity, so a green chain cannot hide defects.
  Why it keeps autonomy: nothing waits for the owner except new scope under
  `review` and an explicit stop-and-replan; deferral is a list, not a gate.
  Risk: a lead may push work to `later` to finish faster; the finish summary
  must list every deferred item and the owner sees the count before the team
  is Done. Owner: yes, with the three critical options above.

## E. Planning and the built-in lead protocol

- The lead protocol (how to plan, task size, fix loops inside one task by
  rejection, phases only for newly found work, a short real user scenario
  after integration, permission rating from B5, report checks) ships inside
  Zerus, versioned with it, like `hgs guide` pages.
- Templates add a project addendum; they cannot replace the built-in part.
- Neither the user nor an agent changes the built-in part or the addendum
  silently: a change is shown as a diff and needs the owner's explicit
  confirmation.
- Owner: yes.

## F. Team creation

- **F1. Members land in General instead of their folder.** Seen in another
  project's team. Likely cause: the New Team dialog ticks "Worktree per
  member" for every git folder (`tray/src/TeamNewDialog.cpp`), a worktree
  member starts in a sibling folder with no project (`dispatcher.rs`
  `member_folder`, `launch_args`), and the GUI files a session without a group
  under General (`SessionOrganization.cpp` `observe()`). Owner: guard every
  scenario so it cannot happen again:
  1. *Group.* A member is filed in the team's project group, never in General:
     the dispatcher passes the team's project when it starts a member, a
     worktree folder resolves to its parent repository's project, and the GUI
     files any session that belongs to a team into the lead's group before
     any default.
  2. *Worktree default.* Nothing is a worktree by default (B6). A project may
     forbid worktrees; that setting wins over templates and the dialog.
  3. *Mismatch.* Team policy and member folders are checked against each other
     at plan, approve and add-member; a mismatch is an error, not a note.
  4. *Folder check before launch.* The member's folder exists, is the team's
     repository (same git common dir) or the team folder, and is on the
     expected branch; otherwise the member is not started and the lead and
     owner see why.
  5. *Fix in place.* Team settings (folder policy, member folders, roles) can
     be corrected while Planning, so a wrong setting never forces stopping and
     recreating the team.
  6. *Visible.* Team status and the members panel show each member's folder,
     project and branch; a member outside the team's project is flagged.
  7. *Leftovers.* Members of Done or Stopped teams that still run are listed
     (F7), and a removed or stopped team never leaves a member unlisted.
  8. *Tests* for each of 1-7, including a worktree team in a folder whose
     sibling folders belong to no project.
- **F2. Workspace policy:** as B6, auto by default.
- **F3. The designer checks roles.** The team designer may check each role
  prompt against the goal and the team protocol and correct contradictions,
  always telling the owner what it changed. Needs a mock-up.
- **F4. A dedicated Team designer session.** Created in General, fixed name
  "Team designer" and its own colour so nobody works in it, small context
  (about 256k where the agent allows it), with the built-in guides
  (`hgs guide designer`, `team`, `team-lead`) as its reference.
- **F5. New Team: the chat is with the lead.** Planning the team is a talk
  with the future lead, so the New Team chat is the lead's session; the Team
  designer stays for templates and the catalog. Proposed flow:
  1. *Open.* New Team shows the form and, in the other pane, the lead's
     agent, model and account (from the template) with a "Start planning"
     button. Opening the tab creates nothing.
  2. *Start planning.* Creates a durable **team draft** (a new team status
     before Planning) and starts the lead session in the team folder, never in
     a worktree. The lead gets the built-in lead protocol (E), the designer
     guide and the draft id.
  3. *Talk and fill.* The lead edits the draft with `hgs team draft --json`
     (name, goal, protocol addendum, members and roles, role prompts,
     permission profile, folder policy, approval). Each update carries the
     draft revision; the form updates live and marks fields the lead changed
     until the owner has seen them. The owner edits the form directly; an
     owner edit wins and the lead is told. The lead may also draft the plan,
     so the team starts Planning with the plan ready for approval.
  4. *Create.* Runs the preflight (F1, B3, B5), creates worktrees for the
     members that need them, starts the members. The draft becomes the team;
     the same lead session leads it.
  5. *Leave or discard.* Closing the tab keeps the draft (listed under
     Teams as Draft) and the lead session. Discard asks for confirmation and
     whether to stop the lead session.
  6. *Existing session as lead* works the same way: the chosen session
     becomes the draft's lead.
  Layout: settings in one column (compact template list with search, name,
  folder, folder policy, lead, members table, permission profile per role,
  approval and phase approval, protocol addendum folded), the other column
  holds the lead with tabs Chat, Goal and Plan; a narrow window shows the two
  columns as tabs. CLAUDE.md change: say that opening New Team creates
  nothing, "Start planning" creates a draft and a lead session only, and
  worktrees are created only on Create. Needs a mock-up before work starts.

  Sketch (2026-10-25):

  ```
  +-- New team ------------------------------------------------------------------------------+
  | SETTINGS                                 | LEAD   [Chat] [Goal] [Plan]                     |
  |                                          |                                               |
  | Template  [search.........] [*fav]      | (before start)                                |
  |  > Feature delivery   4 members         |   Lead  [Claude Code v] [Opus v] [high v]     |
  |    Bugfix             2 members         |         [Default account v]                   |
  |    Dev Team           5 members  *      |         ( ) existing session [.......... v]    |
  |    No template                          |                                               |
  |                                          |           [ Start planning ]                  |
  | Name     [Checkout redesign       ] *    |   Nothing is created until you press it.      |
  | Folder   [D:\Work\Shop        ] [...]    |                                               |
  | Folders  (o) auto  ( ) shared only       | (after start)                                 |
  |          project forbids worktrees: no   |   Lead: I read the template. Two coders share |
  |                                          |   role "Coder"; I renamed them Backend and    |
  | MEMBERS                        3 / 6 [+] |   Frontend so tasks don't cross. QA needs a   |
  |  Role      Agent    Model     Folder Prm |   worktree to build. OK?                      |
  |  Backend * Codex    gpt-5.5   wt     B+C |   You: yes, and no designer.                  |
  |  Frontend  dsh      flash     wt     B+C |   Lead: done - removed Designer (see Members).|
  |  QA        Codex    sol       wt     B   |                                               |
  |  ! QA prompt says "commit"; protocol says |  * = changed by the lead, not yet seen       |
  |    only the lead commits. [Show] [Fix]   |                                               |
  |                                          |  [Message the lead...................] [Send] |
  | Permissions  base: from template  [Edit] |                                               |
  |   widened by lead: QA +network  ! confirm|                                               |
  | Approval  plan [review v] phases [fixes_auto v]                                          |
  | Protocol addendum  (folded) [v]          |                                               |
  |------------------------------------------+-----------------------------------------------|
  | Draft saved 12:41 - rev 7      Preflight: ok 5  ! 1        [Discard draft]  [Create team] |
  +------------------------------------------------------------------------------------------+
  ```
  Legend: `*` a field the lead changed that the owner has not seen yet;
  `wt` worktree, `sh` shared folder; `Prm` permission profile (B build and
  test, C commit via Zerus); `!` a preflight warning with its fix. Goal and
  Plan tabs show the draft's goal and plan as the lead fills them; the owner
  may edit both. A narrow window shows SETTINGS and LEAD as two tabs.
- **F6. Many templates.** A compact list with search and favourites; the big
  card only for the selected template.
- **F7. Members after Done.** Not stopping them: the owner may want the same
  lead and members, with hot context, for the next goal. Proposal: a Done team
  can start a new goal with the same sessions ("next goal": a new plan as
  phase 1 of a new team that reuses the sessions), and the GUI shows members of
  Done teams that are still running so the owner may stop them by hand.
  Owner: yes.
- **F8. Change the team between phases.** After a phase the owner and the lead
  review the work and may want other members. While the team waits for the
  lead (all tasks decided, not finished) it is Running, so `add-member` and
  `extend` already work in the new build. Missing:
  - the lead removes a member on the owner's request (`remove-member` is
    owner-only today): the lead proposes, the owner confirms in Zerus;
  - a member is replaced by another agent or model (H6, `swap-member`);
  - a Done team is reopened for a next goal with the same sessions (F7), so a
    finished team is not a dead end;
  - an existing session may join as a member (today a new member always
    starts its own session), keeping its context.
  Every membership change by the lead needs the owner's confirmation.
- **F9. Roadmap of draft phases.** The owner wants to see every planned phase
  from the start, as drafts, and approve each strictly after the previous one
  ends. Today only one draft phase may exist and `extend` is refused while
  Planning. Proposal: the lead may queue several draft phases in order (in the
  plan or by `extend` in any active status); `approve-phase N` is refused
  until every earlier phase is approved and decided; discarding one draft
  keeps the later ones. Owner: yes, in phase 1 of the next team.
- **F11. Deferred from the phase-1 re-review.** (1) A guard against stale
  draft task ids after a renumber: draft-task operations carry the team
  revision they read, refused when a phase_renumbered came after it. (2) L9
  rest: a Paused team's lead messages are coalesced too.
- **F12. Approval in order, said plainly (owner).** Phases run 1 -> 2 -> 3 ->
  4; the owner cannot approve a phase while the one before it is not ready.
  The desktop says just that on the waiting draft: "Waiting for the previous
  phase to be ready" (the detail - which phase, how many tasks open - stays in
  the tooltip).
  Refinement (owner, 2026-10-26): the owner may approve the next draft while
  the previous phase still runs; the barrier keeps the order, so its tasks
  start only when every earlier phase is finished. Today Approve stays
  disabled until every earlier phase ends.
- **F13. The lead approves on the owner's explicit word (owner).** The lead may
  approve the plan or the next phase itself only when the owner said so
  explicitly in the lead's session, with a key phrase: "план принят" / "plan
  approved" or "план на следующую фазу принят" / "next phase plan approved".
  The lead sends `hgs team approve-phase` (or `approve`) with
  `"owner_word": {"text": "<the owner's exact sentence>", "at": <time>}`. Zerus
  checks that a message typed by the owner in the lead's own session (not a
  team message, a pasted block or an agent's text) contains that sentence and a
  key phrase, and that it is recent (for example 30 min); otherwise it refuses.
  The event and the GUI say "Approved by Lead on your word" with the quote. The
  ordering rule (F12) still applies: the lead cannot open a phase whose
  predecessor is not ready.
- **F10. Editable drafts.** Seen in use: an urgent fix could not be put before
  the queued drafts (a new phase always goes last), a draft cannot take one more
  task, and only the owner may discard. Proposal: until a draft is approved the
  lead may add, change or remove its tasks, insert a phase before other drafts
  and discard its own drafts; every change shows to the owner as a diff.
  Owner: yes, urgent (in the fix phase), with a redesign of phase approval
  (G5): the draft strips move to the top of the members column, compact,
  collapsible, and the Members block becomes collapsible too.
  Numbers: a draft that was never approved burns no phase number or task id;
  discarding or moving drafts renumbers the drafts after it, and a stored
  team with discarded drafts is compacted once. Owner: yes, urgent.

## G. GUI

- **G1. Hierarchy** (Prompt 2) stays the next large step; our own work still
  runs on the installed Zerus without phases.
- **G2.** Show the task number on each member (exists) and make working vs
  idle vs waiting clearly visible (icons from A1).
- **G3. Graph order inside a column.** A dead-end task (no task depends on
  it) goes to the bottom of its column, below tasks whose chains continue,
  so the edges of the chain stay short and the eye follows the path.
  Owner: yes, in phase 2.
- **G4. Bug: layer stripes vanish once a team has phases.** With any second
  phase (a draft is enough) the graph is drawn in phase bands, and banded
  mode paints only one background per phase: the alternating layer stripes
  and the "LAYER n - count" labels of the unbanded graph disappear inside
  phase 1 (`tray/src/TeamWidgets.h`, paint: `if (banded()) { ... } else if
  (m_mode == Wide) { layer stripes }`). Fix: inside each band, keep the
  alternating stripes per layer (and a small layer label). Owner: report,
  goes into the fix phase after phase 1.

- **G7. Session services are not background work (owner).** MCP servers and
  similar services (socraticode, server-pdf) start with a session and live as
  long as it, sometimes longer; today they show as Background, as if a long
  job ran. Show them as a small services icon on the session (tooltip: names,
  since when, still running after the session ended), not in Background tasks
  and not as the Background status. They must not count as Background work
  for the team capture gate (A2), the blocked reminder (#272) or the time
  budgets (C12): otherwise a member with MCP servers is never captured and
  always looks busy. Classify by the process tree and command line (an MCP
  server is started by the agent's MCP config), with a fixture per agent.
  CLAUDE.md fixes the Background status contract across desktop, Android,
  shared fixtures and docs: this is the owner's explicit direction to change
  it; keep all of them aligned. Owner: yes, first, with C7 in the nearest phase.

- **G8. The team's two bars count the right things (owner, urgent).** Seen
  2026-10-26: the session row showed "32/64" tasks and "0/1" phases. Rules:
  - the tasks bar counts only the phase in work (approved, with open tasks);
    with none in work, the next phase after the last finished one (the first
    draft); never tasks of several phases;
  - the phases bar is finished / all: every phase that is not discarded counts,
    phase 1, approved and drafts alike; finished means approved with every task
    decided. This team after phase 31: 2/5.
  Same numbers in the session row, the Teams panel and the workspace header,
  with a tooltip naming the phase. Owner: yes, as soon as possible.

- **G9. Phases stacked vertically in the graph, each with its status,
  decision and health (owner, 2026-10-26).** The workspace graph lays phase
  bands side by side; a long plan becomes a wide strip. Instead:
  - phases are stacked top to bottom, one band per phase, its layers left to
    right inside it (G3/G4 stay inside the band);
  - every band collapses to one compact header row (finished phases start
    collapsed; the choice is remembered per team);
  - the header carries the phase: number and title, a status chip - Done,
    In work, Approved (not started), Draft, Paused - progress done/total, the
    phase health from C7 (critical path, parallelism, warnings; the details
    in a tooltip) and, for a draft, Approve and Discard (disabled with the
    F12 reason while an earlier phase is not ready); every decision keeps
    its confirmation;
  - this replaces the draft strips above the members column (G5); the
    list and panel views get the same header.
  Owner: yes.

## H. Operations

- **H1. Resume after usage limits.** A flag on the lead (or session): when a
  5-hour or weekly limit resets, Zerus continues the sessions that stopped on
  it.
- **H2. JSON input that survives Windows shells.** `--json-file PATH` and
  tolerance of a UTF-8 BOM on stdin (PowerShell 5.1 adds one, and piping turns
  Cyrillic into "?").
- **H3. Shared dsh host.** All DeepSeek members share one host process; one
  crash stops every coder. Show it in team status; consider one host per team.
- **H4. dsh `session-log-deepseek`** uploads whole session logs to DeepSeek.
  Make sure Zerus never enables it.
- **H5. Every hand-out text names the question route** (A3) and the report
  command.
- **H6. Swap a member's agent or model.** When a model's limits run out the
  work must not stop for a week. `hgs team swap-member --json {request_id,
  team, member, agent, model, account}`: Zerus starts the new session (the
  context is lost), leaves the old one running for the owner to stop, and
  hands the current task again with everything needed to continue: task text,
  criteria, "From dependencies", the member's folder and branch, the commits
  and uncommitted changes since the attempt started, the previous attempt's
  partial report and open questions. Only the owner confirms a swap: the lead
  may propose one (`hgs team propose-swap`, shown to the owner with the reason
  and the target agent and model), but never swaps silently on its own. Zerus
  also shows a member stopped by a usage limit (with the reset time when
  known) in the lead's inbox. Owner: yes.
- **H7. Session identity after a resume (urgent, seen 2026-10-26).** After the
  reboot the lead session claude/Zerus/TeamUpdate was resumed and Claude Code
  continued in a new transcript (78521e2d-...) while Zerus still tracks the old
  conversation bfa1e069 (its transcript stopped at 00:10). Zerus therefore
  reads stale usage: the prompt cache was last observed at 23:49 with a 1 h
  TTL, so from 00:49 the session shows "Cold cache" and the send dialog warns
  although the session works with a warm cache (568k tokens read from cache
  on every request). The same stale binding can feed inspect, last reply
  capture and the team's lead identity. Fix: follow the conversation the agent
  actually writes after resume or compaction (the SessionStart/PostCompact hook
  reports it; a newer transcript of the same session in the project folder is a
  fallback), record the change as an event, re-bind team members, and treat a
  usage observation older than the last turn as unknown, never as cold.
  Owner: yes, as soon as possible.

- **U3. A resume must not restore an older conversation (urgent, seen 2026-10-26).**
  After a reboot, Resume restored this lead's conversation as of 23:49 the day
  before: the native transcript held nothing of the next run (00:09-04:30),
  while Zerus's own events held all of it. Before a resume, compare the
  transcript's last turn with the session's last recorded event; when the
  transcript is behind, do not resume silently: warn and offer the missing
  dialogue from the events. Owner: yes, in the next phase.

- **U4. Members stopped by a reboot stay stopped (seen 2026-10-26).** After the
  reboot, Backend, Frontend, QA and Reviewer were stopped; their ready tasks
  (#288, #292) stayed Pending with no word to the lead, until the lead resumed
  the sessions by hand. The dispatcher must notice a member whose session is
  stopped while it has a ready or open task, tell the lead once, and resume it
  (hgs resume -d) when the team setting allows it (default on for team
  members; never for the lead's own session). Owner: to be confirmed.

- **U5. Work continues without the owner as a watchdog (seen 2026-10-26, our
  team and a Codex lead in team 351555ed).** Three cases of one gap, all on the
  installed fa6ff80b:
  (1) a lead's next own task stayed ready with no attempt while the lead was
  active, until the owner asked "what are we waiting for"; (2) after a reject
  the member went on working on the reason in the same turn, the new attempt
  went out only when it stopped, and its report on the rejected attempt was
  dropped silently (#300); (3) a member ended its turn on its time budget
  without a report (#301) and nobody was told, so the team stood still.
  The phase-32 build covers part of it (C1 hands the lead's task out without
  waiting for an idle lead, C2 lists stalled members, A2 captures the last
  reply). Still missing:
  - status names why a ready task has no attempt (`ready_without_attempt`:
    member busy, in Background, needs settings, refused recently, session
    stopped), and the lead's inbox shows it after N minutes;
  - a reject with a reason opens the next attempt at once for the same member
    (the reason is its hand-out), or a report on the just-rejected attempt is
    taken for the next one instead of being dropped;
  - the lead (not only the owner) may use `use-last-reply` for a member that
    stopped without a report;
  - on the installed fa6ff80b, member permission requests and reports often
    never reached the lead as messages (QA waited 30-45 min three times) and
    the lead polled `hgs inspect`; the phase-32 inbox (C2) should fix it,
    to be verified after the install;
  - acceptance scenario: two consecutive lead tasks, the first accepted during
    an active turn; the second gets exactly one attempt and starts without an
    owner message; again with a restart between accept and hand-out; a real
    owner approval still waits. Owner: to be confirmed.

- **U6. A finished Background job continues its task (Codex lead, team
  351555ed).** A member's long job (an upload, a build) ended while its
  attempt had no report and the member was idle; nothing told it. When a
  Background job of a member with an open attempt ends, Zerus sends the member
  one message with the command, exit status and the last output lines, so the
  task goes on; the same check runs after a session restart (U4). The job keeps
  its identity across ticks (pid with start time, as C12 already ties a
  budget). Not Zerus's: the incident's 600 s kill came from the agent's own
  helper (`subprocess.run(timeout=600)`), and "outcome unknown, check the
  remote before retrying a PUT" is the publishing script's logic; Zerus cannot
  see a helper's internal timeouts. Owner: to be confirmed.

- **U7. Members of one role are told apart (seen 2026-10-26).** Two members
  with the role Backend (backend, backend2) both appear as "from: Backend" in
  hand-outs, reports and the inbox. Messages and cards name the member id, or
  a numbered role ("Backend 2"), whenever the role is not unique.
  Owner: to be confirmed.

- **U8. Moving the lead or a member to another account (seen 2026-10-26).**
  The Claude account of the lead and the reviewer reached its weekly limit.
  A running team cannot bind a member to a new session: membership follows
  the session name, so the new lead session took over only after the old
  session was renamed away and the new one given its name. A replacement
  reviewer could not be added (the team already had its 6 members and
  `max_members` is fixed after Planning), and `remove-member` is the owner's
  and refuses a member with draft tasks. H6 (swap-member) must also cover
  the lead's own session (same lead id, open task and inbox), launch on the
  chosen account, and not count the replaced member against `max_members`.
  Owner: to be confirmed.

- **U9. Members do not flood the lead before a phase is approved (owner,
  2026-10-26).** While every open task is in a draft phase waiting for the
  owner's approval, Zerus sends the lead no "starved" notices (seen: one per
  member, each after 20 min, for 22 open tasks): a member with nothing ready
  because the next phase is not approved yet is expected, and the lead cannot
  act on it. Status and the desktop still show that the draft waits for the
  owner. Starved notices start again only once a phase is approved and a
  member still has no ready task. Owner: yes.

- **U10. The desktop crashes when a tab shows a terminal (seen 2026-10-26,
  build 17a8db61).** A tab switch shows an embedded terminal while it is still
  narrow; `TerminalScreen::updateGrid` resizes it (down to 2x2), and libvterm
  0.3.3 aborts ("screen_resize failed to update cursor position") when a
  wrapped line becomes taller than the screen with the cursor on its last row
  (`resize_buffer`, `old_cursor.row < old_row_end` should be `<=`). Fix the
  library (local patch, noted in UPSTREAM.md) and keep the terminal's size
  while it is hidden or tiny. Owner: yes, in the fixes phase after phase 35.

- **U13. Expert and ProExpert take over work that keeps coming back (owner,
  2026-10-26).** Two catalog roles: Expert (Claude Opus 5.5, effort high) and
  ProExpert (Claude Opus 5.5, effort xhigh). When a task has been rejected
  three times, the lead judges the cause: a small remaining fix stays with its
  member; a gap beyond the member's model goes to the Expert (reassign, the
  Expert merges the member's branch and continues from it). The ProExpert is
  added only when the Expert could not finish it either. After an expert's
  takeover is accepted, Zerus compacts that expert's context
  (`hgs compact-context`) so the next takeover starts small. Zerus support:
  the third rejection shows the lead this choice in the inbox (with the
  rejection reasons), an expert member does not count against
  `max_members` or may be added to a full team with the owner's confirmation,
  and the compaction runs on acceptance of a task that was reassigned to a
  member tagged `rework`. Owner: yes.

- **U14. Catalog decisions on the owner's word (owner, 2026-10-26).** The owner
  asked the lead to apply two role proposals while the desktop could not be
  used; catalog decide/apply accept only "by":"human" from outside any agent
  session, so the lead had to hand the owner a script. As F13 does for phases:
  `catalog decide` and `apply` from the lead's own session accept
  `"owner_word": {"text"}` when the owner typed that sentence there within 30
  minutes ("примени изменения каталога" / "apply the catalog changes"); the
  draft and the event say "Applied by Lead on your word" with the quote.
  Owner: yes.

- **U15. Catalog proposals are decided without a designer session (seen
  2026-10-26).** Keep, Reject and Apply did nothing while the Team designer
  session was not active; the designer page drops the result and the error of
  `catalog decide`. They must work in any session state and show errors.
  Owner: yes, in the fixes phase.

- **U16. The Team designer starts with what it needs (owner, 2026-10-26).** Its
  session stopped at Claude's folder-trust prompt in its own Zerus folder, and
  the prompt showed only in the main sessions list. Zerus trusts that private
  folder and writes narrow rules (catalog get/propose/route, guides, read-only
  tools) at the designer's launch, never by answering prompts; start-up trust
  stays manual for every other folder. The designer window shows its
  session's questions and permission cards. Owner: yes, in the fixes phase.

- **U17. Permission answers collapse to one line (owner, 2026-10-26).** Member
  cards list every stored permission answer (up to 20 lines, deny reasons in
  full) and grow the members column. One summary line ('18 allowed · 2 denied ·
  last: ...'), expandable to the newest five; the task card the same for its
  attempt. Owner: yes, in the fixes phase.

- **U18. The lead's inbox does not wait for the lead to be fully idle (owner,
  2026-10-26).** Three member permission requests waited about 30 minutes while
  the lead's own subagent ran: the notifier and reminders need activity idle.
  Deliver to a lead whose turn ended also while its own background work runs;
  a permission unanswered for remind_owner_minutes goes to the owner's
  attention and notification, answerable there. Owner: yes, in the fixes phase.

- **U19. Confirm subagents (owner, 2026-10-27).** A session may start subagents
  on its own and the owner has no say. A global setting in Settings > Sessions,
  'Ask before an agent starts subagents', ON by default: Zerus's pre-tool hook
  asks before a subagent starts, one request per turn listing how many, which
  type, model and effort; never auto-approved; a team member's request goes to
  its lead. Agents that cannot be asked are marked, never blocked.
  Owner: yes, in the fixes phase.

- **U20. Sandboxed members can report and ask (seen 2026-10-27).** A DeepSeek
  member's hgs team report/ask failed with 'attempt to write a readonly
  database'; its report stayed a chat message and A2 did not capture it, so the
  task stood for two hours. Owner: yes, in the fixes phase.

- **U21. Command templates instead of escalations (owner, 2026-10-27).** 133
  sandbox escalations in 6 hours: 124 routine (46 git commit/merge, 78 Qt
  build/test), 9 needing judgment (all 5 denials). A built-in template set,
  shipped with Zerus and on for every team (no approval per plan; a global
  switch turns it off, projects may add templates): typed, allowlisted
  arguments, run by Zerus outside the sandbox in the member's folder (hgs team
  run); clones for DeepSeek members remove the git escalations. Expected: about
  9 requests instead of 133. No automatic approval of native prompts.
  Owner: yes, in the fixes phase.

- **U22. DeepSeek session-log upload (found 2026-10-27).** 18 of 21 dsh sessions
  upload their transcripts to DeepSeek (session-log-deepseek). Settings >
  Sessions 'Block DeepSeek session-log upload', on by default: Zerus starts its
  dsh sessions with it off, verifies it and marks a session that still
  uploads. Owner: yes, in the fixes phase.

- **U23. Stop the trial-and-error loops (owner, 2026-10-27).** DeepSeek flash
  members spent hours in build-run-edit cycles (QA on #356 fixed its tests
  around a bug in its own widget code; frontend on #343 ran out of context), and
  the lead approved each cycle without seeing that the task stood still.
  (1) Loop detection: Zerus notices the same failing test or the same build
  error across N runs (template runs or escalations) of one attempt and tells
  the lead once with the repeated failure, so the lead looks at the cause or
  hands the task to the Expert (U13). (2) Role guard: plan validation warns
  when a QA or Reviewer member gets an implementation task, and when one member
  both implements a task and reviews or tests it; the lead's reassign shows
  the same warning. (3) Model routing: the designer and the lead protocol give
  GUI work and hard debugging to stronger models and keep the light DeepSeek
  model for well-specified backend work; templates carry that default.
  Owner: yes.

- **U24. A starved notice only when the lead can act (owner, 2026-10-27).** The
  'no ready task for 20 min' message kept arriving for members who had nothing
  they could take: every open task was assigned to another member (the Expert)
  or waited for it, and the lead had already said there was no work for them.
  Send it only when the lead can do something: a ready or pending task of the
  member's role is held by a busy member and could be reassigned, or the plan
  has no task the member can ever get (suggest finishing or removing it). Once
  per member and episode, not per 20 minutes; the lead may mute it for a member
  ('expected idle until #N'). U9 stays for drafts. Owner: yes.

- **U25. New UI from the owner's design (owner, 2026-10-27).** The designer's
  clickable prototype and its spec are the source of truth for look and
  behaviour: `D:\Tools\Zerus-UI-handoff\Zerus-prototype.html`
  (opens offline; the Prototype button switches Running / Stuck / Done /
  Planning and the theme) and `SPEC.md` with 52 screenshots in `shots` (sections 0-14, with an
  acceptance checklist in 13; screenshots follow). UI strings are English,
  exactly as in the prototype; colours and sizes go into the skin tokens, never
  hard-coded. Pieces:
  (1) Skin: the prototype's tokens in both themes, the new role colours
  (secretary, coder, qa, designer, leadgpt, expert, proexpert; a role's colour
  comes from the catalog), thin 8 px scroll bars, the real gear and bell-off
  icons, one collapsible block pattern remembered in QSettings, and the
  'agent works' dots under the model icon (static when animations are off).
  (2) Session list: '+' per project, git / background / services chips with
  counts in row 2 (the old cloud goes), team members muted by default with
  bell-off and a grey edge, the lead's team strip without the name, the
  member tree with its lines (spec 2).
  (3) The lead's session header carries the team status, phase/tasks/phases
  progress, time and the team actions; the Team tab's name bar goes. The
  composer row has three zones (spec 3, 4).
  (4) Team tab: phase scrolls in Graph, Board and Layers with Accept /
  Reject (with a comment the lead reworks) / Undo / Auto-accept drafts; the
  right column's Needs your answer, Team stuck and Plan problems collapsed by
  default; Members as a tree with collapsed task lists (spec 5.1-5.3, 5.5,
  5.7). The server adds the phase states rejected and queued, the owner's
  send-back comment, Undo and the team's auto-accept of error-free drafts.
  (5) Graph: two-line nodes without a status word, the working indicator at
  the agent, orthogonal edge routing around nodes with corridors, edge editing
  in planning and drafts (spec 5.4).
  (6) Inspector: one column of collapsible cards instead of tabs (spec 6).
  (7) Projects, Machines (editable connection form with Discard, Refresh,
  Phones), Agent accounts (machine filter, signed in/out) (spec 7-9).
  (8) New session dialog 'B - Dense, labels left' (spec 10).
  (9) Team designer and an editable catalog: roles with prompt order,
  templates with roster counts and 'only if needed', routing with Try it,
  Save / Revert, the same widget in Settings > Teams; the catalog model of
  spec 12 on the server (spec 11, 12).
  (10) A sub-lead inside a team (spec 5.6): `members[].parent` and `sub`; a
  sub-lead hands out, reviews and accepts its reports' tasks, the lead reviews
  only its direct reports. Sub-teams (Hierarchy, Prompt 2) stay for whole
  delegated tasks. In phase 36 with the review fixes. Owner: yes.

- **U26. Team roster changes only on the owner's explicit word (owner,
  2026-10-27).** The owner asked the lead to put frontend on Opus and add a
  backend and an Expert; swap-member, remove-member and max_members are
  owner-only, so the lead had to hand the owner a script. As F13 and U14 do:
  `add-member`, `swap-member`, `remove-member` and a `settings` change of
  `max_members` from the lead's own session are accepted only with
  `"owner_word": {"text"}` when the owner typed that request in the lead's
  session within 30 minutes, and only for what the sentence names (one
  verified word covers the changes of that one request). Without the word the
  lead proposes (F8) and changes nothing; add-member, which the lead may run
  today, needs the word too. The event and status say "Changed by Lead on your
  word" with the quote. Owner: yes.

- **U27. Visual lanes for a readable plan (owner, 2026-10-27).** The plan's
  layers come only from dependencies, so a phase without many dependencies is
  one endless first layer; the lead added soft dependencies just to make the
  graph readable. A task gets an optional `lane` (a number with an optional
  label, such as "Checkpoint 1") that the lead or the owner sets in the plan
  and in drafts; Graph, Board and Layers group a phase's tasks into its lanes,
  in order, each lane a labelled row that can collapse (lanes without a value
  fall back to the dependency layer). Lanes never change hand-out: only
  dependencies, phases and barriers do. Status and the desktop show and edit
  them (drag a task to another lane, rename a lane). Owner: yes.

## Order of work (proposal)

1. Delivery and reports: A1-A6, H5.
2. Permissions without noise: B1, B2, B4, then B3, B5, B6.
3. Lead load: C1-C4, C6; then C5.
4. Team creation: F1, F2/B6, F4, F5, F6, F3.
5. Swap member and limits: H6, H1; H2-H4 alongside.
6. Reports and findings: D1, then D2 (reopen, urgent phase, stop and replan,
   deferred list).
7. Built-in lead protocol (E) as each piece lands.
8. Hierarchy (Prompt 2).

## Open decisions for the owner

None. B5: the owner confirmed that Zerus writes the rule allowing
`hgs team answer` into each lead session's settings. F5: a quick sketch is in
F5; a designed mock-up still precedes the work.
