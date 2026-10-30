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
- **A7. A captured report holds its member.** Until a captured report is
  decided or replaced, its member gets no new task, rebalance or work ahead
  and does not starve. Owner: yes (2026-10-29). Spec: team delivery 7a.

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
  behaviour: the UI handoff's `Zerus-prototype.html`
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

- **U28. Planning rules for the lead (owner, 2026-10-27).** The built-in lead
  protocol gives the lead these rules for every plan; the designer and the
  plan health refer to them.
  Phases: (1) at most 20-25 tasks per phase, each ending with an
  install-ready build and NOTES.md; more work is split into phases.
  (2) Checkpoints (merge, then review and QA, in their own lanes) on demand:
  sometimes after one lane, sometimes after two or three. (3) Choose them by
  efficiency: batch the check when that is cheaper, check riskier work in
  smaller pieces. (4) Checkpoint findings go back to the same member before
  the integration; final review findings go to the next phase. (5) An
  approved plan is not reshaped; new work goes to the next phase's draft.
  Dependencies: (6) a hard dependency only where a task really cannot start
  earlier; one inside a lane splits it, so move that task to the next lane.
  (7) No dependencies on tasks of an earlier phase: the barrier waits for it.
  (8) Shared foundations first (the skin, a server model), at the start of
  the first lane, for a strong member.
  Load and models: (9) balance every lane, not only the phase, by the task's volume and its cost (complexity plus the cost of checking it: a Qt task with clean builds and the window suite weighs about three server tasks), never by the number of tasks. (10) GUI, hard logic and debugging go to strong models
  (Opus, Codex); well-specified server work, small fixes and well-specified GUI pages to DeepSeek.
  (11) Split a heavy task into its server and desktop parts. (12) QA never
  implements tasks; the reviewer never writes code it will review. (13) The
  Expert and ProExpert stay in reserve with no tasks: the Expert takes a task
  returned three times for a model weakness (not a small fix), the ProExpert
  only when the Expert fails; an expert's context is compacted after its work
  is accepted. (14) Before planning, ask the owner how to spread the members
  over accounts. (15) The lead plans only merges, integration and builds for
  itself, and runs no long background work in its session (its inbox waits
  meanwhile), except fixing critical findings at the end before the build.
  Task text: (16) self-contained: the plan item code, the paths of the spec,
  prototype and screenshots, what to change and the acceptance criteria.
  (17) Tests through the real entry point (the CLI or the dispatcher) that
  fail before the fix. (18) The checks named, with totals in the report,
  against the Rust baseline and the known flaky tests; a member runs each suite once and reruns only what failed, the repeated stability runs happen once, at the lead's integration. A member asks the lead (hgs team ask) before a run expected to take more than about ten minutes (the full test_sessionswindow, the full Rust suite, repeated GUI runs) and names the suite and why; the lead may move it to the integration or a separate test pass. The lead plans the test load with the work: it weighs each task's checks, the machine's load from the members' parallel builds and suites, and moves heavy suites out of the members' tasks into one pass (the integration or a test stage of its own) when that is cheaper; a member then runs only the suites of the files it changed. (19) Folders follow the
  agent and the need: DeepSeek members work best in a clone; sometimes all
  members share one folder or one memory. The lead plans it first, then asks
  the owner explicitly how it should work and explains why it chose that.
  (20) Builds and tests of DeepSeek members run through the built-in command
  templates or scripts in their own folder, not dozens of one-off
  permissions.
  Before sending the plan: (21) plan health: no errors, a sensible critical
  path, parallelism near the number of implementers, every lane balanced,
  every warning explained. (22) Offer a phase split above 25 tasks. (23) No
  starved notices before a phase is approved; a member's idleness is reported
  only when the lead can act. (24) The roster and the member limit change
  only on the owner's explicit word (U26); otherwise the lead proposes.
  The rules live in the team catalog, versioned like roles and templates:
  the built-in text is the default, the owner edits it by hand and the
  designer agent proposes changes that the owner keeps or rejects. The Team
  designer shows them in their own tab, Planning rules. Owner: yes.

- **U29. A task added to a running phase, on the owner's word twice (users,
  2026-10-27).** A phase stays fixed once approved, but sometimes a critical
  fix or a forgotten task belongs in it (phase 36's checkpoint fixes had to go
  to a new phase because a running phase takes no tasks). An `add-to-phase`
  operation adds tasks to a running phase only when: (1) the owner explicitly
  asked to add a task to the running phase (an owner_word from the lead's
  session, U26 rules, naming the phase or "the current phase"); (2) Zerus then
  answers with a warning the lead must show the owner: the load balance of the
  lanes shifts and members may sit idle, because members do not work ahead
  and later phases are not approved; (3) the owner confirms again in text, a
  second word newer than the warning, before anything changes (the desktop
  asks the same: the warning, then the owner types the confirmation). Added
  tasks carry `added_late` (by, at, the quotes) and show an "out of the
  phase's original scope" icon in Graph, Board, Layers and the task card; the
  phase's health and the lane balance are recomputed and the change is in the
  feed. Owner: yes.

- **U30. Six session states (owner, 2026-10-27; the owner's explicit
  direction for the status contract).** The design is
  the UI handoff's `Status/SESSION-STATES.md` with 13 shots. A
  session is Working (the turn runs, with its timer), Waiting for you (an
  approval, an input, a reply to read, or the lead's "Needs you"), Waiting
  for dependencies (always with the reason: subagents by name, a long
  operation with its command and duration, a usage limit, the phase or task
  or role a team member waits for, a lead's reports), Cooling (done, the
  prompt cache still warm, a countdown and a heat bar), Cold (done, the cache
  expired: "+ New" opens New session prefilled, "Resume" keeps the session)
  and Stopped (by you); Offline stays. The order of computing them and the
  tokens are in that file; the cache TTL comes from the agent (Claude 5 min
  or 1 h, Keep cache warm extends it). A background job makes a session Wait
  for dependencies only when the session waits for it: a tool call still
  running past the threshold, subagents in flight, or a job the agent waits on
  (hgs wait, a declared budget, a turn blocked on it). A long-lived process the
  session started and does not wait for - a dev server, a watcher, a tunnel,
  an MCP server (U11) - is a service: it never makes the session wait, the
  session is Cooling or Cold by its idle time, the row-2 chip counts it, and
  it never defers the reply badge. A job that outlives its declared budget, or
  runs on after the turn ended with nothing waiting on it, counts as a
  service. Of the former Background rules the rest stay: such a session stays
  in Active and accepts new messages, native work and explicit attention take
  precedence, a native Stop does not prove the jobs finished, and historical,
  unknown or offline jobs never make a session wait. Desktop, Android, the shared fixtures and
  the docs (CLAUDE.md and AGENTS.md included) change together. Owner: yes.

  Addition (owner, 2026-10-27): a seventh state, Build. A build or long test a session starts through Zerus (hgs team run templates, or hgs run for any session) is tracked by Zerus and shows Build with its label, command and time, right after Working; an idle agent with a Zerus build running is never Cooling or Cold, and its reply badge waits. Other unbudgeted background jobs stay services (dev servers, watchers, MCP). Agents start builds through Zerus so the owner does not ping a session that is still working. While a session is in Build, the desktop keeps its context warm with a keepalive every 30 minutes until the build ends.
- **U31. Work ahead (owner, 2026-10-27).** In phase 37 the DeepSeek
  backends finished while the Claude members still worked the heavy GUI
  fixes, and they had nothing to take: the next phase was not approved. A
  team setting work_ahead (off by default, only the owner sets it) lets an
  idle member take a ready task of its own role from the next draft phase
  when that task depends on nothing in the running phase and its files are
  not being changed there; the task is marked as worked ahead, its result
  waits as a report until the owner approves that phase (a rejected or
  discarded phase discards it), and the lead's inbox says so. The desktop
  shows the setting in the team settings and the mark on the task. Owner:
  yes.

- **U32. Adding a member on the owner's plain request (owner, 2026-10-27).**
  The owner asked "maybe add one more DeepSeek backend?" and still had to run
  a script: the installed build lets only the owner add members and raise
  max_members, and U26's check would also refuse that sentence, because it
  must name the new member's id ("backend4") and the new cap ("10"), which the
  owner never types. On the owner's explicit request in the lead's session
  (U26 rules: typed by the owner, at most 30 minutes old), the lead's
  `add-member` is accepted when the sentence names what is added: its role or
  its agent/model, in English or Russian ("backend"/"бэкенд",
  "frontend"/"фронт", "DeepSeek"/"дипсик", "Claude"/"клод", "Opus"/"опус",
  "Codex"/"кодекс"; the generated id is not needed), and the request says
  how many ("one more"/"еще одного" is one, otherwise one). When the add
  would exceed max_members, the same word raises the cap by exactly that
  many, within the tree session budget, and the event says so. One word
  covers only the members it names; a second add needs a new sentence.
  Remove and swap keep U26's rule of naming the member. The feed and status
  say "Added by Lead on your word" with the quote. Owner: yes.

- **U33. The Board scrolls instead of squeezing its columns (owner,
  2026-10-27).** The Board fits every column into the window: the phase
  scrolls turn the horizontal scroll bar off (TeamWorkspace.cpp
  phaseColumn) and the columns stretch with no minimum width
  (boardColumns), so on a narrow window the cards shrink to unreadable
  titles and the layout jumps as tasks move between columns. Each column
  gets a fixed readable width (about 240 px, the cards' two lines unelided
  up to it), the whole Board scrolls horizontally as one page (every phase's
  columns line up and move together), the thin bar of spec 1.2, and the
  horizontal position is kept across renders and polls like the vertical
  one. Graph and Layers keep their behaviour. Owner: yes.

- **U34. A DeepSeek member's new worktree is ready for its sandbox (owner,
  2026-10-27).** backend, backend2, qa and backend4 each started with every
  command failing: `SetNamedSecurityInfoW failed (Win32 5): grantWrite(<worktree>)`.
  The DeepSeek sandbox needs WRITE_DAC and WRITE_OWNER on the folder it
  confines. A folder on a second drive (such as D:\work) inherits the drive's
  default ACL: Authenticated Users Modify, no explicit right for the user, so
  the owner has WRITE_DAC but not WRITE_OWNER and the provisioning fails;
  under C:\Users\<user> the profile's explicit FullControl hides it. Each
  time the member asked to run its ACL repair script and the owner fixed the
  folder by hand. Zerus creates these worktrees itself (dispatcher.rs
  `git worktree add`), so on Windows it grants the signed-in user an
  explicit inheritable FullControl ACE on the folder it just created, before
  the member starts; for a folder it did not create (a direct folder, an
  existing worktree) a DeepSeek session start checks the effective rights
  and, when they are missing, the session and the lead's inbox say so with
  the one-line `icacls` fix for the owner instead of a failing sandbox. The
  member never repairs ACLs itself. Owner: yes.

- **U35. A second message to a working agent is not refused (owner,
  2026-10-27, screenshot).** The team's coordinator composer often refuses a
  message with "hgs: previous input has not been confirmed by the agent;
  inspect Terminal before sending again". The guard in src/state/input.rs
  holds every send until any native event arrives after the previous one
  (`input_pending_at` against `last_event_at`), so a lead that thinks for a
  minute without a tool call blocks the owner, and the error names a
  Terminal the team view does not show. The owner's second message is
  queued instead: hgs keeps it (bounded, in order, with its request id) and
  delivers it once the agent confirms the previous input, the composer says
  "queued until the lead takes your previous message" with Cancel; the
  refusal stays only where it protects a real terminal state (a dialog, an
  unknown prompt) and then offers Open terminal. The coordinator's quick-ask
  chips ("Summarize status", "Re-plan remaining work" and the planning and
  stuck pairs) are removed. Owner: yes.

- **U36. Review and test inside each task's pipeline (owner, 2026-10-28).**
  Today a task goes InProgress, Submitted, then the lead (or the member's
  sub-lead, `reviewer_of`) accepts or rejects it; review and QA are separate
  tasks at the end of a phase that read the whole integration, so findings
  arrive late, in one batch, and the lead relays each of them. A task now
  carries a pipeline: an ordered list of stages after the implementer's
  report, each with its kind and who does it.
  - **Stages.** `review` (a member reads the attempt's change against the
    task's criteria), `test` (a member runs the task's suites and scenarios,
    takes the criteria's shots) and `accept` (the lead, the sub-lead of
    spec 5.6, or `auto`). A stage names a role (the first free member with
    it takes the stage, rebalancing like tasks) or a pinned member; the
    implementer never takes its own task's stage. New task statuses
    `in_review` and `in_test` sit between Submitted and Accepted; the Board
    shows them as columns (with U33's widths), Android and the shared
    fixtures follow. Owner direction for this status change: yes.
  - **Verdicts.** A stage member reports with `hgs team report` and a
    verdict: `pass` moves the task to its next stage, `return` sends the
    findings to the implementer as a new attempt with them attached,
    without the lead. After a preset's maximum rounds (default 3) the task
    goes to the lead with every round's findings. The lead and the owner
    can still accept, reject, skip a stage or send the task back at any
    stage, each with a reason; the feed records who did it.
  - **Where a stage works.** The stage works in the task's own folder: the
    implementer's worktree or clone, or the shared folder when the members
    work in one folder without worktrees (a common case, not an exception).
    Zerus gives the stage member that path and the permissions its stage
    needs there: review reads, test also builds and writes only build
    output and its own temporary folder; nothing else outside its own
    folder. What changed comes from the attempt: its commits (B4) when the
    folder is a Git checkout, otherwise the report's changed-file list and
    the files' state at the report. While a task is in a stage that needs
    its files on disk (test), its folder is held: the implementer's next
    task in that folder waits and its card says "folder held by the test
    of #N". A preset may instead test on a detached checkout of the
    attempt's commits, which frees the implementer and costs one build.
    In a shared folder other members keep working; the stage hand-off says
    which files are the task's.
  - **Presets.** The team catalog gains pipelines: named presets ("Code:
    review, test, lead", "Docs: review, auto", "Check: lead", "None") with
    their stages, maximum rounds and folder rule. A template names its
    default pipeline and a role may name its own (a docs role uses Docs);
    the plan and `edit-draft` can set a task's pipeline; the task keeps a
    snapshot of it when it is assigned, so a later catalog edit does not
    change a running task.
  - **Team designer.** The catalog editor (Team designer page and Settings >
    Teams) gets a Pipelines tab next to Roles, Templates and Routing:
    the presets list, a stage editor (kind, role or member, folder rule,
    rounds, final acceptance), the default per template and per role, the
    designer's proposals with their keep or reject plates, and Try it on a
    sample task. Manual edits stay local until Save, like the other tabs.
  - **Load.** A stage queue per member is visible on the Board and in the
    member's card; a second member with the stage's role takes waiting
    stages. Test stages run only the suites of the files the task changed
    (U28 rule 18); the full suites stay one pass in the lead's integration,
    and a phase's own review and QA tasks become a short smoke check of
    the integrated build.
  Owner: yes, with the Pipelines tab and the task's folder for stages.

- **U37. How long a task has worked, on its node (owner, 2026-10-28).**
  A graph node, a Board card and a Layers row show the task's working time
  in small text right after its activity label (U25 part 5), in the same
  color and size: while it works, the time since the current attempt
  started, ticking once a minute ("12m", "1h 05m"); once it is done, the
  sum of its attempts' working spans and, with U36, its stage spans. Time
  waiting for dependencies, a free member or the lead is not counted. The
  tooltip breaks it down per attempt and stage, with the waits named
  separately. Status gives every task a `timing` object computed on the
  server (worked seconds, running since, waits), so the desktop, Android
  and the profile of U38 read one number. One timer for the whole view,
  no render per node. Owner: yes.

- **U38. A pipeline profile after the work (owner, 2026-10-28).** When a
  task, a phase or the plan is done, the owner and the lead see where the
  time and the tokens went and what was lost.
  - **Spans.** Each task's timeline is a list of spans computed from its
    attempts and events: waiting for dependencies, waiting for a free
    member, delivery and the start delay (A1), work, blocked on a question
    or a permission (who answered, how long), Background and Build (U30),
    waiting for the lead's decision, each U36 stage and round, rework
    (rejected attempts and returned rounds), a held folder, no report or a
    stop. Past phases get the same spans from the timestamps attempts
    already keep; what was never recorded shows as unknown, never guessed.
  - **Tokens.** Each attempt and stage gets the tokens and cost its member's
    session spent inside the span (the native accounting of usage.rs and
    the analytics measures `tokens`, `cost`, `work_ms`), per provider and
    model; a provider without counters shows unknown.
  - **Losses.** `hgs team profile` (a phase, a task or the plan; JSON,
    bounded pages) ranks the losses by time and by tokens: rework and its
    share of the task's tokens, waits on the lead, permission waits, idle
    members while tasks were open, blocked chains, long test runs, merges
    and conflicts; the planned critical path against the real one; per
    member, role and model the utilization and tokens per accepted task.
  - **Profile view.** The team workspace gains Profile beside Graph, Board
    and Layers: a timeline per member lane with the spans colored by kind,
    the real critical path marked, the top losses as a list linked to
    their tasks and spans, a tokens bar per task, and a compare of two
    phases. The Graph and Board can overlay the profile as a heat tint.
  - **Use.** The lead's phase report names the phase's top three losses,
    and planning (U28) reads the per-role numbers when it weighs lanes.
  Owner: yes.

- **U39. Bug: Auto-accept drafts approved drafts the owner did not mean
  (owner, 2026-10-28).** The owner switched Auto-accept drafts on to start
  phase 42; phases 43 and 44, drafts the owner had not approved, were
  approved with it and queued. The switch is team-wide and silent about what it
  starts. Fix: turning it on names the drafts it will approve and asks; an
  approval for one phase never approves another; the feed records each
  automatic approval with the switch that did it. Separately, an approved
  phase behind a running one is held whole (N6 of #499): the owner accepted the
  idle time this once, and the members were compacted before it.

- **U40. A plan score from 0 to 100 (owner, 2026-10-28).** "Healthy 14/15" in a
  phase header counts the tasks without a live problem; it says nothing about
  the plan, so every plan looked ideal. The plan and each phase get a score
  from 0 to 100 with its named penalties: the weight of each task against its
  member (size, criteria, files, the agent and model), the expected time from
  the profile's history (U38) and the critical path against the parallelism,
  the risk of a task (security, merges and migrations, a weak model on hard
  work, past rework of the role), the risk of locks and conflicts (tasks of
  parallel lanes touching the same files, shared folders, a long merge queue),
  the risk of idle members (queued phases, one-lane fan-ins, review and QA
  bottlenecks, long test runs) and the planning rules of U28. The header shows
  the score and the top penalty; the live count becomes "14/15 without
  problems". Owner: asked for.

- **U41. Safe actions (owner, 2026-10-28; done in 59e2818b).** The lead
  records the shape of a member's routine request once (`hgs team
  safe-action add`, `hgs guide team-safe`), in Zerus state, with no build.
  Zerus allows each later request of that shape itself: one one-time allow
  per question, reserved in the team's journal first and recorded "by safe
  action <id>"; a failure leaves it in the lead's inbox. Only the exact
  command the card carries matches (Claude Bash/PowerShell; DeepSeek shell
  calls on hosts started by a build with it); typed slots never take a shell
  metacharacter, an option or `..`, and `{path}` stays in the member's folder.
  Subagent cards and the lead's own requests stay manual. Later: a desktop
  list of the team's safe actions with Remove.

- **U42. Bugs seen in phase 40 (2026-10-28).**
  - A DeepSeek member's queued report (the sandbox route) stored the
    placeholder "poll" as #512's report and closed the attempt, so the full
    report was refused ("attempt ... does not run for this member") and the
    lead had to take it from the member's last message. A queued report must
    store its whole text or nothing, and a member must be able to replace a
    placeholder before the lead decides.
  - The Android app module did not compile since U30 (#509): SessionCardUi.kt
    used `20.sp` without its import, so QA could not run Android and nothing
    showed it (fixed in #535's c68c2e7b). With it compiling, four U30 tests
    fail on the merits: DesktopSessionPaletteTest (3, e.g. Attention expected,
    Neutral shown) and SessionStateContractTest (1: the reason of
    zerus-build-defers-the-reply). They need a fix with the contract. 27 more
    fail only on Windows (POSIX file permissions in AttachmentStore and the
    conversation caches) and one on a queue shutdown wait.
  - A plan edit (`plan`) recreates the phases it is given as Draft records with
    this edit's proposer and time, ordered by number: an order chosen with
    move-phase and the original proposer are lost (#548's residual). Phases
    that already exist keep their order, proposer and time.
  - `hgs team expect` from a DeepSeek member's sandbox is refused ("the sandbox
    blocks the team store") instead of being queued in the member's folder like
    a report, ask or commit, so a sandboxed member cannot declare a budget.
  - After the lead answers a permission with `hgs team answer`, the next
    question sync records the same question again as answered "by unknown":
    the answer leaves `pending` set. The answer clears it.
  - Review #511 of phase 40: C1 the U29 confirmation accepts any sentence
    containing "add"/"yes"/"добав" (a refusal "не добавляй" confirms); C2
    check_upload saves DeepSeek bindings without the bindings lock (a resume
    or rename can be reverted); C3 the owner check on `subagents-confirm` and
    `session-upload-block set` (still open from #499 C6). Also 11 normal
    findings (N1 a Zerus build is not Background in the capture gate; N2
    work-ahead ignores dependencies inside a draft phase; N5 a failed held
    message is lost silently; ...).

- **U43. A team member's mute on every device (for the owner).** A team member
  is muted by default only on the desktop that shows the team (#547 N36): the
  phone and other desktops keep notifying. Phase 42 makes the desktop's text
  honest ("Muted on this computer"). The shared fix makes hgs apply the team
  default itself: a member of an active team without a recorded choice reads
  muted in `notification_preferences`, so Android and every desktop follow.
  That changes the notification semantics and the mobile contract, so it waits
  for the owner's direction.

- **U44. Agent teams on the phone (found in #535).** Android has no agent-team
  view, and the mobile relay and connector carry no team status. Phase 42 adds
  only the Kotlin model and presentation of the pipeline stages, checked
  against the shared fixture. A team view on the phone (the graph or board, the
  lead's inbox, the stage chips) needs a team status API through the connector
  and relay first; it is a feature of its own for the owner to plan.

### U45-U52. Team customization for outside users (owner, 2026-10-28)

An outside user moving a pipeline of their own (a tracker, dispatch scripts,
pre-review gates, a landing queue) onto Zerus teams asked for these. Every one
is opt-in: a team, role or task that sets none of the new fields behaves
exactly as today, and the existing tests keep passing unchanged. The owner
sets them in the catalog, the team settings or the plan; members never do.

- **U45. A model per task, and a task for a role.** A task may name
  `models` (the models allowed for it) and `role` instead of an assignee. A
  member may list `models` it can switch between (`[{model, effort}]`;
  default: only its launch model). A task for a role goes to a free member of
  that role whose list allows one of the task's models; before the hand-out
  Zerus switches the member's model and effort through the agent's native
  settings change (Claude, Codex, DeepSeek where the agent supports it) and
  hands out only after the change is confirmed. A refused or unconfirmed
  change leaves the task waiting with its reason, never on the wrong model.
  Rebalancing keeps a task's model rule.
- **U46. The context before a task.** A role or member may set
  `context_before_task: none | compact | clear` (default `none`). Before each
  hand-out, a pipeline stage's included, Zerus runs the agent's native
  compact or clear, waits for its confirmation and only then hands out; an
  agent without the command gets `none` with a note in the feed. The hand-off
  text then names only the task's own input.
- **U47. A stage Zerus runs itself.** A pipeline preset (U36) may hold a
  `command` stage: a command, its expected exit code and a time limit. Zerus
  runs it in the stage's folder as a detached run (`hgs run`), with no model;
  the exit code is the verdict, and a return carries the output's tail as the
  finding to the implementer. Only the owner's catalog defines the command.
- **U48. Landing and a branch per task.** A preset may end with a `land`
  stage: a `command` stage run once the earlier stages pass, whose exit codes
  map to accepted, returned to the implementer (conflict, red) or held for
  the lead (busy, infrastructure). A team may set `branch_per_task` with a
  name template (`task/{id}`): each hand-out starts the task's branch in the
  member's folder from the team's base, so a squash landing never brings back
  work already landed. Default: one branch per member, as today.
- **U49. Scale and drain.** `scale {role, agent, model, count}` adds several
  members at once (the owner, or the lead on the owner's word naming the
  count); `drain {member}` lets a member finish its current attempt, hands it
  nothing new, then stops its session and removes it.
- **U50. One list of what waits for the owner.** A desktop page lists, across
  all teams, every item that waits for the owner: draft phases, reports or
  permissions escalated to the owner, the owner's word asked for, finished
  teams. It only reads what the teams already record. The phone follows with
  the team status API (U44).
- **U51. An intake for urgent tasks.** The owner may create an intake key for
  a team in Zerus state. A process presenting it (a health watcher) may add
  tasks only as one urgent phase, bounded in size, with the key named in the
  feed; it gets none of the owner's other rights. Without a key, as today,
  only the owner and the lead change a plan.
- **U52. Restart a member that crashed.** A team may set `restart_crashed`
  (default off). A member session that exits on its own (not stopped by the
  owner, not a usage limit) is launched again into the same folder at most
  three times an hour, its open attempt returned as unconfirmed for the lead;
  the feed records each restart.
- **U53. Change the account of a running session (owner, 2026-10-28).** The
  composer shows the session's account beside the model; its menu lists the
  same provider's logged-in accounts. Choosing one, after the owner confirms,
  waits for an idle session, copies the conversation into that account's
  native store and resumes the agent into the same conversation, folder and
  name; a failed launch returns to the old account, an unconfirmed one is
  reported uncertain and never retried. Nothing changes until the owner
  chooses; a team member's account changes only through swap-member.
- **U59. Free planning: an open phase that takes tasks as they come (owner,
  2026-10-30).** Besides the standard mode (phases that are planned, approved,
  run and close with their integration and build), a team can plan in the free
  mode: its phase is not closed when its tasks are done, and the owner and the
  lead keep adding tasks to it.
  - *The modes:* `standard` (the default and today's behaviour) and `free`.
    A free phase runs each task as it is added, under the same pipelines,
    reports and acceptance; it never closes by itself, and an empty queue is
    idle, not done. Integration and a build run when the owner or the lead asks
    (a lead task added to the phase), never as a phase barrier. The owner
    closes a free phase explicitly; the team then continues in standard mode.
  - *Choosing at the start:* when planning begins (Start planning, a new goal,
    a next phase), the lead reads the goal and the tasks and proposes one of
    the two modes with a one-line reason (a known scope with a release: standard;
    a stream of small independent requests, support or polishing: free). The
    owner chooses one of the two; with no choice, standard.
  - *Switching to free:* only on the owner's confirmation. The lead (or the
    owner through the lead's chat) requests it; Zerus shows a pop-up dialog
    with the reason and Switch to free / Keep standard, the same confirmation
    as a U57 card and bound to the request id. Deny keeps standard and its
    reason reaches the lead. Switching back to standard is the owner's close
    of the free phase.
  - The mode is shown beside the phase (status, the plan header, Graph and
    Board), and `hgs team status` carries `plan_mode`. The lead protocol says
    to propose a mode at the start, never to switch to free without the
    dialog, and to add incoming work to the open phase instead of planning a
    new one.
- **U58. Changing a plan on the go costs less than the work (owner, 2026-10-30).**
  From team 6d0aabee (RogueDOD QA bot): a task was cancelled and copied under
  a new id three times to start it earlier, fully cancelled phases stayed on
  the dashboard as approved, and a queued phase could not be edited.
  - `move-task {task, phase}` moves a task that has not started (no delivered
    attempt) to another phase, keeping its id, its deps checked against the
    target phase; `reassign` keeps the id for a member change, as today.
  - A queued or approved phase with no delivered attempt is edited like a
    draft (extend, edit-draft, remove tasks) by the lead; the scope the owner
    approved is the work, not its numbering.
  - A phase whose tasks are all cancelled reads `cancelled`; status and the
    dashboard hide cancelled tasks and phases by default, with a toggle.
  - The owner's word by text (the CLI fallback of U57) is not spent when its
    change fails, and one sentence naming a member and its tasks applies both,
    the member first.
  - A reply that only says it waits (a build lock, a queue) is a status, never
    a captured report.
  - Role and task text normalises CRLF to LF; a refused control character
    names the field and its position.
  - Work ahead (U31) stays an owner setting, off by default.
- **U57. The owner confirms a lead's change in the chat, not by a phrase (owner,
  2026-10-30).** Today the lead asks the owner to type an exact sentence
  ("add a member", "план принят") and the change is refused when the words
  differ. Instead the lead proposes the change and the owner answers a card in
  the lead's chat, the same way a subagent start is approved:
  - *What is proposed:* adding members, adding tasks to a running phase,
    removing (cancelling) tasks, discarding a phase, and the existing roster
    kinds (remove, swap, next goal). The proposal carries the exact change
    (members with role/agent/model, task titles and ids, the phase).
  - *The card:* in the lead's conversation (TeamConversation and the session
    window), in Activity and in the U50/U55 waiting list: what changes, who
    asks, Allow and Deny, and for Deny an optional reason that reaches the
    lead. Allow applies exactly that change, bound to the proposal id and a
    hash of its content; a changed or stale proposal is refused, never
    re-applied. The owner word by text stays as a fallback for the CLI.
  - *A phase with no work done:* a phase whose tasks are all cancelled or
    never started (none accepted, none with a delivered attempt) may be
    discarded, by the owner directly or by the lead through this card, even
    when it is running; its numbering closes up as for a draft.
  - The lead protocol says to propose and wait for the card, never to ask the
    owner for a sentence.
- **U56. A stuck member never blocks its tasks silently (owner, 2026-10-29).**
  From team 6d0aabee (RogueDOD QA bot): member b-body stayed in context
  compaction for over 80 minutes after its report; its pinned tasks #15/#16
  were never handed out, the only signal was a starved notice about another
  member, named by role, and the lead could neither release nor relaunch.
  - *Stuck.* A member busy in one phase (compacting, a turn) with no session
    event for 10 minutes (setting `stuck_minutes`) is marked stuck
    (`state_reason: compaction_timeout | no_progress`); the lead gets one
    `member_stuck` item naming the member id, phase, duration and the tasks
    that wait for it. The member's open attempt, if any, keeps today's silent
    rule.
  - *Tasks waiting for an unavailable member.* A pending task pinned to a
    stuck, stopped or offline member says so in its waits and in one inbox
    item, with the commands that free it.
  - *Ids, not roles.* Every notice names the member id with its role
    ("b-body (backend-opus)"); several members may share a role.
  - *The lead on a stuck member.* With the owner's team setting
    `lead_restarts_stuck` (default off), the lead may interrupt or relaunch a
    member Zerus marks stuck; otherwise the item tells the lead to ask the
    owner and names the command.
  - *Status without inspect.* `hgs team status` shows each member's phase,
    activity summary and minutes without progress.
  - *Small fixes.* A refused owner's word names the expected sentence ("plan
    approved", "phase N approved"); the lead may discard an empty draft phase;
    `hgs team draft` seen documents its request_id; a captured reply that says
    it still waits ("waiting for the editor to close") is not made a report;
    a team setting `keep_assignees` (default off) pins every planned assignee.
- **U55. What waits for the owner is shown where the owner looks (owner,
  2026-10-29).** Mock-up #530 waited a day for the owner, who never knew. An
  item that waits for the owner (the kinds of U50's `hgs team waiting`: a
  draft phase or plan, an escalated report or permission, the owner's word
  asked, a task the owner holds or a report marked for the owner, a finished
  team) shows as a banner at the top of the session's chat and Activity, and
  under Activity in the team view: what waits, for how long, and its action
  (Open, Approve, Accept). Several fold into one line with a count. The first
  time an item starts waiting, one desktop notification says so; it never
  repeats for the same item. A member's report that names the owner ("ready
  for the owner") is the lead's to decide unless the lead escalates it; the
  lead protocol says so.
- **U54. Move a team to another account (owner, 2026-10-29).** Moving team
  528aebfe's Claude members and its lead off an account near its weekly limit
  took two owner scripts in an outside PowerShell, a manual pause of six old
  sessions and a folder fix. Zerus handles it as one ordinary owner action:
  - *One action for a team.* "Move members on account X to account Y" (the
    team's members menu; `swap-member` takes a list) swaps every member of
    that provider whose session runs on X, the lead included, after one
    confirmation that names them. The roster shows each member's real
    account, read from its session, not only the configured one: an empty
    `account` used to hide that the lead and reviewer ran on Additional.
  - *Keep the conversation where it can.* With U53, a same-provider account
    change resumes the member's own conversation on the new account (same
    name, folder and attempt) instead of a fresh session with a continuation.
    A fresh session stays the fallback, and the owner's choice for another
    agent.
  - *The old session never blocks the new one.* A fresh member session
    reuses the member's session name, and `hgs` refused the launch ("This
    session already has an account") while the old session lived. The swap
    pauses the old session once it is idle (never a kill; its conversation
    stays under Saved) before launching, or waits for idle with that reason.
    A busy old session is never interrupted without the owner's choice.
    A pause alone is not enough: the paused session keeps its account
    binding, and `--new` with an explicit account is still refused, so the
    swap also renames the old session to `<name>-<old account label>` (its
    conversation and account kept) before the launch.
  - *A failed launch after a swap is retried.* The failed first launch cleared
    `launch_after_swap`, so after the owner's relaunch the member stayed Not
    created until work was ready. A relaunch of a swapped member launches it
    at once, and the error names its fix.
  - *A member folder left on a task branch.* frontend2's worktree was on
    `team/528aebfe/frontend2-570`, so its launch failed ("is not this member's
    folder"). When the folder is clean and the task branch is already merged
    into the member's or the lead's branch, Zerus checks out the member's
    branch again (the task branch is kept) and records it in the feed;
    otherwise the error offers that checkout as a review action, never a
    reset.
  - *The lead on the owner's word.* The lead may swap, relaunch and pause the
    old sessions of its own team on the owner's word in its session (the U26
    roster rule), so the owner does not need an outside shell. A swap to an
    existing session (the lead moving to a new session) stays owner-confirmed.
  - *A renamed member session stays the member.* After the new lead's session
    was renamed (work-e1f3aa6e to Zerus_Iterations), every team command from
    it was refused ("This session is not a member"): the caller is matched by
    the stored session name only. A rename keeps the member: the caller is
    matched by its run and conversation, and the team record follows the
    session's rename, as `hgs inspect` already does.
  - *The new lead gets the old lead's context.* A lead swapped to another
    conversation gets, in its brief, the previous lead's session name and
    conversation, the path of its transcript and its latest compact summary
    (bounded), and the old lead stays paused, not stopped.

## Phases 37-40 (replanned 2026-10-27)

Phase 36 closed without its own build: its review findings and the build go to
phase 37. Lanes are balanced by volume and cost (U28 rule 9), not by count.

- **37 Fixes of phase 36 and the build** (running): the checkpoint and final
  review fixes of phase 36 (two critical: templates off the worker's tick, a
  dialog from a member menu keeping its hold), then integration, review, QA and
  one build for phases 36-37.
- **38 Server fixes and the Sessions settings (DeepSeek members)** (no
  barrier, runs alongside 37): the server review fixes of phase 35 (C1/C3/C8/C9,
  C7, C14 groups), the draft follow-ups (F5), MCP servers as services (U11),
  the designer's narrow rules (U16), confirm subagents (U19) and the Sessions
  settings for it and for the DeepSeek session-log lock (U22); integration,
  review, QA, build.
- **39 New UI, the rest** (Claude members): New session dialog, Team designer
  and catalog editor, the session list, the inspector, the designer window's
  questions, permission answers in one line, the light theme fix and the
  graph nodes' activity label; a checkpoint after the first lane; build.
- **40 Planning rules, lanes, scope additions and session states**: U28 rules
  and their weight-based health warnings, U27 visual lanes, U29 adding a task
  to a running phase, U30 six session states (server, desktop, Android), U31
  work ahead; build.

## Phases 41-43 (planned 2026-10-28)

- **41 Test stability, upstream merge and the PR draft**: the five recurring
  test failure groups fixed at their cause, a new baseline, the upstream
  ufna/zerus main merged into the fork, and the pull request prepared (no
  push without the owner's word).
- **42 Review fixes of phases 36-39, then task pipelines (U36)**: first the
  fixes of the review of phases 36-39 (#499: six critical, the normal
  findings by area) and the two session window regressions, with their own
  integration and build; then a mock-up
  first (the Pipelines tab, the new Board columns, the stage chips), then the
  server model and CLI, stage delivery in the task's folder, the Pipelines
  tab, the Board and task views, Android, the lead protocol and docs;
  integration, review, QA and the build. It starts after the upstream merge
  (#528) so the merge does not also carry this work. With it, each task's
  working time on its node (U37).
- **43 The pipeline profile (U38)**: a mock-up first, then the spans and
  their tokens on the server, `hgs team profile` with the ranked losses,
  the Profile view and the heat overlay, the lead protocol and docs;
  integration, review, QA and the build.

## Phase 50 (planned 2026-10-30)

- **50 Free planning (U59)**, the last phase of the plan: the plan mode on
  the server and its status field, the lead's proposal at the start of
  planning, the switch-to-free dialog bound to the request (on U57's
  confirmation), adding tasks to an open phase and closing it, the mode in
  the desktop and Android views, the lead protocol and docs; integration,
  review, QA and the build. It starts after phase 45, which brings U57's
  confirmation cards.

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
