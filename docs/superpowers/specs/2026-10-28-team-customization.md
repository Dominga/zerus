# Team customization for outside users and moving accounts

Status: decided by the lead for phase 43 "Team customization for outside users
and the account of a running session (U45-U54)" of team "Teams v2",
2026-10-29. Plan: `docs/superpowers/plans/2026-10-25-teams-next-phase.md`
items U45-U54. Source: an outside user's migration notes of 2026-10-28 (a
pipeline of a tracker, dispatch scripts, pre-review gates and a landing queue
moving onto Zerus teams), their section 3 priorities 1-7. Builds on
`2026-10-26-team-delivery.md`, `2026-10-26-team-permissions-and-lead-load.md`
and the U36 pipeline model (commit 88d41c16, integrated by #537).

Owners: backend3 U45 (#572); backend2 U46 (#573); expert U47, U48 (#574);
backend U49 (#575) and the protocol and docs (#579); backend4 U51, U52 (#576),
U53 (#617), U54 (#619); frontend the desktop fields (#577); frontend2 U50
(#578), the U53 picker (#618) and the U54 desktop (#620).

## 0. Rules for every item

- **Opt-in.** Every new field has a serde default and is skipped when empty
  or default. A team file, catalog, template or plan written before this phase
  loads unchanged and behaves exactly as today. Each server task carries one
  test that loads a team with none of the new fields, runs a hand-out, a
  report, a stage and an accept, and compares the events with today's.
- **Who sets them.** The owner, in the catalog (roles, templates,
  pipelines), the team settings or the plan. The lead sets task-level fields
  (`models`, `role`, `pipeline`) in `plan`, `extend` and `edit-draft`, as it
  sets assignees today. Members never set any of them.
- **Never the wrong state silently.** Every step Zerus takes for a member
  before a hand-out (model switch, context, branch) is confirmed before the
  hand-out. A refused step leaves the task waiting with its reason
  (`task.waits = {for:"zerus", on:[...], reason}`) and one inbox item for the
  lead; an unconfirmed one is recorded `uncertain` and never retried on its
  own. The lead or the owner retries it with `hgs team retry` or reassigns.
- **Bounded.** New per-task and per-member lists keep the last 20 entries;
  outputs kept for findings are tails of at most 4000 characters.

## 1. The hand-out order

For a ready task and a chosen member, the dispatcher runs these steps in this
order, each as its own pass so a restart resumes at the right step:

1. **Choose the member** (U45): the assignee, or a free member of the task's
   `role` whose `models` allow one of the task's `models`.
2. **Branch** (U48): with `branch_per_task`, create or check out the task's
   branch in the member's folder.
3. **Model** (U45): switch model and effort if the member's current ones are
   not the task's choice.
4. **Context** (U46): run `compact` or `clear` if the role or member asks.
5. **Hand out** the attempt, as today.

Steps 2-4 record `attempt.prepare = [{step, from, to, at, result:
"done|skipped|refused|uncertain", reason}]`. A pipeline stage hand-out (U36
stage delivery) runs steps 3 and 4 for the stage member; never step 2.

## 2. U45: a model per task, and a task for a role

Fields:

- `Member.models: [{model, effort}]`, default empty, meaning only the launch
  `model`/`effort`. Set from the role in the catalog (`Role.models`) when the
  member joins (copied into the prompt snapshot, so a later catalog edit
  changes no running team), editable by the owner in the team settings.
- `Task.models: [String]`, default empty: any model of the assignee. Each
  entry is a model id or `model@effort`.
- `Task.role: String`, default empty. A task with `role` and an empty
  `assignee` is a task for a role; `pinned` must be false. A task with both is
  refused at `plan`/`extend`/`edit-draft` ("A task names an assignee or a
  role, not both").

Choosing: among free members (no open attempt, no stage run, not paused, not
limited) with `role`, the first whose `models` (or launch model) intersects
the task's `models`, in roster order; ties keep today's rebalancing order. A
member already on an allowed model is preferred over one that needs a switch.
Rebalancing (`dispatcher.rs` ~928) applies the same rule: it never moves a task
to a member that cannot run one of its models.

Switching: the member's current model and effort come from its session record
(`model`, `effort`). If they differ from the chosen entry, Zerus runs
`hgs settings SESSION --json {model, effort}` while the member is idle
(today's `Step::Settings` path generalized from dsh to every agent whose
`settings` supports it: Claude, Codex, DeepSeek). It hands out only after the
session record shows the new model (polled up to 60 s). `settings_applied`
becomes per change, not once per member.

Failure paths and tests:

- The agent has no settings support (OpenCode, Kimi today): the member is not
  a candidate for a task whose models need a switch; a pinned assignee leaves
  the task waiting "Kimi cannot switch to model X" (test).
- `settings` refuses (busy, unknown model): `refused`, the task waits with
  the stderr line (test).
- No confirmation in 60 s: `uncertain`, waits, no retry (test).
- A task for a role with no member of that role: plan validation error; with
  members but none allowing the models: a health warning at plan time and a
  waiting reason at run time (tests).
- Default: no `models`, no `role` gives today's hand-out (test).

## 3. U46: the context before a task

Field: `context_before_task: "none" | "compact" | "clear"` on `Role` (catalog,
copied into the snapshot) and on `Member` (overrides the role), default
`none`.

Before each hand-out, the stage hand-outs included, Zerus runs `hgs
compact-context` or `hgs clear-context` on the idle member session, waits for
the command's confirmation (its receipt and the session record's
`compact_context_request` or new conversation) and only then hands out. With
`clear` the hand-off text names only the task's own input (title,
description, criteria, deps' reports, the continuation or reopen note), never
"as before".

A clear may change the session's conversation id (Codex starts a new
conversation): the member's `SessionRef.conversation_id` follows the session
record before the hand-out, so identity checks (`calling_member`,
`dsh_member`, expected ids in sends) keep the member (see U54 rename).

Failure paths and tests: an agent without native context actions (OpenCode)
gets `none` with one feed note per member ("OpenCode has no clear; the task
continues the conversation") (test); a refused action leaves the task waiting
(test); unconfirmed is `uncertain`, no retry (test); compact and clear each
wait for confirmation before the hand-out (tests); default `none` sends
nothing extra (test).

## 4. U47: a stage Zerus runs itself

A pipeline preset (U36 `PipelineStage`) gains `kind: "command"` with
`command: {command, expect_exit (default 0), timeout_minutes (default 30)}`.
Only the owner's catalog defines it; the plan can choose the preset, never
write the command. Validation: the command is a non-empty string up to 2000
characters; the preset editor shows it in full.

Running: when the task enters the stage, Zerus starts it with no model as a
detached run in the stage's folder (the task's folder by the preset's folder
rule), `hgs run` semantics: the session shows Building, its output is kept in
the run log. The stage run records `{exit, tail, at}` (`CheckResult`). The
exit equal to `expect_exit` is `pass`; any other is `return` with the tail as
the finding, to the implementer as a new attempt, without the lead, counting
toward `max_rounds`. A timeout is `return` with "timed out after N min". The
same tail on the same commit twice in a row is marked `flaky` on the stage
run and goes to the lead instead of the implementer (the notes' flake rule).

Tests: pass, return with the tail, timeout, flaky on the same commit, the
command never taken from the plan (tests, a real Git folder with a script that
exits 0 or 1).

## 5. U48: landing and a branch per task

`kind: "land"` is a command stage that is the preset's last stage; it runs
once every earlier stage passed. Its `exits` map exit codes to outcomes:
`{"0":"accepted","2":"returned","3":"held"}`, default `{0: accepted}` and any
other `held`. `accepted` accepts the task (`decided_by: zerus`, the stage named
in the feed); `returned` (conflict, red) returns it to the implementer with the
tail; `held` (busy, infrastructure) puts it on `hold` for the lead with the
tail. A land stage needs the owner's `land_allowed: true` in the team
settings, because it may push: without it the preset is refused at save.

`Settings.branch_per_task: {template: "task/{id}"}`, default absent (one
branch per member, as today). Template fields: `{id}`, `{member}`, `{team8}`;
the result must be a valid Git branch name. Before each hand-out Zerus runs, in
the member's folder through `hgs worktrees`, outside any sandbox: refuse if
the folder has uncommitted changes (task waits "folder has changes"); create
the branch from the team's base (`Settings.base_branch`, default the lead's
branch) if missing, else check it out. A reopened or returned task gets its
existing branch. `hgs team commit` accepts the task's branch as the member's
team branch while the attempt is open. Branches are never deleted.

Tests: a real Git repository: two tasks of one member land as squash on the
base, the second branch does not carry the first's commits; returned and held
exits; a dirty folder waits; default keeps `team/<id8>/<member>` (tests).

## 6. U49: scale and drain

- `hgs team scale --json {request_id, team, role, agent, model?, effort?,
  account?, count}`: adds `count` members `role2`, `role3`... as `add-member`
  does, within `max_members` (refused as a whole when it does not fit). The
  owner, or the lead on the owner's word naming the role and the count (U32
  `WordRule` with `all: [count]`).
- `hgs team drain --json {request_id, team, member}`: the member finishes its
  open attempt and stage run, gets nothing new (`member.draining = true`, a
  rebalancing source), then Zerus pauses its session (never a kill) and
  removes it (`remove-member`). Owner, or the lead on the owner's remove word.
  Undo before it ends: `{"member", "draining": false}`.

Tests: scale fits and does not fit, the word rule; drain with an open attempt,
with a stage run, with nothing; cancel (tests).

## 7. U50: one list of what waits for the owner

Read-only `hgs teams waiting --json` (bounded pages, newest first) and a
desktop page beside Teams: across all teams, items of kinds `phase_draft`,
`plan_draft`, `report_escalated`, `permission_escalated`, `owner_word_asked`,
`task_waits_owner`, `team_finished`, each `{team, team_name, kind, title,
since, action}`. It reads what the teams already record (attention, waits,
phases); it stores nothing. The phone follows with the team status API (U44).

Tests: one fixture per kind; paging stable while items arrive; an empty list
(tests, shots in both themes).

Built (#578): the read is `hgs team waiting --json`, beside the other
cross-team reads (`list`, `leftovers`), over the 1000 most recently changed
Planning, Running and Paused teams. Input `{limit, before}`: `limit` 1-200
(default 50), `before` the `next` cursor `{since, id}` of the previous page.
Answer `{items, next, total}`, newest first, ties by `id`
(`<team>/<kind>/<key>`); items may add `phase`, `task`, `member`,
`proposal` or `finding`. Each kind comes from a record the team keeps:
`phase_draft` the attention list's next draft phase; `plan_draft` a Planning
team with tasks whose approval is `review`; `report_escalated` and
`permission_escalated` the attention list's `report` and `permission`, past
`remind_owner_minutes` and `remind_owner_permission_minutes`;
`owner_word_asked` an open lead proposal (F8), an addition to a running
phase waiting for its confirmation (U29) and a paused replan (N17);
`task_waits_owner` a task the owner holds (A5); `team_finished` a Running or
Paused team with every task decided and no draft phase. `task.waits` (U45)
joins `task_waits_owner` once it exists. The desktop page is the rail's
Waiting for you, after the Team designer: pages of 50 with Show older; a
poll reads every shown item and a page more, so an arrival never pushes a
shown item out; a row opens its team's Team tab, a draft phase unfolded.

## 8. U51: an intake for urgent tasks

`hgs team intake-key --json {request_id, team, label}` (owner only) creates a
key, kept hashed in the team (`intakes: [{id, label, hash, created}]`), shown
once. `hgs team intake --json {key, team, tasks}` (no session identity
needed) adds the tasks as one urgent phase (barrier off, approval `auto`, at
most 5 tasks, title "Urgent: <label>"), assignees or roles as in the plan; the
feed names the key's label. Every other op with a key is refused (test). The
owner revokes a key by id. Without a key nothing changes.

## 9. U52: restart a member that crashed

`Settings.restart_crashed: bool`, default false. A member session whose process
exits while Zerus did not stop, pause or swap it and it is not limited
(usage limit, H1) is launched again into the same folder and conversation
(resume), at most 3 times per hour per member (`member.restarts: [at]`, last
20). Its open attempt is returned as `uncertain` for the lead ("the session
crashed at T; restarted"). The fourth crash in an hour leaves it stopped with
one inbox item. Tests: restart, the hourly limit, a pause or a limit never
restarts, default off (tests).

## 10. U53: change the account of a running session

`hgs switch-account NAME --account ID --request-id UUID` (JSON receipt), the
owner's explicit choice from the composer's account picker (desktop) or the
CLI outside a session.

- Preconditions, each refused with its reason: the target account exists, has
  the same provider and is signed in; the session is idle (no turn, tool,
  question, held input, Building job).
- Steps: copy the conversation into the target account's native store (Claude:
  `projects/<project>/<conversation>.jsonl` and its folder; Codex: the
  session file), stop the native process, launch it resumed into the same
  conversation, folder and session name with the new account, confirm the
  resumed conversation id, write the receipt `switched`.
- A failed launch relaunches on the old account (`rolled_back`); an
  unconfirmed one is `uncertain`, never retried. The copy is kept; nothing is
  deleted from the old account.
- `inspect` and `ls` carry `account_id` and its label. A team member's account
  changes only through `swap-member`, which uses this path for the same
  provider (U54).
- Isolated real-agent contract tests for the latest Claude and Codex.

## 11. U54: move a team to another account

From the 2026-10-29 move of team 528aebfe off an account near its limit.

- **Bulk swap.** `swap-member` takes `{from_account, to_account, provider}`
  instead of `member`: every member of that provider whose session runs on
  `from_account` (read from the session record, not only `member.account`),
  the lead included. One confirmation in the desktop lists them. Status and the
  roster show `session_account` beside `account`.
- **Same provider keeps the conversation.** A swap that changes only the
  account uses U53's switch: same session, conversation and attempt, no
  continuation. A fresh session stays the fallback (U53 refused) and the path
  for another agent.
- **The old session never blocks.** Before launching a member's new session
  of the same name, Zerus pauses the old one once idle (`hgs pause`: the
  conversation stays under Saved; never kill) and renames it to
  `<name>-<old account label>`: a paused session keeps its account binding,
  and a launch with an explicit account refuses a name that has one
  (`src/cli.rs`, "This session already has an account"). A busy old session makes the
  member wait "waiting for the old session to finish its turn"; it is never
  interrupted without the owner's choice.
- **Retry after a failed launch.** A failed post-swap launch keeps
  `launch_after_swap`; `relaunch` launches at once; the error names its fix.
- **A folder on another branch.** If the member's folder is on another branch
  (a task branch left behind) and is clean, and that branch is merged into the
  member's or the lead's branch, Zerus checks out the member's branch (the
  other branch is kept) and records it in the feed. Otherwise the member error
  offers "Check out <branch>" as a review action; never a reset.
- **A renamed session stays the member.** `calling_member` matches a caller by
  `run_id` and `conversation_id` as well as by name, and a session rename
  (`hgs rename`) updates the member's stored `SessionRef.name`. Today a renamed
  lead is refused every team command ("This session is not a member").
- **The lead on the owner's word.** The lead may swap, relaunch and pause the
  old sessions of its own team on the owner's verified roster word (U26); a
  swap to an existing session stays owner-confirmed.
- **The new lead's brief** names the previous lead's session, conversation id,
  transcript path and its latest compact summary (at most 8000 characters).

Tests: bulk swap selection by the session's real account; same-provider swap
through switch-account; the old session paused before launch, a busy one
waits; relaunch after a failed launch; the folder checkout rule (clean and
merged, dirty, unmerged); rename keeps the member; the lead on the owner's
word; the brief (tests, one real-agent contract test for the swap).

## 12. Docs and the lead protocol (#579)

`hgs guide team-lead` gains one paragraph per field with its default;
`docs/reference.md` gets the JSON shapes above; the lead protocol (E) adds:
write tasks for a role with `models` when the owner's catalog has model
lists; clear-context roles get self-contained descriptions; a land stage is
the owner's to configure.

## 13. Examples: an outside pipeline

A user moving a tracker and its dispatch scripts onto Zerus configures the
owner's side once — roles, a preset and the team settings — and leaves the
plan to the lead. The example is one such pipeline: an implementer role with
several models and a clear context, a reviewer, a command stage that runs the
project's tests and a land stage that pushes.

The catalog is the whole object (`hgs team catalog get`), saved with the
version just read; these are the parts the pipeline adds (keep `templates`,
`routing` and `planning_rules` from the `get`):

```json
{"version":3,"catalog":{
  "roles":[
    {"id":"implementer","name":"Implementer","agent":"claude",
     "model":"claude-sonnet-4-5","folder":"worktree",
     "models":[{"model":"claude-opus-4-1","effort":"high"},
               {"model":"claude-sonnet-4-5"}],
     "context_before_task":"clear"},
    {"id":"reviewer","name":"Reviewer","agent":"codex","model":"gpt-5",
     "models":[{"model":"gpt-5"},{"model":"gpt-5-codex"}],
     "context_before_task":"compact"}],
  "pipelines":[
    {"id":"deliver","name":"Deliver","max_rounds":2,"folder":"checkout",
     "accept":"lead","stages":[
       {"kind":"review","role":"reviewer"},
       {"kind":"command","command":{"command":"npm test -- --runInBand",
         "expect_exit":0,"timeout_minutes":20}},
       {"kind":"land","exits":{"0":"accepted","2":"returned","3":"held"},
        "command":{"command":"./scripts/land.sh","timeout_minutes":10}}]}]}}
```

The team settings ask for a branch per task, a base to start it from, the
land stage the preset needs and restarts:

```json
{"request_id":"<uuid>","team":"<id>","by":"human","settings":{
  "max_members":10,"branch_per_task":{"template":"task/{id}"},
  "base_branch":"main","land_allowed":true,"restart_crashed":true}} |
  hgs team settings --json
```

`"context_before_task":"clear"` starts a new conversation for every task of
the Implementer, so each task description must stand alone; `compact` keeps
the conversation and folds it. `branch_per_task` gives task 1 the branch
`task/1` from `main`, and the land stage runs only because `land_allowed` is
true.

The lead's plan names the role, the models and the preset:

```json
{"members":[
  {"id":"impl1","role":"Implementer","agent":"claude",
   "model":"claude-sonnet-4-5"},
  {"id":"rev1","role":"Reviewer","agent":"codex","model":"gpt-5"}],
 "tasks":[{"id":1,"title":"Fix the checkout crash","role":"Implementer",
   "models":["claude-opus-4-1"],"pipeline":"deliver",
   "criteria":["Carts survive a reload"]}]}
```

Zerus gives task #1 to `impl1` (the role's member whose models allow one of
the task's), switches it to `claude-opus-4-1` with its `effort: high`, waits
for the session to confirm, clears its context, creates
`task/1` and hands the task out with its full description. The implementer
reports; `rev1` reviews; the command stage runs `npm test`; the land stage runs
`./scripts/land.sh` and maps its exit: 0 accepts the task, 2 returns it to the
implementer with the output, 3 holds it for the lead. A returned task keeps
`task/1`.

The owner grows and shrinks the roster as the queue does:

```sh
{"request_id":"<uuid>","team":"<id>","role":"Implementer","agent":"claude",
 "count":2} | hgs team scale --json
{"request_id":"<uuid>","team":"<id>","member":"impl1"} | hgs team drain --json
```

and lets a health watcher add an urgent task without any of the owner's other
rights:

```sh
{"request_id":"<uuid>","team":"<id>","label":"health watch","by":"human"} |
  hgs team intake-key --json
{"key":"<the key shown once>","team":"<id>",
 "tasks":[{"title":"Fix the outage","role":"Implementer"}]} |
  hgs team intake --json
```

`restart_crashed` in the same settings launches a crashed member again, at most
three times an hour, and returns its open attempt to the lead as `uncertain`;
the fourth crash in an hour leaves it stopped for the owner to look at. The
key's urgent phase is `Urgent: health watch`, approved at once, without a
barrier and ahead of the other ready tasks; the key adds nothing else.

