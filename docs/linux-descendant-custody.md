# Dormant Linux descendant custody

`src/state/processes/custody.rs` is an unconnected support resource for the
REV2 graceful-Terminate design in `zerus-jmxyf.7`. It has no installed command,
supervisor caller, signal operation, archive writer or cleanup consumer. Existing
Terminate, Pause, Stop, macOS and Windows behavior is unchanged. G1 review is
required before merging this resource; its presence grants no graceful eligibility.

A fresh dedicated executable arms Linux child subreaping before spawning its
one original root. Its caller must remain the process's sole spawner and waiter,
with no other threads or signal handlers. Admission checks one thread, no
existing children, default non-auto-reaping SIGCHLD policy and no prior subreaper
or resource. One-use state is retained even after Drop or failed creation.
There is no constructor accepting an existing PID, saved record or status flag.
The resource owns the actual new `Child`, owner/root native start identities and
pidfds. Handles remain private; the resource is neither Clone nor Send/Sync.

The nearest live subreaper adopts orphan descendants, including double-forked
daemons that change Unix sessions. See the [Linux subreaper contract](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html).
Each poll verifies the original owner, native flag and wait policy, pins newly
adopted direct children and refuses vanished or changed pins without its own
positive wait. `waitid` first observes an exit without consuming it; a nonblocking
wait through the retained pidfd consumes that same status. `__WALL` includes
clone children. A zero `si_pid` means pending children, never completion.
Completion requires a positive original-root exit and kernel `ECHILD`, with no
remaining pins or prior uncertainty. See [Linux wait semantics](https://man7.org/linux/man-pages/man2/waitid.2.html).

Bounds are one supplied monotonic deadline, at most 30 seconds from admission,
64 exit events per poll, 4096 identities over the resource's lifetime, 4 KiB per
native identity read and 64 KiB per direct-child read. Polls neither sleep nor
launch helpers. Limits, expiry, changed policy, lost identity and wait errors are
sticky Unresolved outcomes. A later empty census never repairs them. Drop sends
no signal and supplies no drain proof. Root exit data and counts are diagnostic
facts, not effect authority. The source does not claim to bound `Command::spawn`
or own provider pipes; production launch/IO custody remains separate.

This resource qualifies a short-lived original cohort within that window. It
does not yet provide a long-lived session supervisor or bind a later Terminate
request to hours of retained ownership. That lifetime and private-channel
integration must be designed before connecting a production caller.

The harness-free `linux-custody` test executable creates fresh private owners,
framed release barriers and real kernel children. It qualifies original exit,
double fork/setsid, clone waits, late descendants, admission, policy/identity/wait
loss, limits, deadlines, owner death and noninterference with an external child.
It does not operate on installed sessions or qualify provider SIGTERM/state
preservation, Mac descendant closure or production crash recovery.

Before activation, production must supply original supervisor attestation and a
private live observation channel, integrate adopted descendants with the legacy
forced path, retain physical writer/IO and temporary-stop custody across controller
death, complete the mutation fence and typed marker, and qualify bounded original
root grace/finalization against current providers. Enabling subreaping alone in
today's supervisor is unsafe: adopted orphans become siblings of the agent and
fall outside its root-only legacy Terminate census.
