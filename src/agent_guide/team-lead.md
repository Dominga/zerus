Lead a team (hgs guide team-lead)

Plan phase 1; extend {phase_title,tasks}. barrier waits for earlier phases;
pinned keeps assignee. edit-draft/move-phase/rework-phase edit phases.
A check's exit decides. Every task gets an estimate.
Overlapping "output" paths run one at a time: chain them (hard dep).
Follow-up for a miss/bug: "origin":ID of its source, same layer, no dep.

Owner gates are cards (hgs guide team-propose). Never ask the owner to type a phrase.
A prompt left 10 min is denied and queued for the owner; do everything else.
The owner answers later by message.

In the approved goal you plan, no card: add-to-phase, extend "phase",
edit-draft, move-phase, move-task, cancel (split: add + cancel); the owner
sees a digest and may revert. Accepted phase not started:
return-to-draft re-approves. pause-phase {phase, reason}; unpause yours by a card.
Owner-acceptance holds later phases until accept-phase (a card).

Pipelines: code (review/test), docs, check, none. Pass/return; decide at
round 3; skip-stage/send-back override.

{"team":"ID","phase":N} | hgs team profile --json: top three losses,
tokens per accepted task; unknown, never zero.
hgs guide team-tests.
Reject reworks on its member; "fresh":true: a free peer of its role.

Phases: an approved phase runs alongside earlier ones; a task waits for
deps, a barrier, an owner gate, files an open task writes.
Auto drafts (U61): Auto-accept approves a clean draft.
Review drafts need the owner; no draft runs before approval.

Modes: standard or free (the owner closes a free phase).

Members:
- A change or a question for the owner is a proposal, never only
  prose: hgs guide team-propose.
- Routine: team-run, team-safe. hgs team reply answers a question;
  "hold":true holds a reject, release gives it out. No report: use-last-reply.
- Budgets: hgs team expect {"label":"x","minutes":15}.
- Never install a tool; propose it (question card).
- Shared folder: team-broker.
- Codex on Windows: git-commit (read-only .git), powershell.exe (team-run).
- Rights: a fitting member (hgs team rights).
- Compacted: hgs team lore plans. Waits are parks: team-park.
- A message naming a task its member lacks is refused.
- Owner decisions: hgs team decide, never a message.
- Cancel empty tasks first; all decided: extend/finish; owner stop is final
Sub-teams: hgs guide team-tree
