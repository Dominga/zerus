# Team delivery, attempts and reports

Status: decided by the lead for phase 1 of team "Teams v2: delivery and
reports", 2026-10-26. Plan: `docs/superpowers/plans/2026-10-25-teams-next-phase.md`
items A1-A6, H5, F7, F8, F9. Builds on
`docs/superpowers/specs/2026-10-25-team-phases.md`; section 9 replaces its
"one draft phase at a time" rule.

Compatibility for every item: new fields have serde defaults and are skipped
when empty, so old team files load and old binaries ignore them; every
existing CLI call keeps its shape and meaning; new behaviour starts only on new
fields or new commands.

## 1. Why reports were captured wrongly (current code)

`dispatcher.rs` records `turn_ended_at` when the member session is `idle` and
its `reply_at` is newer than the hand-off, and after
`report_timeout_seconds` (120) takes the last reply as the report. Nothing
checks again at capture time, so a member that ended one turn with a progress
line ("Building and running the tests...") and went on working had that line
captured; a question or Background work did not stop it either.

## 2. Start states (A1)

`AttemptState` is unchanged (Delivering, Delivered, Unconfirmed, Refused,
Revoked, Reported, Rejected). `Attempt` gains:

```json
{"started_at": 1792940000.5, "start_warned": false}
```

- `started_at`: the first dispatcher tick after delivery that shows the member
  working on it: `activity` is not `idle`, or `turn_started` or `reply_at` is
  newer than `delivered_at`. Recorded once; event
  `{"kind":"started","task":7,"attempt":1,"member":"Backend"}`.
- Not started `settings.start_timeout_seconds` (new, default 180) after
  `delivered_at`: `start_warned` is set, event `start_late`, and the lead is
  told once when free ("#7 was delivered to Backend 3 min ago and has not
  started; its input may be waiting in the terminal"). Zerus never re-sends
  and never presses keys (CLAUDE.md: unknown delivery never authorizes
  replay).
- The shown attempt state, derived, in `hgs team status` as `"phase"` of the
  last attempt (no stored enum):
  `sent` (Delivering), `delivered` (Delivered, not started),
  `late` (delivered and `start_warned`), `working` (started, no open question),
  `asking` (an open question, section 4), `reported`, `no_report`, `stopped`,
  `held` (section 6), `rejected`, `unconfirmed`.

## 3. The capture gate (A2)

A turn end is recorded only when the session is `idle`, its phase is idle (no
question, input or approval), it has no Background work
(`process_work.count > 0` or activity `background`) and the attempt has no
open team question. Capture (taking the last reply) needs, at capture time,
all of:

1. the same conditions still hold;
2. the session's `reply_id` (or `reply_at`) is the one recorded with
   `turn_ended_at` (new field `turn_reply`), so no newer turn started;
3. `report_timeout_seconds` passed since the turn end.

If the member works again (not idle, or a newer reply), `turn_ended_at` and
`turn_reply` are cleared. A captured report is marked
`"report":{"captured":true,...}`; old captured reports keep their text prefix
and `captured` false.

## 4. Questions to the lead (A3)

From the member's session:

```json
{"request_id":"UUID","team":"ID","text":"Must I keep the old JSON field?"}
  | hgs team ask --json
-> {"status":"asked","question":"q3","task":7,"attempt":1}
```

- The caller is identified like `report`; the question attaches to its open
  attempt (`"task"` optional, required if it has several). Text at most 4000
  characters; at most 20 questions per attempt, at most 3 open at once.
- `Attempt.questions: [{"id":"q3","text":"...","at":..,"answer":"","answered_at":null,"by":""}]`.
- Events `question_asked`, `question_answered`. The lead is told when free, one
  message per question, with the reply command.
- The lead (or the owner) replies; the reply reaches the member as a message:

```json
{"request_id":"UUID","team":"ID","question":"q3","text":"Yes, keep it."}
  | hgs team reply --json
-> {"status":"answered","question":"q3"}
```

  `hgs team answer` stays for native questions and permission requests.
- While a question is open the attempt phase is `asking`, no capture happens
  (section 3) and the start warning does not fire.

## 5. Current attempt (A4)

```json
{} | hgs team current --json
-> {"team":"ID","task":7,"title":"...","attempt":"<attempt id>","n":1,
    "phase":"working","open_questions":["q3"],
    "report":{"request_id":"<fresh UUID>","team":"ID","task":7,
              "attempt":"<attempt id>","text":"..."}}
```

From a member session only; `"team"` optional. A member with no open attempt
gets an error naming its last task and its state ("#7 was rejected and is
held by the lead"). Read only.

## 6. Reject with a hold, and release (A5)

```json
{"request_id":"UUID","team":"ID","task":7,"reason":"...","hold":true}
  | hgs team reject --json
{"request_id":"UUID","team":"ID","task":7} | hgs team release --json
```

- `Task.hold: {"by":"Lead","at":..,"reason":"..."}` or absent. A held task is
  Pending but never ready, rebalanced or handed out. `release` (lead or
  owner) clears it; events `held`, `released`. `reject` without `hold` works
  as before.
- The lead may also hold a pending task (`hold` command with the same fields)
  so a busy member is not handed it; same release.
- Member queue (GUI and `status.members[].queue`): the member's open task
  first, then its tasks that were returned to it (have attempts, not finished:
  held or pending). Nothing else.

## 7. Replacing a captured report (A6)

While the task is Submitted and its report is `captured`, the member may send
`hgs team report` for that attempt; it is accepted (not refused as stale),
replaces the text, keeps the captured text in
`report.replaced: [{"text":..,"at":..}]` (at most 3), sets `captured` false and
the notice to Pending, so the lead hears again. Event `report_replaced`. A
report that was not captured is never replaced; a decided task refuses as
before.

## 7a. A captured report holds its member (A7)

A captured reply may be only an acknowledgement ("#123 is queued, I'll do it
after X"); handing that member the next task at once turned four tasks in a
row into acknowledgement-only reports (owner, 2026-10-29). So while a
member's task is Submitted, its last Reported attempt is the member's and that
report is `captured`, the member counts as busy: no hand-out, it is no
rebalance target, no work ahead, and it is not starving (C7, no
`plan_starved`). Its other ready tasks go to a free member with the same role
as for any busy member; a pinned one waits for it. It is free again once its
reviewer (the lead, a sub-lead or the owner) accepts, rejects, retries,
reassigns or cancels that task, or the member replaces the report with its own
`hgs team report` (section 7). An explicit report holds nothing, and the
lead's own tasks never hold the lead (C1).

The report's inbox item and its reminder say that the member gets no new work
until the report is decided or replaced (inbox `detail`). `hgs team status` and
`hgs team list` add `"captured_hold":{"task":N,"text":"No new work: the
captured report on #N awaits a decision"}` to that member; the field is absent
otherwise.

## 8. The hand-out text (H5)

Every hand-out ends with, filled in:

```
Questions for the lead: {"request_id":"UUID","team":"ID","text":"..."} | hgs team ask --json
Your task and attempt again: {} | hgs team current --json
Report once at the end: <the report command as today>
A progress line is not a report; a question is not a report.
```

The lead's brief names `hgs team reply`, `hold`/`release` and
`hgs guide team-lead`.

## 9. Existing sessions as members, members left running (F7, F8)

- `plan` and `add-member` accept `"session":{"name","run_id","conversation_id","host"?}`
  for a member. Zerus checks with `hgs ls` that the session exists and runs on
  that computer, and that it is not the lead or a non-removed member of another
  team in Planning, Running or Paused (error "dsh/x/y is a member of team
  <name>"). Its folder is its own `cwd` (stored as `directory`, folder
  `shared`); a member marked `worktree` with a session is refused unless the
  session's folder belongs to the team's repository (same git common dir).
  It is adopted, not launched; the first task is its first message.
- `status` and `team list` items of Done and Stopped teams gain
  `"left_running":[{"member":"backend","role":"Backend","session":{...}}]`,
  from the live listing. Zerus never stops them.

## 10. A roadmap of draft phases (F9)

- `plan` may carry `"phases":[{"n":2,"title":"...","barrier":true}]` and tasks
  with `"phase": 2..N`; `extend` is also allowed while Planning. Tasks of
  phases above 1 are Draft.
- Any number of draft phases, in order. A phase added while an earlier phase is
  a draft is a draft, whatever `phase_approval` says.
- `approve-phase N` is refused unless every phase below N is approved and all
  its tasks are decided ("Phase 2 opens after phase 1 ends"). The owner's rule
  holds for every team, old or new: approved phases run in order, so a team
  stored with one draft approves it only after the phase before it ended. The
  earlier "at most one draft" compatibility exception is gone (N6). `discard-phase N`
  discards N only. A draft that was never approved burns no number: discarding
  or moving drafts renumbers the drafts after it (phases and their Draft task
  ids, deps inside the drafts follow) and records phase_renumbered; approved
  phases and their tasks are never renumbered. A lead that edits draft tasks
  re-reads status after a phase_renumbered (a guard against stale ids is
  planned for the next phase).

## 11. Desktop

Icons, each with a tooltip naming the state, the time and the next step; drawn
in the skin's colours, 14 px, in this order of priority:

| Phase | Icon | Tooltip example |
|---|---|---|
| sent | outgoing arrow | Sending to Backend |
| delivered | inbox | Delivered 1 min ago, not started yet |
| late | inbox with amber dot | Delivered 4 min ago and not started: check its terminal |
| working | play triangle (pulsing while the session is busy) | Backend started 12 min ago |
| asking | speech bubble with ? | Backend asks: "Must I keep..." (Answer) |
| held | pause bars | Held by Lead: waiting for the owner |
| reported | document with check | Report from Backend, 3 min ago |
| no_report | document with cross | Turn ended without a report |
| stopped | stop square | Backend's session stopped |
| rejected | curved back arrow | Rejected: returned to Backend |

- Where: task cards (graph, board, layers) left of the status text; the member
  card next to "Working on #N"; the member queue; the task detail header.
- Member card: its queue under the current task (current first, then returned
  tasks with their icons), at most 5 rows plus "+N more".
- Questions: the task detail lists open questions with an Answer field and Send
  (sends `reply`); the lead's inbox later reuses it.
- Hold and Release buttons on a rejected or pending task; Reject offers
  "Return later (hold)".
- A replaced report shows "Replaced the captured reply" with the old text
  folded.
- Members left running of Done/Stopped teams are shown apart from active
  teams, "Left from <team>", with Stop (confirmation) and "Take into a new
  team"; never under the lead as active members.
- Several draft phases render as phases; Approve phase is enabled only for the
  next phase once the previous one ended, otherwise disabled with the reason
  as tooltip.

## 12. Limits and events summary

New events: `started`, `start_late`, `question_asked`, `question_answered`,
`held`, `released`, `report_replaced`, `member_adopted`. New settings:
`start_timeout_seconds` (default 180, 30..3600). Bounds: 20 questions per
attempt, 3 open, 4000 characters each; 3 replaced reports; queue 5 rows in
the GUI.
