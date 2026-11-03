# Explicit Windows desktop service mode

The maintained user task is passive on a fresh installation. Ordinary
`hgs desktop registration install --json` refreshes an owned registration
without starting or stopping it and preserves its existing passive/background
selection. Read-only `registration --json --background` only renders a plan.
An explicit selection that already matches the verified running task is a
no-op, for both modes. In particular, selecting an already running passive
task preserves its unused Service grant and original process.

An explicit change uses the selected installed CLI:

```powershell
hgs desktop registration install --mode background --json
hgs desktop registration install --mode passive --json
hgs desktop registration status --json
```

The CLI advertises `Desktop registration mode ABI: desktop-registration-mode-v1`.
The JSON receipt separates `selected_mode` from `running_mode` and reports
`ok`, `outcome`, `request_id` and `pending_transition`. A pending or unknown
result never establishes that a manager operation did not happen: `changed`
and `manager_changed` are null when the original outcome is unknown. Inspect
the original UUID rather than issuing another logical action.

## Caller and ownership

The caller must be outside an agent tree and have either the actual selected
installed frontend as a retained ancestor, or an already attached, visible,
foreground classic console with a verified current console process list.
Windows Terminal/ConPTY cannot supply this classic-console proof; use the
Zerus Next Settings route. No console is attached, allocated or brought to
the foreground to manufacture admission. Settings callsite integration and
installed acceptance are separate from this native helper.

As with the common frontend boundary, this prevents unintended agent descent.
Deliberate same-user parent-process spoofing is outside its scope. Caller
identities, images, ancestry and console facts are rechecked before effects.
Decoded status, SID, session number and a mode-check result confer no consent.

First install the passive registration and start its passive actor. The
authenticated frontend then submits its explicit Service Grant/Rearm against
that actor. Only a current stored Service grant for the actual selected
configuration and installed identity permits the background selection. Starting
with `--background`, refreshing a task or reading status cannot create or clear
that grant. A retained revoke always requires a fresh explicit frontend Rearm.

## Original task and rollback custody

Each transition checks the actual owned Task Scheduler definition, principal,
owner/DACL and current interactive session. A running instance is retained as
its original `IRunningTask`, immutable instance GUID and independently verified
desktop process handle/birth. The service process is authenticated through its
original named pipe, exact configuration/argv, executable file object and
lifecycle epoch. `EnginePID` alone is insufficient. A separate/shared engine
that cannot be bound to that exact service process refuses before stopping it.
The native Windows round-trip fixture must qualify the supported SDK topology.
Outgoing controller connections verify the kernel pipe server PID and its
actual user token, then join it to that same retained process. Incoming actor
connections keep their client impersonation and integrity checks. These are
separate endpoint roles; a failed check never falls back to the opposite role
or the checking process's own PID.

A registered CLI that refuses startup with exit code 78 retains its last bounded
error in the existing private desktop state's `startup-failure.json`, when that
state is safe and writable. The record includes configuration, PID/birth and the
original guard reason; it creates no state folders, changes no grant and never
retries startup. It is historical diagnostic data, never current readiness,
task-instance identity, authority or proof that resources drained. An unavailable
diagnostic leaves the original stderr and failure exit unchanged. Task Scheduler's
last-run result and this private record can explain an early exit after RunEx;
an absent running instance never permits issuing RunEx again.
The admitted mode helper reserves that diagnostic file before offering a start.
The scheduled process can write through the same checked user-owned object even
if its token chooses a different owner for newly created files. Failure data
includes the token's user/default-owner SIDs; the private fixture also reads
bounded owner/DACL metadata for its four known state entries. These observations
do not change token defaults, repair file ownership or relax a privacy check.
An untouched `reserved` record means no failure text was safely recorded.

The native task probe confirmed that an elevated scheduled process can have
the correct User SID while its default creation owner is
`BUILTIN\Administrators`. Before creating private state or starting workers,
the dedicated desktop service selects its actual primary token's User SID as
that token's default owner. The dedicated mode worker does the same only after
capturing and checking its native caller, and checks that caller again before
continuing. This covers SQLite's internal WAL, SHM and rollback-journal files
as well as locks and atomic replacement files. The operation has no PID or SID
argument, refuses impersonation, and rechecks the original primary token and
the selected owner. It changes only `TokenOwner`: user, privileges, groups,
integrity, session and the default DACL remain unchanged. Windows documents
this creation rule in [Owner of a New Object](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object).

The existing exact-user, ACL, no-reparse and single-link checks still run.
Inherited ACLs must still satisfy the existing private-state guard; the creation
step does not repair a directory, rewrite any ACL, accept an Administrators-owned
file or replace one. A previously refused file remains refused. No task, grant,
revoke receipt or uncertain manager effect is replayed to repair that state.

Passive rollback saves one UUID before offering the original rollback revoke.
Only a known committed prepared command can reconcile its original drain with
that same UUID and original process. An absent or uncertain command is never
resent. After the settled receipt, the original selected `revoke-hold` child
physically excludes Rearm/native takeover throughout the task transition.
Its original stdin writer is not inherited by another child.

Only the retained instance's `IRunningTask::Stop` is called. The helper requires
the same service process handle to signal actual exit and the original instance
GUID to disappear. It never stops all instances, selects a process by name or
uses a failed identity check as evidence of exit. Replacement is UPDATE-only,
preserves verified security, and ignores registration triggers. A single
`RunEx` explicitly selects the verified current session and marks its user
argument as the verified SID (`TASK_RUN_USE_SESSION_ID | TASK_RUN_USER_SID`). Its successful return
is insufficient: the same returned instance must reach exact service
process/pipe readiness.

The rollback hold remains alive through passive readiness. Completion requires
original stdin EOF, the matching Released frame, successful original child
exit, and both original pipe EOFs. Only then may the pending record be cleared.
Background enablement instead retains the actual handoff exclusion and checks
the stored grant plus operation-resource drain; the runtime rechecks authority
after that exclusion is released.

## Interrupted transitions

The private bounded record stores the original UUID, phase and observed instance
identities. It is metadata, never permission to stop or adopt a process.
Each Stop/Update/Start is recorded as offered before its native call. Unknown
manager effects stay pending; repeating the choice never replays them. A
before-manager interruption may resume only with the same original runtime,
UUID and native rollback exclusion. Background and initial passive starts
acquire their actual handoff exclusion and complete drain checks before saving
a new record, so transient refusal leaves no pending intent.

Repeating the same choice at `start_offered` or `ready` is observe-only. The
recorded original started GUID must bind to the exact owned task and a ready
service in the requested mode. Passive recovery also reacquires the same
UUID's read-only revoke hold, or proves an untouched, drained initial passive
bootstrap under the handoff exclusion. Any reacquired hold still requires its
matching release frame, original exit and both pipe EOFs. Completion changes
only installer metadata: no Stop, Update, RunEx or revoke is issued. A missing
GUID, changed definition, unready original instance or unverifiable exclusion
reports its exact unknown phase/reason; another instance is never adopted.

The phase budgets are independent: preflight 2 s, rollback 6 s, hold acquisition
3 s, Stop 2 s, update 2 s, start 2 s, readiness 8 s and release 2 s. Readiness
gets its full budget after earlier phases; it does not spend the drain's
remaining time. The original child exchange remains bounded by the shared
runner's existing 30 s maximum. Its timeout still leaves uncertainty pending
for observation, never permission to replay a manager call.

If a fresh frontend Rearm won before hold acquisition, explicitly cancel only
the obsolete saved rollback metadata:

```powershell
hgs desktop registration cancel-rollback --json --request-id UUID
```

Cancellation checks the original pending UUID and native caller twice. It can
discard any pre-manager record for either mode, including stale background
preparation after a restart. Passive rollback additionally requires a current
Service grant twice; background cancellation requires no grant. It changes no
task, grant or revoke receipt and does not contact Task Scheduler. It also covers
a saved rollback that never took effect.

For a `start_offered` or `ready` record, the same explicit cancellation instead
uses read-only Task Scheduler and native service observations. It requires the
recorded started GUID to bind to the exact owned definition, retained service
process/pipe and requested running mode, with fresh readiness and caller checks
through the metadata save. Other offered phases, a missing GUID, changed task or
unready original instance remain pending. This branch reports
`observed_start_cancelled`, preserves the previous completed UUID, and records
the cancelled UUID. It performs no Stop, Update, RunEx, revoke or hold acquisition.

This allows a completed passive start whose reply was lost to be abandoned after
a later Rearm or revoke prevents the old rollback hold from being reacquired.
The later authority is left untouched; this post-readiness metadata cancellation
requires no current Service grant and confers no authority. A later separately
confirmed choice receives a new UUID; there is no automatic UUID rotation or
unknown-effect replay.

## Private qualification

The Windows unit suite runs the creation-owner regression in one bounded child,
never by changing a parallel test runner's token. On an eligible elevated token
it reproduces the original Administrators-owned lock refusal, selects the actual
User owner, opens the real desktop Store and checks the resulting locks, database,
WAL, SHM, rollback journal and replacement file. It retains the bad original file
and verifies its owner and identity remain refused. Token groups, privileges,
integrity, session, default DACL and parent token remain unchanged, and an
impersonating thread refuses. Hosted CI must execute this control without an
eligibility skip. The task round-trip independently checks the actual scheduled
service's file owners in each mode.
The native gate also runs a separate-process pipe-role control: the client end
refuses server-only client impersonation, verifies the actual server PID, and
the server end independently authenticates the original child PID.

`tests/windows_frontend_auth.py` includes the disposable hosted-Windows task
round-trip. The maintained registration installer stages the stable CLI under
the test's private HOME, with the native test frontend beside that selected
image. A portable `HGS_EXECUTABLE` override never bypasses the Windows stable
path rule. USERPROFILE is retained only for the canonical native instance
namespace shared with the Scheduler-launched service; native caller/process
checks remain mandatory. The runner's actual profile files are never staged or
changed. The fixture refuses a pre-existing per-user task and runs only on
disposable hosted Windows VMs.

It uses private configuration/state, a real authenticated frontend Grant,
passive/background/passive transitions, unchanged runtime on ordinary refresh,
observe-only completion and explicit cancellation without manual record resets,
including Rearm or a later revoke after a lost passive-start reply and a
separately explicit new choice after read-only cancellation,
and a retained unrelated child. Cleanup targets only the exact verified fixture
instance and checks its real process birth and exit; changed-birth cleanup
refuses. Only then are its private images/state removed. A failure preserves
uncertainty and private diagnostics instead of killing by task name or
executable pattern. This suite authorizes no owner's installed task, GUI,
agents or sessions.

Microsoft API contracts: [RunEx](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-iregisteredtask-runex),
[instance Stop](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-irunningtask-stop)
and [EnginePID](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-irunningtask-get_enginepid).
