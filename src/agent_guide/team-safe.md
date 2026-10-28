Allow a member's routine request once for good (hgs guide team-safe)

You answered a member's permission request and the next ones will look the
same: record its shape, and Zerus allows each later request of that shape by
itself, one question at a time, with the answer recorded as "safe action".
  {"request_id":"UUID","team":"ID","action":"add","role":"QA tester",
   "shape":"cmd /c {path}\\.ci-build\\qa-qtbuild.bat {words}",
   "reason":"builds its own test programs in its worktree"} |
  hgs team safe-action --json
"member":"qa" instead of "role" names one member. "tool":"Bash" limits it to
one tool. action "list" shows the team's shapes; "remove" with "id" drops one.
Only you or the owner record them; they live in Zerus state, so no build is
needed, and the team's journal names each change.

The shape is the exact command text with typed slots:
  {path}    a path inside the member's own folder (no .., no link out)
  {word}    a name: letters, digits, _ . - (never an option like -x)
  {words}   such names separated by spaces, commas or single quotes
  {guid} {number}
Literal text must match exactly; any run of spaces counts as one. No slot
takes ; | & < > $ ( ) { } ` " @ % * ? or a line break, so nothing can be
appended to the command. A shape starts with literal text and keeps at least
12 literal characters. Zerus matches only the exact command the agent shows
(Claude's Bash and PowerShell, DeepSeek's shell calls on hosts started by this
build); anything else, a subagent request or your own request still asks you.
Record only what the member's task needs inside its own folder: never a shape
that changes other folders, the system or permissions, or deletes data.
