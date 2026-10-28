Lead a running team: phases, pipelines and finish (hgs guide team-lead)

The plan is phase 1; new work is a new phase (extend: phase_title, tasks; ids
continue, 50 tasks/phase, 500/team). barrier holds its tasks until every
earlier phase is finished; pinned keeps its member. A "review" draft waits for
you; "auto" starts once no draft is ahead; edit it with extend "phase":N,
edit-draft or move-phase, and rework-phase {phase, note?} returns a sent-back
phase. A running phase takes work on your word: add-to-phase {phase, tasks,
owner_word} warns, then {phase, confirm: pending, owner_word}.
A check task runs its command in the member's folder; exit decides.

Pipelines (U36): a task passes review and test stages before you.
- Per task "pipeline":"code" (review, test), "docs" (review), "check" (none)
  or "none" in plan, extend or edit-draft; else the role's or template's
  default; the task keeps the preset it started with.
- A stage member reports a verdict with hgs team report --json: pass moves on,
  return sends the findings straight to the implementer; after the preset's
  rounds (3) you decide with every round's findings. Override with hgs team
  skip-stage {task, reason} or send-back {task, reason}; accept and reject stay
  yours after the last stage.
- Review reads the attempt's change; test runs only the changed files' suites
  and the criteria's shots; neither edits the task's files; a test holds the
  folder, so the implementer's next task waits.

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
