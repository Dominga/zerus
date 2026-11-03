# Desktop readers after a CLI update

Replacing the installed `hgs` file does not replace an already running process.
Both `desktop serve` and `desktop watch` capture their original executable and
reuse the shared `Executable::replacement` detector. Every five seconds they
check for a different regular executable at that path. The same ready replacement
must appear twice; a missing, unready or changing file does not retire a reader.

`desktop watch` emits a fatal `hgs_replaced` display frame in its original stream
scope, then exits successfully. Socket timeouts preserve incomplete frame bytes
while checking the image. The existing client treats the error as a read-only
resnapshot, retains its stale catalog, and reconnects with bounded backoff. It
does not repeat startup Grant, report publication, Resume or an effect request.

The service stops new effect/producer/audit/finalizer admission on replacement.
Existing watch subscribers receive `hgs_replaced`; new watch subscriptions and
positive effect requests are refused while status and ownership status remain available. The maintained
original revoke/revoke-status channel also stays available, using its unchanged
scope, receipt and drain checks. `desktop status --json` reports
`executable_upgrade` with `new_admission: false`. Original background workers and
serve-owner loans keep their real resource charges and keep being drained. No
deadline, returned outcome, absent PID or replacement binary releases a charge.
The current writer/ownership intent is not replaced with a revoke or Rearm.

Only after background/feed drain and the final original serve-owner loan ends
does the service close its listener, finish original client cleanup and checkpoint
SQLite. It then exits with a nonzero replacement diagnostic. The maintained Linux
`Restart=on-failure` and macOS `KeepAlive.SuccessfulExit=false` registrations start
the new image; exit 78 remains reserved for permanent startup refusals. A manually
started service prints the same diagnostic and needs an explicit new invocation.
No service-manager command or self-exec is issued by this retirement path.

Windows task instances have no equivalent automatic restart contract. Replacement
there stops admission, drains existing work and reports
`manual_refresh_required`, but keeps the original
service alive for the maintained explicit registration/instance transition. It
does not adopt another task, call RunEx or replay manager operations.
The Service consumer shows this state in the footer and Settings > About,
including after a watch refusal or revoked background access. Its bounded
projection stays attached to the original checked service epoch. A status check
does not repeat startup Grant/import handling or refresh the task. About directs
the user to read `hgs desktop registration status --json` from a foreground
Command Prompt outside Zerus using the same CLI and configuration, keeping the
selected mode and any original pending request. This build has no Windows
original-instance refresh control; ordinary install and same-mode selection
do not refresh a stale service. That explicit native callsite is tracked separately.
If the actor itself fails, final cleanup waits only for actual owner loans;
supervisor support never makes that loan wait permanent. A later shutdown signal
also ends a replacement connection wait, preserving explicit termination.

An unresponsive original SDK callback or worker can keep retirement pending.
Status stays readable during normal actor retirement while the actual owner is retained. This is not a successful
upgrade, permission to launch a second host, or permission to kill a native agent.
Use the existing resource-custody diagnostics and maintained recovery process.
Unknown notification/operation outcomes stay Unknown across any eventual restart.

Private replacement tests copy the CLI into a temporary profile and replace only
that copy. No installed GUI, service manager, agent, tmux or owner session is used.
Kernel-owner-loan and partial-frame tests cover resource retention and timeout
framing separately from those real CLI replacement checks.
