# Local Teams base compatibility checks

The maintained local entry point is
`scripts/ci/check-teams-base-compatibility.py`. It builds two **source-built test
images**, not release binaries: admitted native4 source
`5216b5b290a67e59b65665fa19d545bb3eb2ce5d` and pinned native4 source
`64eeb5a316f1bca1601b5c732c1d9a2a98d04a52`. Both actually register the base-pin
capability. Inspect those source facts without building anything:

```sh
python3 scripts/ci/check-teams-base-compatibility.py --inspect
```

With Python 3.12+, Rust 1.99.1 and the two Git commits available locally, run:

```sh
# Set ZERUS_HEAVY_WRAPPER to this computer's configured shared heavy wrapper.
python3 scripts/ci/check-teams-base-compatibility.py \
  --evidence artifacts/test-results/teams-base-compatibility \
  --heavy-wrapper "${ZERUS_HEAVY_WRAPPER:?set the configured shared wrapper}"
```

The driver acquires the wrapper separately for each image build and once for
the bounded interoperability scenario; it holds no lease between them. Builds
use four jobs and the scenario uses one test thread per image. Private HOME,
state, TMPDIR and an existing private TMUX_TMPDIR are created below this
worktree's target; inherited HGS/tmux context is cleared for Cargo and image
processes. Existing RUSTUP_HOME/CARGO_HOME are retained, never copied. Debug
information and incremental compilation are disabled by default to bound disk
usage. Source archives and separate Cargo targets stay below this checkout's
CARGO_TARGET_DIR (default `target/`).

The same sealed `tests/fixtures/teams/base_engine_native4.rs` is injected only
as a test module into each exact Git archive. Production source, version marks,
authority and policy are unchanged. Archive, tree, fixture, injected-module and
image hashes are retained with raw build/run logs. The expected executions are
one predecessor control and two current-image controls, all passing exactly
once. Use `--step build-previous`, `--step build-current` and `--step run` to
repeat individual bounded steps under the same wrapper. `--step lint-current`
runs strict all-target Clippy on the current source with the injected fixture.
A run requires both
sealed images and refuses stale fixture/image hashes; it never builds a
fallback source. Keep the evidence directory before removing an unused target.

This is **real-image interoperability**: the predecessor keeps a Store open
across the current image's advance, successfully writes with that cached
handle, then closes/reopens and successfully writes again. Existing assignments
retain their original base; later assignments use the advanced default. Both
images can read the results, the integration-creation origin is retained,
unrelated team rows are unchanged, and fresh request/effect receipts occur once.
Rows come from the product's operation writers in private fixtures. No live
team or person-only CLI operation is invoked. A legitimate native refusal is a
failed scenario to report with its source and raw outcome, never a reason to
change a production floor, capability, assertion or caller proof.

This does **not** prove a native4 writer lacking the base-pin function is
refused. No native4 pre-base release is invented. The historical CORE2 script
`check-teams-base-engines.py` and its original refusal fixture remain unchanged;
they are not a native4 or legacy3 substitute for this command.

The SQL refusal guards have separate, explicit in-process fault-seam controls:

```sh
cargo +1.99.1 test --locked -j 4 --bin hgs -- \
  advance_base_cached_writer_cannot_settle_a_pre_advance_handout \
  advance_base_cached_writer_cannot_insert_a_report_before_any_later_guard \
  --test-threads=4
```

Those tests use product-written rows, then remove only the connection function.
The report-insert control observes attempted INSERTs even when a later guard
rolls back the transaction. Dropping the reports INSERT trigger (Z8) and effects
UPDATE trigger (Z9), one at a time, must produce actual runtime assertion
failures; restore each production file exactly afterwards. This fault seam is
not a real old image. The registry and `advance_base_private_fence` tests
separately cover bit 31 and a simulated next timer-layout step, without enabling
a new core allocation or migration.

When qualifying a newer pair, update the pinned actual sources, fixture and
contract together. An ordinary newer-core refusal is not a missing-function SQL
refusal. This command is local evidence, with no hosted workflow or release gate.

The entry-point unit controls skip explicitly when their pinned Git objects are
unavailable in a shallow/source-archive checkout. A skip is no real-image
evidence; the actual command refuses missing sources without a fallback.
