Lead a team: phases, pipelines, customization (hgs guide team-lead)

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

Customization (U45-U52), opt-in; the owner sets roles, settings, catalog:
- Task "models" (default any) and "role" with no assignee (never both or
  pinned; a member's models default to its launch model): Zerus takes a free
  member of the role whose models allow one, switches model and effort and
  waits; a refused or unconfirmed one leaves the task waiting, never on the
  wrong model.
- context_before_task none|compact|clear (role, then member; default none):
  Zerus compacts or clears and waits before each hand-out; clear needs
  self-contained task text.
- Command stages (expect_exit 0, timeout 30) have no member; a last land
  stage maps exits to accepted, returned or held; needs land_allowed.
  branch_per_task {template} and base_branch are owner settings (default one
  branch per member).
- scale {role, agent, count, model?, effort?, account?} adds members; drain
  {member} finishes it, pauses it and removes it; both on the owner's word.
- Owner's intake key: 5 tasks as one urgent phase; restart_crashed off:
  3 restarts/hour, then stopped.

Members: hgs guide team-run, team-safe. A question is not a report:
answer with hgs team reply; reject with "hold":true, release gives it out; a
report may be replaced; roster: owner_word or propose. Approve a plan or phase
only on the owner's sentence (<30 min): owner_word. Sub-lead: hgs team
sub-lead {"member":"qalead","sub":true}; reports-to {member, parent}. When all
are decided, extend or finish (hgs team finish); the owner's stop is final.
Sub-teams: hgs guide team-tree.
