# Dormant original supervisor owner

This prerequisite of the REV2 graceful-Terminate design (`zerus-jmxyf.7`)
is compiled behind `supervisor::ORIGINAL_OWNER = false`. No setting,
environment variable, CLI command, saved record or channel request enables it.
It adds no signal, input, Force, Terminating marker or worktree cleanup operation.
G1 review is required before merge. Enabling it remains a separate reviewed change.

The disabled route retains the existing `supervisor::spawn` body and actual
`std::process::Child`. Static dispatch forwards the existing status/wait calls
of the Claude startup appearance executor and Kimi launch-prompt loop directly
to that child. It creates no ownership endpoint or metadata and makes no
subreaper or SIGCHLD policy change. Windows always follows that route.

The dormant route reserves an `original_owner` binding under the session writer
before native creation. It holds the original state/configuration directories,
selected running image and supervisor birth, and records the original SID,
run and launch-conversation/expected identity. Unconfirmed conversation identity
stays unconfirmed; this observation does not promote it into a native binding.
The same actual child returned by the original guarded spawn stays private to
one retained owner. Startup borrowers receive only a sealed status/wait facade.
A positive status is cached for that original root; it never describes a later
PID incarnation. The shutdown forwarder's original registration is cleared
while the exiting root is still held, before consuming its wait status.

On Linux, fresh one-thread/no-child admission arms subreaping before spawning
the root. The kernel resource retains pidfds and birth pins; its original-owner
mode has no thirty-second lifetime expiry. Individual polls remain bounded by
64 wait events, 4096 live pins, 4 KiB identity reads and 64 KiB direct-child
reads. Only positive original waits retire pins. The short-lived generic mode
keeps its existing thirty-second deadline and lifetime identity bound. The
future Terminate operation's separate original thirty-second budget is unchanged.

Startup probes only consume their original root's status. They do not run a
global reaper while fixed startup helpers still own their children. After those
borrowers return, polling waits for the core helper pool's physical charges to
settle before collecting adopted descendants. An available writer or a logical
helper error is not physical settlement. No caller callback, arbitrary spawner,
policy setter, raw child accessor or restored-owner constructor is provided.

Continuity is a property of this reviewed original program and its fixed
borrowers, not a fact inferred from two kernel snapshots. This profile never
turns subreaping off or installs auto-reaping SIGCHLD behavior; interposed
loader profiles refuse admission. Policy/source loss revokes observation and
remains unresolved. The maintained off/on escape control deliberately violates
the generic resource's caller contract: a generic `Complete` can coexist with
an escaped descendant. That generic result cannot construct this owner or its
source endpoint and is never effect authority. Future effect consumers must
retain the original source/owner and physical producer/IO admission; they must
not accept `Complete`, a copied JSON field or a fresh reconnect as permission.

On macOS, this owner retains only its original root and native peer/source
identity. Descendant coverage explicitly stays `unproved`; empty snapshots or
root exit do not qualify descendant closure. Kqueue/custodian closure remains
an activation prerequisite.

A private read-only endpoint accepts one exact observation frame per poll,
with one absolute 25 ms frame budget, an 8 KiB frame cap and no queued history.
A one-byte completion handshake keeps the same connection open through the
observer's final native check, within that unchanged original budget.
Both native socket credentials and the peer's running image are checked; a PID
in JSON is not peer evidence. Source replacement, stale run/nonce, incomplete
frames and unrelated native images refuse. The test observer has one absolute
250 ms deadline including nonblocking connection. Replies are diagnostic data,
always `effect_authority: false`; no installed observer or effect consumer exists.

A creation failure keeps its reservation. Unknown creation is not retried,
including the legacy early-resume fallback. This single-original profile does
not qualify an automatic replacement root. Any owned-mode reservation, including
malformed metadata or an already-exited root, refuses legacy root-only Terminate
and archive finalization. Original exit evidence may still commit. Normal records
without that field retain their existing lifecycle behavior.

The private kernel harness is itself a fresh outer subreaper. It retains all
fixture namespaces through normal and unwind settlement, drains only its owned
children within one fifteen-second cleanup deadline, and checks kernel `ECHILD`
independently. Failure reports Unresolved and retains that same reaper and
namespaces through physical retirement; it never turns a later drain into a
successful deadline result or delegates its remaining children to machine PID1.

Before usable graceful Terminate: qualify a complete original launch/source and
writer/IO admission, typed persistent marker, controller-death stop/resume lease,
Mac descendant closure, current-provider original-only grace and saved-state
resume, and cohort-aware finalization. Force remains separately confirmed and
is never automatic. Stop-incomplete and removal-Unknown stay distinct.
