# Original termination outcome retention

The service retains the original explicit Remove-after-Terminate capture through
`desktop-termination-outcome-v1`. The retention boundary stores data only. It neither invokes
Terminate nor authorizes cleanup, establishes current checkout ownership, or
reports native resource drain. The native Terminate runner keeps its existing
45-second local and 60-second peer budgets; this path does not move it to the
30-second owned runner.

The closed `TerminationOutcome` request includes the selected configuration and
current service epoch, original confirmation UUID, Remove choice, original
Terminate UUID, exact original input JSON, and captured target, run,
conversation, machine, checkout, common directory and launch-owner identity.
The confirmation UUID and Terminate UUID are distinct. Missing launch ownership
stays missing. A future reconciliation consumer must use the existing exact
original-ID lifecycle receipt observer; retained input alone proves no result.

Admission uses the native frontend SERVICE lane and its original opaque proof.
The actor begins an IMMEDIATE transaction before checking the proof, captured
plan and current configuration/service epoch. It checks the same proof again
immediately before insertion. Waiting for the database lock never renews the
two-second proof deadline. Generic desktop requests and renderer policy controls
cannot admit this kind.

Store schema 25 follows the real schema-24 creation-launch migration. Its
standalone transaction adds one immutable table, capped at 64 originals of at
most 48 KiB each. Original input JSON is capped at 8 KiB. Capacity exhaustion
refuses new admission; originals are neither expired nor recycled. A confirmation
cannot bind a different Terminate UUID. A duplicate requires identical original
capture, choice and input bytes. The current epoch must match on each admission;
a duplicate after restart retains the first stored epoch and original bytes.
A separate SHA-256 covers the complete stored original bytes, including that
first epoch; the epoch-independent correlation fingerprint cannot replace this
integrity check.
A failed commit never returns durable success.

On reply loss, the read-only `termination_outcome_status` query requires the
selected configuration and original Terminate UUID, plus its own query UUID.
It returns bounded identity/hash metadata, never the captured paths or raw input.
Missing is explicit, and corrupt or conflicting data refuses rather than becoming
an empty result. Neither status nor a retained acknowledgement grants retry,
execution, removal or resource release. The native submission described below
consumes this typed retention boundary; background receipt reconciliation
remains separate work, without an additional Terminate executor.

## Native submission

In service mode, the existing native Remove confirmation consumes its opaque
Offer once and freezes the original Terminate UUID, input and machine-guarded
invocation before dispatch. The read-only offer retains its selected service
relay, helper image, configuration/epoch/generation and native environment;
confirmation carries that same selection without selecting another helper. It
uses that same helper and environment for capability probes, inspection,
retention and the one native call. A failed retention never selects the ordinary
CLI or falls back to a different action. Ordinary/native and replay paths keep
their existing behavior.

The controller checks the actual selected configuration through desktop status,
then uses a fresh original-child registration for retention. A lost reply may
query only the original UUID, while the same live controller has never invoked
Terminate and all old helper/pipe resources have physically finished. Missing,
corrupt, unknown, foreign or cleanup-authorizing responses refuse. After every
slow retention/guard/delay check, the dispatcher rechecks the captured session
and current helper, routing, configuration, environment and cancellation.

Immediately before calling the existing runner, a one-way process-local stage
changes to DispatchBegun. Failure, timeout, GUI exit or a later status cannot
restore it. The original 45-second local / 60-second peer invocation remains;
the metadata RPC's owned runner and its 30-second cap are not used for Terminate.
Original machine guards, scoped receipt validation, completion/Pump hooks and
Quit/exit-work behavior remain intact. A metadata acknowledgement is never a
successful termination receipt or cleanup proposal.

The same Actions claim is retained by actual I/O resources, including after a
cancelled/dropped async caller or bounded error with a pipe still open. Fixed
native environment snapshots preserve OS strings, empty/unset values and the
working directory, apply the existing WebKit child environment restoration, and
strip person-action attribution as before. Retained snapshots are bounded; OS
environment enumeration itself still uses the standard library's snapshot.

The native cleanup owner keeps at most 64 process-local correlation records.
They preserve the original intent and offered/dispatch bookkeeping, and cannot
construct a controller. This confirmation path keeps an unconfirmed native run
fenced across session, alias or conversation changes. These records are not yet
a durable client
recovery journal: restart observation and service-owned receipt reconciliation
remain separate follow-up work. No stored metadata can submit a recovered
Terminate or remove a checkout.
