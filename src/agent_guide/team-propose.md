Propose a change to the owner (hgs guide team-propose)

Propose, then wait for the owner's card in your chat - never ask for a
sentence. Each kind sends its exact change:
  {"kind":"add-member","members":[...]} (one or several)
  {"kind":"add-to-phase","phase":N,"tasks":[...]}
  {"kind":"cancel-tasks","tasks":[ids]}
  {"kind":"discard-phase","phase":N}
  {"kind":"question","question":"...","options":["A","B"],"recommended":"A"}
  {"kind":"switch-setting","setting":"build_broker","value":true}
  remove-member, swap-member, next-goal and switch-plan-mode as before
with "reason" and a request_id, piped to hgs team propose --json.

The receipt carries the proposal id and its content hash. The owner's Allow
applies exactly that change once; a changed or stale card is refused and
nothing is applied twice. A Deny's reason reaches you. The owner's word by
text stays a CLI fallback. A change Allow could not apply is refused at once.

A question to the owner goes this way, never only in your reply: the owner
picks an option or writes an answer on the card, and it reaches your inbox.

The owner's own answer from a plain shell (never from an agent session):
  {"team":"ID","proposal":"p1","accept":true,"expected_hash":"...",
   "reason":"...","by":"human","request_id":"UUID"} |
  hgs team decide-proposal --json
"accept" is required; false rejects and its reason reaches you.
