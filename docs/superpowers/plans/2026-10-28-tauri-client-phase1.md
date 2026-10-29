# Tauri Client Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first real screen of the new desktop client (session list plus a long Activity, on live data from `hgs watch`) and the measurement that decides the runtime per platform.

**Beads:** Task 1 `zerus-5wmh.6`; Tasks 2-5 `zerus-5wmh.5`; Tasks 6-8 `zerus-5wmh.4`; later phases in the table at the end.

**Architecture:** A Tauri 2 application in a new top-level `client/` directory. Its Rust side starts one `hgs [@host] watch --json` per computer, decodes every frame with `crates/watch-protocol`, keeps the catalog and followed Activity in memory and sends the webview coalesced changes at most once per display frame over a Tauri `Channel`. The SolidJS frontend reuses the approved mockup's components (`design/webui`) bound to a store that applies those changes by key.

**Tech Stack:** Rust 1.99.1, Tauri 2 (wry 0.53+ for `with_webview_configuration`), `zerus-watch-protocol`, SolidJS 1.9, TypeScript, Vite, `@tanstack/solid-virtual`; macOS `objc2` bindings for the WebKit preference and the display-link counter.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` (approved 2026-10-27). The approved visual reference is `design/webui` and its review page.

## Global Constraints

- Rust toolchain 1.99.1 (`rust-version = "1.99"`), the same pin as CI; `cargo +1.99.1`.
- `client/core` and `client/src-tauri` are their own Cargo packages with their own `Cargo.lock`, like `crates/watch-protocol`; the root `hgs` package depends on neither.
- All NDJSON parsing happens in Rust; the webview receives ready view rows, never raw frames.
- Changes reach the webview at most once per display frame (the frontend's `requestAnimationFrame` asks for the next batch); one Channel message per batch.
- Every animation uses transform and opacity only; honour `prefers-reduced-motion`.
- No dots as separators in the new UI (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); the check runs in CI.
- macOS: disable WebKit's `PreferPageRenderingUpdatesNear60FPSEnabled` through `WKPreferences` `_features` / `_setEnabled:forFeature:` before the webview is created; feature-detect, read back, log failure, never panic.
- Never replace, quit or restart the installed Zerus GUI or CLI, native agents, tmux servers, services or the mobile connector. The client only runs `hgs watch` (read-only) and, later, guarded actions through `hgs`.
- Development windows run headless (Xvfb on Linux) unless the coordinator has warned the owner first; close every test window afterwards.
- Never commit live session content, screenshots of real data, login material or measurement dumps with real names; Nebula pages may show them on the owner's network only.
- Synthetic fixtures use reserved example domains and synthetic paths.

## Review Focus

- A computer whose installed `hgs` lacks `Event stream ABI: watch-v1`: the client must show that computer as unsupported, keep the others, and never spawn `watch` on it.
- A stream that ends or emits a fatal `error`: rows of that computer stay visible as last known, the computer shows as reconnecting, and restarts back off 5 s, 15 s, 60 s, then 5 min.
- A `gap` for the catalog: the old rows stay displayed until the replacement snapshot's `last` page, then are replaced in one batch (no flash of an empty list).
- Removal reasons (`replaced`, `archived`, `forgotten`, `ended`, unknown): the row leaves the list; `replaced` keeps the selection on the new run when the name matches.
- A burst of thousands of deltas within one frame: one batch per frame, the latest row per key wins, memory stays bounded.

Each line has its test in Task 3 or Task 4.

---

## File structure

```
client/
  README.md                      how to build, run headless, measure
  package.json  vite.config.ts  tsconfig.json  index.html
  src/
    main.tsx                     mounts App, loads skin
    App.tsx                      rail, session list, Activity, status bar (first screen)
    bridge.ts                    Tauri invoke/Channel wrapper; replay fallback for browser tests
    store/catalog.ts             rows by key, applies batches with reconcile
    store/activity.ts            followed session's events and state
    ui/                          components copied from design/webui (SessionList, SessionCard,
                                 model, Activity, Composer shell, icons, styles, skins runtime)
    perf/pacing.ts               rAF interval recorder and statistics
    perf/scenarios.ts            scripted scroll, open and input scenarios
    perf/Diagnostics.tsx         in-app panel that runs scenarios and saves results
  core/                          crate zerus-client-core: pure Rust, no Tauri (fast tests)
    Cargo.toml  Cargo.lock
    src/{lib.rs,fleet.rs,stream.rs,state.rs,view.rs,batch.rs}
    tests/fixtures/*.ndjson      synthetic watch streams
  src-tauri/                     crate zerus-client: the Tauri app, depends on ../core
    Cargo.toml  Cargo.lock  build.rs  tauri.conf.json  capabilities/default.json  icons/
    src/main.rs                  entry
    src/lib.rs                   builder, window creation (with the macOS configuration)
    src/commands.rs              tauri commands: start, next_batch, follow, unfollow, metrics
    src/metrics.rs               process RSS and CPU of the app and its webview processes
    src/platform/macos.rs        WebKit preference, display-link counter
scripts/check-ui-separators.py   CI gate over design/webui and client
```

---

### Task 1: Separator check as a CI gate, and the layout note

**Files:**
- Create: `scripts/check-ui-separators.py`
- Modify: `.github/workflows/checks.yml` (Quick checks job: one step)
- Modify: `AGENTS.md`, `CLAUDE.md` (one paragraph each, identical)

**Interfaces:**
- Produces: `python3 scripts/check-ui-separators.py [paths...]` exits 1 and prints `path:line: U+XXXX: excerpt` for each hit; default paths `design/webui` and `client`, skipping `node_modules`, `licenses`, `dist`, `target` and `src-tauri/icons`.

- [ ] **Step 1:** Write the script (stdlib only), with the character class built from escapes:

```python
#!/usr/bin/env python3
"""Fails when a dot-like separator appears in the new web UI's sources.

Owner rule (2026-10-27): the new web UI never separates values with dots.
Scope: design/webui and client only; the Qt GUI and mobile clients are unchanged.
"""
import pathlib, re, sys

CHARS = "\u00b7\u2022\u2219\u22c5\u2027\u2981\u30fb"
PATTERN = re.compile("[" + CHARS + "]")
TEXT = {".ts", ".tsx", ".js", ".jsx", ".css", ".html", ".json", ".md", ".svg", ".rs", ".toml"}
SKIP = {"node_modules", "licenses", "dist", "target", "icons", ".git"}

def scan(roots):
    hits = []
    for root in roots:
        for path in pathlib.Path(root).rglob("*"):
            if not path.is_file() or path.suffix not in TEXT or SKIP & set(path.parts):
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                for match in PATTERN.finditer(line):
                    hits.append(f"{path}:{number}: U+{ord(match.group()):04X}: {line.strip()[:100]}")
    return hits

if __name__ == "__main__":
    found = scan(sys.argv[1:] or ["design/webui", "client"])
    print("\n".join(found) or "No separator dots in the web UI sources.")
    sys.exit(1 if found else 0)
```

- [ ] **Step 2:** Run it: `python3 scripts/check-ui-separators.py` → exit 0. Probe: a temporary file with `"a \u00b7 b"` under a scratch directory passed as an argument → exit 1 with one line.
- [ ] **Step 3:** Add a step after the existing source checks in the Quick checks job: `- run: python3 scripts/check-ui-separators.py`. No new actions, no cache. Validate the workflow locally (`python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/checks.yml'))"` and the repository's workflow checks named in `docs/ci-and-aur.md`).
- [ ] **Step 4:** Add to AGENTS.md and CLAUDE.md, after the `design/` sentence: "The Tauri desktop client lives in `client/` (SolidJS frontend in `client/src`, Rust side in `client/src-tauri`, its own Cargo package and lock). It grows beside the Qt GUI; see `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`. Its UI never separates values with dots; `scripts/check-ui-separators.py` enforces it in CI."
- [ ] **Step 5:** Commit: `ci: check the web UI for dot separators`.

### Task 2: Client scaffold that builds and opens a window headless

**Files:**
- Create: `client/package.json`, `client/vite.config.ts`, `client/tsconfig.json`, `client/index.html`, `client/src/main.tsx`, `client/src/App.tsx`, `client/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json,src/main.rs,src/lib.rs}`, `client/README.md`, `client/.gitignore`
- Copy: `design/webui/skins/*.json` and `design/webui/src/skin/*` into `client/src/skin` (no edits)

**Interfaces:**
- Produces: `npm run tauri dev` and `npm run tauri build -- --debug` in `client/`; `ZERUS_CLIENT_HGS` overrides the `hgs` path (default: `hgs` on `PATH`).

- [ ] **Step 1:** `client/src-tauri/Cargo.toml`: package `zerus-client`, `rust-version = "1.99"`, `edition = "2021"`, dependencies `tauri = { version = "2", features = [] }`, `serde`, `serde_json`, `zerus-watch-protocol = { path = "../../crates/watch-protocol" }`; `[build-dependencies] tauri-build = "2"`; target `cfg(target_os = "macos")`: `objc2`, `objc2-foundation`, `objc2-web-kit` with the features needed for `WKWebViewConfiguration` and `WKPreferences`.
- [ ] **Step 2:** `tauri.conf.json`: product name `Zerus Next (dev)`, identifier `dev.zerus.next` (distinct from the installed app), no updater, no tray, one window created in code (not in config) so Task 7 can pass the configuration.
- [ ] **Step 3:** `lib.rs` builds the app and creates the main window with `WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))`, title `Zerus Next`, size 1440×900.
- [ ] **Step 4:** Frontend renders the mockup's rail and an empty list placeholder with the Zerus Dark skin.
- [ ] **Step 5:** Build and run headless on arch: `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a -s "-screen 0 1440x900x24" npm run tauri dev` (unsetting `WAYLAND_DISPLAY` alone lets GTK fall back to `wayland-0`, the desktop); capture a screenshot with `import -window root` (ImageMagick) or the WebKit inspector; the window must appear only inside Xvfb.
- [ ] **Step 6:** Commit: `feat(client): scaffold the Tauri client`.

### Task 3: Stream and state in Rust, tested on synthetic streams

**Files:**
- Create: `client/core/src/{lib.rs,stream.rs,state.rs,view.rs,fleet.rs}`, `client/core/tests/fixtures/{basic.ndjson,gap.ndjson,replaced.ndjson,burst.ndjson}`
- Test: unit tests in `state.rs`, `fleet.rs`; integration test `tests/stream.rs` that runs a fake `hgs` (a small Rust test binary or `python3` script writing a fixture) through `stream.rs`

**Interfaces:**
- Produces:
  - `pub struct ComputerId(pub String)` (empty string is this computer).
  - `pub enum Change { Upsert { computer: ComputerId, key: String, row: ViewRow }, Remove { computer: ComputerId, key: String, reason: Option<String> }, Replace { computer: ComputerId, rows: Vec<(String, ViewRow)> }, Computer { computer: ComputerId, status: ComputerStatus }, Activity { computer: ComputerId, key: String, update: ActivityUpdate } }`
  - `pub enum ComputerStatus { Probing, Unsupported, Live, Reconnecting { attempt: u32 }, Ended }`
  - `impl CatalogState { pub fn apply(&mut self, frame: &Frame) -> Vec<Change> }` — snapshot pages are staged until `last`, then one `Replace`; `gap` keeps the current rows until that `Replace`; `delta` gives one `Upsert` per key and one `Remove` per removal.
  - `pub struct ViewRow { name, title, agent, status, phase, attention, unread, machine, project, folder, branch, model, effort, summary, updated_at }` built by `view::row(&Value) -> ViewRow`; unknown fields are ignored, missing ones default.
  - `stream::spawn(hgs: &Path, computer: &ComputerId, sink: Sender<StreamEvent>) -> StreamHandle` with `StreamHandle::send(Control)`; a reader thread decodes with `zerus_watch_protocol::decode`; restart backoff 5 s, 15 s, 60 s, then 300 s.
  - `fleet::probe(hgs, computer) -> bool` runs `hgs [@host] -h` and looks for `HELP_MARKER`.

- [ ] **Step 1:** Write the fixtures (synthetic names such as `claude/sample-project/review`, paths under `/home/example`).
- [ ] **Step 2:** Write failing tests for: a three-page snapshot yields exactly one `Replace` after `last`; a `gap` followed by a snapshot keeps the old rows until the new `last`; a delta with a `replaced` removal yields `Remove` with `Some("replaced")`; a burst of 5 000 upserts of 50 keys yields 5 000 `Upsert` changes (coalescing is Task 4); an unknown frame type is skipped; a fatal `error` yields `Computer(Reconnecting)`.
- [ ] **Step 3:** Implement `state.rs` and `view.rs` until the tests pass (`cargo +1.99.1 test --manifest-path client/core/Cargo.toml`).
- [ ] **Step 4:** Failing test then implementation for `stream.rs` with the fake `hgs`: spawn, receive `Hello`, receive frames, EOF triggers restart after the first backoff (use an injectable clock or a backoff scale for tests).
- [ ] **Step 5:** `fleet.rs`: this computer first; peers from the local stream's `catalog.meta` value (the listing's peers); computers without the marker get `Unsupported` and no stream.
- [ ] **Step 6:** Commit: `feat(client): follow computers through hgs watch`.

### Task 4: Per-frame batches to the webview

**Files:**
- Create: `client/core/src/batch.rs`, `client/src-tauri/src/commands.rs`, `client/src/bridge.ts`, `client/src/store/catalog.ts`
- Test: unit tests in `batch.rs`; `client/src/store/catalog.test.ts` (Vitest)

**Interfaces:**
- Produces:
  - `impl Coalescer { pub fn push(&mut self, change: Change); pub fn take(&mut self) -> Batch }` — keeps the latest `Upsert` per `(computer, key)`, a `Remove` cancels a pending `Upsert`, a `Replace` drops pending row changes of that computer; memory bounded by the number of distinct keys.
  - `#[tauri::command] fn next_batch(state) -> Batch` — the frontend calls it from `requestAnimationFrame`; when nothing is pending it parks until a change arrives or 250 ms pass (so idle costs no polling).
  - `#[derive(Serialize)] pub struct Batch { pub upserts: Vec<RowOut>, pub removes: Vec<KeyOut>, pub replaces: Vec<ReplaceOut>, pub computers: Vec<ComputerOut>, pub activity: Vec<ActivityOut> }`
  - Frontend: `startBridge(onBatch: (b: Batch) => void)`; `applyBatch(b)` on a Solid store keyed by `computer + "\n" + key` with `reconcile`.

- [ ] **Step 1:** Failing tests: 5 000 upserts of 50 keys → one batch with 50 upserts; upsert then remove of the same key → only the remove; replace after pending upserts → only the replace; `take` on an empty coalescer returns an empty batch.
- [ ] **Step 2:** Implement until green.
- [ ] **Step 3:** Frontend store test: applying a batch keeps object identity of unchanged rows (the round 2 flicker root cause); a replace swaps the computer's rows in one update.
- [ ] **Step 4:** Commit: `feat(client): send coalesced changes once per frame`.

### Task 5: First live screen: session list and Activity

**Files:**
- Copy and adapt: from `design/webui/src` the session list (`screens/sessions/SessionList.tsx`, `SessionCard.tsx`, `model.ts`, `organize.ts`), Activity (`Activity.tsx`), `components/`, `lib/`, `styles/` into `client/src/ui`
- Create: `client/src/store/activity.ts`
- Modify: `client/src/App.tsx`

**Interfaces:**
- Consumes: `Batch` rows from Task 4; `follow(computer, key)` / `unfollow` commands that send `Control::Subscribe { topic: "activity", session, tail: 100 }` and later reads with `after`.
- Produces: the selected session's Activity renders its real events (user and agent messages, tool groups, thinking, receipts) newest at the bottom; the list renders every live session of every computer, grouped by project, with the approved sizes and badges.

- [ ] **Step 1:** Replace the mockup's `catalog` store and `selected` with the live stores; keep the mock-only review tools out of the client.
- [ ] **Step 2:** Map `ViewRow` to the card fields the components read; statuses use the same captions as Qt (`SessionPresentation`).
- [ ] **Step 3:** Activity: follow the selected session; render events as the mockup's `Item` kinds; older pages load with `after` on scroll to the top.
- [ ] **Step 4:** Run on arch under Xvfb against the installed `hgs`; screenshot for the Nebula page only (never committed).
- [ ] **Step 5:** Commit: `feat(client): live session list and Activity`.

### Task 6: Frame pacing and resource measurement

**Files:**
- Create: `client/src/perf/{pacing.ts,scenarios.ts,Diagnostics.tsx}`, `client/src-tauri/src/metrics.rs`, `client/scripts/measure-report.py`
- Test: `client/src/perf/pacing.test.ts`

**Interfaces:**
- Produces:
  - `recordFrames(ms: number): Promise<number[]>` (rAF deltas) and `stats(deltas, T)` returning `{ mean, median, p95, p99, over1_5T, over2_5T, band60 }` per the spec's criteria (T = 1000 / display Hz; band60 is the share in 15.0–18.5 ms).
  - Scenarios: list scroll (scrollTop advanced each frame through 2 000 rows), Activity scroll (a session with at least 1 000 events), session open (click to first Activity paint), input-to-frame latency (pointerdown timestamp to the next rAF after the state change).
  - `metrics` command: RSS and CPU of the client process tree (app plus WebKit web and network processes).
  - `measure-report.py` turns result JSON files into the Russian Nebula page (no real session names).

- [ ] **Step 1:** Failing tests for `stats` with synthetic deltas: a perfect 120 Hz series passes; a 60 Hz series fails `band60` and `mean`; a series with 0.3% over 2.5 T fails.
- [ ] **Step 2:** Implement; Diagnostics panel runs the scenarios three times after a 2 s warm-up, keeps the worst run, writes JSON to the app data directory.
- [ ] **Step 3:** Qt comparison: use the existing Qt paint and scroll instrumentation in `tray/` (find the harness behind the Activity scroll paint numbers in Beads `zerus-pgo9` and `zerus-tllp`), idle CPU and RSS of the running GUI from `ps` (read-only), and session open time from its existing timing logs. Never restart or replace the installed GUI.
- [ ] **Step 4:** Commit: `feat(client): measure frame pacing and resources`.

### Task 7: macOS display-rate rendering

**Files:**
- Create: `client/src-tauri/src/platform/macos.rs`
- Modify: `client/src-tauri/src/lib.rs` (window creation passes the configuration on macOS)

**Interfaces:**
- Produces: `macos::webview_configuration() -> Retained<WKWebViewConfiguration>` with the preference disabled when the SPI exists; `macos::preference_state() -> Option<bool>` read back for diagnostics; `macos::display_link_hz(window) -> f64` counting `CADisplayLink` callbacks for 2 s (the native control of criterion 3).

- [ ] **Step 1:** Implement with `objc2`: look up `+[WKPreferences _features]`, find the feature whose `key` is `PreferPageRenderingUpdatesNear60FPSEnabled`, call `-[WKPreferences _setEnabled:forFeature:]` with `NO`; every selector checked with `respondsToSelector:` first; failures are logged and returned, never panics.
- [ ] **Step 2:** Pass it with `WebviewWindowBuilder::with_webview_configuration` before the window exists.
- [ ] **Step 3:** On the Mac (after the coordinator confirms the owner was warned): build the dev client with `cargo +1.99.1`, run the Diagnostics scenarios with the preference disabled and, as the must-fail control, with it left at the default; record the display-link rate.
- [ ] **Step 4:** Commit: `feat(client): render at the display rate on macOS`.

### Task 8: Results page and decision

- [ ] Run Task 6 scenarios on arch (KDE, WebKitGTK; real display only after the owner is warned; Xvfb numbers are labelled as not representative) and on the Mac (Task 7), plus the Qt comparison.
- [ ] Publish the Russian results page on Nebula: per platform the criteria table (pass/fail), p95 frame time while scrolling, input-to-frame latency, session open time, idle CPU, RSS, against Qt.
- [ ] Record the decision per platform in the spec and Beads `zerus-5wmh.4`: WebKit route passes, or the Chromium fallback is evaluated.

---

## Later phases (separate plans)

| Phase | Scope | Beads |
|---|---|---|
| 2 | Guarded session actions through `hgs` (interrupt, mark read, pause, resume, rename, archive, terminate) with Qt's enablement, confirmations, notices and request identities; send moves to phase 4 with the full composer; plan: `docs/superpowers/plans/2026-10-28-tauri-client-phase2-actions.md` | zerus-5wmh.7 |
| 3a | Overview, Projects, Machines and Accounts on live data, reads only: machine cards from `catalog.meta`, account, machine and swarm reads through the screens hub; writes disabled with their phase; plan: `docs/superpowers/plans/2026-10-28-tauri-client-phase3a-screens.md` | zerus-5wmh.8 |
| 3b | Settings, including the Phones panel the mockup shows under Machines | zerus-5wmh.24 |
| 3c | Machines, Accounts and Projects writes (connections, install, accounts, defaults, permissions, models, projects, folders, swarm, the legacy `project ls` import, Open folder, local label and project colours) | zerus-5wmh.25 |
| 3 | Peers without the watch-v1 stream: Qt's polling fallback (`hgs @alias ls --json`) | zerus-5wmh.34 |
| 4 | Composer and Send (Qt's block reasons, cold cache confirmation, resume-then-send, local failed messages, durable drafts), attachments, voice and video, Terminal (xterm.js, PTY in Rust), Processes, Native UI | zerus-5wmh.9 |
| 5 | Tasks and the side panel; New session and worktree flows; the Projects Worktrees panel and its `worktrees` read; New session from a project folder | zerus-5wmh.10 |
| 6 | Skins v1 loader in Settings with user import and validation; i18n; accessibility pass | zerus-5wmh.11 |
| 7 | Packaging and signing per platform (macOS notarised bundle, Arch package, Windows later), updates, Dev channel | zerus-5wmh.12 |
| 8 | Side-by-side period: the client ships beside Qt as a separate app; parity checklist tracking; the owner switches the default | zerus-5wmh.13 |
