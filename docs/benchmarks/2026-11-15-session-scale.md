# Session-state scale audit, 2026-11-15

The worst measured path was legacy search: it produced no result within a
300-second guard, after reading 36.14 GB and reaching 18.08 GiB RSS. Indexed
search's incomplete-coverage fallback used the same inventory. The search fix
bounds discovery before histories are loaded and reports partial coverage.

## Fixture and accounting

Baseline: main `567b34b90f4c2a716bd13f2278cea78dacb8c5e6`, Rust 1.99.1,
production release profile (thin LTO, stripped), Arch Linux kernel 7.2.14,
32 logical CPUs and approximately 125 GiB RAM. These are single observations
on a shared, loaded machine, not isolated-machine latency percentiles.

Private synthetic state contained 20,000 current saved/interrupted bindings,
50,000 archives, 100 catalog projects, 100 operation-written Teams, 10,000 Loro
task documents (100 per project), and two million indexed Activity events
(100 per current session). Records were 258,085–258,181 bytes before the single
incremental title update: 18,070,750,000 bytes total. Each retained eight unknown
jobs with bounded output and sixteen compacted completed jobs. Commands were
small and carried their digests; retained jobs never exceeded the current caps.

The first archive build, warm archive page, no-archive listing and guarded
legacy search were measured immediately after the 70,000 record files were
generated, before project/Teams/task/journal augmentation. Remaining baseline
rows and final search runs used the augmented fixture. The source record
population was retained, apart from the single archive title update. Legacy
search never reached its history scan before its guard. These observations are
not a paired cold-cache comparison, and the two listing rows do not isolate the
cost of the archive preview.

There were no native agents or owner records. Current records used the legacy
label layout, absent PIDs and a shared synthetic public transcript. Task CLI
reads used a real PID/birth proof for the audit's own worker in private state;
the original synthetic record was restored afterwards. Thus this measures
large saved-state discovery, not 20,000 running agents, provider network/model
latency, renderer layout, signing or installation.

The service ran in passive mode; background lifecycle and notification workers
were not activated. Its native watch producer and real catalog pages were
measured separately.

Logical reads are final child `/proc/PID/io` `rchar` values sampled before
reaping; disk reads are `read_bytes`. Peak RSS is `wait4` for finite children
and `/proc` VmHWM for the service and native watch. Measurements include
executable, SQLite and IPC reads, not just JSON bodies. The passive service's
page-walk row reports service I/O; its elapsed time also includes one CLI proxy
launch per page. Full GUI projection validates real service page receipts and
uses `client/core`'s ordinary row projector and coalescer, releasing each page.

The synthetic data was generated before measurement. No global caches were
dropped: “first build” means a cold derived archive catalog, not cold disks.
The failed empty-stdin archive preflight is excluded. Search's guard stopped
only its private process group after 300 seconds; shutdown completed at 302.22
seconds. Its row is a lower bound, not a successful query.

## Baseline observations

GB and MB below are decimal; RSS is MiB unless GiB is explicit.

| Operation | Elapsed | Logical reads | Disk reads | Peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Archive catalog first build, first 50-row bounded page | 61.263 s | 40.141 GB | 1.246 GB | 27.1 MiB |
| Warm archive page | 0.052 s | 14.205 MB | 0 | 30.8 MiB |
| Archive incremental refresh, one replaced record | 4.627 s | 1.162 GB | 124.195 MB | 28.9 MiB |
| `hgs ls --json --local --no-archives`, 20,000 rows | 33.759 s | 10.492 GB | 4.621 GB | 5.17 GiB |
| `hgs ls --json --local`, with recent archive preview | 55.994 s | 11.003 GB | 5.493 GB | 5.18 GiB |
| `hgs search --query synthetic --limit 20` | >300 s, stopped at 302.222 s | 36.142 GB | 6.380 GB | 18.08 GiB |
| Passive desktop service startup and status | 0.254 s | 53.621 MB | 39.748 MB | 21.6 MiB |
| First native watch catalog ingested by service, 20,000 rows | 53.174 s | 16.658 GB | 5.530 GB | 5.20 GiB source; 31.0 MiB service |
| Service catalog walk, 157 pages of at most 128 rows | 7.856 s; first page 0.052 s | 82.579 MB | 0 | 36.5 MiB service |
| `client/core` validation and GUI row projection, same 20,000 rows | 0.633 s | 63.940 MB | 0 | 23.5 MiB |
| Open Activity, first 100 events from two-million-event journal | 0.031 s | 0.644 MB | 0.664 MB | 22.8 MiB |
| Task CLI full list, one 100-task project | 1.991 s | 5.162 GB | 6.119 MB | 36.1 MiB |
| Task CLI first 50-row page | 1.887 s | 5.163 GB | 0.213 MB | 36.2 MiB |

`--local` suppresses peers; this private fixture had no peers. There is no paged
`hgs ls` wire body. The paged listing measurement is the desktop catalog API.
The native catalog source first materializes its listing before publishing
pages, so a responsive passive service does not imply a fast first session list.
Serially adding the measured source, proxy walk and core projection gives about
61.7 seconds for this audit pipeline; renderer/native presentation is excluded.

## Offenders and scope

1. **Search inventory and Cartesian archive comparison.** Baseline
   `src/state/search.rs:64` loads every binding and archive, then checks
   `archive::equivalent` for each binding against every archive
   (`src/state/archive.rs:193`). The 500-session and 256 MiB history budgets
   apply afterwards. On this input that means 20,000 × 50,000 potential
   comparisons after full materialization. Indexed search's fallback enters
   the same path (`src/state/search_index/query.rs:63`). This PR fixes it.
2. **Synchronous archive catalog reconciliation.**
   `src/state/archive_catalog.rs:99` walks every archive after a directory-stamp
   change; its loop reconciles references, updates rows and reads full metadata
   repeatedly. Even one replacement visits the whole catalog. Follow-up:
   `zerus-scbyk`. A future bounded background rebuild must preserve complete
   restore/reference proofs; partial display coverage cannot prove absence.
3. **Full saved-record listing and watch initialization.**
   `src/state/archive.rs:312` calls `storage::records`
   (`src/state/storage.rs:779`), collecting every full record before projection.
   Label-layout bindings are read twice. Watch initialization additionally
   calls `binding_identity` for every binding (`src/watch/mod.rs:390` and
   `src/state/watch_source.rs:379`), which reads and parses full JSON just to
   retain names. Follow-up: `zerus-63r5s`.

Task paging has another real scan: `src/tasks.rs:296–304` constructs `Machines`
with `state::task_bindings` before actor selection or pagination, and
`src/state/mod.rs:1438` reads every binding's raw bytes while retaining routing
metadata. The 5.16 GB is routing setup for both person and agent callers, not
caller-proof or Loro page-read cost. Follow-up: `zerus-bdqbb`.

The desktop service itself retained little memory in this scenario. Its first
catalog bottleneck was upstream canonical-state materialization. The fix here
does not alter listing, archive reconciliation, task authority or any production
consumer of the advisory process index.

## Search change and regression evidence

Unindexed search and indexed fallback now rotate through bounded source
directories, charge directory entries and raw bytes before history search,
retain only search metadata, verify file/record/layout identities and preserve
published name routes. Missing or incomplete discovery sets `truncated: true`
even if no content matches. Discovery is display-only, not a complete inventory
or evidence for an action. Recent ordering applies within the admitted subset;
it does not imply globally newest results from unvisited records.

The initial rewrite's name-route control passed on baseline and failed on that
rewrite; the final implementation preserves the canonical route and ignores
unpublished binding files. The raw-volume controls first failed on baseline at
84.44 MB (ordinary) and 86.82 MB (indexed fallback), against a 12 MiB ceiling.
The no-match coverage control also failed on baseline. The focused runtime
controls and opt-in benchmark are retained in `tests/test_search_scale.py` and
`scripts/bench/search-scale.py`; see `scripts/bench/README.md`.

The final full-fixture run on the rebased implementation returned ordinary
search in **0.063 s**, reading **4.682 MB** logically (**8.114 MB** from disk),
with **23.0 MiB** peak RSS and 20 hits. Indexed search plus its explicit fallback
took **0.564 s**, reading **9.980 MB** logically (**9.912 MB** from disk), at the
same peak RSS. The incomplete index returned 0 indexed hits and 20 fallback
hits; coverage remained explicitly incomplete. The smaller 2,000/5,000 fixture
used 4.163 MB / 23.7 MiB for ordinary search and 6.850 MB / 23.7 MiB for indexed
search plus fallback. Logical discovery work remained independent of inventory
size in these fixtures; these observations do not establish latency percentiles.

Local validation passed: 97 Rust search/index tests, 20 CLI Python search
controls, Rust 1.99.1 all-target clippy with warnings denied on Linux and the
local Windows cross target, 28 source checks, zero privacy findings, and
formatting of new Rust files/blocks. The final benchmark runs passed their
read/RSS ceilings. Three compiled mutants were killed by genuine assertions:
restoring full inventory, dropping incomplete-scan truncation, and ignoring
published name routes. The quiet-field omission preimage also failed its
preservation control (baseline passed; final passed). Quiet public snapshot
text retains the original raw byte charge; unrelated quiet bookkeeping is
excluded. No hosted workflow was dispatched or polled as a gate.

Small raw logs, metrics, source hashes and generator helpers are retained under
`artifacts/test-results/scale-audit-20261115` and `.ci-build/scale-audit` in the
audit checkout. Large synthetic replies were replaced by their byte counts,
SHA-256 hashes and row counts. The run removes its private synthetic state and
own Cargo target after validation.
