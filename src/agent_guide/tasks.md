Project tasks (hgs guide tasks)

The project's shared task list. Run these in your own session: they act as it
and on its project unless a reference names another (PROJECT/T-xxxxxx). Texts
go on stdin; a change prints one line saying what happened (--json: the task).

  hgs task list [--stage review|active|todo|backlog|done]   the tasks, JSON
  hgs task get REF                          one task with its history, JSON
  hgs task add [--backlog] [--position N] <<'EOF'   file a task for yourself
  hgs task add --start <<'EOF'               record work you are doing now
  hgs task start REF                         take or acknowledge work without sending it again
  hgs task report REF <<'EOF'               report a task you were sent
  hgs task comment REF <<'EOF'              a note, never pasted into a session
  hgs task edit REF <<'EOF'                 a new prompt (To do, Backlog)
  hgs task move REF --position N            priority within its current stage
  hgs task stage REF todo|backlog|done      done only when the person asks
  hgs task target REF SESSION|any [--host ALIAS]   where it will be sent
  hgs task target REF new-session|subagent --parent SESSION [--host ALIAS]  plan an executor
  hgs task worker REF --subagent ID --request-id SENT_REQUEST_ID  register a native child
  hgs task send REF [--session SESSION [--host ALIAS]]   Send now
  hgs task handoff REF --session SESSION [--host ALIAS]   transfer your active task
  hgs task cancel REF [--reason TEXT]       stop another session's task

Rules
- Plain add queues a task for later; it does not mean work started. For a direct
  request you are already doing, use add --start, or start REF for a queued task.
- Handoff transfers your active task and its pinned target only after native
  submission to the chosen session. A normal message does not change ownership.
- A report says what changed, how it was checked and what is left undone or
  why it cannot be done; it moves the task to In review.
- You cannot cancel a task you were sent; report what is left undone instead.
- When the person asks to keep something for later, file it with add, not in
  your own plan.
- Assignment plans save intent only; a new executor is created on explicit Start.
  New-session plans start through the desktop launch dialog. Subagent plans go to
  the parent coordinator, which creates the child and registers its observed live
  ID against the current sent receipt before claiming it started. The parent
  remains the coordinator/reporter; children must not impersonate that CLI identity.
- start REF acknowledges a dispatched task once in its exact owner session. It
  neither pastes another prompt nor creates a second dispatch.
- add, edit and comment accept --content-json with text and attachments on stdin.
  Comments retain their media in history; they are not new native input. Inspect
  get REF for current comments before continuing work.
- Each desktop reserves new-session delivery before transport. A restored or
  uncertain reservation needs explicit review, never an automatic retry. These
  local records do not provide a distributed execution lease or task-scoped Pause.
