Plan a team with the owner before it exists (hgs guide team-draft)

A draft is a team in the draft status: nothing runs until the owner creates it.
The owner starts it (New Team > Start planning, or hgs team draft --json with
"action":"start"); its lead session works in the team folder, and you are that
lead when the owner names you.

Read the draft before every change, so the revision is the current one:
  {"action":"get","team":"ID"} | hgs team draft --json
The answer carries draft (revision, unseen, owner_edited), revision, unseen and
preflight - the plan errors the owner would hit on create.
Change the draft (lead or owner), carrying the revision you read:
  {"action":"update","request_id":"UUID","team":"ID","revision":N,
   "set":{"name":"...","goal":"...","addendum":"...","settings":{...},
          "members":[...],"tasks":[...]}} | hgs team draft --json
A field the owner edited after you last read the draft is refused ("Owner
changed members"): read the draft again. The owner's update marks the field
owner_edited and drops it from unseen; your get answers the marks it cleared
in owner_edited, and after that read you may set the field. Your own edits
wait in unseen until the owner calls seen.

The owner also runs the draft: create (status planning, or running with
"approve":true; a wider permissions profile needs "confirm_widening":true),
discard (with "stop_lead":true it stops the lead's session; the reason is
kept), and seen (clears unseen). Only the owner creates, discards or approves;
do not create the team yourself.

Draft the plan as tasks with acceptance criteria and dependencies, as a team
lead does (hgs guide team-lead): the owner approves the phases in order. Keep
the role prompts and settings; propose members the goal needs, and say why in
your reply to the owner.
