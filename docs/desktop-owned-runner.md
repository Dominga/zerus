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

`client/core/tests/run_owned.rs` executes only private synthetic children. It
covers exact UTF-8 input, cap boundaries, guarded refusal, successful-child input
failure, independent output overflow, cancellation and timeout, plus inherited
stdin and stdout retaining admission after child exit. These are real process
and pipe lifetime cases on Unix, with no service-restart, reboot, native-agent
or Windows runtime claim.
