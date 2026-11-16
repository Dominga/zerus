# Bounded content-search recovery

This investigation uses synthetic state exclusively. No installed application,
native agent, account, transcript or session state was changed.

## Causes

Before this change, `search_index/query.rs` advanced discovery and ingestion only
during a first-page request. Its 32-file, 4-MiB, 250-ms slice had no background
owner. A search could therefore leave hundreds of paths pending indefinitely.
`unknown_metadata` means a queued path has no decoded identity;
`invalid_metadata` means a staged path was rejected, rather than a verified empty
conversation. Neither flag proves that the person's history is absent.

`search_index/metadata.rs` read and charged entire canonical JSON records before
selecting the fields used by search. Private shell-job payloads consumed the
foreground discovery budget. The RED 200-record control reached only 15 records;
a single 9-MiB private payload could never fit the original metadata slice.

Two additional discovery defects affected recovery: the catalog classified ID
records using the directory basename, although the default state root is itself
named `sessions`; and bounded ID lookup repeatedly restarted before late records
when the label manifest was absent. Controls reproduce both failures. A continuously replaced binding also restarted
every census page. The new control reaches all 1,024 paths while a real atomic
writer runs between pages; that pass remains incomplete, and a later clean
pass proves closure. Namespace replacement still restarts discovery.

## Resulting contract

One coalesced helper prepares compact metadata before historical ingestion.
One bounded preparation slice groups its durable writes; the control rejects
the former 19-commit cost for 16 headers. Historical ingestion retains its
per-source atomic commit contract. Its
directory census resumes in 256-entry, 1-MiB-path, 25-ms steps. Ingestion retains
the existing interactive limits; transient canonical metadata I/O is separately
bounded to 256 MiB aggregate per background slice and 64 MiB per record.
Only one raw body is read at a time, and at most 32 source handles are retained. A temporary SQLite writer/read overlap is retried inside the same lease. A failed
step drops its census iterator and connection before reopening, so a rolled-back
page never skips paths. Permanent errors remain unavailable.
A helper has a finite
120-second / 8,192-step lease. A later request can resume an unfinished campaign.
The desktop retries a zero-result partial reply once after a 500-ms delay
plus the existing debounce. A new query, hiding search or disposal cancels it.
Generation checks and the four-request concurrency cap still apply. This is a
read-only refresh; it cannot replay a send or lifecycle operation.

For at most two seconds, preparation of at most 500 paths retains its launch
priority. Larger inventories and historical ingestion yield to background CPU
and I/O priority immediately. This avoids starving a waiting small search under
load while keeping bulk work in the background.
It owns private locks, inherits no `HGS_*` variables except its selected state
root, and disconnects stdin, stdout and stderr.

The disposable metadata cache holds at most 32 MiB and 50,000 entries, with a
1-MiB entry limit. Every reuse opens the canonical source without following its
final symlink and verifies its native file version. Replacement, deletion,
in-place edits, unreadable files and changed identities retain incomplete
coverage. Source capture and cache publication bind to the same held file.
Private job bodies are read transiently during preparation and are not retained.

Foreground discovery keeps its 500-file, 2,000-directory-entry, 4-MiB and 500-ms
limits. Prepared records charge only the search projection. A small computer
can wait up to 2 seconds for preparation; a large inventory returns bounded
partial coverage while its helper continues. Complete fresh fallback suppresses
the desktop's false contents-unavailable warning; an incomplete fallback,
unverified hit or outstanding cursor keeps its existing warning.

DeepSeek record namespaces and native archive metadata remain discoverable.
OpenCode has no admitted public-content adapter; its presence remains explicit
incomplete coverage and is tracked separately by `zerus-3tmfc`. No helper starts
or reloads a native provider. Search metadata grants no lifecycle authority.

## Reproduction and accounting

Build the CLI with `cargo +1.99.1 build --release --bin hgs -j 4`, using the
computer's heavy-build lease. Run `bench/search-recovery.py` with that binary,
a session count, private-payload bytes and either `indexed-first` or
`legacy-first`. For example:

```sh
python3 bench/search-recovery.py target/release/hgs 200 9437184 indexed-first
python3 bench/search-recovery.py target/release/hgs 20000 270000 indexed-first
```

The harness creates canonical private temporary roots, clears inherited
`HGS_*` values, and gives each record a genuine UUID-named synthetic Codex
transcript. Only the last session contains the query needle. Each record also
has 8 KiB of public prompt/reply text and the selected private job payload.
The harness syncs only its own canonical files and namespace before timing,
matching existing durable records. This excludes fresh bulk-import writeback
from query latency. No global sync or cache flush is used.
All synthetic state is removed on success and failure. Evidence is retained
under `artifacts/test-results/search-recovery/`.

An index-cold request starts without a content index. On Arch, set
`SEARCH_EVICT_FIXTURE=1` after selecting disk-backed `SEARCH_SCRATCH`: the harness
uses `POSIX_FADV_DONTNEED` on its own synced records and transcripts before the
first request. This requests private-file cache eviction without a global
flush. Each row records that policy and physical read bytes. macOS measurements
are index-cold with OS-warm fixture files; they make no disk-cold claim.
Indexed-first and legacy-first campaigns use independent fixtures.
Foreground elapsed time includes helper readiness. Linux logical reads come
from the unreaped CLI child's main-thread `/proc` counters, excluding I/O folded
in from completed background helpers. Physical reads are recorded separately.
Linux peak CLI RSS uses its kernel VmHWM sampled every millisecond; macOS uses
`/usr/bin/time` child accounting. A small launcher avoids inheriting the Python
fixture generator's RSS floor. Live detached helper I/O and RSS are separate
from foreground accounting. macOS has no Linux logical-read counters here.

## Controls

The regression set covers realistic large private records, native-version
changes, source removal, cache startup-lock retry, parent-path aliases, cache
size bounds, resumable directory pages, default-root naming, ID records without
a label manifest, native namespaces, unsupported-content honesty, preparation
priority and deferred-header fairness. A Python campaign issues one request and
observes all 800 genuine transcript sources finishing without further requests.
Canonical bindings, transcripts and journals are compared byte-for-byte.

The final controls include 118 Rust search tests, 17 labels controls (two
existing ignored), 10 client-core search tests, 23 Python CLI methods and
15 frontend search-model controls. Strict Linux, local Windows cross-target and
Darwin cross-target clippy use Rust 1.99.1 with warnings denied. Client-core
formatting, TypeScript checking and the source privacy guard also run locally.
Twenty-one runtime mutants cover the original metadata charge, file-version
proof, startup contention, discovery resumption and layout, incomplete coverage,
worker ownership, false warnings, metadata priority/fairness, durable-write
batching, bounded foreground priority, busy-worker survival, redundant updates,
retained handles, batch fill, UI refresh and both churn guards.

## Arch measurements

Both binaries use the same optimized release profile. The baseline Rust source
is `2e0dfb8cdc2837e8756fa1fa907b5f0a1153e257`; the qualified production candidate
is `67eedd62b765806451d96ee0ffa1a7ce3e72a134`. The baseline was built by temporarily
overlaying that revision's changed Rust files in the private checkout, restoring
all bytes before building the candidate. This is a source comparison, not a
claim that the baseline executable came from a separate clean checkout.

These are single local measurements under shared-machine load, not percentiles.
Bytes and RSS below belong to the foreground CLI, excluding detached helper I/O.
All first requests used disk-backed private state with per-file eviction requested.
The needle exists only in the last session.

| Source and fixture | Request | Time | Logical read | Physical read | Peak RSS | Coverage / hits |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Baseline, 200 × 270 KB private | Legacy first | 89 ms | 4.00 MiB | 19.75 MiB | 13.7 MiB | 15/200, partial, 0 |
| Candidate, 200 × 270 KB private | Indexed first | 493 ms | 4.28 MiB | 0.62 MiB | 20.9 MiB | 200/200, complete fallback, 1 |
| Candidate, same fixture | Indexed warm | 36 ms | 3.60 MiB | 0 | 21.7 MiB | 200/200, complete fallback, 1 |
| Candidate, same fixture | Legacy warm | 26 ms | 3.39 MiB | 0 | 19.9 MiB | 200/200, complete, 1 |
| Baseline, 200 × 9 MiB private | Legacy warm | 11 ms | 0.01 MiB | 0 | 9.7 MiB | 0/200, partial, 0 |
| Candidate, 200 × 9 MiB private | Indexed first | 2,114 ms | 4.38 MiB | 1.33 MiB | 19.1 MiB | 99/200, partial, 0 |
| Candidate, same fixture | Immediate legacy follow-up | 2,141 ms | 6.56 MiB | 0.39 MiB | 18.9 MiB | 200/200, complete, 1 |
| Candidate, same fixture | Indexed warm | 36 ms | 3.60 MiB | 0 | 21.3 MiB | 200/200, complete fallback, 1 |
| Baseline, 20,000 × 270 KB private | Legacy warm | 37 ms | 4.00 MiB | 3.93 MiB | 13.7 MiB | 15/20,000, partial, 0 |
| Candidate, 20,000 × 270 KB private | Indexed first | 210 ms | 5.23 MiB | 1.88 MiB | 19.7 MiB | 79/20,000, partial, 0 |
| Candidate, same fixture | Indexed later | 175 ms | 9.02 MiB | 1.58 MiB | 22.1 MiB | 492/20,000, partial, 0 |

The baseline indexed path incorrectly reported ready with zero sources because
of the default-root classification defect; its fast empty response is not
successful coverage. The candidate's 200-record, 9-MiB case must initially read
about 1.8 GiB in the background. Its first reply remains honestly partial;
the immediate follow-up completes about 4.3 seconds after the first call began.
The bounded UI refresh addresses that temporary zero. No result claims full
20,000-session coverage inside one interactive request.

The two-second sleep in the harness occurs after the first two requests;
`indexed-after-2s` therefore describes a later warm observation, not a promise
that all background work completed two seconds after the initial request.
The final JSON controls require every candidate foreground call below 12 MiB
logical read and 96 MiB RSS, raw fallback metadata at most 4 MiB, complete
200-record warm fallback, and honest partial 20,000-record discovery.
The canonical-read control performs 200 reads and retains at most 32 source
handles. The redundant-write control verifies that untaken metadata rows are
not updated; a separate already-prepared control observes no readiness commit.

Private native macOS qualification uses the same production source, canonical
private temporary roots and the shared one-step heavy lease. Its OS-warm timings
and final native result are attached to the PR when that bounded run finishes.
It does not install an application, contact a native provider or use owner state.
