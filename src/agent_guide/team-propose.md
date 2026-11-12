Propose a change to the owner (hgs guide team-propose)

Owner gates (goal and plan approval, roster, wider rights, money, release,
destructive actions) are cards: wait for Allow or Reject. Never ask the
owner to type a phrase. Inside the approved goal you add, move, split and
edit tasks yourself, no card (the owner sees a digest, may revert).
Each kind sends its exact change:
  {"kind":"add-member","members":[...]}
  {"kind":"cancel-tasks","tasks":[ids]}
  {"kind":"discard-phase","phase":N}
  {"kind":"question","question":"...","options":["A","B"],"recommended":"A"}
  {"kind":"switch-setting","setting":"build_broker","value":true}
    (also base_branch "main", full_tests "allowed", max_members 8)
  {"kind":"relaunch","member":"ID"} (not started: relaunch it yourself;
  one card a member, withdrawn once it works); drain, remove-member the same
  {"kind":"rights","member":"ID","permissions_profile":"operate"}
  {"kind":"scale","role":"Tester","agent":"claude","count":2}
  {"kind":"approve"}; {"kind":"approve-phase","phase":N}
  accept-phase, unpause-phase, return-to-draft: {"kind":K,"phase":N}
  edit-draft, move-phase, phase-acceptance, extend: fields (before approval)
  {"kind":"settings","settings":{"max_members":8}}
  {"kind":"full-tests","release":"v1.2"}
  {"kind":"iron-rules","scope":"project","project":"ID","rules":[...],
   "expected_revision":N} (or "scope":"session")
  {"kind":"switch-plan-mode","mode":"free","phase":N} (no phase: the team)
  {"kind":"switch-memory-mode","mode":"team"}; swap-member ("session":"NAME"
  binds it), next-goal as before
  {"kind":"scratch-rule","folder":"ABS","agents":["claude"]}: no approval
with "reason" and a request_id, piped to hgs team propose --json. Its
fields may also go in "input":{...}.

The receipt has the id and hash; Allow applies that change once. A
Deny's reason reaches you; a typed owner word is not taken.
"for_tasks":[N] keeps those tasks back until granted.

A question to the owner goes this way, never only in your reply: the
owner's option or answer on the card reaches your inbox.

The owner answers on the card (or decide-proposal in a plain shell). Each
answer is a decision for the tasks it binds: add "spec", "files":["PATH"],
"binds":[N], "forbids":["TERM"]. The owner's word in your chat: hgs team
decide --json with "owner_word"; never message a decision.
