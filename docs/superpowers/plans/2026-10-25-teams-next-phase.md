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
