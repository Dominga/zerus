# Agent teams core (M1): state, `hgs team`, dispatcher

WhiteWh fork. Epic `zerus-crjf`, milestone `zerus-crjf.1`. UI and risk analysis:
`docs/superpowers/specs/2026-10-23-agent-teams-ui.ru.md`; design package: `design/agent-teams/`.

## Owner decisions (2026-10-24)

1. The lead sets each dependency **hard** (the task waits until the dependency is Accepted) or **soft** (it may
   start once the dependency is In progress). A team-wide override, `all_hard`, makes every dependency hard.
2. The lead accepts or rejects a submitted report. A human Accept/Reject is final and overrides the lead.
3. Members report with `hgs team report`. A turn that ends without a report shows **No report**; a human can use the
   member's last reply as the report.
4. Folder policy is a team setting: `worktree` (one per writing member), `shared`, or `lead` (default: the lead
   chooses per member when planning). Worktrees are a poor fit for very large repositories.

## Placement

The code lives in `src/state/teams/` because the state helpers it needs (`root`, `atomic`, `lock`, binding records)
are private to `state`. The CLI command is `hgs [@host] team <subcommand> [--json]`, routed like `wait`.

| File | Role |
|---|---|
| `teams/model.rs` | Team, member, task, dependency, attempt, report types; status enums; JSON (serde) |
| `teams/plan.rs` | Pure rules: cycles (Tarjan), layers (longest path, DFS back edges ignored), chain, readiness, plan validation |
| `teams/store.rs` | SQLite store, revisions, idempotent operations, event journal with keyset pages |
| `teams/ops.rs` | The state machine: each request as a pure change of the team, shared by CLI and tests |
| `teams/commands.rs` | `hgs team …` subcommands, caller identity |
| `teams/dispatcher.rs` | Worker loop: member status, launches, hand-outs, report timeouts, reports to the lead |

## State

`root()/teams/teams.sqlite3`, opened like `waits.sqlite3` (private file, `busy_timeout`, `Immediate` writes):

- `teams(id TEXT PRIMARY KEY, revision INTEGER, updated REAL, payload TEXT)` — the whole team as JSON.
- `events(seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT, at REAL, kind TEXT, payload TEXT)` — the feed.
  Pages: newest first, 50 per page, `before=<seq>` cursor.
- `operations(id TEXT PRIMARY KEY, digest TEXT, receipt TEXT, at REAL)` — every mutating request carries a
  canonical `request_id`; a replay with the same digest returns the stored receipt, another digest is refused.

Every mutation reads the team, checks `expected_revision` when given, applies one change, increments the revision and
appends its events in one transaction.

### Team

`id` (UUID), `name`, `goal`, `project`, `folder`, `computer` (lead's host, empty = this computer), `status`
(`planning | running | paused | stopped | done | archived`), `settings { all_hard, folder_policy, approval: review|auto,
max_members, report_timeout_seconds }`, `lead { member_id }`, `members[]`, `tasks[]`, `created`, `updated`.

### Member

`id` (short, stable in the team, e.g. `backend`), `role`, `agent`, `account`, `model`, `effort`, `permissions`,
`computer`, `folder: worktree|shared`, `directory` (its folder on that computer; empty = the team's folder),
`session` (once created: `{ host, name, run_id, conversation_id }`), `state` (`not_created | active | paused |
removed`), `launch_id` (recorded before the dispatcher starts the session), `error` (why the start failed).

### Task

`id` (integer, 1-based, stable), `title`, `description`, `criteria[]`, `deps[{ task, kind: hard|soft }]`,
`assignee` (member id), `status` (`draft | pending | assigned | in_progress | submitted | accepted | rejected |
cancelled`), `attempts[]`, `decided_by` (`lead | human`), `report`.

### Attempt

`n`, `id` (UUID = the hand-off `request_id`), `member`, `state` (`delivering | delivered | unconfirmed | refused |
revoked | reported | rejected`), `sent_at`, `delivered_at`, `turn_ended_at`, `no_report`, `report { text, at }`,
`notice` (`none | pending | sending | sent | unconfirmed`: the report's message to the lead) with `notice_id`,
`error`.

## Rules (from the prototype, `design/agent-teams/index.html`)

- **Cycles:** Tarjan SCC over "dependency → dependent" edges; every node in a component larger than one is in a cycle.
- **Layers:** depth = longest path from the roots; DFS back edges are ignored so adding a cycle does not reshuffle.
- **Ready:** a `pending` task whose every dependency is satisfied: hard → `accepted`; soft → `in_progress`,
  `submitted` or `accepted` (with `all_hard` every dependency is hard).
- **Plan errors** (block Approve): task without an existing assignee, a dependency cycle, unknown dependency.
  **Warnings:** member on an offline computer, more members than `max_members`.
- **Free member:** active, no open attempt (`delivering | delivered | unconfirmed`), session idle with no Background
  work, pending question/approval, recovery or coordination wait.

## Hand-off (never retried automatically)

1. The dispatcher records the attempt as `delivering` with a new `request_id` **before** sending (durable).
2. It runs `hgs [@host] send <member session> --json` with that `request_id`, the session's expected run and
   conversation, and the text, which starts with `[Zerus team · from: Lead · task: #<id> · attempt: <n>]` and ends with
   the report command for this attempt.
3. Outcome:
   - receipt `submitted` → attempt `delivered`, task `in_progress`;
   - a refusal before delivery (session busy, changed, guards) → attempt `refused`, task back to `pending`
     (nothing reached the agent, so a later tick may hand it out again);
   - any other failure → attempt `unconfirmed`, task stays `assigned`; a human chooses Check session / Send again /
     Cancel attempt. The dispatcher never re-sends it.
4. After a restart, attempts left in `delivering` become `unconfirmed`: nothing is re-sent silently.

## Reports and decisions

- `hgs team report --json` with `{request_id, team, task, attempt, text}` on stdin. Only the task's current open attempt is
  accepted; a stale attempt is rejected and recorded in the feed. The task becomes `submitted` and the dispatcher
  sends the lead a `[Zerus team · from: <role> · task: #N · attempt: n]` report message.
- `accept` / `reject` (with a `reason`) by the lead from its session or by a human (`"by":"human"`, the GUI). A human decision is final: a later lead decision on the same attempt is refused.
  Reject → the task returns to `pending` for a new attempt (or `rejected` when `--final`).
- **No report:** when a delivered attempt's session has been idle for `report_timeout_seconds` (default 120) after its
  turn ended, the member shows No report; `use-last-reply` turns the member's last reply into the report.

## Who asks

- A call from a Zerus session is proven by process ancestry (`caller_record`, as for sends). It acts as that member
  (the lead included); a session that is not a member is refused.
- `report` from elsewhere (a member on another computer reaches the lead's computer over SSH) is authorized by its
  attempt id, which only the hand-off carries. A report for an unknown attempt is refused without a feed entry.
- Anything else must say `"by":"human"`: the desktop and a person at a terminal. Agents are told never to.
- `create` from a session makes that session the lead.

## Commands

`create`, `plan` (set members/tasks/deps from JSON while `planning`), `approve`, `discard`, `pause`, `resume`, `stop`,
`archive`, `status [--json]`, `list`, `events --before N`, `report`, `accept`, `reject`, `reassign`, `take-over`,
`cancel`, `retry`, `check`, `use-last-reply`, `relaunch` (a member whose session failed to start), `worker`. All
subcommands except `worker` read one JSON object on stdin (`--json`); mutating ones carry a canonical `request_id`.
The agent guide topic is `hgs guide team`.

## Dispatcher

`hgs team worker`: one process per computer (lock `root()/teams/worker.lock`), 3 s ticks, serves the teams whose lead
is on this computer. Each tick: one `hgs ls --json --no-archives` (it covers peers), adopt started sessions by
`launch_id`, record turn ends and No report, forward reports to a free lead, then hand out ready tasks to free members
in task order; a member without a session is started first (`hgs [@host] AGENT DIR --new -n TAG --launch-id ID`, after
`hgs worktrees create` for a worktree member). A terminal agent starts with a one-line first turn, because it
reports its status only once a conversation exists. A failed start is shown and not retried until `relaunch`.
Child `hgs` runs are plain processes with file output: `platform::output_until`'s Windows job would end a session
host or DeepSeek host that a launch starts, and it closes stdin. After a
refused hand-off the member gets none for 30 s. `hgs status` reports it as `team_worker`. M1 starts the worker from
`hgs team approve`/`resume` when none is running (detached); it exits when its executable is replaced or after five
minutes without a running team. The GUI supervises it later like the swarm worker.

## Desktop (M2)

- `TeamController` (tray core) polls `hgs team list` every 3 s while the window is shown, and `team status` plus the
  newest `team events` page for the team on screen; an unchanged answer changes nothing on screen. Changes go out
  once with their own `request_id` and `"by":"human"`; a refusal is shown in the panel.
- `TeamModel` derives what the desktop shows: a member's team status (Created on approval, Failed to start, Paused,
  Offline, Needs your answer, Delivery unconfirmed, No report, Working on #N, Submitted, Idle; problems first),
  Stuck (a running team where no open task can move without you), progress, the Next line and feed lines.
- Session list: a team whose lead is listed in a project gets a header row (`SessionRoles::TeamId`) above the lead;
  its members listed in the same project follow, indented with a connector. The header opens the Team tab; its
  chevron keeps only members with a problem (`workspace/collapsedTeams`).
- Inspector tab **Team** (`?N` = questions in member sessions): header with progress, Pause/Resume/Stop…/Archive,
  plan errors with Approve & Run / Discard plan… while planning, Stuck with Show what blocks, Needs your answer,
  members with each problem's actions (Check session / It arrived / Send again / Cancel attempt; Remind / Use last
  reply as report / Reassign…; Reassign… / Wait; Go to question; Start again), tasks as a graph from 400 px (soft
  dependencies dashed) or a list by layer, and the paged feed. A task opens a menu of its actions until M3's card.
- `use-last-reply` without text reads the member's last reply (`hgs [@host] inspect`) on the lead's computer.

## Desktop (M3–M6)

- **Task card (M3).** A task opens over the Team tab (and as a 400 px drawer in the workspace): status, attempt and
  role; the problem banner with the member's actions; description; criteria; Depends on / Blocks links (soft marked);
  assignee; attempts newest first with Open in Activity (revoked and rejected faded); the newest report; inline Reject
  with comment and Mark done manually (`accept` with `"manual":true` and a reason, shown in the feed). The footer
  shows the status's actions; the rest are under More, disabled with the reason.
- **Activity plaques (M3).** A message that starts with Zerus's team header (inside a session envelope or not) reads
  as a plaque: kind (Task from team, Report to lead, Planning brief, Message from you), sender → receiver, a link
  that opens the task's card, the attempt and the literal header; the body folds to three lines and hides the report
  command until Show more. A member session shows its team, role and current task in a strip above Activity.
- **Planning (M4).** While a team plans, the Team tab edits it: members' agent and model, role prompt, own worktree,
  remove, + Member from the catalog's roles; + Task and a task editor (title, description, criteria, assignee, hard or
  soft dependencies, delete); dependencies dragged from a node's handle onto another node and removed by selecting the
  edge (× or Delete). Each edit sends the whole plan once with the revision it was made against. The pinned footer
  holds Approve & Run (disabled while errors), Send back to lead (your comment goes to the lead's session) and
  Discard plan….
- **New team (M4).** Sessions ⋯ → New team…: name, catalog template, goal, folder (a non-Git folder disables
  worktrees with a warning), the lead (a new session with agent, account, model and effort, or an existing session),
  members from the template's roster with an error on each row that cannot work (computer offline, account not signed
  in), planning (lead plans or template tasks), approval and the member limit. Create sends `create` with
  `"start":true`: the dispatcher starts the lead if needed and sends it a planning brief once (goal, members, template
  tasks and the plan command); members start after Approve & Run.
- **Catalog (M4, M6).** `hgs team catalog get|set|propose|decide|apply|discard|route|designer --json`, stored in
  `root()/teams/catalog.json` (built-in roles and templates until first saved) and `catalog-draft.json`. `set`, `decide`,
  `apply`, `discard` and `designer` need `"by":"human"` from no session; `set` and `apply` check the version and write
  the next one; a draft made against an older version cannot be applied. Settings > Teams edits roles (color, prompt
  with its order preview, default agent, permissions, folder policy, can message, dangerous actions, specialization),
  templates (roster, task skeleton, planning, approval, max members) and routing rules with Try it; Import / Export as
  JSON. A member's role prompt travels with every hand-off.
- **Team workspace (M5).** The Team tab next to Activity (⤢ in the inspector's header opens it): team bar with
  Pause/Resume/Stop… or Approve & Run while planning, Stuck / plan-error strip, Graph (left to right, layer columns),
  Board (Pending · Active · Problems · Submitted · Accepted) or Layers, All/Active/Problems, member focus that keeps
  only that member's tasks and feed, zoom and Fit, the collapsible feed, Needs your answer, members, the Lead dock (its
  recent team events and a one-line message) and the task card as a drawer.
- **Team designer (M6).** Sessions ⋯ → Team designer… (or Settings > Teams → Ask Team designer) starts a session
  `…/team-designer` in the selected folder with a brief: read the project, change no file, propose with `hgs team
  catalog propose` (`hgs guide designer`). Its Team tab shows the draft: NEW / CHANGED / REMOVED proposals by kind
  with field-by-field old and new values, Keep / Reject / Undo, Discard draft, and Apply N changes (confirmed) as the
  next catalog version. Running teams keep their roster.
