# Service-owned creation worktree

The second creation effect owned by the desktop actor is the local Linux
worktree checkout that the native creation reducer requests after its catalog
step. It follows the [creation clone](desktop-creation-clone.md) leaf. Its own
kind, binding, ledger and result stay separate from clone and from task-start
reconciliation. G1 agreed this per-kind split on 2026-11-03.

## Current state

The closed `CreationWorktree` frontend request and its wire types live in
`client/core/src/desktop/creation_worktree.rs`. As for clone, native Service
mode and the exact configuration authenticate it. Generic `into_request`
refuses it, and it has its own ordering lane. On Linux the service advertises
`Desktop creation worktree ABI: desktop-creation-worktree-v1`, and the
admitted desktop actor executes it. The owned checkout binds Git to the pinned
common directory through `/proc/self/fd`, which macOS lacks, so macOS and
Windows answer `creation_worktree_unsupported` before admission and advertise
nothing. The client keeps its
existing direct `worktrees create --receipt` path until a separate
default-off service path lands.

## Actor

The actor (`src/desktop/creation_worktree`) mirrors the clone leaf. It shares
clone's machine-source and lease guard; its consent, ledger, ticket and result
are worktree-specific.

- **Admission.** The opaque native permit is rechecked after `BEGIN
  IMMEDIATE`, together with the current writer epoch, the local native machine
  source, the original service owner and the handoff exclusion. Admission then
  durably retains the immutable source, the fixed plan and the consent binding.
  The one live consent value exists only in the running actor. A restarted or
  imported original becomes Unknown and is never executed.
- **Custody and steps.** One worker commits the original inherited-lock
  custody with its exact ticket before any effect. It then runs the owned
  effect, `worktree_requests::create_owned`. Each step needs a fresh actor
  transaction with the same lease, consent, ticket and next step: Reserve,
  Destination, Checkout, Verify, Track, Own.
- **Settlement.** The result settles only after a read-only worker observes
  the original custody lock released. A lost result is read from the original
  CLI receipt and is never re-run.
- **Storage.** The ledger has its own tables, `desktop_creation_worktrees`
  and `desktop_creation_worktree_budget`, added at Store schema 22 by merge
  order. The bounds are the same as clone's: 4096 IDs, 16 MiB, 32 unsettled
  charges and one effect worker.
- **Accounting.** Its undrained charges join `operation_undrained` and the
  takeover, revoke, startup, recovery and status accounting.
- **Lookup.** `creation_worktree_result` is the bounded read-only
  original-result lookup.

## Exact input

`Intent` carries the current configuration and service epoch, the creation
UUID, the worktree request UUID, the checkpoint revision and the original
reducer checkpoint bytes, at most 32 KiB. Verification accepts only a fresh,
local, unambiguous checkpoint that meets all of these:

- state `worktree`, an empty host and a verified expected machine;
- `worktree_receipts` true;
- no `worktree_done`, `launch_dispatched` or `stop_requested`;
- no bound session, creation owner, acquisition confirmation or resource error;
- a settled clone, or no clone at all;
- canonical UUIDs only, and no duplicate keys.

The fixed plan is the checkpoint's working folder (`target`), `worktree_common`,
`branch`, `destination`, request UUID and expected machine. All paths are
absolute POSIX paths without control characters. The branch is non-empty, at
most 1024 bytes, has no whitespace or control characters and does not start
with `-`. The base is always `HEAD`, as on the direct path.

The binding records the source, plan and consent digests. Its consent digest
covers the exact original bytes, so unknown checkpoint fields are retained and
bound. `Plan::arguments` reproduces the exact `worktrees create` arguments
after `create` (without `--receipt`). `Plan::request_fingerprint` is the
existing CLI receipt fingerprint over those arguments. That fingerprint
identifies the original request and receipt. It is neither execution authority
nor a physical repository or destination identity.

## Owned effect

`worktree_requests::create_owned` is the only owned entry. It is Linux-only:
elsewhere it is refused with `creation_worktree_unsupported` before any
reservation, receipt or effect (see Git binding below). The actor's Reserve
check runs before the original receipt is reserved. The same receipt store as
the direct path (request UUID plus argument fingerprint) keeps an original
from ever being replayed. The direct CLI path keeps its behavior.

Every Git command runs through the repository worker under the effect's
close-only custody lock and cancellation lifeline. Each command is anchored in
a pinned directory object.

- **Pins.** The repository folder, the destination's parent and the
  repository's common Git directory are pinned by identity (no-follow
  component walks). Each pin is re-verified around every Git command and step.
- **Git binding.** Git never receives a pathname for an object it writes. The
  checkout runs with the pinned destination as its working directory (path
  `.`) and the pinned common directory as Git's own inherited descriptor,
  `--git-dir=/proc/self/fd/N`. The descriptor stays open only after the
  custody whitelist, through the worker into Git. A swap of either pathname
  after the final identity checks therefore cannot redirect Git; the checks
  after the command then leave the result unconfirmed. macOS has no such
  descriptor link, so the owned path refuses there and the direct path keeps
  serving it. The checkout forces `worktree.useRelativePaths=false`, so the
  links Own checks are absolute. Because the path is `.`, Git names the
  linked-worktree metadata entry `-` (then `-1`, ...); nothing derives meaning
  from that name.
- **Destination.** A private staging folder is published with a no-replace
  rename; Git then creates the checkout inside that published object.
- **Steps.** Destination, Checkout, Verify, Track and Own are separate actor
  checks, so no write after `worktree add` goes unchecked.
- **Track.** Always a checked step. With a safe inherited upstream, its
  writes run inside the pinned common directory (`--git-dir=.`, so Git
  resolves the held object's own path), with no checkout or path indirection.
  Without one it writes nothing, so every repository passes the same six steps.
- **Metadata entry.** Right after `worktree add`, the entry Git made is pinned
  by descriptor: named by the gitfile read through the pinned destination,
  opened through the pinned common directory's `worktrees`, and accepted only
  when its `gitdir` backlink, `HEAD` branch and `commondir` link, read through
  the entry itself, are that checkout's.
- **Own.** Own re-reads those links through the pinned entry, requires Git's
  own `--absolute-git-dir` for the checkout to be that same object, and writes
  `zerus-owner.json` through the entry with a no-replace rename: an existing
  owner file is never overwritten. Every pin is re-verified afterwards. The
  record itself is returned as the receipt's ownership evidence; no later
  plain-Git lookup runs.

After effects start, any refusal, pin or identity mismatch, or read or write
failure records the original receipt as `unconfirmed`, never `created` or
`failed`. Branches and partial files are kept, with no cleanup and no new
UUID.

## Result and recovery

The original CLI worktree request receipt is the original result and is never
replayed. `Receipt::result` advances the reducer only after an actual resource
drain, and only for a `created` or `failed` receipt whose request ID and
request fingerprint match the plan. A created receipt then passes the same
`worktrees::created` check as the direct path, and its path must equal the
destination. Queued, running and Unknown never mean success or failure.

A result carries a bounded projection of that receipt (`project`): only the
fields the reducer checks (request ID, fingerprint, status, path, common
directory, branch, `effects_started`, `detail` and `resource_error`), with each
free text bounded to 2 KiB of encoded JSON. The full receipt, including its
ownership record, stays in the CLI receipt store. Admission (`Intent::verify`)
refuses, before anything starts, a plan whose projection with its own paths and
the longest texts would exceed the 16 KiB result bound. If an original still
cannot be represented, its result is Unknown with
`worktree_result_unrepresentable`. Storing a result therefore never fails on
size, and a physically released charge always drains.

`Query` and `Lookup` form a bounded read-only original-result lookup bound to
the fresh configuration and service epoch. A missing or Unknown result never
authorizes a resend or a fall back to the direct worker in Service mode.
Imported or restarted records never reconstruct consent.
