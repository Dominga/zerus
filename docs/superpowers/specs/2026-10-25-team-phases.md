# Agent teams: phases, pinned tasks, waiting for the lead

WhiteWh fork. Extends `docs/superpowers/specs/2026-10-24-agent-teams-core-design.md` (core). Requirements: "Prompt 1 —
Phases" (owner, 2026-10-24). Owner decisions taken on 2026-10-24:

- A team whose tasks are all decided does **not** become Done by itself. It stays Running, "waiting for the lead"; the
  lead hears it once and either extends the team or finishes it with a summary. The owner can finish too; the owner's
  `stop` stays final.
- New work is appended as a new **phase** at the end of the plan. Existing tasks stay immutable: nothing is inserted
  before or into an earlier phase.

"Owner" below is `Actor::Human` (a request with `"by":"human"`: the desktop or a person at a terminal). "Lead" is the
team's lead session, proven by process ancestry as for every other request.

## Model

### Task

- `phase: u32`, serde default `1`, always serialized. The approved initial plan is phase 1.
- `pinned: bool` (exists; serde default `false`, skipped when false). `plan` and `extend` accept it; `reassign` still
  sets it.
- `plan` forces `phase = 1` on every task, whatever the input says. `extend` sets the computed phase; an input
  `phase` is ignored.

### Phase records

`Team.phases: Vec<Phase>` (serde default empty, skipped when empty). Phase 1 has **no record**; it is implicit (title
empty, no barrier). Every `extend` adds one record:

```json
{"n":2,"title":"QA fixes","barrier":true,"approval":"review","state":"draft","tasks":[18,19,20],
 "added_by":"Lead","added_at":1792868401.5,"decided_by":"","decided_at":null}
```

- `n`: `max(phase of every task, n of every record) + 1`. Numbers are never reused, not even after a discard.
- `title`: optional, trimmed, at most 120 characters; the GUI shows `Phase N` when empty.
- `barrier`: see Readiness.
- `approval`: the effective `phase_approval` when the phase was added (`review` or `auto`).
- `state`: `draft` (waits for the owner), `approved` (owner approved, or `auto`), `discarded`.
- `tasks`: the phase's task ids (at most 50). A discarded record keeps its ids so they are never reused.
- `added_by` / `decided_by`: the actor label (`Lead`, `You`); `decided_*` are set by approve-phase / discard-phase and
  stay empty/null for `auto`.
- At most **100** records per team (discarded included).

### Settings and templates

- `Settings.phase_approval: Option<Approval>` (`"review" | "auto"`), serde default `None`, skipped when `None`.
  Effective value: `phase_approval.unwrap_or(approval)`. A running team keeps its value; there is no operation to
  change it after creation (out of scope).
- Catalog `Template.phase_approval: String` (`""`, `"review"`, `"auto"`; `""` = same as the plan's approval). The New
  team flow copies a non-empty value into `settings.phase_approval`. `agent_guide/designer.md` documents the field.

### Team

- `idle: Notice` (serde default `none`, skipped when `none`): the "all tasks decided" message to the lead, with the
  same states and rules as `brief` (`pending` → `sending` → `sent` | `unconfirmed`; never re-sent after it may have
  arrived).
- `finish: Option<Finish>` (skipped when `None`): `{"by":"Lead","summary":"...","at":1792868401.5}` — set by `finish`.

### Old files

A team JSON without `phase`, `pinned`, `phases`, `phase_approval`, `idle` or `finish` loads unchanged: every task is
phase 1, nothing pinned, effective phase approval = `approval`, no message pending. A team already `done` stays done.
Serializing an unchanged old team adds only `"phase":1` to each task.

## Readiness (barrier)

`plan::ready` keeps today's rules (Pending, not in a cycle, dependencies satisfied) and adds one: a task whose phase
has a record with `barrier: true` is not ready while any task of a **lower** phase is not finished (`accepted`,
`rejected` or `cancelled`). A Draft task of a lower phase is not finished, so it also holds the barrier. Without a
barrier, a new phase's tasks are ready as soon as their own dependencies are satisfied.

## Pinned tasks

The dispatcher's rebalancing within a role (`steps`, "a ready task goes to a free member with the same role", and "a
retry prefers a member that did not fail it") never moves a pinned task, whether it was pinned by `plan`, `extend` or
`reassign`. A pinned task waits for its assignee. Tasks assigned to the lead are never rebalanced (unchanged).

## Commands

All are mutations: one JSON object on stdin with `--json`, a canonical lowercase `request_id`, listed in `MUTATIONS`
and in `USAGE`. Idempotency is the store's existing rule: the same `request_id` with the same input returns the first
receipt (unchanged revision); the same `request_id` with another input is refused ("This request_id was already used
for another request"). Every refusal below is an error (non-zero exit, message on stderr) and changes nothing.

### `hgs team extend --json` (lead or owner)

```json
{"request_id":"UUID","team":"ID","phase_title":"QA fixes","barrier":true,
 "tasks":[{"id":18,"title":"...","description":"...","criteria":["..."],
           "assignee":"backend","deps":[{"task":13,"kind":"hard"},{"task":19,"kind":"soft"}],"pinned":true}]}
```

Receipt:

```json
{"status":"added","phase":2,"tasks":[18,19],"approval":"review","validation":{"errors":[],"warnings":[]},
 "request_id":"UUID","revision":42}
```

- `phase_title` optional (default empty), `barrier` optional (default `false`), `pinned` optional (default `false`).
- Task fields are parsed and bounded like `plan` (`parse_task`: title ≤ 200, description ≤ 20,000, each criterion
  ≤ 500, duplicate deps dropped, no self-dependency).
- Refusals, in this order:
  - actor not the lead or the owner: "Only the lead or you extend the team";
  - status not Running or Paused: "Only a running or paused team takes a new phase" (Planning: "use plan";
    Done, Stopped, Archived: refused the same way);
  - `tasks` missing or empty: "A phase needs at least one task";
  - more than **50** tasks: "A phase has at most 50 tasks";
  - the team would exceed **500** tasks: "A team has at most 500 tasks (it has N)";
  - 100 phase records exist: "A team has at most 100 phases";
  - effective approval `review` and a phase is still `draft`: "Phase N waits for the owner's approval";
  - ids not exactly `next .. next+count-1` in any order, where `next` = 1 + the highest id among current tasks and all
    phase records: "New tasks are numbered from #next";
  - a dependency on a task that is neither an existing task nor in the batch; a cycle; an assignee that is not the
    lead or a member that is not Removed: the `plan::validate` findings that name a new task (or any cycle) are
    joined into the error. Findings about older tasks never block (those tasks are immutable). Warnings (offline
    computers) are returned, not refused.
- Effect: the tasks are appended with `phase = n`, status `draft` (effective approval `review`) or `pending` (`auto`);
  a phase record is added; `idle` becomes `none` (see Waiting for the lead). Event `phase_added`.

### `hgs team approve-phase --json` (owner)

`{"request_id":"UUID","team":"ID","phase":2}` → `{"status":"phase_approved","phase":2,"tasks":[18,19]}`

- Refused unless the actor is the owner ("You approve a phase in Zerus"), the team is Running or Paused, and the phase
  record exists with state `draft` ("Phase N is not waiting for approval").
- Re-validates the phase's tasks (assignees may have been removed since): errors that name its tasks refuse, as
  `approve` does for a plan.
- Effect: the phase's Draft tasks become Pending; record `state: approved`, `decided_by`, `decided_at`. Event
  `phase_approved`.

### `hgs team discard-phase --json` (owner)

`{"request_id":"UUID","team":"ID","phase":2}` → `{"status":"phase_discarded","phase":2,"tasks":[18,19]}`

- Same actor, status and `draft` checks as approve-phase. Only one phase can be draft and it is always the last one,
  so no other task depends on it.
- Effect: the phase's tasks are removed from `tasks`; the record stays with `state: discarded` (its ids stay used).
  Event `phase_discarded`. If every remaining task is decided, the team becomes waiting for the lead again.

### `hgs team finish --json` (lead or owner)

`{"request_id":"UUID","team":"ID","summary":"..."}` → `{"status":"finished"}`

- Refused unless the actor is the lead or the owner ("Only the lead or you finish the team"), the team is Running or
  Paused, the summary is non-empty after trimming and at most **4,000** characters, and every task is finished
  ("N task(s) are not decided: decide or cancel them first"; a draft phase counts — discard it first). The owner ends
  a team with open work with `stop`, as before.
- Effect: status Done, `finish = {by, summary, at}`, `idle` none. Event `finished`. Done is final; `archive` works as
  before.

### Unchanged

`stop` (owner only, cancels every unfinished task, Draft included; final), `pause`, `resume`, `accept`, `reject`,
`reassign`, `retry`, `cancel`, `take-over`. `plan` still works only in Planning and replaces the whole plan.

## Waiting for the lead

- `ops::finish_if_done` is removed: no operation makes a team Done except `finish`.
- **All decided** = the team has at least one task and every task is finished (Draft counts as not finished).
- **Waiting for the lead** (derived, never stored) = status Running and all decided. Paused with all decided is shown
  as Paused.
- `ops::apply` compares all-decided before and after every operation on a Running or Paused team:
  - false → true: `idle = pending`, event `all_decided`;
  - → false (an extend, a human re-decision that reopens a task): `idle = none`.
- The dispatcher, while the team is Running and the lead's session is free, sends once (as `send_brief` does: mark
  `sending` in a mutation, send, then `sent` or `unconfirmed`, or back to `pending` on a refusal; a follow-up change
  writes only if `idle` is still `sending`):

  ```
  [Zerus team · from: Zerus · all tasks decided]
  Team: NAME. All N tasks are decided (A accepted, R rejected, C cancelled).
  Add the next phase with hgs team extend --json, or finish the team with a summary (hgs guide team-lead):
    {"request_id":"UUID","team":"ID","summary":"..."} | hgs team finish --json
  ```

  It records event `lead_told` with `{"member":"Lead","kind":"all_decided"}`. The lead is told again only after the team
  stops being all-decided and becomes all-decided again.

## Events

New feed kinds and payloads (bounded; no task text beyond the phase title):

| kind | payload |
|---|---|
| `phase_added` | `{"by","phase","title","tasks":count,"barrier","approval"}` |
| `phase_approved` | `{"by","phase","tasks":count}` |
| `phase_discarded` | `{"by","phase","tasks":count}` |
| `all_decided` | `{"accepted","rejected","cancelled","tasks"}` |
| `finished` | `{"by","accepted","tasks","summary_chars"}` |
| `lead_told` | existing kind; new `kind` value `all_decided` |

The old `done` kind is no longer written; the GUI keeps rendering it for old feeds.

## Status JSON (`hgs team status --json`)

The team as stored (so `tasks[].phase`, `tasks[].pinned`, `phases`, `settings.phase_approval`, `idle`, `finish`) plus
the existing `validation`, `layers`, `cycle`, `ready`, and two derived fields:

- `"waiting_for_lead": bool` (definition above);
- `"pending_phase": N | null` — the phase whose record is `draft`.

`layers` stays computed over all tasks; the GUI groups by `phase` itself. `team list` (the session list summary) adds
`waiting_for_lead` and `pending_phase` too.

## Hand-off: "From dependencies"

`hand_off_text` keeps the `Depends on:` line and adds, after it, a section for every hard or soft dependency that has a
report, in `deps` order:

```
From dependencies:
#5 Model: phase, pinned (accepted) by backend (Backend), folder: D:\Tools\Zerus-team-f3d22e71-backend
<report text>
```

- The report: if the dependency is Accepted, its last attempt in state `reported` with a report; otherwise its latest
  attempt with a report (any state). A dependency without any report is not listed here.
- The folder: the member's working folder as `member_folder` would give it (the team's or its own folder, or the
  generated worktree path), computed without creating anything; prefixed `host:` when the member's computer is not the
  lead's. The lead's tasks show the team's folder.
- Bounds: each report cut to **2,000** characters (at a character boundary) and **6,000** characters of report text in
  total. A cut report ends with `[cut: shown X of Y characters; full report: hgs team status --json, task #N]`. Once
  the total is used up, the remaining dependencies show only their header line and `[report omitted: limit reached]`.
- Only stored report text is used; journals and transcripts are never read.

## Lead guidance

- New page `src/agent_guide/team-lead.md` (`hgs guide team-lead`, ASCII): extend with an example JSON, phases at the
  end only, barrier, pinned, review vs auto approval, waiting for the lead, finish with a summary.
- `team.md` gets one pointer line and stays ≤ 2,400 bytes ASCII. `brief_text` points the lead to `hgs guide team-lead`.

## Desktop

Follows the existing skin and `design/agent-teams` prototypes; no new visual language.

- **Model** (`TeamModel`): parses `phase` (default 1), `pinned`, `phases`, `settings.phase_approval`, `idle`, `finish`,
  `waiting_for_lead`, `pending_phase`; old JSON without them parses. **Controller** (`TeamController`): sends
  `approve-phase` / `discard-phase` (`phase`) and `finish` (`summary`) with `"by":"human"` and a fresh request id kept
  for retries.
- **Grouping** (plan, board, layers): only when the team has a phase record; a team with phase 1 only looks as today.
  Each phase has a header `Phase N · title · done/total` (done = finished tasks, total = all tasks of the phase) with a
  `barrier` tag when set and a `draft` tag while it waits. A phase whose tasks are all finished starts collapsed; the
  others start open; the owner's toggles are kept per team in memory for the session (bounded, no persistence). A
  collapsed phase builds no task widgets (cards, rows, graph nodes) until expanded.
- **Graph**: phases left to right as labelled bands (each phase's own layers inside its band); a collapsed phase is
  one summary node `Phase N · done/total`, and edges from its tasks attach to that node. `keepScroll` keeps working.
- **Draft phase**: its header shows **Approve phase** and **Discard phase** (Discard asks to confirm: "Discard phase N
  and its K tasks?"). The team header shows the chip `phase N pending approval`.
- **Waiting for the lead**: the team header shows the chip `waiting for the lead`, and a **Finish** button (owner;
  shown when all tasks are decided) opens a confirmation with a required summary field (≤ 4,000 characters). A Done
  team shows its `finish` summary and who finished it.
- **Pin mark** on pinned task cards, next to the assignee, tooltip `Pinned: stays with <member>`.
- **Activity** team plaques render `phase_added` ("Lead added phase 2 · QA fixes · 4 tasks"), `phase_approved`,
  `phase_discarded`, `all_decided` ("All tasks decided · waiting for the lead"), `finished`, and `lead_told` with
  `all_decided`.
- **Catalog editor**: template field "New phases: same as plan / owner approves / lead runs" (`""`, `review`,
  `auto`); New team passes it as `settings.phase_approval`.
- Offscreen shots (`ZERUS_TEAM_SHOTS`): a draft phase with its buttons, collapsed and open phases, waiting for the lead
  with Finish, a pin mark; light and dark.

## Tests

- Rust: extend in each team status and by each actor; ids, deps (existing and batch), cycles, assignees, limits; one
  draft phase at a time; approve/discard in review, Pending at once in auto; barrier readiness; pinned never rebalanced
  (plan, extend, reassign); all-decided transitions, one `idle` message, re-arm after extend; finish rules; hand-off
  bounds and report choice; old JSON loads; `request_id` retry returns the first receipt.
- Qt: model parsing (new and old JSON), controller payloads, grouping and collapse (no widgets for collapsed), draft
  phase buttons and confirmation, waiting chip and Finish, pin mark, activity plaques.
