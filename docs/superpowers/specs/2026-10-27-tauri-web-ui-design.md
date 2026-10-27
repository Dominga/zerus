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

The server side is PR #368 (branch `work-ba0ed9fd-watch`); frame types live in
`crates/watch-protocol` (`Frame { v, seq, body }` with the body tagged by `t`;
`Control` tagged by `op`), documented in `docs/reference.md`, "Event stream".

- Capability: `Event stream ABI: watch-v1` in `hgs -h`. Older peers fall back to
  today's polling (`hgs @alias ls --json`, `inspect --after`) on the same cadence
  as the Qt client. The client keeps stdin open (EOF ends the stream) and uses
  `{"op":"ping","id":N}` / `pong` for liveness checks.
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
- v1 topics are `catalog`, `catalog.meta` and `activity`. `processes:<session>`,
  `git:<path>` and `tasks:<project>` answer `unsupported_topic` until the next
  milestone; until then the client keeps today's bounded polled reads for them,
  and account usage stays polled.
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
  The session list keeps the Qt delegate's sizes: card 104 px (94 compact),
  project header 40, subagent row 64 with a 24 px indent, compact row 40; the
  strip is 98/108 px with square tiles. No DOM measurement on the hot path.
- **Motion** uses only opacity and transforms, honors `prefers-reduced-motion`
  and the existing Reduce motion and Animate working sessions settings.
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

- **L0 tokens.** Groups `color`, `agent`, `entity` (machine and project palette
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
- **macOS.** WKWebView caps `requestAnimationFrame` at 60 fps on ProMotion
  (WebKit bug 294338; no public API). Native scrolling smoothness must be judged
  separately.
- **Measurement (two weeks, one heavy screen: session list plus long Activity,
  on the same data plane):** p95 frame time while scrolling with a trackpad, input
  to frame latency, session open time, idle CPU and RSS; Channel throughput at
  1k, 5k and 20k updates per second batched versus naive; a 20k-50k session list;
  xterm.js WebGL versus DOM. Devices: the owner's Mac (ProMotion) and Arch with
  KDE Plasma at 100%, 125% and 150% scale, NVIDIA and integrated GPUs; WebView2
  on Windows for IPC p99. Decision by numbers against the tuned Qt client.

## Migration

The new client ships beside Qt as a separate application. Screens move in this
order once the measurement passes: Overview, Projects, Machines, Accounts and
Settings; then the session list, Activity, the composer and Tasks; platform
integrations move with the screen that needs them. The Qt client stays the
working client until the new one covers the parity checklist; installers keep
installing Qt until the owner switches the default.

## Open questions for the owner

1. Approve the design, or what to change.
2. Linux runtime: start on WebKitGTK and measure, with CEF as the fallback.
3. The 60 Hz animation cap on macOS.
4. User-imported skins in v1, or built-in skins first.
