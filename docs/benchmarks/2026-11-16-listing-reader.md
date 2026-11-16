# Bounded ordinary session listing, 2026-11-16

This qualifies the Linux ordinary-`ls` reader in zerus-63r5s.2, explicitly
opted in per computer and default-off. Watch remains zerus-63r5s.3;
Windows/macOS activation remain .7/.8. The shared watch/archive-preview path
and canonical action/caller/ownership/confirmation/custody decisions are not
activated by display metadata. Field parity is audited in
[the field/source/control table](2026-11-15-listing-parity.md).

## Fixture and accounting

The code and benchmark candidate is `922984ad76c17e12470ff1d088219055b9ce7554`,
based on `e3dffb7927d8d46263e5fcfe398485869d5c5730`. The release CLI is
optimized; the owner of the paired canonical writes is an unoptimized Rust
1.99.1 test executable. Real JSON serialization, fsync/rename and SQLite FULL
durability remain enabled. These are single campaigns on shared Arch hardware,
not isolated-machine latency guarantees. Canonical-write percentiles have 200
pairs per condition. The added column sorts each pair’s signed added time,
not the difference between independently sorted percentiles.

`scripts/bench/listing-metadata.py` owns and removes private synthetic state.
It grows 20,000 saved/interrupted Codex records to 50,000 across 100 projects,
with unique session/run/conversation identities, absent native PIDs and 24
bounded jobs (eight unknown with bounded output, sixteen compacted completed).
Each record is 261,797 bytes. There are no owner records or native model runs.
This measures saved-state scale, not 50,000 simultaneously running agents.

At each cardinality the display database is removed and canonical files are
fsynced then individually advised `POSIX_FADV_DONTNEED`. Namespace metadata
and executable pages remain warm; public overlay caches from the earlier
cardinality can remain. No global cache flush is used. After background
bootstrap, database/WAL files are individually evicted before the cold-index
observation. Logical bytes are the foreground main thread’s `/proc` `rchar`;
physical bytes are its `read_bytes`. The C launcher samples its RSS and
reaps it separately from the explicitly owned resident worker. Worker reads
and RSS are reported separately. Thus “index absent”, “file data evicted”,
and “warm” are distinct observations.

The first public projection can initialize non-binding telemetry/cache
namespaces, temporarily invalidating a directory witness. Every such refusal
is retained in `readiness_probes`; a successful bounded **one-row** projection
is required before calling subsequent reads warm. This is not an unbounded
foreground prewarm or an inference of absence.

The earlier [20k scale audit](2026-11-15-session-scale.md) measured the old
complete `ls --json --local --no-archives` at 33.759 seconds, 10.492 GB logical
reads and 5.17 GiB RSS on main `567b34b90f4c2a716bd13f2278cea78dacb8c5e6`.
That historical fixture used approximately 258-KB saved records. It is retained
before-evidence, not a paired cold-cache run against this candidate. The table
below is the direct enabled/disabled comparison on the same new fixture; the
new default-OFF compatibility path intentionally refuses these large inputs.

## End-to-end ordinary ls

Times are seconds; bytes are decimal MB, RSS is MiB. Every refusal exits
nonzero with **zero stdout**. A successful 128-row page has ready coverage and
an explicit continuation cursor, not a claim that those are all sessions.

| Sessions | Mode | Seconds | Outcome | Logical MB | Physical MB | Peak RSS MiB |
| ---: | --- | ---: | --- | ---: | ---: | ---: |
| 20,000 | Off, record data evicted | 0.016 | Refused: preflight cap | 0.019 | 0.000 | 14.3 |
| 20,000 | Off, warm | 0.015 | Refused: preflight cap | 0.019 | 0.000 | 15.0 |
| 20,000 | On, index absent | 0.016 | Refused: building | 0.028 | 0.000 | 16.8 |
| 20,000 | On, index data evicted | 5.972 | Complete 20000 rows | 192.475 | 25.719 | 408.4 |
| 20,000 | On, warm | 5.673 | Complete 20000 rows | 192.496 | 0.000 | 409.2 |
| 20,000 | On, warm 128-row page | 0.064 | 128 rows + cursor | 1.319 | 0.000 | 26.6 |
| 50,000 | Off, record data evicted | 0.016 | Refused: preflight cap | 0.019 | 0.004 | 14.7 |
| 50,000 | Off, warm | 0.016 | Refused: preflight cap | 0.019 | 0.000 | 14.2 |
| 50,000 | On, index absent | 0.032 | Refused: building | 0.028 | 0.000 | 17.8 |
| 50,000 | On, index data evicted | 6.223 | Refused: response cap | 237.659 | 61.891 | 405.9 |
| 50,000 | On, warm | 6.073 | Refused: response cap | 237.659 | 0.000 | 405.1 |
| 50,000 | On, warm 128-row page | 0.264 | 128 rows + cursor | 1.317 | 0.082 | 26.5 |

The 20k complete warm body is 57,784,718 bytes; it takes 5.67 seconds and
about 409 MiB RSS. That is materially more expensive than a page. At 50k
legacy bulk JSON exceeds the explicit 64-MiB response budget and refuses
after about 6.1 seconds at about 405 MiB RSS. This slice intentionally keeps
complete legacy JSON or refusal; it does not disguise a truncated array as
a complete listing. Large consumers must use pages. The existing 10-second
cooperative route clock does not preempt an individual OS operation.

The original campaign on `1dff02ccbedfb7ad11cc4f73f577378d8eb0658f` is retained
as `scale-full-1dff-initial.json` and `.log`. Its 20k “warm” attempts refused
while first-projection namespaces caught up; its 50k full-body attempt refused
at 6.17 seconds and approximately 421 MB RSS, and its page was stale. None is
counted as successful full-list or successful page evidence. The corrected
campaign records the admission probes explicitly and uses the narrow
clear-before-fallible-recovery product correction.

| Sessions | Background to admitted projection s | Logical GB | Physical GB | Worker peak MiB | Admission probes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20,000 | 32.948 | 6.581 | 5.243 | 23.7 | 15 |
| 50,000 | 78.163 | 15.863 | 13.154 | 23.5 | 1 |

The 32.95/78.16-second cold bootstrap is background work, not a successful
foreground list latency. While building, the CLI returns an honest refusal.
Native versions of all canonical files and 16 SHA-256 samples per cardinality
remain unchanged throughout the CLI stages. The later 157/391-page native
metadata walks separately verify all 20k/50k rows and fresh versions.

## Canonical write cost and retained state

| Sessions | Condition | Plain p50/p95 ms | Index enabled p50/p95 ms | Paired added p50/p95 ms |
| ---: | --- | ---: | ---: | ---: |
| 20,000 | Quiet | 5.368 / 6.002 | 7.427 / 8.118 | 2.043 / 2.585 |
| 20,000 | Resident worker | 5.671 / 7.241 | 7.211 / 8.697 | 1.547 / 3.024 |
| 20,000 | Resident + pinned RO reader | 5.542 / 7.740 | 6.576 / 8.504 | 0.878 / 3.025 |
| 50,000 | Quiet | 5.275 / 6.570 | 7.780 / 8.980 | 2.468 / 3.594 |
| 50,000 | Resident worker | 5.631 / 8.102 | 7.723 / 9.767 | 1.971 / 3.833 |
| 50,000 | Resident + pinned RO reader | 5.578 / 8.725 | 6.776 / 10.297 | 1.019 / 5.073 |

Every indexed canonical callback runs exactly once and keeps its original
result/body. Under resident contention some advisory publications are skipped
and **coverage gaps do occur**; the later native census repairs them. The
lower added time under contention is not evidence that every publication
settled. Canonical writes remain independent of SQLite/cache availability.

| Sessions | Retained DB + SHM MiB | Resident peak WAL MiB | Pinned-reader peak WAL MiB | Worker peak MiB |
| ---: | ---: | ---: | ---: | ---: |
| 20,000 | 108.15 | 3.15 | 3.14 | 19.5 |
| 50,000 | 269.73 | 3.47 | 3.56 | 19.2 |

The benchmark owner’s peak RSS is 69.2 MiB (128-MiB regression ceiling).
Retained file sizes are not peak WAL sizes; the stress columns measure the
actual resident process, including a pinned read transaction. Synthetic state
is deleted on exit.

## Admission and recovery bounds

- Default-OFF compatibility preflights the complete small roster before any
  record body: at most 256 namespace entries, 16 MiB and 250 ms cooperative
  observation. Unsupported/large input refuses. Enabled unready/corrupt/stale
  input has no whole-record fallback.
- Legacy bodies have a 64-MiB / 10-second cooperative ceiling. Explicit pages
  have at most 128 indexed seeds and 512 KiB of encoded body. Archive preview
  and first-page untracked terminals remain independent observations within
  that encoded-body limit. Selected canonical overlays share 16 files/16 MiB;
  independently bounded implicit reads and a 512-KiB parent-query budget do
  not admit a whole inventory.
- The independent worker owns one inotify FD and at most 16 fixed namespace
  directory watches, never file watches. Its 64-KiB buffer drains at most 256
  events per step. Census work is at most 256 entries, 128 rows, 16 MiB raw
  input, 512 KiB of seeds and a 50-ms cooperative clock per step.
- Events only identify candidates. Native row versions plus a resumable census
  prove coverage; overflow/ignored/replacement/quota/restart loses coverage and
  requires re-observation. Mtime churn does not restart at the same prefix.
- Only typed writer-gate contention before any event/census consumption keeps
  state as building. All other step failures discard it **before** fallible
  cache recovery, including failures of recovery itself. Post-drain SQL or
  real COMMIT busy is never a generic retry of consumed hints.
- The pinned read-only transaction holds no publisher gate. Immutable snapshot
  native stamps cannot borrow a later mutable source stamp. Current readiness
  is checked separately; collection retires a generation before deleting its
  first row. Epoch/store/root changes refuse.
- New reader/writer WAL admission stops above 64 MiB; only the background owner
  tries a nonblocking checkpoint. A 512-MiB database ceiling and bounded
  generation collection prevent indefinite advisory growth. Pinned readers can
  cause building/unavailable coverage, never failure/replay of a canonical
  callback. No unknown action is retried or consent reconstructed.

## Reproduction

Build the release CLI and the root test executable from the same source with
Rust 1.99.1, `-j 4`, private HOME/state and inherited `HGS_*` cleared. Run each
heavy command with the shared one-step lease. The C foreground launcher is
`scripts/bench/foreground-time.c`. With at least 18 GiB of private scratch:

```sh
python3 scripts/bench/listing-metadata.py \
  --scratch-parent /path/to/private-scratch \
  --test-bin /path/to/same-source-root-test-executable \
  --hgs-bin target/release/hgs --clock /path/to/foreground-time \
  --output artifacts/test-results/listing-reader/scale-full-final.json
```

`--small` checks the driver with 64/128 files. Raw final and initial campaign
JSON/logs, control failures/restorations, mutant diffs, native Mac source/log
provenance and the final source/artifact manifest are retained in
`artifacts/test-results/listing-reader/`. Hosted CI is not a gate and no
workflow was dispatched or polled.

## Compiled counterfactuals

All 22 edited production guards compiled and failed their runtime control;
compiler errors and zero-test runs are not counted. Every original source hash
was checked against the code pin and restored. The raw patch/log/result hashes
are in `mutants/results.json`.

| Mutant | Runtime control |
| --- | --- |
| `U1-current-ready` | `later_pages_do_not_reuse_a_pinned_ready_bit_after_a_publisher` |
| `U2-native-row-version` | `an_in_place_change_cannot_reuse_a_fresh_snapshot` |
| `U3-immutable-snapshot-stamp` | `a_generation_row_never_borrows_a_later_sources_stamp` |
| `U4-wal-quota` | `a_pinned_reader_cannot_make_advisory_wal_growth_unbounded` |
| `U5-retired-gc-cursor` | `generation_collection_retires_a_cursor_before_the_first_partial_delete` |
| `U6-parent-query-quota` | `parent_point_reads_share_one_query_byte_budget` |
| `U7-step-seed-quota` | `large_compact_rows_share_a_bounded_background_write_budget` |
| `U8-admission-census-retention` | `an_early_advisory_writer_collision_retains_the_census_and_native_feed` |
| `U9-generic-eagain-retention` | `a_sqlite_failure_after_native_hints_forces_a_new_census` |
| `U10-downstream-commit-retention` | `a_real_commit_busy_after_native_drain_requires_a_new_census` |
| `U11-shadow-retirement` | `an_incremental_rename_shadow_removes_its_old_snapshot_row` |
| `U12-scope-inventory` | `canonical_observation_bounds_implicit_inventory_reads_bytes_entries_and_time` |
| `U13-scope-bytes` | `canonical_observation_bounds_implicit_inventory_reads_bytes_entries_and_time` |
| `C1-entry-preflight` | `test_large_default_off_preflight_precedes_recovery_body_reads` |
| `C2-byte-preflight` | `test_byte_preflight_and_late_response_limits_never_publish_partial_json` |
| `C3-live-pane` | `test_live_terminal_id_rename_reply_and_attention_keep_all_public_fields` |
| `C4-display-not-authority` | `test_a_poisoned_display_completion_cannot_authorize_canonical_archival` |
| `C5-native-clock` | `test_slow_native_providers_keep_membership_with_one_bounded_observation_clock` |
| `U14-native-loss` | `every_serialized_native_loss_flag_taints_closure` |
| `C6-remote-pages` | `test_remote_pages_preserve_the_wire_body_cursor_and_noninteractive_route` |
| `C7-wrapped-peer-capability` | `test_an_old_peer_never_interprets_listing_controls_as_a_launch` |
| `U15-failed-recovery-discard` | `failed_cache_recovery_also_discards_consumed_native_state` |

## Local qualification

The restored touched root modules pass **742 tests**, with 36 private/opt-in
helpers ignored and two explicitly excluded unchanged main fixtures. The scale
benchmark above executes its ignored helper separately. All **21 CLI controls**
pass, including complete public rows, real private process/terminal Background
and Unknown, fake resident providers, remote capability/wire protection,
coalescing, crash/disable/executable replacement and FD release. Strict Linux
all-target clippy passes. Default Rust 1.99.1/edition-2024 formatting introduces
zero new blocks; unrelated baseline drift remains unchanged.

Private native Darwin arm64 qualification at the code pin passes **48**
metadata/scope tests (three private/benchmark helpers ignored) and strict
all-target clippy. It uses a private clone, private HOME/state and one shared
heavy lease per step. Its 4.3-GiB target is removed immediately afterwards; no
app, owner runtime, live host, native session or installation is touched.

The only later production delta is a two-line `cfg(not(unix))` import of
`std::time::Instant`, needed by the new readonly Windows stub/signature. The
initial local Windows cross-clippy E0425 is retained, not presented as a pass.
This conditional import is excluded from the Linux and Darwin configurations;
it changes neither the measured executable paths nor activation policy.

The unchanged fixture exclusions are
`cli::tests::usage_names_tmux_only_where_it_hosts_the_sessions` (zerus-8jsf3)
and `cli::tests::a_reference_hop_checks_person_only_commands_before_ssh`
(zerus-f897v). The neutral help golden still expects a platform-only Teams
ABI, and the Person test conflicts with a genuine agent ancestry. Their
fixtures and corresponding production guard functions are byte-identical to
the base, recorded in `baseline-fixture-audit.json`. No caller-proof bypass is
added. The earlier reduced test count after a temporary formatter/module
registration fault is excluded; declarations were restored before the full
restored controls and qualification reported here.

The restored local Windows all-target cross-clippy passes with warnings denied.
It checks Rust using the maintained C stand-in; it does not execute Windows
code or activate the Windows reader. The exact conditional-only source delta
is retained in `qualification-delta.json`.
