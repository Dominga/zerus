# Advisory process index

The coordinator approved this first slice after the [design record](../../../docs/superpowers/specs/2026-11-14-process-index-person-authority-design.md).
The second slice publishes ordinary Linux/macOS terminal records and runs bounded
bootstrap in the existing recovery worker. There is no production lookup consumer,
caller-policy replacement, Person UI route or task-control activation. The index
remains advisory and every production coverage flag stays incomplete.

`Store` accepts an explicitly supplied SQLite connection. Its metadata scope and
persistent random epoch prevent accidental cross-store use; they do not qualify
that connection's filesystem/VFS custody. The future owner must retain the native
namespace, verify canonical records and precise process identities, establish every
writer's coverage/floor, and recheck the returned stamp after native observation.
`finish` records owner-declared scan/catch-up completion, not proof that a filesystem
inventory occurred. Neither `Candidate` nor `NoMatch` grants Person authority.

Intents are immutable and retained. Pending intents fence negative coverage;
settlement and projection changes commit together. Scan pages cannot overwrite a
post-scan change or a newer revision. No age expiry, repair, journal pruning or live
claim eviction is implemented. Journal admission is capped at 262,144 retained
intents; a new database has a 512 MiB SQLite page quota. Capacity refuses index admission only. It never refuses a canonical source write,
launch, spawn or lifecycle outcome.

Ancestry input is at most 64 distinct PIDs. Entries are at most 4 KiB, intents
16 KiB, and scan/read pages 128 rows and 64 KiB of row payload. All ancestry and
keyset queries pin their named indexes; missing indexes refuse instead of scanning.

The explicit `process_index_bench` example imports the exact module source. Its
synthetic private database measures only index work at 10k/100k sessions and 64
modeled ancestors, including anchor VM/full-scan steps and metadata reads. It does
not measure OS ancestry, canonical-source I/O or native caller qualification.
The same private-store controls also run under the normal root binary's test module.

Explicit checks and benchmark (use the shared heavy wrapper for managed runs):

```sh
cargo +1.99.1 test --locked -j 4 --example process_index_bench
cargo +1.99.1 test --locked -j 4 --bin hgs state::process_index::tests::
cargo +1.99.1 run --locked -j 4 --example process_index_bench
```

## Terminal publisher coverage

`process_index_writer` wraps the existing `storage::atomic_record` and physical
record removal paths. The supervisor captures precise child/supervisor birth
identities immediately after its original spawn, requiring their parent
correlation. The compatible coarse `process_start` remains unchanged. Hooks,
conversation confirmation, start/clear/resume, rename and supervisor exit publish
through those ordinary paths. Activity-only writes reuse the source revision and
do not append an intent. Confirmed supervisor completion retires its binding;
Interrupted/Paused alone does not prove exit.

Immutable intents include the source locator and precede canonical publication.
The canonical record carries the epoch, intent UUID, sequence and revision. Index
projection and settlement commit together. Index/namespace/capacity/marker errors
never replace the original source result or replay its I/O. A best-effort private
`coverage-lost.json` retains the earliest loss time. Missing epoch/high-water
witnesses and a reverse locator sweep expose gaps even when that marker failed.
The auxiliary locator table is a bounded bootstrap hint, never binding authority.

The cache lives in a private `process-index` child under the selected state root,
scoped by native machine identity, state-directory object, UID, boot and (Linux)
PID/user namespace. A retained directory/file handle and nonblocking writer lock
fence namespace replacement. Old boot databases are not reused. This native cache
scope does not assert Swarm membership or authorize any action.

Each background step admits at most 256 directory entries, 2 MiB of source reads
and a 50 ms deadline. SQLite pages stay within 128 rows / 64 KiB. A held iterator
and at most one pending filename/snapshot avoid foreground inventory and unbounded
RAM. The deadline stops admission of further work; an already admitted filesystem
operation or fsync can exceed it. Initialization performs constant namespace/store
I/O outside these scan-step budgets. The worker releases its index lock between
steps. A new pass can restart after a crash. Only an exact surviving post-file
witness can settle a pending write; bootstrap never publishes/deletes a canonical
file. Pending retirements without a surviving witness remain pending. A scan retains an
existing settled publication, including its timestamp, and reports stale revision
witnesses or changed canonical payloads as gaps; scanning never upgrades them into
a newer binding revision. A completed
pass never calls `Store::finish` and never claims negative coverage.

Explicitly uncovered: older/unpinned writers, guarded recovery/picker keepers,
dormant Fresh-ID/owner-transfer, provider backends, resident DeepSeek/OpenCode
hosts and their hosted sessions, Teams-specific paths, and the Windows session
host. A native Teams member using the shared ordinary storage path may publish,
but this does not establish Teams writer coverage. Picker keepers skip all
companion I/O. Legacy PIDs are never sampled to manufacture original launch pins.

Conflicting PID ownership, changed birth/run/boot, malformed sources and uncertain
removals retain incomplete/pending coverage. This slice does not evict uncertain
claims, compact the retained journal or establish a complete writer floor. Those
conditions need reviewed resolution before any consumer relies on the index.

Native adapter controls run under `state::process_index_writer::tests` against
private stores. The explicitly ignored `record_write_cost_at_10k_and_100k_sessions`
benchmark requires `PROCESS_INDEX_BENCH_ROOT`, creates streamed private canonical
files and measures unchanged/changed binding writes including source fsync at both
sizes. Benchmark setup is outside the samples; source-result preservation remains
asserted even when the advisory index cannot admit work.
