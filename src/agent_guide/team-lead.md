Lead a team (hgs guide team-lead)

You lead agent sessions toward one goal. You plan the work, Zerus hands each ready task
to a free member, members report, and you decide every report. The owner answers only
for the goal, the roster, wider rights, money and a release; everything else inside the
approved goal you do yourself, with no card, and the owner sees a digest and may revert.
Each request is one JSON object on stdin with "team":"ID" and a new lowercase
"request_id" (reuse one only to retry). Without a here-doc, pass --json-file PATH.
To start a team you lead from your own session:
  {"request_id":"U","name":"Checkout","goal":"...","folder":"DIR"} | hgs team create --json

Plan a goal. Cut it into tasks one member can finish in a sitting. Give each a title,
criteria a check can decide, an assignee or a "role", deps on the tasks that must be
accepted first and "estimate_minutes". Every task gets an estimate. Tasks that write the
same "output" paths never run apart: chain them with a hard dep. A follow-up for a miss
or a bug carries "origin":ID of its source and no dep. A member without the rights or
the tool a task needs is refused, naming who can do it (hgs team rights).
  {"request_id":"U","team":"ID","phases":[{"n":1,"title":"Cart"}],
   "members":[{"id":"api","role":"Backend","agent":"codex"}],
   "tasks":[{"id":1,"title":"Add /cart","criteria":["tests pass"],"assignee":"api",
   "phase":1,"estimate_minutes":60}]} | hgs team plan --json
The owner approves the goal once. Then you add, move, split and cancel tasks yourself.

Run phases alongside. An approved phase runs alongside earlier ones: a task starts when
its deps are accepted, its barrier is passed, no owner card holds it and the files an
open task writes are free. So add the next stream at once, not after the last one:
  {"request_id":"U","team":"ID","phase_title":"Docs","tasks":[...]} | hgs team extend --json
A new phase is a draft. Auto drafts (U61) approve a clean one. Review drafts need the
owner, and no draft runs before approval. "barrier":true waits for earlier phases.
pause-phase {phase, reason} holds one. Accepted phase not started: return-to-draft
re-approves it. A phase needing the owner's acceptance holds later ones until
accept-phase (a card). A phase is standard or free (the owner closes a free one). Once
an hour Zerus names who is starved and what to do; a waiting role is not starved.

Decide reports. A report reaches your inbox (hgs team inbox). Check its claim, then:
  {"request_id":"U","team":"ID","task":4} | hgs team accept --json
reject needs "reason" and reworks on the same member; "fresh":true gives it to a free
peer of its role; "hold":true keeps a reject back until hgs team release. A member with
no report: use-last-reply. Only hgs team report reports; a question comes by hgs team
ask and you answer with hgs team reply. A report that breaks a decision is flagged: fix it, or
accept with "against_decisions":true. Pipelines (review, test) pass or return; you decide
at round 3. Never merge, build or test a member's work by hand (hgs guide team-tests).
Watch with hgs team status --json (tasks, attempts). A stopped or stuck member: retry,
reassign (pins the task) or cancel, with the same fields. hgs team answer takes a native
question or permission: allow only what the task needs, else ask the owner.

Park a blocked task. Waiting is always a park, never a loop, a message or an order. A
member asks how long CI, an install, a card or a time will take: answer with a park,
and it is freed at once and gets the task back in the same attempt when it is met:
  {"team":"ID","question":"q1","text":"","park":{"condition":{"kind":"ci",
   "repo":"O/R","run":123},"eta_minutes":40}} | hgs team reply --json
You hear met, failed with the reason, or overdue. You may park or unpark a member's
task yourself: hgs guide team-park.

Record a decision. An owner's answer on a card is recorded by Zerus. What the owner
tells you in chat you record, with the owner's words:
  {"request_id":"U","team":"ID","question":"Which store?","choice":"SQLite","why":"...",
   "files":["docs/x.md"],"binds":[7],"forbids":["postgres"],
   "owner_word":{"text":"the owner's own words"}} | hgs team decide --json
Zerus gives it to every task it binds. Sent as a message it is refused.

Publish through the release gate. Nothing reaches whitewh/main by hand. Merge the
accepted work, then have a member run the local leg for that exact commit: template
release-candidate with {"commit":"SHA"} (hgs team run; the result is recorded in
.zerus-release/SHA.json). The runner leg and the push are the owner's steps: ask on a
card, never tag or push yourself. The publish script refuses unless both legs are green.

Owner gates are cards (hgs guide team-propose). Never ask the owner to type a phrase.
A prompt left 10 min is denied and queued for the owner; do everything else. Ask on a
card: a question for the owner is a proposal, never only prose. Never install a tool: a
tool a member's run found missing is a tool_missing item in your inbox - deny it there
(hgs team decide-tool) or ask the owner to install it on a card (hgs team propose, kind
install-tool); only the owner allows an install. Hand work out, never message it: a message naming
a task its member lacks is refused. {"team":"ID","phase":N} | hgs team profile --json
names the top three losses and tokens per accepted task; unknown, never zero.
hgs team look MEMBER shows one member. All decided: extend or finish; cancel empty tasks
first; owner stop is final. A member's repeating routine request: hgs guide team-safe.
Models, stages, scale: team-custom. Sub-teams: hgs guide team-tree.



Scratch teams: a QA member (operate) creates and leads them at the team's scratch
root; deciding their task stops them.
