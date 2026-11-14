Project tasks (hgs guide tasks)

Own session/project. REF: T-xxxxxx or PROJECT/T-xxxxxx; text: stdin; --json: task.

  hgs task list [--stage review|active|todo|backlog|done]
  hgs task get REF
  hgs task add [--backlog] [--position N] [--title TEXT [--request-id UUID]]
  hgs task add --start
  hgs task start REF
  hgs task begin-attempt REF --attempt UUID
  hgs task control plan|execute|status --json
  hgs task report [--progress]|comment REF
  hgs task report-attempt REF --attempt UUID [--progress|--ready] [--considered REV]
  hgs task rename|edit REF
  hgs task move REF --position N
  hgs task stage REF todo|backlog|done
  hgs task target REF SESSION|any [--host ALIAS]
  hgs task target REF new-session|subagent --parent SESSION [--host ALIAS]
  hgs task worker REF --subagent ID --request-id SENT_REQUEST_ID
  hgs task send REF [--session SESSION [--host ALIAS]]
  hgs task handoff REF --session SESSION [--host ALIAS]
  hgs task cancel REF [--reason TEXT]
  hgs task cancel-attempt REF --attempt UUID   person only

Add queues; --start/start records work, no resend. Only the person accepts Done.
Use the prompt's one acknowledgement; begin-attempt replaces start for managed
work once controls are active.
Report with the original command; --progress updates.
Keep its pinned attempt UUID, never a newer one.
Legacy start prints a plain report command.
History-only means saved, not moved; do not retry. Without a pinned command,
ask the person to accept or resend. You cannot cancel your dispatched task;
report or hand it off. Messages do not transfer ownership; submitted handoff does.
An unknown delivery recovers without replay; else ask the person to abandon it.

Plans create none; ask the parent for a child.
Bind its live ID to the receipt. Parent reports; children never use its identity.
Unknown launch: review, not retry.

Edit: To do/Backlog. UUID replay keeps edits; changed input refuses.
Files: --content-json. Read get first.

Discussion: hgs task message REF --request-id UUID --mode queue|send|reopen|note
  --expect-dispatch ID --content-json
Get supplies dispatch_id. Queue waits for Start; note saves. Reopen
leaves Review/Done. Same UUID/mode/anchor/content looks up its
receipt, never repeats input. Coordinator receipts prove no child ownership.
Submitted is transport, not read; unknown stays unknown.
