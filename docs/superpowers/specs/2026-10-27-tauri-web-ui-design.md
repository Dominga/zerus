# Tauri Web Desktop Client Design

Tracking epic: `zerus-5wmh`. Status: **draft**. The owner reviews the clickable
design mockup (`design/webui/`) first; no client implementation starts before
that design is approved. This document records the architecture the mockup
assumes, so review feedback can change it before code exists.

## Goal

A new desktop client on Tauri 2 that grows beside the Qt GUI, screen by screen,
until it reaches functional parity and the Qt client can retire. The owner's
priorities, 2026-10-27: fast, smooth, GPU-composited rendering, and a skin system
from day one.

The client consumes the event data plane designed in
`2026-10-27-event-data-plane-design.md` (branch `work-ba0ed9fd`). The data plane
fixes the visible lag; this client fixes rendering, motion and how quickly new
design work reaches the product.

## Owner constraints (2026-10-27)

| Topic | Decision |
| --- | --- |
| Visual base | The WhiteWh fork prototype's *base style* only: palette, fonts, radii, density, motion. None of its specific solutions or runtime |
| Session list | Today's Zerus structure, sizes and cards. Only the prototype's nested tree lines are borrowed, for the existing one-level subagent nesting |
| Parity | Every function of the current Qt GUI, including those the prototype lacks |
| Orchestration | No fork orchestrator (teams, phases, task graph, iron rules, cache hold, designer). Zerus Tasks are redesigned in the new style |
| Navigation | The main screen switcher stays on the side |
| Scope | No new features from the prototype. The skin system is the one requested addition |
| Process | A mocked-data design is reviewed and approved before implementation |
| Base build | QtWebEngine stays out (`2026-10-13-native-ui-plugin-design.md`); Tauri uses system webviews |
| Refresh rate (round 2) | On macOS, scrolling, animations and `requestAnimationFrame` run at the display's refresh rate (120 Hz on ProMotion), not 60. This is a pass/fail criterion of the measurement |
| Row density (round 2) | The compact 94 px card is the only density; the non-compact 104 px card and its setting are dropped |
| Icons (round 2) | Zerus's own glyphs from `tray/src/WorkspaceIcons.h`, the provider glyphs from `IdentityBadge.h` and the status glyphs from `SessionStatusBadge.h`; a substitute icon set is used only where Qt has no glyph |
| Side panel (round 2) | The prototype's Environment/Context card styling (raised cards, radii, collapsible blocks, show/hide motion) with today's content only |
| macOS refresh fix (round 3) | Decided: disable WebKit's near-60 fps page rendering preference before each UI webview is created; if the measurement fails the criteria or the preference disappears, evaluate Chromium on macOS |
| Account type (round 3) | Every usage limit in the status bar names its account type: the provider badge and the plan when known, next to the numbers |
| Resizing (round 3) | The composer height (76-720 px) has a visible grip; the side panel width is draggable (default 400, at least 240) and remembered |

Non-goals: replacing the Qt client in one step; changing `hgs` CLI, API v1,
state paths or status semantics; plugins (a separate design, see the 2026-10-27
UI review question 3); a mobile web client.

## Process model

```
Tauri process (Rust)
  core: per-computer stores, view-model projection, actions, platform modules
  ├─ child: hgs watch --json            (local computer)
  ├─ child: hgs @alias watch --json     (one per peer, over SSH)
  ├─ child: hgs … --json                (bounded reads and guarded actions)
  ├─ PTY:   hgs a / ssh attach client   (embedded Terminal tab)
  └─ main webview (SolidJS app)  ⇄  IPC: commands + one Channel per window
     optional child webview: Native UI (DeepSeek) at its tunnel URL
```

- **One contract with the core: the `hgs` CLI.** The client spawns
  `hgs [@host] watch --json` per computer and the existing `--json` commands for
  bounded reads and actions, exactly as the Qt client and the mobile connector
  do. Remote computers keep working through `hgs @alias`. The shared serde crate
  `crates/watch-protocol` provides frame types; it has no Loro or Qt dependency.
  Linking CLI internals directly is not part of v1.
- **Parsing never happens in the webview.** Rust reads NDJSON, applies frames to
  a per-computer store and projects screen view models: list rows with their
  derived presentation status, header counters, Activity items with rendered
  Markdown (`pulldown-cmark`, sanitized with `ammonia`), Processes rows.
- **Batched delivery.** Every Tauri `Channel::send` evaluates JavaScript on the
  main thread and has no back-pressure (`tauri` 2.12 `ipc/channel.rs`). The core
  keeps one delta buffer per window and flushes at most once per frame (16 ms),
  with one batch in flight: the webview acknowledges each batch with a cheap
  command and the core merges further deltas meanwhile (latest value wins).
  Compact batches stay under 8 KiB for the direct-eval path; larger payloads go
  as raw bytes or a pull command returning `ipc::Response`.
- **One reactive transaction per batch** in the webview (`batch` plus `reconcile`
  keyed by row identity).
- **Closing the window never stops agents.** The Rust process keeps the tray,
  notifications and stores alive; it never owns agent processes or tmux servers.

### Stream consumption (agreed with the events session, 2026-10-27)

The server and the Qt consumer are on main (PRs #368, #369 and #370); frame
types live in `crates/watch-protocol` (`Frame { v, seq, body }` with the body
tagged by `t`; `Control` tagged by `op`), documented in `docs/reference.md`,
"Event stream". The web client follows the same rules as the Qt consumer.

- Capability: `Event stream ABI: watch-v1` in `hgs -h`. Older peers fall back to
  today's polling (`hgs @alias ls --json`, `inspect --after`) on the same cadence
  as the Qt client. The client keeps stdin open (EOF ends the stream) and uses
  `{"op":"ping","id":N}` / `pong` for liveness checks.
- One stream per computer whose CLI prints the marker. While it is live the
  client stops polling that computer's listing and follows the selected
  session's Activity through it. A stream that fails or stays silent for 30 s
  restarts after 5 s, 15 s, 60 s and then every 5 minutes, and the client polls
  meanwhile. The existing `ZERUS_EVENT_STREAM=0` and `dataPlane/eventStream=false`
  switches keep it on polling.
- The server re-lists all rows every 5 s as a safety net (owner decision,
  2026-10-27), so the client needs no listing poll of its own while live.
- Frames are at most 1 MiB with one `seq` per stream; a `gap` is per topic and
  key and is followed by a bounded snapshot.
- `catalog`: whole-row upserts keyed `run:<run_id>` (`name:<name>` without a
  run). A rename is an upsert on the same key, so selection, drafts and scroll
  survive. A new run sends `remove{key, reason: "replaced"}` plus an upsert; the
  client carries selection and drafts across by name. Removal reasons: ended,
  archived, forgotten, replaced. Live rows only; Archive stays a keyset-paged
  read. Top-level listing fields arrive as `catalog.meta` (metrics and presence
  at most every 10 s).
- `activity:<key>` uses the catalog key. A subscribe without `after` returns an
  `events` frame with the newest `tail` (at most 100); with `after` it returns the
  events after that journal sequence, at most 200 per read. On `gap` (reason
  retention, reset or overflow) a `snapshot` replaces the window. DeepSeek and
  OpenCode sessions always receive window-replacing `snapshot` frames.
- **Text between tools in v1** arrives in activity `state` frames (the
  `inspect --skip-processes` summary without events, cursor and process work),
  re-read every 2 s while the agent is busy, every 15 s when idle and within
  500 ms of a journal or catalog change. `events` stay append-only. A
  replace-by-id `event` frame (`{id, rev, event}`, higher `rev` wins) is defined
  for later versions and handled by the client when it appears.
- Errors: `subscription_ended` (the session left the catalog), `session_changed`,
  `too_many_subscriptions` (at most 16), `unsupported_topic`, `bad_request`,
  `row_too_large`, `meta_too_large`.
- `git` (PR #372, in review): `{"op":"subscribe","topic":"git","path":"/abs/folder"}`
  and the matching `unsubscribe`. The reply is a `state` frame keyed by the path
  as subscribed whose `value` is exactly the `hgs git-status --json` object. It
  is sent at once, then only when the object changes (ignoring `sampled_at` and
  `remote_age_seconds`). The server re-checks a folder after Git metadata changes
  (HEAD, index, refs, packed-refs, FETCH_HEAD), after file-changing tool calls in
  sessions under it, when its 60 s remote check expires, and at least every
  30 s. `hello.limits.git_subscriptions` is 64; an older server's hello decodes
  as 0, and the client then keeps polling `git-status`. Non-fatal errors:
  `bad_request` (missing, relative or oversized path), `too_many_subscriptions`
  (65th folder), `git_status_failed` (at most once a minute per folder). The
  client subscribes only to folders on screen (the selected session's checkout
  and the Worktrees panel). Like the Qt client in PR #372, it re-dates a
  streamed value every 15 s while subscribed (adds the elapsed time to
  `remote_age_seconds`), so freshness badges do not lapse while the stream
  vouches for them.
- `processes:<session>` and `tasks:<project>` answer `unsupported_topic`; the
  client keeps today's bounded polled reads for them (Processes keeps its own
  interval), and account usage stays polled.
- Connection state per computer comes from hello, the 10 s heartbeat, 30 s of
  silence and gaps.
- Volatile fields (`goal_observed_at`, `process_work.sampled_at`) are masked by
  the server for change detection; the client never animates on them.
- Measured on arch: hello 407 B, catalog snapshot 291 KB for 35 rows, deltas of
  6-12 KB per changed row. Raw rows therefore never go to the webview as is: Rust
  projects them into compact view rows, so a typical batch stays under the 8 KiB
  direct-eval path.

### Actions

Actions keep today's guarded CLI calls and their rules: request identities,
expected run and conversation, destructive confirmations with the safe choice
focused, and **unknown outcomes are never replayed automatically**. The client
adds no new action semantics.

## Frontend

- **SolidJS 1.9 + TypeScript + Vite.** Fine-grained signals update only the DOM
  bound to a changed row; `createStore` plus `reconcile` applies whole-row
  upserts. In js-framework-benchmark (Chrome 156, 2026-10-25) Solid and Svelte 5
  stay within about 5-15% of vanilla JS on keyed operations, while React 19 is
  about 7x slower on swaps and about 11x larger compressed. Svelte 5 is an equal
  alternative; Solid is chosen for size and the reconcile fit. Solid 2.0 is not
  adopted before it is final.
- **Lists are virtualized with exact row heights** (`@tanstack/solid-virtual`).
  The session list keeps the Qt delegate's compact sizes: card 94 px, project
  header 40, subagent row 64 with a 24 px indent, collapsed row 40; the strip
  is 98 px with 84 px square tiles. No DOM measurement on the hot path.
- **Rows are bound to their identity.** The virtualizer's items are one store
  array reconciled by index, so the list iterates stable row ids and looks up
  each row's offset: a catalog delta updates bound text in place and never
  rebuilds a row. Rebuilding rows on every delta made the round 1 mockup flicker
  on hover and restarted badge animations. Scroll containers reserve their
  scroll-bar gutter; hover changes only colours, never layout.
- **Panel modes follow `SessionPanelDock`.** Docked (resizable splitter, minimum
  270, default 310), Collapsed (the strip) and Peek (the panel over the
  conversation). Hover opens Peek after 150 ms of rest and closes it after
  350 ms outside; menus and an active search keep it open. The panel moves by
  transform only (220 ms, OutCubic) and the conversation resizes once.
- **Activity column**: 900 px at 100 % content scale (480-2400), both edges drag
  together, double-click resets, Full-width Activity and Reset Activity width in
  the context menu; the composer, questions and banners share the column.
- **Zerus mark**: tinted with the attention colour and turning while any session
  needs attention, as `SwarmButton` does. The mark is one layer promoted ahead
  of time and animated by transform only, with no glow; it is paused while the
  window is hidden and static under Reduce motion. When attention clears, the
  current cycle finishes and the mark settles. The default motion spins up from
  rest to two turns and settles with a 6° overshoot; the owner chooses between
  it, a stronger three-turn spin-up and the Qt turn.
- **Session rows**: the emphasis edge is 4 px, 6 px from the card's top and
  bottom and 4 px in the compact row. Switching a card and its compact row
  morphs in 220 ms: the card's height eases, the title and badges move from
  their old place by transform, the detail lines fade and the rows below slide
  with the same timing, so the title never jumps.
- **Session header**: agent and machine badges, then the git branch as a
  labelled chip with the branch glyph, then folder, worktree and model.
- **Media messages**: the voice player and the round video note both keep the
  transcript button on the right; the video note plays in place with a progress
  ring. The voice/video mode switch is always beside the record button and is
  locked while recording, as in Qt.
- **Motion** uses only opacity and transforms, honors `prefers-reduced-motion`
  and the existing Reduce motion and Animate working sessions settings. The one
  exception is the compact-row morph, which eases a single card's height for
  220 ms inside a layout-contained row.
- **Heavy parts load on demand**: xterm.js when Terminal first opens.
- **Embedded Terminal**: xterm.js 6 (DOM renderer by default on Linux, WebGL
  addon on macOS and Windows with context-loss handling); PTY in Rust with
  `portable-pty` (Unix PTY and ConPTY with a kill-on-close Job Object), output
  coalesced per frame into byte chunks.
- **i18n**: the existing Russian translation moves to a frontend catalog; tray and
  menu strings are translated in Rust.
- **Accessibility**: the bounded accessible card text, keyboard-modality focus
  ring and every keyboard rule of the inventory carry over.

The mockup under `design/webui/` is written on this stack. After approval its
components move into the client package; its mock data is replaced by the store.

## Skin format v1

One JSON document per skin, independent of the UI stack.

```json
{
  "schema": 1,
  "id": "fjord",
  "name": "Fjord",
  "appearance": "dark",
  "extends": "zerus-dark",
  "tokens": { "color": { "accent": "#79b8ff" }, "radius": { "card": 10 } },
  "components": { "status-badge": "dot", "agent-mark": "glyph" },
  "layout": { "rail": "labeled", "activity-column": "full" }
}
```

- **L0 tokens.** Groups `color`, `agent`, `list` (the session list's badge,
  edge, tile frame, Git tone and counter colours, taken from the Qt delegate so
  cards keep today's look in every skin), `entity` (machine and project palette
  plus the fill and ink mix amounts that reproduce `MachineAppearance`), `font`
  (UI and mono stacks, base size, line height), `radius`, `density`, `motion`
  (durations, easing, pulse) and `shadow`. The base skin (`zerus-dark`) defines
  the complete token set; any other skin may only override existing tokens.
- **L1 components.** Enumerated variants of existing components only:
  `status-badge` (pill, dot), `agent-mark` (chip, glyph), `tree-lines` (lines,
  indent), `attention-edge` (bar, tint).
- **L2 layout.** Enumerated layout variants only: `rail` (icons, labeled),
  `sessions-pane` (left, right), `inspector` (docked, collapsed),
  `activity-column` (centered, full). The rail always stays on the side.
- **L3 functions are never skins.**
- **Validation.** Values are type-checked per token (hex or `rgb()/rgba()`
  colors, `{group.token}` references, bounded numbers, font family names,
  easing keywords or `cubic-bezier`, restricted shadow grammar). Unknown keys,
  unknown variants and `extends` cycles are rejected. **No CSS, selectors, URLs
  or code**, so a skin cannot break layout or rendering performance.
- **Compilation.** The web client compiles a resolved skin to one rule of CSS
  custom properties (`--z-<group>-<token>`) and sets variant attributes on the
  root element; components carry the variant styles. Generators for Qt tokens,
  Compose and SwiftUI can read the same JSON later.
- **User skins** live in a skins folder in the client's configuration directory
  and can be imported from Settings → Appearance; an invalid file is refused
  with the validator's message. The fork's `WorkspaceSkin.h` schema 1 (11 tokens
  per theme) is to be read as a subset once its key list is mapped; its free QSS
  is ignored.
- **Theme** System, Dark and Light selects a dark or light skin, with System
  following the OS appearance.

## Platform integrations

| Integration | Mechanism |
| --- | --- |
| Tray icon with counters, tooltip, menu with machines and sessions, Open in ▸ | `tauri` `tray-icon` and `menu`; icon rendered in Rust (two counters, stale dot, "?" on error); no update while the menu is open. **Linux:** Tauri 2's appindicator tray reports no clicks, so a custom StatusNotifierItem (`ksni`/`zbus`) restores primary-click activation on KDE |
| Notifications with per-conversation tokens, withdraw, actions, foreground banners | Custom module: `zbus` `org.freedesktop.Notifications` with actions and activation tokens; `UNUserNotificationCenter` on macOS; WinRT toasts with an AppUserModelID on Windows. `tauri-plugin-notification` 2.7 covers posting only |
| Dock and taskbar badge | `set_badge_count`/`set_badge_label` on macOS, `set_overlay_icon` on Windows; on Linux emit `com.canonical.Unity.LauncherEntry` directly (Tauri's path needs libunity, AUR-only on Arch) |
| Presence (idle, lock) | Rust: CoreGraphics on macOS; `zbus` ScreenSaver/Mutter and `x11rb` on Linux; written through `hgs presence` |
| Keep above other windows | `set_always_on_top`; KDE Wayland through KWin scripts over `zbus`, as today |
| Terminal launch (Konsole, xdg-terminal-exec, Terminal.app, Windows Terminal) | Rust module porting today's backends |
| Embedded Terminal PTY | `portable-pty` (see Frontend) |
| Voice and video recording | Native capture in Rust (`cpal`, platform camera APIs) first; `getUserMedia` only after per-engine permission and `MediaRecorder` format checks |
| Main-thread watchdog, termination signals, Windows restart registration | Rust thread pinging the event loop; `signal-hook`; `RegisterApplicationRestart`; crash restart stays with systemd/launchd |
| Single instance with service handoff | `tauri-plugin-single-instance` plus the existing wait-for-service logic |
| Autostart and services | Installer-managed units as today (`hgs-tray.service`, LaunchAgent); not `tauri-plugin-autostart` |
| Updates and restart | Existing catalog logic in Rust; `tauri-plugin-updater` stays off for AUR, Homebrew and winget builds (its bundle-type marker would make repackaged builds update themselves) |
| Clipboard, file pickers, opening files | `tauri-plugin-clipboard-manager`, `-dialog`, `-opener`; Klipper/`wl-copy` fallback, quarantine and Zone.Identifier markers in Rust |
| Native UI (DeepSeek) | Child webview at the tunnel URL, or the system browser through `hgs native-ui` |
| Mute, read marks, automatic approvals, worktree policy, swarm, speech providers | Stay in `hgs`; the client calls them |

## Packaging and signing

- **macOS:** `.app`/`.dmg`, Developer ID signing and notarization through
  `tauri build`; hardened runtime on. WKWebView runs JavaScript out of process,
  so no JIT entitlement is expected; verify by notarizing a build without
  entitlements. A distinct bundle identifier from the Qt app.
- **Linux:** system `webkit2gtk-4.1` for Arch and source packages (no AppImage on
  Arch); `.deb`/`.rpm` from the bundler for other distributions.
- **Windows:** NSIS per-user installer or the existing portable zip layout with
  the WebView2 bootstrapper; Authenticode through `signCommand`.
- Binary names and paths must not collide with `hgs-tray` while both clients
  exist. `hgs`, `HGS_*`, state and config paths stay unchanged; GUI settings move
  from QSettings keys (listed in the parity inventory) through a one-time import.

## Risks and measurement plan

- **Linux (main risk).** Tauri 2 uses WebKitGTK through webkit2gtk-4.1 (GTK 3).
  WebKitGTK 2.54 moved to a Skia compositor with damage tracking, but NVIDIA
  blank windows and crashes, silent software WebGL and GTK 3's integer scaling on
  KDE fractional scales remain. Fallbacks in order: the Tauri 3 CEF runtime
  (GTK 4, Chromium; about +120 MB; alpha today) for Linux only, then Electron.
- **macOS refresh rate (hard requirement; the owner chose this fix on
  2026-10-27).** WKWebView keeps page rendering
  updates near 60 fps through the internal preference
  `PreferPageRenderingUpdatesNear60FPSEnabled`, on by default in current WebKit
  including macOS 26 and 27 (WebKit bug 294338; WebKit's animation-frame-rate
  explainer cites power use). Scrolling of painted content and accelerated
  (transform and opacity) CSS animations already run at the display rate;
  `requestAnimationFrame`, JS-driven drag and resize, newly painted virtual rows,
  xterm.js and non-accelerated animations stay near 60. There is no public API.
  The plan:
  - Create each window that shows our own UI with a `WKWebViewConfiguration`
    whose preferences have that key disabled through the WebKit SPI
    `+[WKPreferences _features]` and `-[WKPreferences _setEnabled:forFeature:]`
    (macOS 13.3+; the Zerus floor is 14), passed in before creation with
    `WebviewWindowBuilder::with_webview_configuration` (Tauri 2.8+, wry 0.53+).
    Toggling a live page is unreliable. Feature-detect the selectors and the
    key, read the value back, log a failure and keep running; never panic
    across the Objective-C boundary. Leave third-party pages at the default.
  - Developer ID builds with notarization may call the SPI; the Mac App Store
    would reject it and is not a Zerus channel. `macOSPrivateApi` does not
    affect the frame rate.
  - Animate only transform and opacity, so scrolling and motion stay at the
    display rate if WebKit removes or renames the key (it was renamed once, in
    2021). Ship a frame-rate self-check in diagnostics.
  - Fallback if the SPI fails the criteria below or disappears: Chromium on
    macOS, which paces rAF and compositing from the display link. Prefer the
    Tauri 3 CEF runtime once it is beta and within one Chrome major (alpha
    today, pinned to Chromium 154 against Chrome 156); otherwise Electron. Either
    adds roughly 200-300 MB per install, several times the memory, helper apps
    to sign and Chromium security releases we ship ourselves; Electron also
    moves the Rust core into a sidecar.
- **Measurement (two weeks, one heavy screen: session list plus long Activity,
  on the same data plane):** p95 frame time while scrolling with a trackpad, input
  to frame latency, session open time, idle CPU and RSS; Channel throughput at
  1k, 5k and 20k updates per second batched versus naive; a 20k-50k session list;
  xterm.js WebGL versus DOM. Devices: the owner's Mac (ProMotion) and Arch with
  KDE Plasma at 100%, 125% and 150% scale, NVIDIA and integrated GPUs; WebView2
  on Windows for IPC p99. Decision by numbers against the tuned Qt client.
- **macOS pass/fail: frame pacing equals the display refresh rate.** On the
  owner's ProMotion Mac, on AC power with Low Power Mode off and a nominal
  thermal state, the window fully visible, a release build, a cross-origin
  isolated page; three 10 s runs after a 2 s warm-up, worst run counts;
  T = 1000 / `NSScreen.maximumFramesPerSecond` (8.33 ms at 120 Hz). All must pass:
  1. rAF on a light page and during a CSS transform animation: mean interval
     within T ± 1%, median within T ± 0.5 ms, at least 99% of intervals at most
     1.5 T, at most 0.2% above 2.5 T, under 1% in the 15.0-18.5 ms band.
  2. A window capture shows at least 117.6 changed frames per second, with at
     most 1% of gaps above 1.5 T, for a transform animation, for a
     non-accelerated animation (the real test of the SPI) and for smooth and
     synthetic trackpad scrolling of a long virtualized list; rAF and scroll
     events during the scroll meet criterion 1.
  3. Validity: a native display-link counter in the same window reads at least
     117.6 Hz, and a run with the preference left at its default fails
     criterion 1; otherwise the harness is invalid. Power use with the
     preference on and off is reported, not gated.
  The same harness qualifies the Chromium fallback.

## Migration

The new client ships beside Qt as a separate application. Screens move in this
order once the measurement passes: Overview, Projects, Machines, Accounts and
Settings; then the session list, Activity, the composer and Tasks; platform
integrations move with the screen that needs them. The Qt client stays the
working client until the new one covers the parity checklist; installers keep
installing Qt until the owner switches the default.

## Open questions for the owner

1. Approve the round 3 design, or what to change; which Zerus mark motion to keep.
2. Decided 2026-10-27: the WebKit preference fix with the criteria above,
   falling back to Chromium on macOS (CEF once beta, otherwise Electron).
3. Linux runtime: start on WebKitGTK and measure, with CEF as the fallback
   (pending since round 1).
4. User-imported skins in v1, or built-in skins first (pending since round 1).
