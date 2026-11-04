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
New requests still check the original source and ancestor identities. Debug helpers above 256 MiB (including roughly 500 MiB debug builds) are
refused; shipped release helpers are far smaller. Hosted component fixtures use
a private small native executable rather than the large Tauri test binary. Unsupported
sealing or executable-memfd policy refuses; there is no mutable-fd fallback.
Native ELF does not inherit the execution descriptor; a script retains only its
own sealed source descriptor for the interpreter.

The macOS exception is confined to this service-native caller:
`Context::new` passes through `image::prepare` to `Image::selected`, which
requires the **same retained** `Arc<relay::Connection>` and its genuine paired
helper route. `Image::selected` reports `termination_native_image_unavailable`
for missing native selection or stale generation, and path-only Mac capture
remains unsupported. A broken pair can also refuse earlier at relay/preflight;
those existing failures remain closed. No direct or PATH fallback is admitted.

Every `Context::spawn`, including help/ABI, scope reads, retention and the final
native call, observes the same owned unreaped child with
`Connection::observe_original_helper` before any stdin is taken or written.
The kernel audit token, birth, selected signature/CDHash and full bundle
generation are checked by that primitive. Immediately after observation, and
again after the retention RPC's original-child registration, the caller checks
its **same** Service, configuration, epoch, generation, relay and cancellation.
Only a passing observation plus these current checks releases input. A refusal
closes stdin, permanently latches rejection and returns that same child to the
existing stop/reap/pipe/worker custody. It cannot become a successful receipt or
be cleared by another probe. The original resources survive outer timeouts.
This is a point-in-time observation, with a remaining exec window after its
last native check; it is not a constrained-spawn proof or new effect permission.
Ordinary/native/replay paths and the old Service fallback refusal are unchanged.

Actual installed-pair qualification remains separate under
`zerus-5wmh.66.3.40.4`: macbundle-u's next coordinated Mac window must use the
reviewed maintained signed pair and explicitly confirm Terminate with Remove
worktree for one approved disposable generated checkout. The exact original
confirmation/capture/run/conversation/request identity must correlate through
retention, original-child binding, native Terminate, verified process exit and
the actual Removed outcome. Retention alone, a primitive test or a protocol
fixture is not this positive. Preserve the branch/archive identity, settle the
original child and all pipe charges, and keep unknown outcomes original-ID
lookup-only. This result contributes to #1340's actual-caller gate; source
enablement does not flip the Service default or qualify the installed action.

On Linux, the service captures its actual `/proc/self/exe` object once under
its real owner, before publishing the listener. The retained sealed self-copy
uses roughly 37 MiB of resident memory per shipped Linux service. An owned setup worker copies
and seals that reference under the same 256 MiB/five-second bounds. Failure or
setup timeout leaves the sealed-relay branch unavailable for that service;
a late result is discarded only after the original copy worker joins, with
its owner loan retained until then. The installed pathname never refreshes
the reference. The existing inode-based authentication route is unchanged.

A sealed relay gains only an alternative image predicate. The kernel peer PID,
UID, original birth/direct parent, exact argv, selected native frontend,
configuration/mode, original request/attempt/fingerprint and ancestry/tracer
checks remain independent. The bounded client worker opens the peer's actual
`/proc/PID/exe`, requires a finite native image with all four seals and exact
length, and compares every byte against the immutable startup reference within
the original two-second proof deadline. A name, digest or supplied descriptor
cannot substitute. The acquired original object and owner loan stay with the
opaque native proof. Every subsequent freshness check, including the typed
after-BEGIN check, reopens the peer's kernel executable and verifies that SAME
sealed object and unchanged birth/parent. Exec to another memfd refuses even
when its bytes and displayed name match. These final checks do no full-image
I/O on the SQL actor. No equal image grants a role, consent or native effect.
Existing paired metadata and connected-peer code observations are not relabeled
as an execution fence. Platform component tests do not establish installed or end-to-end controller
qualification. The Linux CLI contract uses an original native fixture parent,
actual kernel peer/child/birth and the real typed actor; cached frontend proof
data is synthetic, and it is not an installed GUI/Terminate acceptance. Run its
positive cases outside agent ancestry. Linux fake-actor
protocol tests explicitly construct their private fixture context rather than
passing through the production platform gate; they cannot enable it.

### Refused removal availability

G1 decision: the old Service `selected:None` path is not a safe fallback.
It reselects `default_hgs` and does not retain the selected relay/image/config
through the original effect. Its refusal remains intact. While the new submission
is unavailable, Terminate visibly explains that automatic folder removal is
unavailable in background mode. The ordinary guarded Terminate remains available;
the separate manual folder review opens its existing native guard flow without
implying termination success, cleanup consent or replay. All offer errors are
visible. Native caller/Terminate qualification remains a separate release gate;
source enablement alone does not qualify the installed action.

Design coordinator sign-off: the unavailable-removal notice and “Review working
folder…” button are the interim Terminate-dialog variant, replacing the checkbox
only while removal is unavailable. The design/webui mockup needs a matching
variant later; the coordinator owns that follow-up. Manual review loads only on
its explicit action, so importing the action menus does not initialize its
native event subscription. The original computer/path are copied before the
asynchronous import and checked independently by the existing native review.
