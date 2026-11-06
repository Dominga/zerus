# Service-owned creation clone

The first creation effect owned by the desktop actor is a local POSIX repository
clone. Its exact help marker is `Desktop creation clone ABI: desktop-creation-clone-v1`. The selected paired relay must also advertise the
frontend-request ABI. Windows does not advertise this executor.

Only native consent in the current creation host sends a fresh intent,
consumed once after the existing creation journal write barrier: a Submit,
direct or from the task executor's Start (the same New session button, with a
reserved identity), or the clone step of an explicit Continue setup's chain
(see [Continue](#continue)). The closed `CreationClone` request carries the
exact checkpoint, its revision, creation and clone UUIDs, and current
configuration/service epoch. Native cached Service mode and the original relay
child authenticate the request. Caller JSON, readiness, notification ownership
and imported journals cannot grant execution consent. The existing task-start
binding and storage remain independent.

The actor rechecks the opaque native permit after `BEGIN IMMEDIATE`, then checks
the actual writer epoch, local native machine source, original service owner and
handoff exclusion. It durably retains the immutable source, fixed plan and consent
binding. One live consent value belongs to that original admission; restart never
reconstructs it. Once admitted, that fixed clone may continue after the GUI exits.
A restarted queued original becomes Unknown and is never automatically executed.

One worker prepares the original inherited-lock custody, which is committed with
its exact ticket before execution. Each new step—receipt reservation, destination
creation, Git clone and checkout verification—requires a fresh actor transaction
checking the same current lease, consent, ticket and next step. Revocation or
source/exclusion replacement cancels further steps. Cancellation closes the
original Git deadline owner's stdin; its stdout, actual exit and watcher remain
owned. The Git worker and descendants inherit only the exact close-only custody
lock in addition to the existing anchored checkout descriptors.

A repository result cannot advance the GUI until a read-only worker acquires the
original custody lock and the actor commits settlement while still holding it.
The exact request, plan fingerprint, destination, name and terminal receipt must
match. A passive actor can observe the original receipt and custody after restart;
it cannot start the clone again. A missing, malformed, replaced or held resource
stays Unknown/retained. Process names and absent PIDs are never drain evidence.
Takeover, revoke-hold, notification reattachment and legacy writer checks include
this kind's undrained resources.

`creation_clone_result` is a bounded indexed read with the original creation/clone
IDs and plan fingerprint, bound to a fresh configuration/service epoch. A missing
result never authorizes a resend. A lost mutation reply, reconnect, Check or
recovered journal uses only this lookup. Its reader may stop when the GUI quits;
that does not cancel the admitted service job.

A completed probe that finds no clone kind on the selected service sends the
clone and its status reads down the unchanged direct path, since nothing of
this kind reached that service; a probe that fails is no such proof. Otherwise
the native host never falls back to the legacy clone worker in service mode.
The reducer emits a clone only for a request ID it reserved and persisted in
this process, so a clone without fresh consent, or one stopped before its
admission request is written, was never sent. It fails as definitely
effect-free (`effects_started: false`), its UUID is retired into
`clone_previous`, and the creation stays resumable instead of Unknown.

## Continue

One explicit Continue setup in the current native host authorizes the
remaining clone, worktree and launch steps of that creation and its reviewed
plan, in order (zerus-5wmh.66.4.3.9). The chain is one process-local original
(`Chain` in `client/src-tauri/src/creations.rs`), never a stored consent flag
or a renderer-supplied value. Only a Continue that the reducer applies starts
one. The reducer requires definite clone and worktree predecessors and no
dispatched launch, so an Unknown original stays lookup-only and never resends
or rotates its ID.
- **Binding.** Each step consumes its own binding before its intent: the
  kind, the step ID (a clone or worktree request reserved after the Continue,
  or the launch ID reserved before it), a later revision and the reviewed
  plan's fixed choices (`host`, `machine`, `cmd`, `tag`, `account`, `model`,
  `effort`, `source`, `url`, `repository_name`, `branch`, `terminal`,
  `swarm`, `auto`, `repository`). Each intent still needs fresh native proof
  and current lease and service checks after `BEGIN`. An earlier step's
  success is not a permit and does not extend a proof deadline, and each step
  uses the service that is current when it runs.
- **Order.** The chain moves forward only after the preceding consumed step's
  definite success (`clone_done`, `worktree_done`). Any mismatch discards the
  remainder.
- **Stopping.** Failed, Unknown, Stop, a stopped or recovering creation, a
  changed plan, source or identity, or a lost host ends the chain. A Submit's
  unused consent ends the same way. Restart, recovery, import, reconnect and a
  replayed Continue cannot reconstruct it, and plan edits create a new
  creation that no old Continue covers.

## Limits and tests

This independent ledger retains at most 4096 IDs and 16 MiB of reserved storage,
with at most 32 unsettled resource charges and one effect worker. The original
checkpoint is at most 32 KiB, reply at most 24 KiB and repository result at most
16 KiB; the existing transport control-line cap also applies. Capacity refuses
atomically, and terminal IDs are not evicted to permit another effect. Status is
display-only and reports task-start and creation-clone totals separately.

Private actor tests use an explicit synthetic native/source seam, a real
inherited-lock helper/descendant, revoke between steps and restart recovery.
Transport tests use the real native cache and original Child binding against a
private fake service. Existing repository CLI tests exercise actual Git in
private profiles. These controls are separate evidence; they do not establish
installed canonical frontend authority or full creation lifecycle acceptance.

Repository workers must never re-execute a unit-test image. Libtest interprets
worker arguments such as `clone` as test filters instead of entering the CLI
dispatcher; re-execution can therefore recurse. Unit images now refuse the
worker entry globally, independent of any individual test's guard. Production
also rejects nested worker contexts and a Git executable matching its own image.
The same shared guard covers other production self-exec entry points, including
detached voice/recovery and worktree/mobile transport helpers. Check it before
service-manager activation as well as process creation. Reading an image identity
or recording an installed CLI path is not self-exec and remains available to
model tests; explicit exact-filter test subprocesses remain separate.

Run compiled repository-worker tests and mutation probes on Linux through
`python3 tests/bounded_process.py --seconds 60 --max-processes 32 -- TEST_BINARY FILTER`.
Compile separately. This private subreaper retains kernel pidfds for its own
descendants, including new sessions and orphaned grandchildren, freezes parents
before cleanup, kills and reaps the original subtree, and fails on timeout or
process-budget overflow. It never selects processes by name, path or shared
process group. CLI fixtures additionally require actual CLI help, so a libtest
image cannot be selected as `HGS_TEST_BIN`. An interrupted test's parent exiting
is not evidence that its descendants were cleaned up.
Temporary signal permission failures retain the original pidfd and are retried
within the same cleanup deadline. If cleanup remains unconfirmed, the fixture
fails with a bounded PID/birth record; permission denial never counts as exit.

Worktree creation, placement, launch and remote creation remain separate leaves
of the creation migration; the worktree leaf is specified in
[desktop-creation-worktree.md](desktop-creation-worktree.md). The
desktop-service switch remains default-off.
