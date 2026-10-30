# Public-content index component

This crate is the storage/query slice of `zerus-5wmh.66.2.2.1`. It creates no
worker or service. Qt keeps its existing search contract. Next probes for the
additive indexed-query capability and retains the existing bounded scan when
coverage needs it.

The root CLI now provides explicit on-demand ingestion diagnostics:
`hgs search --index-status` reports coverage without creating an absent index;
`hgs search --index-step` accepts at most 16 exact targets on JSON stdin:

```json
{"abi":"content-search-index-v1","targets":[{"name":"codex/example/search","run_id":"run-one","conversation_id":"conversation-one","archive_id":""}]}
```

The caller verifies each authoritative binding, reads public Codex/Claude lines
and exact journal pages with persistent cursors, and retains an incomplete
coverage state for unsupported native histories or over-limit text. A step has
32 source-file opens, 128 records, 4 MiB and a 250 ms monotonic deadline; it
starts no continuation after return. The CLI lowers only its own CPU/I/O
priority. The exported step accepts cancellation and explicit bounded budgets
for a later G1 owner.

`hgs search --index-discover` stages candidate filenames and file stamps in the
same capped database, without reading binding bodies or transcripts. The
directory iterators live only for this short call. Discovery permits at most
65,536 directory entries and 8 MiB of paths, with 256-row transactions and a
500 ms monotonic deadline. A cancelled, interrupted or changing-directory pass
never establishes complete discovery. Repeated passes reuse staged paths;
an unchanged directory stamp avoids enumeration. New or changed metadata
returns that path to pending. Status reads constant-size path/pending counters.
Complete discovery still leaves history coverage cold or incomplete until
queued bindings and their histories have actually been consumed.

`hgs search --index-catchup` combines that bounded discovery pass with one
250 ms ingestion pass. The persisted queue uses indexed 16-path keyset pages;
public history cursors resume on the next explicit call. Metadata reads count
against the source-file/byte budget. A completed catalog pass also prunes at
most 16 vanished paths; retirement excludes their text before bounded secure
cleanup. Constant counters distinguish pending paths from sources that still
need the legacy search (live, unsupported, invalid, evicted or incomplete).
Live histories retain the fresh legacy read even after cache ingestion.

`hgs search --indexed --query TEXT --limit 1..100 [--cursor CURSOR]` advertises
`content-search-query-v1` in help. It publishes at most 100 freshly verified
archive hits and 2 MiB per page, with a separate `next_cursor`, `index_coverage`
and optional `fallback` legacy reply. Each first page performs at most one
bounded discovery/catch-up call; continuations schedule no indexing. Publication
visits at most 512 posting candidates with a 500 ms deadline, 128 source opens
and 8 MiB of metadata/fingerprints. Every distinct returned target needs a fresh
exact binding and transcript-identity/fingerprint proof. Forgotten or changed
sources retire before any hit is exposed. If proof or output budgets run out,
the cursor stays before the unconsumed hit.

Cold, incomplete, live or unsupported coverage retains the existing bounded
scan in a separate fallback channel. Fallback payloads are capped at 5 MiB
with explicit truncation, so the whole response stays below Next's 8 MiB child
output limit. The cursor advances only through index hits, never fallback hits.
Next validates both channels separately, deduplicates exact run/conversation/
archive/message identities with the fresh fallback copy preferred, and retains
its existing 100-hit host bound. The UI keeps one stable row per session and
names unavailable computers inline, including cold index coverage.
An unsafe or unavailable cache fails back to the existing search on the first
page. A stale/mismatched cursor requires a new query. No new UI is introduced.
Live/native queries still use legacy history reads; extending fresh indexed
coverage to those sources remains a separate adapter/invalidation step.

The adapter refuses copied/restored loose directory, database or auxiliary-file
permissions and links. New index directories are 0700, database and rollback
journal files 0600. Configuration identity prevents reusing a copied index
under another state root. A bounded source sweep retires forgotten targets
without needing them to appear in the next request, and a persistent purge queue
drains secure payload cleanup. Indexed publication revalidates the exact target immediately before returning
a hit; the index never authorizes a session action.

The caller owns a private, disposable database in the selected configuration's
state directory, with directory mode 0700 and file mode 0600 (owner ACL on
Windows). It rejects links, creates the private file before opening SQLite and
disposes/rebuilds a damaged index. Only normalized public messages enter the
typed API; raw provider JSON, reasoning, attachments and credentials do not.
The adapter must reuse the current search's parsing/redaction rules. It must
verify the authoritative exact source both at ingestion and before returning
a hit, so a forgotten binding cannot remain visible between index passes.

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
