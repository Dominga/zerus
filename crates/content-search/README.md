# Public-content index component

This crate is the storage/query slice of `zerus-5wmh.66.2.2.1`. It creates no
worker or service. Qt and Next continue to use the existing bounded search until
catalog discovery and indexed result publication are delivered.

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
for a later G1 owner. There is no advertised indexed-query capability yet.

The adapter refuses copied/restored loose directory, database or auxiliary-file
permissions and links. New index directories are 0700, database and rollback
journal files 0600. Configuration identity prevents reusing a copied index
under another state root. A bounded source sweep retires forgotten targets
without needing them to appear in the next request, and a persistent purge queue
drains secure payload cleanup. Indexed UI activation must still revalidate the
target immediately before publication; that integration follows separately.

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
