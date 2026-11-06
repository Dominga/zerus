Send team requests that are accepted the first time (hgs guide team-input)

Every hgs team OPERATION --json reads one JSON object on stdin. PowerShell has
no <<<: save the object and pass it with --json-file PATH instead.

A member has one shape in plan, add-member and a draft:
  {"id":"dev","role":"Backend","agent":"claude","account":"work",
   "model":"opus","effort":"medium"}
optionally with models ([{"model":"opus","effort":"high"}]), prompt, folder,
computer and permissions. A lead is the same object; a task is
{"id":1,"title":"...","description":"...","assignee":"dev"}.
An unknown field is refused with the accepted ones and an example: nothing is
dropped in silence. A task's text is "description".

Without "account" a member starts on its computer's default account for its
agent; the preflight names those members. Set "account" when the owner wants
another one. max_members counts the members besides the lead.

A draft update sets only the fields it names. "members" is a list that
replaces them all, or an object of changes by member id:
  "members":{"dev":{"model":"opus"},"ux":{"role":"UX","agent":"kimi"},"qa":null}
changes dev, adds ux and removes qa. draft get answers fields, what the draft
holds now. plan_mode is a draft field (yours an offer with plan_mode_reason).

A swap card names what changes for each member: agent, model, effort, account,
prompt or session, or "fresh":true for a new session with the same settings.
One card swaps several members:
  {"kind":"swap-member","reason":"...","members":[{"id":"dev",
   "account":"additional"},{"id":"qa","account":"additional"}]}
Zerus pauses each replaced session and keeps it as <name>-replaced, so the new
session takes the member's name. The owner's word may name them together
("relaunch them all"): one sentence then pays for each.

While the team runs, hgs team settings takes its prompt, model, effort,
permissions and settings:
  {"team":"ID","members_update":[{"id":"dev","prompt":"...","model":"opus",
   "effort":"high"}]}
A model or effort change switches the member before its next task; a prompt
change reaches it as one message marked "replaces previous instructions"
("clear_context_first":true clears it first). agent, account and computer stay
with Swap member. Only the owner changes a running member; the lead changes its
model and effort.
