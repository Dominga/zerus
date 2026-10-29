Lead a team: phases, pipelines and members (hgs guide team-lead)

The plan is phase 1; new work is a phase (extend: phase_title, tasks).
barrier holds its tasks behind earlier phases; pinned keeps its member. A
"review" draft waits for you, "auto" starts once none is ahead: extend
"phase":N, edit-draft, move-phase, rework-phase. A running phase takes work on
your word: add-to-phase {phase, tasks, owner_word}, then confirm. A check's
exit decides.

Pipelines (U36): per task "pipeline":"code" (review, test), "docs" (review),
"check" (none) or "none", else the role's or template's default. A stage
member's verdict (hgs team report --json) passes it on or returns it with
findings; after the rounds (3) you decide. Override with skip-stage or
send-back; a test holds the folder for the next task.

Customization (U45-U52): hgs guide team-custom.

Members:
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not a
  report: answer it with hgs team reply; reject with "hold":true, release gives
  a held task out; a report may be replaced; roster: owner_word or propose.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- A permission: allow what the task needs in the member's folder; ask the owner
  for anything outside or destructive.
- Approve a plan or phase only on the owner's sentence in your session (<30
  min): {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Sub-lead: hgs team sub-lead {"member":"qalead","sub":true}; reports-to
  {member, parent} sets who it reviews.
- Cancel empty tasks first; when all are decided, extend or finish: hgs team
  finish --json. The owner may finish too; the owner's stop is final.
Sub-teams: hgs guide team-tree.
