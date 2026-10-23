# Agent teams core (M1): state, `hgs team`, dispatcher

The first milestone of agent teams: the team record, the `hgs team` command and the dispatcher. The desktop UI
builds on it in a separate change.

## Decisions (2026-10-24)

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

## Rules

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
