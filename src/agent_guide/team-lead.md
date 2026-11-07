Lead a team (hgs guide team-lead)
Plan phase 1; extend {phase_title,tasks}. barrier waits for earlier phases;
pinned keeps assignee. edit-draft/move-phase/rework-phase edit phases.
A check's exit decides. Every task gets an estimate.
Phase checks default off: target_hours warns; requires blocks. Owner: approve_anyway. See docs/reference.md.
Task "output" paths guide rebalance. Iron rules add up.
Owner gates are cards: propose (hgs guide team-propose); owner clicks Allow/Reject. Never ask the owner to type a phrase. Cards: approve, approve-phase,
add-to-phase, relaunch, scale, drain, remove/swap, phase changes, settings, rules.
Accepted phase not started: edit-draft/extend "phase"/move-phase are cards;
return-to-draft re-approves. pause-phase {phase, reason}; unpause yours by a
card. Owner-acceptance holds later phases until accept-phase (a card); extend
reworks a sent-back phase.
Pipelines: code (review/test), docs/check/none; role/template default.
Pass/return; decide at round 3; skip-stage/send-back override.
{"team":"ID","phase":N} | hgs team profile --json: top three losses, roles[],
tokens per accepted task; unknown, never zero.
hgs guide team-tests.
Reject reworks on its reserved member; unavailable: reassign. "fresh":true tries a free peer of its role.
Auto drafts (U61): with Auto-accept, a phase stays a draft you edit until first hand-out.
If members are idle and tasks independent:
{"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.
Modes: standard/release (default); free: extend open phase, owner closes it.
Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: hgs guide team-run; shapes: hgs guide team-safe. A question is not a report:
  hgs team reply answers it. "hold":true holds a reject; release gives it out.
  Captured reports hold members; replaceable.
- Budgets: hgs team expect {"label":"x","minutes":15}; red says once.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- Codex/Windows: git-commit (read-only .git); powershell.exe, not Store pwsh (team-run).
- Permissions: allow task needs; outside/destructive: ask the owner.
- Cancel empty tasks first; all decided: extend/finish; owner stop is final.
Sub-teams: hgs guide team-tree.
