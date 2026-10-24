Work in an agent team (hgs guide team)

A team has a lead session and members on any agent, model or computer. The
lead plans tasks with dependencies; Zerus hands each ready task to a free
member and sends the member's report back to the lead. Requests need a
lowercase canonical UUID: uuidgen | tr A-F a-f. Keep it to retry.

Lead, from your own session:
  echo '{"request_id":"UUID","name":"Checkout","goal":"...","folder":"DIR"}' |
    hgs team create --json        prints the team id; you are its lead
  plan while it is Planning (the whole plan each time):
    {"request_id":"UUID","team":"ID","members":[{"id":"api","role":"Backend",
     "agent":"codex","model":"","computer":"","folder":"worktree"}],
     "tasks":[{"id":1,"title":"...","description":"...","criteria":["..."],
     "assignee":"api","deps":[{"task":2,"kind":"hard"}]}]} | hgs team plan --json
  hard waits for Accepted; soft lets the task start once its dependency is
  In progress. The owner approves the plan in Zerus; nothing runs before.
  {"team":"ID"} | hgs team status --json   tasks, attempts, ready and findings
  Reports arrive as messages that begin with [Zerus team. Decide each one:
    {"request_id":"UUID","team":"ID","task":N} | hgs team accept --json
    add "reason" and use reject; the task returns for another attempt.
  reassign (with "member"), retry and cancel take the same fields.
  A missing skill: add-member with {"member":{"id","role","agent"}}, then
  reassign a task to it; it starts with that task.
  A member's question comes to you as a message that begins with
  [Zerus team · from: ROLE · question]. Answer it once with the command at
  its end, choosing option ids or writing text:
    {"team":"ID","member":"MEMBER","question_id":"Q","expected_question_hash":"H",
     "answers":[{"question_id":"ITEM","selected_option_ids":["0"],"text":""}]} |
      hgs team answer --json
  Members' permission requests come the same way ([Zerus team · from: ROLE ·
  permission]; options allow and deny): allow only what the task needs inside
  the team's folder, deny the rest, and ask the owner when in doubt.
  The owner's decisions are final; yours cannot undo them.

Member: a task arrives as a message that begins with [Zerus team. Do it,
then report once with the command at its end (it names the attempt):
  {"request_id":"UUID","team":"ID","task":N,"attempt":"ATTEMPT","text":"..."} |
    hgs @LEAD team report --json
Summarize what changed, where, and how you checked it. A report for a task
taken from you is refused; stop that work.
