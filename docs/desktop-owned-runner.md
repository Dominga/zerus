# Owned desktop child exchanges

`zerus_client_core::run::run_owned` uses the existing `run_inner` process and
pipe implementation. It is an opt-in primitive for G1's future admitted worker,
with no operation authority, scheduler, source reader or positive IPC. Existing
GUI, streamed import and decoded fleet callers keep their current defaults.

The caller supplies the executable, original Invocation, environment, cancellation
flag, a one-use guarded spawn hook and one shared resource admission. Input must
already exist and fit 1 MiB; the runner never reconstructs it from a plan. The
deadline must be positive and at most 30 seconds. The supplied hook must enforce
the host's current fence and use its existing guarded process creation path.
Pre-cancellation and invalid input refuse before that hook runs.

The runner strips the existing `HGS_SESSION` and `HGS_RUN_ID` attribution fields,
preserving the other selected configuration. It caps stdout at 1 MiB and stderr
at 16 KiB independently. It returns only stdout after exit zero, successful full
stdin writing, and real EOF from both readers. A write failure, read failure,
partial pipe exchange, nonzero exit, cancellation, timeout or overflow returns a
body-free `OwnedError`; private child diagnostics never enter that error.

One resource guard copy lives with each of the child loop, input writer, stdout
reader and stderr reader. Cancellation or timeout stops and waits for only the
owned child. An inherited stdin reader can leave the writer blocked, just as an
inherited stdout/stderr writer can leave a reader blocked. These detached threads
keep their guard after the bounded exchange returns. A short join or returned
error cannot release global worker admission. No process group or descendant is
signalled; synthetic descendants in the tests end on their own short timer.

If kill/wait cannot confirm the child's exit, `ExitUnverified` conservatively
pins one resource guard until the host exits. A lexical child scope ending is
not proof of OS process exit. The host must bound admissions (G1 allows one)
and retain the durable charge for verified recovery and operator review; it
must never use this error as permission to clear the charge or retry.

The G1 actor must reserve durably before using this primitive, supply a current
authenticated producer drain and exact writer/service/adapter lease, and match
the returned bytes to the original policy and reservation. Terminal outcome and
actual resource release remain separate events. A last-guard notification can
race the worker's outcome; the actor must not turn that notification into a
drained SQL flag before it has settled the exact attempt. Returned success also
grants no permission to retry or create a new delivery identity.

This runner does not recover a previous service's charges. Before any real
operation admission ships, G1 .66.3.22 must qualify verified four-resource release
across service restart and reboot, with actionable status and updater refusal
messages. Missing retained input and unknown runtime identity stay review-only.
The passive [operation store](desktop-operation-store.md) cannot infer this proof
from imported records or a marker phase.

## Inherited custody descriptor

`run::custody::InheritedLock` is available on Linux and macOS. It accepts an
already host-verified private `File`, without opening paths, creating a journal
or granting recovery authority. `try_lock` takes a nonblocking exclusive flock
and keeps a close-on-exec descriptor above standard input/output/error.
`inherit` installs the intended helper's final pre-exec hook. The parent's flags
stay unchanged, including during concurrent ordinary spawns.

The child marks every other non-stdio descriptor close-on-exec, then clears that
flag only for custody. Linux uses `close_range(CLOSE_RANGE_CLOEXEC)` with a bounded
fcntl fallback; macOS uses the fallback. Marking flags preserves Rust's internal
exec-error pipe until exec succeeds. The hook performs no allocation or locking.
Before fork, the module checks only this process's descriptor roster (at most
8192) and soft limit (at most 1048576). Existing descriptors above a reduced soft
limit are included. Unsupported or incomplete coverage refuses inheritance.
The host must use its guarded spawn turn, must not concurrently raise the limit,
and must not add later hooks that alter inheritance. This is background work.

The guard never calls `LOCK_UN`. Duplicated/forked descriptors refer to the same
lock, and an explicit unlock would release it for all holders. See the
[Linux flock contract](https://man7.org/linux/man-pages/man2/flock.2.html) and
[Apple flock contract](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html).
The host must keep the same Arc with the child loop and all three pipe workers;
a bounded return or parent close cannot unlock a still-inherited description.
The child may pass it to descendants, which conservatively extends custody.

This primitive has no production caller. The future actor must first durably
bind a prepared custody file to the exact operation ticket and its private file
identity, then spawn with the diagnostic attempt environment marker. It must
durably register helper identity before returning Child to `run_owned`. A failure
after spawning does not prove exit, even if the hook returns an error. Recovery
must verify the same existing file and its binding before acquiring its lock,
and hold that acquisition through the exact drain commit. A held, missing,
replaced, altered or otherwise unverifiable file remains charged; custody files
must never be deleted while charged. No outcome, timer, environment scan or
imported phase substitutes for that proof. Windows remains unsupported here.

The existing `hgs task start-deliver ... --recover-only` dispatch reads and
parses its complete bounded stdin before opening task state or performing any
task effect. This ordering is a required custody invariant: a crash before the
registration commit cannot deliver input through `run_owned`. EOF or partial
input exits without task effects. `tests/test_desktop_custody.py` pins this with
the actual isolated CLI, including retention of an inherited lock while waiting
for the rest of a partial input. `client/core/tests/run_custody.rs` separately
checks the spawn whitelist, close-only lifetime, reduced descriptor limits,
failed exec, parent exit before registration and an output worker retaining
custody. These mechanism tests do not qualify actor/journal recovery or reboot.

`client/core/tests/run_owned.rs` executes only private synthetic children. It
covers exact UTF-8 input, cap boundaries, guarded refusal, successful-child input
failure, independent output overflow, cancellation and timeout, plus inherited
stdin and stdout retaining admission after child exit. These are real process
and pipe lifetime cases on Unix, with no service-restart, reboot, native-agent
or Windows runtime claim.
