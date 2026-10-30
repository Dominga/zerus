# Passive desktop source feed

The selected local `hgs desktop feed --json` forwards data from an existing
owner to the background desktop catalog. It launches no upstream watch,
listing, inspect, agent or service. Without a producer the service stays idle.
The existing GUI keeps its source and writers; this interface grants display
coverage only, including on Arch before explicit handoff.

Before invoking a selected executable, probe its bounded `--help` output for the
exact trimmed line `Desktop source feed ABI: desktop-source-feed-v1`. An older
CLI must never receive an unknown command that it might treat as a session name.
The proxy connects only to an already-running service for its selected canonical
configuration. A missing service fails without creating desktop state.

## Admission and receipts

The private desktop-v1 endpoint authenticates the same OS user as other desktop
connections. Its bounded hello uses `op: "feed"`, the existing configuration
fingerprint and this request:

```json
{"abi":"desktop-source-feed-v1","producer":"00000000-0000-4000-8000-000000000001","mode":"display_only"}
```

Unknown fields, incompatible ABI/mode and noncanonical UUIDs refuse. One
producer may own a configuration at a time. A rogue process running as that
same user can hold this slot; the OS user is the trust boundary. The service
assigns a monotonically
increasing incarnation within its own epoch; the caller's UUID grants no
identity or authority over a computer. Every source is bound to the service
epoch, assigned incarnation, caller-selected computer alias and generation.
The empty computer alias denotes local, as in the shared source API.

The proxy first prints the desktop-v1 `t: "feed"` receipt. Each accepted input
line then gets one `t: "feed_receipt"` receipt before another input is read.
Receipts have `value.feed_abi`, `accepted`, `durable: false`, `display_only: true`,
`incarnation` and a static `code`. Their epoch/incarnation must stay unchanged.
An observed receipt means the catalog transaction returned successfully; it is
not a durable command, delivery reservation, peer identity check or writer
permit. False acceptance ends the producer. No automatic retry occurs.

The parent owns this child, closes stdin to detach, and bounds its lifetime.
While waiting on parent stdin, the proxy does not monitor service EOF; the
parent must close or stop its own child when cancelling. No other processes are
stopped. The service independently bounds socket reads and retires dead feeds.

## Input

One UTF-8 JSON object per LF-terminated line, including LF at most 1 MiB +
2048 bytes. Send only one item at a time and wait for its receipt. A quiet
producer sends `{"t":"keepalive"}` within 30 seconds. This keeps the connection
open but never renews upstream freshness. Each partial frame also has a
30-second read deadline. The service polls cancellation at most every 200 ms
while waiting on the socket; receipt waits are bounded at ten seconds, socket
writes at the existing two-second timeout. The proxy allows twelve seconds
for a receipt. Slow or malformed producers close without logging their body.

All envelopes are flat, with unknown fields refused:

```json
{"t":"watch","computer":"peer","generation":1,"frame":{"v":1,"t":"heartbeat","seq":3,"at":1.0}}
{"t":"legacy_begin","computer":"peer","generation":2,"metadata":{"machine_id":"example-machine"},"rows":0,"pages":1}
{"t":"legacy_page","computer":"peer","generation":2,"index":0,"items":[]}
{"t":"legacy_complete","computer":"peer","generation":2}
{"t":"offline","computer":"peer","generation":2}
{"t":"keepalive"}
```

A generation begins with a real watch hello or typed legacy begin. Within one
producer, every replacement starts a strictly greater positive generation;
a callback for an older generation refuses. A watch generation forwards every
frame in order, including other topics, so sequence holes remain visible. The
shared catalog checks frame bounds, sequence and snapshot integrity. Other
topics advance sequence but this slice exports no projection for them.

Typed legacy pages hold at most 128 catalog items. Metadata, declaration, row
identity, page counts and storage bounds use the existing
[indexed catalog rules](desktop-catalog.md). Only explicit legacy complete
publishes the replacement. The producer sends it after its owned child exits
successfully and its bounded spool replay verifies EOF, counts and digests.
Neither this message nor a watch's final page establishes freshness.

## Bounds and retirement

One shared ingress budget charges encoded line bytes plus 1024 bytes per item,
up to 16 MiB and 1024 items, including queued and actor-owned items until apply
or discard. Admission waits are cancellation-aware. Serial receipts currently
limit this producer to one outstanding item. Its parser additionally retains
one bounded line and decoded item. There are thirteen bounded connection
parsers total, including the one producer slot, sixteen queued actor events,
and at most 64 small source scopes. No per-session row map exists.

The actor writes pages directly into the existing indexed SQLite catalog.
Storage limits and background collection stay unchanged. It publishes small
`catalog_changed` invalidations through the existing single fanout: eight
subscribers, each capped at 4 MiB / 1024 frames. Feed status exposes bounded
source scopes, incomplete/complete display coverage, ingress counters and
retirement errors. [Indexed page delivery](desktop-catalog-read.md) uses bounded
read requests. GUI adaptation follows separately; this slice does not change
GUI behavior.

EOF, malformed data, a refused item or cancellation closes an atomic producer
fence. Already-queued callbacks then refuse. Before a replacement may enter,
one transaction retires at most 64 exact source tuples, abandons staging and
keeps last-known visible rows and published metadata. A stale tuple cannot
retire a replacement. If storage prevents retirement, admission stays paused
and retries after ten seconds. Shutdown closes the feed and retires it before
the checked database checkpoint.

`complete` in feed status describes display coverage only. Top-level ownership
remains passive, writers remain disabled, upstreams remain empty and effect
requests remain refused. A reported host/machine ID is untrusted display input
here. Positive takeover must separately verify configured machine identity,
fresh delegation, actual worker drain, eligible migration evidence and writer
fences. It must drain/disable this feed before admitting the service-owned
shared supervisor; feed arrival or an old queued heartbeat never grants native
attention, presence, read, badge or approval authority.
