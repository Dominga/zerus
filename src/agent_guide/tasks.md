Project tasks (hgs guide tasks)

Own session/project; REF can be PROJECT/T-xxxxxx. Text: stdin; --json: task.

  hgs task list [--stage review|active|todo|backlog|done]
  hgs task get REF
  hgs task add [--backlog] [--position N] [--title TEXT [--request-id UUID]]
  hgs task add --start
  hgs task start REF
  hgs task report REF [--progress]
  hgs task report-attempt REF --attempt UUID [--progress|--ready] [--considered REV]
  hgs task comment REF
  hgs task rename REF
  hgs task edit REF
  hgs task move REF --position N
  hgs task stage REF todo|backlog|done
  hgs task target REF SESSION|any [--host ALIAS]
  hgs task target REF new-session|subagent --parent SESSION [--host ALIAS]
  hgs task worker REF --subagent ID --request-id SENT_REQUEST_ID
  hgs task send REF [--session SESSION [--host ALIAS]]
  hgs task handoff REF --session SESSION [--host ALIAS]
  hgs task cancel REF [--reason TEXT]

Add queues; --start/start records work, no resend. Only the person accepts Done.
Report changes/checks/blockers with the original command.
Managed work: keep its original pinned report-attempt UUID; --progress gives updates.
Never substitute an ID from get/start. An explicit legacy start prints its ordinary
report command without a UUID. History-only means saved, not moved; do not retry it.
Without a pinned command, ask the person to accept or resend.
You cannot cancel your dispatched task; report or hand it off. Messages do not
transfer ownership; handoff does so after native submission.

Plans create no executor. Start opens a dialog or asks the parent for a subagent.
Bind its observed live ID to the receipt. Parent reports; children never use its
identity. Unknown launch needs review, not retry. No lease or task Pause yet.

Rename keeps description/owner; edit is for To do/Backlog. Creation UUID replays
keep edits; changed input refuses. Add/edit/comment take --content-json
(text/attachments). Read get REF before continuing.

Discussion: hgs task message REF --request-id UUID --mode queue|send|reopen|note
  --expect-dispatch ID --content-json
Get supplies dispatch_id/executor. Queue waits for Start; note saves. Reopen
leaves Review/Done after submission. Retry SAME UUID/mode/anchor/content: receipt
lookup never repeats input. Coordinator receipts prove no child delivery/ownership.
Submitted means transport accepted, not model-read; unknown stays unknown.
