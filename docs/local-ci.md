# Local CI before GitHub

On Arch, run the command from the checkout containing the runner, choosing an
exact commit hash. The checked commit does not itself need to contain the runner.
Nothing from the caller's uncommitted tree is added to the tested source.

```sh
python3 scripts/ci/local-linux.py --sha COMMIT \
  --output /path/to/new-private-results \
  --cache /path/to/private-local-ci-cache \
  --online --workers 12
```

The host needs Python with PyYAML, bubblewrap with unprivileged user namespaces,
D-Bus, Fish, the Rust 1.99.1 toolchain including rustfmt/clippy, and
the native build dependencies documented in installation.md. Online mode also
needs slirp4netns (a private executable can be selected with `--slirp`). The
CLI job builds its workflow's exact tmux in its private home. Additional private
fixture executables, including Fish, can be supplied with `--tool-bin`; a missing
Fish leaves the CLI job not run rather than silently skipping shell coverage.
PostgreSQL checks use Docker, the workflow's pinned official image, a unique
private network and an original labelled container, without published host ports.
They never connect to a pre-existing database. Package installation on the host
is not part of this command.

The default worker limit is 12. CPU affinity, Cargo/libtest and Python worker
counts are capped. Before starting each check, the runner waits if the one-minute
host load exceeds 20; ten minutes above that ceiling leaves the check not run.
It lets an already running check finish. A 40 GiB free-space floor stops new work.
Use `--workers` and `--max-load` to lower those limits for a busy host.

Each job uses its own detached clean checkout, private home, PID/network/IPC
namespaces, short temporary socket directory and D-Bus session. It inherits no
`HGS_*`, desktop-service, agent-account or display selection. Online mode gives
the namespace outbound access with host loopback disabled. No host credentials,
agent homes, tmux socket or Docker daemon socket are mounted into the checks.
Headless Xvfb uses software Mesa; the owner's display and GPU are not used.
Network setup may be recreated at most twice after a verified refusal before
the test-entry write. The old namespace/helper must settle first. The entry is
latched before writing; any uncertainty after it, or any test failure, is never
retried. These preparation refusals stay visible in the report.

Only public tool/cache data are seeded. The cache and Cargo target are
locked for the run. `--target /path/to/exclusively-used-target` can reuse a build
tree; do not select a target another build is using. The exact CLI and fake agent
are rebuilt before Python integration; failure refuses reuse of an older binary.
The CLI's debug, shell smoke, Python and release-build bodies are read from its
own script. Shell failure does not conceal the later Python failures. An unknown
script layout refuses rather than guessing how to split it.

`report.json` is saved after checks, with the commit, workflow/runner hashes,
step bodies and hashes, tool versions, exit codes and raw-log SHA256 values.
The exact runner/helper source is retained in `harness/` alongside its hashes.
Every job in `checks.yml` has a pass, fail or not-run status. A partially executed
job is not run unless a check actually failed, in which case it is fail. Logs
and the checkout remain in the private output directory for diagnosis. Namespace
children and network helpers physically exit before their check returns. A job
container retains its descendants until the job ends; timeout stops that original
container and never adopts a replacement. Owned service containers/networks are
removed at job completion. Never delete evidence
still cited in a report.

Phase 1 supports frontend, Quick, CLI, native Linux, connector, PostgreSQL,
Android and Arch package checks. Android installs public JDK 17, command-line
tools and the workflow's SDK/NDK/target versions into private caches. It never
changes the host JDK, Rust toolchain or SDK. `--android-cache` can select space on
another filesystem. The Arch package job runs its pinned container and preserves
its original unprivileged builder and installed-package verification.

GitHub checkout/cache/upload operations and hosted-machine disk cleanup
are replaced by local infrastructure, not executed on the host. Downloaded Node
uses the workflow's declared major and official checksums. Relay image/memory
qualification stays **not run locally, GitHub only**, by owner decision on
2026-11-07: rootless Docker has no delegated cgroups here. The host daemon socket
is not exposed to tests and the 256 MiB check is not weakened. Other actions,
unknown expressions and other platforms remain explicitly not run until their
local adapters are supplied. This is a local preflight, not
a release candidate builder or a substitute for GitHub's final clean matrix.

Exit 1 means a failed check; exit 2 means incomplete coverage or a refused local
prerequisite. An incomplete plan cannot report a fully green matrix. `--plan`
writes the exact job inventory without starting checks. `--jobs quick,cli` and
`--match REGEX` narrow a diagnostic run; exclusions remain visible in the report.

For the original 0.48.3 regression control, use commit
`83b4c5f21569c6a3ee12f9d47243e667c547e361` (the Fish picks on the 0.48.2 main
base). One run must expose the `creation_binding.rs` format diff, the
`test_hgs.sh` exact-ended-row failure and `test_pause.Recipes`' obsolete dry-run
parser. Do not fix the checked source to obtain the control result.

Native macOS and Windows adapters follow the Arch command. Their jobs stay
not run here; Linux fixture success is not native Mac or Windows qualification.

## Native Mac preflight

Run `scripts/ci/local-macos.py` on the Mac, selecting a full commit hash and a
new result directory below one private root. It takes the shared
`~/zerus-dev/.heavy.lock` without displacing another holder and releases only
its own lock. The Mac adapter reports every workflow job, including other
platforms and GitHub aggregates as not run.

```sh
python3 scripts/ci/local-macos.py --repo /path/to/public-source-cache \
  --sha FULL_COMMIT --root /path/to/private-mac-ci \
  --output /path/to/private-mac-ci/new-results --workers 4
```

The host supplies Xcode and Homebrew Python, tmux and build libraries. The
runner supplies a checksum-verified Node 24 and copies only the pinned public
Rust toolchain and registry into private caches. Rust components/targets are
added to that private copy. It does not install packages, select global Xcode,
accept licenses or use signing identities. Mac filesystem and signal isolation
uses Seatbelt, private homes and short temporary directories. This is not a
Linux PID or network namespace: ordinary outbound IP is available. Before any
test body, real synthetic controls must prove that file contents and process
signals outside the sandbox are refused, while an owned child can be reaped.

The system's setuid `ps` cannot run in Seatbelt. A private copy keeps its code,
removes setuid/private entitlements and receives an ad-hoc signature. The report
records the system and copied hashes and mode. It queries actual current-user
kernel data; it is not a fake process table or an installed helper observation.
The startup canary is created with exclusive, no-follow creation inside an atomically created private directory outside the test sandbox. Cleanup verifies the original directory and leaf identities, retaining replacements. Before any check, the runner also verifies its own Seatbelt policy denies SecurityServer and the securityd/secd credential-service namespaces. That control uses `sandbox_check` only: it makes no service lookup or keychain-item request. A check that needs these denied services stays unavailable; the runner never widens the boundary for it.

The system binary and keychains stay untouched. Native original-child/signature
controls continue to select their own original executables.

The private Bash launcher preserves real login startup and the original command
and positional arguments. After startup, it restores only the private `ps`
directory ahead of system tools: macOS `path_helper` otherwise moves the
unusable setuid `/bin/ps` ahead of it. Explicit shell choices remain explicit.
The `mktemp` shim preserves explicit template paths and `-p` parents; only an
implicit parent is redirected to the private job directory. Neither routing
change adds Seatbelt permissions.

For a reviewed runner fix against frozen product source, select the separately
reviewed runner with `--runner-sha FULL_RUNNER_COMMIT`, while `--sha` remains the
product commit. Supply the unchanged `local*.py` helpers from that runner commit.
Preparation verifies their bytes against the immutable runner tree before any
helper executes, then compiles those captured bytes directly. Cached bytecode
and caller package paths cannot supply helper code. The command selects the
same interpreter in isolated mode before its other imports. The workflow and
job checkouts still come from the product tree. The
report records both `runner_commit` and `source_commit`, plus helper hashes.
This does not establish trust in an unreviewed runner: review is a prerequisite.
Compare affected target results with the original run rather than replacing its
failures. For CLI targets, include `cli-macos-debug` as their required exact-source
binary producer.

Independent exact-source CLI and fake-agent builds precede integration and
real-agent tests. Their executable hashes bind that local handoff; a failed
unit suite still fails its job and cannot conceal those independent suites;
GitHub artifact admission/download/sealing is explicitly local infrastructure,
not a claimed artifact acceptance. The source script's remaining shell, Python
and release bodies run independently. Build/test results, sandbox prerequisite
failures, ignored controls and missing platforms remain distinct in the report.

The default Mac worker limit is six, the load ceiling ten and the disk floor
10 GiB. Use four workers on the 16 GiB machine, and lower limits when needed.
No installed GUI, LaunchAgent, agent home or existing tmux server is selected.
Display tests require the coordinator's current window. GitHub remains the final
clean matrix and sole release-candidate producer.

Apple's compiler/Python shims also use the OS per-user temporary directory even
when `TMPDIR` is private. Seatbelt permits only its `xcrun_db` cache names there,
plus nonrecursive reads of their ancestors. This is public compiler-cache data;
the rest of that directory, owner preferences and credentials are excluded.
The private Rustup copies disable self-update, so target installation cannot
try to replace the read-only runner proxies or the owner's Rustup.


## Native Windows preflight

`scripts/ci/local-windows.py` reads the selected exact source's workflow and
materializes clean Git checkouts from that immutable commit, excluding staged,
untracked and ignored files, then runs supported Bash bodies with private profiles and verified
Python3.13.15, Node24 and Rust1.99.1 tools. The private root supplies public tool
copies; no installer, global PATH or default-toolchain mutation is performed.
The controller needs PyYAML. For example, within a coordinator-admitted slot:

```powershell
python scripts/ci/local-windows.py --sha FULL_COMMIT --repo PRIVATE_SOURCE `
  --root PRIVATE_ROOT --output PRIVATE_ROOT/new-results `
  --node-directory node-v24.22.2-win-x64 --until $SlotEndUtc
```

The MSVC environment comes from the existing Visual Studio tools, with owner
user PATH entries removed and the actual MSVC linker selected ahead of Git's
unrelated `link.exe`. Four workers and a 10 GiB free-space floor bound each run.
`PRIVATE_ROOT/STOP` pauses before the next command and stops only the current
original private job. Each gated worker joins a non-breakaway kill-on-close job
before release. An assignment refusal closes the unreleased gate and settles
the same original worker. Completion waits for that job's physical settlement; no WMI,
PID adoption, path wildcard cleanup or existing scheduled task is used.

This is a private-profile preflight, not a Windows security sandbox. Use only
reviewed source under the coordinator's machine slot. Installer/runtime setup,
hosted frontend adversaries, shared desktop/registration checks and outside-job
bodies stay **not run**, with their reasons. No local result qualifies an
installed pair, public package or hosted adversarial check. A failed exact-source
build cannot hand an older cached executable to its later integration suite.
The report retains all workflow jobs, unchanged bodies/hashes, tool versions,
logs and the exact helper source. Remove private build trees when returning the
slot and retain cited reports/logs separately.


Mac fixture runs receive `ZERUS_TEST_SHORT_TMPDIR` pointing at the job's owned,
short private temporary directory. The test fixtures otherwise keep their
existing `/tmp` defaults; the native worker's 104-byte socket check is unchanged.
OpenCode's private fixture explicitly supplies its own TMPDIR as well. The
installed Arch bundle check resets XDG roots along with HOME, so its fake manager
and the tested setup command inspect the same private drop-ins. No owner or
container-builder profile is used as fixture state.

iOS remains **not run locally, GitHub only**: its global Xcode selection and
device-service environment are not isolated by this preflight. Selection-helper
calls, including mixed bodies, are refused rather than substituted or executed.
The other Mac jobs use the supplied SDK through a private DEVELOPER_DIR, without
changing global tool selection or accepting a licence.
