Lead a team (hgs guide team-lead)

Plan phase 1; extend {phase_title,tasks}. barrier holds tasks behind earlier phases;
pinned keeps assignee. edit-draft/move-phase/rework-phase edit phases.
A check's exit decides. Every task gets an estimate.
Phase checks are separate owner opt-ins, both off by default:
settings.phase_checks {critical_path_target, unresolved_prerequisites}.
A phase's target_hours is optional (extend/plan phases/edit-draft; null clears).
The card warns if its estimate-weighted path exceeds the target; never blocks.
Task requires: [{task:N}|{commit:"SHA"}|{branch:"name"}], at most 32, no files.
Same-phase: a distinct existing task; earlier-phase: accepted. Git references
must be ancestors of base_branch (default: team folder HEAD). Missing/later/self
or unknown refs, Git failure/timeout: unresolved, shown on the card. No implicit deps.
The prerequisite check refuses approve-phase; the owner can click Approve anyway
(approve_anyway:true), also on an approve-phase proposal card. Never preselect
the override in a proposal. Missing estimates show a lower bound and count.
Give tasks "output" paths; rebalance respects them. Iron rules add up.

Owner gates are cards: propose (hgs guide team-propose); the owner clicks Allow
or Reject. Never ask the owner to type a phrase. Cards: approve, approve-phase,
add-to-phase, relaunch, scale, drain, remove/swap, phase changes, settings, rules.

Accepted phase not started: edit-draft/extend "phase"/move-phase are cards;
return-to-draft re-approves. pause-phase {phase, reason}; unpause yours by a
card. Owner-acceptance holds later phases until accept-phase (a card); extend
reworks a sent-back phase.

Pipelines: "code" (review/test), "docs"/"check"/"none"; default role/template.
Pass/return; decide at round 3; skip-stage/send-back override.

{"team":"ID","phase":N} | hgs team profile --json: top three losses, roles[],
tokens per accepted task; unknown, never zero.

hgs guide team-tests.

Reject reworks on its member (reserved; unavailable: reassign); "fresh":true
tries a free peer of its role.

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
- Codex on Windows: read-only .git, so it commits via git-commit; no Store
  pwsh, so powershell.exe (team-run).
- Permissions: allow what the task needs; outside or destructive: ask the owner.
- Cancel empty tasks first; all decided: extend/finish; owner stop is final.
Sub-teams: hgs guide team-tree.
