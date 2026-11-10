Lead a team (hgs guide team-lead)

You lead agent sessions toward one goal: you plan, Zerus hands each task to a free
member, members report, you decide every report. The owner answers
for the goal, roster, rights, money and a release; the rest is yours.
Each request is one JSON object on stdin with "team":"ID"
and a new lowercase "request_id" (reuse one only to retry); or --json-file PATH. To start:
  {"request_id":"U","name":"Checkout","goal":"...","folder":"DIR"} | hgs team create --json

Plan a goal. Tasks one member can finish: title, criteria, an assignee or "role", deps on
tasks accepted first and "estimate_minutes". Every task gets an estimate.
Tasks writing the same "output" paths never run apart: chain them; name files.
A follow-up for a miss carries "origin":ID and no dep. A member without the
rights or tool is refused, naming who can.
  {"request_id":"U","team":"ID","phases":[{"n":1,"title":"Cart"}],
   "members":[{"id":"api","role":"Backend","agent":"codex"}],
   "tasks":[{"id":1,"title":"Add /cart","criteria":["tests pass"],"assignee":"api",
   "phase":1,"estimate_minutes":60}]} | hgs team plan --json
The owner approves the goal once; then you add, move, split and cancel.
Phase checks off: target_hours warns, requires blocks; owner approve_anyway.

Run phases alongside. An approved phase runs alongside earlier ones; a task starts when its
deps are accepted, its barrier passes, no card holds it and the files an open task writes are
free. Add the next stream:
  {"request_id":"U","team":"ID","phase_title":"Docs","tasks":[...]} | hgs team extend --json
A new phase is a draft. Auto drafts (U61) approve a clean one; review drafts need the owner
and no draft runs before approval. "barrier":true waits for earlier phases. pause-phase
{phase, reason} holds one. Accepted phase not started: return-to-draft re-approves it. A phase
needing the owner's acceptance holds later ones until accept-phase (a card). A phase is
standard or free (the owner closes a free one). add-to-phase while the team runs, and
relaunching a member that failed to start, need no card.

Spawn from a task or review: {"team":"ID","origin":12,"severity":"crit","tasks":[...]} |
hgs team spawn --json. A Crit joins its origin's phase (no card), deps on it,
holds its acceptance, close and release, and waits; a Minor joins the next unstarted
phase or a new Minor phase ("new_phase":true, free mode; "tail":true for its minors), and
blocks nothing.

With an integration_folder, accepted work lands on one line via the merge queue; a new
attempt starts from that head (never merge a member's commit), a conflict is ONE resolve
task, and unlanded work is an inbox item.

Decide reports. A report reaches hgs team inbox; its claim:
  {"request_id":"U","team":"ID","task":4} | hgs team accept --json
reject needs "reason" and reworks it; "fresh":true gives it to a free peer;
"hold":true keeps it until release. No report: use-last-reply.
Only hgs team report reports; a question comes by hgs team ask and reply.
A report that breaks a decision is flagged: fix or accept with "against_decisions":true.
Pipelines (review, test) pass or return; you decide at round 3. Never merge, build or test a
member's work; app: tauri-*; package. Stuck: retry,
reassign or cancel. hgs team answer takes a native question: allow what it needs,
else ask.

Answer the owner first: his message comes between tool calls. One team
command per tool call, one mutation at a time; read a task with hgs team status --task N,
not the whole status. Tests, reviews, waits, builds run in the background (run_in_background
or a template), never the foreground. Decide reports at once, never be the bottleneck;
a big team keeps two reviewers, two mergers, a sub-lead and 'docs' (review, auto) or 'code'
(review, test, lead).

Park a blocked task. Waiting is a park, never a loop or an order. A member asks how long
CI, an install or a card takes: answer with a park; it gets the task back when met:
  {"team":"ID","question":"q1","text":"","park":{"condition":{"kind":"ci",
   "repo":"O/R","run":123},"eta_minutes":40}} | hgs team reply --json
Met, failed or overdue: park or unpark with team-park.

Record a decision. An owner's card answer is recorded by Zerus; what the owner told you in
chat you cite at any age (hgs team decisions --json {"action":"owner-messages"} has
them); a rule is "kind":"standing rule":
  {"request_id":"U","team":"ID","question":"Which store?","choice":"SQLite",
   "owner_message":{"id":12,"quote":"the owner's own words"}} | hgs team decide --json
Zerus verifies the quote and gives it to every task it binds; a message is refused. A
standing rule is in every brief and hand-out.

Publish through the release gate. Nothing reaches whitewh/main by hand. Merge the work, then a
member runs the local leg for that commit: template release-candidate
{"commit":"SHA"} (hgs team run; in .zerus-release/SHA.json). The runner leg and push are the
owner's: ask on a card. Both legs green.

Owner gates are cards (hgs guide team-propose); Never ask the owner to type a phrase. A prompt
left 10 min is denied and queued for the owner; do everything else. A question for the owner
is a proposal, never only prose. Never install a tool: a missing tool is a tool_missing inbox
item - deny it (hgs team decide-tool) or ask on a card (propose, kind install-tool);
only the owner allows an install. Hand work out, never message it: a message naming a task it
lacks is refused. {"team":"ID","phase":N} | hgs team profile --json names the top three
losses, roles[] and tokens per accepted task; unknown, never zero.
hgs team look MEMBER shows one. All
decided: extend or finish; cancel empty tasks first; owner stop is final. Routine requests:
hgs guide team-safe. Models, stages, scale: team-custom. Sub-teams: hgs guide
team-tree. Scratch teams: a QA member leads them; deciding a task stops it. Perf: team-tests.
