# Private Windows membership measurements

The opt-in CI input `retain_membership_probe` retains an optimized release-profile
test executable and its exact source/hash/profile manifest. The builder requires
Cargo's level-3 optimization and disabled debug assertions; a debug test image
cannot receive this artifact identity. The artifact name is
`membership-qualification-windows-release-<source SHA>`.
It is separate from distributables and publication
candidates, expires after one day, and grants no removal capability. Normal CI
does not retain it. The build runs under the maintained owned-job timeout; a
failed build cannot upload a partial fixture.

Use only a coordinator-booked machine slot, the exact reviewed CI artifact, and
a private fixture directory. Run one known case with `--exact --nocapture
--test-threads=1`; add `--ignored` only for the manifest's ignored case. Never
start the executable unfiltered. The original launcher must retain its process,
birth/image identity, owned job and pipes through actual exit and EOF. It must
start windowless, use the actual unelevated console user's token, and refuse a
locked or changed console. Do not add OS consent/security grants, enable token privileges, change security
software, touch native agents or use an installed GUI for this fixture.

The fixed measurement case is
`platform::windows_membership::native::qualification::private_measurement`.
Its per-child environment contains:

- `ZERUS_MEMBERSHIP_QUALIFICATION=readonly-v1`;
- `ZERUS_MEMBERSHIP_SCOPE`, an existing private fixture directory;
- `ZERUS_MEMBERSHIP_NONCE`, the original fixture UUID;
- `ZERUS_MEMBERSHIP_OBJECTS`, exactly `128`, `4096` or `32768`;
- `ZERUS_MEMBERSHIP_PASS`, `prepare`, `first-pass` or `warm`;
- `ZERUS_MEMBERSHIP_UNTIL_TICK`, one absolute `GetTickCount64` expiry set by the
  original launcher before process creation, at most 900 seconds ahead.

Run `prepare` once per size, then `first-pass` and `warm` against that retained
fixture and nonce. Each invocation has its own original bounded request; no
object renews its deadline. Preparation only creates new private data. It never
overwrites a directory. The other passes verify the recorded native root
identity, then walk all fixture objects, sample process/image/CWD reads and query
owned disk objects. Reparse points, caps, changed identities, failed reads and
expired bounds remain incomplete. A root-object refusal is never a per-process
denial waiver; later objects can still supply positive data.

The 900-second limit is an experimental measurement ceiling, not a default:
each coordinator-booked plan must select an explicit original pass deadline and
an outer fixture bound that fits its machine slot. It does not select
the product background budget or change the GUI's review timeout. Record the
launcher's whole-process duration as well as the reported phases. Pending native
I/O stays owned until physical completion; a timeout is not a drain witness. If
the original launcher cannot prove exit, job-empty and both EOFs, retain the
private scope and report it instead of starting another run.

Reports contain bounded timing/count/token context, without raw SIDs, PIDs,
paths or environments. `first-pass` is not cold-cache evidence: fixture creation
already warmed metadata. Do not reboot or flush global caches for this fixture.
Record genuine cold evidence separately if a safe independent condition exists.
Use `--nocapture` for the existing native observation cases, since normal libtest
captures successful-case output.

Neither successful class47 replies nor these measurements prove complete
filesystem coverage. Missing use classes, reserved-API behavior, unelevated
actual-caller evidence and the measured final budget remain part of the later
actual-source G1. Windows removal, program stop/Force and leftover deletion stay
closed. The original launcher owns bounded cleanup of only its verified private
fixture after every original resource has settled.

The initial debug-profile fixture at source `fdc5e4bde` measured 128-file scans
at about three seconds and 4096-file scans at about 87–89 seconds on Windows
build 26200, x64, NTFS. Both 32768-file scans exhausted their original 120-second
deadline and remained incomplete. Those measurements are debug observations,
not production timing or a selected product budget. Preserve them separately
from the optimized fixture, including their changed process censuses and the
absence of cold-cache evidence.

The opt-in hosted job also invokes the separate ignored `hosted_measurement`
case against 128 and 4096 files, using the same measurement body, readers and
original deadlines. It reports its actual token context under the closed scope
`hosted-cost-only`; it does not require or qualify the console user's medium,
unelevated context. The strict `private_measurement` case remains unchanged in
that requirement. Hosted admission additionally requires the GitHub-hosted
environment and the explicit ignored test name. Its source-bound timing summary
is retained separately as `membership-hosted-timing-windows-<source SHA>` for
one day; no publisher consumes it.
Failed comparisons may retain bounded diagnostics with `completed: false`;
they are never qualification evidence.

Test-only counters separate metadata open, file identity, output-buffer reset,
the original native query, completion polling, actual pending sleep, decoding
and physical file close. They add no native query, retry, timing-policy change
or permission. Compare these phases before selecting a background budget.
Differences in token, OS, filesystem, storage and filter drivers remain part of
the comparison; a hosted-versus-PC difference alone does not identify a driver
as the cause. No security setting or timer-resolution change is part of this
diagnostic.
