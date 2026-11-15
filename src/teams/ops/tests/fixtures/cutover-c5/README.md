# Pre-fix cutting-over stores

These SQL dumps were produced on main
`c5a34bbb675c52b764385ac1fc46502cc8e81504`, before changing any production
code for zerus-63rru.166. `producer.rs` is the temporary test appended to
`src/teams/ops/tests/cutover_delivery.rs` (a new registered test module on the unchanged base production code) for that one run; it is retained here as provenance,
not compiled into the suite. All rows come from `Harness` operations, the
literal product writers at that commit. No database rows were written by hand.

Run the producer with `WP166_LEGACY_OUT` pointing to private scratch and select
`wp166_generate_old_stores`. After it exits and closes the stores, use Python's
`sqlite3.Connection.iterdump()` on each output database and append
`PRAGMA user_version=2;`. The maintained regression restores these dumps into
its private store, checks foreign keys, then opens the normal current writer.
The old store's unique indexes, request receipts and outcome timestamps remain
unaltered.

- `open.sql`: a cutting-over hand-out and another node's genuine unknown.
- `recovered.sql`: the person's Reassign, refused reopen and Release have
  already superseded the old effect; a new hand-out is submitted.
- `generation.sql`: an earlier genuine unknown and a later cutting-over
  refusal for the same node have identical reason and timestamp. Only the
  latter's exact outcome receipt identifies its false uncertainty item.

The generation log is retained as
`artifacts/test-results/wp166/legacy-producer.log` (one test passed).
