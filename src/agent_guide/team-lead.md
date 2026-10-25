Lead a running team: phases and finish (hgs guide team-lead)

The approved plan is phase 1. Later work (QA findings, fixes) goes in a new
phase at the end; earlier tasks never change. While the team is Planning,
Running or Paused, the lead or the owner appends one:
  {"request_id":"UUID","team":"ID","phase_title":"QA fixes","barrier":true,
   "tasks":[{"id":18,"title":"Fix the login crash","description":"...",
     "criteria":["..."],"assignee":"api","pinned":true,
     "deps":[{"task":12,"kind":"hard"}]}]} | hgs team extend --json
- ids continue after the highest used; deps may name any task. At most 50
  tasks per phase, 500 per team.
- barrier: no task of the phase starts before every task of the earlier
  phases is accepted, rejected or cancelled; "earlier" follows the phase
  order. Without it, a task starts once its deps allow.
- pinned: the task stays with its assignee (keeps its context); plan takes
  it too.
- approval "review": a draft waits for the owner (approve-phase,
  discard-phase) and opens only after the earlier phase ends; "auto" starts
  at once if no draft is ahead.
- Before the owner approves it, change a draft: extend with "phase": N adds
  tasks to draft N; edit-draft {phase, title?, barrier?, tasks_update?: [{id,
  title?, assignee?, deps?, pinned?}], tasks_remove?: [ids]} edits it;
  move-phase {phase, to, position} reorders drafts; the order decides which
  opens first. Numbers are never reused; the lead may discard a draft.

Members and their questions:
- A question (hgs team ask) waits for your reply; it is not a report:
    {"request_id":"UUID","team":"ID","question":"q3","text":"..."} |
    hgs team reply --json
- reject with "hold":true, or hold a pending task: Zerus keeps it from its
  member until release ({"request_id","team","task"}).
- A captured report may be replaced by the member's own.
- A missing skill: add-member {"member":{"id","role","agent"}}, reassign.

When every task is decided the team waits for you; Zerus tells you once.
Extend it, or finish it (summary at most 4000 characters) once nothing is
open:
  {"request_id":"UUID","team":"ID","summary":"What was built and checked"} |
    hgs team finish --json
Cancel tasks that have nothing left to do first. The owner may finish too;
the owner's stop is final.
