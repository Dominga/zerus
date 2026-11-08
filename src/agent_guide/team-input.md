Send team requests that are accepted the first time (hgs guide team-input)

Every hgs team OPERATION --json reads one JSON object on stdin. PowerShell has
no <<<: save it and use --json-file PATH.

A member has one shape in plan, add-member and a draft:
  {"id":"dev","role":"Backend","agent":"claude","account":"work",
   "model":"opus","effort":"medium"}
Optional: models ([{"model":"opus","effort":"high"}]), prompt, folder,
computer, permissions. A lead has the same shape; a task is
{"id":1,"title":"...","description":"...","assignee":"dev"}.
Unknown fields are refused with accepted fields and an example; nothing is
silently dropped. Task text is "description".

Without account, a member uses its computer's default for its agent; preflight
names these members. Set account when the owner wants another one.
max_members counts members besides the lead.

Draft updates change only named fields. "members" replaces the list, or maps
changes by id:
  "members":{"dev":{"model":"opus"},"ux":{"role":"UX","agent":"kimi"},"qa":null}
changes dev, adds ux, removes qa. draft get answers fields and the current draft.
plan_mode is a draft field (yours an offer with plan_mode_reason).

A swap card changes agent, model, effort, account, prompt, session, or
"fresh":true for a new session with the same settings. One card swaps several:
  {"kind":"swap-member","reason":"...","members":[{"id":"dev",
   "account":"additional"},{"id":"qa","account":"additional"}]}
Zerus pauses replaced sessions as <name>-replaced; new ones take their names.
One owner sentence ("relaunch them all") may pay for each.

While running, settings accepts members_update:
  {"team":"ID","members_update":[{"id":"dev","prompt":"...","model":"opus",
   "effort":"high"}]}
Model/effort switches before the next task. Prompt changes send one "replaces
previous instructions" message; clear_context_first:true clears first.
after_task:compact|clear (after_task_on:percent|tokens, after_task_at) runs
before the next task once the context passes the threshold.
The owner edits prompt, model, effort, permissions and settings; the lead edits
model/effort. Agent, account and computer stay with Swap member.

hgs team look MEMBER (or {"member":"dev"} | hgs team look --json) shows its
screen, each source's view, reconciled state (uncertain on disagreement), and
Zerus's next action.
