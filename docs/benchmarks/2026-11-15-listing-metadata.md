# Advisory listing metadata: writer and storage qualification

The first slice of zerus-63r5s publishes display inputs and repairs an advisory
SQLite cache. The separately reviewed listing/watch reader slices require full
row parity, live-writer catch-up and bounded bootstrap scheduling. These numbers
qualify the storage/publisher slice. They do not measure a switched `hgs ls`,
watch status projection, native provider overlays or GUI rendering.

The original full-record bottleneck is recorded in
[the session scale audit](2026-11-15-session-scale.md). Canonical JSON and all
existing action, lifecycle, ownership and custody checks remain authoritative.

## Fixture and method

The opt-in driver `scripts/bench/listing-metadata.py` creates and removes its own
private real-file fixture. Records have unique session/run/conversation UUIDs,
100 project labels, an interrupted Codex phase, no native PID, eight unknown jobs
with bounded 28-KiB outputs and sixteen settled jobs with 1-KiB outputs. Each
record is about 261,694 bytes. No native agent or owner runtime participates.

This benchmark has 20,000 and then 50,000 **saved bindings**. The earlier audit's
separate 50,000 archives are outside this display index. The first 20,000 files
are written as legacy records and backfilled once; the additional 30,000 use the
instrumented publisher. At each cardinality, 200 interleaved pairs compare the
same original atomic file write with and without advisory publication. Each
sample serializes the real record, fsyncs and renames it. SQLite uses FULL
synchronous durability. The paired column is the distribution of each pair's
signed added time, rather than a subtraction of independently sorted percentiles.

The owner is an unoptimized Rust 1.99.1 unit-test binary. This retains real
filesystem and SQLite durability but includes debug serialization cost. The
shared Arch heavy wrapper reported load 34.22 at admission and 19.64 at exit;
this is one run under shared load, not an isolated latency qualification.

## Results

| Saved bindings | Canonical p50 / p95 ms | With index p50 / p95 ms | Paired added p50 / p95 ms | Retained index MiB |
| ---: | ---: | ---: | ---: | ---: |
| 20,000 | 8.127 / 10.779 | 10.715 / 14.185 | 2.549 / 5.262 | 58.48 |
| 50,000 | 8.751 / 16.693 | 11.732 / 98.205 | 3.080 / 53.498 | 166.07 |

The 50k tail is material: paired added p95 was 53.498 ms. The run does not
attribute that tail to a particular scheduler, storage or SQLite operation.
Median overhead meets the intended few-millisecond range; the tail remains
visible for subsequent reader/load qualification.

Runtime peak RSS was **48.91 MiB**. Index size includes SQLite, WAL and SHM at
the observation after owners close: main files were 61,325,312 and 174,141,440
bytes; WAL/SHM were zero then. This is retained footprint, not a measured peak
WAL size. The cache's page-count ceiling remains separate from canonical writes.

| Bindings | Initial repair | Raw canonical bytes | Post-write repair | Raw canonical bytes | Metadata pages | Page read time / logical bytes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 20,000 | 98.308 s, 1,677 steps | 5,233,600,000 | 7.449 s | 0 | 157 | 0.963 s / 21,753,315 |
| 50,000 | 20.011 s, 784 steps | 0 | 23.394 s | 0 | 391 | 4.142 s / 55,990,610 |

Each repair call checked the 256-entry, 128-projection-row and 16-MiB raw-read
limits. The 50-ms clock is checked between units of work; it cannot preempt an
individual filesystem or SQLite operation. The driver pumps steps directly;
these durations exclude the production recovery worker's scheduling interval.
Warm repair verifies file versions and reuses checked small seeds. Page reads
use immutable generations and bounded metadata rows. Logical page bytes are the
owned process's `/proc/self/io` `rchar` delta, including SQLite/open overhead.

Population took 209.505 s for the first legacy 20k files and 405.888 s for the
additional published 30k. Total test execution was 783.16 s. The driver removed
the entire synthetic fixture on exit and retained small JSON/log evidence under
`artifacts/test-results/listing-metadata/`.

## Reproduction

Run one heavy step at a time with the shared local wrapper. Choose a private
scratch parent with at least 18 GiB free:

```sh
/path/to/shared-heavy python3 scripts/bench/listing-metadata.py \
  --scratch-parent /path/to/private-scratch \
  --output artifacts/test-results/listing-metadata/scale-full.json
```

`--small` uses 64/128 bindings to check the driver. The driver clears inherited
`HGS_*`, keeps targets in the checkout, uses `-j 4`, and owns its temporary files.
The regression ceilings cover bounded metadata page reads and a 128-MiB runtime
RSS budget. Full source/control/mutant and platform evidence is reported with
the PR; hosted CI is outside this local qualification.
