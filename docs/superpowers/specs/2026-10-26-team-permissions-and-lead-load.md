# Team permissions and lead load

Status: decided by the lead for phase 32 "Permissions and lead load" of team
"Teams v2", 2026-10-26. Plan: `docs/superpowers/plans/2026-10-25-teams-next-phase.md`
items B1-B6, C1-C7, C12, G3, G7, F12, F13, U3 and the parts of E they need.
Builds on `2026-10-25-team-phases.md` and `2026-10-26-team-delivery.md`.

Owners: backend B1, B2, B4, clones (task 288); B3, B5, B6, U3 (289).
backend2 C1-C4, C6 (290); C5, C7, C12, G7, F12, F13 (291). frontend
permissions and G3 (292); lead load, services, F12, F13, U3 (293).

Compatibility for every item: new fields have serde defaults and are skipped
when empty; old team files load; every existing CLI call keeps its shape and
meaning; new behaviour starts only on new fields, commands or settings.

## 1. Permission requests (B1, B2)

Today the lead's permission message shows the agent's own justification. It
gains the exact command and, after the answer, who answered.

- The dispatcher reads the command from the member's pending question:
  Claude and Codex from the tool input of the pending tool call
  (`tool_input.command` or the patch path list); DeepSeek from the
  `processes` item whose `call_id` equals the question's `tool_call_id`,
  else the newest item in state `starting` of that run. Unmatched:
  `"command": null, "command_source": "unknown"`; the message says
  "command not shown by the agent - read it with hgs inspect".
  At most 4000 characters, cut with a note.
- The lead's message and `status.members[].question` carry
  `{"question_id","tool","command","command_source":"tool_input|process|unknown","reason","asked_at"}`.
- After the answer, the attempt (or the member, outside a task) keeps
  `{"question_id","decision":"allow|deny","by":"lead|owner|rule|timeout|unknown","at","reason"}`,
  the last 20. `by` is `lead`/`owner` when the answer came through
  `hgs team answer` (the actor), `owner` when the desktop answered it,
  `unknown` when the question vanished without our answer. Event
  `permission_answered`.
- B2: `hgs team answer` takes `"text"` with a deny. The native answer still
  selects the deny option; Zerus then sends the text to the member as one
  message: `"Lead: denied <tool>: <text>"`. A deny without text works as
  before. An allow ignores text.

```json
{"request_id":"UUID","team":"ID","member":"frontend","question_id":"Q",
 "expected_question_hash":"H","answers":[{"question_id":"decision",
 "selected_option_ids":["deny"],"text":""}],
 "text":"Stop-Process kills every ninja on the machine; run build.cmd only"}
```

## 2. Commit through Zerus and clones (B4, B6)

- `hgs team commit --json {request_id, team, task?, attempt?, message, paths?}`
  from a member session (identity as `report`, attempt id accepted from
  another computer). Zerus runs git itself, outside the sandbox, in the
  member's folder only, refuses if the folder's current branch is not the
  member's team branch (`team/<id8>/<member>`) or if the folder is the team's
  shared folder. `paths` given: stages exactly them (inside the folder);
  absent: `git add -A` within the folder. Never pushes, never amends, never
  changes branches. The message gets the member's `Fork: ` prefix rule from
  the protocol, not enforced. Result `{"status":"committed","sha","files"}`;
  the sha is recorded on the attempt (`attempt.commits`, last 20); event
  `committed`. Nothing to commit: `{"status":"clean"}`.
- Clones: a new member folder kind `clone`. Zerus creates
  `<parent>/<name>-team-<id8>-<member>` with `git clone --local --no-hardlinks`
  from the team folder, then `git switch -c team/<id8>/<member> <team HEAD>`.
  Its `.git` is inside the folder, so commit, merge and fetch work in a
  sandbox. The lead brings work in with `git fetch <member folder>
  team/<id8>/<member>` and merges; `hgs team status` names the fetch command.
  Default: `clone` for agents whose sandbox cannot write outside the folder
  (dsh today; per agent a capability flag), `worktree` otherwise; the plan may
  say either explicitly. Old teams keep their worktrees.

## 3. Preflight, folder policy, profiles (B3, B5, B6)

- B6: `settings.folder_policy` gains `auto` (new default for new teams;
  stored teams keep their value) next to `shared`, `worktree`, `lead`. `auto`
  lets the lead choose `shared`, `worktree` or `clone` per member. A project
  setting `teams.worktrees: allowed|forbidden` (project catalog) wins over
  templates and the desktop; `forbidden` refuses `worktree` and `clone`.
- B3: `plan`, `approve` and `add-member` run a preflight and put findings in
  `validation.warnings`: a sandboxed agent in a `worktree` (git metadata
  outside its folder: commits will ask), a member folder that does not exist
  or is not the team's repository, a folder policy the member contradicts.
  Contradictions are errors (F1 guard), sandbox notes are warnings.
- B5: a role in the catalog template gains `permissions_profile`:
  `read`, `write` (own folder), `build` (write + build and test tools),
  `commit` (build + hgs team commit), `network`. Zerus maps a profile to the
  agent's own permission mode when it launches the member (Claude
  `--permission-mode`/allowed tools, Codex sandbox and approval, dsh account
  mode) where the agent supports it; otherwise the profile is advisory and
  status says so. A plan or add-member that widens a role's profile beyond
  the template stores `"profile_widening":{"member","from","to"}` and the plan
  cannot be approved until the owner confirms it in the desktop (an approve
  without `"confirm_widening": true` from the owner is refused).
- B5, the lead's own session: when Zerus starts a lead (or adopts a session as
  lead) it allows `hgs team answer` and `hgs team reply` for that session:
  Claude through `--allowedTools "Bash(hgs team answer:*)" "Bash(hgs team reply:*)"`
  at launch; an adopted running session gets the same through its
  session-only settings where the agent has them, else status shows
  `"lead_answer_rule":"missing"` with the line to add. Owner: confirmed.

## 4. Resume guard (U3)

Seen 2026-10-26: a Claude transcript held nothing of a 4-hour run that Zerus
recorded event by event; Resume restored the older conversation silently.

- Before `hgs resume` (and the desktop's Resume and send) of a Claude session,
  Zerus reads the transcript's last 256 KiB, finds the newest user or
  assistant entry time, and compares it with the newest `UserPromptSubmit` or
  `Stop` event of that session's conversation (indexed by session name in
  `events.sqlite3`).
- Behind by more than 120 s: the resume is refused with
  `{"error":"transcript_behind","transcript_at","events_at","missing":N}`
  unless `--accept-stale` or `--with-lost-dialogue`.
- `--with-lost-dialogue` writes the missing prompts and replies (from the
  events after the transcript's end, at most 400 entries and 400 KiB, oldest
  cut first with a note) to `<state>/sessions/recovered/<conversation>-<time>.md`,
  resumes, and sends as the first message: "Your transcript stops at <time>;
  Zerus recorded the dialogue after it: <file>. Read it before you go on."
- Desktop: the Resume dialog shows the warning with the times and offers
  "Resume with the lost dialogue" (default), "Resume as it is", Cancel.
- Codex, Kimi, dsh: the same check where the transcript format allows it
  (Codex rollout files); otherwise no guard and no change.

## 5. The lead's load (C1-C4, C6)

- C1: a ready task assigned to the lead is not sent as a message. It goes to
  the inbox (C2); the lead reports it with the usual `report` and accepts it
  in one step: `report` with `"accept": true` from the lead on its own task
  records the report and the acceptance together (event `accepted`, by Lead).
- C2: `{"team":"ID"} | hgs team inbox --json` ->
  `{"items":[{"kind":"report|question|permission|lead_task|stalled|budget",
  "task","member","since","summary","command"}]}`, oldest first, at most 100.
  Notifier: when items are new and the lead is free, one message lists them
  (at most one per `settings.inbox_interval_seconds`, default 120), instead
  of one message per item. Existing per-item messages become that one message.
- C3: every open task in status gains `"waits":{"for":"deps|member|lead|owner|hold",
  "on":[ids or names],"unblock":"accept #12" | "answer q3" | "approve phase 33"}`.
- C4: a report undecided for `settings.remind_lead_minutes` (default 15)
  reminds the lead once; after `remind_owner_minutes` (default 60) it shows
  on the owner's attention list in the desktop. Both 0 = off.
- C6: among a member's ready tasks the dispatcher hands out first the one with
  the most transitive dependants, then the lowest id.

## 6. Check tasks, plan health, budgets, services (C5, C7, C12, G7)

- C5: a task may be `"kind":"check","check":{"command":"cargo test --locked --bin hgs teams","expect_exit":0,"timeout_minutes":30}`.
  The hand-out asks the assignee to run exactly that command in its folder and
  report `{"exit":N,"tail":"last 40 lines"}` through `hgs team report` with
  `"check"` set; Zerus accepts at the expected exit code and rejects otherwise,
  without the lead. The plan approval view lists every check command.
- C7: `plan`, `extend`, `edit-draft` return `validation.health`:
  `{"tasks","critical_path","parallelism":total/critical_path,
  "per_member":{"backend":{"tasks","on_critical_path"}},"widest_ready",
  "fan_in":[{"task","waits_for":[ids],"kind":"review|qa|accept"}],
  "warnings":["..."]}`. Warnings: parallelism below 1.5 with 3+ members;
  a hard dependency on a review/QA/acceptance task from a task that does not
  name what it needs (C10 note); a member with more than half the critical
  path. While running, every 10 minutes: members idle with no ready task while
  open tasks exist for 20 minutes -> `"starved"` warning with the blocking
  chain, told to the lead once per episode. The desktop shows the block.
- C12: `hgs team expect --json {request_id, team, label, minutes, red_minutes?}`
  from a member: ties a budget to its newest live Background job (or the next
  one within 60 s). States `ok`, `amber` (past `minutes`), `red` (past
  `red_minutes`, default 2x). At red the lead is told once with the job's
  command and last output lines. Jobs without a budget get red at
  `settings.background_red_minutes` (default 30). Budgets end with the job and
  never stop anything.
- G7: a process is a session service, not Background work, when it is an MCP
  server the agent started from its MCP config: its command line matches a
  configured MCP server of that agent (Claude `.mcp.json`/settings `mcpServers`,
  Codex `config.toml` `[mcp_servers]`), or it is a child of such a server.
  Services show as an icon with a tooltip (names, since when, outliving the
  session) and are excluded from Background status, the capture gate (A2), the
  blocked reminder and C12. Fixtures per agent. This changes the Background
  contract of CLAUDE.md on the owner's direction: desktop, Android fixtures and
  docs change together.

## 7. Approval wording and the owner's word (F12, F13)

- F12: a draft whose predecessor is not ready says "Waiting for the previous
  phase to be ready"; the tooltip names the phase and its open tasks. The
  refusal text of `approve-phase` stays specific.
- F13: `approve` and `approve-phase` from the lead accept
  `"owner_word":{"text":"<the owner's exact sentence>"}`. Zerus finds a
  `UserPromptSubmit` event of the lead's own session, typed by the owner (not
  starting with "[Zerus team", not a task notification, not an attachment-only
  message), at most 30 minutes old, whose text contains `owner_word.text` and a
  key phrase: "план принят", "план на следующую фазу принят", "plan approved",
  "next phase plan approved" (case-insensitive). Otherwise refused with the
  reason. Event `phase_approved`/`approved` with `"by":"Lead","owner_word"`;
  the desktop shows "Approved by Lead on your word" and the quote. Ordering
  (F12) still applies.

## 8. Graph order (G3)

Inside one layer of the graph (and inside a phase band), tasks with
dependants come first, ordered as today; dead-end tasks (no task depends on
them) go to the bottom of the column.

## 9. The built-in lead protocol (E)

`src/agent_guide/team-lead.md` (and a new page `team-permissions` if the 2400
byte cap needs it) gains: rate a permission request by its command (inside the
member folder and reversible: allow; outside, destructive or system: ask the
owner; wrong: deny with a reason and a better way); read the plan health
before asking for approval; prefer clones for sandboxed agents; never approve
on the owner's behalf without F13.

## 10. Events and settings summary

Events: `permission_answered`, `committed`, `held_for_widening`,
`inbox_told`, `reminded`, `check_passed`, `check_failed`, `plan_starved`,
`budget_amber`, `budget_red`. Settings (all optional, defaults above):
`inbox_interval_seconds`, `remind_lead_minutes`, `remind_owner_minutes`,
`background_red_minutes`. Status: `members[].question`,
`members[].services`, `tasks[].waits`, `attempts[].permissions`,
`attempts[].commits`, `health`.

## 11. The team's two bars (G8, added on the owner's request)

Owner: frontend, first thing in task 292. The tasks bar counts only the phase
in work (approved, with open tasks); with none in work, the first draft after
the last finished phase; never several phases. The phases bar is finished /
all non-discarded phases (phase 1, approved and drafts); finished = approved
with every task decided. The same numbers in the session row, the Teams panel
and the workspace header; the tooltip names the phase ("Phase 32 Permissions
and lead load: 1/11"). Team 528aebfe after phase 31 reads 2/5 phases.
