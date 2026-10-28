# Zerus Next (Tauri client)

The new desktop client of Zerus, growing beside the Qt GUI (`tray/`). Design:
`docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`; plan:
`docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`. The approved visual
reference is `design/webui`; its components moved here and are bound to live data.

| Path | What it is |
| --- | --- |
| `src/` | SolidJS frontend: `bridge.ts` (per-frame batches), `store/` (catalog, Activity, interface state), `ui/` (rail, session list, Activity, styles), `skin/` (skin format v1 and the built-in skins) |
| `src-tauri/` | Tauri 2 app `zerus-client`: window creation, commands (`start`, `next_batch`, `follow`, `unfollow`, `metrics`), the change pump and the replay |
| `core/` | `zerus-client-core`: watch streams, catalog state and the coalescer, pure Rust |
| `fixtures/` | Synthetic replay streams (`scripts/make-fixture.py` writes them) |

The app is a separate application (`dev.zerus.next`, "Zerus Next (dev)"). It only
runs `hgs [@host] watch --json`, which is read-only; it never starts, stops or
replaces the installed Zerus GUI, CLI, agents or services.

## Build and run

```sh
cd client
npm ci
npm run check                  # TypeScript
npm test                       # Vitest: stores, transcript, screens
cargo +1.99.1 test --manifest-path src-tauri/Cargo.toml
RUSTUP_TOOLCHAIN=1.99.1 npx tauri dev
```

`npm run dev` alone serves the frontend in a browser; without Tauri it replays
`fixtures/replay-batches.json`, so the UI works without the app.

Environment:

| Variable | Effect |
| --- | --- |
| `ZERUS_CLIENT_HGS` | The `hgs` to run (default: the first `hgs` on `PATH`, then in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`, since an app started from Finder or a launcher gets a short `PATH`) |
| `ZERUS_CLIENT_REPLAY` | Replay synthetic watch streams instead of running `hgs`: a path list (`:` on Unix), the first file is this computer, `alias=file` entries are peers, e.g. `fixtures/replay.ndjson:mac=fixtures/replay-mac.ndjson` |
| `ZERUS_CLIENT_REPLAY_INTERVAL_MS` | The pause per `heartbeat` frame of a replay (default 900) |
| `ZERUS_CLIENT_QUERY` | URL parameters for the window, e.g. `select=claude/sample-project/api-refactor` opens that session and `page=settings` that screen |

A build without the `live` feature (`--no-default-features`) has no data source;
the frontend then uses its built-in replay.

## Headless runs

Development windows never open on a desktop someone is using. On Linux run the
app inside Xvfb and force X11: with `WAYLAND_DISPLAY` merely unset, GTK falls
back to the default Wayland socket (`wayland-0`) and the window opens on the
desktop. `ZERUS_CLIENT_HEADLESS=1` makes the app exit (code 3) instead of
opening a window anywhere but an X server display. WebKitGTK paints a black
window in Xvfb unless its DMA-BUF renderer is off:

```sh
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a -s "-screen 0 1440x900x24" \
  sh -c 'RUSTUP_TOOLCHAIN=1.99.1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson npx tauri dev'
```

## Actions

The session menus, the header and the Rename and Terminate dialogs run guarded
actions through the `hgs` CLI (`core/src/actions.rs`, `src-tauri/src/actions.rs`),
with Qt's enablement, confirmations and texts (`actions-ui.ts`). A peer's call
is prefixed with `@alias` and uses the longer timeout. Every call is made with
the child's own environment: the agent attribution (`HGS_SESSION`,
`HGS_RUN_ID`) is never passed on.

| Action | Arguments (`NAME` is the session name) | Standard input | Timeout here, peer |
| --- | --- | --- | --- |
| Stop (interrupt) | `interrupt NAME --json` | JSON: `request_id`, `expected_run_id`, `expected_conversation_id`, `expected_turn_started` | 15 s, 45 s |
| Mark as read | `read NAME --json` | JSON: `run_id`, `conversation_id`, `reply_id`, `attention_signature` | 15 s, 35 s |
| Mute or Unmute, probe (always on this computer) | `--help` | none | 5 s |
| Mute or Unmute | `notifications NAME --json` | JSON: `run_id`, `conversation_id`, `muted`, `expected_revision` | 15 s, 35 s |
| Pause | `pause NAME` | none | 45 s, 45 s |
| Resume | `resume NAME -d` | none | 30 s, 30 s |
| Start fresh | `resume NAME --fresh [--expected-run-id RUN] -d` | none | 30 s, 30 s |
| Pause all, one computer | `pause --all` | none | 15 s + 30 s per live session, same on a peer |
| Resume all in background, one computer | `resume --all -d` | none | 15 s + 15 s per saved session, same on a peer |
| Rename | `rename NAME NEW [--archive ID]` | none | 5 s, 15 s |
| Move to archive | `archive NAME` | none | 30 s, 45 s |
| Terminate a tracked Codex, Claude or Kimi session | `terminate NAME --expected-run-id RUN` | none | 45 s, 60 s |
| Terminate any other live session | `kill NAME` | none | 5 s, 15 s |
| Forget saved session | `kill NAME` | none | 5 s, 15 s |
| Forget the OpenCode binding | `session-action NAME --json` | JSON: `request_id`, `action` (`forget`), `expected_run_id`, `expected_conversation_id` | 15 s, 45 s |
| Restore session | `resume NAME --archive ID -d` | none | 30 s, 30 s |
| Forget archived session | `kill NAME --archive ID` | none | 5 s, 15 s |
| Archive page (a read) | `archives --json` | JSON: `from`, `until`, `limit` (50), `cursor`, `query` | 30 s, 30 s |

Outcome rules:

- A call is made once and never repeated. A timeout, a failed hand-over or an
  abnormal end is an unknown outcome, shown as a failure and never replayed.
  Stop words them "Request timed out. Check Terminal before retrying." and
  "Transfer failed. Check Terminal before retrying."; the other actions show
  "timed out" or the trimmed standard error `hgs` printed.
- Stop succeeds only when the receipt echoes the request id, session name, run
  id and conversation id of the call (status `submitted`); anything else is
  "Invalid acknowledgement. Check Terminal before retrying." Its notices are
  Qt's: "Interrupt sent to NAME (COMPUTER). Waiting for the agent to stop…",
  "Stop NAME (COMPUTER): the turn already ended or is waiting for input. No
  action needed." when `hgs` answers "This turn is no longer working", and
  otherwise a sticky "Stop NAME (COMPUTER) failed: …" with "Open Terminal to
  check the agent before trying again."
- Pause, Resume, Rename and kill succeed on exit 0. Move to archive and
  terminate also read the single worktree report line on standard error: a
  removed worktree gives an information notice, a kept or unconfirmed one a
  sticky warning with its reason. Otherwise a failure shows the trimmed
  standard error.
- An entry acts on the row as it was when its menu, button or dialog was
  built. If the listed row has another run id by then (or, for a row without
  one, is gone), nothing runs and the notice says "This session changed. Open
  its actions again." Stop, Mark as read and terminate need the row's run id
  (Stop and Mark as read also its conversation id); the other actions name
  the session only.
- One write per session at a time. While it runs the session's entries show
  "Updating…"; another write for that session is a silent no-op (the Rust side
  answers `busy`). Stop, Mute and the Rust worker's read marks each run in their own
  lane: Stop waits only for its own interrupt, as in Qt, and neither a Mute nor
  a read mark holds up a write. Mark as read stays available while a write runs.
- Terminate stops a tracked Codex, Claude or Kimi session into Archive and
  kills any other live session; the confirmation says which. After the
  confirmation (and after a menu's Pause or Resume) the session must still be
  running, or still not running, as it was; otherwise nothing runs and the
  notice says "The session state changed. Open its actions again."
- Rename edits only the tag after the agent and project, with Qt's tag rule
  ("Enter a session name.", "Remove spaces at the beginning or end.", "Names
  cannot contain /, : or . or control characters.") and "This name is already
  in use on COMPUTER." for a live session of that name on the same computer.
- Stop is shown while the selected session's turn works (`working`, `tool` or
  `compacting`) and enabled when its computer is online and the details say
  `interrupt_supported`; it sends the details' run, conversation and
  `turn_started`. Hosts never list `interrupt` among the allowed actions.
- Details (allowed actions and their reasons, `auth_required`, Stop's fields)
  come from the followed session's Activity `state` value when it names the
  row's run and conversation. Only when the subscription answered without one
  does the client run one read-only `inspect`, refetched when the run,
  conversation or state changes, and never while the computer is offline.
- Read state is local first, as Qt's FleetState keeps it (`src/store/marks.ts`).
  This device keeps attention marks (marked for review, the acknowledged
  attention) and, for hosts without shared read state, the replies it read.
  Each row is shown with them applied: a reply is unread until the host's
  read mark or a pending read mark of the same run and conversation names it
  (on an older host, until this device read it); an attention the host signs
  is acknowledged by the host's mark or a pending one, and one without a
  signature by this device's mark for its fingerprint. Acknowledged attention
  shows its acknowledged status, and an unread reply blanks the current action
  unless the session needs action. A mark names the reply, request and
  attention it was made for, so a newer one shows again. The store's Mark as
  read, Mark needs attention and Mark all as read change the rows at once, save
  only the changed entries (`update_attention_marks`, after the stored marks
  have loaded) and queue read marks for hosts with shared read state
  (`queue_read_marks`). A host that starts sharing read state gets this
  device's older marks for its current replies and attention queued once.
- Pending read marks are kept in `read-marks.json` in the app data directory
  (`app_data_dir`, e.g. `~/.local/share/<bundle id>/` on Linux; a replay uses
  `read-marks-replay.json`), so a restart keeps them. A mark is settled, and
  dropped, once a row shows another run or conversation, or each of its parts
  unset, no longer current or read by the host; a complete snapshot of its
  computer that no longer lists the session drops it too (an empty one never
  does). The window keeps a copy for the rows: loaded at start, replaced by
  each `queue_read_marks` answer (an older answer never undoes a newer mark)
  and settled by the same rules. Nothing in the window sends a mark again.
- Read marks reach their hosts through a worker in the Rust app, as Qt's
  `ReadMarkSync` does. `queue_read_marks` merges marks in memory and wakes it;
  otherwise it wakes every 5 s. Each wake first writes new marks to
  `read-marks.json`, so a mark is saved before any call. At most four
  `hgs read` calls run at once, each free slot going to the computer with the
  fewest. Until a snapshot settles a mark, a delivered one is sent again after
  120 s (doubling to 960 s while its host applies only part of it) and a
  failed one after 10 s, doubling to 300 s; a changed mark goes at once. Each
  call takes its session's read lane; a busy lane leaves the mark for the next
  wake. A delivery shows no notice. Once this computer's listing names its
  peers, marks for a computer it no longer names are dropped (this computer's
  stay). A replay without `ZERUS_CLIENT_HGS` sends nothing.
- This device's attention marks and the read replies of hosts without shared
  read state are kept in `attention-marks.json` (a replay uses
  `attention-marks-replay.json`). `update_attention_marks` writes or removes
  entries in memory; each map keeps its 20 000 newest written entries, in the
  window too: `attention_marks` answers each map's keys oldest written first,
  and the window writes one update's entries in key order as the Rust side
  does, so both drop the same entries. A Mark needs attention made before the
  stored marks arrive changes only `review`. A writer thread reads the file at
  start and saves the latest state after each change, listing entries in write
  order, so the window never waits for the disk. The computers whose older
  marks were queued are remembered in the webview's local storage. Outside the
  app (a browser) there is no pending list, and the window keeps its own.

- Start fresh is offered for a paused or stopped Codex, Claude or Kimi session
  of an online computer with no write pending. The dialog "Start fresh?" says
  that the agent starts in the same folder with the same account and model
  without the current conversation context, and its default button is Cancel.
  On confirmation the row must still qualify and be the same run and
  conversation; otherwise nothing runs and the sticky notice says "The session
  changed. Open its actions again." Otherwise the call passes the run as
  `--expected-run-id` (omitted when the row has none) and the notice says "Starting NAME with a
  new conversation…". The header offers it in the main context only, in place of
  Clear session, and not at all while a subagent is open.
- Forget saved session ends a saved session that is not running, or an OpenCode binding (usually running). The dialog
  "Forget saved session?" says "Remove NAME from HGS? The agent's native
  conversation history is kept." and, for an OpenCode session that only keeps
  a binding, "Forget NAME in Zerus? The conversation stays in OpenCode; only
  Zerus forgets it." with the computer named below; Cancel is the default.
  On confirmation the session must be as when the dialog opened (still not
  running unless it is an OpenCode binding, the same binding-only case and, for a binding, the same run and
  conversation); otherwise nothing runs and the sticky notice says "The
  session state changed. Open its actions again." A binding with a run goes
  through `session-action`, any other session through `kill`; an offline
  computer ends silently; a vanished row raises the same notice.
- Forget archived session needs the archive id and an allowed forget. The
  dialog "Forget archived session?" says "Remove this archived instance of NAME
  from HGS? Other sessions with this name and the agent's native conversation
  history are kept." with the computer named below; Cancel is the default. On
  confirmation the archived row must still be listed as the instance the dialog
  named (its archive id and name, and `created` when known), otherwise nothing runs and
  the sticky notice says "This session is no longer available. Refresh the list
  and open its actions again."; an offline computer or a running batch ends
  silently. The call is `kill NAME --archive ID`; after a success the archive
  pages reload, and a pending restore's follow is cancelled.
- The binding forget counts only when the receipt echoes the request id,
  session name, run id and conversation id of the call with status
  `completed`; anything else is a failure and is never replayed. Success shows
  "Zerus forgot NAME. The conversation stays in OpenCode."
- Start fresh and Forget saved session first recheck the listed row when the
  entry is clicked: a session that is gone or whose conversation or `created` changed opens
  no dialog and shows "This session is no longer available. Open its actions
  again." Restore session and Forget archived session do the same, and also
  for an archived row listed again under another name (the client keys an
  archived row by its archive id; Qt's key also holds the name).

Restore session is an archived row's change entry. It is enabled when the
computer is online, no write is pending, the row is resumable and has its archive
id, and no live row of that computer has its name. As in Qt's header and the
menu of the selected row (a right-click selects the row), it does not read the
row's allowed operations; a batch's Resume / restore does. The call is
`resume NAME --archive ID -d`. While the same session's restore is pending, another single restore runs or
64 restores are pending, nothing runs and the sticky notice says "Finish or
refresh the earlier restores before restoring another session." A restore stays
pending until its live (running) row appears, or until its call fails.

- After a success the archive pages reload, and the Archive filter becomes All
  when the restored row is still selected there; the search, machine and project
  filters stay. The archived row stays selected until its live row appears.
- When the live row appears with the restored conversation, the selection moves
  to it (also when it carries no conversation at all). With another
  conversation the selection clears with the sticky notice "The restored
  conversation could not be verified." Qt adds that the archived draft remains
  in Saved drafts; the client has no saved drafts before phase 4
  (`zerus-5wmh.9`).
- A change of the state filter or the search, a Rename, a Forget archived
  session or a batch cancels the follow: the verified live row then only clears
  an old selection of the archived row.

A successful Rename or Move to archive also reloads the archive pages, as Qt's
`writeDone` does. Sending, the Mark read chip of the composer toolbar and the
Open terminal entries arrive with phase 4. The copy entries below are live.

### Batches

Selecting more than one row shows the selection bar above the list (the client
has no single-row bulk mode). The same actions are in the context menu of a
selected row, under the heading "N selected sessions", followed by "Deselect all".

- Tools, in this order: Compact session rows, Expand session rows, Mark as
  read, Mark needs attention, Pause, Resume / restore, Move to archive, Move to
  project, Forget, Terminate. The tool text is "ACTION (COUNT)" and the hint
  "ACTION — COUNT of N selected sessions". A tool with a count of 0 is hidden
  from the bar; the others that do not fit move into "More actions for selected
  sessions". The close button has the hint "Deselect all".
- A tool is enabled when its count is above 0, and, except Compact and Expand,
  when no batch is running. The count is the number of selected rows the action
  applies to, by Qt's rules: Compact and Expand follow the row preference; Mark
  as read needs a row that is not archived and needs attention, Mark needs
  attention one that does not; every other action needs the computer online, no
  write pending for the session and an operation the row allows (Resume / restore
  is Resume for a live-listed saved session and Restore for an archived one);
  Move always applies. Only listed rows count: a search or filter hides rows from the selection, and vanished sessions drop out. An archived Resume
  also needs the archive id and no live row of that computer with its name,
  checked against one index of the live names.
- Move to project is shown with its count, disabled, with the hint "Arrives with
  project organisation." until the organisation stream's move lands.
- Forget and Terminate first confirm. The title is the action, the text
  "Forget N selected sessions?" or "Terminate N selected sessions?", then
  Qt's suffixes: what is removed or stopped and what is kept, the OpenCode line
  when bindings are forgotten, and "K unavailable or inapplicable sessions will
  be left as they are." when some selected rows do not apply. "Show details"
  lists "MACHINE / NAME" per target. The buttons are Cancel (focused and the
  default) and "Forget N sessions" or "Terminate N sessions"; Cancel runs nothing.
  Qt's "Remove working folders after stopping" checkbox and its "Stop N
  sessions" labels arrive with `zerus-5wmh.10`.
- Mark as read and Mark needs attention run through the marks store at once and
  report "ACTION: C of N sessions updated."; an error notice that stays when
  the counts differ.
- The other actions run one session at a time. Before each call the row is
  read again by its key and must be the same instance as when the menu opened
  (name, archive id, `created`, run, conversation and state) and still
  apply; otherwise the item fails with "MACHINE / NAME: session changed or is
  unavailable" and no call is made. Each item shows "ACTION: I / N…"; the end
  shows "ACTION: D of N completed." and, when items failed, "F failed or
  changed; refresh before retrying." with the first three errors, as a notice
  that stays. A call is made once and never retried, and a failure does not stop
  the next item, also when it throws. The calls are the single-session ones of
  the table above, with their own success and failure notices replaced by the
  batch's. An archived row restores (past the restore limit, also for a second
  instance of a name restored earlier in the batch, it fails with "NAME: earlier
  restores still need a refresh") or forgets its archived instance. When a write batch ends, the archive pages reload
  (Qt's own reload after a batch never runs, so this is a deviation).
- While a batch (or a Pause all or Resume all) runs, every write entry is
  disabled, the header shows "Updating…" and a second batch does not start. Stop
  and the copy entries are not writes.

Remove loaded archived sessions… in the batch menu is Qt's Clear archive. It is
enabled when no batch runs and some loaded archived row in the machine and
project filters can be forgotten. It runs Forget on the loaded archived rows in
those filters and the date range, with the title and button "Clear archive" and
the text "Remove N loaded archived sessions in the current date and machine
scope? Unloaded entries are kept." plus the Forget suffixes.

Pause all and Resume all are in the batch menu, one group per computer: a
heading with the machine name, "Pause all (LIVE)" and "Resume all in background
(SAVED)". LIVE counts the running sessions, SAVED the resumable ones that are
not running; archived rows are skipped. Pause all is enabled when the computer
is online, LIVE is above 0 and every live session is idle and resumable and
whose process has not exited; otherwise "Some sessions are busy or context is not
tracked" appears. Rows of OpenCode or a host without bulk lifecycle are not
counted and show "OpenCode uses individual Pause / Resume controls" with the
hint "OpenCode sessions are excluded; use their individual Pause / Resume
controls." The calls are `pause --all` and `resume --all -d` (table above), take
the computer-wide write claim and name no session. They show "Pausing sessions
on MACHINE…" or "Resuming sessions on MACHINE…", then "Session updated.
Refreshing…" or the trimmed standard error as a notice that stays, and are never
retried.

### Mute and Unmute

The entry changes the notification preferences a host shares with all devices.
It first runs `hgs --help` on this computer (5 s) and refuses unless a line
of its output is exactly `session-notifications-v1`; it then runs
`hgs [@alias] notifications NAME --json` with `run_id`, `conversation_id`,
`muted` and `expected_revision` on standard input (15 s here, 35 s on a peer).
The change counts only when the reply is exactly `ok`, `applied`, `run_id`,
`conversation_id` and `preferences`, names this session's run and
conversation, carries the requested `muted`, and has revision
`expected_revision + 1` when applied or the same revision when not. The
entry is disabled, with its reason in this order, when the computer is offline,
the host does not share notifications, the row's identity is missing, the
preferences conflict or a change is already pending. There is one pending
change per conversation and 16 in all. The change has its own lane: it never
waits for a write, Stop or a read mark. A confirmed answer is kept as an
override of the listed row until a listing shows a higher revision; a late
answer for a replaced run or conversation is dropped.

### Mark all as read

The entry "Mark all as read (N)" is in the batch menu, the list footer button
and the Rail Sessions menu. N counts every unread reply on every computer,
hidden sessions included, except sessions marked for review; at 0 the entry is
disabled. A run marks those replies read and shows "1 reply marked as read."
or "N replies marked as read."

### Copy actions

The copy entries build text locally or ask `hgs` for it and put it on the
clipboard. They are not writes: they need no row lock and show no "Updating…".

| Entry | Text |
| --- | --- |
| Copy connect command | `hgs [@alias] a NAME` |
| Copy restore command (archived row) | `hgs [@alias] resume NAME --archive ID` |
| Copy session reference | the `zerus://session?` URI printed by `hgs [@alias] reference NAME --json --run RUN --conversation CONVERSATION [--archive ID]` (15 s here, 25 s on a peer) |
| Copy folder path | the `cwd` of the selected session's details, else the row's; disabled when empty |
| Copy session name | `NAME` |

- The commands are quoted with Qt's SessionCommands rules (POSIX shell; PowerShell
  on Windows) and name the `hgs` the client runs: `ZERUS_CLIENT_HGS`, else the
  one found on `PATH` or in the install folders.
- The reference takes the run and conversation of the selected session's
  details, else the row's. A reply must be a `zerus://session?` URI of at most
  8192 characters; otherwise the notice says "Could not read a session
  reference. Update hgs on this computer." Success shows "Session reference
  copied".
- The clipboard is written from Rust (tauri-plugin-clipboard-manager). A text
  that is empty or holds a line break is refused with Qt's texts. A failure
  shows "Zerus: could not copy" with the error. A success shows no notice,
  except for Copy session reference. The browser build (no Tauri) copies
  through `navigator.clipboard` with the same two checks.
- Each copy rechecks its row when it runs, as Qt does: a session that is gone
  or whose conversation changed copies nothing and shows "This session is no
  longer available. Open its actions again."; otherwise the current name,
  archive id, folder and run are copied.

### Archive

The state filter Archive lists archived sessions page by page, as Qt's
`ArchiveBrowser` does. The rows are loaded on demand and are never part of the
live catalog.

- Each live computer inside the machine filter has one page of at most 50 rows.
  Older and Newer replace the page (a history never grows), and Refresh archive
  drops the pages and loads the first. Pages load only while the Archive filter
  shows the Sessions list and are kept when it is left, as Qt keeps them.
  Leaving the filter, a change of the computers or their names, a change of
  the search text, a change of the date range and Refresh archive cancel the
  reads that are running, also one sent before the cancel that has not started
  yet; an answer that arrives afterwards is dropped. Each read carries the
  store's request number, so a late older read never cancels a newer one.
- The search text is the archive query, applied 300 ms after the last change.
  It does not filter the loaded rows.
- The date picker offers All dates (the default), Today, Last 7 days, Last 30
  days and a custom range, which starts at the last 30 days. A range whose
  start is not before its end loads nothing.
- The status line reads "Loading archive…" while a page loads, then
  "{a}–{b} of {total}" or "{shown} shown of {total}" for a single computer
  with a window, "{shown} loaded of {total}" once every total is known and
  "{shown} loaded" before. Each computer's window is in its tooltip. An
  error reads "{machine}: {error}" (240 characters shown, the full text in
  the tooltip). A read that fails reads as Qt words it: "timed out",
  "cannot start '{hgs}': {error}", the standard error (4000 characters at
  most) or "exit code {n}". Older and Newer lose their text below 310 px. The
  count chip of the filter shows "…" while some computer's total is unknown and
  no archived row is counted, otherwise the count with a "+" while more can be
  loaded; its tooltip says how many are loaded.
- The list shows the loaded archived rows as flat cards, newest archived first
  (Qt groups them by project: `zerus-5wmh.40`). Selecting one shows its header
  and its details, read with `inspect --archive ID`; its Activity pane stays
  empty (`zerus-5wmh.42`). Restore session, Forget archived session, the copy
  entries and Rename work on a loaded archived row (see Actions).
- The page is checked as Qt checks it. An invalid reply shows one of three
  texts: "Invalid archive page. Update the computer CLI and refresh." (version,
  shape, cursors, total or echo of the query), "Invalid archive window. Refresh
  the archive." (position or previous cursor) and "Invalid archived session
  identity. Refresh the archive." (state, name, lowercase hyphenated UUID or
  `archived_at` outside the range).

### Actions smoke run

`ZERUS_CLIENT_QUERY=actions-smoke=1` (hook in `src/ui/PerfHost.tsx`, beside the
perf hooks) checks the actions end to end without the real `hgs`. It starts only
when the native side reports replay mode (`ZERUS_CLIENT_REPLAY`), an explicit
`ZERUS_CLIENT_HGS` and `ZERUS_CLIENT_PERF` (any value but `auto`; `exit_app` is
allowed only in a measurement run); otherwise it logs why and does nothing. In
replay mode `run_action`, `inspect_session` and `session_reference` refuse unless
`ZERUS_CLIENT_HGS` is set ("Actions in replay mode need ZERUS_CLIENT_HGS."), so
a replay never drives the real CLI.

Put `core/tests/support/fake_hgs_actions.py` as `hgs` in a scratch folder with a
`config.json` beside it containing a `replies` list (the first entry whose
`match` is a prefix of the arguments answers; `reply` answers the rest):
`reference` gets a URI, `session-action` gets `echo_session_action` with status
`completed`, and every other call is a plain success (the write actions ignore
standard output, and exit 0 is a success for them; `echo_interrupt` parses
standard input and fails on writes that have none). The fake logs each call to
`log.ndjson` beside it. Run it only with the headless environment:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_actions.py $S/hgs
echo '{"reply":{"exit":0},"replies":[{"match":["reference"],"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=claude%2Fsample-project%2Freview"}},{"match":["session-action"],"exit":0,"stdout":"echo_session_action","receipt_status":"completed"},{"match":["archives"],"exit":0,"stdout":{"version":1,"from":0,"until":253402300800,"query":"","next_cursor":"","previous_cursor":"","total":1,"position":0,"sessions":[{"name":"claude/sample-project/archived","state":"archived","resumable":true,"archive_id":"00000000-0000-4000-8000-000000000001","archived_at":1700000000}]}},{"match":["inspect","claude/sample-project/archived"],"exit":0,"stdout":{}},{"match":["resume","claude/sample-project/archived"],"exit":0},{"match":["kill","claude/sample-project/archived"],"exit":0}]}' > $S/config.json
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=actions-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" src-tauri/target/release/zerus-client
cat $S/log.ndjson
```

The `archives` entry answers the Archive step with one archived row (the page
echoes the All dates range and an empty query); the `inspect` entry for that
row answers its details (an object with no run or conversation, as the row has
none). The `resume` and `kill` entries for that row answer the Restore and
Forget archived steps with a plain success.

The hook first switches to the Archive filter, waits for the page and logs
`actions-smoke archive-page: <status text> (<row count> rows)`. It then opens
the date panel while the list panel carries a transform (as the strip's peek
does) and logs `actions-smoke archive-date-panel: done (...)` when the panel is
in the page's body at the picker button and Escape closed it with the focus
back on the button. Over a hover-opened strip peek it then logs
`actions-smoke archive-date-panel-peek: done (...)` when Shift+Tab and Tab wrap
inside the panel, Escape with the focus outside it closes only the panel (the
peek and a selection stay), leaving the list with the panel open keeps the peek
(as Qt keeps a hover peek under a popup) and, once it closed, the next leave
closes the peek. It selects the archived row, waits up to 15 s for its
details and logs `actions-smoke archive-inspect: done <archive id>` (or
`failed`); the fake's `log.ndjson` then holds `archives --json` (with the page
query on standard input) and an `inspect NAME ... --archive ID` call. On the
same archived row it then runs the Restore session entry's path and logs
`actions-smoke restore: done (state filter now all)` (the Archive filter becomes
All, as the row is selected and followed; `log.ndjson` holds `resume NAME
--archive ID -d`), Forget archived session through the dialog's confirmation
path and logs `actions-smoke forget-archived: done` (`kill NAME --archive ID`),
and Clear archive: it takes the batch menu's entry, presses the confirmation
dialog's "Clear archive" button and logs `actions-smoke clear-archive: done
(...)` with the notices of the batch (a second `kill NAME --archive ID`), or
`failed (... is disabled)` when the entry is disabled, since the fake's row can
be forgotten. The fake keeps answering the
archived row; the hook shows the Archive filter again after the restore, as the
pages load only there, so each step finds the row after the archive reloads. It
returns to the All filter and the replay session. The hook selects `claude/sample-project/review` (its details come from the
replay's Activity state value or, as here, one `inspect` of the fake), first
marks it as read, marks all as read (logging `actions-smoke mark-all-as-read:
N marked`) and marks it as needing attention, through the marks store the menus
use (`actions-smoke mark-as-read: done`, `mark-needs-attention: done`; they run
no `hgs`), then copies the session reference, the connect command, the session name and the
folder path through the store the menus use (the first calls `reference`; the
fake's URI is the clipboard text), logging each as
`actions-smoke copy-<entry>: done <the exact clipboard text>`, then runs start
fresh (`resume NAME --fresh --expected-run-id RUN -d`), forget (`kill NAME`) and
forget binding (`session-action NAME --json`, answered with a completed
receipt) through `perform`, which the dialogs' confirmations call on a row that passed their
rechecks (the replay row is running and not an OpenCode binding, so `forget` and the binding
forget skip the recheck; `actions-smoke start-fresh: done`,
`forget: done`, `forget-binding: done`), then runs pause, resume, rename (to `claude/sample-project/review-renamed`) and archive,
then the end call the Terminate dialog picks for the row through the dialog's
confirmation (`kill`, since the replay row is not tracked), then the other end
call (`terminate`), once each through the store the menus use. It logs each
outcome to stderr (`zerus-client perf: actions-smoke ...`) and exits 0, or 1
when an outcome failed.

Then the batch steps, before the marks: `run("review_later", [row])` and
`run("mark_read", [row])` through the batch store the selection bar uses
(`actions-smoke batch-review-later: ...` and `batch-mark-read: ...` log the
notice text, or that the step was skipped because the action does not apply to
the replay row by Qt's rules), and Pause all and Resume all for the row's
computer, enabled by the same `machineEntries` rules as the batch menu and
started through `runMachine` (`actions-smoke pause-all: ...` and `resume-all:
...`; a started call puts `pause --all` or `resume --all -d` into `log.ndjson`).
A step the rules disable logs `skipped (...)` instead of being forced. In the
replay fixture the row already needs attention (Mark needs attention is
skipped), Pause all is disabled because some sessions are not idle, and Resume
all has no saved session, so only `batch-mark-read` runs; the two calls are
covered by the core's fake-hgs test.

The marks must survive a restart. Mark needs attention runs last of the three
so the first run ends with the session marked for review, and the replay keeps
its marks in `attention-marks-replay.json` in the app data directory. A second
run with `ZERUS_CLIENT_QUERY=actions-smoke=marks` (same command, the same
`XDG_DATA_HOME` or app data directory as the first) only waits for the replay
session and logs `actions-smoke marks-after-restart: review true` (exit 0) or
`review false` (exit 1) once the stored marks have applied. Give both runs one
scratch `XDG_DATA_HOME` so a smoke run never touches the real marks files.

## Screens

Overview, Projects, Machines and Accounts show live data and read only;
every write keeps its place, label and Qt enablement shape, disabled with the
phase that brings it ("Arrives with machine editing.", "... account
management.", "... project editing.", "... the New session dialog.", "... the
worktree flows.", "... the copy actions.").

| Data | Source |
| --- | --- |
| Machine cards (host, version, platform, metrics, console fields) | `catalog.meta`, projected in the core (`machine.rs`) and sent as `Batch.machines` |
| Connection state | `Batch.computers`: only `live` is fresh; `probing` and `reconnecting` connect (amber), `ended` is offline, `unsupported` reads "Update hgs" |
| Accounts | `hgs [@host] account ls` per host (every 30 s while Overview or Accounts shows, once per new host otherwise) and `account inspect ID --refresh` per installed profile (every 5 min, two at once, offline hosts skipped) |
| Machines | `hgs machine ls` on show and Refresh, `machine resolve ALIAS` on selection, `machine check ALIAS` only on Test connection |
| Projects | `hgs swarm get` every 5 s while Projects shows |

The reads run in `src-tauri/src/screens.rs` on the blocking pool with the
core's timeouts and output limits (`core/src/reads.rs`); when each read is
due is decided in `core/src/views/schedule.rs`, and the core projects every
reply into a view model (`core/src/views`). A changed view emits
`screens-changed` with the screen's name, and the frontend pulls it again.
In replay mode the reads refuse unless `ZERUS_CLIENT_HGS` is set ("Reads in
replay mode need ZERUS_CLIENT_HGS."). Peers without the event stream get no
polling fallback yet (see the phase table).

### Screens smoke run

`ZERUS_CLIENT_QUERY=screens-smoke=1` (hook in `src/ui/PerfHost.tsx`) shows
the four screens on the replay with a fake `hgs` answering the reads from
`core/tests/fixtures/views`, and exits 0 once each has loaded:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_reads.py $S/hgs; chmod +x $S/hgs
python3 - "$S" "$PWD/core/tests/fixtures/views" <<'EOF'
import json, sys
replies = {
    "account ls": {"file": "accounts-local.json"},
    "@mac account ls": {"file": "accounts-studio.json"},
    "account inspect claude-work --refresh": {"file": "inspect-claude-work.json"},
    "account inspect native-codex --refresh": {"file": "inspect-native-codex.json"},
    # The replay's other installed profiles have no fixture: their inspect fails.
    "account inspect native-claude --refresh": {"stderr": "no fixture", "exit": 1},
    "@mac account inspect claude-studio --refresh": {"stderr": "no fixture", "exit": 1},
    "@mac account inspect kimi-studio --refresh": {"stderr": "no fixture", "exit": 1},
    "machine ls": {"file": "machines.json"},
    "swarm get": {"file": "swarm.json"},
}
json.dump({"fixtures": sys.argv[2], "replies": replies}, open(sys.argv[1] + "/config.json", "w"))
EOF
cat > $S/run.sh <<'EOF'
src-tauri/target/release/zerus-client 2> "$S/app.log" &
app=$!
for page in overview machines accounts projects; do
  until grep -q "screens-smoke $page: shown" "$S/app.log" || ! kill -0 "$app" 2>/dev/null; do sleep 0.2; done
  sleep 1 # the frame after the page switch
  import -window root "$S/$page.png"
done
wait "$app"
EOF
env -u DISPLAY S="$S" WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_REPLAY="$PWD/fixtures/replay.ndjson:mac=$PWD/fixtures/replay-mac.ndjson" \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=screens-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" sh $S/run.sh
echo "exit $?"; grep screens-smoke $S/app.log
python3 -c 'import collections, json, sys; print(collections.Counter(" ".join(json.loads(l)["argv"]) for l in open(sys.argv[1])))' $S/log.ndjson
```

The screenshots stay in `$S`; they are never committed.

## Measurement run

`ZERUS_CLIENT_PERF=auto` runs the Diagnostics scenarios unattended (`src/perf`),
writes the report to `ZERUS_CLIENT_PERF_OUT` and exits; every stage goes to
stderr as `zerus-client perf: ...` (no session names). It picks a session whose
Activity has events and scrolls, bounds each scenario at 60 s and the run at
8 minutes, and ends with 10 s idle windows: as displayed, with Animate working
sessions off, with Reduce motion (both only tried, not stored), with the window
hidden and with it minimised (`perf_away`; a bare Xvfb has no window manager, so
there the window stays shown and the log says `minimized false`), each of the
last two followed by one as displayed again.
`ZERUS_CLIENT_PERF_LABEL` names the platform, `ZERUS_CLIENT_PERF_HZ`
overrides the display rate (`display_hz`), `ZERUS_CLIENT_PERF_REPRESENTATIVE=0`
marks Xvfb runs and `ZERUS_CLIENT_PERF_PREFERENCE=default` keeps WebKit's
near-60 fps preference on macOS and Linux (the control run). The page is cross-origin
isolated (COOP and COEP headers) for precise timers.

Screenshots of real sessions are never committed; the fixtures are synthetic
(example domains, `/home/example` paths).

## Data path

Rust reads every watch frame (`client/core`), keeps the catalog and the followed
Activity, and folds changes into one batch per display frame. The frontend calls
`next_batch` from `requestAnimationFrame` with one call in flight; the command
parks up to 250 ms while nothing is pending and answers with raw JSON bytes. A
batch is one reactive transaction: rows reconcile by `computer + "\n" + key`, so
unchanged rows keep their identity and DOM. Only the selected session's Activity
is followed (newest 100 events); scrolling to the top reads earlier events with
`after`.

While the window is hidden or minimised the pump asks for nothing and schedules
no frame; a batch that arrives meanwhile waits, the Rust side keeps folding
changes into the next one, and the first frame after the window returns applies
it. The Working clocks stop too, as Qt stops its timers when a window hides.

## Settings and motion

The Settings screen has, so far, the two motion settings of the Qt GUI with its
QSettings keys and defaults: Animate working sessions
(`workspace/animateActivity`, on) and Reduce motion (`workspace/reduceMotion`,
off). They are stored per user in the webview's local storage, one JSON object
under `zerus-client-settings` keyed by the QSettings names (`src/store/settings.ts`).
`src/ui/motion.ts` turns them into root attributes, and `src/ui/styles/motion.css`
lists every endless animation: Animate working sessions off stops the working
indicators (halos, the project count, status and typing dots), Reduce motion or
the system's reduced-motion preference stops all of them (also the Zerus mark,
shimmer and spinners) and makes entrances instant, and a hidden or minimised
window pauses everything. `src/ui/styles/motion.test.ts` checks that every
keyframe animates only transforms and opacity and that every endless animation
is in those lists.

## Renderer (Linux)

WebKitGTK picks its renderer when GTK starts, so `main` calls
`renderer::choose()` first, before Tauri, GTK or any thread (`src-tauri/src/renderer.rs`).
On NVIDIA Wayland the shared-memory renderer repaints the whole window every
frame (about 30% app CPU at idle with motion); DMA-BUF with
`__NV_DISABLE_EXPLICIT_SYNC=1` costs about 6%, and DMA-BUF without that variable
dies at start with Wayland Error 71. Evidence and the full policy:
`bench/results/spike-29-webkitgtk-idle-2026-10-28.md`.

`ZERUS_CLIENT_RENDERER` is the only switch; there is no setting.

- `auto` (default): with `WEBKIT_DISABLE_DMABUF_RENDERER` absent, NVIDIA Wayland gets
  `__NV_DISABLE_EXPLICIT_SYNC=1` (never overwritten if you set it) and everything else is
  left to WebKit. With the variable set to `0` it behaves as `dmabuf`; set to anything
  else it is respected and the client stays on shared memory ("set by the system").
- `dmabuf`: `WEBKIT_DISABLE_DMABUF_RENDERER=0`, plus the NVIDIA variable on NVIDIA Wayland.
- `shm`: `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

NVIDIA means `/proc/driver/nvidia/version`, a DRM card bound to the `nvidia` driver and
`nvidia_drm` modeset `Y`. Wayland is decided as GDK does: `GDK_BACKEND` unset or
`wayland*` and a compositor socket; `GDK_BACKEND=x11` changes nothing. An unknown value
counts as `auto`.

The DMA-BUF paths write a crash marker, `renderer-marker.json` (`renderer`, `driver`,
`written_at`), in the client's private data directory (next to the read marks, for
example `~/.local/share/dev.zerus.next/`), before GTK starts. The webview clears it
through `renderer_frame` after the second animation frame following load. A marker
found at start means the last DMA-BUF start never reached a frame: that start uses
shared memory and the marker stays. A marker of another driver version is dropped and
DMA-BUF is retried once. An early exit before the first frame keeps shared memory until
the driver changes; deleting `renderer-marker.json` in the app data folder retries DMA-BUF.
The decision (`renderer`, `reason`, `nvidia`, `wayland`,
`marker_state`) is logged at start and in the `renderer` field of `perf_config`.
A blank window is not detected. Known gap: on the GL path a minimised window keeps painting.

## Rules

- The UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5,
  U+2027, U+2981, U+30FB): use layout, commas or line breaks.
- Motion uses transform and opacity only and honours Reduce motion, Animate
  working sessions and the system's reduced-motion preference (`motion.css`).
- `src/ui/lib/os-icons.ts` embeds two Tabler Icons outlines (MIT, see
  `licenses/`); `src/ui/lib/icons.generated.ts` is markup generated from
  `lucide-solid` (ISC).
