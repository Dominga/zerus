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
| `ZERUS_CLIENT_HGS` | The `hgs` to run (default: `hgs` on `PATH`) |
| `ZERUS_CLIENT_REPLAY` | Replay synthetic watch streams instead of running `hgs`: a path list (`:` on Unix), the first file is this computer, `alias=file` entries are peers, e.g. `fixtures/replay.ndjson:mac=fixtures/replay-mac.ndjson` |
| `ZERUS_CLIENT_REPLAY_INTERVAL_MS` | The pause per `heartbeat` frame of a replay (default 900) |
| `ZERUS_CLIENT_QUERY` | URL parameters for the window, e.g. `select=claude/sample-project/api-refactor` opens that session |

A build without the `live` feature (`--no-default-features`) has no data source;
the frontend then uses its built-in replay.

## Headless runs

Development windows never open on a desktop someone is using. On Linux run the
app inside Xvfb with GTK forced to X11 and the Wayland socket name pointing
nowhere. Unsetting `WAYLAND_DISPLAY` is not enough: GTK then tries the default
socket `wayland-0` and the window opens on the desktop while Xvfb stays empty.

```sh
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  xvfb-run -a -s "-screen 0 1440x900x24" \
  sh -c 'RUSTUP_TOOLCHAIN=1.99.1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson npx tauri dev'
```

Without `WEBKIT_DISABLE_DMABUF_RENDERER=1` WebKitGTK paints a black window in
Xvfb.

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

## Rules

- The UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5,
  U+2027, U+2981, U+30FB): use layout, commas or line breaks.
- Motion uses transform and opacity only and honours reduced motion.
- `src/ui/lib/os-icons.ts` embeds two Tabler Icons outlines (MIT, see
  `licenses/`); `src/ui/lib/icons.generated.ts` is markup generated from
  `lucide-solid` (ISC).
