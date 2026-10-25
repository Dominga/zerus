# Explicit path identity

`hgs path-ref` describes lexical path identities and explicitly maps a selected
Windows workspace to one WSL distribution and user. It performs no filesystem
I/O, never creates a folder, and does not change any stored project or session ID.
Existing session catalogs and physical filesystem checks remain unchanged.

The Rust dependency uses CanonicalPath release `2026.10.24-1`, pinned to commit
`50b390675a146874b1555f361e491d1efeb393a7` and recorded in `Cargo.lock`. Its Rust
package currently declares `2026.5.18-2`; the full Git revision identifies the
release source, independently of that package-internal version.

## Contract

A `PathRef` contains `schema_version: 1`, `machine_id`, `environment_id`,
`path_flavor` (`windows` or `posix`), `native_path`, `canonical_path`,
`workspace_id`, and `mapping_id`. Nullable fields are serialized as JSON null.
The native spelling is retained for its execution environment. Canonical spelling
does not resolve symlinks or prove authorization. It must never replace native
filesystem checks, session IDs, or root-bound filesystem access.

Both native grammars require absolute paths. Native POSIX preserves literal
backslashes, filename case, spaces, percent escapes, and drive-like components.
Windows normalization lowercases only the drive letter. It supports drive paths,
UNC paths, and valid extended prefixes. Relative paths and NUL fail. No URI
decoding or filename trimming occurs.

`/mnt/c/Projects/Orbit` remains a POSIX identity until a caller supplies a mapping.
`/home/alice/Orbit` in two distributions remains two different environment
identities. Matching canonical strings alone never establishes the same project.

## Commands

Each command reads exactly one JSON request from stdin, up to 64 KiB. Unknown
fields fail. Success prints a `PathRef`; malformed input or a failed contract
exits with an error. This JSON transport avoids shell-specific path quoting.

- `hgs path-ref describe`: input is `{ "path": <PathInput>, "mapping": null }`.
- `hgs path-ref validate`: input is `{ "path": <PathRef>, "mapping": null }`.
- `hgs path-ref map`: input is `{ "path": <PathRef>, "mapping": <WslMapping> }`.

For mapped references, `validate` requires the corresponding mapping. It
recomputes canonical identity from native input; serialized canonical fields are
never trusted. `map` accepts a verified unmapped or already mapped reference and
switches between the two exact environments in the mapping.

Example `describe` request:

```json
{
  "path": {
    "machine_id": "workstation-a",
    "environment_id": "windows-native",
    "path_flavor": "windows",
    "native_path": "C:\\Projects\\Orbit\\My Game",
    "workspace_id": "orbit"
  },
  "mapping": null
}
```

In PowerShell 7, `(Get-Content -Raw request.json) | hgs path-ref describe` sends
this request. On POSIX, use `hgs path-ref describe < request.json`.

## Explicit drive mapping

Use this object as `mapping` with either `describe`, `validate`, or `map`:

```json
{
  "mapping_id": "orbit-drive",
  "workspace_id": "orbit",
  "machine_id": "workstation-a",
  "windows_environment_id": "windows-native",
  "wsl_environment_id": "wsl:Ubuntu-24.04:alice",
  "distribution": "Ubuntu-24.04",
  "user": "alice",
  "mount_root": "/mnt",
  "windows_root": "C:\\Projects\\Orbit"
}
```

The WSL environment ID must be exactly `wsl:<distribution>:<user>`. A mapping
belongs to one machine, workspace, distribution, user, and mount configuration.
Every value is explicit; there is no distro discovery or default-user inference.
The source must match these identities and lie within the mapped Windows root
by complete path components. Sibling prefixes and lexical traversal outside the
workspace fail. A literal POSIX backslash cannot be silently converted into a
Windows separator. A mapped native POSIX path must round-trip exactly after
lexical dot cleanup: for example, `/mnt/C` cannot silently become `/mnt/c`.
Trailing spaces are retained in the lexical result, including Windows spellings;
this is not a guarantee of Windows API filename semantics or file accessibility.
The output carries the mapping ID and retains a shared
canonical drive spelling while using the target environment's native path.

Custom mount roots and `/` are supported. A mounted drive path can be mapped
only when it has a lossless Windows representation. UNC shares, WSL home
directories, and arbitrary network aliases need a separately specified mapping
contract; this command does not guess them from spelling.

Mappings assert an operator-selected alias, not that the mount actually exists
or refers to the same files. Runtime connection setup must verify its selected
distribution/user separately. These helpers deliberately perform no I/O or
automatic persistent-identity migration. A future catalog migration must retain
stable session IDs and explicitly review collisions before writing anything.


## Windows desktop folder pickers

For a configured `wsl-local` machine, **Map Windows path...** in the folder browser
opens an explicit mapping editor. Enter the source Windows path, the allowed
Windows workspace root, the configured WSL drive mount root and a workspace
identity. Distribution and user come from the selected local connection, not the
default WSL instance. The editor shows the exact Windows, WSL and canonical
spellings before **Use mapped path** becomes available.

Before accepting, the editor re-reads the local connection and refuses a changed
distribution, user or receiver. The ordinary target-side folder browser then reads
the native directory; a lexical preview alone never makes an absent or unreadable
folder selectable. New-session and project folder selection share this browser.
No project or session catalog is rewritten, no directory or worktree is created
by previewing, and no OpenCode service is started.

**Remember this mapping** is a separate, unchecked opt-in. It stores bounded JSON
in local GUI settings, keyed by the local machine identity and exact WSL
connection. Another user/distribution does not inherit it. Saved presets do not
remember a selected path or a previous verification result: every use requires
a new preview. Mapping changes receive a new mapping identity.

This folder step never silently turns a remote file reference into a local one.
The separate, explicit reverse-opening workflow below verifies its own access
and content evidence on every use.


## Explicit WSL file references opened as Windows originals

On a Windows desktop, a remote session file-reference dialog offers **Open mapped
Windows original...**. This is distinct from downloading a copy. Select the
workspace root, drive mount root and workspace identity explicitly, or reuse a
previously remembered preset for the exact connection. Use **Preview and verify**
and review both paths before **Open mapped original**. Any mapping edit clears
the preview. Merely opening the dialog queries connection metadata, not file
contents. Preview does not open an editor or modify either original.

The local `hgs machine file-proof <alias> --json` contract accepts a bounded JSON
selection, including the exact WSL distribution/user/receiver, session name,
run/conversation identity, optional archive/child identity, absolute POSIX path
and explicit mapping. It uses only that named WSL connection. A closed-input help
probe checks the scoped native read capability before sending `file-read`; no SSH,
default distribution, new session or service-start fallback exists. Native reads
are bounded to the existing 10 MiB regular-file limit and retain the original
conversation, archive and child checks. The input is limited to 64 KiB and the
native response to a bounded encoded snapshot. Snapshot bytes are never returned
in the proof or persisted by this operation.

The proof checks all echoed request/identity fields, recomputes the snapshot
SHA-256, checks the native resolved path against the selected mapped path, and
re-reads the local connection after the operation. A changed connection, another
native target, malformed reply or missing capability fails closed. The proof
contains paths, digest, size and a preview identity; it explicitly states
`local_verified: false`. It is evidence for the desktop, not an authorization
token to bypass another environment's permissions. An opening request repeats
the native check and must reproduce the preview identity exactly.

The desktop then independently verifies the Windows file on a worker thread.
It accepts only ordinary paths on a local fixed drive, rejects UNC/device/ADS
names, trailing-dot/space aliases and reparse points in every path component,
checks final handle paths, and hashes the same bounded bytes. Read handles remain
held while dispatching the explicit opener, denying ordinary replacement or
write access to the checked leaf during that dispatch. A mismatch opens nothing;
there is no automatic fallback to an unrelated local file or downloaded copy.

The original is passed to the existing `ExternalFile` policy as **ThisComputer**:
no Mark of the Web is added, removed or rewritten. Runnable scripts/programs
remain reveal-only rather than executed. Data-file handlers remain the owner's
configured default Windows applications. The dialog warns that editing changes
the original. Optional mapping persistence remains unchecked and local-only.

### Deliberate boundaries

This workflow is for explicitly mapped drive-mounted workspace files, not WSL
home directories or arbitrary UNC/network aliases. Linux symbolic redirects,
Windows junctions and ambiguous final spellings are refused. Files above 10 MiB,
empty/nonregular files, unknown child identities and unselected conversations
remain unsupported by this scoped workflow. Ordinary download actions are not
silently repurposed as proof of local authorization.

Equal content and an operator-selected alias do not establish identical inode or
volume identity across WSL/Windows. The implementation verifies the requested
alias and current bytes, not an independently attested mount topology. Handles
are held through dispatch, not through the lifetime of the external editor;
after handoff the file may be edited or changed normally. A default application
may also use a custom file association; Zerus preserves its existing executable
classification rather than claiming to sandbox that application.

Implementation references: Microsoft's [CreateFileW sharing/reparse semantics](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
and [GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew).
Tests use disposable session bindings, local files and fake open/reveal handlers;
no owner conversation, native model prompt or external editor is used.
