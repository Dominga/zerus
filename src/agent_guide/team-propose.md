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
  swap-member and next-goal as before
with "reason" and a request_id, piped to hgs team propose --json. An
operation's fields may also go in "input":{...}.

The receipt carries the proposal id and its content hash. The owner's Allow
applies exactly that change once; a changed or stale card is refused and
nothing is applied twice. A Deny's reason reaches you. A typed owner word
stays a fallback only. A change Allow could not apply is refused at once.

A question to the owner goes this way, never only in your reply: the owner
picks an option or writes an answer on the card, and it reaches your inbox.

The owner's own answer from a plain shell (never from an agent session):
  {"team":"ID","proposal":"p1","accept":true,"expected_hash":"...",
   "reason":"...","by":"human","request_id":"UUID"} |
  hgs team decide-proposal --json
"accept" is required; false rejects and its reason reaches you.
