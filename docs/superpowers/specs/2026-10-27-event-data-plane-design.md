# Event data plane

Epic: `zerus-604d`. Owner direction, 2026-10-27: an event-driven data channel
is mandatory, and parsing and similar processing must run off the GUI (main)
thread. This design replaces per-request `hgs` process polling with one
long-lived event stream per computer and moves all data preparation in the
desktop off the GUI thread. The stream contract is shared by the Qt desktop,
the planned Tauri client (`2026-10-27-tauri-web-ui-design.md`), the mobile
connector and a future plugin host's `events.subscribe`.

## Baseline (2026-10-27)

Measured on the owner's Mac (M1 Pro, all sessions on `arch`) and on `arch`
itself, both running Dev 0.47.1-dev.1398 with `hgs 1.56.0`. Raw logs, scripts
and hashes are kept with the review report, outside the repository.

| Measure | arch GUI | Mac GUI |
|---|---|---|
| `hgs` children started per second (60 s idle window) | 3.63 | 3.60 |
| of which `git-status` per minute | 123 | 114 (each one SSH to arch) |
| of which local `ls` / peer `ls` / `inspect` per minute | 42 / 13 / 20 | 29 / 10 / 16 |
| GUI process CPU (self) | 2.4 % | 17.0 % |
| CPU of reaped children | 35.5 % of a core | n/a |
| RSS | 166 MiB | 139 MiB |

Command costs (median of 3–5 runs):

| Command | on arch | from the Mac to arch |
|---|---|---|
| `ls --json --local --no-archives` (35 live rows) | 280 ms, 294 KB | 599 ms, 294 KB |
| `inspect NAME --after CURSOR` | 117 ms, 171 KB | 524 ms, 171 KB |
| same with `--skip-processes` | 50 ms, 51 KB | — |
| `git-status --path P --json` | 21–223 ms, 0.4 KB | 500 ms |
| `ssh arch true` | — | 240 ms |

Of the 171 KB incremental inspection, 120 KB is the `processes` section. The
desktop includes it every 2.5 s while the Processes tab is shown and every 30 s
otherwise; other polls pass `--skip-processes` (51 KB). `--after` trims only
`events`. Every other section is rebuilt and resent each time.

JSON parsing itself is cheap. Qt `fromJson` + `boxFromObject` of the 294 KB
listing takes 1.75 ms on arch, and 6.9 ms for the 1.1 MB listing with archives.
The GUI-thread cost lies after the parse. A 15 s `sample` of the Mac GUI
(5,834 main-thread samples, about 2.6 ms each) attributes:

- 11.4 % of the main thread to `ProcessRunner` result callbacks;
- 7.2 % to `TrayAgent::onLocalReady`, about **150 ms per local poll**: three
  QSettings JSON re-parses, several full unread passes, `SessionsWindow::rebuild`
  and `AccountsPage::render`;
- 3.9 % to SHA-256 through OpenSSL (`QCryptographicHash`) inside `rebuild`, for
  account identity keys and row preference keys recomputed for every row on
  every poll;
- about 52 ms of GUI time per applied inspection, and 87 ms per peer listing;
- 19 % to painting, which is outside this design.

Each local listing row changes on every poll even when nothing happened:
`goal_observed_at` and `process_work.sampled_at` are observation timestamps.
Any delta protocol must mask them, or every delta becomes a full snapshot.

Session open from the Mac is therefore at least one SSH inspection (0.4–0.5 s)
plus about 50 ms of GUI work before the first Activity render. A reply appears
on the next 2.5 s poll plus one inspection: on average about 1.75 s and at worst
about 3 s after the agent wrote it. These two values are derived from component
timings. Milestone 1 adds instrumentation that measures them end to end.

## Rule: no data preparation on the GUI thread

The GUI thread posts requests and applies prepared, immutable values. It never
parses JSON, merges, diffs, sorts, filters, hashes or builds models from data
that came from `hgs`, the stream or disk.

### ProcessRunner preparation stage

`ProcessRunner` already starts children on its worker thread. Every result now
also passes a single serial preparation thread, `hgs-prepare`, before it reaches
the owner:

```cpp
ProcessRunner::run(request, owner,
    [](ProcessRunner::Result result) { return prepareListing(result); },   // hgs-prepare
    [this](PreparedListing listing) { apply(std::move(listing)); });       // owner thread
```

The prepare step sees only the result and values it captured. It must not touch
the owner or any other application-thread object. Results keep the order in
which their children finished, whether or not they have a prepare step, because
the legacy `run(request, owner, done)` passes the same serial thread unchanged.
A destroyed owner still drops a prepared value.

`HgsClient` converts each `runHgs` call to a prepared variant. In the first
step, signals keep their existing types (`BoxState`, `QJsonObject`, parsed
lists). Those are built off-thread and handed over by implicit sharing, so
receivers stay unchanged. Later steps replace them with typed prepared models.

### Enforcement

- `tray/src/JsonParse.h` is the tray's only JSON parser. In a GUI application it
  records a violation when it runs on the application thread. With
  `ZERUS_GUI_THREAD_CHECKS=fatal`, which the Qt test suites set, a violation
  aborts the test with the call site.
- `scripts/ci/check-source.py` rejects `QJsonDocument::fromJson` and
  `QCborValue::fromCbor` in `tray/src` outside `JsonParse.cpp`.
- During M1, small values the tray wrote itself were still read on the GUI
  thread through `JsonParse::local()`, capped at 64 KiB, and `check-source.py`
  listed the files and call counts allowed to use it. The list could only
  shrink, and each entry had a Beads task. Owner decision, 2026-10-27:
  «согласен, оставляем исключения до M2» — the list must be empty at the end
  of M2. It is empty (zerus-604d.11): `local()` is removed, `check-source.py`
  rejects any call to it, and the GUI thread has no parse exception. All `hgs`
  and stream output is parsed off the GUI thread from M1 on, without
  exception. The former entries now use:
  - prepared values: the user's answers in a prompt or activity detail are
    parsed with the row or the inspection in its prepare step and carried in
    `SessionInfo`; the update catalog, the Windows installer record, the
    connector configuration and notification tokens are parsed through
    `ProcessRunner::post`; question-answer draft keys are decoded by the
    draft index on its worker;
  - representations without a parse: in-process drag data is a
    `QDataStream`, account bindings are length-prefixed fields, and worktree
    reviews are kept per repository;
  - `SettingsJson` for the pending executor launches.
  Persisted formats are unchanged: draft keys, notification tokens and
  QSettings values written by earlier builds still read.
- JSON values kept in QSettings go through `SettingsJson`, and the GUI thread
  never parses them. Before the tray and its windows exist, `main()` parses
  every key the desktop keeps this way on a worker thread and waits for it
  (`TrayAgent::jsonSettings()`, with `*` for per-project and per-computer
  groups); writers store through `SettingsJson::store`, which records the
  value it serialized. A GUI-thread read of bytes nobody parsed, because
  another process changed the value or a key is missing from the startup
  list, is reported like a parse (fatal in the Qt suites); it returns the
  value last parsed for that key, or an empty one, and parses the stored bytes
  on the preparation thread for later reads. On arch the saved organization is
  66 KB and the swarm versions 102 KB.
- Tests: preparation runs on `hgs-prepare` and application happens on the owner
  thread; global result order is preserved; a destroyed owner drops prepared
  values; a fixture listing and inspection through `HgsClient` parse off the GUI
  thread; and a GUI-thread parse fails under the fatal check.

### Model preparation off the GUI thread

After the parse moves, the dominant cost is the work that follows each listing:
unread and attention computation, fleet merges, account catalogs, sorting,
grouping and per-row roles. Milestone 2 introduces a `FleetModel` worker. It
owns the merged fleet, read marks and organization inputs, and publishes an
immutable `PreparedFleet`: rows in display order with precomputed roles, group
headers, counts, unread and attention results, and a changed-row set. The GUI
applies only the changed rows to `SessionList` and the pages that are visible.
Read marks and attention marks are held in memory, written through on change,
and never re-parsed from QSettings per poll. Account and row-preference
identity hashes are computed once per identity, on the worker.

M2a, the first step, keeps the existing models. Each listing carries a digest
prepared off the GUI thread, with the observation fields masked. A listing equal
to the last one, with unchanged stored read state, only refreshes poll times,
metrics and background observations: rows rebuild only when the clock changes
their text, a peer crosses the staleness age, or the selected session's latest
inspection reads differently. Accounts and Machines render only while shown,
the tray menu rebuilds only when its contents change, the organization is
serialized and sent to the swarm only when it changed, and read replies,
attention marks and pending reads are applied in one unread pass. On arch, GUI
time per unchanged listing fell from 5.5 ms to 0.36 ms with the window shown
and per changed listing from 5.6 ms to 4.1 ms; M2b moves the remaining rebuild
off the GUI thread.

M2b-2 prepares the Sessions rows off the GUI thread. `SessionsWindow::rebuild()`
keeps on the GUI thread only the reconciliation of state the window owns: the
listed sessions with their organization identities, placement of launched
sessions, row-preference migration, and restore, fork and rename continuations.
It then copies what the rows read into `RowInputs`: the entries, fleet,
organization, row preferences, filters, the selection and its inspection, the
composers' unsent-draft keys from memory, the git status cache, machine colors
and the tooltip font. `prepareRows()` runs on the shared preparation thread
(`ProcessRunner::post`) and reads only that copy. It filters, counts and groups
the sessions, orders the rows with their subagents and launch placeholders, and
builds every role, the account catalog, the counts, the wrapped accessible text
and project tooltips (`QTextLayout` with the copied font lays text out without
painting), and a digest per row. Several rebuilds in one pass of the event loop
prepare once. Each preparation has a generation, and a result superseded by a
newer request is dropped. The GUI thread applies the newest result: filter
counts and list chrome, then the rows by key. Items are kept, moved with their
selection and persistent indexes, inserted or removed; roles are written only to
rows whose digest changed, and a reused item reads exactly as a new one would.
Selection, scroll and the details pane are applied as before. A choice made
during reconciliation (a placed creation, a fork, a restore or a rename) and a
row to reveal (Go to, notifications, attention navigation, a new launch) wait
for the applied rows. Status semantics are unchanged: the predicates the window
uses (`SessionRows.h`) are the ones the preparation uses. Preparing rows on the
GUI thread is a violation, counted and fatal in the Qt suites like a GUI-thread
JSON parse. Suites that read rows right after an action select Blocking
preparation, which still runs on the preparation thread while the GUI thread
waits; the application always prepares asynchronously.

On arch, GUI-thread CPU per changed 35-row listing with the window shown fell
from 4.1 ms to 1.7 ms in the listing handler, and from 5.3 ms to 3.6 ms
including the applied rows and the paint that follows (medians of three runs).
The row preparation itself, about 2.9 ms, runs on the preparation thread; the
list update of a changed listing costs 0.03 ms. What stays on the GUI thread is
the reconciliation (about 0.4 ms), the details pane render (about 0.5 ms),
selection actions and the paint.

## Event stream: `hgs [@host] watch --json`

### Transport

- One long-lived `hgs watch --json` process per computer. Local: a child of the
  desktop. Peer: `hgs @host watch --json`, one SSH connection with
  `ServerAliveInterval=10` and `ServerAliveCountMax=3`. The remote command is
  guarded by the `watch-v1` marker, the same way other newer commands are, so
  an old peer never receives an unknown command as an agent launch.
- Server to client: NDJSON on stdout, UTF-8, one frame per line. A frame is at
  most 1 MiB. Larger content is paged or truncated with a reference that the
  client fetches with an existing bounded command.
- Client to server: NDJSON control operations on stdin. EOF on stdin ends the
  stream, so a closed desktop or a dropped SSH connection never leaves an
  orphan watcher behind.
- `hgs -h` prints the line `Event stream ABI: watch-v1`. Clients that do not
  find it keep today's polling for that computer.

### Frames

Every frame has `v` (1), `t` (type) and `seq`. `seq` is monotonic per stream,
and each stream has a random `stream` ID that appears in its `hello`.

| `t` | Fields | Meaning |
|---|---|---|
| `hello` | `machine_id`, `host`, `hgs_version`, `stream`, `heartbeat_ms`, `max_frame_bytes`, `topics`, `limits` | first frame |
| `snapshot` | `topic`, `key?`, `items` or `value`, `page`, `last`, `cursor` | bounded replacement of one topic or key. Retain the previous display until `last` |
| `delta` | `topic`, `upsert[]`, `remove[]` with `key` and `reason`, `cursor` | keyed collection change |
| `events` | `topic: "activity"`, `key`, `events[]`, `cursor` | append-only Activity events after `cursor` |
| `event` | `topic: "activity"`, `key`, `id`, `rev`, `event` | replace one open event by stable ID; a higher `rev` wins |
| `state` | `topic`, `key`, `value`, `rev` | replace one keyed object (git status, process set, session summary) |
| `heartbeat` | `at` | sent after 10 s without other frames |
| `gap` | `topic`, `key?`, `reason` | the client must discard that topic's state; a snapshot follows |
| `error` | `code`, `message`, `topic?`, `fatal` | a fatal error ends the stream |

Control operations: `subscribe` and `unsubscribe` with `topic` and its key
(`session` as `{name, run_id, conversation_id}`, `path` or `project`), an
optional `after` cursor and `tail` count for Activity, and `ping`. The catalog
topic is always on.

### Topics

| Topic | Key | Content | Resume |
|---|---|---|---|
| `catalog` | `run:<run_id>`, or `name:<name>` for rows without a run | exactly today's `ls --json --local --no-archives` row objects. A rename is an upsert on the same key. A resume or restart that starts a new run removes the old key with `reason: "replaced"`. Removals carry `ended`, `archived`, `forgotten` or `replaced` when known | a fresh bounded snapshot. Clients diff it against their retained rows |
| `catalog.meta` | — | the non-row fields of the listing (`projects`, `peers`, `preferences`, `desktop_presence`, `archive_revision`, `hgs_version`, `metrics`) | snapshot |
| `activity` | session key | `events`: the same event objects as `inspect`, cursor = journal `seq`. `state`: the inspection summary without `events` and `processes`. `event`: open-event updates such as provider messages between tools | `after` = the last `seq`. Below retention the server sends `gap` and a tail snapshot |
| `processes` | session key | `state`: the `processes` section of `inspect` | snapshot |
| `git` | checkout path, as subscribed | `state`: the `git-status --json` object, sent when it changed apart from `sampled_at` and `remote_age_seconds` | `state` on subscribe |
| `tasks` | project | `delta` of task rows, as `task list` | snapshot |

Archives stay outside the stream. They remain a keyset-paged read (`archives
--json`), so the snapshot is bounded by live sessions, not by history.

Status semantics do not change. Rows carry the same fields; Working,
Background, Scheduled, Needs attention and unread reply identity are derived
by the clients exactly as today.

Change detection masks the volatile observation fields (`goal_observed_at`,
`process_work.sampled_at`). A row is sent when anything else in it changes.
`catalog.meta` with fresh `metrics` and presence is sent at most every 10 s.

### Event sources

No file watcher or async runtime exists in `hgs` today, and v1 adds no
dependency. One watcher process uses `std::thread` and cheap change detection
on a 250 ms pulse:

- **Activity**: a held read-only connection to `events.sqlite3`. When
  `PRAGMA data_version` changes, read `seq > cursor` for subscribed
  conversations (at most 200 rows per read). `seq` is global and monotonic and
  already serves as the `inspect --after` cursor. Hooks write it on every
  native hook event, and retention keeps 50,000 rows. Text between tools exists
  only in native transcripts. For subscribed sessions the watcher stats the
  transcript and, when it grows, re-derives provider messages from an offset
  cursor keyed on device, inode, prefix hash and offset (the existing
  tail-reader pattern). DeepSeek and OpenCode keep their inspect RPCs, polled
  only while subscribed.
- **Catalog**: per-binding stamps plus stamps of `renames/`, `name_routes/`,
  `read-marks.json`, `notifications.json`, `waits.sqlite3`, `launches/`, the
  archive revision and the provider binding directories. A changed binding,
  or new journal rows of its session, re-derives only that session's row
  through the listing's own row steps under the same lock (150 ms debounce, at
  most every 500 ms); any other change lists all rows again. A 5 s probe covers
  what no file shows: tmux sessions that start, end, are renamed, attached or
  detached, exited agent processes, ended background work, sessions without a
  hook event yet, and DeepSeek and OpenCode hosts. All rows are listed again
  every 5 s for terminal choosers and status lines of sessions that already
  reported hooks. 15 s would cut the watcher's CPU from about 6 % to 4 % of a
  core on arch but show such a chooser up to 15 s late; owner decision,
  2026-10-27: «делай а вариант как рекомендуешь» (keep 5 s). Within one status observation the effort
  footer and every terminal chooser share one capture per pane (35 instead of
  62 `capture-pane` per listing on arch). Bindings are replaced by rename;
  temporary `.hgs-*` files are ignored. Measured on arch: the watcher used
  12.9 % of a core with whole listings on every change and 6.1 % after.
- **Processes**: hook-recorded shell jobs (the binding stamp) plus one `ps`
  work sample every 2 s per subscribed session. The full snapshot, which
  re-reads the transcript tail, runs at most every 10 s.
- **Git**: stats of `HEAD`, `index`, the current branch ref, its upstream's
  tracking ref, `packed-refs`, `FETCH_HEAD` and the reftable list in the git
  directory and common directory of each subscribed path, on the pulse.
  Journal rows of finished `Edit`, `Write`, `MultiEdit`, `NotebookEdit`,
  `apply_patch` and `Bash` calls in sessions whose `cwd`, `cwd_canonical` or
  `git_root` lies under that path also trigger a check, and so does the expiry
  of its 60 s remote check, which keeps a verified remote verified. There is a
  30 s safety tick, and checks of one path start at least 1 s apart. The
  `git-status` code (with its 60 s remote cache) runs in-process on two worker
  threads, the concurrency the desktop's polls had, so no check delays the
  stream's other frames. The desktop gives each live stream the folders of its
  computer's sessions (selected first, then shown rows) and polls only folders
  the stream does not follow; a followed value is dated anew every 15 s, with
  its remote age carried forward.
- **Tasks**: stats of `replica/<project>/project.loro` and its `tasks/`
  directory, then the existing task listing. Loro stays inside `crates/replica`.

A later version may replace stat pulses with inotify/kqueue/FSEvents behind
the same frames. The protocol does not depend on how changes are detected.

### Bounds and backpressure

- Subscriptions: at most 16 Activity sessions, 8 process sets, 64 git paths
  and 8 task projects per stream. Excess subscriptions fail with an `error`
  frame.
- A writer thread owns stdout with a bounded queue (4 MiB). When the client
  stops reading, pending `state` and `delta` frames coalesce per topic and key
  (latest wins). If the queue still overflows, the affected topic receives
  `gap` followed by a fresh snapshot once the client drains.
- Snapshot pages are at most 512 KiB. Activity tails are at most 100 events,
  with older history read through the existing paged commands.
- Listing work is bounded exactly as today. The watcher never adds per-session
  caches beyond subscribed sessions.

### Consumers

- **Qt desktop**: one `EventStream` per computer. Its reader, line framing,
  JSON parsing and per-computer store run on a worker thread. The store
  publishes immutable prepared values to the GUI: Activity within 50 ms, and
  listings at most every 2 s until M2 makes applying one cheap. A listing
  arrives as the same `BoxState` a poll delivers, and Activity as an
  inspect-shaped increment, so the existing apply paths stay unchanged. While a computer's stream is healthy, the desktop
  stops its `ls`, inspection, `git-status` and `processes` polls for it. A
  missing marker, or three stream failures within 60 s, falls back to today's
  polling, and the desktop retries the stream every 5 minutes.
- **Tauri client**: its Rust backend spawns the same subprocess per computer,
  parses frames in Rust (never in the webview) and pushes view deltas to the
  webview, coalesced per animation frame. A small serde crate
  `crates/watch-protocol` defines the frame types. It is shared by `hgs` and
  the Tauri crate and contains neither Loro nor Qt.
- **Mobile connector**: it keeps polling until it opts in, then consumes the
  same frames. API v1 and its journals stay unchanged.
- **Plugin host**: `events.subscribe` maps onto these topics, filtered by the
  manifest's permissions.

## Compatibility

- `hgs` CLI commands, `HGS_*` variables, state paths, API v1 and the connector
  journal stay unchanged. The stream is additive.
- Older peers lack the `watch-v1` marker and keep today's polling.
- An older desktop never starts `watch`. The command runs only while a client
  holds its stdin open.
- Session counts may reach tens of thousands. The live catalog is bounded by
  live sessions, archives stay paged, Activity is cursor-based, and the
  watcher holds no unbounded per-session state: its only per-session map,
  from binding file to session and journal name, is bounded by the binding
  files.

## Milestones

| | Scope | Measure afterwards |
|---|---|---|
| M1 | Preparation stage, every parse of `hgs`, connector and network output off-thread, enforcement, settings values parsed once per change, cached identity hashes | Mac `sample` main-thread share, per-poll GUI time |
| M2 | `FleetModel` worker: unread, attention, sort, group and row roles prepared off-thread; the local-value list empty; open/reply latency instrumentation | per-poll GUI time below 5 ms |
| M3 | `hgs watch` server: catalog and Activity topics, `crates/watch-protocol`, `docs/reference.md` | spawns per second, idle CPU on arch |
| M4 | Qt `EventStream` consumer with fallback, local and peers | session open and reply latency from the Mac |
| M5 | git, processes and tasks topics; connector and plugin adoption | spawns per second close to zero while idle |
