# Explicit Windows desktop service mode

The maintained user task is passive on a fresh installation. Ordinary
`hgs desktop registration install --json` refreshes an owned registration
without starting or stopping it and preserves its existing passive/background
selection. Read-only `registration --json --background` only renders a plan.

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
`RunEx` explicitly selects the verified current session. Its successful return
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
manager effects stay pending; repeating the choice does not replay them or
adopt a replacement instance. A before-manager interruption may resume only
with the same original runtime, UUID and native rollback exclusion.

If a fresh frontend Rearm won before hold acquisition, explicitly cancel only
the obsolete saved rollback metadata:

```powershell
hgs desktop registration cancel-rollback --json --request-id UUID
```

Cancellation checks the original pending UUID, native caller and current
Service grant twice. It changes no task, grant or revoke receipt. It also covers
a saved rollback that never took effect. It refuses after any manager effect
was offered. A later separately confirmed passive choice receives a new UUID;
there is no automatic UUID rotation or unknown-effect replay.

## Private qualification

`tests/windows_frontend_auth.py` includes the disposable hosted-Windows task
round-trip. It uses private images/configuration/state, a real authenticated
frontend Grant, passive/background/passive transitions, unchanged runtime on
ordinary refresh, and a retained unrelated child. Cleanup targets only the
exact verified fixture instance and checks its real process birth and exit;
changed-birth cleanup refuses. A failure preserves uncertainty and private
diagnostics instead of killing by task name or executable pattern. This suite
does not authorize an owner's installed task, GUI, agents or sessions.

Microsoft API contracts: [RunEx](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-iregisteredtask-runex),
[instance Stop](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-irunningtask-stop)
and [EnginePID](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-irunningtask-get_enginepid).
