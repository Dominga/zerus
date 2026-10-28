Project tasks (hgs guide tasks)

Own session/project; PROJECT/T-xxxxxx selects another. Text: stdin; --json: task.

  hgs task list [--stage review|active|todo|backlog|done]
  hgs task get REF
  hgs task add [--backlog] [--position N] [--title TEXT] [--request-id UUID]
  hgs task add --start
  hgs task start REF
  hgs task report|comment REF
  hgs task rename|edit REF
  hgs task move REF --position N
  hgs task stage REF todo|backlog|done
  hgs task target REF SESSION|any [--host ALIAS]
  hgs task target REF new-session|subagent --parent SESSION [--host ALIAS]
  hgs task worker REF --subagent ID --request-id SENT_REQUEST_ID
  hgs task send REF [--session SESSION [--host ALIAS]]
  hgs task handoff REF --session SESSION [--host ALIAS]
  hgs task cancel REF [--reason TEXT]

Add queues later work; add --start or start REF records current work without
resending. Report changes, checks and remaining work/blockers: it moves to Review.
Only the person accepts Done. You cannot cancel your dispatched task; report
instead. Handoff moves active ownership after native submission; messages do not.

Plans create no executor. Explicit Start opens the launch dialog or asks the
parent to create a subagent. Register its observed live ID
against the sent receipt. The parent reports; children never use its identity.
Unknown launch state needs review, not retry; no distributed lease or task Pause.

Rename changes neither description nor owner. Edit is for To do/Backlog.
Creation UUIDs bind title, content, target, stage and start intent. Exact replay
recovers the card without reverting edits; changed input with that UUID fails.
Add/edit/comment accept --content-json with text and attachments. Comments retain
media without native input; read get REF before continuing.

Discussion: hgs task message REF --request-id UUID --mode queue|send|reopen|note
  --expect-dispatch ID --content-json
Read get REF for dispatch_id and immutable executor. Queue waits for explicit
Start; note only saves. Send targets the current execution. Reopen leaves
Review/Done only after submission. A coordinator receipt does not prove child
delivery or change ownership. Retry only the SAME UUID, mode, anchor and content:
it reads the receipt without repeating reserved native input. Submitted means
transport accepted, not model-read. Unknown stays unknown.
