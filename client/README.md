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
npm test                       # Vitest: stores, transcript
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
| Rename | `rename NAME NEW [--archive ID]` | none | 5 s, 15 s |
| Move to archive | `archive NAME` | none | 30 s, 45 s |
| Terminate a tracked Codex, Claude or Kimi session | `terminate NAME --expected-run-id RUN` | none | 45 s, 60 s |
| Terminate any other live session | `kill NAME` | none | 5 s, 15 s |

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
- The menu's Mark as read still sends the reply and attention it showed with
  one `hgs read` call; if a newer reply or question arrived meanwhile, nothing
  runs and the notice says "A new reply or question arrived. Open the menu
  again to mark it as read." The call saves its mark among the pending ones
  first. A host without shared read state keeps the entry disabled.
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

- Mark as read, Mark needs attention and Mark all as read are local first:
  they change this device's attention marks and read replies at once
  (`attention-marks.json`, or `attention-marks-replay.json` in a replay; at
  most 20 000 entries per map, the oldest written dropped first, where Qt keeps
  them unbounded). A host that shares read state also gets a queued read mark
  (`read-marks.json`), which the Rust worker delivers with Qt's ReadMarkSync
  rules: at most 4 calls at once, the computer with the fewest first, backoff
  from 10 s to 300 s, a resend after 120 s and, while only part of a mark is
  applied, up to 960 s. Marks for computers no longer configured are dropped,
  and a delivery shows no notice. In the overlay a pending mark counts as read
  for the same run and conversation, so the row changes before the host
  answers; a newer reply or question shows again.

Forgetting and restoring arrive in later phases; their entries are disabled
with a reason. Sending, the Mark read chip of the composer toolbar and the
Open terminal entries arrive with phase 4. The copy entries below are live.

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
`config.json` beside it containing
`{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=claude%2Fsample-project%2Freview"}}}`
(`reference` gets that URI; the write actions ignore standard output, and exit 0
is a success for them; `echo_interrupt` parses standard input and fails on
writes that have none). The fake logs each call to
`log.ndjson` beside it. Run it only with the headless environment:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_actions.py $S/hgs
echo '{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=claude%2Fsample-project%2Freview"}}}' > $S/config.json
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=actions-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" src-tauri/target/release/zerus-client
cat $S/log.ndjson
```

The hook selects `claude/sample-project/review` (its details come from the
replay's Activity state value or, as here, one `inspect` of the fake), first
marks it as read, marks all as read (logging `actions-smoke mark-all-as-read:
N marked`) and marks it as needing attention, through the marks store the menus
use (`actions-smoke mark-as-read: done`, `mark-needs-attention: done`; they run
no `hgs`), then copies the session reference, the connect command, the session name and the
folder path through the store the menus use (the first calls `reference`; the
fake's URI is the clipboard text), logging each as
`actions-smoke copy-<entry>: done <the exact clipboard text>`, then runs pause, resume, rename (to `claude/sample-project/review-renamed`) and archive,
then the end call the Terminate dialog picks for the row through the dialog's
confirmation (`kill`, since the replay row is not tracked), then the other end
call (`terminate`), once each through the store the menus use. It logs each
outcome to stderr (`zerus-client perf: actions-smoke ...`) and exits 0, or 1
when an outcome failed.

The marks must survive a restart. Mark needs attention runs last of the three
so the first run ends with the session marked for review, and the replay keeps
its marks in `attention-marks-replay.json` in the app data directory. A second
run with `ZERUS_CLIENT_QUERY=actions-smoke=marks` (same command, the same
`XDG_DATA_HOME` or app data directory as the first) only waits for the replay
session and logs `actions-smoke marks-after-restart: review true` (exit 0) or
`review false` (exit 1) once the stored marks have applied. Give both runs one
scratch `XDG_DATA_HOME` so a smoke run never touches the real marks files.

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
near-60 fps preference on macOS (the control run). The page is cross-origin
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

## Rules

- The UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5,
  U+2027, U+2981, U+30FB): use layout, commas or line breaks.
- Motion uses transform and opacity only and honours Reduce motion, Animate
  working sessions and the system's reduced-motion preference (`motion.css`).
- `src/ui/lib/os-icons.ts` embeds two Tabler Icons outlines (MIT, see
  `licenses/`); `src/ui/lib/icons.generated.ts` is markup generated from
  `lucide-solid` (ISC).
