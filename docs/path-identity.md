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

This step maps folder inputs into WSL. Automatic reverse mapping for opening
remote files in Windows editors remains a separate integration: it must preserve
file-read authorization and must not silently turn remote paths into local ones.
