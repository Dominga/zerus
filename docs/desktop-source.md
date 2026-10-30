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

The future actor creates exactly one source channel for its already-owned
configuration. `source::channel` is watch-only. The optional
`source::channel_with_legacy` also accepts a host `SpoolFactory` and creates one
shared two-slot private-spool pool. Either constructor starts nothing. Raw
admission requires the exact whole help marker line before any watch invocation.
It passes the host's guarded spawn function: hgs on Unix uses
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

`Handle::resnapshot(generation)` requests a complete snapshot by reconnecting
the current greeted watch through that same supervisor. It is a single
coalesced, nonblocking transient request: stale, offline and repeated requests
refuse, and closing that generation cancels it. Admission never waits for an
input-queue slot. The actor continues consuming/discarding old ingress while the
owned watch closes, rejects its old generation, and waits for a new hello and
complete snapshot before restoring coverage. It grants no effect authority.
Existing `reconnect()` remains the offline manual retry with its old semantics.

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

Retained supervisor lifetimes have their own 64-worker admission bound,
independent of handle registrations. The endpoint and source reader keep that
worker reservation until their work ends. Receiver drop sets each admitted
source's weak cancellation flag and wakes its existing control queue, so a
quiet source does not need another frame to notice closure. Cancelled endpoints
cannot admit another reader group.

Stop cancels blocked admission, closes only the owned watch's stdin and uses the
existing bounded child-exit grace. Old-generation input is discarded before
reconnect status admission, so stale permits cannot deadlock a full ingress.
The stop flag remains authoritative even when a queued stop wake is discarded.
Source-only handle stop uses a nonblocking wake and waits at most one second
for its supervisor. Pending OS I/O retains its worker/reader reservation after
that bounded return; the existing GUI stream shutdown keeps its behavior.
Returning from stop proves cancellation was requested, not that a writer drain
finished. When the service wires sources, revocation must wait for workers,
readers, connection attempts and component reservations to reach zero before
settling takeover. A stuck operation keeps that drain refused.
Per-reader line/probe buffers and decoded JSON allocation are additional bounded
memory; the 256 MiB whole-service RSS target still requires qualification.

## Optional older-peer listing

Watch-only channels continue to report older peers as unsupported without
starting listings or inspections. When the host supplies a private-spool
factory, the same supervisor instead uses `legacy::Schedule` with no followed
sessions. It runs the existing `hgs [@alias] ls --json` arguments single-flight
at the existing 60-second interval, with a 15-second child/read deadline. It
never calls `inspect`, starts another poller or populates client projections.
Capability re-probes use the existing ten-minute interval. An upgraded peer
returns to watch on that same alias/supervisor, without parallel listing work.
Today's GUI fallback keeps its existing path.

Before file creation or child spawn, the adapter reserves the alias reader
group and one of the channel's two 32 MiB spool slots. A busy pool waits at most
15 seconds, cancellably; shared connection admission still caps attempts at
four. The factory runs on the supervisor background thread and must supply a
new empty private ephemeral file through the host's existing filesystem checks.
No file paths are opened here. The host removes its file on drop or supplies an
already unlinked file. A blocked factory retains its worker, reader, connection
and spool admissions after bounded handle stop; it cannot satisfy writer drain.
Pending child-pipe readers also retain worker and alias admission. Usage exposes
`legacy_spools` and `legacy_reserved_bytes` for that future drain.

Incremental decode stages at most 32 MiB of input and 32 MiB of encoded private
spool per slot, with at most 250,000 rows, 128 rows per page and the existing
frame/page/metadata bounds. It keeps one current page, not a sessions array or
page-offset map. Only successful decode, real EOF, both pipe completions and
owned-child exit zero permit replay. Connection admission is released before
replay waits on ingress. Replay has a separate 15-second deadline, including
backpressure waits. File I/O that remains blocked keeps its reservations.

The local `Event::Legacy` carries the configured alias/incarnation, replacement
generation and one immutable observation timestamp captured before child spawn.
`Legacy::Begin` carries bounded top-level metadata and row/page/input-byte
counts. Numbered `Page` items retain raw catalog fields, including unknown
background facts. `Complete` is emitted separately only after full spool digest,
counts and EOF verification. Begin and pages remain provisional until Complete;
a failure, replacement or retired source abandons them. Every item is charged
against the same global ingress byte/item budgets, including consumer ownership.
No legacy event manufactures a peer watch hello. The actor must verify configured
computer identity from metadata and decide freshness; missing identity or a
recent local apply alone cannot establish current-action authority.

`Handle::resnapshot(generation)` also coalesces a legacy replacement through
that same schedule. It never starts a parallel child. Manual refreshes have a
minimum one-second start gap across legacy generations, while configured
periodic intervals retain their semantics. A request during replay waits for
that bounded replay to finish or fail; queued old-generation items remain old.
The actor must still bound repeated watch-mode resnapshots across greetings.
Multiplexed Activity/Processes/Tasks interests and root service activation remain
separate work before takeover.

## Focused proof

Tests cover global byte/item/connection limits, cancellation, duplicate aliases,
incarnation and stream generations, exact raw background facts, strict hello
and sequence admission, fatal errors, unsupported peers and child-pipe guards.
A 24 MB burst proves bounded ingress and prompt stop. Another fixture passes
50,000 source rows through the lifecycle path and verifies zero resident or
staged catalog rows. Existing stream, fleet and legacy suites verify the client
mode. Platform memory, native child cleanup and live handoff qualification remain
part of G1 acceptance. Older-peer tests additionally cover a 100,000-row private
listing, exact raw fields, failed child exits, late spool corruption, bounded
resnapshots, capability upgrade, private-factory failures and cancellation while
two shared factory reservations remain blocked.
