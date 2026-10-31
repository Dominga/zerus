Lead a sub-team and pass work up (hgs guide team-tree)

A team may delegate a task to a sub-team. The parent task's assignee becomes
the sub-team's lead; the sub-team has its own members, plan, phases and
reports.

Create one from the assignee's session (or the parent lead, or you):
  {"request_id":"UUID","team":"PARENT","parent":{"team":"PARENT","task":18},
   "name":"Checkout migration","goal":"..."} | hgs team create --json
The parent task then shows its sub-team and rolled-up done/total counts.

Reports: finishing the sub-team files its summary as the sub-lead's report on
the parent task; the parent lead accepts or rejects it. A rejection reopens the
sub-team with a Rework draft that names the reason.

Questions: a member asks its own lead. The sub-lead may answer, or pass it one
level up to the parent lead:
  {"request_id":"UUID","team":"SUBT","question":"q3"} | hgs team escalate --json
The parent lead replies normally; the answer returns to the sub-team question.
A permission request is escalated the same way, with its command, source and
reason; the parent lead answers the native card by its question id and hash.

Tree actions: pausing or stopping a parent reaches its sub-teams; archiving
needs every child stopped or done. Cancelling or reassigning a task that holds
a sub-team needs "subteam":"stop" or "keep".

Limits: two levels below the master (settings tree_depth) and sixteen sessions
in the whole tree (tree_sessions), checked at create, add-member, scale,
swap-member and extend. Sub-teams are approved by their parent lead, or by the
owner.
Sub-lead: hgs team sub-lead {"member":"qalead","sub":true}; reports-to
{member, parent} sets who it reviews.
