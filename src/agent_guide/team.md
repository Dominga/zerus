Work in an agent team (hgs guide team)

A team: a lead and members on any agent, model or computer. The lead plans;
Zerus hands ready tasks to free members and reports to the lead.
Requests use canonical lowercase UUIDs (uuidgen); reuse one to retry.

Lead, in-session:
  {"request_id":"UUID","name":"Checkout","goal":"...","folder":"DIR"} |
    hgs team create --json    prints team id; you lead it
  plan while Planning (whole plan):
    {"request_id":"UUID","team":"ID","members":[{"id":"api","role":"Backend",
     "agent":"codex"}],"tasks":[{"id":1,"title":"...","criteria":["..."],
     "assignee":"api","deps":[{"task":2}]}]} | hgs team plan --json
  hard waits for Accepted, soft for In progress. Nothing runs before
  the owner approves.
  {"team":"ID"} | hgs team status --json   tasks, attempts, waits
  {"team":"ID"} | hgs team inbox --json    reports, questions, your tasks
  Reports arrive as [Zerus team messages; decide:
    {"request_id":"UUID","team":"ID","task":N} | hgs team accept --json
    reject needs "reason"; rework keeps its member, "fresh":true tries a peer.
    "hold":true keeps it back:
    {"request_id":"UUID","team":"ID","task":N} | hgs team release --json
    gives it out; hgs team hold --json holds a pending one.
  reassign, retry, cancel take the same fields; a free peer of the same
  role may get it; reassign pins it. Answer a question with hgs team
  reply; native questions and permissions: hgs team answer.
  Allow only what the task needs; else ask the owner.
  Stopped member: message, retry, reassign or ask the owner.
  Phases, extend, finish: hgs guide team-lead
  Tests: hgs guide team-tests; a dsh lead is not the owner.

Member: the message names your task and attempt. Do it, report once:
  {"request_id":"UUID","team":"ID","task":N,"attempt":"ATTEMPT","text":"..."} |
    hgs @LEAD team report --json
  Blocked: ask the lead; the attempt stays open:
    {"request_id":"UUID","team":"ID","attempt":"ATTEMPT","text":"..."} |
    hgs team ask --json
  Your task: {"attempt":"ATTEMPT"} | hgs team current --json
  A question goes through hgs team ask, never a report; a progress line
  is interim; captured, it holds new work.
  Codex on Windows: hgs guide team-run. First: hgs team lore find.
Say what changed and how you checked it; a taken task's report is
refused. Sub-teams: hgs guide team-tree.
