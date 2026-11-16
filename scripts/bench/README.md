# Private session search scale benchmark

Build an optimized CLI, then run the opt-in Linux regression benchmark:

```sh
cargo +1.99.1 build --release --locked --bin hgs -j 4
python3 scripts/bench/search-scale.py --check
```

The default fixture has 2,000 saved/interrupted session bindings and 5,000
archives. `--full --scratch-parent DIRECTORY` uses 20,000 bindings and 50,000
archives, about 18 GB, under an explicit disk-backed scratch parent. Each record
has eight retained unknown jobs with bounded output and sixteen compacted
completed jobs, roughly 250 KiB per record. No agent, tmux server, SSH peer,
installed service or user state is used. The temporary fixture is removed on
success and failure. In an agent workspace, use its registered scratch runner
and shared heavy-job lock as usual.

Both ordinary search and indexed search's incomplete-coverage fallback run once.
The report includes elapsed time, final child `/proc/PID/io` logical and physical
read bytes before reaping, `wait4` peak RSS, result count and discovery coverage.
No global page-cache eviction is performed; fixture generation warms file data.
An initial archive catalog means a cold *derived catalog*, not cold disks.
The timeout and RSS guards kill only the benchmark's own process group and
record an incomplete measurement. `--check` rejects regressions above 16 MiB
logical reads or 96 MiB RSS for these search fixtures.

`--existing-fixture DIRECTORY` reuses an explicitly marked synthetic audit
fixture (`.zerus-scale-fixture` and `fixture.json`); ordinary state directories
are refused. This lets before/after runs share the exact 18 GB input. Derived
search caches may be created in that private fixture. The smaller runtime
controls in `tests/test_search_scale.py` additionally enforce a 12 MiB ceiling
and check that an incomplete scan with no matches still reports `truncated`.

`examples/session_scale.rs` supplies two optional helpers for the broader audit:
`tasks PRIVATE_REPLICA_ROOT` requires the private fixture marker and an absent
replica destination, and writes 100 tasks in each of 100 projects through the
replica API; `pages` reads real desktop catalog page receipts from stdin,
validates them with `client/core` and projects rows through its normal model and
coalescer. The first line may carry the original watch Hello metadata. Pages
are consumed and released individually. Its default stdin mode decodes watch
frames through `CatalogState`. These headless measurements exclude renderer
layout, native agents and platform installation.

The ignored `teams::cli::tests::harness::export_scale_teams` test exports 100
operation-written Teams into `ZERUS_SCALE_ROOT/state/teams` only when the private
fixture marker exists and that destination is absent. It never claims Person
authority for an agent benchmark process and never replaces an existing store.

## Opt-in ordinary session listing

`listing-metadata.py` owns a real-file 20k/50k saved-session fixture (roughly
262 KB per canonical record, 24 bounded shell jobs and 100 projects). It records
paired durable canonical writes with and without advisory publication, bounded
background migration, immutable metadata pages and index size. It requires an
explicit scratch parent with 18 GiB free and removes only its own fixture.
`--small` uses 64/128 rows for harness validation.

Pass `--hgs-bin` and `--clock` to include ordinary `ls --json --local` with the
index disabled/enabled, index-cold and warm. Compile `foreground-time.c` locally;
it captures only the foreground CLI's main-thread logical/physical I/O before
reaping and samples its RSS. It excludes detached worker I/O; worker readiness
is timed separately. Canonical data files are individually evicted with `posix_fadvise(DONTNEED)`
after durable generation; namespace metadata stays warm. Derived-index files
also have a separate eviction run. No global cache eviction is performed. The stage checks every canonical file's native
version plus 16 body hashes before/after and requires complete legacy rows or
nonzero/empty-stdout refusal. `--paged` reports one bounded ready page.

An already compiled same-source unit-test executable may be supplied through
`--test-bin`; otherwise the driver compiles the ignored benchmark with four
Cargo jobs. Agents must use their normal shared heavy lease for each step and
retain source/binary seals alongside the output. No native agent, user tmux
server, deployment, account or installed host is used.

Final enabled/disabled cold-file, warm, publication and pinned-reader results:
[ordinary listing qualification](../../docs/benchmarks/2026-11-16-listing-reader.md).
