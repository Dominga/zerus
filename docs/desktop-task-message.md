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
Service mode and the exact configuration authenticate it, both in the
service and in the GUI's responder before it answers for the request. Generic
`into_request` refuses it, and it has its own ordering lane. On Linux the
service advertises `Desktop task message ABI: desktop-task-message-v1`, and
the admitted desktop actor executes it (below). The leaf admits only queue and
note originals and verifies only their queued or note canonical metadata; send,
reopen and native delivery or consumption stay unavailable.

macOS advertises nothing and answers `task_message_native_image_unavailable`
before any preparation, row or child: no qualified seam yet attests the code a
macOS child runs before it receives the private input. Windows answers
`task_message_unsupported` and advertises nothing.

No client sends the kind yet: the editor still sends every message through its
direct path. The client path, in desktop-service mode only, is a separate
slice. It must keep the outcome described in [Oversize
originals](#oversize-originals).

## Actor

The actor (`src/desktop/task_message`) mirrors the creation launch leaf. It
shares the machine-source and lease guard; its consent, ledger, ticket and
result are its own.

- **Admission.** The opaque native permit is rechecked after `BEGIN
  IMMEDIATE`, together with the current writer epoch, the local native machine
  source, the original service owner and the handoff exclusion. Admission then
  durably retains the immutable source, the fixed plan and the consent binding.
  Live consent exists only in memory: a restarted or imported queued original
  is retired as Unknown (`task_message_interrupted`) and never executed.
- **Storage.** The ledger has its own tables, `desktop_task_messages` and
  `desktop_task_message_budget`, added at Store schema 26 by merge order. Rows
  are keyed by the original task message UUID. The budget allows 4096 rows,
  16 MiB and 32 active originals; a source is at most 64 KiB. The same request
  ID with another original is refused and never replaces the first.
- **Effect.** One checked step, then one original child: this computer's own
  `hgs` with the plan's exact `hgs task message` arguments
  (`Verified::call`), in its own process group. Its stdin is exactly the
  record's frozen input, written without blocking inside the drain loop. Its
  stdout is kept up to 1 MiB for the reply and its stderr discarded, with the
  direct path's 90-second deadline. An exit counts only after the child read
  all of its stdin, exited and both of its output pipes ended within the
  deadline.
- **Image.** On Linux the service pins its own executable when it starts
  (`zerus_client_core::image::linux::Pin`), and the child executes that
  retained descriptor, never a reopened pathname. The final check, last
  before the spawn, requires the pinned image still at its path: a
  replacement before it starts no child (Failed, never started), and one
  after it never runs or receives the input. The pinned image is one sealed
  in-memory snapshot of at most 256 MiB: a larger image, such as a debug
  build, is refused, and without a pinned image nothing starts.
- **Custody.** The child inherits the effect's close-only custody lock, and its
  `main` first marks that descriptor close-on-exec
  (`HGS_TASK_MESSAGE_CUSTODY_FD`, then removed). The lock lives exactly as long
  as the child, and nothing it starts inherits it.
- **Timeout, revoke, exit.** At the deadline only the original child is killed,
  never its process group, and the result is Unknown. Revoke or a service stop
  never kills the child: a step check before the child refuses to start it,
  and a running child finishes on its own while custody keeps its charge.
  After a service exit the child keeps its lock; recovery waits for the exact
  lock's release and then drains the row as Unknown.
- **Physical drain.** The original's resources outlive every original pipe.
  They are the same custody lock and the original's consent, which holds the
  lease and the serve-owner loan. A pipe that is still open when the result is
  recorded, for example because a descendant holds it, keeps a copy in its own
  reader until its real end; what it reads is discarded and nothing is
  signalled. The recovery therefore drains the row, and the charge and a
  takeover are released, only after that end of file and the kernel release
  of the lock. A revoke or stop changes neither. An end that can never be
  observed (a failed read or an unverified wait) keeps the resources until the
  service exits.
- **Result.** Exit 0 is recorded only with the closed outcome that the exact
  reply proves, read by the recovery policy's own `reply_state` (the same task,
  project, request, mode, dispatch and a `queued` or `note` state, with no
  native delivery fields). The ledger keeps only that outcome, never the raw
  reply, in its own immutable column. Exit 0 without that proof is Unknown
  (`task_message_reply_unconfirmed`), and a non-zero exit is recorded without
  an outcome.
- **Settlement.** Only after the actual resource drain: a proven exit 0 is
  Recorded, a non-zero exit is Failed (`task_message_exited`), and a child
  that never started is Failed (`task_message_not_started`). Never started is
  a closed internal result, only before any child exists: the own image
  refused, custody refused, or `spawn` failed. Everything else, including a
  signal, the deadline, incomplete input or output, a lost result or a restart
  before the result, is Unknown, with its reason as the code. Unknown is never
  sent again.
- **Lookup.** `task_message_result` reads the exact original by its query; a
  changed scope is refused and a missing original is reported as not found.

## Oversize originals

Attachments travel inline in the original, base64-encoded in its content, so
most messages with an attachment exceed the 64 KiB control frame: an inline
original carries less than 48 KiB of attachment data in total. Such an
original is refused before it is sent, with `task_message_original_too_large`
from `Intent::verify`, which the service applies again. The user-visible
outcome the client path must keep:

- a clear refusal that names the size limit and the attachments, shown in the
  editor and in recovery review, never a silent drop;
- the draft and its attachments are kept unchanged, nothing was sent, and the
  message is never retried through another path on its own;
- the outbox checkpoint, if already reserved, stays reviewable as never sent.

A later slice should send attachments by reference so that such messages can
use the service at all. The GUI would stage each attachment in a private,
digest-named file under the configuration's desktop state. The original would
carry only each attachment's digest and size. The service would read the
staged files from its own fixed root (never a renderer-supplied path), rebuild
the exact stdin and check it against the frozen `input_sha256` before
admission. The control frame stays 64 KiB, and a staging budget bounds the
staged bytes.

## Client

`client/src-tauri/src/desktop_service/task_message.rs` sends one original and
then only reads its result. It has no caller yet: the editor's routing in
desktop-service mode lands together with attachments by reference.

- **Consent.** Only the outbox record reserved by the same explicit Submit is
  sent, and only once. A record from before is never sent again.
- **Handoff marker.** Before the admission request is written,
  `tasks_write::outbox::hand_off` durably records that the exact record (its ID
  and snapshot digest) was handed to the service. The marker is evidence, never
  permission. A marked record recovers only through the scoped read-only
  lookup, never through the direct `--recover-only` check: the editor's
  explicit recovery and the recovery policy (`TaskMessageRecovery`) both refuse
  a present marker, and an unreadable or foreign one too. A marked record that
  the lookup does not find, including after a crash between the marker and the
  request, stays in review until the user explicitly discards it or confirms
  sending it again as a new record. An unreadable marker, or one that names
  another record, is never taken as absent. Releasing or settling the record
  removes the marker last, in the same durable step, after a directory sync
  that makes the record's removal durable first: a record never outlives its
  marker.
- **Answers.** Recorded settles like a delivered reply. Unsent means nothing
  ran: the request never left the client, or the exact drained receipt proves
  that the child never started. Only an Unsent answer releases the original,
  through the caller's release step, before it is returned. Unconfirmed means
  the original failed or its result is unknown; it is never sent again and
  never takes the direct path. Direct is the only answer that uses the direct
  path, after a completed capability probe that shows no support for the kind.

## Wire

- **Intent.** The scope, the project and task, the original task message UUID
  (`request_id`), `original` and its explicit `form`. The default inline form
  (never serialized) carries the exact outbox record bytes. The
  `reference-v1` form carries the record's reference projection (below).
- **Reference form.** `tasks_write::references` projects a record whose stdin
  was frozen in its canonical form (`input_sha256`) and has attachments. Each
  attachment's `data_base64` becomes a `data_ref` with the raw bytes' SHA-256
  and size; every other field, the attachment order and metadata included,
  stays exact, and only the one canonical base64 encoding projects. The
  existing attachment limits hold: at most 8, each at most 10 MiB, 20 MiB in
  total. Rebuilding requires each raw attachment's exact size and digest
  (`task_message_reference_mismatch`), uses the same canonical serializer, and
  the rebuilt stdin must hash to the frozen `input_sha256`
  (`task_message_input_changed`); its exact length is known before any
  attachment is read.
  The form is stated, never inferred: an inline intent never accepts a
  projection, and a reference intent never accepts inline data. A reference
  intent has no call until it is rebuilt (`Verified::rebuild`), and then
  exactly the call its inline original would have had.
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
  digests, and the plan and consent digests. A reference intent's binding also
  names its form, and its source, snapshot and consent digests are those of
  the projection. Its plan, and with it the frozen input digest, is that of
  its inline original.
- **Receipt.** The phases are Queued, Reserved, Running, Recorded, Failed and
  Unknown. Recorded carries the closed outcome: the request, mode, dispatch
  and a `queued` or `note` state. That is the exact CLI metadata outcome,
  never native delivery or consumption. Only the exact original's drained
  Recorded or Failed receipt settles; Queued, Running and Unknown never do.
- **Lookup.** A scoped read-only lookup of the original by its plan
  fingerprint. A missing result never authorizes anything.
