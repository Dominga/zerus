# Teams person decisions from a window gesture

Date: 2026-11-15. Status: design for coordinator selection and G1 review;
no implementation is included. Tracking: `zerus-63rru.31`, with the
`zerus-ppd26` audit as supporting evidence.

Read against main `9a2fa6df16d5ff7bf6166dab530c51b52d06eaab`.

## Decision and scope

A new decision exercising a person's Teams authority must originate in an
explicit confirmation in the native Zerus window. A CLI classification of
`Person`, a JSON `confirmed` value, an unmarked process, an SSH login, an
existing receipt and a saved database actor cannot issue that authority.
Reuse the native desktop's original-child request admission, with a closed
Teams request and a typed, consumed permit. Keep the existing team policy,
revision checks, exact session bindings and service authority underneath it.

A command acting as a proved team lead/member keeps its existing permitted
operations and delegation checks. A terminal command that would act as a
person becomes read-only unless a later slice implements confirmation in the
window. This design grants no new standing authority to agents or services.

This closes the product-entry-point bypass class recorded in `.31`. It is
not protection against unrestricted malicious code running as the account
owner; the trust boundary and its limits are explicit below.

## What the current code establishes

These are source reads, followed through the write or returned value; they
are not new runtime qualifications. The retained source capture and hashes
are in `artifacts/test-results/wp31/` in the design worktree.

| Path | Current result and proposed reuse |
| --- | --- |
| `src/teams/caller.rs:150`, `:204`; `src/teams/cli/actor.rs:27` | An unmarked command with no detected agent ancestor becomes `Caller::Person`, then `Actor::Person { origin: CliInterim }`. The source documents deliberate detach, hidden/unregistered host and state-root limits. It is an accidental-use guard. |
| `src/teams/ops/authority.rs:6`; `src/teams/ops/apply.rs:11` | Policy checks the Person variant and ownership, not a live gesture. `apply` locks with `BEGIN IMMEDIATE`, verifies actor, checks exact saved actor/request replay, authorizes, changes state/revision and saves the receipt in one transaction. Keep that kernel, but require a typed admission for a new Person mutation before first-store creation or effects. |
| `src/teams/mod.rs:108`; `src/teams/cli/catalog_cmd.rs:28` | Preview and catalog/template changes have separate writers. Changing only the operation policy leaves these outside the new boundary. Preview ultimately writes/removes its marker; catalog import ultimately publishes an atomic replacement. |
| `src/teams/tasks/intake.rs:18`, `:64`, `:293` | Task intake separately maps a non-agent task actor to `CliInterim` Person. That changes automatic admission and ultimately calls `ops::apply`. It must not provide a second way to exercise Person authority after the Teams CLI is fenced. |
| `client/src-tauri/src/teams.rs:179`, `:199`; `client/core/src/teams/action/wire.rs:35` | Today the window prepares an ordinary `hgs team` invocation. `TeamCall.confirmed` is a presentation/confirmation-flow field. It is not the native frontend admission described below. The explicit Again path can resend an uncertain ordinary command; migrated Person actions must use receipt-only recovery instead. |
| `client/core/src/desktop/ui_requests.rs:17`, `:377`, `:511` | The native responder registers an exact closed request in a bounded in-memory cache (8 slots, 10-second lifetime). It refuses agent-marked/non-live/replay/noncanonical application contexts, binds configuration and native mode, and assigns a native serial. A nonce is only a lookup key. |
| `client/src-tauri/src/desktop_service/creation_clone.rs:136` | Send, answer and creation requests share `Read::exchange`: register the exact request, spawn the selected `desktop frontend-request --json` original, bind its PID/birth before sending private stdin, and retain the registration and original resources until settled. |
| `src/desktop/mod.rs:1865`; `src/desktop/ui_authority.rs:1259`, `:1416`, `:1505` | The service authenticates its kernel peer; checks the selected relay image/argv, canonical window image/configuration, direct original-child relation, birth, exact request ID/fingerprint and attempt; and rechecks freshness. It creates an opaque `Authorized`, not an actor named by JSON. |
| `src/desktop/ui_authority.rs:1205`, `:1805`, `:675` | Consuming `Authorized` checks the original identities and native serial/order. The resulting non-Clone/non-Deserialize `Validated` checks its original two-second deadline again when consumed at the writer boundary. |
| `src/desktop/deferred_send/runtime.rs:291`; `src/desktop/deferred_answer/runtime.rs:293`; `src/desktop/creation_launch/runtime.rs:171` | Each actor consumes its typed permit after the database writer lock, checks the exact scope/lease and durably admits the original. Only first admission retains live consent. Rows or recovered receipts cannot reconstruct it. |
| `src/cli.rs:1914`, `:1967` | Remote `team` commands have a source-side marker/ancestry refusal because SSH loses caller provenance. Retain it as defense in depth; target-side admission must independently refuse a Person write without a target-window permit. |

The Send/answer action supplies its reserved checkpoint and exact input to
the native route; creation uses the native host's one-use launch consent
(`client/src-tauri/src/lib.rs:1025`, `:1095`; `creations.rs:1084`). These
mechanisms prove origin in a particular native action. They do not attest a
hardware input event. The Teams issuer below must add an explicit native
review/confirmation and must not treat a renderer Boolean as that event.

The current macOS deferred-send/answer actors still have their own early
native-image refusal (`deferred_send/runtime.rs:306`,
`deferred_answer/runtime.rs:308`). Reusing the admission pattern does not
remove those fences or qualify an unavailable execution path.

## Threat model and limits

The in-scope adversary is a same-user agent calling supported Zerus commands
or IPC while attempting to obtain Person privileges. It may remove every
`HGS_*` marker; double fork, use `setsid`, leave an orphaned background job or
re-exec after detach; delete/repoint its own host records; set `HOME`,
`HGS_STATE_DIR` or `HGS_CONFIG_DIR`; choose an operation/request UUID; replay
or alter a captured JSON envelope; or call `hgs @host`. None supplies a
window registration, native confirmation or a live consumed permit.

The installed window, service and selected native helper and their runtime
code are trusted, as are the OS peer/process/image observations on which
existing desktop admission rests. The person reviews native text and makes
the final affirmative choice. Merely opening a dialog grants nothing.
Starting a fresh legitimate window with cleared markers also grants nothing
until that confirmation. Native review text must be rebuilt from canonical
bounded data, with untrusted team/card text visibly delimited; it cannot be
an attacker-supplied prompt that substitutes another operation or computer.

This mechanism cannot distinguish a physical person from privileged GUI
input synthesis or an attacker controlling the trusted window/renderer,
service memory, installed binaries or canonical stores. Same-UID file modes,
a socket secret, a signature whose key that same user can read and code
signing alone do not solve that problem. Arbitrary direct database/config
writes, debugger injection, executable replacement and rollback to an old
writer are outside this product-entry-point boundary. Strong isolation from
all same-user code would require an additional OS/security-principal or
hardware-backed user-presence boundary and a separate owner decision. Do
not label the proposed mechanism such an isolation boundary.

No qualification may bypass a caller-proof refusal by invoking another
service manager, account, detached helper or GUI helper from an agent. The
existing private ppd26 attack evidence supplies the motivation. Future real
attack tests need an explicitly controlled harness with a service minted by
product writers; a refused probe remains refused.

## Proposed native admission

### The reviewed operation

Introduce a versioned closed `TeamsPersonAction` in the existing frontend
request family, plus typed native issuance and consumption methods. Do not
add a generic `run command`, serialized Actor, bearer grant, `--as-person`,
`--confirmed` or environment-variable authority path.

A native draft pins all of the following before confirmation:

- native target computer identity, canonical configuration/state-store
  identity and service/writer epoch; an existing store's nonce/generation;
- Person owner identity and exact operation variant;
- team UUID and expected revision; for Create, the new team UUID, absence
  precondition and revision zero;
- original request UUID, canonical payload digest and immutable native draft
  identity; every value affecting the decision, including settings,
  delegation flags, dependency edges, member/node/card IDs and decision;
- the pinned template/catalog revision and bundle digest for creation, or
  the expected preview/catalog state for their separate writers;
- for repository-changing decisions, the reviewed repository object,
  branch/ref and expected source/base identity, not a path or team name alone.

Use the current canonical `Request` encoding for equality/digesting rather
than inventing an alternative JSON normalization. Store identity creation
must happen only after admission of the explicit creation action; a missing
store is not a reason to mint Person authority or a nonce from CLI input.

The window reads current facts, creates an opaque draft, and shows the exact
operation, team/computer and consequential changes in a native confirmation.
Accept/Cancel must come from the native dialog callback. A renderer action
can request that review, not claim it succeeded. On Accept the host consumes
the draft once, checks that selection/revision/digest remain current, and
registers this exact original request. On Cancel, expiry or changed facts it
registers nothing; a fresh review is required. Do not grant a reusable
"approve this team" capability.

Reuse the existing 8-slot/10-second registration and two-second authenticated
writer-boundary freshness; no renewal after queuing or lock contention. A
new confirmation is needed after expiry. Keep exact original Child custody,
birth/image/configuration checks and native serial retirement. Add a closed
Teams ordering category; out-of-order proofs refuse, and every admitted
operation still rechecks its pinned revision under its own writer lock.

### Consumption, commit and recovery

`Actor::Person`/`PersonOrigin::DesktopUi` remain audit descriptions, never
constructors of authority. Factor the mutation kernel so a new Person path
requires a non-deserializable, non-cloneable `TeamsPersonPermit`. The ordinary
CLI cannot construct it. Session and Service entry points keep their existing
proofs and cannot borrow it. Require it at the common write boundary, not
only in the Tauri handler or command parser; audit all production Person
constructors, including task intake. An ancestry-derived reader must never
fall back to this write path.

For Teams SQLite decisions, after acquiring the existing writer lock:

1. Recheck and consume the live exact permit, computer/config/store/epoch and
   payload identity. No first-store initialization before this admission.
2. Look up the request UUID. An exact settled duplicate is only its original
   receipt; a changed actor/payload/scope is a conflict. Never execute a
   duplicate because its prior permission was once valid.
3. For a new request, check team ownership, expected revision, operation
   policy, current card/node/delegation and repository preconditions.
4. Commit the admission's identity/audit evidence, state mutation, revision
   and original receipt in one transaction, preserving existing savepoints.
   A refusal is explicit; it cannot later turn into an admitted original.

Reserve a unique durable original before any asynchronous or external effect.
The live permit may hand that original once to its exact existing executor;
a database row, recovered receipt, elapsed timeout or new process cannot
reconstruct live consent. Persist Unknown before losing custody. Recovery is
read-only: return the exact receipt or Unknown, never resend a team command,
re-run a Git action, auto-approve a card or select a new request ID. If an
operation needs later work, distinguish the person's recorded decision from
the service's existing independently authorized effect/fact; that fact never
becomes Person authority. The common actor must not block all teams while a
person considers a dialog or a helper drains.

Preview and catalog writes use their own locks/publication protocol, not an
assertion of atomicity across unrelated databases and filesystem rename.
Extend the existing catalog editor's durable uncertain-before-publication
receipt (`catalog/editor.rs:153`) to bind gesture/scope, and route legacy
save/import/remove through that same guarded writer. Give preview changes a
bounded durable reserve/settle record before marker replacement/removal. On
restart an uncertain publication remains uncertain and never re-publishes.
Duplicate observation reports the original result, not whether the current
marker happens to match. No permit can authorize another canonical root.

Keep bounded indexed lookup by request UUID and bounded pending capacity.
Never prune spent identity into permission to re-admit; a retired namespace
must fail closed or keep a durable spent-ID fence. Do not scan full request
journals on an action or accumulate an unbounded GUI intent table.

## Terminal and remote behavior

The initial safe behavior is explicit: Person mutations from ordinary local
or remote CLI return `person_gesture_required` before a write or effect,
with a message directing the person to Zerus on the target computer. Reads
keep their current paging and actor/access checks; this does not make
member-restricted/private catalog reads public. Proved lead/member mutations
remain permitted only by the current policy. No request UUID is silently
changed to make a refused/unknown original succeed.

A later optional confirm-in-window command submits only a bounded untrusted
proposal. It returns a proposal identity/read-only outcome handle, never a
permit. It neither auto-launches a program that executes a decision nor
opens an unlimited prompt stream: explicit proposal admission, bounded
pending capacity and expiry are required. The person opens the target
window, reviews canonical facts and confirms there. The window executes its
own exact original; the terminal observes the result. With the window closed,
headless, locked, stale or unavailable, nothing is approved. A proposal
cannot be approved by replying `y` in stdin or by passing a saved receipt.

For `hgs @host`, preserve existing source-side agent refusals and native
machine binding through aliases, references and `--expect-machine` wrappers.
The target enforces the new boundary independently, including for an older
source CLI that never sets markers. Local-window proof is not forwarded over
SSH and cannot be substituted for target-window proof. Initial slices provide
remote reads under existing policy and refuse Person writes; confirmation on
the target window is the later remote story. A source-window confirmation
for a remote decision would need a separately reviewed target-issued protocol
bound to both computers and the exact target writer. SSH identity alone is
not evidence of a person's gesture.

## Operation coverage and migration

Ship each slice fail-closed, with its matching window path and refusal in
every ordinary CLI/IPC writer. Do not leave a compatibility switch that
re-enables `CliInterim` mutation authority for a migrated operation. Partial
migration must state which remaining operations still have the `.31` limit.

| Surface | Required boundary |
| --- | --- |
| Preview on/off | Native confirmed setting plus exact prior state/computer. Status stays a read; refusal while preview is off creates no Teams files. The admitted setting may use the existing desktop control ledger. |
| Catalog/template import/save/remove and creation | Native exact revision/content and template bundle; guard legacy writers and first-store creation too. Reads and explicit exports do not gain mutation authority. |
| Settings/delegations, owner-card decisions, dependencies, membership, name/goal, Stop/Archive team/member | Typed permit for every new Person decision, not just affirmative approvals. Bind exact team/revision/card/member and decision content. |
| Person alternatives on Pause/Resume, route/admit/hold/release/return/reassign/cancel, draft phases, Accept/Reply/AddTask | Require a permit when acting as Person; keep independently proved lead/member authority and existing delegation/restriction checks. Fence task intake's implicit Person admission before task/document writes. |
| `advance_base` and phase acceptance | Apply this same rule when these writers land, including exact reviewed ref/base and phase generation. This main revision has no `advance_base` operation; approval availability for phase drafts is explicitly false. Do not invent a current CLI entry or treat future operations as implicitly authorized. |
| Service observers, standing confirmations and task/session execution | Keep current narrow Service/session authority. They do not turn into Person actors and cannot refresh a Person permit. Existing standing-confirmation scope is unchanged. |

Ancestry, markers and hosted per-call proof remain defense in depth and still
identify Session actors; their refusal never becomes a Person grant. Keep
`CliInterim` and old receipt actor encodings readable for historical audit.
Read-only original-receipt lookup must compare the stored historical actor
and content without reauthorizing a mutation or upgrading origin. New
`DesktopUi` receipts need not pretend to be old CLI receipts.

Advertise a new Teams Person-admission capability/version. A new desktop
requires that exact capability before offering a migrated mutation. An old
client/peer can read compatible projections; its migrated write is refused
with the upgrade/window reason, never an ordinary CLI fallback. A new client
must refuse an older target/service lacking the capability. Existing preview
state and team/catalog data are retained, not reset to obtain a new proof.

Old executable versions writing files/SQLite directly cannot be controlled
by a new service handler; deploying current writers is an operational
prerequisite, and arbitrary same-user store writes remain outside scope.
Do not claim a new on-disk marker makes an old program enforce this rule.

## Qualification plan

This is design-only: the following are requirements for the implementation
slices, not tests or mutants executed by this note. Build fixtures through
product catalog/team/task writers and the native issuer. Name any unavoidable
handwritten malformed row as such. Never inject `Caller::Person` or a hand
constructed Actor as the success evidence for a gesture test.

- Through real CLI/IPC entry points, refuse Person mutations with ordinary
  markers, cleared markers, a double fork/`setsid`, orphaned background job
  and detached re-exec. Run under a private controlled fixture; if caller
  proof refuses, do not evade it with another route. Keep the supported
  DeepSeek per-call Session identity controls and ppd26 legacy-host/no-record
  and wrong-launcher-attribution shapes distinct.
- Redirect HOME/state/config and remove fixture host records: a fresh fake
  store or missing agent record cannot issue a window permit. Prove no
  canonical or redirected mutation, settings marker, task/document write,
  launch, signal or Git ref update occurred.
- Forge `actor`, `origin`, `confirmed`, PID, serial, attempt and fingerprint;
  copy an envelope to another child; replace PID/birth or helper image;
  expire its slot, change canonical window/configuration/store nonce or
  service epoch, or move it to another computer. Each must refuse before
  effects, using bounded failure paths.
- Positive native control: one confirmed exact action changes only that
  target once. Cancel and stale revision/draft/card/template/ref do not write.
  Change the payload under the same UUID and require conflict. Send an exact
  duplicate and observe only the original receipt; different teams and
  independent agent/service actions keep their current permissions.
- Hold the writer lock until the two-second proof expires, crash before
  admission/commit and around external-effect reservation, lose the reply,
  restart the GUI/service, and repeat Again/status/lookups. No expired grant
  is renewed and no Unknown original is replayed. Catalog/preview publication
  cuts preserve Unknown without overwriting a later person's change.
- Exercise direct local, plain `@host`, reference hop, `--expect-machine`,
  older-client and unsupported-service routes. A source-side refusal performs
  no SSH; target-side refusal works even when the source supplies no proof.
  Returned terminal/remote handles are incapable of executing a write.
- Audit every current/future Person writer, including `hgs task --team`,
  preview, catalog and first Create. Keep bounded requests/pages, stable
  request identity and private fixtures on Arch and a headless Mac. A native
  platform refusal is not a passed positive control. Hosted CI is not a gate.

At least these six independent production mutants must be killed in the
relevant implementation slice: (M1) restore `CliInterim` write admission;
(M2) omit exact operation/payload digest binding; (M3) omit target
computer/config/store binding; (M4) omit original child PID/birth or native
cache/serial check; (M5) consume proof before the writer lock without a final
freshness check; (M6) reconstruct consent from a saved/Unknown receipt or
re-send an uncertain original. Add a seventh for bypassing the task-intake
or preview/catalog boundary once that surface migrates. Compile failure alone
is not a killed authority mutant.

## Smallest safe slices

1. **Closed admission plus owner-card decisions.** Add native immutable
   draft/confirmation, closed request, typed kernel admission and durable
   original/receipt-only recovery. Wire the real window's `decide` action;
   refuse new unproved Person `decide` from CLI/IPC. Preserve other operations
   explicitly as interim until migrated. G1 reviews exact scope, freshness,
   duplicates and failure cuts before merge.
2. **Remaining team decisions and intake.** Route Person alternatives and
   person-only team mutations, Create and task intake through the same gate;
   prove genuine Session/Service paths unchanged. Migrate the window actions
   and remove uncertain Person resend in the same slice; split by operation
   families only if every intermediate release accurately states its limits.
3. **Preview and configuration writers.** Native confirmed preview and
   catalog/template edits with their writer-specific durable outcome fences;
   cover legacy import/save/remove and root-redirection controls. No claim
   that `.31` is closed before all Person-authority writers are covered.
4. **Optional terminal proposal workflow.** Bounded explicit review in the
   target window and receipt-only terminal observation; remote proposals only
   after their target-bound protocol is separately approved. Initial CLI
   refusal is already a complete safe terminal behavior.

Every authority-changing slice receives G1 review and local qualification.
The coordinator chooses slices; this note authorizes no implementation or
installation and changes no current Person/session/service behavior.
