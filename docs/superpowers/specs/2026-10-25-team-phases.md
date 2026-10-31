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

### `hgs team return-to-draft --json` (owner, or the lead on the owner's word; U42, #587)

`{"request_id":"UUID","team":"ID","phase":3}` → `{"status":"phase_returned_to_draft","phase":3,"from_phase":3,"tasks":[21],"renumbered":{"phases":[],"tasks":[]}}`

- An approved or queued phase whose work has not started (every task Pending or Draft with no attempt beyond a
  revoked or refused one) goes back to a draft that waits for approval again; a done phase and started work are
  refused and named. Refused while `auto_accept` is on, which would accept it again at once.
- The lead passes `owner_word` with the owner's sentence from its own session (under 30 minutes) naming the phase
  and the draft ("return phase 3 to draft", "верни фазу 3 в черновик"); `discard-phase` of an accepted phase takes
  the lead's word the same way, naming the phase and the discard. A word is spent on that one change.
- Effect: the phase's Pending tasks become Draft and `decided_*`, `owner_word` and a pause clear. U63 (owner,
  2026-10-31): the phase keeps its place in the order, its number and its task ids; nothing is renumbered
  (`from_phase` equals `phase` and `renumbered` is empty, kept for older callers). Event `phase_returned_to_draft`
  `{by, phase, title, tasks, from, owner_word}`.
- To change an accepted phase that has not started, edit it in place (see Phase control below); return it to a draft
  only when it must be approved again.
- A `plan` edit while planning keeps each phase it already had: its order (a move stays), `added_by` and `added_at`;
  a new phase goes after them by number.

## Phase control (U63, owner 2026-10-31)

**Order.** A phase keeps its place when it is edited or returned to a draft. Numbering (F285) renumbers only the
drafts after the last accepted phase in the display order (`order`, then `n`), on a discard, a move and the compaction
of an old discarded record; a draft between accepted phases keeps its number and task ids.

**Changing an accepted phase.** An approved or queued phase whose work has not started (no attempt beyond a refused
one) is changed in place and stays accepted:

- `edit-draft` and `extend` with `"phase"` (U58), and `move-phase`, which now moves such a phase too: `phase` and
  `to` must each be a draft or an accepted phase that has not started. After a move, an approved phase in the moved
  range that has open work before it becomes `queued`; barriers read the new order.
- The owner does it directly. The lead does it only with `owner_word`, the owner's sentence naming the phase and the
  change ("change phase 3: ...", "move phase 3 before phase 2"), verified by the CLI as for `return-to-draft`;
  without it: "The lead edits an accepted phase only on the owner's word: ...". Drafts stay the lead's own.
- Every change recorded on an accepted phase carries `"accepted": true` in `changes[]`, so the owner sees the lead's
  changes after the approval too. `phase_edited` carries `owner_word`.

**Pause.** `hgs team pause-phase --json` `{"request_id","team","phase","reason"?}` (owner, or the lead on its own)
holds an approved or queued phase whose work has not started → `{"status":"phase_paused","phase":N,"pause":{by,
owner, at, reason}}`, event `phase_paused`. The record keeps `pause {by, owner, at, reason}`. Its tasks are not
handed out and wait "Paused by ...: reason"; it keeps its place, so a later barrier or queued phase waits for it as
for an unfinished phase, and a queued paused phase does not start. `hgs team unpause-phase --json`
`{"request_id","team","phase"}` → `{"status":"phase_unpaused"}`, event `phase_unpaused`: the owner's pause only the
owner removes; the lead removes its own pause by itself when the owner set `settings.lead_removes_own_pause`, else
(the default) on the owner's word naming the phase ("remove the pause of phase 3"). `return-to-draft` clears a pause.

**Owner acceptance.** A phase record carries `acceptance: "lead" | "owner"` (skipped when `lead`); a new phase takes
`settings.phase_acceptance` (default `lead`, the old behaviour; urgent phases always `lead`). `hgs team
phase-acceptance --json` `{"request_id","team","phase","acceptance"}` sets it for a phase that is not done (the
owner; the lead on the owner's word), event `phase_acceptance_changed`.

- When every task of an approved owner-acceptance phase is decided, it becomes `awaiting_acceptance`
  (`awaiting_since`, event `phase_awaiting_acceptance {phase,title,accepted,rejected,cancelled}`); `phase_done` waits.
  While it waits (and while its rework after a send-back is open), no later phase in the display order starts:
  `ready`, start-ahead, work ahead and queued phases are held, urgent and non-barrier phases included. `finish` is
  refused.
- `hgs team accept-phase --json` `{"request_id","team","phase"}` (the owner; the lead on the owner's word, "phase 3
  accepted") → `{"status":"phase_accepted"}`: the record becomes `done`, event `phase_owner_accepted`, then
  `phase_done`. `done` is reached only through this path; a phase the lead accepts stays `approved` when finished.
- `hgs team send-back-phase --json` `{"request_id","team","phase","comment"}` (owner) → `{"status":"phase_sent_back"}`:
  the phase is `approved` again with `acceptance_rework`, `rejection` and a note; event `phase_acceptance_sent_back`.
  The lead's inbox shows `phase_acceptance_sent_back`; the lead adds the rework to the same phase with `extend`
  `"phase"` (no word needed), and the phase waits for the acceptance again once that work is decided.
- The owner's attention list and Waiting for you carry `phase_acceptance` (action `accept_phase`) with a summary
  `{accepted, rejected, cancelled, tests, minutes}`. The lead's session reads "Waiting for you to accept phase N"
  (`team_phase_acceptance`, a Waiting for you reason under the existing contract) while nothing else is out; a
  member held behind it waits for the owner. Task timelines show `phase_pause` and `owner_acceptance` spans.

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
#5 Model: phase, pinned (accepted) by backend (Backend), folder: D:\work\Zerus-team-f3d22e71-backend
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

## Owner's phase model (U25, server)

The owner's design (`SPEC.md` 5.2 of the UI handoff) names five phase states. The server stores two new record
states and derives the rest; the old `approved`, `draft` and `discarded` keep their meaning.

- `PhaseState` gains `rejected` (the owner sent the draft back; its tasks stay Draft and are not handed out) and
  `queued` (accepted while an earlier phase still has open tasks; its tasks are Pending but not ready, as behind a
  barrier). A rejected phase counts as a draft for numbering, editing (`edit-draft`, `extend` with `"phase"`,
  `move-phase`, `discard-phase` by the lead), `stop` and `finish`; a queued one counts as accepted.
- Phase records gain `note` (shown under the phase header) and `rejection {comment, by, at, reworked_at?}`; both
  are skipped when empty. `Settings.auto_accept: bool` (skipped when false) is an owner setting: the lead's
  `settings` change of it is refused ("Only you change the team's auto_accept").
- `approve-phase` keeps its checks (F12: every earlier phase accepted; no errors naming its tasks) and receipt, and
  adds `"phase_state":"running"|"queued"` (the `phase_approved` event carries `state`). The phase runs at once when
  every earlier phase is finished, else it is `queued`. After every operation a queued phase whose earlier work is
  all finished becomes `approved` (event `phase_started {phase,title,tasks}`), and an accepted phase whose tasks all
  ended records `phase_done {phase,title}` once. Urgent phases and `phase_approval: auto` extends are unchanged.
- `hgs team reject-phase --json` `{"request_id","team","phase","comment"}` (owner, or the parent lead of a sub-team):
  a `draft` becomes `rejected`; the comment is required, at most 2,000 characters. Receipt
  `{"status":"phase_rejected","phase":N}`, event `phase_rejected {by,phase,title,comment}`. Approving it is refused
  ("Phase N was sent back to the lead; undo the send-back or wait for the rework").
- `hgs team undo-reject-phase --json` `{"request_id","team","phase"}` (same actors): only while `rejected` and not
  yet reworked; the phase is a `draft` again without the comment. Event `phase_reject_undone`.
- The lead's inbox shows a rejected phase as item `phase_rejected` (member `owner`, the comment, the command
  `rework-phase`). `hgs team rework-phase --json` `{"request_id","team","phase","note"?}` (lead or owner): the phase
  is a `draft` again with `note` = `Reworked after your comment: "<comment>"` (typographic quotes; the lead's own
  note follows after ` · `). Event `phase_reworked`.
- Auto-accept: while the team is Running or Paused and `auto_accept` is on, every operation (the `settings` change
  that turns it on included) accepts the drafts in display order as `approve-phase` would, by `Auto`, until the first
  one that cannot be accepted (an earlier draft, a plan error, a send-back). Event `phase_auto_accepted
  {phase,title,tasks,state}`. A rejected phase is never auto-accepted. In Planning nothing is auto-accepted; after
  Approve & Run the roadmap drafts are.
- Status: `phase_summary[].state` is `planned` (team in Planning), `draft`, `rejected`, `queued`, `running` or `done`
  (the G9 `status` stays as it was; a queued phase shows `approved`). A queued phase's `waiting.text` is
  `Starts after phase N`. `stuck_scope` lists the open tasks of running phases: only these can make the team Stuck;
  a draft, a send-back or a queued phase never does. `pending_phase` stays the first plain draft.
- `attention` (status and `team list`) gets `{"kind":"phase_draft","phase","since","summary":"Phase N · Title —
  draft waits for your Accept"}` for the first draft in order, whatever `remind_owner_minutes` says.
- The `team list` phase rows carry `note` and `rejection`; the list adds `stuck_scope` and `auto_accept`.
