# Original termination outcome retention

The service retains the original explicit Remove-after-Terminate capture through
`desktop-termination-outcome-v1`. This leaf stores data only. It neither invokes
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
A failed commit never returns durable success.

On reply loss, the read-only `termination_outcome_status` query requires the
selected configuration and original Terminate UUID, plus its own query UUID.
It returns bounded identity/hash metadata, never the captured paths or raw input.
Missing is explicit, and corrupt or conflicting data refuses rather than becoming
an empty result. Neither status nor a retained acknowledgement grants retry,
execution, removal or resource release. Native UI submission and background
receipt reconciliation remain separate work; this leaf supplies their typed
retention boundary without an additional executor.
