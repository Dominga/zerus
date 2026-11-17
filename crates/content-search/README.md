# Public-content index component

This crate is the storage/query slice of `zerus-5wmh.66.2.2.1`. It creates no
worker or service. Qt keeps its existing search contract. Next probes for the
additive indexed-query capability and retains the existing bounded scan when
coverage needs it.

The root CLI prepares its private, disposable index after a search request.
One coalesced helper owns the selected state directory, detaches with all three
standard streams closed, and stops after draining the queue or a 120-second
lease. A later search restarts unfinished work. No native agent, provider host,
canonical session record, consent or action is changed by preparation.

`hgs search --index-status` reports coverage without creating an absent index;
`--index-step`, `--index-discover` and `--index-catchup` remain bounded diagnostic
operations. Exact-target steps accept the existing `content-search-index-v1`
request. Public Codex, Claude and Kimi text keeps the existing parser, redaction,
source fingerprint and exact run/conversation/archive checks. DeepSeek and other
unsupported histories retain their bounded native/legacy fallback.

The helper stages at most 256 directory entries / 1 MiB of paths / 25 ms in a
slice. Small search headers are prepared across the catalog before historical
message ingestion, so a long history cannot hide later sessions from fresh
fallback. Each preparation/ingestion slice consumes at most 32 file opens / 128 public records / 4 MiB of
retained metadata and history / 250 ms. Cold metadata preparation has a separate
256 MiB aggregate raw-I/O bound; one record and its transient body remain
capped at 64 MiB, and at most 32 source handles are retained. It validates canonical
JSON and retains only search fields, excluding private job output. Small
projections (at most 1 MiB each) live in a bounded 32 MiB / 50,000-row cache.
Every cache reuse opens the canonical file without following its final link and
checks its native file identity, length and timestamps (including Unix ctime).
Replacement, deletion or in-place edits invalidate the projection. Larger
public snapshots retain their original metadata/history limits.

A census retains its native directory iterators and at most one deferred path
between slices, under a private discovery lease. Interrupted/changing censuses
never prove absence; a crash drops the handles and starts an incomplete pass.
The persisted queue and message cursors survive independently. An older decoder's
refusals are revisited in indexed batches of at most 32. `unknown_metadata` means
unprepared identity, while `invalid_metadata` means a failed metadata or binding
validation; neither is inferred from a missing search match. A constant-size
`worker.json` reports running, complete, pending or unavailable preparation.

Foreground discovery retains its 500-file / 2,000-entry / 4 MiB raw-I/O / 500 ms
bounds. It uses the compact projections rather than repeatedly reading large
private histories. Small computers may wait up to 2 seconds for their first
preparation; a catalog above 500 candidates returns bounded partial results
while the helper continues. No partial inventory establishes absence.

`hgs search --indexed --query TEXT --limit 1..100 [--cursor CURSOR]` keeps the
`content-search-query-v1` response, exact navigation identities, insertion-bound
cursor, 512 posting-candidate limit and 2 MiB indexed reply cap. Every result
still needs fresh binding and transcript/wire fingerprint proof. Live histories
retain a fresh bounded fallback channel, capped at 5 MiB. A complete fresh
fallback can establish complete combined coverage even while the advisory live
index is partial; an incomplete fallback stays visibly limited/unavailable.
Unsupported, missing, malformed and over-limit sources remain explicit, never
empty-match evidence. Fresh fallback duplicates take precedence in Next.

Kimi still admits only exact, unique main-agent public wire text under the
existing bounded profile-inventory and locator checks. No engine is contacted.
Cursor continuations keep their original insertion high-water while background
work proceeds. Forgotten/replaced sources cannot publish cached text, and the
index grants no authority to session actions or cleanup. Copied/restored loose
permissions, links and configuration mismatches still fail back to bounded
search on a first page; a stale cursor requires a new query.

`Index` uses Unicode simple-case-folded two-character postings, followed by the
same literal regex match as current search. Queries visit at most 128 posting
candidates, and return at most 200 hits and 2 MiB of payload. A query-bound
keyset cursor preserves its insertion high-water mark while new records arrive;
even a page containing only nonmatches or retired sources advances the cursor.
Callers must follow the cursor rather than treating an empty partial page as
proof of no matches. Status uses transactionally maintained counters, without
catalog enumeration or full-table counts.

Each ingest call accepts at most 16 messages, 512 KiB of encoded public data
and 32,768 distinct postings. Each message holds at most 128 KiB of public text.
An over-limit message must leave coverage incomplete and retain legacy fallback;
the source adapter may later add overlapping fragments with exact original
message identities. An unconsumed source cursor or skipped text never establishes
complete coverage.
The caller supplies cancellation, monotonic deadline, byte/file/row budgets and
idle CPU/I/O priority. Indexing runs only on demand with bounded catch-up; the
future G1 owner may call the same component without adding another daemon.

The database defaults to a hard 64 MiB page cap. SQLite's DELETE journal is
bounded by one database image plus page headers; status reports that combined
disk ceiling. There is no WAL, disk temp store or automatic full VACUUM.
`evict` deletes at most 32 oldest documents and marks coverage incomplete; freed
pages are reused. A failed batch, including a full database, preserves the
caller's input cursor. Catalog/indexing/unavailable/eviction coverage is explicit.
Cold or incomplete coverage must retain today's bounded search fallback.

`retire` excludes a source atomically before cleanup. `purge` securely deletes at
most 32 documents per pass. The small hashed tombstone remains to reject delayed
ingestion of that exact run/conversation/archive identity. Forget/delete/removal
integration must retire in the operation or the next pass, and revalidate hits
before exposing them. This index is never a source of session/action authority.

Focused tests include a 30,000-message query-plan fixture, Unicode/cursor
contracts, cancellation-independent retirement, bounded purge, secure deletion,
coverage counters and physical file-cap/eviction behavior. Run with:

```sh
RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=8 cargo test --manifest-path crates/content-search/Cargo.toml -- --nocapture
```
