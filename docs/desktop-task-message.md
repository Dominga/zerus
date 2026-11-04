# Service-owned task message

The task message leaf moves one native task message, a queued message or a
note, through the admitted desktop actor (zerus-5wmh.66.4.4). G1 agreed its
plan on 2026-11-04. Its kind, binding, ledger and result stay separate from the
creation leaves and from task-start reconciliation.

## Scope

Only a queue or note `hgs task message` call belongs to this leaf. It runs from
its exact outbox checkpoint (`tasks_write::outbox::reserve_message`), which
froze the call's stdin before anything was sent. Send, reopen and deferred
native input stay review-only (see `desktop-operations.md`), and so does a
legacy record that froze no stdin.

## Current state

The closed `TaskMessage` frontend request and its wire types live in
`client/core/src/desktop/task_message.rs`. As for the creation kinds, native
Service mode and the exact configuration authenticate it. Generic
`into_request` refuses it, and it has its own ordering lane. No service
advertises `Desktop task message ABI: desktop-task-message-v1` yet, and an
authenticated request is answered `task_message_unsupported`. The actor, at
Store schema 26 by merge order, and the client path are separate slices.

## Wire

- **Intent.** The scope, the project and task, the original task message UUID
  (`request_id`) and `original`: the exact outbox record bytes, inline.
- **Verification.** Duplicate keys at the record or intent level are refused
  before any value is built. The record must decode (`outbox::decode`, which
  refuses a malformed frozen input), carry the request ID and have frozen its
  stdin as `input` or `input_sha256`. Its mode must be `queue` or `note`, its
  dispatch empty or a UUID, and `message_call` must reproduce the frozen stdin.
- **Size.** The complete service line must fit the existing 64 KiB control
  frame: the `desktop-v1` hello that the `frontend-request` child wraps around
  the envelope, plus its newline (`ui_requests::fits_frontend_line`). A test
  pins that helper to the child's own encoding. An original that does not fit
  is refused before it is sent and stays reviewable. There is no frame
  increase, no renderer-supplied path and no import Publisher as execution
  authority.
- **Plan.** The project, task, request, mode, dispatch and the SHA-256 of the
  exact stdin. `Verified::call` builds the one call, or its `--recover-only`
  check, with that stdin.
- **Binding.** The kind, schema 1, the configuration, the project, task and
  request IDs, the raw source, UI snapshot (`outbox::snapshot`) and input
  digests, and the plan and consent digests.
- **Receipt.** The phases are Queued, Reserved, Running, Recorded, Failed and
  Unknown. Recorded carries the closed outcome: the request, mode, dispatch
  and a `queued` or `note` state. That is the exact CLI metadata outcome,
  never native delivery or consumption. Only the exact original's drained
  Recorded or Failed receipt settles; Queued, Running and Unknown never do.
- **Lookup.** A scoped read-only lookup of the original by its plan
  fingerprint. A missing result never authorizes anything.
