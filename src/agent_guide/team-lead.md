Lead a running team: phases and finish (hgs guide team-lead)

The approved plan is phase 1. Work found later (QA findings, fixes, the next
wave) goes in a new phase at the end; earlier tasks never change. While the
team is Running or Paused, the lead or the owner appends one:
  {"request_id":"UUID","team":"ID","phase_title":"QA fixes","barrier":true,
   "tasks":[{"id":18,"title":"Fix the login crash","description":"...",
     "criteria":["..."],"assignee":"api","pinned":true,
     "deps":[{"task":12,"kind":"hard"}]}]} | hgs team extend --json
  -> {"status":"added","phase":2,"tasks":[18],"approval":"review"}
- ids continue after the highest id ever used (status shows it); deps may
  name any task or one of the same batch. At most 50 tasks per phase and
  500 per team; assignees are you or a member that was not removed.
- barrier: no task of the phase starts before every task of the earlier
  phases is accepted, rejected or cancelled. Without it, a task starts as
  soon as its own deps allow.
- pinned: the task stays with its assignee; Zerus never gives it to a free
  member of the same role (keeps that member's context). plan takes it too.
- approval "review": the phase waits as Draft until the owner approves or
  discards it in Zerus (approve-phase, discard-phase); add no other phase
  meanwhile. "auto": its tasks start at once.
- A member's task message carries the reports and folders of its deps.

Members and their questions:
- A question (hgs team ask) waits for your reply; it is not a report:
    {"request_id":"UUID","team":"ID","question":"q3","text":"..."} |
    hgs team reply --json
- reject with "hold":true, or hold a pending task: Zerus keeps it from its
  member until release ({"request_id","team","task"}).
- A report Zerus took from a member's last reply may be replaced by its own.
- A missing skill: add-member {"member":{"id","role","agent"}}, then
  reassign a task to it.

When every task is decided the team does not close: it waits for you, and
Zerus tells you once. Extend it, or finish it with a summary (at most 4000
characters) once nothing is open:
  {"request_id":"UUID","team":"ID","summary":"What was built and checked"} |
    hgs team finish --json
Cancel tasks that have nothing left to do first. The owner may finish too;
the owner's stop is final.
