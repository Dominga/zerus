# Explicit existing OpenCode conversations

Zerus can bind one explicitly selected native OpenCode 2 conversation. The
operation creates only its local binding and request receipt; it never creates
or renames a native conversation, changes permissions, sends a prompt or starts
the OpenCode service. It is not a bulk importer and does not scan native history.

## Desktop

Choose the OpenCode account on its Linux/macOS or WSL machine, then **Bind existing
conversation...**. Enter the native `ses_...` identity, its exact canonical native
directory and a new Zerus name in `opencode/project/session` form. The selected
machine and native account profile remain visible. Use **Preview**, review the
native title and identities, then use **Bind conversation**. Editing any field
invalidates the preview. A busy/offline/incompatible native service is an error,
not a reason to create a substitute conversation.

Use the directory recorded by OpenCode, not a Windows spelling for a Linux path.
A local lexical path mapping does not itself prove that the native session lives
at that location. Zerus checks the physical native directory and the exact
session response. Existing titles, metadata, messages and native IDs stay intact.
Once bound, ordinary inspection, history, send and scoped session actions apply;
readiness does not submit a message automatically.

## CLI contract

Run `hgs [@machine] account bind-existing native-opencode --json` with one JSON
object on stdin, limited to 16 KiB. The native account must already be running.
The help capability marker is `opencode-bind-existing-v1`.

```json
{
  "schema_version": 1,
  "request_id": "11111111-2222-4333-8444-555555555555",
  "name": "opencode/project/existing",
  "conversation_id": "ses_selected",
  "directory": "/workspace/project",
  "agent_home": "/home/alice/.config/opencode",
  "preview": true
}
```

A successful preview returns a bounded native title and `identity`. To commit,
submit the same selection and request ID with `preview: false` and
`expected_identity` set to that token. A service restart or changed selection
invalidates the token. `native_changed` is always false on success. An exact
existing binding returns `already_bound` with its original run identity.

Names cannot overwrite another binding, and the same native conversation/profile
cannot gain a second Zerus owner under another name. A request ID is permanently
bound to its selection. Reusing it for another payload is refused. A repeated
confirmed operation reuses the exact run identity. After Forget, replaying the
old request does not resurrect the binding; a fresh explicit selection and
preview are required. An uncertain local write is reconciled by previewing the
same selection; it is never used to issue a native creation request.

The duplicate check streams bounded binding metadata (at most 5000 entries,
1 MiB per record and a five-second admission budget). A corrupt, oversized or
incomplete inventory fails closed rather than claiming there are no conflicts.
This initial admission bound is separate from paged native message history.

## Qualification

The opt-in native suite uses a private OpenCode service and keyless local model.
It creates a conversation directly through the native API, writes a prior turn,
then verifies that preview/binding preserve its session object, messages and
registration. It also verifies reopened history, single delivery of a continued
turn, duplicate/name/profile/directory refusals, stale previews after restart,
Forget replay protection and absence of service autostart. It never selects an
owner conversation. Tests do not authorize automatic import of a real profile.
