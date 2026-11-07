Lead a team (hgs guide team-lead)

Plan phase 1; extend {phase_title,tasks}. barrier waits for earlier phases;
pinned keeps assignee. edit-draft/move-phase/rework-phase edit phases.
A check's exit decides. Every task gets an estimate.
Overlapping "output" paths run one at a time: chain them (hard dep).
Follow-up for a miss/bug: "origin":ID of its source, same layer, no dep.

Owner gates are cards (hgs guide team-propose). Never ask the owner to type a phrase.
A prompt left 10 min is denied and queued for the owner; do everything else.
The owner answers later by message.

Accepted phase not started: edit-draft/extend "phase"/move-phase are cards;
return-to-draft re-approves. pause-phase {phase, reason}; unpause yours by a
card. Owner-acceptance holds later phases until accept-phase (a card); extend
reworks a sent-back phase.

Pipelines: "code" (review/test), "docs"/"check"/"none"; default role/template.
Pass/return; decide at round 3; skip-stage/send-back override.

{"team":"ID","phase":N} | hgs team profile --json: top three losses,
tokens per accepted task; unknown, never zero.

hgs guide team-tests.

Reject reworks on its member (reserved; else reassign); "fresh":true tries
a free peer of its role.

Auto drafts (U61): with Auto-accept, a phase stays a draft you edit until first hand-out.
If members are idle and tasks independent:
{"team":"ID","phase":N} | hgs team start-ahead --json.
Review drafts need the owner; Work ahead is for review drafts.

Modes: standard (default); free: extend the free phase; owner closes it.

Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: hgs guide team-run, shapes: team-safe. A question is not a report:
  hgs team reply answers it. "hold":true holds a reject; release gives it out.
  No report: hgs team use-last-reply.
- Budgets: hgs team expect {"label":"x","minutes":15}.
- Never install a tool; propose it on the question card.
- Shared folder: hgs guide team-broker.
- Codex on Windows: git-commit (read-only .git), powershell.exe (team-run).
- Rights: a task needs a fitting member (hgs team rights).
- Compacted: hgs team lore plans. Waiting is always a park, never an
  instruction: hgs guide team-park
- Cancel empty tasks first; all decided: extend/finish; owner stop is final
Sub-teams: hgs guide team-tree.
