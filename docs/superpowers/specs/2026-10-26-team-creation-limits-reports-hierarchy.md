# Team creation, limits, reports and hierarchy

Status: decided by the lead for phase 35 "Team creation, limits, reports and
hierarchy" of team "Teams v2", 2026-10-26. Plan:
`docs/superpowers/plans/2026-10-25-teams-next-phase.md` items F1, F3-F8, F11,
F12 (refinement), H1-H4, H6, H7 (rest), D1, D2, C7 (refinement), G9, U8, U9 and
REQUIREMENTS Prompt 2 (hierarchy). Builds on `2026-10-25-team-phases.md`,
`2026-10-26-team-delivery.md` and `2026-10-26-team-permissions-and-lead-load.md`.
Section 9 (hierarchy) keeps the defaults of Prompt 2; the owner confirmed
them on 2026-10-26, so tasks 351 and 352 implement them as written.

Owners (task numbers of phase 35):

| Task | Member | Sections |
|---|---|---|
| 339 | backend | 1 (F1 guards), 2.4 (F3), 2.5 (F11) |
| 340 | backend | 3 (F5 draft), 4 (F7 next goal, F8 proposals), 2.6 (F12) |
| 349 | backend | 7 (D1 reports) |
| 350 | backend | 8 (D2 findings) |
| 355 | backend | 10 (C7 per phase, phase summary for G9) |
| 341 | backend2 | 5 (H6 swap, U8), 6.1 (H1 limits) |
| 342 | backend2 | 6.2-6.5 (H2, H3, H4, H7 rest), 10.3 (U9) |
| 351 | backend2 | 9.1-9.4 (hierarchy model, limits, approval, plan) |
| 352 | backend2 | 9.5-9.8 (result flow, routing, cascades, catalog) |
| 343 | frontend | 11.1 (F5 New Team), 11.2 (F6), 11.3 (F4) |
| 344 | frontend | 11.4 (F7, F8, H6, H1, H3) |
| 353 | frontend | 11.5 (D1, D2) |
| 354 | frontend | 11.6 (hierarchy) |
| 356 | frontend | 11.7 (G9) |

Compatibility for every item: new fields have serde defaults and are skipped
when empty; old team files load; every existing CLI call keeps its shape and
meaning; new behaviour starts only on new fields, commands or settings. Every
mutating call carries a canonical `request_id` and is idempotent on it. Lists
kept on the team are bounded (stated per list) and status pages them.
Frontend tasks may start before the backend ones: they code against the JSON
in this spec and test with fixtures shaped exactly like it; the integration
task replaces fixtures that differ with real status output.

## 1. Members land in their team's project (F1)

1. *Group.* After a member's launch is adopted the dispatcher files the
   session into the team's project with `hgs swarm assign-launch --json`
   (`{request_id, name, expected_run_id, expected_conversation_id, swarm_id,
   project_id}`, see `src/swarm/launch_project.rs`); the project is
   `team.project`, or, when empty, the project of `team.folder`. A worktree or clone folder resolves to the
   project of its repository (git common dir), never to General. The GUI
   files any session that is a team member (status `members[].session`) into
   the lead's group before any default rule (`SessionOrganization`).
2. *Folder kind by default.* `folder_policy: auto` (default since phase 32)
   keeps `dispatcher::default_folder`: a member without an explicit folder in
   a Git team folder gets a `clone` when its agent is dsh (its sandbox cannot
   write the shared `.git`, so merge and commit need no permission request)
   and a `worktree` otherwise; a non-Git team folder gives `shared`. The plan
   may name any kind. `settings.worktrees_forbidden` and the project's
   `teams.worktrees: forbidden` refuse `worktree` and `clone` (the member is
   `shared`). Existing members keep their folders.
3. *Mismatch is an error.* `plan`, `approve`, `extend`, `add-member`: a member
   folder that the policy forbids, or a `shared` member when the policy is
   `worktree`, is a validation error (`kind: "folder"`), not a warning.
4. *Folder check before launch.* Before `hgs worktrees create` / launch the
   dispatcher checks: the folder exists (or will be created by Zerus), belongs
   to the team's repository (same git common dir) or is the team folder, and a
   worktree/clone is on `team/<id8>/<member>`. On failure the member is not
   started: `members[].error` = the reason, one inbox item `member_error` for
   the lead, the desktop shows it on the card. `relaunch` retries after a fix.
5. *Fix in place.* While Planning, `hgs team settings --json {request_id, team,
   settings?, members_update?: [{id, folder?, directory?, role?, prompt?,
   permissions_profile?}]}` (lead or owner) changes them; a widening keeps the
   B5 confirmation. Running teams accept only `members_update` for members
   with no session yet.
6. *Visible.* `members[]` in status gains `project` and `branch` (read from
   the folder, cached per tick); `outside_project: true` flags a member whose
   session's project differs from the team's.
7. *Leftovers.* `hgs team leftovers --json {}` lists sessions that are still
   running and belong to Done, Stopped or Archived teams: `[{team, name,
   member, session, since}]`, at most 200. Nothing is stopped.
8. *Tests* for 1-7, one with a worktree team whose sibling folders have no
   project.

## 2. Small rules

### 2.4 Role checks by the designer (F3)

`hgs guide designer` tells the designer to check each role prompt against the
team goal and the built-in protocol and to report every correction.
`catalog propose` gains `"notes": [{"role", "change", "why"}]` (at most 20,
each 500 characters); the desktop shows them with the proposal diff; nothing
is applied without the owner's decision (unchanged).

### 2.5 Stale draft ids and paused coalescing (F11)

- `edit-draft`, `move-phase`, `discard-phase`, `extend` with `phase` accept
  `"revision": N` (the team revision the caller read). When a
  `phase_renumbered` or `phase_discarded` event came after N, the call is
  refused: "Draft numbers changed after revision N; read status again".
  Without `revision` the call works as today.
- A Paused team's messages to the lead go through the same inbox coalescing
  as a Running team's.

### 2.6 Approve the next draft while the previous phase runs (F12)

`approve-phase N` is allowed when every phase before N is approved (none is a
draft); it no longer waits for them to finish. The barrier keeps the order:
an approved phase with `barrier: true` hands out nothing until every earlier
phase is finished. `phases[].waiting` stays for a draft whose predecessor is
still a draft ("Waiting for the previous phase to be approved") and for an
approved phase behind a barrier ("Starts when phase N is finished").
F13 (owner's word) is unchanged.

## 3. The team draft (F5)

A draft is a team in the new status `draft`, before Planning, stored like
any team. Old binaries never see one unless the owner creates it.

- `hgs team draft --json` with `action`:
  - `start` (owner/desktop): `{request_id, name?, folder, template?, lead:
    {agent, model?, effort?, account?} | {session}}` creates the draft and
    starts the lead session in `folder` (never a worktree), or adopts the
    named session. The lead gets one brief: the built-in lead protocol, `hgs
    guide designer` and the draft id. Result `{team, revision}`.
  - `update` (lead or owner): `{request_id, team, revision, set: {name?, goal?,
    addendum?, settings?, members?, tasks?}}`. `revision` must equal the
    draft's; else refused with the current revision. A field the owner edited
    after the lead last read it is refused for the lead ("owner changed
    members"); the owner's update always wins and the lead is told once
    ("Owner changed: members, goal"). Lead-changed fields are listed in
    `draft.unseen` until the owner calls `seen`.
  - `seen` (owner): `{request_id, team, fields}` clears `unseen` entries.
  - `get`: the draft with `revision`, `unseen`, `preflight`.
  - `create` (owner): runs the preflight (section 1, B3, B5); errors refuse.
    Creates worktrees/clones, sets status `planning` with the drafted plan, or
    `running` when the plan is approved in the same call
    (`"approve": true`, B5 widening still needs `confirm_widening`). The same
    lead session leads the team.
  - `discard` (owner): `{request_id, team, stop_lead: bool}`; the team becomes
    `stopped` with reason `draft_discarded`; the lead session is stopped only
    with `stop_lead: true`.
- `hgs team list` shows drafts with `status: "draft"`. Opening New Team or
  any dialog creates nothing; only `start` does.

## 4. Next goal and membership proposals (F7, F8)

- `hgs team next-goal --json {request_id, team, goal, name?}` (owner, or the
  lead with an accepted proposal): from a Done team, creates a new team in
  Planning with the same folder, settings, lead and members, adopting their
  live sessions (`members[].session`), `previous: <team id>`; the old team
  stays Done with `next: <new id>`. Members whose session is gone are
  NotCreated as usual.
- Proposals: `hgs team propose --json {request_id, team, kind, reason, ...}`
  by the lead, `kind` one of `add-member` (`member`), `remove-member`
  (`member`), `swap-member` (section 5 fields), `next-goal` (`goal`). Stored
  in `team.proposals` (at most 50 open) with `id`, `by`, `at`, `state:
  open|accepted|rejected`. The owner decides with `hgs team decide --json
  {request_id, team, proposal, accept, reason?}` (owner only); accepting runs
  the operation as the owner. The lead gets the decision as an inbox item.
- `remove-member` by the owner also works for a member whose only open tasks
  are drafts if `"reassign_drafts_to": <member>` is given.
- An existing session joins with `add-member {member: {..., session}}`
  (exists since phase 1, F8).

## 5. Swap a member's session, agent, model or account (H6, U8)

`hgs team swap-member --json {request_id, team, member, agent?, model?,
effort?, account?, session?}` (owner; the lead proposes it, section 4).

- Applies to any member, the lead included. The member keeps its id, role,
  tasks, queue, folder and branch; it does not count twice against
  `max_members`.
- `session` given: that live local session is adopted (as `add-member`
  adoption checks: same repository for worktree members, not in another
  team). Otherwise Zerus launches a new session with the given agent, model,
  effort and account in the member's folder. The old session keeps running,
  is no longer bound, and is listed in status `swapped: [{member, old, at}]`
  (last 20) for the owner to stop.
- An open attempt is revoked (`swapped`) and handed again to the new session
  with a continuation packet: task text and criteria, "From dependencies",
  folder and branch, `git log` since the attempt began (sha, subject, at most
  20) and `git status --short` (at most 50 lines), the previous attempt's
  partial report and open questions. For the lead nothing is handed out; its
  open task stays in the inbox and the new lead session gets one brief with
  the team id and `hgs team inbox`.
- A member stopped by a usage limit appears in the lead's inbox as
  `member_limited` with `resets_at` when the session reports one.
- Event `member_swapped {member, from, to, by}`.

## 6. Operations

### 6.1 Resume after usage limits (H1)

`settings.resume_after_limits: bool` (default false; team setting, the lead
and owner may set it). When on, a member session (not a DeepSeek host, not
the owner's own sessions) that shows "Usage limit reached" with a known reset
time gets the existing automatic-recovery "Wait for reset" continuation for
that episode, even when the shared policy is off. No new retry logic: the
recovery job owns timing, cancellation and the `[HGS automatic recovery]`
text. Unknown reset: inbox item only. The lead's own session follows the same
setting.

### 6.2 JSON input on Windows (H2)

Every `--json` command also accepts `--json-file PATH`; stdin and the file
drop a leading UTF-8 BOM. A non-UTF-8 input is refused with "Input is not
UTF-8 (PowerShell 5.1: use --json-file)".

### 6.3 Shared dsh host (H3)

Status gains `hosts: [{agent: "dsh", pid, started, members: [ids]}]` for the
DeepSeek host(s) serving members; each such member has `host_shared: true`
when the host serves more than one member. Read only; one host per team stays
a later decision.

### 6.4 session-log-deepseek (H4)

Launch arguments and any dsh configuration Zerus writes never enable session
log upload; a test asserts it for member, lead and designer launches; B3
preflight warns when the user's dsh configuration enables it.

### 6.5 Session identity after a resume (H7, rest)

The re-bind of a member to a new run or conversation of the same session name
exists (dispatcher `Adopt`). Remaining: a usage observation older than the
session's last turn is reported as unknown, never as a cold cache; a re-bind
records event `member_rebound {member, from, to}`.

## 7. Structured reports (D1)

`hgs team report` accepts, besides `text`, an optional `report` object:

```json
{"summary":"...","commit":"sha",
 "criteria":[{"n":1,"result":"pass|fail|partial|n/a","evidence":[0,2],"note":""}],
 "evidence":[{"kind":"command","caption":"cargo test teams","command":"...","exit":0,"tail":"..."},
             {"kind":"image","caption":"New Team, dark","path":"D:/.../shot.png"},
             {"kind":"html|file|log","caption":"...","path":"..."},
             {"kind":"url","caption":"...","url":"https://..."},
             {"kind":"text","caption":"...","text":"..."}],
 "limits":["what was not checked or was mocked"],
 "findings":[ see section 8 ]}
```

- Bounds: the whole report 64 KB, 50 evidence items, 50 criteria, 100
  findings; `tail` 4000 characters. `criteria[].n` is 1-based on the task's
  criteria; unknown n is refused.
- `text` empty and `report` given: `text` becomes `summary`. Both empty:
  refused. A report without `report` is stored as today and shown as
  `"unstructured": true` in status.
- Paths are stored as given; Zerus does not read them. The desktop previews
  images (png, jpg, webp, at most 10 MB) and HTML (rendered without scripts or
  network) only from the team folder, member folders or the shots folders in
  `settings.evidence_folders` (default empty), else a plain link.
- Status: `attempts[].report.structured` (the object), `.unstructured`.

## 8. Findings by severity (D2)

- Findings come in a report's `report.findings`: `{"severity":
  "critical|normal|later", "title", "file"?, "line"?, "scenario", "task"?}`
  (`task` = the task that caused it, when known). Accepting the report files
  them in `team.findings` (at most 1000; status pages them, newest first):
  `{id: "f<N>", from_task, severity, title, file, line, scenario, task,
  state: "open|reopened|urgent|replan|fix_phase|deferred|fixed|dropped", by,
  at, note}`.
- `hgs team finding --json {request_id, team, finding, action, ...}` (lead or
  owner):
  - `reopen` (`task`, defaults to the finding's `task`): the accepted task
    gets a new attempt for the same member, pinned, the finding as its
    message; status becomes `in_progress`, its old acceptance stays in
    history (`decided_by` moves to `history`). Dependants not started wait
    again; decided dependants are untouched.
  - `urgent` (`tasks` as in extend): `extend` with `"urgent": true` - a new
    phase appended last that runs at once (no approval under `review`, no
    barrier) and whose tasks are handed out before other pending tasks.
  - `replan` (`proposal` text): the team becomes Paused, running attempts may
    finish; the owner gets the proposal (inbox and desktop); the owner
    resumes the team after changing the plan.
  - `fix_phase` (`phase`): marks it for that draft phase (normal findings).
  - `defer` (`note`): `later` and `normal` by the lead; `critical` only by
    the owner.
  - `fixed` / `dropped` (`note`): closes it.
- `later` findings are deferred on filing; the owner gets one notice per
  report ("3 findings deferred"). `finish` lists every deferred and open
  finding in its result and refuses while a critical finding is open
  (owner may finish anyway with `"force": true`).
- Status: `findings_summary: {open: {critical, normal, later}, deferred}` and
  `findings` (page of 50, `before` cursor).

## 9. Hierarchy (Prompt 2)

Defaults as proposed by REQUIREMENTS Prompt 2; confirmed by the owner.

1. *Link.* `Team.parent: Option<{team, task, member}>`, `Team.children:
   Vec<String>` (at most 32). `hgs team create` with `"parent": {team, task}`
   from the session of that open task's assignee (proven identity), or by the
   parent lead or owner on its behalf. The assignee's session becomes the
   sub-team's lead.
2. *Limits.* Depth at most 2 below the master; at most 16 active sessions in
   the whole tree; checked at create, add-member, swap-member and extend.
   Settings on the master: `tree_depth`, `tree_sessions`.
3. *Approval.* A sub-team's plan and phases are approved by the parent lead
   (default, `settings.subteam_approval: "parent"`) or the owner
   (`"owner"`); the owner can always approve, reject, pause or stop.
4. *Plan.* The parent task shows `subteam: {team, status, done, total}`; the
   sub-team's tasks are its own; plan health counts each tree level
   separately.
5. *Result flow.* `finish` of a sub-team files its summary (4000 characters,
   plus the sub-team id) as the sub-lead's report on the parent task; the
   parent lead accepts or rejects as usual. A reject reopens the sub-team:
   status Running with a draft phase request (title "Rework", the reason)
   for the sub-lead.
6. *Routing.* Questions and permission requests go one level up at a time:
   member -> sub-lead -> parent lead -> owner. Each level may answer or pass
   it on (`hgs team escalate --json {request_id, team, question}`).
7. *Cascades.* Pause/stop of a parent pauses/stops its sub-teams (the desktop
   confirmation names the number of sessions); cancel or reassign of a parent
   task with a sub-team asks first (`"subteam": "stop|keep"` required).
   Archive only after children are stopped or done. Nothing kills native
   agents outside these operations.
8. *Catalog.* A role may have `can_lead_subteam: true`; a roster seat may name
   `subteam: <template>`; unknown templates and template cycles are catalog
   errors. `hgs guide team` and `designer` explain sub-leads.
   `hgs team status` shows `children: [{team, name, status, done, total}]`.
Old teams have no parent and behave as today.

## 10. Plan health per phase and for the plan (C7, U9, G9 data)

### 10.1 Health

`health` keeps its fields and meaning changes to "the plan": every task not
yet decided, across phases (decided tasks only in `totals: {tasks, decided}`).
New `phases_health` in status and in `validation` of `plan`, `extend`,
`edit-draft`: `{"<n>": Health}` for every non-discarded phase including phase
1, computed only from that phase's tasks (dependencies on earlier phases count
as met). `Health` gains:

- `chain`: the longest hard-dependency chain (tasks);
- `critical_path`: the schedule length when every member does one task at a
  time (unit tasks, ready tasks in id order, soft dependencies ignored); for
  the old meaning read `chain`;
- `parallelism` = tasks / critical_path.

Warnings are per scope; a team-wide warning ("Backend holds 8 of 14") appears
only in the scope it was computed for.

### 10.2 Phase summary for the desktop (G9)

Status gains `phase_summary: [{n, title, status, done, total, barrier,
health_line}]` in display order, phase 1 included. `status`:

- `done`: approved, every task decided;
- `in_work`: approved, has open tasks and is not held by a barrier;
- `approved`: approved, not started (behind a barrier);
- `draft`: waits for the owner;
- `paused`: the team is Paused and the phase is `in_work`.

`health_line`: "path 8 · par 2.4 · 1 warning" for the header.

### 10.3 No starved notices before approval (U9)

`starved_since` is not set and no `plan_starved` message is sent while every
open task is in a draft phase (or in an approved phase behind a barrier whose
predecessor is the only open work and is itself a draft). Starved resumes once
a phase is in work and a member still has nothing ready.

## 11. Desktop

1. *F5 New Team.* The layout and flow of plan F5 (settings column, lead
   column with Chat, Goal and Plan; Start planning; drafts listed under Teams;
   unseen marks; Discard draft with the stop-lead choice; Create team runs the
   preflight). Opening the tab creates nothing.
2. *F6.* A compact template list with search and favourites; the full card
   only for the selected template.
3. *F4.* "Team designer" session: fixed name and colour, in General, started
   with the designer guides; the desktop never puts other work in it.
4. *F7, F8, H6, H1, H3.* Next goal from a Done team; proposals with Accept and
   Reject (confirmation for remove and swap); Swap member (agent, model,
   account, or an existing session) with confirmation; the
   `resume_after_limits` switch with the reset time on a limited member;
   a shared-host indicator; leftovers list.
5. *D1, D2.* The report view renders structured reports: criteria table,
   evidence with image and HTML previews (section 7 rules), limits; findings
   by severity in the team header, the deferred list, and the finding actions
   (reopen, urgent phase, replan, defer, fixed, dropped) with confirmation.
6. *Hierarchy.* Team list as a tree with rolled-up counts, breadcrumb in the
   workspace, a parent task card shows its sub-team badge and opens it, the
   graph draws a delegated task as a group node, "Delegate to a sub-team" on
   a task card, confirmations for tree actions.
7. *G9.* The workspace graph stacks phase bands top to bottom (layers left to
   right inside a band, G3/G4 kept); each band collapses to one header row
   (finished phases start collapsed, remembered per team); the header shows
   number, title, a status chip from `phase_summary` (Done, In work, Approved
   (not started), Draft, Paused), done/total, `health_line` with
   `phases_health` in its tooltip, and for a draft Approve and Discard
   (disabled with the `waiting` text, each with confirmation). The draft
   strips above the members column go; list and panel views use the same
   header. Without `phase_summary` (older hgs) the header falls back to
   today's heading.

## 12. Events and settings summary

Events: `member_error`, `member_swapped`, `member_rebound`, `member_limited`,
`proposal_added`, `proposal_decided`, `draft_started`, `draft_updated`,
`draft_created`, `next_goal`, `finding_filed`, `finding_changed`,
`task_reopened`, `replan_proposed`, `subteam_created`, `escalated`. Settings:
`resume_after_limits`, `evidence_folders`, `subteam_approval`, `tree_depth`,
`tree_sessions`. Status: `members[].project|branch|outside_project|host_shared`,
`hosts`, `swapped`, `proposals`, `findings_summary`, `findings`,
`phases_health`, `phase_summary`, `totals`, `children`, `parent`, `previous`,
`next`, `attempts[].report.structured|unstructured`.

## A sub-lead inside a team (U25 part 10)

The owner's design (`SPEC.md` 5.6 and 2.5 of the UI handoff) adds a hierarchy inside one team. Sub-teams (separate
teams with their own lead, above) stay as they are.

- `members[].parent`: the member it reports to; empty means the lead (old teams load unchanged, and the field is
  skipped when empty). `members[].sub`: the member is a sub-lead. A sub-lead keeps its own coalesced inbox in
  `members[].inbox` (`told_at`, `told`), as the team's `inbox` is the lead's.
- A parent is the lead or a non-removed sub-lead of the same team, never the member itself or one below it; the lead
  has no parent and is no sub-lead. `plan`, `add-member` ("Reports to") and the commands below are refused otherwise:
  "X reports to unknown member Y", "X reports to Y, which is not a sub-lead", "X would report to itself".
- `hgs team reports-to --json` `{"request_id","team","member","parent"}` (lead or owner; `parent` may name the lead)
  records `reports_to {by, member, to}`. `hgs team sub-lead --json` `{"request_id","team","member","sub"}` (lead or
  owner) records `sub_lead {by, member, sub}`. Stopping being a sub-lead, and `remove-member` of one, are refused while
  it has reports: "Move its reports to another lead first: <roles>".
- The reviewer of a member is its sub-lead parent, else the lead (`Team::reviewer_of`). The reviewer may `reply` to
  the member's questions and `hold`, `release`, `retry`, `accept` and `reject` its tasks; the lead and the owner still
  may do all of these for anyone. A sub-lead's own tasks are reviewed by its own parent.
- Hand-outs to a sub-lead's report say `from: <sub-lead role>` and name it as the route for questions, the report
  and blockers ("Questions for QA lead", "Zerus gives it to QA lead"); the lead's own reports keep the old words. The
  `handed_out` event's `by` is the sub-lead's role ("QA lead: Task to Tester"), `reported` gains `to` ("Report to QA
  lead"), and a sub-lead's decision has its role as `by` ("QA lead: Accepted #5").
- Report, question and stall items of a task go to the inbox of the reviewer of its assignee: a free sub-lead gets
  one coalesced message per interval like the lead (step `SubInbox`, event `inbox_told {items, member}`), and `hgs team
  inbox --json` called from a sub-lead's session (or with `"member"`) lists its items. The lead's inbox and its report
  reminder keep the team's own items and its direct reports'.
