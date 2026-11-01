Lead a team: phases, pipelines and members (hgs guide team-lead)

Plan phase 1; new work is a phase (extend: phase_title, tasks). barrier holds
tasks behind earlier phases; pinned keeps its member. extend, edit-draft,
move-phase, rework-phase; add-to-phase {phase, tasks, owner_word} on your word,
then confirm. A check's exit decides. Every task gets an estimate.

Accepted phase not started: edit-draft/extend "phase"/move-phase only on the
owner's word; return-to-draft re-approves. pause-phase {phase, reason};
unpause yours on the owner's word. An owner-acceptance phase holds
later ones until accept-phase (owner's word); rework one sent back by extend.

Pipelines: a task "pipeline":"code" (review, test), "docs", "check"
or "none", else the role's or template's default. A stage verdict passes or
returns the task; after 3 rounds you decide; skip-stage/send-back override.

Profile: {"team":"ID","phase":N} | hgs team profile --json: top three losses,
roles[]; tokens per accepted task, unknown, never zero.

Tests: hgs guide team-tests.

Auto drafts (U61): with Auto-accept, a drafted phase stays a draft you edit
until its first hand-out. Start it ahead when members are idle and its tasks
are independent: {"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.

Plan modes: standard (a release) or free (a stream: extend adds to the
open phase, the owner closes it); default standard.

Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not
  a report: answer with hgs team reply; "hold":true rejects; release gives a
  held task out; a captured report holds its member; a report may be replaced.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- A permission: allow what the task needs; outside or destructive: ask the owner.
- A plan or phase needs the owner's sentence in your session (<30 min):
  {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Cancel empty tasks first; when all are decided, extend or team finish;
  the owner's stop is final.
Sub-teams: hgs guide team-tree.
