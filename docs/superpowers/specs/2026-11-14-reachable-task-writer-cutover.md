# Reachable task-writer cutover

2026-11-14, zerus-cg5h.4.13. Owner-directed availability change, with source
G1 by `g1-2317-cx`. Computers are optional: an absent, unverified or timed-out
writer is an exact `not_cut_over` fact, never a required acknowledgement.
TASK_CONTROLS remains false. Native request identities and no-replay rules do
not change.

## Root and causal admission

The migration root of an existing full-history task is its actual unique first
schema-7 `meta.floor` operation F and its causal prefix at F's operation counter.
`floor_matching` saves this C2-qualified floor before C3, so a task refused only
because a writer was absent can have this root without ever having received a
certificate. G1 accepted that migration rule in principle on 2026-11-14 under
the existing cooperative floor-aware/C2-constructor contract. It is not
cryptographic origin proof against arbitrary hostile CRDT authors.

`cutover_root` checks the original task/project/genesis, F's self-ID and content,
and every historical floor write. A replacement, conflicting/malformed floor,
missing causal closure or shallow history refuses the affected task. The root
is never the current version, current LWW floor, `.managed` mark, schema repair
or a relay's claimed baseline. Historical F can occur inside a coalesced Loro
change: the prefix is derived at the operation, including implicit same-peer
causal order. Old floors need no new commit message.

`cutover` checks **every** span after that intrinsic prefix, including spans
already merged by a relay. Every span must causally descend from F. Thus
`F + offline concurrent edit + later repair` cannot be laundered into a new
baseline; neither can a stale same-peer prefix followed by a current edit.

The schema-8 seal contains the exact root, task/project/genesis, fixed native
node/store/version/token captures, their exact union, and absent node/store
facts. Its own two-operation metadata commit has that union as its exact deps
and the persisted message `zerus-task-causal-cutover-v1`. Loro can coalesce
ordinary adjacent commits during import even when local merging is disabled;
the persisted message protects the new seal boundary. Every span after that
seal must descend from it too. New tasks can be born sealed only through the
in-process `TaskDoc::create` witness and native first-save constructor; imported
bytes cannot manufacture that witness.

## Native store custody and bootstrap

A local `.cutover` pin binds the immutable shared seal to this native replica
store incarnation. Replacing the store identity cannot reuse an old pin.
A `.prepare` freezes the exact local version before contributions are accepted.
No normal save/import can widen that capture. No lock spans a network call.
The original proposed bytes, digest, version and seal are retained before any
remote adoption, then the exact pin is persisted before canonical task save.
A crash or uncertain reply retains the original; retry settles or looks it up,
never makes another seal, adds an acknowledgement or replays native input.
A peer lost after prepare is not a required adopter and does not veto others.
Its own task stays held until the original can settle/catch up.

Ordinary update bytes are only candidates. A fresh/returning store adopts an
already admitted seal through a separate current-source lane: a fresh verified
route/hello binds the exact source node/store; its read-only native exporter
can advertise only an already admitted immutable pin. Export cannot mint,
repair or rebase a root. The receiver independently checks the full history,
root/seal and own retained capture before pin/save. Clean prefixes catch up
automatically. Extra old local operations are refused and their original files
remain kept for review; there is no selective salvage or automatic replay.
Conflicting seals hold only that task, with original native outcomes retained.
Unrooted/damaged/shallow tasks receive a task-specific refusal, not a computer
or project-wide optional-writer veto. There is no generic shallow bootstrap.

## Ingress and writer census

| Path | Admission boundary |
| --- | --- |
| `TaskDoc::import` / `import_block` | Import into an inert fork; validate before assigning the canonical document. Root/seal replacement refuses. |
| `sync::import_task` | Common candidate validation and native pin gate, including a fresh copy, before recovery/set-aside/save. |
| `sync::recovered` | Retained local blocks cannot enlarge the admitted baseline. Existing damaged-copy protections remain. |
| `sync::refresh_phone_task` | Same history/pin gate before replacing a shallow copy; local-version inclusion alone is insufficient. |
| `ProjectStore::save` / `Admitted::save` | Validate the proposed write and any stale-handle merge, including saved=None and unchanged-base paths. |
| `load_admitted` / native counted reads | Reconstructed disk bytes must match native pin custody; incomplete or replaced copies remain held. |
| `sync::import_project` | Project metadata cannot replace a task pin or clear a separate prepare hold. |
| Root `replica::exchange` responder | Ordinary request imports use common refusal; request annotations grant nothing. |
| Root `replica::exchange_pages` initiator | Only a response bound to the fresh exact node/store can consume the native export's admitted-seal evidence. |
| Root `fence::task` / proof-round imports | Common import/save checks; prepare/export/adopt are explicit scoped actions. |
| Native task Start/send/handoff/reopen | Final catalog/project/store admission rechecks seal, own pin, prepare hold, floor and original task/version before reserving the original request. |
| `phone` / replica FFI | Native save/import gates remain strict. The explicitly separate phone-cache handle receives sealed state read-only, writes no native pin and refuses native prepare/adopt/export/settlement. Native reopening cannot use that cache as admitted state. |
| Explicit damaged-copy review | Existing exact person confirmation fingerprints and archives document, pin, prepare and original proposal together; it grants no bootstrap or dispatch authority. |

Read-only legacy DTO projection remains possible. It does not qualify a local
writer, construct a migration root or authorize native input. Released older
readers treat schema 8 as a newer read-only task. The updated mobile/FFI library uses the read-only phone-cache handle for schema-8 receipt. It keeps receiving new/updated tasks without constructing native authority. Future mutable mobile writer qualification remains separate (`zerus-exooq`); ordinary serialized source claims cannot qualify it.

## Reachable certificates and availability

Fence capability 2 and certificate format 2 bind the immutable seal, exact
covered node/store identities and negative absent facts. Old certificates
cannot bypass the new checks. Available participants retain same-version,
raw-schema, store-identity and final-admission checks. Discovery, bootstrap and
proof calls share the original deadline. Discovery uses one aggregate pool,
shared among queued routes, rather than a fresh timeout for each absent peer.
A fixed remote cutoff reserves part of the original deadline for local
settlement and final admission; real timeout sleepers cannot consume it.
A node lost during confirmation becomes a negative fact, never an invented
acknowledgement. The remaining exact covered cohort must remain unchanged.
Membership/store/seal drift still requires re-evaluation.

Existing historical-floor tasks also advance without Start. The existing
swarm worker and manual sync perform protocol-only cutover work: one project
and candidate per round, a persistent disposable index cursor over at most
32 rows, and project rotation even when candidates are held or missing. The
ordinary index may perform its existing cache rebuild/revalidation; the new
candidate query does not decode an unbounded queue. All index/project locks
are released before networking, and the actual task/root/store are re-read
under native admission before freeze. This creates no acknowledgement,
certificate, execution attempt, report, or native action. Ordinary task/Teams
and Person checks remain unchanged. A returning partial save resumes the
exact retained original; it never replaces the proposal with newer bytes.

Final admission and certificate reuse independently require this store's active
pin and no pending prepare. Reusing a certificate is schema-barrier evidence,
never consent or native delivery authority. Native reservations, settlement,
Unknown and the existing no-replay boundary remain unchanged.

## What happens when an absent computer returns

The available current computers continue starting tasks. The absent computer
is listed with its exact node and last verified store (or explicitly unknown),
without an acknowledgement. On return an updated computer automatically syncs
and adopts the existing admitted seal before its own task writes/starts. A
clean prefix needs no person ceremony. If it made extra old-rule edits, those
bytes stay kept and the task's write/start is refused with the cause and sync
path; direct and relayed stale updates are refused elsewhere. No edit or native
request is silently replayed. An older disconnected executable cannot be made
to learn a new floor remotely; its newer-format task is read-only after sync,
and current receivers never accept its stale branch as current authority.
No owner process, live state or optional computer is reconfigured by this PR.

## Local qualification

The source controls cover a genuine C2 floor with zero certificate, A/C current
and B absent; A's original reservation; stale B direct/relayed rejection;
clean B adoption of the same root and a new original; fresh receiver adoption
only from native admitted source custody; replaced store identity; stale saves;
coalesced historical F; mixed same-peer spans; immutable seals; shallow/missing
proof; retained proposals and crash settlement; early/wrong-store replies;
late lost peers and bounded slow-first discovery. Mutants independently remove
root/span/store/consumer/certificate boundaries. Evidence is local Arch and a
private headless Mac run. Hosted CI is not dispatched, polled or a merge gate.

## Proof resource exhaustion (`zerus-s6pk0`)

Root inspection and per-span causal validation use the same work accounting
policy: at most 131,072 visits per scan. Root work includes each visited change
and each inspected operation position in a change touching task metadata;
causal work counts each visited change span. Existing identity, floor, seal,
closure, value-size and causal checks remain mandatory. Incomplete proof never
returns an admitted root or permits a canonical mutation.

The wall clock is a last-resort resource stop at 30 seconds per scan, rather
than a two-second scheduling sensitivity. This gives a loaded development host
15 times the previous scheduling margin while retaining a finite safety net.
It is not an input or admission timeout: each original request retains its own
deadline and final admission checks. Scoped thread-local test accounting is
available only in tests and `test-support`, with no environment or configuration
switch in production.

Exhaustion is typed `ErrorKind::Busy`, meaning the proof is incomplete and a
fresh read can be tried. Invalid historical evidence remains a rule refusal.
Busy must propagate through native load/save/import, stored-update replay and
recovery; it cannot become a damaged copy, cached error, replacement candidate,
schema repair, certificate, native outcome or replay permission. Teams exposes
this as `store_busy` with a retry message, not `storage_failed`. The phone FFI
retains its existing error variants and exposes this non-admitted result as a
refusal with the busy/retry message. Retained original request identities,
uncertain outcomes and no-replay protections continue to apply.

Retryable proof is not proof that an earlier native request never started.
Once durable attempt saving begins, its existing uncertain-result handling
still retains the original request and hold. Busy does not settle it or grant
native replay. Only the pre-reservation delivery path retains the ordinary
busy/non-start cause.
