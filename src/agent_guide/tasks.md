Tasks (hgs guide tasks)

Own session/project; REF: T-xxxxxx or PROJECT/T-xxxxxx; stdin text; --json: task.

  hgs task list [--stage review|active|todo|backlog|done]
  hgs task get REF
  hgs task add [--backlog] [--position N] [--title TEXT [--request-id UUID]]
  hgs task add --start
  hgs task start REF
  hgs task begin-attempt REF --attempt UUID
  hgs task control plan|execute|status --json
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
  hgs task cancel-attempt REF --attempt UUID [--reason TEXT]   person only

Add queues; --start/start records work, no resend. Only person accepts Done.
Acknowledge once as prompted. Controls inactive: start now; after queue activation,
begin-attempt replaces managed start.
Report: original command/pinned UUID, never get/start's latest attempt ID.
--progress updates. Legacy start prints report command without UUID.
History-only: saved, not moved; no retry. No pinned command: ask person to accept/resend.
Agents cannot cancel dispatched tasks: report/handoff. Only submitted handoff
transfers ownership, never messages.
Unknown delivery: recover, never replay. If unresolved, ask person to abandon
that exact attempt.

Plans launch none. Use Start dialog or ask parent for a child; bind live ID
to receipt. Parent reports; children never use parent identity.
Unknown launch: review, no retry. No task Pause.

Edit: To do/Backlog. Same UUID keeps edits; changed input refuses.
Files: --content-json; get first.

hgs task message REF --request-id UUID --mode queue|send|reopen|note
  --expect-dispatch ID --content-json
Get: dispatch_id/executor. Queue waits for Start; note saves. Reopen leaves
Review/Done after submission. Same UUID/mode/anchor/content: receipt lookup,
never repeated input. Coordinator receipts prove no child ownership.
Submitted: transport, not model-read; unknown stays unknown.
