Lead a team (hgs guide team-lead)

Plan phase 1; extend later work (phase_title, tasks). barrier holds later
phases; pinned keeps a member. edit-draft/move-phase/rework-phase edit them;
add-to-phase {phase, tasks, owner_word}, then confirm, adds on your word.
A check's exit decides. Every task gets an estimate.
Give tasks "output" paths so rebalance respects shared files.

Accepted phase not started: edit-draft/extend "phase"/move-phase need the
owner's word; return-to-draft re-approves. pause-phase {phase, reason};
unpause yours on the owner's word. Owner-acceptance holds later phases until
accept-phase (owner's word); extend reworks a sent-back phase.

Pipelines: a task "pipeline":"code" (review, test), "docs", "check"
or "none", else the role's or template's default. A stage verdict passes or
returns the task; after 3 rounds you decide; skip-stage/send-back override.

Profile: hgs team profile --json: top three losses, roles[];
tokens per accepted task; unknown, never zero.

Tests: hgs guide team-tests.

Reject reworks on the same member; "fresh":true tries a free peer of its role.
Rework reserves its member; unavailable members wait until reassign.

Auto drafts (U61): with Auto-accept, a phase stays a draft you edit until
first hand-out. When members are idle and tasks independent:
{"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.

Plan modes: standard (release) or free (extend adds to the open phase, owner
closes it); default standard.

Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe.
  Answer questions with hgs team reply. "hold":true holds a reject; release
  gives it out. A captured report holds its member; members can replace reports.
- Budgets: hgs team expect {label, minutes}; red says once.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- Permissions: allow the task's needs; otherwise ask the owner.
- A plan or phase needs the owner's sentence in your session (<30 min):
  {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Cancel empty tasks first; all decided: extend or team finish; owner stop is final.
Sub-teams: hgs guide team-tree.
