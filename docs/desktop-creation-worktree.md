# Service-owned creation worktree

The second creation effect owned by the desktop actor is the local POSIX
worktree checkout that the native creation reducer requests after its catalog
step. It follows the [creation clone](desktop-creation-clone.md) leaf. Its own
kind, binding, ledger and result stay separate from clone and from task-start
reconciliation. G1 agreed this per-kind split on 2026-11-03.

## Current state

The closed `CreationWorktree` frontend request and its wire types live in
`client/core/src/desktop/creation_worktree.rs`. As for clone, native Service
mode and the exact configuration authenticate it. Generic `into_request`
refuses it, and it has its own ordering lane. On Linux and macOS the service
advertises `Desktop creation worktree ABI: desktop-creation-worktree-v1`, and
the admitted desktop actor executes it. Windows answers
`creation_worktree_unsupported` and advertises nothing.

On Linux in desktop-service mode only (default-off), the native creation host
routes a local, receipt-backed `CreateWorktree` and its `status` reads to the
service
(`client/src-tauri/src/desktop_service/creation_worktree.rs`):
- **Fresh intent.** Only a direct Submit, consumed once and separately from
  clone's, sends a fresh intent. Recovered and task-reserved creations get
  read-only lookups.
- **Lookup results.** A missing or Unknown lookup is unconfirmed. It never
  resends and never falls back to the direct path.
- **Older CLI.** If the CLI does not advertise the kind, a fresh intent fails
  as definitely unsent, and only then may the reducer retire its request UUID.

Cancel, remote creations, macOS and every creation outside desktop-service
mode keep the existing direct `worktrees` calls.

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

`worktree_requests::create_owned` is the only owned entry. The actor's Reserve
check runs before the original receipt is reserved. The same receipt store as
the direct path (request UUID plus argument fingerprint) keeps an original
from ever being replayed. The direct CLI path keeps its behavior.

Every Git command runs through the repository worker under the effect's
close-only custody lock and cancellation lifeline. Each command is anchored in
a pinned directory object.

- **Pins.** The repository folder, the destination's parent and the
  repository's common Git directory are pinned by identity (no-follow
  component walks). Each pin is re-verified around every Git command and step.
- **Destination.** A private staging folder is published with a no-replace
  rename. Git then creates the checkout from inside that published object:
  its working directory is the pinned destination, the path is `.`, and the
  explicit `--git-dir` is the pinned common directory, verified immediately
  before and after. Because the path is `.`, Git names the linked-worktree
  metadata entry `-` (then `-1`, ...); nothing derives meaning from that name.
- **Steps.** Destination, Checkout, Verify, Track and Own are separate actor
  checks, so no write after `worktree add` goes unchecked.
- **Track.** The upstream writes run inside the pinned common directory
  (`--git-dir=.`), with no checkout or path indirection.
- **Own.** The ownership record is written only for metadata that is an entry
  of the pinned common directory. That metadata stays pinned while the record
  is written, and every pin is re-verified afterwards. The record itself is
  returned as the receipt's ownership evidence; no later plain-Git lookup runs.

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

`Query` and `Lookup` form a bounded read-only original-result lookup bound to
the fresh configuration and service epoch. A missing or Unknown result never
authorizes a resend or a fall back to the direct worker in Service mode.
Imported or restarted records never reconstruct consent.
