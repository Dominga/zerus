Propose a change to the owner (hgs guide team-propose)

The owner gates the goal and plan approval, the roster, wider rights, money
(paid resources), release and destructive actions: each is a card;
wait for Allow or Reject. Never ask the owner to type a phrase or a sentence.
Inside the approved goal, adding, moving, splitting and editing tasks are
your own operations, never a card: the owner sees a digest and may revert.
Each kind sends its exact change:
  {"kind":"add-member","members":[...]} (one or several)
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
  edit-draft, move-phase, phase-acceptance, extend: fields (before approval)
  {"kind":"settings","settings":{"max_members":8}} (any owner setting)
  {"kind":"full-tests","release":"v1.2"} (one full test run)
  {"kind":"iron-rules","scope":"project","project":"ID","rules":[...],
   "expected_revision":N} (or "scope":"session" with the member's session)
  {"kind":"switch-plan-mode","mode":"free","phase":N} switches phase N
    only (free or standard); without "phase", the team's mode
  {"kind":"switch-memory-mode","mode":"team"}; swap-member, next-goal as before
with "reason" and a request_id, piped to hgs team propose --json. An
operation's fields may also go in "input":{...}.

The receipt carries the proposal id and its content hash. The owner's Allow
applies exactly that change once; a changed or stale card is refused and
nothing is applied twice. A Deny's reason reaches you; a typed owner word
is not taken. A change Allow cannot apply is refused at once.

A question to the owner goes this way, never only in your reply: the
owner's option or answer on the card reaches your inbox.

The owner answers from the desktop or a plain shell (decide-proposal with
"accept" true or false; never from an agent session).
