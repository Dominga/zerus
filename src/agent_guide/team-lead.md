Lead a team (hgs guide team-lead)

Plan phase 1; extend {phase_title,tasks}. barrier waits for earlier phases;
pinned keeps assignee. edit-draft/move-phase/rework-phase edit phases.
A check's exit decides. Every task gets an estimate.
Task "output" paths guide rebalance. Iron rules add up.
Follow-up for a miss/bug: "origin":ID of its source task; same layer, no dep.
Phase checks default off: target_hours warns; requires blocks. Owner: approve_anyway.

Owner gates are cards: hgs guide team-propose. Never ask the owner to type a phrase.
A prompt left 10 min is denied and queued for the owner; do everything else.
The owner's answer arrives as a message.

Accepted phase not started: edit-draft/extend "phase"/move-phase are cards;
return-to-draft re-approves. pause-phase {phase, reason}; unpause yours by card.
Owner acceptance holds later phases until accept-phase (a card); extend reworks
a sent-back phase.

Pipelines: "code" (review/test), "docs"/"check"/"none"; default role/template.
Pass/return; decide at round 3; skip-stage/send-back override.

{"team":"ID","phase":N} | hgs team profile --json: top three losses, roles[],
tokens per accepted task; unknown, never zero. hgs guide team-tests.

Reject reserves its member for rework; unavailable: reassign. "fresh":true
tries a free role peer.

Auto drafts (U61): with Auto-accept, a phase stays a draft you edit until first hand-out.
If members are idle and tasks independent:
{"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.
Modes: standard (default), free (owner closes it); phases override it.

Members:
- A change or a question for the owner is a proposal, never only
  prose: team-propose.
- Routine: team-run; shapes: team-safe. A question is not a report; team reply
  answers it. "hold":true holds a reject; release gives it out.
  Captured reports hold replaceable members.
- Budgets: hgs team expect {"label":"x","minutes":15}; red once.
- Never install a tool; propose on the question card.
- Shared folder: team-broker. Sub-teams: team-tree.
- Codex/Windows: git-commit (read-only .git), powershell.exe (team-run).
- Permissions: allow task needs; outside/destructive: ask the owner.
- Cancel empty tasks first; all decided: extend/finish; owner stop is final.
