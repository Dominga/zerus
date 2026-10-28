Start a build or long test as your work (hgs guide run)

Start every build and long test through Zerus so it shows as work: while it
runs your session is Building, your reply waits for it and nobody takes your
idle session for done. A job you leave running in your own shell after your
turn is a service (like a dev server): your session looks idle while it runs.
Team members use hgs team run with a template (hgs guide team-run); anything
else, and any other session, uses hgs run:

  {"request_id":"UUID","label":"cargo test","command":"cargo test --locked",
   "cwd":"/absolute/folder","timeout_minutes":30} | hgs run --json [--wait]

label is a short name (at most 80 characters); command runs in sh -c (cmd /C
on Windows) in cwd (default: here), stopped with its whole process tree after
timeout_minutes (default 30, at most 240). The output keeps its last 4000
characters. Use a new request_id for every run. Without --wait the answer is
"running" at once; --wait returns the exit code as soon as it is ready (90 s).
Ask again with the same request_id to read the result: it never runs twice.
  hgs run --list --json       your session's newest runs, running ones first
A run belongs to the session that started it; run it from inside that session.
