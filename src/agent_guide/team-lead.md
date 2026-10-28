Lead a running team: phases, checks and finish (hgs guide team-lead)

The approved plan is phase 1; other work is a new phase:
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
- Running phase: add-to-phase {phase, tasks, owner_word} gives a warning and
  pending; show it, then {phase, confirm: pending, owner_word} (later yes).
- Read validation.health. A check task {"kind":"check","check":
  {"command":"cargo test"}} runs in the member's folder; exit decides.

Members:
- Routine commands: hgs guide team-run; record a shape once: hgs guide team-safe.
- A question (hgs team ask) waits for your reply; it is not a report:
    {"request_id":"UUID","team":"ID","question":"q3","text":"..."} |
    hgs team reply --json
- reject with "hold":true; release gives a held task out. A captured report may
  be replaced. Roster: owner_word or propose.
- Budgets: hgs team expect {"label":"install","minutes":15}; red says so once.
- Rate a permission by its command: allow what the task needs in the member's
  folder; ask the owner for anything outside or destructive; deny a wrong one.
- Approve a plan or phase only on the owner's sentence in your session (<30
  min): {"owner_word":{"text":"plan approved"}} with approve or approve-phase;
  approval auto starts the plan alone.
- Sub-lead: hgs team sub-lead {"member":"qalead","sub":true} makes one; hgs
  team reports-to {"member":"qa","parent":"qalead"} sets who reviews; it
  decides its reports.
- Cancel empty tasks first. When all tasks are decided, extend or finish:
  {"request_id":"UUID","team":"ID","summary":"What was built and checked"} |
    hgs team finish --json
- The owner may finish too; the owner's stop is final.
Sub-teams: hgs guide team-tree.
