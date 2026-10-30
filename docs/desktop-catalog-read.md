# Private desktop display catalog reads

The selected local CLI advertises the exact trimmed help line
`Desktop catalog ABI: desktop-catalog-v1`. Probe it before using these additive
operations through the existing authenticated `hgs desktop request --json`
lane. A missing service fails without creating state or starting a reader.
The GUI adapter and positive source/writer handoff remain separate.

## Request and response

Start a walk with a canonical request UUID and the configured computer alias
(empty for local):

```json
{"op":"catalog_page","request_id":"00000000-0000-4000-8000-000000000001","computer":"peer","cursor":null}
```

Accepted values contain `abi: "desktop-catalog-v1"`, `request_id`, `epoch`,
`computer`, `accepted: true`, `durable: false`, `display_only: true`,
`authority: false`, `complete`, `items: [{key,row}]` and `next_cursor`.
Unknown private row fields and exact identities are preserved. A null
`next_cursor` ends the walk. An unknown source refuses; a known source with no
published replacement returns an empty incomplete page.

Pass the opaque `next_cursor` in the next request, with the same computer and
a new request UUID. Each continuation is consumed once, including when a later
read/encoding fails. Reusing it refuses rather than skipping a page. A lost
response or expired continuation requires a fresh display walk; request UUIDs
are correlation identities, not durable read receipts or replay permissions.
No row pages are cached for replay.

A cursor binds the computer, visible SQLite generation and original insertion
watermark. New inserts cannot extend that walk. Updates and removals may change
rows during traversal; this is bounded keyset paging, not a frozen transactional
snapshot. A replacement invalidates the old continuation. Service restart
discards every continuation. Numeric internal cursors never cross IPC.

To abandon a retained continuation explicitly:

```json
{"op":"catalog_close","request_id":"00000000-0000-4000-8000-000000000002","computer":"peer","cursor":"00000000-0000-4000-8000-000000000003"}
```

An accepted close returns `closed: true`. Unknown, consumed, mismatched or
expired cursors refuse. Unknown fields and invalid aliases/UUIDs also refuse.

## Bounds and failure

The actor keeps at most 64 small continuation records for the entire
configuration. They expire after 120 seconds idle or ten minutes from the
original walk, whichever is earlier; continuing refreshes only the idle bound.
A continuation rotates in its existing capacity slot. New walks needing
continuation refuse when capacity is full. A single-page read needs no slot.
Expired records are pruned on the next read/close; retained metadata never
exceeds the fixed cap. Status exposes these bounds and `retained_walks`.

Every query uses the catalog's existing indexed read: at most 128 rows and
1 MiB of charged row data, with sizes checked before allocating row strings.
The response is additionally encoded against the exact desktop-v1 envelope
limit (1 MiB + 4096 bytes, including LF), using the maximum sequence width.
This catches extra JSON/key escaping and corrupt stored sizes. Encoding,
parsing and storage failures return a static refusal local to that read;
they cannot terminate the actor or return a private row as a diagnostic.
Other walks and the existing source remain available. No database write,
filesystem scan, per-session map or additional upstream reader is introduced.

`complete` is display coverage, additionally gated by the current passive
producer's atomic open flag. Disconnect makes it false before retirement
finishes, while last-known rows remain readable. Arrival time, a complete page,
a cursor or IPC connectivity never grants peer freshness, native effect
authority or a writer permit. Native operations remain fenced. Consumers
combine the existing bounded `catalog_changed` fanout with a new paged walk
when they need to resnapshot; slow consumers do not grow a service row cache.

Focused tests cover 10,000 rows over 79 pages with one retained continuation,
scope/replay/capacity/idle/absolute bounds, replacement and service restart,
append watermarks, unknown raw fields, valid near-frame singletons, corrupt
encoded keys, storage repair and immediate incomplete coverage after EOF.
