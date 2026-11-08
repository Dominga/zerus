Work in an agent team (hgs guide team)

A lead plans tasks and Zerus hands each ready task to a free member, on any agent and
computer. Requests are JSON on stdin with a lowercase UUID "request_id"; reuse one to retry.
A doorbell in your terminal means unread messages: {} | hgs team inbox --json.
Work only on what hgs team current shows, else ask the lead. A message naming a task you
do not hold is refused.

Report once, when the task is done:
  {"request_id":"UUID","team":"ID","task":N,"attempt":"ATTEMPT","text":"..."} |
    hgs @LEAD team report --json
Say what changed and how you checked it. A progress line is interim; only hgs team
report reports, and a report on a task you did not take is refused.

Blocked: ask the lead. The attempt stays open:
  {"request_id":"UUID","team":"ID","attempt":"ATTEMPT","text":"..."} | hgs team ask --json
A question goes through hgs team ask, never a report. Anything else for the lead:
  {"request_id":"UUID","to":"lead","text":"...","urgent":true} | hgs team message --json
A task that can go on only after CI, an install or a card: never poll or sleep, park it
(hgs guide team-park). A job past its estimate: {"label":"x","minutes":15} | hgs team
expect --json. Your pulse for the lead, each part at most 500 characters:
  {"attempt":"ATTEMPT","done":"...","next":"...","blocked":"..."} | hgs team note --json

First: hgs team lore find. Builds and tests: hgs guide team-run and team-tests. Codex on
Windows: team-run too. Leading a team: hgs guide team-lead.

Scratch teams (the QA right). With the operate profile (the QA right) you run live
checks in your own scratch teams: hgs team create --json from your session with a
folder inside the team's scratch root (its scratch_root, or <team folder>/.qa-scratch).
You lead it, its plan needs no card, and the parent's caps apply. Outside the root,
without the right, or before the owner granted the team's standing rule, the create
is refused. Zerus stops it when the task you held when creating it is decided.
In a sandbox call hgs team directly, one command per call with --json-file PATH: your
own calls and your scratch team's queue in your folder; a script still asks.

A performance run checks the machine first, and a comparison alternates the
binaries in rounds (hgs guide team-tests).
Tauri shows archive stubs; open a report to read that task only.
