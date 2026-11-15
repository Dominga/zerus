# Task-list session routing at 20k sessions

Measured locally on Arch, 2026-11-15, for zerus-bdqbb. The old task-list path
called `state::task_bindings` through `machines_for` before caller admission
and paging (`src/tasks.rs`, main `84adf8079`). That read the full JSON
of every current session to retain seven routing fields. The task stream's
window performed the same inventory independently.

The list now admits its caller and selects its rows before resolving their
local session references. A private, disposable metadata cache checks the
canonical binding namespaces and file versions on every lookup. Incomplete
coverage returns an explicit error; it never resolves a reference or proves
absence. Caller proof and task changes retain canonical record reads.

## Method

The scale audit's original synthetic generator supplies 20,000 current and
50,000 archived records, with eight unknown jobs containing 28 KiB outputs
and sixteen settled jobs per record. This run generated 18,071,380,000
bytes, 258,094–258,190 bytes per record. There are 100 catalog projects and
10,000 tasks seeded through the public
`ProjectStore`/`TaskDoc` APIs in `examples/task_list_scale.rs`. Project 000 has
100 unassigned tasks; project 001 has 100 targets pinned to one synthetic
session's exact run ID. Teams and Activity events are not required by this
binding-inventory path and were not seeded.

The benchmark worker proves its own real PID and birth identity in private
state. Every native call uses a private home, configuration and state root,
clears inherited `HGS_*`, and sets only the fixture's markers. No person
authority is claimed, and no real agent or service is started. The corpus is
held in a managed temporary directory and removed on success or failure.

Both binaries use Rust 1.99.1, unoptimized, with debug info and incremental
builds disabled. The original session-scale audit used a release build.
Each generator, task seed and measured command runs through the shared heavy
wrapper. Times below exclude its queue. Linux `/proc/PID/io` is sampled after
exit and before reaping; logical reads include executable, SQLite and IPC
bytes. Peak RSS comes from `wait4`. No global filesystem caches were dropped.
The unchanged corpus is used for all before/after commands.

`full` runs `hgs task list --project project-000`. `page50` and `page100`
add `--stage todo --limit 50` or `--limit 100`. `assigned50` uses project 001
with `--stage todo --limit 50` to require local session-reference resolution.

Baseline binary SHA256: `4152a6956c6b1164f42981104c23b7aa5cf1f45fd1240115ab8e054b77bb682f`.
Candidate binary SHA256: `d9d5ff59f8c95cf24fb418bf35931b0f38468e8769d736daf7364470cb91d311`.
Candidate source tree before this report: `97653136b92ec498f035fc0ae9e341e29c3af0d1`.

## Results

All four before/after JSON responses are byte-for-byte identical, including
task identities, labels, counts, revisions and cursors.

| Query | Version | Seconds | Logical read bytes | Physical read bytes | Peak RSS KiB |
| --- | --- | ---: | ---: | ---: | ---: |
| full | before | 49.492 | 5,162,670,374 | 5,236,920,320 | 61,288 |
| full | after | 0.413 | 790,492 | 45,342,720 | 47,760 |
| page50 | before | 39.609 | 5,162,870,941 | 4,657,635,328 | 62,520 |
| page50 | after | 0.706 | 683,047 | 4,427,776 | 48,484 |
| page100 | before | 46.926 | 5,162,563,065 | 4,991,856,640 | 55,264 |
| page100 | after | 0.810 | 999,287 | 59,731,968 | 49,896 |
| assigned50 | before | 53.657 | 5,162,946,713 | 5,142,700,032 | 60,180 |
| assigned50 | after | 1.229 | 8,554,590 | 31,793,152 | 91,148 |

Cold metadata migration is charged separately: one bounded worker
invocation, 52.485 seconds of child execution, 5,204,233,182
logical read bytes, 4,858,687,488 physical read bytes and
77,188 KiB peak RSS. Every invocation's reads and time are
included; the aggregate uses the maximum RSS. A worker's 60-second working
budget is checked between slices. An incomplete invocation retains verified
progress; a later invocation continues it. Warm-page results do not hide the
initial canonical record reads in that migration.

Warm routing still checks the bounded source-path inventory and reads the
metadata cache. It retains at most 32 MiB of metadata, examines at most
100,000 directory entries per inventory, and refreshes at most 256 records /
64 MiB of canonical bodies per foreground slice. Empty, unchanged and remote-only
pages bypass routing inventory. The cache neither rewrites canonical records
nor supplies caller or effect authority. A helper has at most sixteen slices
of 4,096 records / 512 MiB each.

The retained evidence is `artifacts/test-results/task-page-bounded/`:
`scale-measurements.jsonl`, `scale-equivalence.json`,
`metadata-coverage.json`, the source/binary seals, and per-command logs.
The fixture and generated build targets are removed after qualification.
