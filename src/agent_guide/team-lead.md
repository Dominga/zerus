Lead a team: phases, pipelines and members (hgs guide team-lead)

Plan phase 1; new work is a phase (extend: phase_title, tasks). barrier holds
tasks behind earlier phases; pinned keeps its member. A "review" draft
waits for you, "auto" starts once none is ahead: extend "phase":N, edit-draft,
move-phase, rework-phase. add-to-phase {phase, tasks, owner_word} on your
word, then confirm; a check's exit decides.

Pipelines (U36): per task "pipeline":"code" (review, test), "docs",
"check" (none) or "none", else the role's or template's default. A stage
verdict (hgs team report --json) passes or returns it with findings;
after 3 rounds you decide. skip-stage/send-back override; a test holds the
folder.

Profile (U38): before you report or finish a phase, read its losses:
{"team":"ID","phase":N} | hgs team profile --json. Name its top three losses
by time in the report; weigh lanes in the next plan with roles[] (utilization,
tokens per accepted task). Unknown tokens (another computer, a missing record)
are unknown, never zero.

Plan modes (U59): plan_mode standard (a release) or free (a stream) plus
plan_mode_reason; the owner picks, else standard. Free: extend adds to the
open phase, tasks run as added, an empty queue is idle, the owner closes it;
builds are tasks. Switch: propose {"kind":"switch-plan-mode","mode":"free",
"reason":"..."}; only the owner's Allow switches.

Members:
- U57: propose and wait for the owner's card in your chat, never a sentence:
  hgs guide team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not
  a report: answer with hgs team reply; reject with "hold":true, release gives
  a held task out; a captured report holds its member: decide it; a report may
  be replaced.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- A permission: allow what the task needs in the member's folder; ask the
  owner outside or destructive.
- A plan or phase needs the owner's sentence in your session (<30 min):
  {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Sub-lead: hgs team sub-lead {"member":"qalead","sub":true}; reports-to
  {member, parent} sets who it reviews.
- Cancel empty tasks first; when all are decided, extend or finish: hgs team
  finish --json. The owner may finish too; its stop is final.
Sub-teams: hgs guide team-tree.
