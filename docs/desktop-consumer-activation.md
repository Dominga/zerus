# Native Service consumer activation

On Linux and macOS, Zerus Next uses the Service consumer when
`ZERUS_CLIENT_DESKTOP_SERVICE` is absent. `=1` explicitly selects the same path;
`=0` requests the guarded Native/rollback path. Other explicit values do not
enable the consumer. A Native request still needs its existing takeover and
drain checks; the flag supplies no writer authority and never forces a handoff.
Windows keeps explicit `=1` opt-in and its maintained manual task transition.

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

An existing Service/background installation stays on that path after the
default change. A Native installation needs a guided first consumer start and
explicit Resume when offered; changing the default does not silently clear a
revoke or an explicit passive choice. The paired backend must already be
prepared by the maintained installer before the GUI starts.

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
The helper's read-only status must confirm an existing managed registration;
the CLI's lifecycle ABI alone does not establish one. An unregistered host keeps
the plain revoke-only Qt path, without creating or enabling a desktop service.
That helper retains its original revoke UUID and original EOF hold across the
service transition and verified passive readiness. A refusal keeps Qt stopped.
The updater then reserves its own original final revoke UUID durably before
offering the authority reduction. An interrupted updater only reads that same
UUID; another invocation does not generate a replacement command or rerun the
mode helper while that result is unknown. The current-scope Root receipt must
confirm revocation and drain before Qt can proceed. An unknown result keeps the
current GUI/rollback copy available and prints the exact original request for
review. No raw manager stop, native-agent signal or replay is introduced.
The final-revoke journal lives beside the draft-snapshot directory. An interrupted
original at the older snapshot location stays on that same lookup-only path;
finding both locations requires review before any new offer.

A stale or invalid managed registration keeps Qt stopped. Inspect the helper's
record without deleting or repairing it: on Linux,
`$XDG_CONFIG_HOME/systemd/user/.hgs-desktop.zerus-registration.json` (default
`~/.config/systemd/user/`); on macOS,
`~/Library/LaunchAgents/.com.hgdev.hgs-desktop.zerus-registration.json`.
An owned, registered but stopped service is started in passive mode by this
rollback, so the helper can verify IPC and settle its original hold before Qt
starts. An unregistered host does not get a new service.

Cutover and rollback preserve accounts, catalogs, drafts, native agents, tmux
servers and DeepSeek hosts. Component tests use private manager/transport
fixtures and original child objects. The real Linux helper's mode-transition
test checks its written unit's `KillMode=process` and exact manager-call scope;
this does not qualify installed agent survival. Installed Linux/macOS delivery,
activation, presence and device acceptance remain separate release evidence.
