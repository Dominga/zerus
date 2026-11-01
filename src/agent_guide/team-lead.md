Lead a team (hgs guide team-lead)

Plan phase 1; extend {phase_title,tasks}. barrier holds tasks behind earlier phases;
pinned keeps a member. edit-draft/move-phase/rework-phase edit phases;
add-to-phase {phase, tasks, owner_word}, then confirm, adds on your word.
A check's exit decides. Every task gets an estimate.
Give tasks "output" paths so rebalance respects shared files.
Iron rules: project+team+member+session; owner only; lead needs verified owner_word.

Accepted phase not started: edit-draft/extend "phase"/move-phase need the
owner's word; return-to-draft re-approves. pause-phase {phase, reason};
unpause yours on the owner's word. Owner-acceptance holds later phases until
accept-phase (owner's word); extend reworks a sent-back phase.

Pipelines: "code" (review/test), "docs"/"check"/"none", default role/template.
Pass/return; decide after 3 rounds; skip-stage/send-back override.

{"team":"ID","phase":N} | hgs team profile --json: top three losses, roles[],
tokens per accepted task; unknown, never zero.

hgs guide team-tests.

Reject reworks on the same member; "fresh":true tries a free peer of its role.
Rework reserves its member; unavailable: wait until reassign.

Auto drafts (U61): Auto-accept stays a draft you edit until first hand-out.
If members are idle and tasks independent:
{"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.

Modes: standard (release, default); free (extend adds to open phase, owner closes it).

Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not a report:
  hgs team reply answers it. "hold":true holds a reject; release gives it out.
  Captured reports hold members; reports are replaceable.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- Permissions: allow what the task needs; outside or destructive: ask the owner.
- Plans/phases need the owner's sentence in your session (<30 min):
  {"owner_word":{"text":"plan approved"}} with approve/approve-phase.
- Cancel empty tasks first; all decided: extend/team finish; owner stop is final.
Sub-teams: hgs guide team-tree.
