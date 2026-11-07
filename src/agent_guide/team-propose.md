Propose a change to the owner (hgs guide team-propose)

Every change only the owner may make is a card: propose it, then wait for the
owner's Allow or Reject. Never ask the owner to type a phrase or a sentence.
Each kind sends its exact change:
  {"kind":"add-member","members":[...]} (one or several)
  {"kind":"add-to-phase","phase":N,"tasks":[...]}
  {"kind":"cancel-tasks","tasks":[ids]}
  {"kind":"discard-phase","phase":N}
  {"kind":"question","question":"...","options":["A","B"],"recommended":"A"}
  {"kind":"switch-setting","setting":"build_broker","value":true}
    (also base_branch "main", full_tests "allowed", max_members 8)
  {"kind":"relaunch","member":"ID"}; drain and remove-member the same
  {"kind":"rights","member":"ID","permissions_profile":"operate"}
  {"kind":"scale","role":"Tester","agent":"claude","count":2}
  {"kind":"approve"}; {"kind":"approve-phase","phase":N}
  accept-phase, unpause-phase, return-to-draft: {"kind":K,"phase":N}
  edit-draft, move-phase, phase-acceptance, extend: the operation's fields
  {"kind":"settings","settings":{"max_members":8}} (any owner setting)
  {"kind":"full-tests","release":"v1.2"} (one full test run)
  {"kind":"iron-rules","scope":"project","project":"ID","rules":[...],
   "expected_revision":N} (or "scope":"session" with the member's session)
  {"kind":"switch-plan-mode","mode":"free","phase":N} switches phase N
    only (free or standard); without "phase", the team's mode
  {"kind":"switch-memory-mode","mode":"team"}; swap-member, next-goal as before
  {"kind":"scratch-rule","folder":"ABS","agents":["claude"]}: scratch teams
    you create there start without plan approval, the folder trusted
with "reason" and a request_id, piped to hgs team propose --json. An
operation's fields may also go in "input":{...}.

The receipt has the proposal id and content hash; Allow applies exactly
that change once, a changed card is refused. A Deny's reason reaches you.
Plan owner steps early, together: "for_tasks":[N] on a card keeps those
tasks back until it is granted; cards reach the owner as one batch when
present.

A question to the owner goes this way, never only in your reply: the
owner's option or answer on the card reaches your inbox.

The owner answers on the card (or hgs team decide-proposal --json, from a
plain shell, with "accept" and the card's "expected_hash").
