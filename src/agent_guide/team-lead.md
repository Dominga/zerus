Lead a running team: phases, checks and finish (hgs guide team-lead)

The approved plan is phase 1; new work goes in a new phase at the end:
  {"request_id":"UUID","team":"ID","phase_title":"QA fixes","barrier":true,
   "tasks":[{"id":18,"title":"Fix the crash","assignee":"api","pinned":true,
     "deps":[{"task":12}]}]} | hgs team extend --json
- ids continue after the highest used; 50 tasks per phase, 500 per team.
  barrier: no task of the phase starts before every earlier phase is
  finished; "earlier" follows the phase order. pinned keeps its assignee.
- "review": a draft waits for the owner (approve-phase, discard-phase);
  "auto" starts at once if no draft is ahead. Until then extend "phase":N,
  edit-draft {phase, title?, tasks_update?, tasks_remove?} or move-phase
  {phase, to, position}; discarding frees its numbers.
- Read validation.health before asking for approval: critical path,
  parallelism, fan-in, warnings. A check task
  {"kind":"check","check":{"command":"cargo test","expect_exit":0}} runs in
  the member's folder; Zerus decides it by exit code, without you.

Members:
- A question (hgs team ask) waits for your reply; it is not a report:
    {"request_id":"UUID","team":"ID","question":"q3","text":"..."} |
    hgs team reply --json
- reject with "hold":true, or hold a pending task; release gives it out. A
  captured report may be replaced by the member's own. Missing skill:
  add-member {"member":{"id","role","agent"}}, reassign.
- A member states a budget with hgs team expect
  {"label":"install","minutes":15}; Zerus says once when the job is red.
- Rate a permission by its command: allow what the task needs in the member's
  folder; ask the owner for anything outside, destructive or system-wide;
  deny wrong ones with a reason. MCP servers are services, not Background.
- Approve a plan or phase only on the owner's exact sentence in your own
  session, under 30 min old:
  {"owner_word":{"text":"plan approved"}} with approve or approve-phase;
  otherwise the owner approves in Zerus.

When all tasks are decided, Zerus tells you once. Extend the team, or finish
it (summary at most 4000 characters):
  {"request_id":"UUID","team":"ID","summary":"What was built and checked"} |
    hgs team finish --json
Cancel tasks with nothing left first. The owner may finish too; the owner's
stop is final.
