# Admitted mobile task edits

2026-11-15, zerus-exooq. Extends the read-only phone-cache receipt contract in
[reachable task-writer cutover](2026-11-14-reachable-task-writer-cutover.md).
The coordinator approved the design in principle before implementation. This
document specifies the new lane; its existence does not enable a writer.
`TASK_CONTROLS` remains false.

## Covered routes

The first slice covers the everyday encrypted connection on both Android and
iOS: phone -> relay -> connector on the same Arch or Mac as the target `hgs`.
The relay still carries opaque encrypted requests. It needs no deployment or
new endpoint. The target connector verifies the enrolled Phone member and
executes the native operation locally.

The exact predicate is an encrypted v2 request whose `route_id` equals the
receiving connector's own device ID, whose `gateway_id` names that same
device, and whose protected `machine_id`/`computer_id` equals the native
machine observed by that connector. The ordinary local snapshot publishes
this binding. Android `EncryptedRelay.kt` and iOS `EncryptedRelay.swift` keep
the authenticated snapshot's route in their workspace journals and bind new
requests to it. `encrypted_runtime::receive_before` already distinguishes this
local route from a forwarded one. An arbitrary relay URL or display label is
not a source or target identity.

One-hop forwarding through a connector on another computer remains read-only
for sealed tasks in this slice. Legacy API v1 connections and downlevel hosts
retain their existing legacy task behavior and read-only schema-8 receipt.
There is no retry through plaintext, another route, schema 1 or an unqualified
writer after a new-lane refusal or unknown result. Advertising the new
capability requires the exact current native marker, the offered task exchange
and the qualified local encrypted route. It is scoped to that target, not a
union of a gateway's capabilities.

## Authority and independent native verification

The already authenticated v2 request is the source proof. A producer name,
phone-store ID, seal, public key, `writable` flag or capability field supplied
inside ordinary JSON grants nothing. The connector retains the original
encrypted envelope and its exact request identity rather than synthesizing a
claim after decryption.

A separate bounded native entry independently verifies that original envelope
against the target's own configured private workspace record. The request
cannot choose a trust-root file. Reuse the existing cryptographic format and
current-member/known-head checks: workspace, Phone role, device/key generation,
signature, recipient, expiry, route, physical machine and exact request/body
binding must all agree. A revoked or unknown source is refused before a task
write. The native operation also holds native project/store admission and
rechecks the admitted immutable pin, root and seal before committing.

There must be one custodian of workspace authorization across the native
commit. The connector currently holds its authorization file lock across a
native call; the new native entry must not recursively acquire that lock.
For this lane the connector reserves its existing permanent request identity,
then transfers responsibility to the independently verifying native entry:
release the connector's file guard before the native entry takes the same
guard and freshly verifies the original envelope. This gap confers no
authority. A membership change in it causes native refusal. Hold the native
guard through verification and commit. Recheck current authorization before
publishing the encrypted result. Keep the existing connector serialization and
permanent request/outcome binding; a dropped or unknown original is never
executed as a new request.

The ordinary phone exchange remains unqualified. It cannot import new
schema-8 task mutations merely because candidate bytes repeat a valid seal.
The qualified phone lane is distinct from native/swarm import admission;
native peers keep their existing current-source and pin checks.

## Host receipt and phone edit handle

Qualification starts with the host's existing admitted full-history task. It
cannot create a floor, make a replacement seal, add a participant or treat a
phone as a native producer. A held prepare, missing/replaced pin or store,
conflicting seal or unprovable history refuses that task.

The host issues a bounded source-specific receipt bound to:

- workspace and authenticated phone device/key generation;
- that phone's persisted replica-store incarnation;
- physical target node and native replica-store incarnation;
- project, immutable task UUID and original genesis/root;
- exact admitted immutable seal and complete baseline version;
- the original qualification request and a fresh retained nonce.

This receipt is delivered only in the authenticated result of that exact
request. The phone obtains the complete history through bounded document
pages; a shallow projection cannot qualify itself. The source stays frozen
while rows arrive. Metadata, snapshots, framing and assembly each retain their
existing explicit bounds. No lock spans a network request.

A distinct phone-edit handle consumes the authenticated receipt, not a public
deserialized `Producer` DTO. It checks project/task/store identity, full
intrinsic root/seal and that the host baseline covers every local causal
operation. An extra old local operation, including a stale same-peer prefix,
refuses qualification even if a later operation appears current. There is no
selective salvage, automatic rebase, new root or automatic overwrite of the
original local files.

Only edits made after that qualified baseline are eligible for this lane. The
handle retains the exact source, baseline, receipt and nonce with the edited
bytes. A refresh cannot retarget it to a different project, task, phone store,
host or seal. Persist the original pending edit identity/bytes before sending
them. A second explicit edit has its own identity; a lost result does not
silently repeat an edit or duplicate a comment.

The handle is a phone projection writer. It never writes a native `.cutover`
pin, prepares/adopts/exports/settles native custody, starts an executor or
becomes native authority when its folder is reopened. Plain phone-cache
handles stay read-only for schema 8. Unknown newer schemas stay read-only.
Schema-1 reads and the existing schema-1/2 legacy writer boundary are unchanged.

The first writer surface covers requirement text/media, target, board order
and comments through existing person/stage rules. It grants no new execution
Start, claim, resume, report or input authority. Existing refused stage and
execution transitions remain refused. Do not enable all task actions merely
because content editing is qualified; expose the corresponding scoped edit
capabilities to the clients.

## Import, outcomes and transfer bounds

The receiver requires the original authenticated source, retained receipt and
nonce, exact original source/target/store/root/seal and a complete validated
candidate before canonical import. It rechecks native custody under admission.
A supplied baseline is evidence to compare with retained authority, never a
new authority. Import must preserve the immutable root/seal and every existing
cutover/causal-history guard.

Keep API v1 framing, existing operation identities and the durable connector
journal. Carry the new exchange lane through versioned opaque update bytes,
using an explicitly authenticated mutation-class v2 request when it exercises
write qualification. Legacy exchange and media-frame classification is
unchanged. A serialized frame alone cannot create the authenticated native
context. No relay operation or server migration is needed.

Large edit bytes use the advertised bounded media-upload path: project/task/
hash/size/offset binding, one private bounded assembly slot, exact acknowledgements and complete verification before import. No partial task import. A
missing, changed or unknown acknowledgement cannot advance another draft.
An interrupted document transfer recovers only through the original document
sync/status path, never by replaying native input or inventing a new outcome.

Receipt and outcome lookups use indexed exact keys and bounded pages. Do not
scan complete task histories, workspace journals or all phone-store permits
on every action. Keep original uncertain records, and distinguish partial
transfer, verified refusal and unknown import outcome.

## Android and iOS

The same campaign supplies the FFI handle and thin wiring on both phones.
Neither client enables schema-8 edits from a schema number or display label.
Resolve the capability for the original connection and physical target, and
retain that source across every asynchronous read, picker, save and exchange.

A stale-local-extra refusal names the affected task, explains that the draft
was kept and asks for review on its computer. Preserve unsaved text and picked
bytes under their original project/task/draft identity. Do not silently discard
them after refresh or automatically apply them to a newer selection. Add new
English/Russian strings through the platform's localization resources.

Unsupported routes keep previews/read-only receipt and explain why editing is
unavailable. Both clients keep immutable session keys, exact current-label
resolution and the independent target-advertised action-schema negotiation.
Before a native action or releasing its unknown-outcome hold, require exchange
proof for the selected task and project. An unrelated damaged document may
remain a partial-exchange result; it does not authorize use of stale selected
task content.

## Qualification and review

Implement the spec, native/connector verification and FFI lane in one PR where
practical, then Android/iOS wiring in the next. Arrange early G1 review of this
contract and final review of the exact source. No hosted run is a gate.

RED controls cover a current admitted seal and the ordinary local encrypted
route on Arch/Mac, plus an unadmitted relay/serialized producer claim, wrong or
revoked source/key, wrong connection/route/machine/store/root/seal, a stale
local extra (including same-peer history), replaced native custody, shallow
history, missing or unknown receipt, duplicate original request and attempted
native prepare/export/dispatch from a phone handle. Exercise the existing
private large-media framing and byte-complete native-send controls after caller
integration. Kill a mutant for every new acceptance guard.

Run Arch Rust tests/strict Clippy and local Windows cross-Clippy for root
changes, Android unit tests/lint/unsigned assemble, and private headless Mac
Rust/ZerusKit tests plus an unsigned simulator build. Use only private stores,
profiles and sockets. No owner sessions, devices, signing, installations,
relay deployment or automatic task controls.
