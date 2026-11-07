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
