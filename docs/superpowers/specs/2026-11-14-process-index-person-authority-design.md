# Process-indexed callers and verified desktop Person controls

Status: **proposal; implementation awaits the coordinator's go**, 2026-11-14.
Tracking: `zerus-j43fm`, `zerus-cg5h.4.11`. Source baseline: `01a65151e`.
This note enables no writer, caller policy, desktop control or task-control gate.

## Two independent proofs

The process index answers whether a command descends from an agent or resident
host recorded in the selected state. It does not prove a human gesture. A complete
negative is an **outside-agent candidate**, never verified Person. Unregistered
agents, other state roots and reparented children remain outside that inference;
arbitrary same-UID modification of executable/state is outside this boundary.

Current `tasks::actor` uses m7b08's readable-ancestor-marker refusal; withheld
ancestor environments are tolerated for ordinary tasks. Teams' separate interim
proof refuses some unreadable boundaries. Both remain unchanged in the first
slice. They must not be renamed to verified Person or silently interchanged.
Ordinary Terminal.app `hgs task add/list/start` keeps its current person-facing
behavior. Privileged Person-only actions migrate individually to positive native
approval, rather than making every ordinary terminal command require a dialog.

## Index and lookup

Use a dedicated, local, versioned SQLite projection under the selected state
root, isolated from the optional graph/ID caches. Existing `session_index` is
best-effort and can fall back to a scan; neither behavior is suitable for this
admission. Records and native OS identity remain authoritative.

A binding entry contains `machine/configuration`, index generation, OS boot
identity, **pid, precise birth, session_id, run_id, conversation_id,
published_at**, role, canonical source kind/key, binding revision and binding
fingerprint. `published_at` is diagnostic, not a lease or freshness proof.
Session names are labels. A not-yet-confirmed conversation is explicitly absent;
confirmation updates that same binding, never adopts another run.

Separate a process anchor from its session edges:

- One bounded anchor per `(boot, pid)` stores its precise birth, role, verification
  state and one qualified source locator (at most 4 KiB). Roles distinguish agent,
  correlated supervisor and resident host; no row contains a session vector.
- Indexed edges bind `(boot, pid, birth, session_id, run, conversation, role)` to
  the canonical record. Shared-host edges are paged metadata; they cannot each
  impersonate ownership of the host process.
- A host anchor alone establishes “inside an agent host.” Exact DeepSeek session
  attribution still needs `caller_hosted`'s native peer/per-call proof. No first
  hosted row, inherited host marker or parent session supplies that identity.

Resolve ancestry once, at most existing `ANCESTRY_STEPS` (64), with **one indexed
anchor lookup per ancestor, at most 64**, independent of saved-session count.
Compare births, verify the bounded canonical source and recheck the native chain
within one absolute deadline. Bracket the reads with the coverage generation and
binding-change revision; a change or pending intent returns unavailable without
an internal second walk. Exact claimed-session lookup adds one fixed point read.
No foreground inventory scan, repair or hosted-session enumeration is allowed.

Outcomes are proven agent/host, complete outside-agent candidate, and unavailable.
Conflicting sources, incomplete coverage, unreadable process identity, namespace
mismatch, ancestry overflow or a changing chain are unavailable, never a negative
proof. Unclaimed ancestor evidence refuses acting as Person; it does not silently
adopt that session's identity. Keep existing DeepSeek-specific refusals first.

## Writer coverage and publication

Every authoritative **process-binding** write participates, including routes that
bypass `storage::write`. Activity-only updates need not rewrite a stable binding.
Before enforcement, audit and test this inventory against actual write sites:

| Writer family | Required publication |
| --- | --- |
| `storage::write/atomic_record`, supervisor spawn/finish, Windows session-host callbacks, hooks and lifecycle paths | Native Claude/Codex/Kimi agent and validated supervisor births; startup, first conversation, clear, resume, rename, pause, exit and removal. |
| Fresh-ID publication, legacy mirrors/routes and owner transfer | Preserve session UUID and exact run/birth; publish the canonical source, not rename shadows or duplicate mirror claims. |
| `provider_binding::Provider::save/remove` | Both DeepSeek and OpenCode, in label and ID layouts; generic rename/forget paths included. |
| `dsh::host_identity` and its launch/ready records | A host anchor before accepting hosted work; starting/unconfirmed launch remains explicit. Retain a live host anchor even with zero session edges. |
| DeepSeek hosted bindings / `shell_identity` | Exact host-to-session edges after every run/conversation binding change. Shell identity files remain claims, not Person proof. |
| OpenCode service registration and bindings | Independently qualified resident service anchor plus session edges; a service-file PID alone is not qualified ownership. |
| Teams member launch/recovery and native subagent records | The same underlying writers and native pins; a Teams/replica membership document alone publishes no live process. |
| Archive/restore, direct canonical writes and migrations | Retire/replace only the exact previous binding; saved records without a live pin grant no live entry. |

New launches need a publication/ready fence before agent or host tool work can
run; an after-exec best-effort note leaves a gap. Qualify the owned launch
handshake on each platform without restarting existing agents. Supervisor entries
require the actual agent/parent correlation, not a generic desktop/SSH ancestor.

Under the existing source lock: durably reserve a change intent, publish the
source, then transactionally publish edges/anchors and settle the intent. If intent
durability fails, refuse the new binding mutation/launch before publication. Preserve
source-lock order; never acquire source locks inside an index transaction. A crash
at either publication cut leaves unaccounted coverage until exact reconciliation;
an index failure cannot certify a miss, undo the source or discard prior claims.
**Advisory rollout clarification, coordinator decision 2026-11-14:** the
pre-publication refusal above applies only to a later, separately reviewed slice
in which a production decision relies on this index. During advisory publishing,
an unavailable, full or failing index never blocks or fails a canonical write,
launch, spawn or lifecycle outcome. Reserve the immutable intent before source
publication when possible. If it fails, continue the original source operation
and mark that source's coverage lost; persist a lost-since epoch/time marker when
possible. Otherwise a missing/mismatched canonical publication witness or an
indexed source whose file has disappeared must make the next bounded bootstrap
report an unaccounted gap. Post-file settlement failure retains pending/incomplete
coverage and preserves the original source result. No index error permits an
undo or replay of canonical I/O. The enforcement outage contract is not activated
by this advisory slice.

JSON and SQLite are not one atomic transaction. Deletion uses binding-version CAS.
Busy/corrupt/moved/future-schema indexes return unavailable without foreground
repair. Capacity cannot evict live or uncertain claims.

## Bounded bootstrap, staleness and rollout

Build a new generation in background pages, proposed limits **256 directory
entries, 2 MiB and 50 ms per step**; a deadline may end a step sooner. Stream held
no-follow directory iterators instead of sorting/materializing a whole directory.
Retain journal changes after the scan's high-water mark until every building
generation passes them; replay them in indexed pages. Capacity leaves coverage
incomplete rather than dropping deltas. After a crash, restart the incomplete
scan; never mark unseen sources absent. Complete requires every source family
and intervening change accounted for. Status uses indexed byte-bound pages.

Coverage requires a supported writer floor. Older loaded writers and unaccounted
source changes cannot be assumed to participate. File notifications are hints,
not completeness proof. Before enforcing negative results, qualify a writer-floor
and unaccounted-change protocol for every source family. Until then the index is
shadow/positive evidence only and existing interim policies remain explicit.
There is no automatic legacy fallback that promotes Unknown to verified Person,
and no forced agent/host restart to establish coverage.

Recheck actual liveness/birth and current canonical binding on a hit. A PID reused
with another precise birth is not the recorded agent; stale rows queue bounded
background CAS retirement. Never infer expiry from `published_at`, phase or age.
Linux birth is kernel start ticks plus boot; macOS uses native start seconds and
microseconds; Windows uses held-process creation FILETIME. Compatible Unix
`process_start` text is coarser and cannot be upgraded into a launch-time precise
pin by sampling a possibly reused PID. Bootstrap legacy entries only through an
independently qualified original binding; otherwise retain an unverified entry
or incomplete coverage, with an explicit diagnostic.

## Platforms and remote calls

On macOS reuse native PID/PPID/birth inspection (`PROC_PIDTBSDINFO`), not environment
readability. Terminal.app's restrict-flagged zsh may withhold its environment and
still be an outside-agent candidate. Linux retains the reviewed namespace/UID
admission and birth-pinned ancestry; a sandboxed partial tree proves no absence.

Windows needs held process handles, creation times and checked parent correlation.
Avoid today's repeated process-table snapshot inside each parent lookup: use one
bounded snapshot per admission or a qualified point-parent reader. First-slice
portable storage/types do not change the existing Windows caller path; enforcement
waits for native Windows controls, including handle exit/reuse and restricted-token
cases. Cross Clippy alone does not qualify that path.

For `hgs @host`, the proof and index run **where the state lives**. Forward no
`HGS_*` authority and perform no local session inventory to answer for the peer.
SSH ancestry/account identifies the receiving command, not its originating human.
A remote Person-only control therefore needs receiver-verified native approval;
missing forwarded markers or an “outside” SSH shell cannot grant it. Initially
approve on that computer's trusted Zerus surface. A cross-computer frontend issuer
would need separately reviewed enrollment and transport binding; an SSH alias is
not enrollment. This design introduces no phone writer.

## cg5h.4.11: a gesture bound to one original control

Proposed entry point: an explicit **Pause task** or **Resume task** action in the
selected task/attempt view, backed by the authenticated original native frontend
lane (`desktop::ui_authority`), followed by native confirmation, default Cancel.
Show computer, task, attempt/generation, executor/run, action and the exact observed
Pause/Stop IDs selected for crossing. Renderer JSON and opening the view grant
nothing. The concrete presentation still needs the bead's designated design
approval, independent review and coordinator Arch/Mac preview before activation.

After confirmation, the native custodian mints one short-lived approval bound to
configuration/store, original frontend connection/image, task/attempt/generation,
executor/native turn, adapter guard, observed hold IDs, original request UUID and
plan digest. Consume it durably with that request before effects; freshly recheck
permissions, custody and unknown holds. The CLI Executor bridge cannot mint
Person. A copied handle, `person:true`, same UID or ancestry is insufficient. This
uses the trusted-native-frontend boundary; it claims no defense against same-UID
instrumentation or proof of physical input from ancestry.

Cancellation/expiry permits no effect; consumption is never reconstructed after
restart. Duplicate execute/status reads the original outcome. Unknown delivery
keeps the original holds and cannot authorize a fresh approval/replay. New/unseen
Stop IDs survive. Task Pause never becomes session-wide interruption or cleanup
of jobs/processes. The const-false task-control gate, no automatic resume and the
existing [.4.7 controls contract](2026-11-08-task-controls-design.md) remain intact.

## First mergeable slice and later gates

**Slice 1: inactive index foundation only.** Add the versioned entry/anchor model,
local store, bounded point/page APIs, change-intent/coverage state machine and
synthetic private-store controls. No production writer, startup rebuild, caller
classification, native Person route, marker or gate is connected. Tests/mutants
must cover precise PID reuse, boot/source/configuration mismatch, shared-host
fan-out without enumeration, partial generations, publication crash cuts, changed
bindings, capacity/lock failure and EXPLAIN of exact indexed lookups. Benchmark
10k/100k synthetic records and a 64-ancestor chain; record p50/p95, rows visited and
source reads. The admission work must remain independent of saved-session count.

Subsequent separately reviewed slices wire every publisher plus bounded bootstrap
and writer-floor coverage; replace foreground record discovery with qualified
lookups; then add the concrete native Person plan/confirm/execute surface. Only
after complete source coverage, Mac withheld-environment and Windows runtime
qualification, remote refusal controls and the required Arch/Mac preview may each
privileged action retire its interim Person rule. No slice infers Person from an
index miss, enables task controls by changing a constant alone, or claims to catch
a reparented unregistered child. Implementation of Slice 1 awaits the coordinator's
go on this proposal.
