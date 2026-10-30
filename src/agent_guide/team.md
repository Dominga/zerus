Work in an agent team (hgs guide team)

A team has a lead and members on any agent, model or computer. The lead
plans tasks; Zerus hands ready ones to free members and reports to the lead.
Requests need a lowercase canonical UUID (uuidgen); reuse it to retry.

Lead, from your own session:
  {"request_id":"UUID","name":"Checkout","goal":"...","folder":"DIR"} |
    hgs team create --json        prints the team id; you lead it
  plan while Planning (the whole plan):
    {"request_id":"UUID","team":"ID","members":[{"id":"api","role":"Backend",
     "agent":"codex"}],"tasks":[{"id":1,"title":"...","criteria":["..."],
     "assignee":"api","deps":[{"task":2}]}]} | hgs team plan --json
  hard waits for Accepted, soft for In progress. The owner approves
  in Zerus; nothing runs before.
  {"team":"ID"} | hgs team status --json   tasks, attempts, waits, findings
  {"team":"ID"} | hgs team inbox --json    reports, questions, your tasks
  Reports arrive as [Zerus team messages; decide each:
    {"request_id":"UUID","team":"ID","task":N} | hgs team accept --json
    add "reason" to reject (it returns); "hold":true keeps it back:
    {"request_id":"UUID","team":"ID","task":N} | hgs team release --json
    gives it out; hgs team hold --json holds a pending task.
  reassign, retry, cancel take the same fields; a free peer of the same
  role may get it; reassign pins it. Members ask with hgs team ask and
  answer with hgs team reply; native questions and permission requests need
  hgs team answer. Allow only what the task needs; else ask the owner.
  When a member stops: message, retry, reassign or ask the owner.
  Phases, extend, finish: hgs guide team-lead
  Tests: hgs guide team-tests; a dsh lead is not the owner.

Member: the task message names your task and attempt. Do it, then report once:
  {"request_id":"UUID","team":"ID","task":N,"attempt":"ATTEMPT","text":"..."} |
    hgs @LEAD team report --json
  Ask the lead when blocked; the attempt stays open:
    {"request_id":"UUID","team":"ID","attempt":"ATTEMPT","text":"..."} |
    hgs team ask --json
  Your task again: {"attempt":"ATTEMPT"} | hgs team current --json
  A progress line is not a report; captured, it holds new work.
Say what changed and how you checked it. A report for a task taken from you
is refused. Sub-teams: hgs guide team-tree.
