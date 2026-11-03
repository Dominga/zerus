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
`into_request` refuses it, and it has its own ordering lane. On Linux and
macOS the service advertises `Desktop creation launch ABI:
desktop-creation-launch-v1`, and the admitted desktop actor executes it.
Windows answers `creation_launch_unsupported` and advertises nothing. The
client keeps its direct launch until a separate default-off service path
lands.

## Actor

The actor (`src/desktop/creation_launch`) mirrors the clone and worktree
leaves. It shares their machine-source and lease guard; its consent, ledger,
ticket and result are launch-specific.

- **Admission.** The opaque native permit is rechecked after `BEGIN
  IMMEDIATE`, together with the current writer epoch, the local native machine
  source, the original service owner and the handoff exclusion. Admission then
  durably retains the immutable source, the fixed plan and the consent binding.
  Live consent exists only in memory: a restarted or imported queued original
  is retired as Unknown and never executed.
- **Storage.** The ledger has its own tables, `desktop_creation_launches` and
  `desktop_creation_launch_budget`, added at Store schema 24 by merge order.
  Rows are keyed by launch ID, because a creation can rotate its launch ID.
  The bounds are the same as clone's. The child's drained exit is stored in its
  own immutable column until the resource drain settles the receipt.
- **Effect.** One checked step, then one original launch child: this
  computer's own `hgs` with the plan's exact arguments
  (`launch::launch_invocation`), in its own process group, with closed input,
  drained output and the direct path's 90-second deadline. An exit counts only
  after the child exits and both of its output pipes end within the deadline.
- **Custody.** The child inherits the effect's close-only custody lock, and
  its `main` first marks that descriptor close-on-exec
  (`HGS_CREATION_LAUNCH_CUSTODY_FD`, then removed). The lock lives exactly as
  long as the launch child, and nothing it starts (tmux, its server or the
  agent) inherits it. The desktop unit's `KillMode=process`, the tmux scope on
  Linux and `AbandonProcessGroup` on macOS keep a service stop away from
  agents as before.
- **Timeout, revoke, exit.** At the deadline only the original child is
  killed, never its process group, tmux or an agent, and the result is
  Unknown. Revoke or a service stop never kills the child: a step check before
  the launch refuses it, and an already running child finishes on its own while
  custody keeps its charge. After a service exit the child keeps its lock;
  recovery waits for the exact lock's release and then drains the row as
  Unknown. Output that another process keeps open past the deadline is Unknown,
  and that process is never stopped.
- **Settlement.** After the actual resource drain, a recorded exit 0 is
  Launched and any other exit is Failed, read through the direct path's own
  `launch::launched`. Everything else is Unknown, with its reason as the code.

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

The service's `creation_launch_result` lookup reads that evidence with the
bounded in-process reader (`src/state/launch_observation.rs`), within a
quarter of a second, on a separate thread. At most one read is outstanding;
while it is, the evidence is Unknown and no further reader starts.
