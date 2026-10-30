# Indexed desktop catalog

`zerus-desktop-catalog` is the private SQLite projection of raw watch-v1 catalog
rows. Root desktop schema 5 creates it and resets coverage at every service
start. This passive slice starts no source or effect worker. Client delegation,
legacy adaptation, runtime freshness and native delivery remain separate work.

The actor owns one connection on its background thread. It retains each source
ingress item until its transaction finishes. `connect` pins the selected alias,
service epoch, source incarnation/generation and hello stream identity. Before
calling it the actor must verify the hello's machine identity against its
configured computer; a reported host name cannot select an alias. Replaced
source callbacks and previous service epochs cannot change current coverage.

Every decoded frame must pass through `apply` in receive order, including other
topics. Sequence holes discard staging and invalidate coverage. A catalog gap
keeps last-known visible rows while a new snapshot starts. Catalog source errors
(including omitted oversized rows), fatal errors, malformed pages and reordered
frames retire that connection; reconnect with a new generation. A complete new
snapshot is required before deltas are accepted. The watch server sends catalog
snapshot pages contiguously; interleaved catalog deltas refuse conservatively.

Snapshot pages insert rows directly into a staging generation. Pages must start
at zero, remain contiguous, use one revision and contain unique canonical keys.
The final page swaps the visible pointer in the same transaction. A crash or
restart keeps last-known visible rows but never promotes partial staging. Raw
JSON, unknown fields and background-work counts remain unchanged. Projection
and attention/read policies reuse their existing shared decoders later.

`complete` describes catalog coverage, never source freshness or effect authority.
The actor must separately retain monotonic receive timestamps, current ownership,
delegation and writer fences. Processing an old queued heartbeat must not renew
freshness. **Any storage error pauses that component**, even if recording its
incomplete state also fails. This crate supplies no effect admissions.

`page` returns at most 128 rows and 1 MiB of charged row bytes (JSON, key and
64 bytes of envelope allowance). It reads sizes before allocating row strings.
Its internal cursor binds alias, visible generation and an insertion watermark;
new inserts cannot extend that walk, updates/removals may change it, and a
replacement invalidates the cursor. It is an actor-owned cursor, not an
untrusted IPC token. No full-catalog materialization or per-session map exists.

Maintained counters avoid scans. Admission caps 64 retained source aliases,
256 generations, one million stored rows and 128 MiB of charged stored rows,
including staging and obsolete generations. Source metadata is bounded by frame
limits (hello 16 KiB). The root database additionally retains its existing hard
page cap; SQLite overhead and WAL are outside the charged row budget. Alias
retirement and whole-service disk/RSS qualification remain integration work.

`collect` removes at most 256 rows and 4 MiB of charged rows from one indexed
obsolete generation per transaction, then removes its empty metadata. `Some(0)` means an empty generation
was collected; `None` means no obsolete generation remains. The actor yields
between calls and runs collection outside input transactions. Capacity pressure
refuses coverage rather than evicting visible rows or unrelated durable intent.

The service actor now runs one such batch on a maintenance turn, waiting at
least 250 ms after progress and 10 seconds after idle/failure. It checks control
requests between turns, including after a successful watch attachment. Startup
marks interrupted staging obsolete, so the same bounded path reclaims it.
`catalog_projection` status reports retained source metadata, maintained row/byte
totals, obsolete generation count and collection failure. These are storage
diagnostics, not live upstream counts or freshness. Collection errors pause
that component and retry at the idle interval; other durable journals remain
available. The service still starts no fleet or effect workers.

Focused tests walk 100,000 synthetic rows, check query plans without scans or
temporary sorting, replace snapshots, collect obsolete rows in bounded steps,
and cover callback fences, restart, malformed input, exact identities, byte and
metadata admission limits, stable cursors and aggregate-counter updates.
