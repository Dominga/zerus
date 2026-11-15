# Original startup writer enrollment

Design proposal for `zerus-jmxyf.7.2.11`, after #2448, based on main
`f821172ee` and REV2 SHA-256
`592bd8393057e723f6655fa735ef7c0d24df7a605e1dc20b9191e44d55b2adb9`.
Coordinator design approval precedes implementation; independent G1 precedes merge.

## Choice and first slice

Choose actual writer/IO enrollment before controller-death stop/resume custody.
The original supervisor now owns a native writer domain, but its observation
correctly says `initialized_not_enrolled`. Neither a free gate nor the
process-wide `physically_idle()` counter proves that every writer of this run
has drained. Death custody cannot supply that missing enrollment, and adding
agent stop/resume effects would require separate authority and qualification.

The first mergeable slice enrolls the **fixed Claude startup appearance helper
family** in that live original domain. It is small enough to establish one real
end-to-end adapter: this family already has closed commands, original native
source checks, an existing single-use Enter permit, actual Child/pipe/join
custody and a producer-completion witness. It needs no new peer verb or broad
backend callback. The same approach can then cover Kimi's one-shot prompt and
the ordinary mutator inventory, without claiming those are enrolled today.

## Current source evidence

- `supervisor/owned.rs` retains the original native source and the newly created
  domain. Its `startup` flag and the Core counter are not a per-run writer census.
- `claude_theme/executor.rs` selects a fixed native facade. The helper forms in
  `claude_theme/helper.rs` are Capture, Pane, Layout, Process and Enter. The host
  consumes its existing Enter permit and records possible delivery before spawn.
- `claude_theme/helper.rs::run` already reserves Core admission before the
  actual Child exists and retains its source through actual wait, input closure,
  output EOF and joins, including retirement after cancellation or Drop.
  It does not reserve a durable row in the original run's writer store.
- `writer_records/admission.rs` and `io.rs` pair original source, pending row
  and helper custody, but no actual startup producer uses that pairing.
  Those types explicitly do not authorize input or establish crash settlement.
- `kimi_launch.rs::wait` spends the original prompt once, reserves its existing
  durable launch state and calls `input::submit`. That broader submit adapter
  and all external mutators stay outside this first PR.

## Authority and dormant boundary

Keep `supervisor::ORIGINAL_OWNER` const-false with no environment, setting,
installed command or renderer override. Select the enrolled adapter only from
the fixed owned-supervisor startup path. The ordinary legacy path keeps its
existing helpers, keys, bytes, receipts, deadlines and fallback behavior.

Derive enrollment from the actual retained original owner/domain and native
source, not from `original_owner` JSON, a status response, PID absence, copied
source hashes or an empty store. Do not expose the raw agent Child, accept an
arbitrary executable/argv callback, or add a general owner-channel operation.
An observation or reconnect cannot recreate or adopt a live enrollment.

The existing appearance Scope remains the authority for each exact command and
the one Enter. Enrollment is an additional restriction on an already admitted
operation. It grants no new signal, input, marker, Force, archive or cleanup
permission. Existing Core cancellation remains limited to its same owned helper;
this PR adds no agent, group or descendant stop operation.

## Enrollment and physical lifetime

For every existing helper invocation in this family:

1. Validate the live original owner/domain and that the fixed startup producer
   is still open. Use the host's current native source, selected record, target,
   request and existing Scope checks. Keep initialization absent from this path.
2. Under the **same already-held** state writer, reserve the exact run's durable
   row before `source.offer`, possible-delivery publication or physical spawn.
   A helper row has its own canonical UUID and binds the parent appearance
   request, closed command kind, original SID/run/source and intent digest.
   The UUID is accounting, never authority to repeat Enter or retry an unknown.
3. Reuse the existing original Child and pipe ownership through the paired IO
   bridge. Keep source and native lease in the same Core charge through positive
   wait, stdin closure/join and both real output EOFs/reader joins. A helper exit,
   successful write, timeout, cap, error or free lock is not physical settlement.
4. Retire only this same row after its original IO has physically settled and
   all its borrowed custody has returned. Preserve delivery/appearance outcomes
   separately. An unknown or failed native operation never gains replay from
   physical retirement, a different helper UUID or a later source check.

Use fresh per-helper source capture under the fixed native owner, so legitimate
original completion metadata between helpers does not become a stale source
snapshot or an adopted replacement. Never relax the immutable run/native target
checks merely to accept startup progress. If a change cannot be proven to be
the original run's permitted progress, refuse that helper and preserve custody.

The marker/controller's exclusive run gate still precedes the state writer.
Ordinary enrollment uses only the existing **nonblocking shared-gate attempt**
under its supplied writer. Never wait for a run gate while retaining that writer,
and never acquire a second global writer, input gate or unrelated recovery path.

Retain the original helper/Scope deadline, capped by the existing custody limit;
no phase or retirement callback renews its effect budget. A bounded later row
retirement is accounting only. Physical source/persistence IO bounds and
cross-process inherited custody remain explicit activation debt.

Closing the startup producer is a retained program/resource transition, not an
empty-list observation. No late capture may admit another helper after closure.
Dropped or unsettled helpers keep their pending row and actual resources; owner
death never reconstructs them or treats PID/lock absence as completion.

## Coverage and remaining prerequisites

Report at most that this one fixed producer is enrolled and closed/draining.
Keep the global domain coverage `initialized_not_enrolled` and effect authority
false. No `all_writers_drained`, Terminating readiness or cleanup capability is
derived from this partial family, its Core counter, a row count or a root exit.

Kimi prompt, ordinary/queued/scheduled send, Terminal Input/Clear, compact,
pause/resume/rename/fork/settings/answers, supervisor retry and other record
writers still need their own native admission and full IO census. Controller-
death stop/resume custody, continuous descendant/source proof (including the
Mac gap), physical bounds, typed marker production, provider grace/state-resume
and finalization remain separate gates before graceful Terminate can activate.

## Controls and review

Start with an actual private const-true original-supervisor RED: hold an exact
appearance helper's original output EOF or worker completion after its Child
exits, and observe that today's real producer has no durable run row. The fixed
version must keep the same row/source/lease and forbid a closed/drained claim
until real settlement. Release only that owned fixture and observe exact
retirement, without interpreting it as native delivery success.

Cover pre-spawn row refusal (zero helper/Enter), malformed or missing domain,
actual run/source/writer replacement, cancellation/expiry/Drop with held IO,
success and failure after possible Enter, closure racing a late helper, duplicate
helper IDs, old receipts and unknown-result no replay. Keep original completion
hooks progressing and unmarked/gate-off launches unchanged. Mutants remove the
durable enrollment, release before EOF/join, admit after producer closure and
permit replay after unknown; each needs a runtime failure and restoration.

Use private state/tmux/fixtures only. Local Arch and private headless Mac runs
include latest official agents, the native IO controls, touched module tests,
Root/Core clippy with warnings denied, local Windows/Darwin cross-clippy as
applicable, both client fmt checks and source/privacy checks. Use short `/tmp`
fixture paths on the Mac. Follow its one-step lease and immediate target cleanup
protocol. No hosted CI dispatch/polling gate or owner runtime operations.
