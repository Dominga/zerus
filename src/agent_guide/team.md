Work in an agent team (hgs guide team)

A team has a lead and members on any agent, model or computer. The lead
plans tasks with dependencies; Zerus hands ready tasks to free members and
sends reports to the lead. Requests need a lowercase canonical UUID
(uuidgen | tr A-F a-f); keep it to retry the same request.

Lead, from your own session:
  {"request_id":"UUID","name":"Checkout","goal":"...","folder":"DIR"} |
    hgs team create --json        prints the team id; you are its lead
  plan while Planning (the whole plan each time):
    {"request_id":"UUID","team":"ID","members":[{"id":"api","role":"Backend",
     "agent":"codex"}],"tasks":[{"id":1,"title":"...","criteria":["..."],
     "assignee":"api","deps":[{"task":2}]}]} | hgs team plan --json
  hard waits for Accepted; soft, for In progress. The owner approves it
  in Zerus; nothing runs before.
  {"team":"ID"} | hgs team status --json   tasks, attempts, ready, findings
  Reports arrive as [Zerus team messages. Decide each one:
    {"request_id":"UUID","team":"ID","task":N} | hgs team accept --json
    add "reason" to reject; it returns for another attempt. "hold":true
    keeps it back: {"team":"ID","task":N} | hgs team release --json gives
    it out; hgs team hold --json keeps a pending task back.
  reassign, retry, cancel take the same fields; a free peer of the same
  role may get a task; reassign pins it. Members ask with hgs team ask;
  answer with hgs team reply. Allow only what the task needs in the team's
  folder; ask the owner when in doubt. Zerus says when a member stops on
  its task: message it, retry, reassign or ask the owner.
  Owner decisions are final. Phases, extend, finish: hgs guide team-lead

Member: the task message begins with [Zerus team and names your task and
  attempt. Do it, then report once with its command:
  {"request_id":"UUID","team":"ID","task":N,"attempt":"ATTEMPT","text":"..."} |
    hgs @LEAD team report --json
  Ask the lead when something blocks you; the attempt stays open:
    {"request_id":"UUID","team":"ID","text":"..."} | hgs team ask --json
  Your task and attempt again: {} | hgs team current --json
  A progress line is not a report; a question is not a report.
Say what changed and how you checked it, or end with the report as your
last message. Take blockers to the lead; a report for a task taken from you
is refused.
