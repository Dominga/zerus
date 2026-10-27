Lead a running team: phases, checks and finish (hgs guide team-lead)

The approved plan is phase 1; new work goes in a new phase:
  {"request_id":"UUID","team":"ID","phase_title":"QA fixes","barrier":true,
   "tasks":[{"id":18,"title":"Fix the crash","assignee":"api","pinned":true,
     "deps":[{"task":12}]}]} | hgs team extend --json
- ids continue after the highest used; 50 tasks/phase, 500/team. barrier: no
  task starts before every earlier phase is finished. pinned keeps its assignee.
- "review": a draft waits for the owner (approve-phase, discard-phase);
  "auto" starts at once if no draft is ahead. Until then extend "phase":N,
  edit-draft {phase, title?, tasks_update?, tasks_remove?} or move-phase
  {phase, to, position}. Sent back: edit it, then rework-phase
  {phase, note?} returns it as a draft.
- Read validation.health before approval. A check task {"kind":"check",
  "check":{"command":"cargo test","expect_exit":0}} runs in the member's
  folder; Zerus decides it by exit code.

Members:
- Routine commands run outside a member's sandbox: hgs team run (hgs guide
  team-run). Anything else asks you.
- A question (hgs team ask) waits for your reply; it is not a report:
    {"request_id":"UUID","team":"ID","question":"q3","text":"..."} |
    hgs team reply --json
- reject with "hold":true; release gives a held task out. A captured report may
  be replaced. Roster: owner_word or propose.
- A member states a budget with hgs team expect
  {"label":"install","minutes":15}; Zerus says once when the job is red.
- Rate a permission by its command: allow what the task needs in the member's
  folder; ask the owner for anything outside, destructive or system-wide;
  deny wrong ones with a reason. MCP servers are services, not Background.
- With approval auto the plan starts without the owner. Otherwise approve a
  plan or phase only on the owner's exact sentence in your own session, under
  30 min old: {"owner_word":{"text":"plan approved"}} with approve or
  approve-phase; else the owner approves in Zerus.

When all tasks are decided, extend the team or finish it (summary in 4000
characters):
  {"request_id":"UUID","team":"ID","summary":"What was built and checked"} |
    hgs team finish --json
Cancel empty tasks first. The owner may finish too; the owner's stop is final.
Sub-teams: hgs guide team-tree.
