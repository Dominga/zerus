# Service-owned creation launch

The third creation effect owned by the desktop actor is the local, detached
agent launch that the native creation reducer starts after its folder,
clone and worktree steps. It follows the [creation clone](desktop-creation-clone.md)
and [creation worktree](desktop-creation-worktree.md) leaves. Its kind,
binding, ledger and result stay separate from theirs and from task-start
reconciliation. G1 agreed this per-kind split on 2026-11-03 (zerus-5wmh.66.4.3.8).

## Scope

Only a local, detached launch belongs to this leaf: the reducer's `Launch`
effect for an empty host, without `terminal`, for an agent in the closed set
`codex`, `claude` and `kimi`, each of which `launch::detached` starts with
`-d`. DeepSeek hosts (`dsh`), OpenCode, external-terminal launches, remote
hosts and every launch outside desktop-service mode keep the GUI-direct path.

## Current state

The closed `CreationLaunch` frontend request and its wire types live in
`client/core/src/desktop/creation_launch.rs`. As for clone and worktree,
native Service mode and the exact configuration authenticate it. Generic
`into_request` refuses it, and it has its own ordering lane. No service
advertises `Desktop creation launch ABI: desktop-creation-launch-v1` yet, and
an authenticated request is answered `creation_launch_unsupported`. The
client keeps its direct launch until the actor and a separate default-off
service path land.

## Wire

- **Intent.** The exact durable reducer checkpoint written just before the
  launch: state `launching`, `launch_dispatched`, an empty host, no
  `terminal`, no stop, no bound session and no resource error, with any clone
  and worktree settled. The launch ID is the checkpoint's own; the actor never
  mints or rotates one.
- **Plan.** The agent, target, session name (`tag`), account, launch ID and
  expected machine. Its arguments are the shared `launch::launch_invocation`
  for this computer, exactly what the direct path runs. Their SHA-256 matches
  a result to the plan; it is neither execution authority nor a current
  target identity.
- **Binding.** Kind, schema, configuration, creation ID, launch ID, revision
  and the source, plan and consent SHA-256s.

## Result and evidence

The original result is the original launch child's exit after its physical
drain, never a replayed launch. Exit 0 is launch-command success, not a
running or current agent; the session listing still binds it. Any other exit
is passed to the direct path's own `launch::launched`, so the reducer records
it as it does today. The retained stderr is cut to 4 KiB of encoded JSON on a
character boundary, so storing a result never fails on size. Queued, running
and Unknown never mean success or failure.

`Query` and `Lookup` form a bounded read-only lookup bound to the fresh
configuration and service epoch. Beside an Unknown original only, a lookup may
carry a bounded `Observation` of the original launch: its start and end
records and its name's binding. It is display evidence only. It never settles
the row, releases a charge, rotates the launch ID or authorizes another
launch. An end record is nonexclusive (`binding_unresolved` stays true), and a
binding still waits for a fresh exact session listing.
