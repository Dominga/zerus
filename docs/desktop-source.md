# Shared raw watch source

`client/core::source` is the source-only output mode of the existing watch
supervisor. It shares capability probes, child lifecycle, reconnect timing,
silence detection and manual reconnect admission with the client stream. It
forwards decoded frames in source order without applying catalog, Activity,
Processes, Tasks or organization data to the client projections. Its empty
`CatalogState` supplies only the existing hello/failure lifecycle policy.

The service does not start this mode yet. Registration, delegation, old-writer
drain, the indexed service projection and native effects remain separate work.
Existing client streams and legacy polling retain their current output mode.

## Ownership and API

The future actor creates exactly one `source::channel` for its already-owned
configuration. Raw admission requires the exact whole help marker line before
any watch invocation. It passes the host's guarded spawn function: hgs on Unix uses
`platform::spawn_in_turn`, so probes and watch children share its macOS pipe
inheritance guard with every other threaded child. The hook changes neither
arguments nor environment. Existing watch arguments and SSH aliases stay intact.

`Sender::spawn` owns one computer alias on that channel. A duplicate, invalid
alias, closed channel or exhausted source capacity refuses before starting a
thread or child. A successful handle has an incarnation that is never reused
on that channel. Each watch restart also increments its stream generation.
Every frame/status carries both, so a stopped alias's queued output cannot be
mistaken for a replacement source. The actor must also bind the host's hello
identity and stream token to its current configured computer.

The first known frame must be hello; a second hello or non-increasing sequence
ends that connection. No hello arrives later than the silence deadline, even
when unknown frames arrive. Unknown frame types retain the existing decoder's
skip behavior; the projection must treat unexplained sequence gaps as incomplete
coverage and resnapshot before authorizing effects. Frames carry a monotonic
receive timestamp captured before waiting for ingress space; applying an old
queued frame must not renew freshness. Status alone is never freshness proof.
Fatal errors end a source
regardless of topic. This transport grants no freshness or effect authority.

The caller borrows `Item::event()` and retains the item until it has applied or
explicitly discarded the frame. Its ingress reservation remains held while the
consumer owns it. Dropping the item releases the reservation. The actor must
retire an incarnation itself when deliberately stopping its handle; cancellation
does not wait for an Ended status to fit in a full queue.

## Bounds and cancellation

All sources on a channel share 16 MiB and 1,024 owned ingress items, including
reader-to-supervisor input and consumer-owned work. A frame is charged its UTF-8
line, newline and 1 KiB of source-envelope allowance; status costs 512 bytes.
Invalid UTF-8 is refused before conversion can expand it. Within those totals,
one status slot and 512 bytes per admitted source are reserved from data traffic,
so input-only pressure cannot prevent a lifecycle wake from reaching the actor.
Full ingress applies
backpressure, with cancellation checks, rather than dropping catalog deltas.
`Sender::usage` reports current and high-water counters.

At most 64 aliases and 64 reader groups exist per channel. At most four probes
or pre-hello connections run together. A group covers a probe's stdout or both
watch stdout and stderr. If an old descendant retains either pipe, that alias
cannot accumulate replacement reader threads: retry refuses until the group
ends. It reports incomplete/reconnecting coverage and sends no native effects.
No process groups or unrelated descendants are killed to release a pipe.

Stop cancels blocked admission, closes only the owned watch's stdin and uses the
existing bounded child-exit grace. Old-generation input is discarded before
reconnect status admission, so stale permits cannot deadlock a full ingress.
The stop flag remains authoritative even when a queued stop wake is discarded.
Per-reader line/probe buffers and decoded JSON allocation are additional bounded
memory; the 256 MiB whole-service RSS target still requires qualification.

This first raw mode supports watch-v1 peers only. An older peer is explicitly
unsupported and re-probed through the same supervisor; it starts no implicit
listing or inspection worker. A bounded adapter for the existing legacy schedule
and multiplexed Activity/Processes/Tasks interests must be added before service
takeover. This restriction does not change today's client legacy fallback.

## Focused proof

Tests cover global byte/item/connection limits, cancellation, duplicate aliases,
incarnation and stream generations, exact raw background facts, strict hello
and sequence admission, fatal errors, unsupported peers and child-pipe guards.
A 24 MB burst proves bounded ingress and prompt stop. Another fixture passes
50,000 source rows through the lifecycle path and verifies zero resident or
staged catalog rows. Existing stream, fleet and legacy suites verify the client
mode. Platform memory, native child cleanup and live handoff qualification remain
part of G1 acceptance.
