# Original writer-domain bootstrap

Approved `zerus-jmxyf.7` prerequisite, `zerus-jmxyf.7.2.10`, based on main
`06e978973` and REV2 SHA-256
`592bd8393057e723f6655fa735ef7c0d24df7a605e1dc20b9191e44d55b2adb9`.
The coordinator approved this design before implementation. The separate
layout-transition refusal in `zerus-jmxyf.7.2.9.1` goes first.

## Why this comes next

The owned supervisor retains the actual original child, native source and
read-only peer channel behind `supervisor::ORIGINAL_OWNER = false`.
`writer_records::admission::OriginalRequest::capture` already requires an
existing original run store, but `supervisor::owned::spawn` never initializes
one. A status query cannot supply that missing original creation authority.

Choose original writer/IO admission before adding stop/resume authority. Its
first mergeable part is to let that fresh original supervisor create and retain
its own writer domain. This closes the original-host initialization gap; it
**does not** close actual mutator enrollment or physical IO custody. Connecting
ordinary sends, queued input, settings, answers and the remaining writers is the
following work. The typed marker's production freshness mint stays absent.

## Authority and dormant boundary

Only the fixed fresh owned-launch program may create the domain for its exact
canonical session/run. Keep the production gate const-false, without a setting,
environment override, installed command or new peer verb. With the gate off,
ordinary launch code and all existing effects remain unchanged.

In the owned branch, retain the existing durable `original_owner` creation
reservation before the first domain write or possible native child creation.
That binding carries the original launch nonce and source correlation. Never
clear it on failure. An existing `original_owner` or existing/partial run
domain refuses; no lookup, reconnect, process absence, empty ledger, old result
or new request UUID can initialize, reset or adopt it. Preserve the current
relaunch/fallback/physical-spawn refusal boundaries.

Introduce a private, non-cloneable original domain resource produced only by
successful exclusive creation under that original writer/source. Retain the
original native directory/file identities in the actual supervisor owner.
Consume the live creation source's single attempt even on refusal; restoring
its configuration cannot replay that attempt.
Serialized IDs, native identities and peer observations remain data; they
cannot recreate the resource or authorize input, signal, archive or cleanup.
Drop closes owned handles and never deletes or repairs domain files.

The existing peer remains read-only and bounded. If it exposes domain
correlation, explicitly label coverage as initialized but not enrolled. An empty
pending list and in-process `physically_idle()` never mean that all real
writers/IO are settled. Do not promote them into readiness, completion,
Terminate permission or descendant-closure evidence.

## Creation order and bounds

Use the supplied fresh state writer and original native state/configuration/
image/owner pins. Validate session/run, source and transition exclusions before
the first write and at publication boundaries. Any `terminating`, `pausing` or
`layout_transition` evidence is a refusal, not a recovery request.

Create only the exact `terminating-writers/<session>/<run>` namespace. Anchor
creation below the retained native root and verify the configured route; use
no-follow descriptor-relative parents and exclusive new leaves. Do not chmod,
overwrite or follow an existing scope object, sweep unrelated runs, repair
rename/worktree state, or fall back to an ambient path. Harden/reuse the current
initializer through this original-owner boundary rather than adding a second
store. Retain its bounded ledger format and row count.

This is fresh creation, not acquisition of another run's exclusion: never wait
for an existing run gate while holding the global writer. Any contention on an
unexpected pre-existing object refuses. Later actual writers and the marker
factory retain their existing run-before-state-writer order.

Capture one monotonic creation deadline, at most 30 seconds, immediately at
`owned::spawn` entry, before source capture and the bounded existing-writer wait.
The current static spawn signature carries no caller deadline. The coordinator
approved this allocation on 2026-11-15; existing provider startup bounds stay
unchanged. Do not renew the creation deadline between phases.
Require positive file/directory durability and original-object readback before
returning the resource or starting the native child. A late/uncertain write
keeps the original reservation and any partial namespace, returns no successful
domain and never replays creation. A pre/post deadline check does not bound a
kernel fsync; physical persistence/helper custody remains an explicit later
host gate, not a claim made by this resource.

If the native child or its final publication fails after domain creation,
retain the same domain and original creation reservation. Record only the
existing known exit/unconfirmed result; do not manufacture a new domain or
consume generic custody `Complete` as effect authority. Preserve the fixed
continuous subreaper/non-auto-reap policy and the Mac descendant-coverage gap.

## Controls and review

Start with a compiled refusal stub and a private owned-launch positive runtime
RED. Use private state/tmux/fixtures only, including a private const-true test
image; never enable or replace an installed image.

- A fresh original creates exactly one native domain before child execution;
  the owner retains the same resource and emits read-only data only.
- Existing, partial, malformed, linked, special or oversized scope objects
  refuse without reset, second creation or unrelated namespace changes.
- Replace root/configuration/image/writer or retarget a directory alias at
  controlled boundaries. Refuse without adopting the replacement.
- Inject create, write, file/directory sync and readback failures. Keep the
  original reservation/partial data and prove zero child creation before a
  positive domain result. Preserve no duplicate root on later fallback.
- Drop, owner death, status and reconnect cannot initialize, clear, adopt or
  mint a resource. Empty/pending/corrupt ledger observations grant no effects.
- Keep the unmodified default launch controls for latest official Claude,
  Codex and Kimi. Qualify the owned private path and native filesystem controls
  on Arch and the private headless Mac, plus local Windows/Darwin clippy,
  client fmt and source/privacy checks. Mutate existing-scope refusal, original
  source recheck and false durability; require runtime failures and restoration.

Independent G1 is required before merge because this connects durable support
to the real owned supervisor, even while dormant. No stop/signal authority or
activation is proposed. Real writer enrollment/IO/crash custody, native request
and consent admission, marker production, controller-death resume custody,
provider grace/state-resume qualification and cohort-aware finalization remain
open before an installed graceful Terminate can be enabled. Force and
removal-Unknown remain separate.
