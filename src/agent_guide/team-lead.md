Lead a team (hgs guide team-lead)

Plan phase 1; extend (phase_title, tasks) adds work. barrier holds tasks behind earlier phases;
pinned keeps a member. edit-draft/move-phase/rework-phase edit phases;
add-to-phase {phase, tasks, owner_word}, then confirm, adds on your word.
A check's exit decides. Every task gets an estimate.
Give tasks "output" paths so rebalance respects shared files.

Accepted phase not started: edit-draft/extend "phase"/move-phase need the
owner's word; return-to-draft re-approves. pause-phase {phase, reason};
unpause yours on the owner's word. Owner-acceptance holds later phases until
accept-phase (owner's word); extend reworks a sent-back phase.

Pipelines: "pipeline":"code" (review, test), "docs", "check", "none", else role/template default.
A verdict passes or returns; after 3 rounds you decide; skip-stage/send-back override.

Profile: {"team":"ID","phase":N} | hgs team profile --json: top three losses, roles[];
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
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not a report:
  hgs team reply answers it. "hold":true holds a reject; release gives it out.
  Captured reports hold members; members can replace reports.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- Permissions: allow what the task needs; outside or destructive: ask the owner.
- Plans/phases need the owner's sentence (<30 min):
  {"owner_word":{"text":"plan approved"}}.
- Cancel empty tasks first; all decided: extend or team finish; owner stop is final.
Sub-teams: hgs guide team-tree.
