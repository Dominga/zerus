# Session creation progress in the session list

Status: Accepted for desktop implementation, 2026-10-26. Design task:
zerus-hvsz; implementation: zerus-5371.

After the final **Start session** or **Clone and start** action, close the form
and immediately show a selectable creation card in Sessions. Folder checks,
cloning, worktree creation and agent startup continue outside the dialog. Keep
progress, errors and recovery actions on that card until the result is resolved.

This proposal covers the desktop workflow and the native contracts it needs.
Android and iOS should eventually present the same lifecycle through their
existing durable action journals. They are separate delivery scopes; a desktop
change must preserve API v1 and older clients.

## Behavior before this change

- `NewSessionDialog::start` checks the directory before emitting
  `launchRequested`. Auto worktree creation and repository acquisition are also
  owned by the dialog. `reject` refuses to close it during worktree creation or
  cloning. This explains the modal state in the reported screenshot; it does
  not establish why that particular Git operation was slow.
- `SessionsWindow::showNewSession` creates and saves a pending launch only when
  `launchRequested` arrives. Its existing **Starting…** card therefore covers
  agent startup, but not the earlier preparation. It is not selectable, is
  limited to some filter combinations, and disappears on a launch failure.
- `HgsClient::createWorktree` generates a UUID inside the call and waits up to
  330 seconds for its final response. Native desktop worktree creation has a
  315-second total deadline, including up to 300 seconds for checkout.
  `src/state/worktrees.rs::create` echoes `--request-id`; that function does not
  implement a durable request lookup or duplicate-result replay. Repeating this
  command with the same UUID is not a recovery protocol.
- `RepositoryClone` already saves an original clone UUID before dispatch and
  supports receipt-based **Check result**. Its desktop settings retain one
  pending clone per CLI/computer key. The new workflow must retain independently
  addressable operations rather than overwrite this slot.
- `src/state/launches.rs` records started and early-ended launches. Existing
  launch IDs and `ended_launches` are valuable reconciliation evidence, but
  presence of a launch UUID alone does not prove that every backend can safely
  replay a launch command.

Relevant sources: [NewSessionDialog](../../../tray/src/NewSessionDialog.cpp),
[SessionsWindow](../../../tray/src/SessionsWindow.cpp),
[HgsClient](../../../tray/src/HgsClient.cpp),
[RepositoryClone](../../../tray/src/RepositoryClone.cpp),
[native worktrees](../../../src/state/worktrees.rs),
[launch evidence](../../../src/state/launches.rs), and the existing
[launch and repository contracts](../../reference.md).

## Admission and immediate feedback

Keep local field validation and existing explicit confirmations before
admission, including the Windows session-host warning. Opening the form,
browsing a folder or changing a selection never creates anything. Preserve
ordinary folders, Auto, None, existing worktrees and explicit custom worktrees.

On the final action, capture an immutable snapshot and allocate one operation
UUID. Close the form and publish the card in the same GUI event turn, before
network access or a durable write can delay it. Initially it says **Saving
request…**. Save the request on the ordered background storage worker and wait
for its durable acknowledgement before dispatching any mutation. A save failure
leaves a visible **Could not save request** card with the original form values;
no clone, worktree or agent may have started. A crash before that acknowledgement
may lose an unsaved card, but cannot leave an unrecorded native mutation.

The snapshot includes the owning computer identity, CLI compatibility evidence,
agent and exact account binding, session name, requested project and explicit
versus inferred placement, source/destination, worktree mode/branch/base,
model/effort and terminal preference. Resolve a symbolic base such as HEAD once
at the native preparation boundary and retain its verified commit for recovery.
Never silently substitute another account, agent, computer or working folder.
If native validation proposes a materially different destination, show that
proposal for explicit review before reserving or creating it.

Repeated click, Enter or delayed form signals consume the same admission token.
A deliberate second submission creates another operation. Reserve the exact
destination, branch and eventual session name on the owning computer; a
conflicting operation explains the conflict instead of attaching to another
session or generating an undisclosed replacement name.

## The creation card

Show the requested name, project, computer, agent, folder or planned worktree,
current stage and elapsed time. Clicking the card opens a preparation view in
the main pane with completed/current steps, a bounded diagnostic tail, retained
resources and the actions valid for the current state. Switching conversations
or closing this view leaves the operation running.

Use truthful stages, omitting those that do not apply:

```text
Saving request → Queued → Checking folder and choices
    → [Checking repository access → Cloning repository]
    → [Creating worktree] → Verifying checkout
    → Starting agent → Confirming conversation → Session
```

Also support **Needs your input**, **Stopping…**, **Stopped**, **Failed** and
**Checking result…**. Connectivity and result certainty are independent of the
stage: a disconnected computer does not turn a possibly running checkout into
a failed one. Show **Connection lost · checking creation result** when that is
what is known. Show **Taking longer than usual** with the current stage and last
confirmed update when progress is quiet. A heartbeat proves liveness only.
Do not invent percentages or claim that quiet Git output is a hang.

An error remains on the card with a short explanation, failed stage, retained
folder/branch and one useful next action. Technical detail is expandable and
copyable after redaction. Native startup questions use the existing manual
question/permission flow. A creation operation grants no approval authority.

Selecting the new card is explicit navigation; checkpoint the previous
conversation's draft first. The preparation view may keep a local draft under
the operation UUID. **Send** stays disabled until the exact conversation is
bound. Transfer that draft only to the verified result, without overwriting an
existing draft or sending automatically. Stop, failure and restart retain it.
Pending cards have no usable session reference, task-drop target or fake native
run/conversation identity.

Show creation cards in All and in a separately counted creation area of Active.
Failures and input requests remain there until resolved or explicitly hidden.
This is operation presentation; existing native Working, Background, Scheduled,
unread and attention semantics remain intact. Apply project, machine, account
and search scopes using the captured metadata. The final Create action reveals
and selects its card even if current filters exclude it, while retaining those
filters for an explicit return. Later user filtering ends that temporary reveal.
Keep hidden unresolved operations reachable through a creation summary.

Do not repeatedly steal selection, scroll or focus as stages change. On success,
replace the card in place using its original operation key, retaining selection,
draft and diagnostic access. Once the person has navigated away, completion
must not reopen it. Use one completion or actionable failure notification under
existing foreground/mute rules, with no duplicate notification from the first
normal session listing. Creation events are not agent replies or unread marks.

## Ownership and durable identities

Introduce an application-owned `SessionCreationController` and a versioned,
private operation journal. The dialog collects choices; the controller owns the
pipeline, subscriptions and recovery independently of any view. All storage,
process work, status reads and diagnostics stay off the GUI thread. Do not grow
the synchronous `QSettings` pending-launch object into a full operation store.

Keep one operation UUID across its lifetime. Give each mutating step an immutable
request UUID and arguments fingerprint. Persist reservation before delivery and
persist the confirmed result before scheduling the next mutation. A new attempt
records its predecessor and reuses confirmed preparation; it does not erase an
unknown previous attempt. Guard callbacks with operation, step, attempt,
computer identity and revision. SSH aliases are routing information, not proof
that a machine identity is unchanged.

Extend native preparation with a capability-gated receipt contract supporting
reservation, status and scoped cancellation. For each request it must distinguish
not dispatched, running, confirmed success, definite failure and unknown result.
An atomic request reservation admits at most one execution; duplicate delivery
with matching arguments returns that operation, and mismatching arguments are
refused. A crash after reservation never authorizes a duplicate Git invocation.
Retain repository, destination, branch/commit and ownership evidence sufficient
to verify a recovered result. Mere path existence is insufficient.

Reuse repository-clone receipts and launch evidence where they already provide
these guarantees. Supply the missing durable worktree request/status contract;
do not mistake the current echoed UUID or cleanup ownership receipt for it.
For startup, durably claim dispatch and reconcile the exact launch ID against
live sessions, saved run/conversation records and ended evidence. If a backend
cannot prove whether dispatch happened, expose an unknown result rather than
replaying it. A missing row in one bounded listing is not proof of absence.

Closing a preparation view does not cancel its owner. Application quit is a
different boundary: flush journal barriers, stop advancing the pipeline and use
the native wrapper's supervised preparation shutdown. Preserve receipts and
partial files. After a crash or restart, read original receipts and session
evidence automatically, but require **Continue setup** before dispatching any
new mutation from an interrupted pipeline. A confirmed late-created session is
shown normally. This proposal does not require a new always-running daemon or
promise that cloning continues after the application exits.

## Recovery and cancellation scenarios

| Situation | Required behavior |
| --- | --- |
| Valid ordinary folder with None | Skip Git preparation and start the selected agent. Never initialize Git. |
| Verified Git folder with Auto | Create the displayed branch/worktree once after admission, verify it, then start there. |
| Existing or explicitly created worktree | Revalidate that exact checkout. Do not generate another worktree. |
| Repository source | Check access/destination, clone once, retain the checkout, then apply Auto/None and start. |
| Invalid name or required field | Explain it in the form before admission; no pending operation is needed. |
| Background journal save fails | Keep a failed card with the captured choices and a safe save retry. Dispatch nothing. |
| Computer becomes offline before dispatch | Show that no operation was sent and offer Continue when available. Do not schedule a surprising future launch automatically. |
| Connection drops after dispatch | Read the original receipt; prohibit replay until the result is authoritative. Keep the last confirmed stage visible. |
| Slow clone, checkout, hook or filter | Keep the application usable; show stage, elapsed time and available output. Enforce native deadlines separately from the UI warning. |
| Another Git operation owns the repository lock | Show Queued or a definite busy refusal. Bound lock waiting; unrelated repositories can progress. |
| Destination, branch or session name already exists | Report the exact conflict. Preserve it, and offer reviewed edits or selection of an existing verified checkout. |
| Folder or repository identity changed | Stop before the next mutation and require review. Never accept a replacement merely because its path matches. |
| Account removed, model changed or agent unavailable | Show the failed prerequisite and retain preparation. Offer Edit choices or the appropriate account/setup page. |
| Native startup needs trust, login or input | Show Needs your input and an explicit route to the verified startup terminal or existing native UI. Never auto-answer. |
| Git reports failure after creating files | Keep partial checkout, branch and native ownership evidence. Offer Inspect and Check result; failure does not authorize deletion. |
| Worktree succeeds but agent startup definitely fails | Keep the worktree and retry only startup after revalidation. Do not create a second checkout. |
| Startup command succeeds but no conversation appears | Remain Confirming conversation. Reconcile run and ended evidence; a command exit of zero is not Ready. |
| Session appears before command completion | Bind it once using exact identity. A later generic transport error cannot delete or downgrade the proven session. |
| Agent exits before first listing | Show the native ended reason and bounded output on the same card. Preserve any real saved conversation. |
| A known session is created and then exits | Show the real session's current lifecycle. It is not an uncreated session or permission to launch a replacement. |
| Project is renamed, deleted or moved during preparation | Follow the stable ID through a rename. If the intended project no longer exists or is inaccessible, retain resources and ask for a new placement. |
| Catalog assignment fails after agent launch | Show the real session plus a placement error. Retry the assignment only; never relaunch the agent. |
| Stop before mutation dispatch | Withdraw the queued work and confirm Stopped. No native process needs to be killed. |
| Stop during clone or worktree creation | Durably fence later launch steps and request cancellation of only the owned preparation process group/job. Show Stopping until its owner proves exit and the final result; acceptance of the cancellation request is insufficient. Keep all files and branches. |
| Stop is offline or its result is unknown | Show Stop requested with its unconfirmed delivery. Do not claim Stopped, delete the card, or retry creation. |
| Stop races with native session creation | Reconcile first. If a session was created, show it and explain that creation already completed. Use the normal explicitly confirmed Terminate action to end it. |
| User hides an operation | Hide only its presentation. Keep receipts, late-result reconciliation and project placement. Do not cancel or remove files. |
| Application closes, crashes or upgrades | Restore cards from the private journal and check original results. Unfinished stages need explicit Continue; discovered sessions are adopted once. |
| Two forms or clients target the same resources | Native atomic reservations and Git/session guards decide admission. UI button disabling alone is insufficient. |
| Status response is stale or belongs to another attempt | Ignore it without changing the selected card, resuming setup or replacing a newer error. |
| Receipt is missing, malformed or from an unsupported schema | Retain an unresolved card and offer inspection. Do not infer success, absence or permission to retry. |

**Stop creating**, **Hide status**, **Retry failed step**, **Edit choices** and
**Remove retained files** must remain distinct actions. Destructive cleanup uses
the existing explicit worktree review and confirmation workflow, including
ownership, process, uncommitted and ignored-data protections. Creation failure
and elapsed time are never cleanup authority.

Retry read-only checks freely within bounded backoff. Retry a mutation only when
the previous attempt is durably proven not dispatched or definitely ended and
fresh checks permit another attempt. If the outcome is unknown, offer **Check
result**. Recovered successful preparation requires explicit Continue; a status
read cannot silently advance an interrupted pipeline.

Serialize cancellation and step advancement through the operation revision. A
launch dispatch must atomically claim the still-current, uncancelled revision;
checking a cancellation flag before an asynchronous save is insufficient.

Edit choices is available only when no mutation is in flight or uncertain.
Retain the old attempt and resources. Changes to computer, source, destination
or worktree mode start a reviewed successor request; changing an account for a
definitely failed startup may reuse the verified checkout. Choosing None is an
explicit new choice and never an automatic fallback after Auto fails.

## Project placement and terminal behavior

Keep pending placement private until its existing success boundary. Ordinary
outside folders join the project only after the matching successful launch;
generated worktrees do not become extra project folders. A confirmed clone keeps
the current contract: it joins the selected project even if agent startup fails.
Create a requested new logical project only at that confirmed acquisition
boundary, using a stable project UUID privately reserved by the operation and an
idempotent local assignment. Pending presentation may show its requested name but
must not publish that prospective project into shared catalogs.

Preserve the distinction between explicit Inbox and an untouched inferred
Inbox. Late reconciliation must not overwrite a newer explicit project move.
Receipt matching uses stable computer identity, original launch ID and verified
run/conversation identity, with name/agent/directory checks as applicable.
Never adopt an unrelated session because its display name or folder is similar.

Opening a separate terminal is a presentation request for the verified startup
run, at most once. It must not create a second agent. Failure to open that window
is a warning if the session itself exists. Where a legacy terminal launcher
still owns creation, keep one launch dispatch and explicitly reconcile its
result; do not also issue a detached launch. Closing an attached terminal uses
the existing agent/session lifetime contract. New-session cancellation never
becomes a general native Stop or a tmux/server shutdown.

## Bounded work and compatibility

Initially allow four concurrent preparations globally and one mutating Git step
per repository, with a bounded queue of 32 unsettled creations. Refuse further
admission with an explanation before side effects; never evict unresolved
receipts to make room. These are proposed desktop limits, not API v1 changes.
Queued, running and unknown operations consume admission slots; definite failed
and stopped cards do not. Keep native cross-process locks as well as controller
scheduling. Repository scheduling keys include the verified owning computer.

Use an indexed operation store and stable cursor pages of 50 rows. Keep at most
64 KiB of rendered diagnostics for the selected operation and a 256 KiB persisted
diagnostic tail per operation, with explicit truncation. Store structured stage
transitions separately from coalesced heartbeat/progress events. Do not scan all
session histories to resolve a launch or keep an output widget for every card.
Redact credentials and sensitive environment values before writing diagnostics;
do not export operation journals into Beads, shared memory or source control.

Preserve native deadlines. A long-operation hint does not extend them or turn a
transport timeout into a definite native failure. A live operation can resume
observation after reconnect; it cannot gain authority for another mutation.

Migrate legacy pending launches as observation-only startup operations. Import
the original pending clone UUID without starting it again; retain older hidden
placement intents as hidden. Do not turn historical settings into visible new
work or reset existing configuration/state paths. Unsupported journal schemas
remain inspectable and cannot be overwritten as an empty queue.

Probe the exact selected CLI for new receipt/cancellation capabilities. Older
peers may still use the current folder/launch commands and current one-shot
worktree preparation, but their cards must expose conservative recovery and
limited cancellation. Unknown outcomes stay blocked from replay. Do not advertise
safe Retry, confirmed Stop or restart continuation that the peer cannot prove.
Keep repository acquisition disabled where its existing capability is absent.

Mobile follow-up should carry one parent creation identity across its existing
`worktree_create`, repository and launch request identities, preserving API v1,
connector journal receipts and exact connection/computer routing. A standalone
mobile worktree operation must continue to create only a worktree. Pending local
creation cards do not become replicated sessions visible on every device before
the owning computer confirms a real session.

## Validation and implementation boundaries

The acceptance check for immediate feedback is deterministic: hold all fake CLI
responses, click Start once, and assert that the dialog closes and the selected
card is visible before releasing any response. Add a delayed storage control:
the card appears, but no mutation is dispatched before durable acknowledgement.
Measure click-to-first-card paint under a deliberately slow checkout and CPU
load; target under 100 ms on the fixture, independently of preparation duration.

Exercise every scenario above with controlled response order, including success
before response, failure after success, ended-before-listed, stale callback,
cancel versus launch, duplicate submissions and two clients. Assert native
invocation counts and retained identities/resources, not only status strings.
Inject crashes before reservation, after reservation, after mutation and before
result persistence, and between confirmed preparation and launch dispatch.

Use isolated real Git repositories for slow/failing hooks, busy locks, disk or
permission errors, branch/destination conflicts and partial checkout retention.
Use private SSH/transport fixtures and isolated terminal backends; never mutate
live agents, their tmux servers or production checkouts. Native startup/lifecycle
changes require the repository's latest-release agent contract suites on the
affected platforms. Fake-agent tests do not replace that qualification.

Verify desktop light/dark themes, enlarged text, small windows, keyboard and
screen-reader access, previous drafts, filters, scrolling and selection through
card replacement. A pending card must expose its stage and actions accessibly.
Test queue/log/page limits with many operations and normal session catalogs in
the tens of thousands. Check old settings, old peers and read-only/corrupt journal
recovery. Mobile needs its own journal and native UI qualification before claiming
the same behavior there.

Implement the controller and early selectable row first, retaining conservative
unknown-result behavior. Add durable native worktree recovery and scoped
cancellation before exposing those actions. Integrate clone recovery and stable
card-to-session replacement, then qualify cross-platform lifecycle and UI cases.
Investigate the original checkout latency separately with stage timing and
bounded Git/hook diagnostics; moving progress cannot establish or fix its cause.

## Implemented delivery scope

The desktop implementation uses an application-owned controller, an ordered
background atomic journal and selectable cards before preparation starts. The
current native helper adds opt-in worktree receipts, scoped cancellation and
computer UUID guards. Legacy helpers retain one-shot preparation; unknown
outcomes never replay. Existing standalone clone/worktree dialogs retain their
own preparation flows.

The initial journal is a bounded UUID-indexed JSON object (128 records / 8 MiB),
with 32 stage transitions and 4,000 characters of redacted error per operation.
The history menu snapshots pages of 50; only the selected detail view renders
text, capped at 64 KiB. Hidden settled history becomes eligible for pruning at
96 records. This deliberately uses smaller diagnostic retention than the upper
limits proposed above. Hiding a card never invalidates a reserved stage's
durable acknowledgement. Cancellation does invalidate advancement.

Restored operations are observation-only until the user checks the original
result or continues a step with no uncertain mutation. Native session listings
continue to reconcile exact launch identities automatically. Clone shutdown
uses its existing wrapper supervision; the card offers Stop after preparation
to prevent startup without claiming immediate clone cancellation. New worktree
helpers expose scoped Stop creating. Startup questions and saved conversation
drafts use existing native/session flows; the preparation pane has no composer.
Mobile presentation, live cross-platform agent qualification and investigation
of the originally reported slow Git operation remain separate scopes.

The current user-facing contract is documented in
[Session creation progress](../../reference.md#session-creation-progress).
