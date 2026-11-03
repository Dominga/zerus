# Native Service consumer activation

The orchestration rollout keeps the startup default unchanged. Select the
consumer with `ZERUS_CLIENT_DESKTOP_SERVICE=1`; the later Linux/macOS default
change is a separate release step after this path reaches both owner machines.
Windows keeps its explicit opt-in and maintained manual task transition.

The paired backend is prepared before the GUI starts, through the existing
installer/updater. The consumer selects that same backend, reads its native
configuration and registration, and obtains a fresh authenticated Service Grant.
Only a newly installed, implicitly passive registration has `mode_selection`
`default`. At this initial connection the native controller may select background
through the maintained helper. A running background registration is a no-op.
Ordinary updates preserve mode. Older records without provenance are `retained`;
an explicit passive selection or rollback remains paused until the owner chooses
**Resume background work**.

A revoked or rearm-required installation opens the limited consumer. Resume is
one explicit action: authenticated Rearm when needed, followed by the same
helper activation. If the current Service Grant is already valid, Resume only
selects background. The helper independently checks the original installed
frontend or foreground terminal, current Grant, selected CLI, configuration and
owned registration; local status and JSON are routing observations only.
Automatic read-only reconnection never grants, rearms, imports or activates.
There is no legacy GUI writer fallback, and automatic approvals stay off.

## Original activation and failure handling

The native controller reserves one activation UUID before spawning the original
helper. The helper durably records that UUID and its original Grant/image/
definition binding before any mode or manager change. A known completion can be
written only by that helper's retained process-local attempt. The controller
keeps the original child and both pipes charged through actual exit and EOF.
The same immutable record survives GUI/helper restart; an uncertain attempt is
looked up by its original UUID and is never offered again or upgraded from a
current readiness observation. The fixed journal retains at most 64 entries
within the existing 64 KiB registration-record bound.

- A helper refusal leaves the consumer usable. **Check result** looks up the
  original. A verified missing/not-started result offers Resume again; a missing
  helper or obsolete helper requires a maintained CLI/GUI update. Never clear a
  retained revoke or alter the registration by hand.
- A Grant timeout keeps its original ownership intent. **Check result** queries
  that original; activation waits for a confirmed current Grant. Revoked access
  offers explicit Resume, rather than silently rearming.
- A service crash after activation uses bounded read-only reconnect and keeps
  the last known catalog visible. **Retry now** checks the connection. It does
  not repeat the activation command; an uncertain activation stays on Check.

## Qt rollback

Use the existing updater/installer rollback. Before a current desktop service
can be replaced by Qt, the updater asks the maintained helper to select passive.
That helper retains its original revoke UUID and original EOF hold across the
service transition and verified passive readiness. A refusal keeps Qt stopped.
The updater then reserves its own original final revoke UUID durably before
offering the authority reduction. An interrupted updater only reads that same
UUID; another invocation does not generate a replacement command or rerun the
mode helper while that result is unknown. The current-scope Root receipt must
confirm revocation and drain before Qt can proceed. An unknown result keeps the
current GUI/rollback copy available and prints the exact original request for
review. No raw manager stop, native-agent signal or replay is introduced.

Cutover and rollback preserve accounts, catalogs, drafts, native agents, tmux
servers and DeepSeek hosts. Component tests use private manager/transport
fixtures and original child objects. Installed Linux/macOS delivery, activation,
presence and device acceptance remain separate release evidence.
