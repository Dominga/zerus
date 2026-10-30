# Frontend producer admission and current drain

This is the implementation contract for G1 `zerus-5wmh.66.3.23`. The existing
delegation hello, passive feed, Staged publisher and revoke receipt do not
implement it. There is no positive producer/drain command or capability yet.
Do not invoke proposed commands against an older selected hgs.

G1 owns the authenticated control connection, current runtime proof, durable
handoff journal, marker phases and effect admission. G3 owns existing source
decoders, bounded Bundle construction and source identity rechecks. G2 owns
operation policy. Existing settings, layout, drafts, cache contents and native
recovery/voice services retain their stores and owners until their specific
handoff is implemented and qualified.

## Versioned ownership profiles

The coordinator's 2026-10-30 ruling admits per-role handoff. The initial fixed
`g1-background-v1` profile contains Fleet, Attention, NativePosting, Badges,
ReadDelivery and Presence. Attention, NativePosting and Badges always move
together. AutomaticApprovals remains GUI-owned and off until its own later
qualified profile; it is excluded from v1. Operations, Drafts, Providers,
Downloads and Cleanup also retain their current GUI owners. G2 Operations
requires its own profile, current drain and the G1 .22 execution guards.

The hgs journal binds the profile ID, its hash and every role's owner and
permissions. One journaled transition moves the entire profile after fresh
drain of every member. Revoke and rollback return that whole profile to its
previous GUI owners in one transition, never role by role. Failure before a
completed move leaves all previous logical owners intact and admissions
fenced until exact reconciliation; it never automatically reopens a gate.
A partially moved profile is never a resting state.

The marker payload stays unchanged and grants no role permission by itself.
Its internal Complete phase records only the journal-bound profile's handoff.
Internal descriptions and status say **g1-background-v1 handed off**, list
service-owned and GUI-owned roles, and never imply G2 or Qt retirement is
complete. Live legacy Qt or an unknown owner of any required role refuses
handoff. G3 captures all families required by that profile; unrelated history
remains Staged. These rules still need executable integration and qualification.
Because ImportRecord evidence applies to the entire record, out-of-profile
families use separate immutable Staged bundles and import UUIDs. A Drained
record may contain only the selected profile's exact required family allowlist.
Original shared-store bytes can prove source hashes, never additional role
authority. The allowlist and profile digest bind the initial Drained receipt.

## Identity and bounded control

The future exact help line is `Desktop producer ABI: desktop-producer-v1`.
The selected local CLI and configuration must be pinned and revalidated before
any new command. The proposed `hgs desktop producer --json` is a single owned
stdio proxy to the existing same-user desktop-v1 endpoint, never a second
daemon or fleet reader. The service admits one producer per configuration;
the caller's UUID alone proves nothing. A missing service creates no state.

Admission binds the canonical configuration hash, production application
`com.hgdev.hgs-tray`, clean build identity, live/non-replay mode and exact
frontend instance UUID. The service supplies a fresh challenge UUID and its
service epoch. The frontend's background controller installs that exact
challenge in a single cached slot in its canonical instance responder and
acknowledges readiness over the producer connection. The service then probes
the canonical endpoint off its actor thread, with a fresh challenge on each
admission. Only a reply from the same OS user, matching the pending challenge,
build, configuration and instance, can bind the connection. The challenge is
one-use, expires after ten seconds, and is never an effect permit.

The additive query `desktop-producer-v1 UUID\n` fits the existing 64-byte
instance line. Replies remain bounded to 4096 bytes and one total one-second
socket deadline. The ordinary delegation-v1 reply, show/sessions activation,
canonical socket ownership and updater discovery stay unchanged. No per-probe
child or GUI-thread I/O is allowed. Absent, legacy, dev, replay, stale,
mismatched, late, unsolicited and oversized replies stay passive. A connected
older app ignoring the query is live unknown, never proof of absence.

The cached responder now implements this query, but no production controller
installs a challenge and no producer command/help capability is advertised.
Its closed reply contains `abi`, `version`, the unchanged delegation `hello`,
an optional `challenge` (nonce, service epoch, producer epoch), and optional
`read_delivery` diagnostics. An unmatched query returns diagnostics with no
challenge and does not consume the pending offer. A matching query consumes
the offer once; it expires at ten seconds and cannot be replaced while live.
Binding requires the exact pending challenge and the previously observed
frontend build/configuration/instance identity, not merely a valid reply.
The receiver must also retain and check its own monotonic pending deadline;
an expired reply cannot bind. Fresh challenges always have new random UUIDs.
The ordinary delegation reply has no added fields.

Control messages use a closed versioned schema, canonical UUIDs and at most
16 KiB per line. There is one outstanding request and one response per producer,
with no private bodies in control or status. A server-generated producer epoch
identifies this connection. It is distinct from the frontend instance UUID,
service epoch, prospective writer epoch, drain UUID and import UUID. An exact
duplicate is status-only; it never repeats a freeze, capture or effect.

## Closing admissions and retaining resources

The control order is bind, durably prepare handoff, close frontend admissions,
stop/cancel owned work, verify actual resource completion, flush the existing
stores, and only then begin a fresh source capture. A prepared handoff must be
visible to a replacement frontend before it admits writers. Service death,
connection loss or a missing reply must never restore legacy writers while a
handoff's outcome is unknown.

Use explicit fixed roles, with no free-form role names or per-session cache:

| Role | Resources that must end or cross an ordered durability barrier |
| --- | --- |
| Fleet | Existing shared supervisor, workers, connections, readers, queued callbacks and source generations; no parallel reader starts |
| Attention/native posting/badges | Detector inputs and timer, queued native calls/results, native connection/executor, persistence and badge offers; stale clicks are fenced |
| Read delivery/presence | Read admissions and owned children plus stdin/stdout/stderr, pending-mark writer, presence samples and writes |
| Creation/task/session operations | Existing queue admissions, workers, callbacks, journal writes, frozen original input and unknown outcome records |
| Draft/provider operations | Composer and GUI voice jobs, manifest writes, submitted/unknown callbacks and provider operations; native recovery/voice services are not stopped |
| Download/cleanup operations | Transfers and their callbacks, cache worker writes, the owned trim timer and exact cleanup outcomes; no content removal or replay during capture |
| Automatic approvals | Discovery/admission, private audit reservations and native-input workers; remain off throughout migration and require separate new local consent |

These are ownership groups, not a claim that the present GUI can drain them.
An uninstrumented or unsupported required role refuses the handoff. Partial
coverage never authorizes that profile's handoff or another profile's roles.
UI-only preferences may remain client-owned; their source snapshots still
require exact fresh rechecks and cannot prove operation quiescence.

Core's `desktop::drain::Gate` provides only atomic close and bounded lifetime
accounting. It holds twelve fixed role counters and at most 1024 resource-group
admissions (the host may choose a smaller cap). Admission and close share one
lock. An already admitted resource stays charged until its last guard is
dropped, including copies retained by queued callbacks,
the owned child, input writer and both output readers. Cancelling or returning
from a bounded join does not drop those copies. Pressure refuses new admission;
unknown pending resources are never evicted. Poisoned accounting stays unknown.
`Closed::wait_until` bounds waiting with an absolute monotonic deadline; a
Pending result keeps charges and admissions closed. A gate with zero charges
does not attest that all writers were instrumented, that stores flushed successfully,
or that the service granted any authority.

The frontend's integration must stop each role's producers before waiting on
its charges and must execute ordered store barriers after all late callbacks.
An exit flush, timer, timeout, claimed count or imported record is insufficient.
The existing Exit versus updater Quit ordering remains unchanged. No gate close
automatically signals an agent, process group or unrelated service.

The read-delivery integration retains its existing one loop and four delivery
slots. App state holds a dormant `ReadDelivery` control handle; only a future
explicit stop closes that loop's gate, wakes it and cancels its owned reads.
There is no producer-control caller yet. Each callback and its actual child,
stdin writer and output readers retain the same admitted group. A returned
outcome releases neither a blocked pipe nor its capacity. The selected read
argv, payload, local/peer deadlines, settlement and backoff stay unchanged;
stdout is now bounded to 1 MiB. The separate pending-mark store can still accept
writes, so delivery Idle is not a complete Read role or handoff proof.

Four fixed resource slots now retain that same child/input/output/callback
charge. If any copy survives the callback, one bounded log line names only
`role=read_delivery` and `slot=0..3`. Slot release still requires the last
resource guard; diagnostics never reclaim it. Cached health contains a
monotonic revision, capacity 4, in-flight and pinned counts, and stopped state.
The existing notice bus warns once per app instance when all four slots pin.
This is a delivery-stall notice, not an individual read success/failure notice.

`hgs desktop status --json` reports `frontend_read_delivery`: the matching
frontend instance and its read-delivery counts, or explicit absent/unknown
coverage. With a service, one off-actor probe supplies a bounded cache, spaced
at least ten seconds apart and stale after fifteen seconds. A blocked probe
cannot stack workers. Without a service, status makes one read-only canonical
probe; it never creates desktop state. A legacy reply, invalid counts, missing
identity or unsupported platform supplies no health. This field is GUI-owned
resource visibility and always has `authority:false`; it is not the service's
read queue, a complete role drain or a takeover permit. An orphan remains
charged until its actual holders exit, or until that GUI process is restarted.

## Fresh capture and publication boundary

G1 supplies G3 an opaque, non-serializing capture context only after the exact
instance's required roles are closed, resources ended and barriers succeeded.
Its closed wire binding contains version 1, configuration, service epoch,
prospective writer epoch, producer epoch, frontend instance UUID, drain UUID,
import UUID and a digest of the fixed required-role profile. G3 neither mints
nor renews this binding and never treats its serialization as authority.

The context remains bound to the live control connection. Heartbeats every
three seconds have a fifteen-second idle limit and a ten-minute absolute limit
from drain preparation. Slow drain has a bounded 120-second attempt; timeout
is unknown, never a success. Expiration or EOF fences G1 activation and new
publication. A frozen frontend remains closed until an exact durable abort or
handoff result is reconciled; it never resumes because a lease expired.
Even after a verified abort, an old Gate stays closed. Any replacement requires
a new explicit ownership decision and a distinct gate; old callbacks retain
their original charges.

After drain, G3 invokes the existing source readers anew, creates an untouched
Bundle with the retained import UUID, and performs each source's fresh identity
and byte-hash recheck before the final record. A pre-drain Bundle, a previously
published Staged import and a historical source phase cannot be promoted.
Memory-only rows/outcomes require a bounded frozen runtime export from their
existing owner; they do not acquire invented source files.

The ordinary Publisher remains Staged-only and unchanged. A future separately
advertised drain-publication mode consumes a fresh Bundle under the exact
capture context, using the existing bounded import framing and limits. G1
alone verifies the current control/role proof and constructs the initial
immutable Drained record during that new publication. It never edits or
relabels an existing Staged record. G3 validates the exact derived receipt,
record digest, configuration, import identity, profile and complete original
source/page/snapshot membership, digests, lengths, counts and fixed references;
lost delivery uses only retained-ID status, never an automatic new capture.

Slow input precedes the short publication lock. G1 verifies the capture again
under handoff.lock before immutable commit and before any marker transition.
Revocation fences new admission first, cancels publication, waits for its real
resources, and checkpoints before settling its receipt. A partial publication
or disconnected producer never authorizes a writer. Final Complete and any
later effect still need fresh canonical-owner checks, the held ownership locks,
current epochs and the exact effect journal. Automatic approvals remain off.

## Implementation and proof order

1. Agree this contract and qualify the fixed-size close/lifetime gate, without
   positive IPC or authority.
2. Add the authenticated one-producer control and durable prepared/frozen/abort
   journal, with replacement-frontend startup refusal for an unsettled handoff.
3. Instrument and qualify actual role drains and store barriers. Missing roles
   keep production admission refused, including newly added GUI workers.
4. Connect G3 fresh capture and initial Drained publication, then qualify marker,
   revoke, reconnect, restart and lost-result ordering before writer takeover.

Each slice is a separate reviewable PR. Private fixture tests must cover close
versus concurrent admission, retained callbacks/pipes, failed barriers, frontend
replacement, service/producer loss, stale epochs, capture expiry and partial
publication. Socket/setup changes additionally need a built-app private D-Bus
and Xvfb smoke preserving Show and --sessions, with no second instance. Native
Windows custody and production acceptance remain separately unqualified.
