Lead a team: phases, pipelines and members (hgs guide team-lead)

Plan phase 1; new work is a phase (extend: phase_title, tasks). barrier holds
tasks behind earlier phases; pinned keeps its member. "review" drafts wait for
you; "auto" drafts start at their first hand-out. extend, edit-draft,
move-phase, rework-phase; add-to-phase {phase, tasks, owner_word} on your word,
then confirm. A check's exit decides.

Pipelines (U36): per task "pipeline":"code" (review, test), "docs", "check"
or "none", else the role's or template's default. A stage verdict passes or
returns the task; after 3 rounds you decide. skip-stage/send-back override;
a test holds the folder.

Profile (U38): read a phase's losses:
{"team":"ID","phase":N} | hgs team profile --json; name the top three losses by
time and weigh lanes with roles[] (utilization, tokens per accepted task).
Tokens unknown, never zero.

Auto drafts (U61): with Auto-accept, a drafted phase stays a draft you edit
(extend, edit-draft, move-task, cancel) until its first hand-out. Start it
ahead when members are idle and its tasks are independent:
{"team":"ID","phase":N} | hgs team start-ahead --json. Review drafts
need the owner; Work ahead is for review drafts.

Plan modes (U59): standard (a release) or free (a stream), with a reason;
the owner picks (default standard). Free: extend adds to the open phase.

Members:
- U57: propose and wait for the owner's card, never a sentence: hgs guide
  team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not
  a report: answer with hgs team reply; "hold":true rejects; release gives a
  held task out; a captured report holds its member; a report may be replaced.
- Budgets: hgs team expect {"label":"x","minutes":15}.
- A permission: allow what the task needs; ask the owner for outside or
  destructive ones. Never install a tool: Zerus offers it to the owner;
  propose it on the question card.
- A plan or phase needs the owner's sentence in your session (<30 min):
  {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Sub-lead: hgs team sub-lead {"member":"qalead","sub":true}; reports-to
  {member, parent} sets who it reviews.
- Cancel empty tasks first; when all are decided, extend or hgs team finish
  --json. The owner may finish too; its stop is final.
Sub-teams: hgs guide team-tree.
