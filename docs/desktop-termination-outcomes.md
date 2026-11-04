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

The controller first probes exact UTF-8 ABI markers on its retained image,
before any new desktop subcommand. It checks the actual selected configuration
through desktop status, then uses a fresh original-child registration for
retention. A failed binding closes stdin while returning the same Child to its
runner for physical stop/reap/pipe custody; no private envelope is released. A lost reply may
query only the original UUID, while the same live controller has never invoked
Terminate and all old helper/pipe resources have physically finished. Missing,
corrupt, unknown, foreign or cleanup-authorizing responses refuse. After every
slow retention/guard/delay check, the dispatcher rechecks the captured session
and current helper, routing, configuration, environment and cancellation.

Immediately before calling the existing runner, a one-way process-local stage
changes to DispatchBegun. Failure, timeout, GUI exit or a later status cannot
restore it. The original 45-second local / 60-second peer invocation remains;
the metadata RPC's owned runner and its 30-second cap are not used for Terminate.
The native receipt is capped at 64 KiB during reading, matching the existing
Capture validator; overflow stays unconfirmed and cannot restore dispatch.
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

## Execution custody and platform boundary

Windows uses the maintained read/execute PinnedImage, with write/delete sharing
excluded for the file and its ancestor chain. Before releasing stdin it checks
the same original Child against the retained loaded-image section. A rejected
image withholds input, keeps that child in runner custody and cannot produce a
successful receipt. The shared Linux Core primitive copies the selected image
once (at most 256 MiB, with a five-second copy deadline), then applies and
verifies all four write/shrink/grow/seal seals on an executable memfd. Help,
ABI probes and subsequent executions use that same immutable snapshot. A leaf,
ancestor or same-inode rewrite after the last check cannot change its bytes.
New requests still check the original source and ancestor identities. Unsupported
sealing or executable-memfd policy refuses; there is no mutable-fd fallback.
Native ELF does not inherit the execution descriptor; a script retains only its
own sealed source descriptor for the interpreter.

The new service submission refuses on macOS with
`termination_native_image_unavailable`, before preparation or any new child.
Ordinary/native/replay paths are unchanged. The independent
`zerus-5wmh.66.3.40.4` follow-up must qualify observation of the same unreaped
helper's audit token and selected bundle requirement before this path is enabled.
The new Linux submission similarly refuses with
`termination_native_relay_unavailable` before preparation or any new child.
The separate `zerus-5wmh.66.4.6.9` authentication work must qualify the sealed
own-child relay against an immutable startup reference of the actual running
service image. The existing inode-based authentication is unchanged here.
Existing paired metadata and connected-peer code observations are not relabeled
as an execution fence. Platform component tests do not establish installed,
Root-authenticated or end-to-end controller qualification. Linux fake-actor
protocol tests explicitly construct their private fixture context rather than
passing through the production platform gate; they cannot enable it.
