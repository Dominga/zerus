# Dormant typed Terminating reservation

Proposed next slice of `zerus-jmxyf.7`, based on main `4b16a2674` and REV2
SHA-256 `592bd8393057e723f6655fa735ef7c0d24df7a605e1dc20b9191e44d55b2adb9`.
This is the design note before implementation. Independent G1 is required
before merge. The production owner gate remains false.

Main already has presence-based mutation/archive fences, dormant per-run
writer records and native exclusion, original source/IO resource composition,
and the retained original supervisor from #2414. It has no typed durable
Terminating reservation. The existing Terminate path still performs its legacy
stop/archive operation, and owned-mode records refuse that path.

This slice adds only an unused typed reservation factory and data lookup under
`state::terminating` (new `terminating/writer_records/marker.rs`, plus a minimal borrow seam
in the existing dormant run-gate module). No installed command, supervisor, action dispatcher,
answer channel or controller calls the factory. It neither initializes writer
storage nor enables the owned launch route. No signal, terminal input,
temporary stop, finalizer, archive removal, worktree cleanup or clearing API is
added. The factory creates a fence resource; it does not grant permission to
perform Terminate. `zerus-do7mu` and formatting bead `zerus-jv69k` stay separate.

## Original reservation and stored data

Use a versioned envelope in the existing record's `terminating` field, with
strict field/length validation and a bounded canonical representation. It
contains the original canonical non-nil request UUID and intent digest, exact
session/run/conversation binding, provider, immutable original root and
supervisor correlation, native terminal/pane correlation, source/configuration
correlation and the original reservation owner's PID/native birth. The first
phase is only `reserved`; future grace/stop/removal outcomes are not introduced.
PID, birth, image, terminal and configuration values in this envelope remain
correlation data, never newly reconstructed native authority.

The caller captures one original request and selected binding. After acquiring
the existing original run exclusion **before** the normal state writer, the
factory rechecks that captured source against the fresh record under that same
writer. It refuses changed identity, unconfirmed startup/resume transition,
unsupported provider, damaged source, pending writer rows or an existing
`terminating` field of any shape. It never adopts a new run, conversation,
configuration root or record object. Only its actual new publication can return
the non-cloneable local reservation resource. Saved data and status lookup
cannot recreate that resource.

The existing `RunWriterGate` supplies run/state-lock exclusion. Add only a
borrow-only internal seam for the factory's same-writer validation and
publication; do not clone explicitly unlocking guards. Its empty ledger is
**not** proof that all real writers are enrolled or physically settled. Those
host obligations remain unmet, so no installed controller may use this factory
yet. The original freshness mint exists only in the test build; there is no
production constructor or conversion from saved data. Original source/IO settlement tokens and generic custody `Complete` also
remain data, never Terminate or cleanup permission.

## Durability, failure and lifetime

Bound the marker to 32 KiB and each selected record read to 1 MiB (stricter than
the generic record reader). Use the existing guarded authoritative record publication with native
file/directory durability barriers. Preserve the current legacy/ID binding behavior; do not
introduce a second marker store or bypass normal record publication/indexing.
The guarded publication retains the normal session-ID index and legacy mirror
updates. As with its existing recovery callers, it conservatively leaves native
process-companion/SQLite coverage stale until ordinary reconciliation; that
cache never becomes removal authority.
After publication, verify the exact reserved envelope and unchanged immutable
binding before returning a durable reservation. Completion/attention metadata
outside that immutable projection may still advance.

Use one captured monotonic deadline, at most the original 30 seconds, and bound
lock acquisition by its remaining time. There is no serialized expiry that
unfences a session and no renewed budget at a later stage. Blocking persistence
must remain inside the future host's original physically owned helper/IO
custody; a pre/post deadline check is not a bound on a kernel `fsync`. This
slice does not install that host adapter or claim its settlement. A late write,
uncertain sync/readback, interrupted owner or lost resource yields no successful
reservation result and never licenses another attempt. No fresh UUID, automatic
repair, timeout deletion or rollback of a possibly published marker follows.

Drop releases only owned native resources. It never edits or removes the
marker. Releasing the normal writer retains the same original run exclusion,
so the future owner can let original-run completion hooks progress without
holding the global writer during a wait. No effect or finalization API is
exposed through that retained resource.

Lookup reports absent, recognized stored data, or unrecognized/unreadable data.
All are observations. An exact repeat is existing data, never a new live
owner; a wrong/stale request refuses. `terminating::pending` keeps its current
Unix presence rule, including null, false and damaged markers. Windows keeps
its existing behavior. No lookup can turn missing data or an old receipt into
fresh consent or an original request.

## Controls and delivery

For the new API, start with a refusal-only stub so the first positive control
compiles and fails at its runtime reservation assertion; a missing-symbol
compiler error is not the RED evidence. Then add deterministic controls:

- Fresh original publication is visible only after file/directory sync and
  exact readback. Inject failure at write, rename, directory sync and readback;
  retain any published marker and never mint a second original resource.
- Existing, malformed, conflicting or stale markers cannot mint ownership.
  Replace the selected root, record or state lock; change run/conversation,
  source or request; expire/contend the original deadline. All refuse without
  adopting the replacement or renewing time.
- Hold an actual original writer/IO loan and pending row: reservation cannot
  pass by seeing a free lock, helper exit or timeout. Once the qualifying
  private original settles, an unchanged binding can reserve; a binding-changing
  completion during that wait refuses. Preserve completion hooks and all
  existing mutation/archive fences after the typed marker is stored.
- Drop, owner exit and status reexecution leave the fence as data. No saved
  marker can construct an owner, clear itself, call a native effect or finalize
  an archive. Guard-removal and false-durability mutants must fail the controls.

Use isolated Root/Rust and actual-CLI fixtures, local three-platform clippy,
both client fmt checks and source/privacy checks. Qualify the relevant native
compatibility controls on Arch and the private headless Mac with the latest
official agents; no owner sessions or hosted CI gate. Production/unmarked
launch and mutation behavior must remain unchanged.

Before any later controller connection: enroll the actual mutation/answer
writers and retain their complete physical IO/crash custody; admit the original
durable native request/consent and target; complete descendant/crash coverage,
provider grace/state-resume qualification and cohort-aware finalization. Only
then can a separately reviewed controller publish this marker before its first
possible graceful effect. Force and removal-Unknown remain separate contracts.
