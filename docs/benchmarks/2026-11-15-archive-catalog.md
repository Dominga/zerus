# Bounded archive catalog qualification

This opt-in Arch qualification uses only a managed synthetic home, state,
configuration and tmux directory. There are no native agents. The fixture has
20,000 saved/interrupted bindings and 50,000 archives across 100 project labels.
Each record retains eight unknown jobs with 28 KiB outputs and sixteen compacted
completed jobs: 258,085–258,181 bytes per record, 18,070,750,000 bytes altogether.
The shared transcript is synthetic. Provider, SSH and service executables in the
private home refuse execution.

The generator and measurement method are the original retained session-scale
harness described in [the scale audit](2026-11-15-session-scale.md). Its helpers
were copied before use; the original agent's fixture and binaries were not used.
Only archive queries and one atomic archive replacement were measured. The
unrelated Teams, task and two-million-event stages were not run.

The unchanged baseline is main `ffb70cada91801bf0c329c0497c5d387951923ec`.
Both CLIs use Rust 1.99.1 and the same optimized release profile. The baseline
image SHA256 is
`5b16dd2869dbcc34df03e539f43e98acb6fa7a4c274c1377974e25b5417047a0`.
Builds and measurements enter the shared heavy gate, one step at a time;
compilation uses four jobs. Command times exclude queue time. No global cache
was dropped: “cold” means an unbuilt derived catalog, not cold disks. Other
local work and scheduling affect elapsed time.

Each owned child is sampled through its final `/proc/PID/io` before `wait4`
reaps it; the report also retains peak RSS and physical input bytes. Logical
reads include executable, SQLite and other input, not just archive JSON. The
full fixture is removed after qualification; small logs and helper copies are
retained under `artifacts/test-results/archive-catalog-bounded`.

The archive command is `hgs archives --json --bounded`, with `{}` on stdin.
The changed archive is UUID integer 269999: its title becomes
`Incremental synthetic archive update`, written to a sibling and atomically
replaced. Original bytes are restored in `finally`.

Worker accounting is separate from request accounting. The benchmark holds its
own private worker lock during request measurements, so an unmeasured automatic
child cannot hide refresh cost. It then measures the identical
`hgs __state archive-catalog-worker` entry as an owned foreground child, counting
every bounded resumption until complete. The independent native CLI control in
`tests/test_archive_pages.py` instead releases the launch exclusion and verifies
actual detached startup, explicit incomplete coverage, eventual complete pages
and unchanged canonical files.

## Controls

The unchanged baseline failed structural read-count controls: an eight-file
cold request read 24 archive bodies, and one replacement read its body three
times. The new worker leaves cold body reads out of requests and shares one
captured body among search, native identity and folder-reference projections.

Private Rust controls cover slice count/byte limits, committed restart progress,
arrival/removal freshness, unchanged-catalog writes, inode/ctime replacement,
corrupt search and native identity rows, independent search/reference omission
limits, late workers after root removal, and a worker commit between one page's
count and row reads. The last control first returned two rows with total eight;
both exact and bounded pages now use one pinned SQLite read view, released
before journal summaries.

A worker validates compact rows in ID order through individually bounded index
queries. A compound key query in an intermediate implementation exhausted its
60-second budget while loading cache progress at 50,000 archives; this was
caught by the full fixture rather than accepted as successful resumption.

Complete identity membership is independent of display omissions. Selected
positives still verify the exact native body; an indexed miss requires a fresh
complete native filename/version census. Cleanup retains its existing separate
canonical reference proof, 5,000-file/32 MiB/deadline bounds, and no-sidecar native
reader. A cold, stale, damaged or over-budget catalog never establishes absence.
The worker's explicit coverage limits and retry behavior are documented in
[the archive contract](../reference.md).

## Results

The candidate image SHA256 is
`b03809ce7d1226f105220fcbab796d733d2ee192dd6c11a099a74b025d06d45b`.
Both first builds start with no derived SQLite catalog.

| Step | Seconds | Logical read bytes | Peak RSS (KiB) |
| --- | ---: | ---: | ---: |
| Baseline cold complete page | 115.119663 | 40,140,923,086 | 29,812 |
| Candidate cold request (incomplete) | 0.012990 | 8,148 | 20,416 |
| Candidate full background build, two helpers | 112.584715 | 15,371,797,936 | 30,952 |
| Candidate first complete page | 0.082453 | 14,210,352 | 32,248 |
| Baseline warm complete page | 0.040945 | 14,205,608 | 31,404 |
| Candidate warm complete page | 0.041802 | 14,205,420 | 34,428 |
| Baseline one-change complete page | 1.716465 | 1,161,033,007 | 34,100 |
| Candidate change request (incomplete) | 0.010785 | 20,568 | 21,780 |
| Candidate one-change background worker | 1.953759 | 825,242,006 | 36,628 |
| Candidate changed complete page | 0.042035 | 14,205,467 | 34,388 |

The complete cold path, including its request, every worker resumption and the
first ready page, took 112.680 seconds and read 15,386,016,436 logical bytes.
Physical input for the two helpers was 13,145,153,536 bytes, close to the
baseline's 13,144,977,408 bytes: this run remained limited by native input and
local disk/scheduling conditions. The foreground cold read returns explicit
incomplete coverage in 13 ms instead of waiting for that work.

For one replacement, request plus worker plus ready page took 2.007 seconds and
839,468,041 logical bytes, versus the baseline's 1.716 seconds and 1,161,033,007
bytes. The worker still validates the compact metadata roster and witnesses the
filename/version census; this work runs in bounded background slices. It does
not reread unchanged native archive bodies. The structural controls require
exactly one changed-body read and zero unrelated body reads, independently of
wall time and page-cache state.

The complete 50-row incremental page matches the baseline, including counts,
flags, row fields and decoded cursor positions. Only the opaque catalog nonce
differs, as required for a genuinely fresh index. Cross-catalog/rebuilt cursors
remain refused by the native CLI tests; the benchmark never substitutes a nonce
or reuses a cursor across those catalogs.

Evidence includes `scale/final-measurements.json`, final child stdout/stderr,
`scale/page-equivalence.json`, clean binary/source seals, structural RED logs,
seven compiled runtime mutant failures, and local qualification logs. The source
mutants were restored byte-for-byte after compilation.

Local validation: 38 Rust archive/catalog/reference controls and 54 private CLI
archive/lifecycle tests passed. The CLI controls include a 20,000-entry preview
fixture, actual detached startup, stable pagination across inserts, exact lookup
without a catalog, archive-source replacement fences, failed writes, name reuse,
Pause, explicit exits and every supported fake-agent restore path. Seven compiled
mutants were killed by their intended runtime assertions: inline rebuilding,
duplicate capture, dropped native-version comparison, unchecked absence,
unchecked positive identity, stale namespace proof and unpinned page reads.

After measurement, strict Clippy cleanup removed three redundant borrows in the
shared validator and a blank line after a doc comment. Validation and worker
logic are unchanged; the image hash above identifies the measured build.

Strict root `cargo +1.99.1 clippy --locked --all-targets -j 4 -- -D warnings`
and local `scripts/ci/check-windows-target.sh clippy -j 4` both passed. Source
syntax/privacy checks passed. No macOS-specific paths changed.
