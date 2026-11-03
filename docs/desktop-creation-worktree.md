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
`creation_worktree_unsupported` and advertises nothing. The client keeps its
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
