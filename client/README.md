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

The session menus and the Rename and Terminate dialogs run guarded actions
through the `hgs` CLI (`core/src/actions.rs`, `src-tauri/src/actions.rs`),
with Qt's enablement, confirmations and texts (`actions-ui.ts`). A peer's call
is prefixed with `@alias` and uses the longer timeout. Every call is made with
the child's own environment: the agent attribution (`HGS_SESSION`,
`HGS_RUN_ID`) is never passed on.

| Action | Arguments (`NAME` is the session name) | Standard input | Timeout here, peer |
| --- | --- | --- | --- |
| Stop (interrupt) | `interrupt NAME --json` | JSON: `request_id`, `expected_run_id`, `expected_conversation_id`, `expected_turn_started` | 15 s, 45 s |
| Mark as read | `read NAME --json` | JSON: `run_id`, `conversation_id`, `reply_id`, `attention_signature` | 15 s, 35 s |
| Pause | `pause NAME` | none | 45 s, 45 s |
| Resume | `resume NAME -d` | none | 30 s, 30 s |
| Rename | `rename NAME NEW [--archive ID]` | none | 5 s, 15 s |
| Move to archive | `archive NAME` | none | 30 s, 45 s |
| Terminate | `terminate NAME --expected-run-id RUN` | none | 45 s, 60 s |

Outcome rules:

- A call is made once and never repeated. A timeout, a failed hand-over or an
  abnormal end is an unknown outcome: it is shown as a failure ("Request timed
  out. Check Terminal before retrying.", "Transfer failed. Check Terminal
  before retrying.") and is never replayed. Unknown outcomes are never replayed.
- Stop succeeds only when the receipt echoes the request id, session name, run
  id and conversation id of the call (status `submitted`); anything else is
  "Invalid acknowledgement. Check Terminal before retrying."
- Pause, Resume and Rename succeed on exit 0. Move to archive and Terminate also
  read the single worktree report line on standard error: a removed worktree
  gives an information notice, a kept or unconfirmed one a sticky warning with
  its reason. Otherwise a failure shows the trimmed standard error.
- One action per session at a time, for every kind. A second request while one
  runs (or one for a row whose run id changed) is a silent no-op, answered
  `busy` by the Rust side; a changed row shows "This session changed. Open its
  actions again."
- Details (allowed actions and their reasons, `auth_required`) come from one
  read-only `inspect` per selected session, refetched when its run, conversation
  or state changes.
- Mark as read applies at once. The mark is saved before the call in
  `read-marks.json` in the app data directory (`app_data_dir`, e.g.
  `~/.local/share/<bundle id>/` on Linux) so a restart keeps it, and is dropped
  once the row shows the host has settled it. While a mark is pending and a
  batch shows its row unread again for the same reply and attention signature,
  the call is retried quietly, at most once per 5 s per session.
  A host without shared read state keeps the entry disabled.

Forgetting, restoring, attention marks and sending arrive in later phases; their
entries are disabled with a reason.

### Actions smoke run

`ZERUS_CLIENT_QUERY=actions-smoke=1` (hook in `src/ui/PerfHost.tsx`, beside the
perf hooks) checks the actions end to end without the real `hgs`. Use a fake
`hgs` (`core/tests/support/fake_hgs_actions.py` copied as `hgs`, with a
`config.json` beside it, `{"reply":{"exit":0}}` (exit 0 with no output is a success for these writes);
it logs each call to `log.ndjson`) as `ZERUS_CLIENT_HGS`, and the replay
fixtures as the catalog. The hook selects `claude/sample-project/review`, runs
pause, resume, rename (to `claude/sample-project/review-renamed`), archive and
terminate once each through the same `perform` the menus use, logs each outcome
to stderr (`zerus-client perf: actions-smoke ...`) and exits 0, or 1 when an
outcome failed. `ZERUS_CLIENT_PERF` must be set to something other than `auto`
(for example `smoke`), because `exit_app` is allowed only in a measurement run.
Run it only with the headless environment above.

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
