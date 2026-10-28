# Windows, PowerShell 7 and local WSL

## Windows shell policy

Native Windows folder shells and agent installer scripts prefer PowerShell 7 or
newer with `PSEdition=Core`. Zerus probes the exact executable with profiles
disabled. `HGS_PWSH` selects an absolute executable path; an invalid explicit
override is an error, not permission to use a different shell. Without an
override, Zerus checks PATH and the standard PowerShell 7 installation
directories. When none is found, a clean Windows keeps working with Windows
PowerShell 5.1: the CLI warns on stderr and the desktop shows a notice, both
recommending `winget install --id Microsoft.PowerShell --source winget`. The
fork update helper keeps its Windows PowerShell bootstrap.
The fork update feed remains unset; this does not enable automatic publication.

## Explicit local WSL connection

The Machines page has a **Local WSL (Windows)** connection type. Supply a
machine alias, installed distribution name, Linux user and absolute Linux path
to this fork's `hgs` executable. SSH host, key and port fields are not used.
A connection test checks the selected Linux receiver, not an unrelated SSH host.
The terminal button opens that same distribution and user. HGS installation
through this page is disabled for WSL: install or build it explicitly first.

Build Windows and Linux artifacts into different target directories. For example,
inside the selected WSL environment and source checkout:

```sh
CARGO_TARGET_DIR="$HOME/.cache/zerus-wsl-build" cargo +1.99.1 build --locked
```

Use the resulting absolute Linux executable path in the profile. Both ends must
come from this fork. An older receiver rejects the versioned command in the
existing `swarm` namespace instead of interpreting it as an agent launch.

A synthetic profile, accepted by `hgs machine set linux-tools --json <JSON>`:

```json
{
  "transport": "wsl-local",
  "enabled": true,
  "wsl": {
    "distribution": "Ubuntu-24.04",
    "user": "alice",
    "hgs_path": "/home/alice/.cache/zerus-wsl-build/debug/hgs"
  }
}
```

Profiles are stored in local `machines.local.json`; they are not deployment
configuration for other machines. Missing distribution/user values, mixed SSH
settings, unsupported transport names and invalid Linux executable paths fail.
There is no fallback to the default distribution, default user or SSH.

The transport uses `wsl.exe --distribution ... --user ... --cd / --exec ...`.
Arguments and stdin are carried separately; no PowerShell, CMD or POSIX shell
parses the hgs arguments. It preserves the child exit code. Background listing
and connection probes have bounded output and deadlines. Windows `HGS_*`
variables and WSLENV forwarding are not propagated to Linux: the chosen user
retains its own Linux configuration and service registry.

## Existing-only OpenCode

Every request admitted by the Linux WSL receiver sets
`HGS_OPENCODE_SERVICE_POLICY=existing-only` before dispatch. This policy also
applies to the explicit `account start native-opencode` action and new-session
launches. A missing, stale, incompatible or unresponsive registration produces
an error; Zerus does not start, replace, restart or kill the owner's service.
PID checks, registration reads and authenticated loopback requests happen inside
Linux, rather than confusing a Linux PID or loopback address with a Windows one.

Inspect the service with `opencode service status` in the chosen Linux user
context. A separately started `opencode serve` is not proof that the registered
per-user service is healthy. Do not rewrite a stale registration or stop an
unrelated server to make a connection test pass. The service must be configured
and started explicitly by its owner before real sessions can be qualified.

Outside the WSL receiver, the existing managed lifecycle is retained. An explicit
`HGS_OPENCODE_SERVICE_POLICY=existing-only` also selects this policy for a direct
Linux CLI invocation; `managed` retains the original behavior. Unknown values fail.
Other native adapters and their user-triggered commands retain their own policy.

## Paths and compatibility

Use native Linux paths for commands sent to the WSL peer. Windows-to-WSL aliases
are explicit operations of [PathRef](path-identity.md), never automatic string
replacement or a blanket migration of saved projects. Its lexical identities do
not authorize filesystem access. Existing session IDs and saved catalogs are
not silently re-keyed. Native DeepSeek browser tunnels are not added by this
transport; its existing platform limitations remain.

## Verification

Run the existing Rust unit tests on Windows and Linux, and the Windows Qt suites
`windowsdesktop`, `machinespage`, `terminalbackend` and `processrunner`.
Windows private-storage tests need a private per-user TEMP/TMP directory: a shared
system-wide temporary folder can correctly fail their ownership/ACL checks. Set
these variables only for the test process; do not relax the privacy guard.

`tests/test_wsl_local.py` is opt-in. Set `HGS_TEST_BIN` to the Windows CLI and
`ZERUS_TEST_WSL_DISTRO`, `ZERUS_TEST_WSL_USER`, `ZERUS_TEST_WSL_HGS` to an explicit
local WSL target and this fork's Linux test binary. It creates isolated profiles
and synthetic helpers, exercises argv/stdin/exit status and missing-target errors,
and proves that even an explicit Start does not execute an agent under
existing-only. It never starts or stops the owner's OpenCode service.

The separate existing `test_opencode_native.py` suite qualifies the native adapter
with a private service, private home and keyless loopback model. Its results and
the transport contract are separate evidence, not a claim that a production
service or every desktop/mobile platform has been qualified.


### Read-only live-service acceptance

`tests/test_wsl_live_opencode.py` is a separate explicit opt-in. Set
`ZERUS_TEST_WSL_LIVE=1` and the distribution, user and binary variables described
above. Run it only against an owner-started registered service. It isolates HGS
configuration/state while retaining the chosen Linux user's native registration,
checks the account/model catalog, and proves that the explicit Start action
leaves the service PID and complete registration unchanged. It creates no native
session, submits no model request, and does not start or stop OpenCode.

For an owner-managed stack, prefer the official `opencode service` lifecycle so
all clients discover the same registration. Its service configuration can select
a loopback hostname and port before startup; do not synthesize the registration
from a separate `serve` process. Changing service configuration may stop the
service and must be an explicit operator action, never a Zerus readiness probe.

## Selected existing conversations

The account page and `account bind-existing native-opencode --json` provide an
explicit preview/commit workflow for one existing native conversation. See
[existing conversation identity and continuity](opencode-existing.md).

## Downloading a WSL copy

`hgs @configured-wsl download --path NATIVE_PATH --output WINDOWS_FILE` keeps the
destination on the Windows client. It probes the exact configured Linux
receiver for `wsl-file-snapshot-v1`, reads a bounded native snapshot with that
user's filesystem permissions, verifies the echoed path/size/SHA-256, checks
that the connection is unchanged, and atomically publishes the local copy.
Windows output paths are never forwarded to Linux. No SSH/default-distribution
fallback, agent startup or conversation creation occurs. An older receiver or
a failed/changed/malformed transfer leaves an existing local destination intact.

This initial WSL copy path accepts regular files up to 10 MiB, including empty
files. Absolute paths and `~/` are interpreted by the selected Linux user.
It does not use lexical Windows mappings and can therefore copy files from a
WSL-native home directory. Larger files need another transfer route; no size-limit
bypass is attempted. Existing SSH/SFTP download behavior is unchanged.

The desktop's existing Download, Download and open and cache workflows consume
the same copy receipt. Downloaded copies retain their existing downloaded-file
marking and runnable-file guard. They do not gain permission to open a Windows
original: the separate mapped-original workflow still requires exact conversation
identity, mapping and matching local evidence.
