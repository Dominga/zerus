# Tauri Client Phase 3a: Overview, Projects, Machines and Accounts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Overview, Projects, Machines and Accounts screens replace their "later screen" placeholders with the approved mockup (`design/webui/src/screens/{Dashboard,Projects,Machines,Accounts}.tsx`), bound to live data and reading only (Beads `zerus-5wmh.8`).

**Architecture:**
- **Data sources:**
  - Machine cards come from `Batch.machines` (`MachineOut {computer, machine: MachineMeta}`, PR #391).
  - Connection states come from `Batch.computers`; session rows are the existing catalog store.
  - Everything else is an `hgs` read through `zerus_client_core::reads::read(hgs, computer, &Read)` (PR #390). Both PRs merge before this plan runs; if a name differs on main at execution time, follow main.
- **Pure projection in `client/core/src/views/`:**
  - `usage.rs` reads usage windows, with ISO resets parsed to seconds.
  - `accounts.rs` ports Qt's account grouping, member states, texts and the Overview's hidden-card rule.
  - `machines.rs` builds the `machine ls` rows and the `resolve` and `check` texts.
  - `projects.rs` projects `swarm get` and Qt's status line.
  - `schedule.rs` makes every "which read runs now" decision over a clock the caller passes in.
- **The hub in `client/src-tauri/src/screens.rs`:** it knows which screens are visible, runs due reads on the blocking pool and emits `screens-changed` with the screen's name when a view changed. The commands are:
  - `screen_visible`
  - `accounts_view`
  - `machines_view`
  - `machine_resolve`
  - `machine_check`
  - `projects_view`
- **Frontend:**
  - stores for machine cards, accounts, machines and projects;
  - Overview selectors created by the Overview screen, so they cost nothing elsewhere;
  - the four screens ported from the mockup with its markup and classes.

Every write keeps its place, label and Qt enablement shape but stays disabled with a reason.

> **Decisions (controller rulings, 2026-10-28, and this plan's own choices).**
> Where the mockup and Qt differ, these rulings decide. The rest follows Qt.
> - **Connection states.**
>   - `live` is Connected (green). Only `live` counts as fresh or online: for the "Machines online" tile, the load colours and the offline notice.
>   - `probing` is connecting (amber, pulsing), text "Connecting".
>   - `reconnecting` is connecting (amber) with the last values kept, text "Reconnecting". Its sample tooltip is Qt's "Last known values. This machine is unavailable or has not replied recently.".
>   - `ended` is Offline (red).
>   - `unsupported` is not connected (red), text "Update hgs", with the CPU detail "Update hgs for metrics".
>   - A `machine ls` row with `enabled: false` is Disabled (grey).
>   - Accounts keeps Qt's lenient rule: a peer is offline only after a failed stream (`reconnecting` with `attempt > 0`, or `ended`).
> - **Peers without watch-v1:** no polling fallback in 3a. Task 9 files the follow-up (no Beads child of `zerus-5wmh` covers it, checked 2026-10-28).
> - **Overview:**
>   - CPU and memory percentages are the main figures (22 px, load colour). The details read "N cores, load X.XX" and "A of B" with Qt's `bytes()` GiB/MiB rule.
>   - The sample time shows only when it is not current, as "Last sample d MMM HH:mm". "Current" means a `live` stream and a sample under 60 s old.
>   - The section title is "Attention & in progress".
>   - Sessions sort with needs-attention first, then the last event descending, then the key (`DashboardPage.cpp:452-456`).
>   - Account cards take the pace tone (`AccountUsage::severest`).
>   - The hidden-card rule, "Loading accounts…", "Open Manage accounts to view or set up accounts.", "No sessions are working, need attention or have drafts." and the offline notice are Qt's.
> - **Accounts usage panel:**
>   - It lists all windows in the mockup's layout. The bar takes Qt's threshold colour (`AccountUsage::color`), followed by "Resets d MMM HH:mm" and the forecast line.
>   - Meters on rows, cards and the Overview use the pace tone.
>   - The cached `account_status` of `account ls` seeds windows, balances, credits, identity and `checked_at` until the first `account inspect` answers. Qt seeds identity and `checked_at` only.
> - **`usage.ts` gaps 1-4 and 7 of the inventory close here:**
>   - an adapter from the projected window;
>   - a missing reset counts as current;
>   - `period()` reads the label and handles a null length;
>   - "reset unknown";
>   - the tooltip gains identity lines, balances and the refresh notes.
> - **Projects:**
>   - `swarm get` runs every 5 s only while Projects is visible, with a 10 s timeout. `project ls` is not read: it serves only Qt's one-time legacy import, a write for 3c.
>   - The Worktrees panel and its `worktrees` read move to phase 5 (`zerus-5wmh.10`).
>   - "Open folder" stays disabled until 3c (it needs a platform module), and "New session…" until phase 5.
>   - The legacy import is 3c.
> - **Moved out of 3a:**
>   - the Phones panel under Machines moves to 3b Settings (`zerus-5wmh.24`);
>   - the Add to machine dialog is not ported; its entry is disabled.
> - **Local preferences:**
>   - Machine label colours and styles and the project "vivid" style are not hgs data. 3a shows the defaults (a project's own `color`, else the palette slot by index) and persists no edits (3c).
>   - "All swarm projects" works for the session without being saved.
> - **This plan's own boundary choices:**
>   - `MachineMeta` (PR #391) gains `local_sessions` and `local_host`. The Overview, Accounts and Machines need the control-console flag and Qt's Windows session-host line, and `catalog.meta` is their only source.
>   - The Machines connection state line and summary are computed in the frontend from `Batch.computers`, with the same `connectionOf` table as the Overview. The Rust Machines view carries the `machine ls` rows and the resolve and check texts. A Rust state line would need a second, throttled push of stream state that the webview already holds.
>   - The fleet the hub reads is `Pump::fleet()`: the pump's mirror of statuses and machine cards. It is the one source that covers live and replay alike.
>   - Qt's middle-dot joins become parentheses or commas (the dots rule): "New sessions: opus (high)" and "API key, 2 accounts".
>   - Reasons on disabled entries:
>     - "Arrives with machine editing." (Machines);
>     - "Arrives with account management." (Accounts);
>     - "Arrives with project editing." (Projects, including Open folder);
>     - "Arrives with the New session dialog." (New session…);
>     - "Arrives with the worktree flows." (Worktrees…);
>     - "Arrives with the copy actions." (Copy folder path, `zerus-5wmh.19`).

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2), SolidJS 1.9 + TypeScript + Vite, `@tanstack/solid-virtual`, vitest, Python 3 stdlib for the fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`: "Parsing never happens in the webview", "Rust ... projects screen view models", the Overview rounds 7-8, the account usage round 5 and the Machines round 6. Phase table: `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`. The behavioural reference is the Qt GUI; every rule below names its lines (verified on main `0a6fa30b`):
- `tray/src/AccountCatalog.h:13-79`: `profileKey`, `key`, `betterUsage`, `named`, `group`.
- `tray/src/AccountUsage.h`:
  - `period` 26-32, `reset` 33-36, `current` 37, `remaining` 38-45;
  - `pace` 76-90, `severest` 93-97, `forecast` 98-104, `windowText` 111-115;
  - `color` 184, `state` 185-196, `tooltip` 197-219;
  - `summaryText` 350-353, `Panel::setData` 368-384.
- `tray/src/AccountUsageStore.cpp:9-64`: `online`, `data`, `ensure`, `dispatch`, `finish`. `RefreshInterval` is in `AccountUsageStore.h:22`.
- `tray/src/AccountsPage.cpp`:
  - permission helpers 72-92;
  - `accountHosts` and `remoteOnlyHint` 165-177;
  - the 30 s timer 354;
  - `pendingAccounts` and `updateRefreshState` 419-447;
  - the warm-up of new hosts in `setFleet` 378-381;
  - `ensureCatalogs` and `requestQueuedCatalog` 450-468;
  - `profiles` 469-488;
  - the display hold in `render` 519-536;
  - `selectionChanged` 589-704, with identity lines 625-636 and detail texts 644-663;
  - `online` 709-715.
- `tray/src/DashboardPage.cpp`:
  - `bytes`, `age` and `updateLoadBar` 29-71;
  - the machine card 110-138;
  - the account card 255-266 and its stripe 279;
  - `setAccounts` with the hidden-card rule 341-360;
  - `render` 401-456, with the session order 452-456.
- `tray/src/MachinesPage.cpp`:
  - check texts 201-211, the resolve failure 218;
  - `renderRows` 301-322, `loadSelection` 324-341;
  - `resolveConnection` and `showEffectiveConnection` 353-377;
  - `updateControls`, with the session host line 405-419, 378-420;
  - `check` 431.
- `tray/src/SwarmController.cpp:191-219` (`received`: schema check and status precedence); the order is in `presentation` 131-170.
- `tray/src/ProjectsDialog.cpp:251-263` (unconnected peers) and 300-303 (the folder name and Computer columns).
- `tray/src/OpenCodeModels.cpp:64-80, 115-130`.

The data inventory with every key and type is in the session's scratch notes. The rules this plan depends on are quoted in the tasks.

## Global Constraints

- Rust toolchain 1.99.1 (`cargo +1.99.1`, `rust-version = "1.99"`); `client/core` stays pure Rust without Tauri.
- The new UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); `scripts/check-ui-separators.py` enforces it, and it also scans `.rs` files under `client/`.
- No obvious keyboard hints in the UI (owner rule, round 6): no `kbd` on menu entries, no "Enter to …" or "Alt+Up" texts.
- Never run the real `hgs` against live sessions in tests; tests use the fake `hgs` in `client/core/tests/support`.
- Headless GUI runs only with `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...` (`client/README.md`, "Headless runs").
- Repository text in English; fixtures synthetic (reserved example domains, `/home/example/...`, `claude/sample-project/review`).
- Reads run off the UI thread with per-command timeouts and bounded output (`reads` limits: 8 MiB, the Qt timeouts). The webview never parses CLI output; it receives projected view models.
- Session counts can reach tens of thousands: view models are per screen and bounded (at most 256 profiles per host, 64 machines, 500 projects, 200 folders per project). The Overview selectors are O(rows) at most once per batch. The 1 s and 15 s timers update only time labels, never per-row work.
- No read on a hidden screen, except Qt's account warm-up: every newly listed account host gets one `account ls`, as `AccountsPage::setFleet` does.
- Never run `machine check`, `account inspect` or any write against real machines in tests. The live-read test of PR #390 stays `#[ignore]` and prints kinds only.
- Writes stay disabled with the reasons in the Decisions note. Navigation works: tiles, links to Sessions with a filter, Machines, Accounts and opening a session.

## Review Focus

1. A peer goes `reconnecting` while Overview is open: its card keeps the last values with the unknown load colour, it leaves the "Machines online" count, and its sessions count in the offline notice (test in Task 5).
2. An account appears on two hosts with the same identity: one card with both machines. Signed-out or unidentified profiles stay separate cards (test in Task 1).
3. A `resets_at` in ISO form, a null reset, or a window with `window_minutes: null` and a label ("Monthly") gives the right label, counts as current and never shows NaN (tests in Tasks 1 and 4).
4. A slow or failing `account inspect` (25 s timeout): other accounts still load, at most two run at once, and the failed one keeps its last data with a refresh note. Nothing retries inside 5 minutes unless forced (tests in Tasks 2 and 3).
5. Projects hidden: no `swarm get` runs. Projects shown: one every 5 s, never two at once (tests in Tasks 2 and 3).

---

## File Structure

```
client/core/src/views/mod.rs           FleetEntry; JSON helpers shared by the views
client/core/src/views/usage.rs         Window, Identity, Balance, Credits, Usage; parse_iso(), reset_seconds(), window(), usage()
client/core/src/views/accounts.rs      AccountsState (CatalogSlot, UsageSlot, UsageRef), MemberState, Profile, Account, AccountsView,
                                       AccountsDisplay; hosts(), online(), reachable(), refs(), group(), project()
client/core/src/views/schedule.rs      Screen, Poll, the timing of AccountsState (ensure_catalogs, warm, finish_catalog,
                                       ensure_usage, dispatch, finish_usage, reading)
client/core/src/views/machines.rs      MachinesState, MachineRow, Effective, CheckResult, MachinesView
client/core/src/views/projects.rs      SwarmState, Project, Folder, ProjectsView, leaf()
client/core/src/machine.rs             MachineMeta gains local_sessions and local_host (LocalHost)
client/core/src/lib.rs                 pub mod views
client/core/tests/fixtures/views/*.json   synthetic read replies for the view and hub tests
client/core/tests/support/fake_hgs_reads.py  a reply may sleep (concurrency tests)
client/src-tauri/src/screens.rs        Screens hub, the six commands, REPLAY_NEEDS_HGS
client/src-tauri/src/pump.rs           Pump::fleet()
client/src-tauri/src/commands.rs       AppState::pump()
client/src-tauri/src/lib.rs            manages Screens, registers the commands
client/src/types.ts                    MachineMeta, MachineOut, Batch.machines, the view types, Screen
client/src/bridge.ts                   machines in batches; screenVisible, accountsView, machinesView, machineResolve,
                                       machineCheck, projectsView, onScreensChanged
client/src/dev/replay.ts               replays the fixture's machine cards
client/src/store/machines.ts           machine cards (Batch.machines) and the Machines page view
client/src/store/accounts.ts           the accounts view, the machine filter, the profile an Overview card opens
client/src/store/projects.ts           the projects view
client/src/store/screens.ts            page visibility to screen_visible, screens-changed to re-pulls, latest()
client/src/store/overview.ts           connectionOf(), fleetCards(), overviewStats(), liveSessions(), createOverview()
client/src/store/catalog.ts            createCatalog(onMachines) applies machine cards right after computers
client/src/store/session.ts            starts the screens wiring
client/src/ui/lib/usage.ts             the gap fixes, fromProjected(), severest(), summaryText(), stateText(), accountTooltip()
client/src/ui/components/UsageMeter.tsx   nullable resets
client/src/ui/components/MachineLabel.tsx, Status.tsx   the mockup's label and status helpers
client/src/ui/screens/reasons.ts       the disabled-entry reasons
client/src/ui/screens/Overview.tsx, overview-ui.ts      the Overview port and its pure helpers
client/src/ui/screens/Machines.tsx, machines-ui.ts      the Machines port and its pure helpers
client/src/ui/screens/Accounts.tsx, accounts-ui.ts      the Accounts port and its pure helpers
client/src/ui/screens/Projects.tsx, projects-ui.ts      the Projects port and its pure helpers
client/src/ui/styles/screens.css       design/webui setup.css lines 1-154 (shell, Overview, Projects, table)
client/src/ui/styles/machines.css      design/webui settings.css: status rows, Machines and Accounts
client/src/App.tsx                     routes the four pages to the screens
client/src/main.tsx                    imports the two stylesheets
client/src/ui/PerfHost.tsx             the screens-smoke=1 hook
client/scripts/make-fixture.py, client/fixtures/*   metrics in the replay meta, machine cards in the browser batches
client/README.md                       "Screens" section
docs/superpowers/plans/2026-10-28-tauri-client-phase1.md   phase table: 3a, 3b, 3c and phase 5 additions
```

---

### Task 1: Usage windows and the accounts projection (`client/core/src/views/`)

**Files:**
- Create: `client/core/src/views/mod.rs`, `client/core/src/views/usage.rs`, `client/core/src/views/accounts.rs`
- Create: `client/core/tests/fixtures/views/accounts-local.json`, `accounts-studio.json`, `inspect-claude-work.json`, `inspect-native-codex.json`
- Modify: `client/core/src/lib.rs` (`pub mod views;` after `pub mod view;`)
- Test: unit tests in `usage.rs` and `accounts.rs`

**Interfaces:**
- Consumes: `ComputerId` (`client/core/src/lib.rs`); the JSON of `hgs [@host] account ls` and `account inspect ID --refresh` as `zerus_client_core::reads::read` returns it (PR #390: `Read::Accounts`, `Read::AccountUsage { id }`).
- Produces (all `Serialize`, snake_case JSON; the TypeScript twins are in Task 4):

```rust
// views/mod.rs
pub struct FleetEntry { pub computer: ComputerId, pub status: String, pub attempt: u32, pub local_sessions: bool,
                        pub local_host: bool, pub host: Option<String>, pub platform: Option<String> }
impl FleetEntry { pub fn name(&self) -> String }
// views/usage.rs
pub struct Window { pub id: String, pub label: String, pub used: f64, pub minutes: Option<u64>, pub resets_at: Option<f64> }
pub struct Identity { pub email, pub name, pub plan, pub organization, pub account_id, pub auth_method: String }
pub struct Balance { pub kind: String, pub balance: String, pub currency: String }
pub struct Credits { pub balance: String, pub unlimited: bool }
pub struct Usage { pub status, pub provider, pub label, pub auth_status: String, pub checked_at: Option<f64>, pub identity: Identity,
                   pub windows: Vec<Window>, pub balances: Vec<Balance>, pub credits: Option<Credits>,
                   pub refreshing: bool, pub refresh_error: bool, pub offline: bool }
pub fn parse_iso(text: &str) -> Option<f64>;
pub fn reset_seconds(value: Option<&Value>) -> Option<f64>;
pub fn window(value: &Value) -> Option<Window>;
pub fn usage(value: &Value) -> Usage;
// views/accounts.rs
pub type UsageKey = (ComputerId, String, String);
pub struct UsageRef { pub host: ComputerId, pub id: String, pub home: String, pub provider: String, pub auth_revision: String }
pub struct CatalogSlot { pub requested: Option<u64>, pub pending: bool, pub queued: bool, pub data: Option<Value>, pub error: Option<String> }
pub struct UsageSlot { pub auth_revision: String, pub attempted: Option<u64>, pub pending: bool, pub failed: bool, pub data: Option<Value> }
pub struct AccountsState { pub catalogs: BTreeMap<ComputerId, CatalogSlot>, pub usage: BTreeMap<UsageKey, UsageSlot>, /* queue, running, warmed: pub(crate) */ }
pub enum MemberState { Ready, Offline, NotInstalled, Apikey, SignedOut, Keychain, Desktop, Unavailable, Expired, Checking, Unknown } // kebab-case
pub struct Host { pub computer: ComputerId, pub name: String, pub label: String, pub online: bool }
pub struct Profile { key, host, machine, id, label, provider, native, installed, is_default, home, auth_revision, online,
                     state: MemberState, detail, signed_in, permission, permission_label, permission_hint, model_supported,
                     model_text, usage: Usage, opencode: Option<OpenCode> }
pub struct Account { key, label, provider, installed, machines: Vec<String>, members: Vec<Profile>, usage: Usage,
                     permission: String, identity_lines: Vec<String>, identity_note: String }
pub struct AccountsView { hosts: Vec<Host>, accounts: Vec<Account>, overview: Vec<Account>, snapshot: bool, loading: bool,
                          pending: usize, loaded_machines: usize, busy: bool, notices: Vec<String>, no_hosts: Option<String> }
pub struct AccountsDisplay { /* held rows and the sticky snapshot flag */ }
pub fn online(entry: &FleetEntry) -> bool;      // AccountsPage::online
pub fn reachable(entry: &FleetEntry) -> bool;   // AccountUsageStore::online
pub fn hosts(fleet: &[FleetEntry]) -> Vec<Host>;
pub fn refs(fleet: &[FleetEntry], state: &AccountsState) -> Vec<(UsageRef, bool)>;
pub fn group(profiles: &[Profile]) -> Vec<Account>;
pub fn project(fleet: &[FleetEntry], state: &AccountsState, display: &mut AccountsDisplay) -> AccountsView;
```

Rules (quoted from Qt; the code in Step 4 is their port):
- **Hosts** (`accountHosts`, `AccountsPage.cpp:165-170`): this computer when `local_sessions`, then every peer. The label is `"<host> (this machine)"` for this computer.
- **Profiles:**
  - Hosts are read in fleet order ("" first, then aliases), profiles in catalog order.
  - The local Windows computer lists no `dsh` or `opencode` (`AccountCatalog::launchable`).
- **Usage of a profile** (`AccountUsageStore::data` and `AccountsPage::profiles`):
  - A slot of another credential revision does not count.
  - Without data the status is `loading` (host reachable) or `offline`.
  - Without inspect data the cached `account_status` seeds identity, `checked_at`, windows, balances and credits (ruling). With data whose status is `unavailable` or `offline` and that has no identity, it seeds identity and `checked_at` (Qt).
  - `refreshing` means an inspect is pending, `refresh_error` that the last one failed, `offline` that the host is not reachable.
- **Grouping** (`AccountCatalog::group`):
  - The key is the provider plus `account_id` (else the lowercased trimmed email) plus the lowercased organization.
  - A signed-out or unidentified profile gets its own `profile:` key.
  - An email-only profile joins the one account ID known for that email.
  - The better snapshot wins: `ok`/`configured` beats the rest (offline ranks lower), then the newer `checked_at`.
  - Named labels win; an unnamed group takes the email.
- **The Overview** leaves out a native, unnamed profile whose key is `profile:` while its status is `loading`, `unavailable` or `offline` (`DashboardPage.cpp:348-350`).
- **Display hold** (`AccountsPage.cpp:525-535`): a profile still resolving (installed, online, loading or refreshing) without identity is shown only as its previous row.
- **Pending and snapshot:** `pending` counts hosts without a first `ls` or with one in flight, plus the resolving profiles. `snapshot` becomes true once a profile is shown or nothing is pending, and stays true.

- [ ] **Step 1: Write the fixtures** (synthetic; `client/core/tests/fixtures/views/`).

`accounts-local.json`:

```json
{"host":"workstation","revision":"rev-1","defaults":{"claude":"claude-work"},"removed_profiles":[],"profiles":[
 {"id":"claude-work","label":"Work","provider":"claude","native":false,"installed":true,"is_default":true,"home":"/home/example/.config/hgs/accounts/claude-work","credential_file":true,"portable":true,"auth_status":"signed_in","auth_revision":"a1","permission_mode":"provider","effective_permission_mode":"auto","permission_detail":"From ~/.claude/settings.json","model_defaults":{"model":"opus","effort":"high"},"model_defaults_supported":true,
  "account_status":{"status":"ok","auth_status":"signed_in","checked_at":1793000000,"identity":{"email":"person@example.com","name":"Example Person","plan":"max","organization":"","account_id":"acct-1","auth_method":"oauth"},"windows":[{"id":"five_hour","label":"","used_percent":34.0,"window_minutes":300,"resets_at":1793010000}],"balances":[]}},
 {"id":"native-claude","label":"Default account","provider":"claude","native":true,"installed":true,"is_default":false,"home":"/home/example/.claude","auth_status":"unknown","auth_revision":"b1","permission_mode":"provider","effective_permission_mode":"default","model_defaults_supported":true,"account_status":null},
 {"id":"native-codex","label":"Default account","provider":"codex","native":true,"installed":true,"is_default":true,"home":"/home/example/.codex","auth_status":"signed_out","auth_revision":"c1","permission_mode":"bypass","effective_permission_mode":"bypass","model_defaults_supported":false,"account_status":{"status":"signed_out","checked_at":1793000000,"identity":{},"windows":[],"balances":[]}},
 {"id":"native-dsh","label":"Native DeepSeek account","provider":"dsh","native":true,"installed":false,"is_default":true,"home":"/home/example/.dsh","auth_revision":"","permission_mode":"provider","effective_permission_mode":"default","model_defaults_supported":false,"account_status":null}]}
```

`accounts-studio.json`:

```json
{"host":"studio","revision":"rev-9","defaults":{},"removed_profiles":[],"profiles":[
 {"id":"claude-studio","label":"Default account","provider":"claude","native":true,"installed":true,"is_default":true,"home":"/home/example/.claude","auth_status":"signed_in","auth_revision":"s1","permission_mode":"provider","effective_permission_mode":"default","model_defaults":{"model":"","effort":""},"model_defaults_supported":true,
  "account_status":{"status":"ok","checked_at":1793000100,"identity":{"email":"Person@Example.com ","name":"Example Person","plan":"max","organization":"","account_id":"acct-1","auth_method":"oauth"},"windows":[{"id":"five_hour","label":"","used_percent":71.0,"window_minutes":300,"resets_at":"2026-10-26T09:00:00Z"},{"id":"seven_day","label":"","used_percent":40.0,"window_minutes":10080,"resets_at":1793400000}],"balances":[]}},
 {"id":"kimi-studio","label":"Moonshot","provider":"kimi","native":false,"installed":true,"is_default":true,"home":"/home/example/.config/hgs/accounts/kimi-studio","auth_status":"signed_in","auth_revision":"k1","permission_mode":"provider","effective_permission_mode":"auto","model_defaults_supported":false,
  "account_status":{"status":"ok","checked_at":1793000200,"identity":{"email":"","name":"","plan":"Pro","organization":"","account_id":"","auth_method":""},"windows":[{"id":"limit_month_total","label":"Monthly","used_percent":50,"window_minutes":null,"resets_at":null}],"balances":[]}}]}
```

`inspect-claude-work.json`:

```json
{"id":"claude-work","provider":"claude","label":"Work","home":"/home/example/.config/hgs/accounts/claude-work","host":"workstation","status":"ok","auth_status":"signed_in","source":"oauth","checked_at":1793000300,
 "identity":{"email":"person@example.com","name":"Example Person","plan":"max","organization":"","auth_method":"oauth","account_id":"acct-1"},
 "windows":[{"id":"five_hour","label":"","used_percent":12.0,"window_minutes":300,"resets_at":"2026-10-26T09:00:00Z"},{"id":"seven_day","label":"","used_percent":40.0,"window_minutes":10080,"resets_at":1793400000},{"id":"seven_day_sonnet","label":"Sonnet","used_percent":5.0,"window_minutes":10080,"resets_at":1793400000}],
 "balances":[],"credits":{"balance":"12.5","unlimited":false}}
```

`inspect-native-codex.json` (used by Task 3):

```json
{"id":"native-codex","provider":"codex","label":"Default account","home":"/home/example/.codex","host":"workstation","status":"signed_out","auth_status":"signed_out","checked_at":1793000400,"identity":{},"windows":[],"balances":[]}
```

- [ ] **Step 2: Write the failing tests.** In `usage.rs` (`#[cfg(test)] mod tests { use super::*; use serde_json::json; ... }`):

```rust
#[test]
fn iso_and_number_resets_become_seconds() {
    assert_eq!(parse_iso("2026-10-28T17:00:00+00:00"), Some(1793206800.0));
    assert_eq!(parse_iso("2026-10-28T20:00:00+03:00"), Some(1793206800.0));
    assert_eq!(parse_iso("2026-10-28T17:00:00.250Z"), Some(1793206800.25));
    assert_eq!(parse_iso("2026-11-04T09:30:00-0130"), Some(1793790000.0));
    assert_eq!(parse_iso("2024-02-29T00:00:00Z"), Some(1709164800.0));
    assert_eq!(parse_iso("1970-01-01T00:00:00"), Some(0.0));
    for bad in ["", "tomorrow", "2026-02-30T00:00:00Z", "2026-13-01T00:00:00Z", "2026-10-28T24:00:00Z",
                "2026-10-28T17:00:00+5", "2026-10-28T17:00:00.Z", "2026-10-28T17:00:00 UTC"] {
        assert_eq!(parse_iso(bad), None, "{bad:?}");
    }
    assert_eq!(reset_seconds(Some(&json!(1793206800))), Some(1793206800.0));
    assert_eq!(reset_seconds(Some(&json!("2026-10-28T17:00:00Z"))), Some(1793206800.0));
    assert_eq!(reset_seconds(Some(&json!(null))), None);
    assert_eq!(reset_seconds(Some(&json!(true))), None);
    assert_eq!(reset_seconds(None), None);
}

// Review Focus 3: ISO, null and unlengthed resets keep their labels, and no NaN reaches the view.
#[test]
fn windows_keep_iso_null_and_unlengthed_resets() {
    let reply = json!({"status": "ok", "provider": "kimi", "checked_at": 1793000000, "windows": [
        {"id": "limit_5h", "label": "", "used_percent": 34.5, "window_minutes": 300, "resets_at": "2026-10-28T17:00:00Z"},
        {"id": "limit_7d", "label": "", "used_percent": 12, "window_minutes": 10080.0, "resets_at": null},
        {"id": "limit_month_total", "label": "Monthly", "used_percent": 50, "window_minutes": null, "resets_at": 1793206800},
        {"id": "broken", "used_percent": "lots"}],
        "balances": [{"kind": "Topped up", "balance": "18.40", "currency": "USD"}], "credits": {"balance": "3", "unlimited": false}});
    let u = usage(&reply);
    assert_eq!(u.windows.len(), 3, "a window without a numeric share is left out");
    assert_eq!(u.windows[0], Window { id: "limit_5h".into(), label: String::new(), used: 34.5, minutes: Some(300), resets_at: Some(1793206800.0) });
    assert_eq!((u.windows[1].minutes, u.windows[1].resets_at), (Some(10080), None));
    assert_eq!((u.windows[2].label.as_str(), u.windows[2].minutes, u.windows[2].resets_at), ("Monthly", None, Some(1793206800.0)));
    assert!(u.windows.iter().all(|w| w.used.is_finite()));
    assert_eq!(u.balances, vec![Balance { kind: "Topped up".into(), balance: "18.40".into(), currency: "USD".into() }]);
    assert_eq!(u.credits, Some(Credits { balance: "3".into(), unlimited: false }));
    assert_eq!(u.checked_at, Some(1793000000.0));
    let value = serde_json::to_value(&u).unwrap();
    assert_eq!(value["windows"][1]["resets_at"], json!(null));
    assert_eq!(value["windows"][2]["minutes"], json!(null));
    assert_eq!(usage(&json!({"status": "ok", "credits": {}})).credits, None);
}
```

In `accounts.rs` (`#[cfg(test)] mod tests`):

```rust
use super::*;
use serde_json::json;
use std::path::Path;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/views").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn entry(computer: &str, status: &str, attempt: u32) -> FleetEntry {
    FleetEntry {
        computer: ComputerId::from(computer),
        status: status.into(),
        attempt,
        local_sessions: true,
        local_host: false,
        host: computer.is_empty().then(|| "workstation".to_owned()),
        platform: Some("linux".into()),
    }
}

fn loaded() -> AccountsState {
    let mut state = AccountsState::default();
    for (computer, file) in [("", "accounts-local.json"), ("studio", "accounts-studio.json")] {
        state.catalogs.insert(
            ComputerId::from(computer),
            CatalogSlot { requested: Some(0), data: Some(fixture(file)), ..CatalogSlot::default() },
        );
    }
    state
}

fn inspected(state: &mut AccountsState, host: &str, id: &str, home: &str, revision: &str, reply: Value) {
    state.usage.insert(
        (ComputerId::from(host), id.into(), home.into()),
        UsageSlot { auth_revision: revision.into(), data: Some(reply), ..UsageSlot::default() },
    );
}

fn signed_out() -> Value {
    json!({"id": "native-claude", "home": "/home/example/.claude", "provider": "claude", "status": "signed_out",
           "identity": {"email": "person@example.com", "account_id": "acct-1"}, "windows": [], "balances": []})
}

/// Every installed profile has answered except claude-studio, which shows its cached status.
fn with_inspects() -> AccountsState {
    let mut state = loaded();
    inspected(&mut state, "", "claude-work", "/home/example/.config/hgs/accounts/claude-work", "a1", fixture("inspect-claude-work.json"));
    inspected(&mut state, "", "native-claude", "/home/example/.claude", "b1", signed_out());
    inspected(&mut state, "", "native-codex", "/home/example/.codex", "c1", fixture("inspect-native-codex.json"));
    inspected(&mut state, "studio", "kimi-studio", "/home/example/.config/hgs/accounts/kimi-studio", "k1",
        json!({"id": "kimi-studio", "home": "/home/example/.config/hgs/accounts/kimi-studio", "provider": "kimi", "status": "ok",
               "identity": {"plan": "Pro"}, "windows": [{"id": "limit_month_total", "label": "Monthly", "used_percent": 50,
               "window_minutes": null, "resets_at": null}], "balances": []}));
    state
}

// Review Focus 2.
#[test]
fn one_identity_on_two_hosts_is_one_account_and_signed_out_or_unidentified_profiles_stay_apart() {
    let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
    let view = project(&fleet, &with_inspects(), &mut AccountsDisplay::default());
    let groups: Vec<(&str, Vec<&str>)> = view.accounts.iter()
        .map(|a| (a.label.as_str(), a.members.iter().map(|m| m.id.as_str()).collect()))
        .collect();
    assert_eq!(groups, vec![
        ("Work", vec!["claude-work", "claude-studio"]),
        ("person@example.com", vec!["native-claude"]),
        ("Default account", vec!["native-codex"]),
        ("Native DeepSeek account", vec!["native-dsh"]),
        ("Moonshot", vec!["kimi-studio"]),
    ]);
    let work = &view.accounts[0];
    assert_eq!(work.machines, ["workstation", "studio"]);
    assert_eq!(work.permission, "Varies");
    assert_eq!(work.usage.checked_at, Some(1793000300.0), "the answered inspect is the better snapshot");
    assert_eq!(work.identity_lines, ["Example Person", "person@example.com", "Plan: max", "Sign-in: oauth"]);
    assert_eq!(view.accounts[2].identity_note, "Account identity is unavailable.");
    assert_eq!(view.accounts[3].identity_note, "Reading account details…");
    // The Overview leaves out a native, unnamed profile while it loads (native-dsh).
    let overview: Vec<&str> = view.overview.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(overview, ["Work", "person@example.com", "Default account", "Moonshot"]);
}

#[test]
fn member_states_and_texts_follow_qt() {
    use MemberState::*;
    let member = |view: &AccountsView, id: &str| -> Profile {
        view.accounts.iter().flat_map(|a| &a.members).find(|m| m.id == id).unwrap().clone()
    };
    let state = |view: &AccountsView, id: &str| { let m = member(view, id); (m.state, m.detail) };
    let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
    let view = project(&fleet, &with_inspects(), &mut AccountsDisplay::default());
    assert_eq!(state(&view, "claude-work"), (Ready, "Ready on workstation.".into()));
    assert_eq!(state(&view, "native-claude"), (SignedOut, "Sign in on workstation to use this account.".into()));
    assert_eq!(state(&view, "native-codex"), (SignedOut, "Sign in on workstation to use this account.".into()));
    assert_eq!(state(&view, "native-dsh"), (NotInstalled, "DeepSeek is not installed on workstation.".into()));
    assert_eq!(state(&view, "claude-studio"), (Checking, "Checking sign-in on studio…".into()));
    assert_eq!(member(&view, "kimi-studio").state, Ready);
    assert_eq!(member(&view, "native-codex").permission_label, "Bypass");
    assert_eq!(member(&view, "claude-work").model_text, "New sessions: opus (high)");
    assert_eq!(member(&view, "claude-studio").model_text, "New sessions: Provider settings");
    assert_eq!(member(&view, "claude-work").permission_hint,
        "From provider settings\nFrom ~/.claude/settings.json\nApplies to new and resumed sessions; a running session may use a different mode.");
    // studio's stream failed: its profiles are offline, and the cached inspection
    // stands in until the first inspect answers (ruling).
    let away = [entry("", "live", 0), entry("studio", "reconnecting", 1)];
    let view = project(&away, &with_inspects(), &mut AccountsDisplay::default());
    let studio = member(&view, "claude-studio");
    assert_eq!((studio.state, studio.detail.as_str(), studio.online), (Offline, "studio is offline.", false));
    assert!(studio.usage.offline);
    assert_eq!(studio.usage.status, "offline");
    assert_eq!(studio.usage.windows.iter().map(|w| w.resets_at).collect::<Vec<_>>(), [Some(1793005200.0), Some(1793400000.0)]);
    assert_eq!(studio.usage.checked_at, Some(1793000100.0));
    assert!(member(&view, "kimi-studio").usage.offline && member(&view, "kimi-studio").state == Offline);
    // Qt's lenient rule: a peer is offline only after its stream failed.
    for (status, attempt, up) in [("reconnecting", 0, true), ("probing", 0, true), ("unsupported", 0, true), ("live", 0, true),
                                  ("reconnecting", 2, false), ("ended", 0, false)] {
        assert_eq!(online(&entry("studio", status, attempt)), up, "{status} {attempt}");
    }
    let mut console = entry("", "live", 0);
    console.local_sessions = false;
    assert!(!online(&console) && reachable(&console));
}

#[test]
fn a_control_console_has_no_account_hosts_of_its_own() {
    let mut console = entry("", "live", 0);
    console.local_sessions = false;
    assert!(hosts(std::slice::from_ref(&console)).is_empty());
    let view = project(std::slice::from_ref(&console), &AccountsState::default(), &mut AccountsDisplay::default());
    assert_eq!(view.no_hosts.as_deref(), Some("Add a remote machine in Machines to manage agent accounts; this computer runs no agents."));
    console.local_host = true;
    console.platform = Some("windows".into());
    let view = project(&[console], &AccountsState::default(), &mut AccountsDisplay::default());
    assert_eq!(view.no_hosts.as_deref(),
        Some("To manage agent accounts, enable local sessions for this computer in Machines or add a remote machine."));
    let labels: Vec<String> = hosts(&[entry("", "live", 0), entry("studio", "live", 0)]).into_iter().map(|h| h.label).collect();
    assert_eq!(labels, ["workstation (this machine)", "studio"]);
    // The local Windows computer lists neither DeepSeek nor OpenCode.
    let mut windows = entry("", "live", 0);
    windows.platform = Some("windows".into());
    let view = project(&[windows], &loaded(), &mut AccountsDisplay::default());
    assert!(view.accounts.iter().all(|a| a.provider != "dsh"));
}

#[test]
fn pending_counts_unread_hosts_and_resolving_profiles_and_the_display_holds_them_back() {
    let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
    let mut state = loaded();
    state.catalogs.remove(&ComputerId::from("studio"));
    state.catalogs.get_mut(&ComputerId::local()).unwrap().error = Some("hgs did not answer in time".into());
    let mut display = AccountsDisplay::default();
    let ids = |view: &AccountsView| -> Vec<String> {
        view.accounts.iter().flat_map(|a| a.members.iter().map(|m| m.id.clone())).collect()
    };
    let view = project(&fleet, &state, &mut display);
    // studio never answered; claude-work, native-claude and native-codex are loading.
    assert_eq!(view.pending, 4);
    assert!(view.busy && view.loading);
    assert_eq!(ids(&view), ["claude-work", "native-dsh"], "unidentified profiles wait while they resolve");
    assert!(view.snapshot);
    assert_eq!(view.loaded_machines, 1);
    assert_eq!(view.notices, ["This machine: hgs did not answer in time"]);
    inspected(&mut state, "", "native-claude", "/home/example/.claude", "b1", signed_out());
    assert_eq!(ids(&project(&fleet, &state, &mut display)), ["claude-work", "native-claude", "native-dsh"]);
    // A refresh without data yet: the previous row stays.
    let slot = state.usage.get_mut(&(ComputerId::local(), "native-claude".into(), "/home/example/.claude".into())).unwrap();
    slot.data = None;
    slot.pending = true;
    let view = project(&fleet, &state, &mut display);
    assert_eq!(ids(&view), ["claude-work", "native-claude", "native-dsh"]);
    assert_eq!(view.accounts[1].members[0].usage.status, "signed_out", "the held row");
}

#[test]
fn opencode_rows_show_its_native_configuration() {
    let raw = json!({"id": "native-opencode", "label": "OpenCode", "provider": "opencode", "native": true, "installed": true,
        "home": "/home/example/.local/share/opencode", "catalog_state": "not_running",
        "catalog_reason": "OpenCode is not running on studio.", "catalog_at": 1793000000.5, "providers_truncated": true,
        "providers": [{"id": "anthropic", "label": "Anthropic", "kind": "oauth", "credential_count": 1, "model_count": 12},
                      {"id": "openai", "label": "", "kind": "key", "credential_count": 3, "model_count": 1},
                      {"id": "anthropic", "label": "Again"}, {"id": "bad id"}]});
    let studio = ComputerId::from("studio");
    let mut state = AccountsState::default();
    state.catalogs.insert(studio.clone(),
        CatalogSlot { requested: Some(0), data: Some(json!({"profiles": [raw], "revision": "r"})), ..CatalogSlot::default() });
    state.usage.insert((studio, "native-opencode".into(), "/home/example/.local/share/opencode".into()),
        UsageSlot { data: Some(json!({"id": "native-opencode", "provider": "opencode", "status": "configured"})), ..UsageSlot::default() });
    let view = project(&[entry("studio", "live", 0)], &state, &mut AccountsDisplay::default());
    let p = &view.accounts[0].members[0];
    assert_eq!(p.detail, "OpenCode is not running on studio.");
    let oc = p.opencode.as_ref().unwrap();
    assert_eq!(oc.providers, vec![
        OpenCodeProvider { label: "Anthropic".into(), connection: "Signed in".into(), models: "12 models".into() },
        OpenCodeProvider { label: "openai".into(), connection: "API key, 3 accounts".into(), models: "1 model".into() },
    ]);
    assert!(oc.truncated && oc.stopped);
    assert_eq!(oc.stale_catalog_at, Some(1793000000.5));
}
```

- [ ] **Step 3: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core views::`. Expected: compile errors (`views` not found).

- [ ] **Step 4: Implement.** `client/core/src/lib.rs` gains `pub mod views;` after `pub mod view;`.

`client/core/src/views/mod.rs`:

```rust
//! Screen view models of the Overview, Projects, Machines and Accounts
//! screens: pure projections of `hgs` read replies (`crate::reads`) and of
//! the fleet's stream state into compact serde structs the webview renders
//! as they are. Texts that depend on the clock (ages, resets, pace) stay in
//! the frontend, which owns the clock; every other text Qt words is worded here.
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod accounts;
pub mod machines;
pub mod projects;
pub mod schedule;
pub mod usage;

/// One computer of the fleet as the screens see it: the Tauri pump's mirror
/// of the stream statuses and machine cards. An ended computer is not listed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetEntry {
    pub computer: ComputerId,
    /// `probing`, `unsupported`, `live` or `reconnecting`.
    pub status: String,
    pub attempt: u32,
    /// False only on a control console (`catalog.meta.local_sessions`).
    pub local_sessions: bool,
    /// The meta names a Windows session host (`catalog.meta.local_host`).
    pub local_host: bool,
    /// `catalog.meta.host`, once its card arrived.
    pub host: Option<String>,
    /// `catalog.meta.metrics.platform`.
    pub platform: Option<String>,
}

impl FleetEntry {
    /// The name people know the computer by: a peer's alias, this computer's host name.
    pub fn name(&self) -> String {
        match self.computer.alias() {
            Some(alias) => alias.to_owned(),
            None => self
                .host
                .clone()
                .filter(|host| !host.is_empty())
                .unwrap_or_else(|| "this computer".to_owned()),
        }
    }
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

pub(crate) fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite())
}

/// A whole non-negative number, also when sent as a float such as `300.0`.
pub(crate) fn whole(value: &Value, key: &str) -> Option<u64> {
    let found = value.get(key)?;
    found.as_u64().or_else(|| {
        found
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0)
            .map(|n| n as u64)
    })
}

pub(crate) fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
```

`views/schedule.rs`, `views/machines.rs` and `views/projects.rs` are created in Task 2. To compile this task alone, create them now with only their first line, `//! Task 2.`.

`client/core/src/views/usage.rs`:

```rust
//! Account usage as `hgs [@host] account inspect ID --refresh` reports it,
//! and the cached `account_status` of `account ls`: limit windows, balances,
//! credits and the provider's identity (tray/src/AccountUsage.h). A reset
//! arrives as Unix seconds or an ISO 8601 date (`AccountUsage::reset`,
//! AccountUsage.h:33-36); both become seconds here, so the frontend only
//! compares them with its clock.
use super::{flag, items, number, text, whole};
use serde::Serialize;
use serde_json::Value;

/// The most windows and balances one usage keeps.
const WINDOWS: usize = 16;
const BALANCES: usize = 8;

/// One limit window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Window {
    pub id: String,
    /// The provider's name ("Sonnet", "Monthly"); empty for the plain 5 h and 7 d windows.
    pub label: String,
    /// The used share as reported, normally 0 to 100.
    pub used: f64,
    /// The window length; None when the provider reports none (Kimi's monthly windows).
    pub minutes: Option<u64>,
    /// The reset in Unix seconds; None when unknown or unreadable.
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Identity {
    pub email: String,
    pub name: String,
    pub plan: String,
    pub organization: String,
    pub account_id: String,
    pub auth_method: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Balance {
    pub kind: String,
    pub balance: String,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Credits {
    pub balance: String,
    pub unlimited: bool,
}

/// One account's usage as the screens show it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    /// `ok`, `loading`, `unavailable`, `offline`, `configured`, `signed_out`,
    /// `expired`, `credentials_locked`, `desktop_session_unavailable` or
    /// `credentials_unavailable`.
    pub status: String,
    pub provider: String,
    pub label: String,
    pub auth_status: String,
    pub checked_at: Option<f64>,
    pub identity: Identity,
    pub windows: Vec<Window>,
    pub balances: Vec<Balance>,
    pub credits: Option<Credits>,
    /// An inspect runs; the values shown are the previous ones.
    pub refreshing: bool,
    /// The last inspect failed; the values shown are the previous ones.
    pub refresh_error: bool,
    /// The machine is offline.
    pub offline: bool,
}

const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(text: &str, from: usize, len: usize) -> Option<i64> {
    let part = text.get(from..from + len)?;
    part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok()).flatten()
}

/// Unix seconds of an ISO 8601 date and time: `YYYY-MM-DDTHH:MM:SS`, an
/// optional fraction, then `Z`, `+HH:MM`, `+HHMM` or nothing. A time without
/// an offset is read as UTC (the providers behind hgs send one; Qt would read
/// it as local time).
pub fn parse_iso(text: &str) -> Option<f64> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (digits(text, 0, 4)?, digits(text, 5, 2)?, digits(text, 8, 2)?);
    let (hour, minute, second) = (digits(text, 11, 2)?, digits(text, 14, 2)?, digits(text, 17, 2)?);
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let last = if month == 2 && leap(year) { 29 } else { MONTH_DAYS[month as usize - 1] };
    if !(1..=last).contains(&day) {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.0;
    if let Some(after) = rest.strip_prefix(['.', ',']) {
        let count = after.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..count]).parse().ok()?;
        rest = &after[count..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        zone => {
            let sign = match zone.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let clock = zone[1..].replace(':', "");
            if clock.len() != 4 {
                return None;
            }
            let (hours, minutes) = (digits(&clock, 0, 2)?, digits(&clock, 2, 2)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    Some(seconds as f64 + fraction)
}

/// `resets_at` as Unix seconds: a number is seconds, a string an ISO 8601 date.
pub fn reset_seconds(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|n| n.is_finite()),
        Value::String(text) => parse_iso(text),
        _ => None,
    }
}

/// One window, or None when its used share is not a number.
pub fn window(value: &Value) -> Option<Window> {
    Some(Window {
        used: number(value, "used_percent")?,
        id: text(value, "id"),
        label: text(value, "label"),
        minutes: whole(value, "window_minutes").filter(|minutes| *minutes > 0),
        resets_at: reset_seconds(value.get("resets_at")),
    })
}

/// The usage of one inspect reply, or of a cached `account_status`.
pub fn usage(value: &Value) -> Usage {
    let identity = value.get("identity").unwrap_or(&Value::Null);
    Usage {
        status: text(value, "status"),
        provider: text(value, "provider"),
        label: text(value, "label"),
        auth_status: text(value, "auth_status"),
        checked_at: number(value, "checked_at"),
        identity: Identity {
            email: text(identity, "email"),
            name: text(identity, "name"),
            plan: text(identity, "plan"),
            organization: text(identity, "organization"),
            account_id: text(identity, "account_id"),
            auth_method: text(identity, "auth_method"),
        },
        windows: items(value, "windows").iter().filter_map(window).take(WINDOWS).collect(),
        balances: items(value, "balances")
            .iter()
            .take(BALANCES)
            .map(|b| Balance { kind: text(b, "kind"), balance: text(b, "balance"), currency: text(b, "currency") })
            .collect(),
        credits: value
            .get("credits")
            .filter(|c| c.as_object().is_some_and(|o| !o.is_empty()))
            .map(|c| Credits { balance: text(c, "balance"), unlimited: flag(c, "unlimited") }),
        ..Usage::default()
    }
}
```

`client/core/src/views/accounts.rs` (the test module of Step 2 goes at its end):

```rust
//! The Accounts screen and the Overview account cards: the `account ls`
//! profiles of every host, each with its usage, grouped by the provider's
//! identity (tray/src/AccountCatalog.h), with Qt's member states and texts
//! (tray/src/AccountsPage.cpp) and the Overview's hidden-card rule
//! (tray/src/DashboardPage.cpp:348-350). When the reads run is decided in
//! [`super::schedule`].
use super::usage::{self, Identity, Usage};
use super::{flag, items, number, text, FleetEntry};
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// The most profiles a host's catalog shows.
const PROFILES_PER_HOST: usize = 256;
/// The most OpenCode providers a row lists (OpenCodeModels::providers).
const OPENCODE_PROVIDERS: usize = 64;

/// A profile's usage is kept per host, profile ID and home (AccountUsageRef::key).
pub type UsageKey = (ComputerId, String, String);

/// The profile an inspect reads (AccountUsageRef::profile).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageRef {
    pub host: ComputerId,
    pub id: String,
    pub home: String,
    pub provider: String,
    pub auth_revision: String,
}

impl UsageRef {
    pub fn key(&self) -> UsageKey {
        (self.host.clone(), self.id.clone(), self.home.clone())
    }
}

/// One host's `account ls`.
#[derive(Clone, Debug, Default)]
pub struct CatalogSlot {
    /// When the last `account ls` started, on the hub's millisecond clock.
    pub requested: Option<u64>,
    pub pending: bool,
    /// A forced refresh arrived while one ran: one follow-up runs after it.
    pub queued: bool,
    /// The last good reply; a failure keeps it.
    pub data: Option<Value>,
    /// The last failure, cleared by a good reply.
    pub error: Option<String>,
}

/// One profile's inspect (an AccountUsageStore entry).
#[derive(Clone, Debug, Default)]
pub struct UsageSlot {
    /// The credential revision the data belongs to.
    pub auth_revision: String,
    pub attempted: Option<u64>,
    pub pending: bool,
    pub failed: bool,
    pub data: Option<Value>,
}

/// Everything the account reads produced.
#[derive(Clone, Debug, Default)]
pub struct AccountsState {
    pub catalogs: BTreeMap<ComputerId, CatalogSlot>,
    pub usage: BTreeMap<UsageKey, UsageSlot>,
    pub(crate) queue: VecDeque<UsageRef>,
    pub(crate) running: usize,
    pub(crate) warmed: BTreeSet<ComputerId>,
}

/// A profile's state on its machine, as the mockup's machine tags show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberState {
    Ready,
    Offline,
    NotInstalled,
    Apikey,
    SignedOut,
    Keychain,
    Desktop,
    Unavailable,
    Expired,
    Checking,
    Unknown,
}

/// A machine that keeps agent accounts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Host {
    pub computer: ComputerId,
    pub name: String,
    /// `"<host> (this machine)"` for this computer (machineChoice, AccountsPage.cpp:178).
    pub label: String,
    pub online: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenCodeProvider {
    pub label: String,
    pub connection: String,
    pub models: String,
}

/// OpenCode's native configuration on one machine (AccountsPage.cpp:657-704).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenCode {
    pub providers: Vec<OpenCodeProvider>,
    /// "Showing the first N providers."
    pub truncated: bool,
    /// The last known catalog's time when the catalog is not ready ("Last known catalog from ...").
    pub stale_catalog_at: Option<f64>,
    /// Installed but not running ("Start OpenCode on ...").
    pub stopped: bool,
}

/// One profile on one machine.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Profile {
    /// `host + U+001F + id` (AccountCatalog::profileKey).
    pub key: String,
    pub host: ComputerId,
    pub machine: String,
    pub id: String,
    pub label: String,
    pub provider: String,
    pub native: bool,
    pub installed: bool,
    pub is_default: bool,
    pub home: String,
    pub auth_revision: String,
    pub online: bool,
    pub state: MemberState,
    /// The status line under the machine tags.
    pub detail: String,
    pub signed_in: bool,
    /// `bypass`, `auto`, `plan`, `default` or `unknown`.
    pub permission: String,
    pub permission_label: String,
    pub permission_hint: String,
    pub model_supported: bool,
    pub model_text: String,
    pub usage: Usage,
    pub opencode: Option<OpenCode>,
}

/// One account: the profiles of one identity across machines.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Account {
    pub key: String,
    pub label: String,
    pub provider: String,
    pub installed: bool,
    pub machines: Vec<String>,
    pub members: Vec<Profile>,
    /// The better snapshot of the members (betterUsage).
    pub usage: Usage,
    /// Bypass, Auto, Plan, Default, or Varies when the members differ.
    pub permission: String,
    pub identity_lines: Vec<String>,
    /// Shown without identity lines: "Reading account details…" or "Account identity is unavailable.".
    pub identity_note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AccountsView {
    pub hosts: Vec<Host>,
    pub accounts: Vec<Account>,
    /// The Overview's cards: the same grouping without the hidden native profiles.
    pub overview: Vec<Account>,
    /// A first snapshot exists ("Loading accounts…" until then).
    pub snapshot: bool,
    /// A shown profile's usage is still loading ("Loading account details…").
    pub loading: bool,
    /// Hosts and profiles still being read ("Updating… N remaining").
    pub pending: usize,
    pub loaded_machines: usize,
    /// A read runs; Refresh is disabled.
    pub busy: bool,
    pub notices: Vec<String>,
    /// Why there is no machine to list, when there is none.
    pub no_hosts: Option<String>,
}

/// What the previous projection showed: the rows held back while they
/// resolve, and whether a first snapshot exists.
#[derive(Clone, Debug, Default)]
pub struct AccountsDisplay {
    held: BTreeMap<String, Profile>,
    snapshot: bool,
}

/// AccountsPage::online: this computer unless it is a control console; a
/// peer until its stream failed (Qt counts a peer offline only after a failed poll).
pub fn online(entry: &FleetEntry) -> bool {
    if entry.computer.is_local() {
        return entry.local_sessions;
    }
    !(entry.status == "ended" || (entry.status == "reconnecting" && entry.attempt > 0))
}

/// AccountUsageStore::online: this computer always, a peer as [`online`].
pub fn reachable(entry: &FleetEntry) -> bool {
    entry.computer.is_local() || online(entry)
}

fn keeps_accounts(entry: &FleetEntry) -> bool {
    !entry.computer.is_local() || entry.local_sessions
}

/// The machines that keep agent accounts (accountHosts, AccountsPage.cpp:165-170).
pub fn hosts(fleet: &[FleetEntry]) -> Vec<Host> {
    fleet
        .iter()
        .filter(|entry| keeps_accounts(entry))
        .map(|entry| Host {
            computer: entry.computer.clone(),
            name: entry.name(),
            label: if entry.computer.is_local() { format!("{} (this machine)", entry.name()) } else { entry.name() },
            online: online(entry),
        })
        .collect()
}

/// The local Windows computer runs neither DeepSeek nor OpenCode (AccountCatalog::launchable).
fn launchable(entry: &FleetEntry, provider: &str) -> bool {
    !(entry.computer.is_local()
        && entry.platform.as_deref() == Some("windows")
        && matches!(provider, "dsh" | "opencode"))
}

fn usage_ref(host: &ComputerId, raw: &Value) -> UsageRef {
    UsageRef {
        host: host.clone(),
        id: text(raw, "id"),
        home: text(raw, "home"),
        provider: text(raw, "provider"),
        auth_revision: text(raw, "auth_revision"),
    }
}

/// The installed profiles an inspect reads, with whether their host is reachable.
pub fn refs(fleet: &[FleetEntry], state: &AccountsState) -> Vec<(UsageRef, bool)> {
    let mut out = Vec::new();
    for entry in fleet.iter().filter(|entry| keeps_accounts(entry)) {
        let Some(data) = state.catalogs.get(&entry.computer).and_then(|slot| slot.data.as_ref()) else {
            continue;
        };
        for raw in items(data, "profiles").iter().take(PROFILES_PER_HOST) {
            let r = usage_ref(&entry.computer, raw);
            if flag(raw, "installed") && !r.id.is_empty() && launchable(entry, &r.provider) {
                out.push((r, reachable(entry)));
            }
        }
    }
    out
}

pub(crate) fn provider_name(provider: &str) -> String {
    match provider {
        "claude" => "Claude",
        "codex" => "Codex",
        "kimi" => "Kimi",
        "opencode" => "OpenCode",
        "dsh" => "DeepSeek",
        "sh" => "Shell",
        other => other,
    }
    .to_owned()
}

/// AccountUsageStore::data plus the seeding of AccountsPage::profiles.
fn profile_usage(entry: &FleetEntry, raw: &Value, r: &UsageRef, state: &AccountsState) -> Usage {
    let slot = state.usage.get(&r.key());
    // A snapshot of another credential revision is not this profile's any more.
    let data = slot
        .filter(|slot| r.auth_revision.is_empty() || slot.auth_revision == r.auth_revision)
        .and_then(|slot| slot.data.as_ref());
    let reachable = reachable(entry);
    let mut u = match data {
        Some(data) => usage::usage(data),
        None => Usage {
            status: if reachable { "loading" } else { "offline" }.to_owned(),
            ..Usage::default()
        },
    };
    if u.provider.is_empty() {
        u.provider = r.provider.clone();
    }
    u.label = text(raw, "label");
    u.refreshing = slot.is_some_and(|slot| slot.pending);
    u.refresh_error = slot.is_some_and(|slot| slot.failed);
    u.offline = !reachable;
    if let Some(cached) = raw.get("account_status").filter(|cached| cached.is_object()) {
        let cached = usage::usage(cached);
        if data.is_none() {
            // Until the first inspect answers, the cached inspection stands in
            // (owner ruling; Qt seeds identity and checked_at only).
            u.identity = cached.identity;
            u.checked_at = cached.checked_at;
            u.windows = cached.windows;
            u.balances = cached.balances;
            u.credits = cached.credits;
        } else if matches!(u.status.as_str(), "unavailable" | "offline") && u.identity == Identity::default() {
            u.identity = cached.identity;
            u.checked_at = cached.checked_at;
        }
    }
    u
}

/// The state and status line of a profile (AccountsPage.cpp:637-654).
fn member(online: bool, installed: bool, provider: &str, u: &Usage, machine: &str) -> (MemberState, String, bool) {
    use MemberState::*;
    let status = u.status.as_str();
    let signed_in = status == "ok" || status == "expired" || u.auth_status == "signed_in";
    let (state, detail) = if !online {
        (Offline, format!("{machine} is offline."))
    } else if !installed {
        (NotInstalled, format!("{} is not installed on {machine}.", provider_name(provider)))
    } else {
        match status {
            "configured" => (Apikey, format!("API key configured on {machine}.")),
            "signed_out" if provider == "dsh" => (SignedOut, format!("Set a DeepSeek API key on {machine} to use API models.")),
            "signed_out" => (SignedOut, format!("Sign in on {machine} to use this account.")),
            "credentials_locked" => (Keychain, format!("The login Keychain on {machine} is locked. Unlock it in Keychain Access, then refresh.")),
            "desktop_session_unavailable" => (Desktop, format!("Desktop sign-in on {machine} is unavailable. Log in to macOS and check the HGS installation.")),
            "credentials_unavailable" => (Unavailable, format!("The saved sign-in on {machine} could not be used. Check Keychain access or sign in again.")),
            "expired" => (Expired, format!("Sign-in expired on {machine}.")),
            "loading" => (Checking, format!("Checking sign-in on {machine}…")),
            _ if signed_in => (Ready, format!("Ready on {machine}.")),
            _ => (Unknown, format!("Account status is unavailable on {machine}.")),
        }
    };
    (state, detail, signed_in)
}

/// permissionMode (AccountsPage.cpp:72-74).
fn permission_mode(raw: &Value) -> String {
    if text(raw, "permission_mode") == "bypass" {
        return "bypass".into();
    }
    Some(text(raw, "effective_permission_mode")).filter(|mode| !mode.is_empty()).unwrap_or_else(|| "unknown".into())
}

/// permissionLabel (AccountsPage.cpp:75-78).
pub fn permission_label(mode: &str) -> &'static str {
    match mode {
        "bypass" => "Bypass",
        "auto" => "Auto",
        "plan" => "Plan",
        "mixed" => "Varies",
        _ => "Default",
    }
}

/// permissionHint (AccountsPage.cpp:89-92).
fn permission_hint(raw: &Value) -> String {
    let source = if text(raw, "permission_mode") == "bypass" { "Set in Zerus" } else { "From provider settings" };
    let detail = Some(text(raw, "permission_detail"))
        .filter(|detail| !detail.is_empty())
        .unwrap_or_else(|| "Provider defaults have not been verified.".into());
    format!("{source}\n{detail}\nApplies to new and resumed sessions; a running session may use a different mode.")
}

/// The model line (AccountsPage.cpp:610-612), with the effort in parentheses (the dots rule).
fn model_text(raw: &Value) -> String {
    let defaults = raw.get("model_defaults").unwrap_or(&Value::Null);
    let (model, effort) = (text(defaults, "model"), text(defaults, "effort"));
    match (model.is_empty(), effort.is_empty()) {
        (true, _) => "New sessions: Provider settings".into(),
        (false, true) => format!("New sessions: {model}"),
        (false, false) => format!("New sessions: {model} ({effort})"),
    }
}

/// OpenCodeModels::kindText (OpenCodeModels.cpp:115-130).
fn kind_text(kind: &str) -> &'static str {
    match kind {
        "free" => "Free, no key",
        "key" => "API key",
        "oauth" => "Signed in",
        "env" => "Environment variable",
        "config" => "Configuration",
        "command" => "Command",
        "external" => "External credentials",
        "local" => "Detected locally",
        _ => "Connected",
    }
}

fn short(value: &Value, key: &str, limit: usize) -> String {
    text(value, key).chars().filter(|c| !c.is_control()).take(limit).collect()
}

/// OpenCode's providers (OpenCodeModels::providers, OpenCodeModels.cpp:64-80):
/// distinct valid IDs, at most 64; a provider ID here is 1 to 128 bytes
/// without spaces or control characters.
fn opencode(raw: &Value) -> Option<OpenCode> {
    if text(raw, "provider") != "opencode" {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut providers = Vec::new();
    for row in items(raw, "providers") {
        if providers.len() >= OPENCODE_PROVIDERS {
            break;
        }
        let id = text(row, "id");
        if id.is_empty() || id.len() > 128 || id.chars().any(|c| c.is_control() || c.is_whitespace()) || !seen.insert(id.clone()) {
            continue;
        }
        let label = Some(short(row, "label", 160)).filter(|label| !label.is_empty()).unwrap_or(id);
        let active = short(row, "active_label", 160);
        let credentials = row.get("credential_count").and_then(Value::as_i64).unwrap_or(-1);
        let mut connection = kind_text(&short(row, "kind", 32)).to_owned();
        if !active.is_empty() {
            connection.push_str(&format!(", {active}"));
        } else if credentials > 1 {
            connection.push_str(&format!(", {credentials} accounts"));
        }
        let models = match row.get("model_count").and_then(Value::as_i64).unwrap_or(-1) {
            count if count < 0 => String::new(),
            1 => "1 model".into(),
            count => format!("{count} models"),
        };
        providers.push(OpenCodeProvider { label, connection, models });
    }
    let state = text(raw, "catalog_state");
    let at = number(raw, "catalog_at").filter(|at| *at > 0.0 && *at < 253_402_300_800.0);
    Some(OpenCode {
        providers,
        truncated: flag(raw, "providers_truncated"),
        stale_catalog_at: if state != "ready" { at } else { None },
        stopped: flag(raw, "installed") && state == "not_running",
    })
}

fn profile(entry: &FleetEntry, raw: &Value, state: &AccountsState) -> Profile {
    let r = usage_ref(&entry.computer, raw);
    let machine = entry.name();
    let installed = flag(raw, "installed");
    let online = online(entry);
    let usage = profile_usage(entry, raw, &r, state);
    let (member_state, mut detail, signed_in) = member(online, installed, &r.provider, &usage, &machine);
    if r.provider == "opencode" {
        // OpenCode keeps its own configuration (AccountsPage.cpp:657-663).
        let catalog = text(raw, "catalog_state");
        let reason = if !installed {
            text(raw, "install_reason")
        } else if !catalog.is_empty() && catalog != "ready" {
            text(raw, "catalog_reason")
        } else {
            String::new()
        };
        detail = if !online {
            format!("{machine} is offline.")
        } else if !reason.is_empty() {
            reason
        } else if !installed {
            format!("OpenCode is not installed on {machine}.")
        } else {
            format!("Configure providers, models and permissions with OpenCode on {machine}. Zerus uses that native configuration.")
        };
    }
    let permission = permission_mode(raw);
    Profile {
        key: format!("{}\u{1f}{}", entry.computer.0, r.id),
        host: entry.computer.clone(),
        machine,
        label: text(raw, "label"),
        native: flag(raw, "native"),
        installed,
        is_default: flag(raw, "is_default"),
        online,
        state: member_state,
        detail,
        signed_in,
        permission_label: permission_label(&permission).into(),
        permission_hint: permission_hint(raw),
        permission,
        model_supported: flag(raw, "model_defaults_supported"),
        model_text: model_text(raw),
        usage,
        opencode: opencode(raw),
        id: r.id,
        home: r.home,
        provider: r.provider,
        auth_revision: r.auth_revision,
    }
}

/// AccountCatalog::named.
fn named(label: &str) -> bool {
    !label.is_empty() && label != "Default account" && label != "Native DeepSeek account"
}

/// AccountCatalog::key, with or without the account ID.
fn identity_key(p: &Profile, with_account: bool) -> String {
    let identity = &p.usage.identity;
    let email = identity.email.trim().to_lowercase();
    let account = if with_account { identity.account_id.as_str() } else { "" };
    if p.usage.status == "signed_out" || (email.is_empty() && account.is_empty()) {
        return format!("profile:{}", p.key);
    }
    let who = if account.is_empty() { format!("email:{email}") } else { format!("id:{account}") };
    format!("account:{}\n{who}\n{}", p.provider, identity.organization.trim().to_lowercase())
}

fn quality(u: &Usage) -> u8 {
    match u.status.as_str() {
        "ok" | "configured" if u.offline => 2,
        "ok" | "configured" => 3,
        _ => 1,
    }
}

/// AccountCatalog::betterUsage: a quota belongs to the account; pick a snapshot, never add them up.
fn better(usage: &Usage, previous: &Usage) -> bool {
    quality(usage) > quality(previous)
        || (quality(usage) == quality(previous) && usage.checked_at.unwrap_or(0.0) > previous.checked_at.unwrap_or(0.0))
}

/// accountPermission (AccountsPage.cpp:80-88).
fn account_permission(members: &[Profile]) -> String {
    let mut mode = String::new();
    for member in members {
        if !mode.is_empty() && mode != member.permission {
            return "mixed".into();
        }
        mode = member.permission.clone();
    }
    mode
}

/// The identity block (AccountsPage.cpp:625-636).
fn identity_lines(title: &str, u: &Usage) -> (Vec<String>, String) {
    let identity = &u.identity;
    let mut lines = Vec::new();
    let mut seen = vec![title.trim().to_lowercase()];
    for field in [&identity.name, &identity.email, &identity.organization] {
        let text = field.trim();
        if !text.is_empty() && !seen.contains(&text.to_lowercase()) {
            lines.push(text.to_owned());
            seen.push(text.to_lowercase());
        }
    }
    if !identity.plan.is_empty() {
        lines.push(format!("Plan: {}", identity.plan));
    }
    if !identity.auth_method.is_empty() && identity.auth_method != "none" {
        lines.push(format!("Sign-in: {}", identity.auth_method));
    }
    let note = if !lines.is_empty() || *identity != Identity::default() {
        String::new()
    } else if u.status == "loading" {
        "Reading account details…".into()
    } else {
        "Account identity is unavailable.".into()
    };
    (lines, note)
}

/// AccountCatalog::group (AccountCatalog.h:37-79).
pub fn group(profiles: &[Profile]) -> Vec<Account> {
    // An email-only profile can join a known ID only when that email and
    // scope resolve to one account.
    let mut known: HashMap<String, BTreeSet<String>> = HashMap::new();
    for p in profiles {
        let identity = &p.usage.identity;
        if identity.account_id.is_empty() || identity.email.is_empty() || p.usage.status == "signed_out" {
            continue;
        }
        known.entry(identity_key(p, false)).or_default().insert(identity_key(p, true));
    }
    let mut groups: Vec<Account> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for p in profiles {
        let mut key = identity_key(p, true);
        if let Some(only) = known.get(&key).filter(|ids| ids.len() == 1).and_then(|ids| ids.iter().next()) {
            key = only.clone();
        }
        let at = *positions.entry(key.clone()).or_insert_with(|| {
            groups.push(Account {
                key: key.clone(),
                label: p.label.clone(),
                provider: p.provider.clone(),
                installed: false,
                machines: Vec::new(),
                members: Vec::new(),
                usage: p.usage.clone(),
                permission: String::new(),
                identity_lines: Vec::new(),
                identity_note: String::new(),
            });
            groups.len() - 1
        });
        let group = &mut groups[at];
        if !group.machines.contains(&p.machine) {
            group.machines.push(p.machine.clone());
        }
        if better(&p.usage, &group.usage) {
            group.usage = p.usage.clone();
        }
        if !named(&group.label) && named(&p.label) {
            group.label = p.label.clone();
        }
        group.installed |= p.installed;
        group.members.push(p.clone());
    }
    for group in &mut groups {
        if !named(&group.label) && !group.usage.identity.email.is_empty() {
            group.label = group.usage.identity.email.clone();
        }
        group.permission = permission_label(&account_permission(&group.members)).into();
        (group.identity_lines, group.identity_note) = identity_lines(&group.label, &group.usage);
    }
    groups
}

/// A native profile awaiting its identity is not another account on the
/// Overview (DashboardPage.cpp:348-350).
fn hidden_on_overview(p: &Profile) -> bool {
    p.native
        && !named(&p.label)
        && identity_key(p, true).starts_with("profile:")
        && matches!(p.usage.status.as_str(), "loading" | "unavailable" | "offline")
}

fn resolving(p: &Profile) -> bool {
    p.installed && p.online && (p.usage.refreshing || p.usage.status == "loading")
}

/// The Accounts screen and the Overview cards from the fleet and the reads so far.
pub fn project(fleet: &[FleetEntry], state: &AccountsState, display: &mut AccountsDisplay) -> AccountsView {
    let hosts = hosts(fleet);
    let mut all = Vec::new();
    for entry in fleet.iter().filter(|entry| keeps_accounts(entry)) {
        let Some(data) = state.catalogs.get(&entry.computer).and_then(|slot| slot.data.as_ref()) else {
            continue;
        };
        for raw in items(data, "profiles").iter().take(PROFILES_PER_HOST) {
            if launchable(entry, &text(raw, "provider")) {
                all.push(profile(entry, raw, state));
            }
        }
    }
    // pendingAccounts (AccountsPage.cpp:419-433).
    let mut pending = hosts
        .iter()
        .filter(|host| state.catalogs.get(&host.computer).is_none_or(|slot| slot.requested.is_none() || slot.pending))
        .count();
    pending += all.iter().filter(|p| resolving(p)).count();
    // Identities appear as they become known, without flashing unresolved
    // duplicates while another machine is still being inspected.
    let shown: Vec<Profile> = all
        .into_iter()
        .filter_map(|p| {
            let identified = !p.usage.identity.account_id.is_empty() || !p.usage.identity.email.trim().is_empty();
            if !resolving(&p) || identified { Some(p) } else { display.held.get(&p.key).cloned() }
        })
        .collect();
    if !shown.is_empty() || pending == 0 {
        display.snapshot = true;
    }
    display.held = shown.iter().map(|p| (p.key.clone(), p.clone())).collect();
    let loaded: BTreeSet<&ComputerId> = shown.iter().map(|p| &p.host).collect();
    let visible: Vec<Profile> = shown.iter().filter(|p| !hidden_on_overview(p)).cloned().collect();
    let notices = hosts
        .iter()
        .filter_map(|host| {
            let error = state.catalogs.get(&host.computer)?.error.as_ref()?;
            let name = if host.computer.is_local() { "This machine".to_owned() } else { host.name.clone() };
            Some(format!("{name}: {error}"))
        })
        .collect();
    let console = fleet.iter().find(|entry| entry.computer.is_local());
    let can_enable = console
        .is_some_and(|l| !l.local_sessions && l.local_host && l.platform.as_deref() == Some("windows"));
    let no_hosts = hosts.is_empty().then(|| {
        if can_enable {
            "To manage agent accounts, enable local sessions for this computer in Machines or add a remote machine."
        } else {
            "Add a remote machine in Machines to manage agent accounts; this computer runs no agents."
        }
        .to_owned()
    });
    AccountsView {
        loading: shown.iter().any(|p| p.usage.status == "loading"),
        busy: pending > 0 || state.catalogs.values().any(|slot| slot.pending),
        accounts: group(&shown),
        overview: group(&visible),
        snapshot: display.snapshot,
        pending,
        loaded_machines: loaded.len(),
        notices,
        no_hosts,
        hosts,
    }
}
```

- [ ] **Step 5: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core views::`, then `cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core --all-targets -- -D warnings` and `cargo +1.99.1 fmt --manifest-path client/core/Cargo.toml --check`. Expected: PASS, no warnings (run `cargo fmt` first if the check reports formatting).

- [ ] **Step 6: Commit**

```bash
git add client/core/src/lib.rs client/core/src/views client/core/tests/fixtures/views
git commit -m "feat(client): usage windows and the accounts projection for the screens"
```

---

### Task 2: Read timing, Machines and Projects views, and the console fields of `MachineMeta`

**Files:**
- Create (replacing the Task 1 one-line stubs): `client/core/src/views/schedule.rs`, `client/core/src/views/machines.rs`, `client/core/src/views/projects.rs`
- Create: `client/core/tests/fixtures/views/machines.json`, `resolve-studio.json`, `check-studio.json`, `swarm.json`, `swarm-old.json`
- Modify: `client/core/src/machine.rs` (PR #391): `local_sessions`, `local_host`
- Test: unit tests in the three new files and in `machine.rs`

**Interfaces:**
- Consumes: `AccountsState`, `CatalogSlot`, `UsageSlot`, `UsageRef` and the helpers of `views/mod.rs` (Task 1); `MachineMeta::project` (PR #391).
- Produces:

```rust
// views/schedule.rs
pub const CATALOG_INTERVAL: u64 = 30_000;  pub const USAGE_INTERVAL: u64 = 300_000;
pub const USAGE_CONCURRENCY: usize = 2;    pub const SWARM_INTERVAL: u64 = 5_000;
pub enum Screen { Overview, Projects, Machines, Accounts }      // snake_case; name() -> &'static str
pub struct Poll;  impl Poll { pub fn new(interval: u64) -> Self; pub fn due(&mut self, now: u64, force: bool) -> bool;
                              pub fn finish(&mut self); pub fn pending(&self) -> bool }
impl AccountsState {
    pub fn ensure_catalogs(&mut self, hosts: &[ComputerId], now: u64, force: bool) -> Vec<ComputerId>;
    pub fn warm(&mut self, hosts: &[ComputerId], now: u64) -> Vec<ComputerId>;
    pub fn finish_catalog(&mut self, host: &ComputerId, reply: Result<Value, String>, now: u64, hosts: &[ComputerId]) -> bool;
    pub fn ensure_usage(&mut self, r: &UsageRef, reachable: bool, now: u64, force: bool);
    pub fn dispatch(&mut self, reachable: &dyn Fn(&ComputerId) -> bool) -> Vec<UsageRef>;
    pub fn finish_usage(&mut self, r: &UsageRef, reply: Option<Value>);
    pub fn reading(&self) -> bool;
}
// views/machines.rs
pub struct MachineRow { alias, enabled, source, inherited, hostname, user, port: Option<u64>, identity_file, origin }
pub struct Effective { alias, pending, error, text, hostname, user, port, key }
pub struct CheckResult { alias, pending, error, text }
pub struct MachinesView { loaded, refreshing, error: Option<String>, local, environment_override, machines: Vec<MachineRow>,
                          effective: Option<Effective>, check: Option<CheckResult>, busy }
pub struct MachinesState;  impl MachinesState { start_list(now) -> bool, finish_list(reply), start_resolve(alias) -> bool,
    finish_resolve(alias, reply), start_check(alias) -> bool, finish_check(alias, reply), busy() -> bool, view() -> MachinesView }
// views/projects.rs
pub struct Folder { id, name, path, computer, machine }
pub struct Project { id, name, color: Option<String>, accessible, is_default, palette: usize, folders: Vec<Folder> }
pub struct ProjectsView { loaded, refreshing, status, status_error, conflicts: usize, unconnected: Vec<String>,
                          notice: Option<String>, projects: Vec<Project> }
pub struct SwarmState { pub poll: Poll, /* last valid snapshot, status */ }   // Default polls every 5 s
impl SwarmState { pub fn finish(&mut self, reply: Result<Value, String>); pub fn view(&self, peers: &[String]) -> ProjectsView }
pub fn leaf(path: &str) -> String;
// machine.rs
pub struct MachineMeta { ..., pub local_sessions: Option<bool>, pub local_host: Option<LocalHost> }
pub struct LocalHost { pub state, pub conpty, pub integrity, pub error: Option<String>, pub pid, pub windows_session: Option<u64> }
```

Rules:
- **Account catalogs** (`ensureCatalogs`, `requestQueuedCatalog`, `AccountsPage.cpp:450-468`):
  - At most one `account ls` per host runs, again 30 s after the last start, or at once when forced.
  - A forced refresh during a running one queues exactly one follow-up.
  - A failure keeps the last catalog and records the error.
  - `warm` reads each newly listed host once, visible or not (`setFleet`, 378-381).
- **Inspects** (`AccountUsageStore.cpp:24-64`):
  - One attempt per profile per 5 minutes, unless forced or its `auth_revision` changed; a changed revision drops the old data.
  - At most two run at once, and an unreachable host is skipped.
  - A reply must carry the profile's `id` and `home` (and its `provider` when it names one) and a `status`. Anything else, a failed read and `status: unavailable` are failures.
  - A failure keeps the previous data, or stores `{id, provider, status: "unavailable"}` when there was none.
- **Polls:** a `Poll` starts when none runs and `interval` passed since the last start (or when forced). Projects uses it every 5 s; the Machines list uses it forced only (on show and Refresh).
- **Machines** (`MachinesPage.cpp`):
  - The peer origin text (337-338) is followed by the `HGS_PEERS` note.
  - The effective connection text (364-372) has the pending text "Reading effective SSH connection…" and the failure text (218).
  - The check texts (201-211) have the pending text "Testing SSH and checking HGS…" (431).
  - A resolve for a newer selection replaces an older one, and an older reply is dropped.
  - A check runs only while nothing else of the page runs.
- **Projects** (`SwarmController.cpp:191-219`, `ProjectsDialog.cpp:251-263, 300-303`):
  - `schema != 1` or no `organization` gives "Project sync needs an updated HGS installation." (error). A failed read gives "Project sync unavailable: {error}" (error). Either keeps the last good snapshot.
  - Status precedence: conflicts ("N project conflict(s) need review in Swarm.", error), then unavailable peers ("N connection(s) unavailable. Changes are saved here and will sync when reachable.", error), then connected ("Project sync connected to N computer(s)"), else "Local catalog. Connect another computer to sync projects.".
  - The order follows `project_order`, then the listed order.
  - The folder name is `name`, else the path's last part; the Computer column is `machine_name`, else `machine`.
  - Unconnected computers are the fleet's peers that are not keys of `peers`, listed only once `node_id` is known.

- [ ] **Step 1: Write the fixtures** (`client/core/tests/fixtures/views/`, the shapes of `src/machines.rs:218-338` and `src/swarm/model.rs:333-336`).

`machines.json`:

```json
{"environment_override":false,"local":"workstation","revision":"rev-3","machines":[
 {"alias":"studio","connection":{"enabled":true,"hostname":"studio.example","identity_file":"","port":2222,"user":"person"},"enabled":true,"inherited":false,"source":"local"},
 {"alias":"build-01","connection":{},"enabled":true,"inherited":true,"source":"config"},
 {"alias":"old-mac","connection":{"enabled":false},"enabled":false,"inherited":false,"source":"local"}]}
```

`resolve-studio.json`:

```json
{"alias":"studio","effective":{"identity_files":["/home/example/.ssh/id_ed25519","/home/example/.ssh/id_rsa"],"proxy_command":false,"hostname":"studio.example","user":"person","port":"2222","proxy_jump":"bastion.example.net"}}
```

`check-studio.json`:

```json
{"alias":"studio","ok":false,"ssh_ok":true,"os":"Linux","arch":"x86_64","hgs":"hgs 1.56.0","tmux":"","cargo":"","error":""}
```

`swarm.json`:

```json
{"schema":1,"node_id":"node-1","swarm_id":"swarm-1","digest":"d1","initialized":true,"conflicts":[{"key":"k1","field":"name","selected":null,"variants":[]}],
 "machines":[{"connection":null,"id":"m-1","local":true,"name":"workstation"}],
 "peers":{"studio":{"last_sync":1793000000,"node":"node-2"},"mac":{"error":"ssh: connect to host mac.example.net port 22: Connection timed out","last_sync":1792990000,"node":"node-3"}},
 "organization":{"version":2,"default_project":"ungrouped","project_order":["sample","ungrouped"],"projects":[
  {"id":"ungrouped","name":"Inbox","color":"","accessible":true,"sessions":[],"folders":[]},
  {"id":"remote-only","name":"Studio renders","color":"#7a5cff","accessible":false,"sessions":[],"folders":[]},
  {"id":"sample","name":"Sample project","color":"#3fb950","accessible":true,"sessions":["claude/sample-project/review"],"folders":[
   {"id":"f1","name":"","path":"/home/example/src/sample-project","machine":"workstation","machine_id":"m-1","machine_name":""},
   {"id":"f2","name":"Docs","path":"C:\\Users\\example\\docs","machine":"win-lab","machine_id":"m-4","machine_name":"Windows lab"}]}]},
 "preferences":{"mobile_delivery":"auto"},"versions":{},"worker":{"running":false,"current":null},"written":{}}
```

`swarm-old.json`:

```json
{"schema":2,"machines":[],"digest":"d0"}
```

- [ ] **Step 2: Write the failing tests.**

In `schedule.rs` (`#[cfg(test)] mod tests { use super::*; use serde_json::json; ... }`):

```rust
fn r(id: &str) -> UsageRef {
    UsageRef { host: ComputerId::local(), id: id.into(), home: format!("/home/example/{id}"), provider: "claude".into(), auth_revision: "a1".into() }
}

fn reply(id: &str, status: &str) -> Value {
    json!({"id": id, "home": format!("/home/example/{id}"), "provider": "claude", "status": status})
}

fn ids(refs: &[UsageRef]) -> Vec<&str> {
    refs.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn account_catalogs_run_once_per_host_every_thirty_seconds_with_one_forced_follow_up() {
    let hosts = [ComputerId::local(), ComputerId::from("studio")];
    let mut s = AccountsState::default();
    assert_eq!(s.ensure_catalogs(&hosts, 0, false), hosts);
    assert!(s.ensure_catalogs(&hosts, 40_000, false).is_empty(), "one in flight per host");
    assert!(s.ensure_catalogs(&hosts[..1], 40_000, true).is_empty(), "a forced refresh waits for the one running");
    let catalog = json!({"profiles": [], "revision": "r1"});
    assert!(s.finish_catalog(&hosts[0], Ok(catalog.clone()), 41_000, &hosts), "the queued follow-up starts");
    assert!(!s.finish_catalog(&hosts[0], Err("hgs did not answer in time".into()), 42_000, &hosts));
    assert_eq!(s.catalogs[&hosts[0]].data, Some(catalog), "a failure keeps the last catalog");
    assert_eq!(s.catalogs[&hosts[0]].error.as_deref(), Some("hgs did not answer in time"));
    assert!(!s.finish_catalog(&hosts[1], Ok(json!({"profiles": [], "revision": "r2"})), 42_000, &hosts));
    assert_eq!(s.ensure_catalogs(&hosts, 60_000, false), [hosts[1].clone()], "this computer read at 41 s, studio at 0 s");
    let mac = [ComputerId::from("mac")];
    assert_eq!(s.warm(&mac, 60_000), mac);
    s.finish_catalog(&mac[0], Ok(json!({"profiles": []})), 61_000, &mac);
    assert!(s.warm(&mac, 200_000).is_empty(), "a host is warmed once");
    assert!(s.reading(), "studio's second read still runs");
}

// Review Focus 4: slow and failing inspects.
#[test]
fn inspects_run_two_at_a_time_and_a_failure_keeps_its_data_and_waits_five_minutes() {
    let mut s = AccountsState::default();
    let all = |_: &ComputerId| true;
    for id in ["a", "b", "c"] {
        s.ensure_usage(&r(id), true, 0, false);
    }
    assert_eq!(ids(&s.dispatch(&all)), ["a", "b"]);
    assert!(s.dispatch(&all).is_empty(), "at most two at once");
    // a times out (the read fails); c takes its place while b is still slow.
    s.finish_usage(&r("a"), None);
    assert_eq!(ids(&s.dispatch(&all)), ["c"]);
    s.finish_usage(&r("b"), Some(reply("b", "ok")));
    s.finish_usage(&r("c"), Some(reply("c", "ok")));
    let a = &s.usage[&r("a").key()];
    assert!(a.failed && !a.pending);
    assert_eq!(a.data, Some(json!({"id": "a", "provider": "claude", "status": "unavailable"})));
    // Nothing retries inside five minutes unless forced.
    s.ensure_usage(&r("a"), true, 299_999, false);
    assert!(s.dispatch(&all).is_empty());
    // b's next reply names another home: a failure that keeps b's data.
    s.ensure_usage(&r("b"), true, 300_000, false);
    assert_eq!(ids(&s.dispatch(&all)), ["b"]);
    s.finish_usage(&r("b"), Some(json!({"id": "b", "home": "/home/example/elsewhere", "provider": "claude", "status": "ok"})));
    assert!(s.usage[&r("b").key()].failed);
    assert_eq!(s.usage[&r("b").key()].data, Some(reply("b", "ok")));
    // An unavailable reply is a failure too, and keeps the earlier data.
    s.ensure_usage(&r("c"), true, 1, true);
    assert_eq!(ids(&s.dispatch(&all)), ["c"]);
    s.finish_usage(&r("c"), Some(reply("c", "unavailable")));
    assert!(s.usage[&r("c").key()].failed);
    assert_eq!(s.usage[&r("c").key()].data, Some(reply("c", "ok")));
    // A forced refresh runs at once.
    s.ensure_usage(&r("a"), true, 299_999, true);
    assert_eq!(ids(&s.dispatch(&all)), ["a"]);
    assert!(s.reading());
}

#[test]
fn a_new_credential_revision_reads_at_once_and_an_unreachable_host_is_skipped() {
    let mut s = AccountsState::default();
    s.ensure_usage(&r("a"), true, 0, false);
    s.dispatch(&|_: &ComputerId| true);
    s.finish_usage(&r("a"), Some(reply("a", "ok")));
    let mut next = r("a");
    next.auth_revision = "a2".into();
    s.ensure_usage(&next, true, 1_000, false);
    assert_eq!(s.usage[&next.key()].data, None, "the old revision's data is dropped");
    assert_eq!(ids(&s.dispatch(&|_: &ComputerId| true)), ["a"]);
    s.finish_usage(&next, Some(reply("a", "ok")));
    s.ensure_usage(&r("b"), false, 0, false);
    assert!(s.dispatch(&|_: &ComputerId| true).is_empty(), "an offline host is never asked");
    s.ensure_usage(&r("c"), true, 0, false);
    assert!(s.dispatch(&|_: &ComputerId| false).is_empty(), "a host that went offline before its turn is skipped");
    assert!(!s.usage[&r("c").key()].pending);
    assert!(!s.reading());
}

// Review Focus 5 (the timing half; the hub test in Task 3 covers visibility).
#[test]
fn the_swarm_poll_runs_every_five_seconds_and_never_twice_at_once() {
    let mut poll = Poll::new(SWARM_INTERVAL);
    assert!(poll.due(0, false));
    assert!(!poll.due(6_000, false), "one runs");
    assert!(!poll.due(6_000, true), "a forced read waits for the one running");
    poll.finish();
    assert!(poll.due(6_000, false));
    poll.finish();
    assert!(!poll.due(10_999, false));
    assert!(poll.due(11_000, false));
    poll.finish();
    assert!(poll.due(11_001, true), "forced: at once");
    assert_eq!(serde_json::to_value(Screen::Overview).unwrap(), json!("overview"));
    assert_eq!(Screen::Projects.name(), "projects");
}
```

In `machines.rs` (`#[cfg(test)] mod tests`):

```rust
use super::*;
use std::path::Path;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/views").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn rows_carry_the_saved_overrides_and_qt_origin_texts() {
    let mut state = MachinesState::default();
    assert!(state.start_list(0));
    assert!(!state.start_list(10), "one list read at a time");
    assert!(state.view().refreshing && state.view().busy);
    state.finish_list(Ok(fixture("machines.json")));
    let view = state.view();
    assert!(view.loaded && !view.refreshing && view.error.is_none());
    assert_eq!(view.local, "workstation");
    let aliases: Vec<&str> = view.machines.iter().map(|m| m.alias.as_str()).collect();
    assert_eq!(aliases, ["studio", "build-01", "old-mac"]);
    let studio = &view.machines[0];
    assert_eq!((studio.hostname.as_str(), studio.user.as_str(), studio.port, studio.enabled), ("studio.example", "person", Some(2222), true));
    assert_eq!(studio.origin, "SSH supplies inherited values from your OpenSSH configuration and defaults. Fields below are Zerus overrides; blank fields keep the inherited value.");
    assert!(view.machines[1].origin.starts_with("This alias comes from HGS configuration. SSH supplies"));
    assert!(!view.machines[2].enabled);
    let mut overridden = fixture("machines.json");
    overridden["environment_override"] = serde_json::json!(true);
    state.start_list(20);
    state.finish_list(Ok(overridden));
    assert!(state.view().machines[0].origin.ends_with(" HGS_PEERS in the environment overrides the list of monitored machines."));
    state.start_list(30);
    state.finish_list(Err("machine settings are locked".into()));
    let view = state.view();
    assert_eq!((view.machines.len(), view.error.as_deref()), (3, Some("machine settings are locked")), "a failure keeps the rows");
}

#[test]
fn the_effective_connection_follows_the_latest_selection() {
    let mut state = MachinesState::default();
    assert!(state.start_resolve("build-01"));
    assert!(state.start_resolve("studio"), "a new selection replaces the pending one");
    assert!(!state.start_resolve("studio"));
    assert_eq!(state.view().effective.unwrap().text, "Reading effective SSH connection…");
    state.finish_resolve("build-01", Ok(fixture("resolve-studio.json")));
    assert!(state.view().effective.unwrap().pending, "an older selection's reply is dropped");
    state.finish_resolve("studio", Ok(fixture("resolve-studio.json")));
    let effective = state.view().effective.unwrap();
    assert_eq!(effective.text,
        "Effective connection: person@studio.example, port 2222\nKey paths: /home/example/.ssh/id_ed25519, /home/example/.ssh/id_rsa\nJump host: bastion.example.net");
    assert_eq!((effective.hostname.as_str(), effective.user.as_str(), effective.port.as_str(), effective.key.as_str()),
        ("studio.example", "person", "2222", "/home/example/.ssh/id_ed25519"));
    assert!(state.start_resolve("build-01"));
    state.finish_resolve("build-01", Err("ssh -G timed out".into()));
    let failed = state.view().effective.unwrap();
    assert!(failed.error);
    assert_eq!(failed.text, "Could not read effective SSH settings: ssh -G timed out\nYour connection overrides are shown below.");
}

#[test]
fn a_check_runs_alone_and_reports_qt_texts() {
    let mut state = MachinesState::default();
    state.start_list(0);
    assert!(!state.start_check("studio"), "not while the list reads");
    state.finish_list(Ok(fixture("machines.json")));
    assert!(state.start_check("studio"));
    assert!(!state.start_check("studio") && state.busy());
    assert_eq!(state.view().check.unwrap().text, "Testing SSH and checking HGS…");
    state.finish_check("studio", Ok(fixture("check-studio.json")));
    let check = state.view().check.unwrap();
    assert!(check.error && !check.pending);
    assert_eq!(check.text, "SSH connected: Linux x86_64. Missing: tmux, Rust. Install the newest stable Rust and tmux in the SSH terminal if needed, then use Install / update HGS.");
    let texts = |reply: Value| { let mut s = MachinesState::default(); s.start_check("studio"); s.finish_check("studio", Ok(reply)); s.view().check.unwrap() };
    let ok = texts(serde_json::json!({"ok": true, "ssh_ok": true, "os": "Linux", "arch": "x86_64", "hgs": "hgs 1.56.0", "tmux": "tmux 3.5a"}));
    assert_eq!((ok.error, ok.text.as_str()), (false, "Connected: Linux x86_64, hgs 1.56.0, tmux 3.5a"));
    let down = texts(serde_json::json!({"ok": false, "ssh_ok": false, "error": "Permission denied (publickey)."}));
    assert_eq!(down.text, "SSH connection failed: Permission denied (publickey).\nOpen SSH terminal to check credentials or approve the host key, then test again.");
}
```

In `projects.rs` (`#[cfg(test)] mod tests`):

```rust
use super::*;
use serde_json::json;
use std::path::Path;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/views").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn peers() -> Vec<String> {
    vec!["studio".into(), "build-01".into()]
}

#[test]
fn the_catalog_keeps_the_shared_order_folders_and_unconnected_peers() {
    let mut swarm = SwarmState::default();
    assert!(!swarm.view(&peers()).loaded);
    swarm.finish(Ok(fixture("swarm.json")));
    let view = swarm.view(&peers());
    assert!(view.loaded);
    let ids: Vec<&str> = view.projects.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["sample", "ungrouped", "remote-only"]);
    assert!(view.projects[1].is_default && !view.projects[0].is_default);
    assert!(!view.projects[2].accessible);
    assert_eq!(view.projects[0].color.as_deref(), Some("#3fb950"));
    assert_eq!(view.projects[1].color, None);
    assert_eq!(view.projects[1].palette, 1);
    let folders: Vec<(&str, &str)> = view.projects[0].folders.iter().map(|f| (f.name.as_str(), f.computer.as_str())).collect();
    assert_eq!(folders, [("sample-project", "workstation"), ("Docs", "Windows lab")]);
    assert_eq!(view.unconnected, ["build-01"]);
    assert_eq!(view.notice.as_deref(), Some("Session connections for build-01 do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping."));
    assert_eq!((view.status.as_str(), view.status_error, view.conflicts), ("1 project conflict(s) need review in Swarm.", true, 1));
    assert_eq!(leaf("C:\\Users\\example\\docs"), "docs");
    assert_eq!(leaf("/"), "/");
}

#[test]
fn the_status_line_follows_qt_precedence() {
    let status = |change: &dyn Fn(&mut Value)| {
        let mut snapshot = fixture("swarm.json");
        change(&mut snapshot);
        let mut swarm = SwarmState::default();
        swarm.finish(Ok(snapshot));
        let view = swarm.view(&[]);
        (view.status, view.status_error)
    };
    assert_eq!(status(&|s: &mut Value| s["conflicts"] = json!([])),
        ("1 connection(s) unavailable. Changes are saved here and will sync when reachable.".to_owned(), true));
    assert_eq!(status(&|s: &mut Value| { s["conflicts"] = json!([]); s["peers"]["mac"] = json!({"node": "node-3"}); }),
        ("Project sync connected to 2 computer(s)".to_owned(), false));
    assert_eq!(status(&|s: &mut Value| { s["conflicts"] = json!([]); s["peers"] = json!({}); }),
        ("Local catalog. Connect another computer to sync projects.".to_owned(), false));
    let unknown = status(&|s: &mut Value| { s["node_id"] = json!(""); });
    assert_eq!(unknown.0, "1 project conflict(s) need review in Swarm.");
}

#[test]
fn an_older_hgs_or_a_failed_read_keeps_the_last_catalog() {
    let mut swarm = SwarmState::default();
    swarm.finish(Ok(fixture("swarm-old.json")));
    let view = swarm.view(&peers());
    assert_eq!((view.loaded, view.status.as_str(), view.status_error), (false, "Project sync needs an updated HGS installation.", true));
    swarm.finish(Ok(fixture("swarm.json")));
    swarm.finish(Ok(json!({"schema": 1, "machines": [], "digest": "d2"})));
    let view = swarm.view(&peers());
    assert_eq!(view.status, "Project sync needs an updated HGS installation.");
    assert_eq!(view.projects.len(), 3, "the last good catalog stays");
    swarm.finish(Err("hgs did not answer in time".into()));
    let view = swarm.view(&peers());
    assert_eq!((view.status.as_str(), view.status_error), ("Project sync unavailable: hgs did not answer in time", true));
    assert_eq!(view.projects.len(), 3);
}
```

In `machine.rs`, a new test beside PR #391's:

```rust
#[test]
fn a_control_console_and_its_session_host_project_to_the_card() {
    let meta = json!({"host": "win-lab", "local_sessions": false,
        "local_host": {"state": "running", "pid": 4120, "protocol": 1, "conpty": "2", "integrity": "Medium", "windows_session": 1},
        "metrics": {"platform": "windows"}});
    let machine = MachineMeta::project(&meta);
    assert_eq!(machine.local_sessions, Some(false));
    let host = machine.local_host.unwrap();
    assert_eq!((host.state.as_deref(), host.pid, host.conpty.as_deref(), host.windows_session), (Some("running"), Some(4120), Some("2"), Some(1)));
    assert_eq!(MachineMeta::project(&json!({"host": "workstation"})).local_sessions, None);
    assert_eq!(MachineMeta::project(&json!({"local_host": "yes"})).local_host, None);
}
```

- [ ] **Step 3: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core views:: machine::`. Expected: compile errors (`Poll`, `MachinesState`, `SwarmState`, `local_sessions` not found).

- [ ] **Step 4: Implement.**

`client/core/src/views/schedule.rs` (tests of Step 2 at its end):

```rust
//! When the screen reads run: pure decisions over a millisecond clock the
//! hub passes in (client/src-tauri/src/screens.rs runs the reads). Sources:
//! AccountsPage::ensureCatalogs and requestQueuedCatalog
//! (AccountsPage.cpp:450-468), AccountUsageStore::ensure, dispatch and
//! finish (AccountUsageStore.cpp:24-64), SwarmController's 5 s poll.
use super::accounts::{AccountsState, UsageRef};
use super::text;
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One `account ls` per host per 30 s unless forced (AccountsPage.cpp:456).
pub const CATALOG_INTERVAL: u64 = 30_000;
/// One inspect per profile per 5 minutes (AccountUsageStore::RefreshInterval).
pub const USAGE_INTERVAL: u64 = 300_000;
/// Inspects in flight at once (AccountUsageStore::dispatch).
pub const USAGE_CONCURRENCY: usize = 2;
/// The shared project catalog's poll (SwarmController pollInterval).
pub const SWARM_INTERVAL: u64 = 5_000;

/// A screen whose reads the hub runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Overview,
    Projects,
    Machines,
    Accounts,
}

impl Screen {
    /// The name the `screens-changed` event carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Projects => "projects",
            Self::Machines => "machines",
            Self::Accounts => "accounts",
        }
    }
}

/// A read that runs at most once at a time, again `interval` after the
/// previous start.
#[derive(Clone, Debug, Default)]
pub struct Poll {
    interval: u64,
    started: Option<u64>,
    pending: bool,
}

impl Poll {
    pub fn new(interval: u64) -> Self {
        Self { interval, started: None, pending: false }
    }

    /// Whether a read starts now; when it does, it is marked running.
    pub fn due(&mut self, now: u64, force: bool) -> bool {
        if self.pending {
            return false;
        }
        if !force && self.started.is_some_and(|at| now.saturating_sub(at) < self.interval) {
            return false;
        }
        self.started = Some(now);
        self.pending = true;
        true
    }

    pub fn finish(&mut self) {
        self.pending = false;
    }

    pub fn pending(&self) -> bool {
        self.pending
    }
}

impl AccountsState {
    /// The hosts whose `account ls` starts now (marked running).
    pub fn ensure_catalogs(&mut self, hosts: &[ComputerId], now: u64, force: bool) -> Vec<ComputerId> {
        let mut start = Vec::new();
        for host in hosts {
            let slot = self.catalogs.entry(host.clone()).or_default();
            if slot.pending {
                // Only an explicit refresh can queue one follow-up.
                slot.queued |= force;
                continue;
            }
            if force || slot.requested.is_none_or(|at| now.saturating_sub(at) >= CATALOG_INTERVAL) {
                slot.requested = Some(now);
                slot.pending = true;
                start.push(host.clone());
            }
        }
        start
    }

    /// Each newly listed host once, even before Accounts is opened.
    pub fn warm(&mut self, hosts: &[ComputerId], now: u64) -> Vec<ComputerId> {
        let fresh: Vec<ComputerId> = hosts.iter().filter(|host| self.warmed.insert((*host).clone())).cloned().collect();
        self.ensure_catalogs(&fresh, now, false)
    }

    /// Files a reply; true when a queued follow-up starts now (marked running).
    pub fn finish_catalog(&mut self, host: &ComputerId, reply: Result<Value, String>, now: u64, hosts: &[ComputerId]) -> bool {
        let slot = self.catalogs.entry(host.clone()).or_default();
        slot.pending = false;
        match reply {
            Ok(value) => {
                slot.data = Some(value);
                slot.error = None;
            }
            Err(error) => slot.error = Some(error),
        }
        if std::mem::take(&mut slot.queued) && hosts.contains(host) {
            slot.requested = Some(now);
            slot.pending = true;
            return true;
        }
        false
    }

    /// Queues a profile's inspect when it is due (AccountUsageStore::ensure).
    pub fn ensure_usage(&mut self, r: &UsageRef, reachable: bool, now: u64, force: bool) {
        if r.id.is_empty() {
            return;
        }
        let slot = self.usage.entry(r.key()).or_default();
        if slot.pending {
            return;
        }
        let changed = !r.auth_revision.is_empty() && r.auth_revision != slot.auth_revision;
        if changed && !slot.auth_revision.is_empty() {
            slot.data = None;
        }
        if !r.auth_revision.is_empty() {
            slot.auth_revision = r.auth_revision.clone();
        }
        if !reachable || (!force && !changed && slot.attempted.is_some_and(|at| now.saturating_sub(at) < USAGE_INTERVAL)) {
            return;
        }
        slot.attempted = Some(now);
        slot.pending = true;
        self.queue.push_back(r.clone());
    }

    /// The queued inspects that start now, at most two in flight.
    pub fn dispatch(&mut self, reachable: &dyn Fn(&ComputerId) -> bool) -> Vec<UsageRef> {
        let mut start = Vec::new();
        while self.running < USAGE_CONCURRENCY {
            let Some(r) = self.queue.pop_front() else {
                break;
            };
            if !reachable(&r.host) {
                if let Some(slot) = self.usage.get_mut(&r.key()) {
                    slot.pending = false;
                }
                continue;
            }
            self.running += 1;
            start.push(r);
        }
        start
    }

    /// Files an inspect's reply; None is a failed read (AccountUsageStore::finish).
    pub fn finish_usage(&mut self, r: &UsageRef, reply: Option<Value>) {
        self.running = self.running.saturating_sub(1);
        let slot = self.usage.entry(r.key()).or_default();
        slot.pending = false;
        let matches = reply.as_ref().is_some_and(|data| {
            data.get("status").is_some()
                && text(data, "id") == r.id
                && (r.home.is_empty() || text(data, "home") == r.home)
                && (text(data, "provider").is_empty() || text(data, "provider") == r.provider)
        });
        let failed = !matches || reply.as_ref().is_some_and(|data| text(data, "status") == "unavailable");
        slot.failed = failed;
        if matches && (!failed || slot.data.is_none()) {
            slot.data = reply;
        } else if slot.data.is_none() {
            slot.data = Some(json!({"id": r.id, "provider": r.provider, "status": "unavailable"}));
        }
    }

    /// An account read runs or waits for its turn.
    pub fn reading(&self) -> bool {
        self.running > 0 || !self.queue.is_empty() || self.catalogs.values().any(|slot| slot.pending)
    }
}
```

`client/core/src/views/machines.rs` (tests at its end):

```rust
//! The Machines screen's data: `hgs machine ls`, `machine resolve ALIAS` and
//! `machine check ALIAS` (tray/src/MachinesPage.cpp). The frontend combines
//! each row with the stream's connection state and the machine card.
use super::schedule::Poll;
use super::{flag, items, text, whole};
use serde::Serialize;
use serde_json::Value;

/// The most saved machines the screen lists.
const MACHINES: usize = 64;

/// One saved peer of `machine ls` with its Zerus overrides.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MachineRow {
    pub alias: String,
    pub enabled: bool,
    /// `local` (saved in Zerus) or `config` (from HGS configuration).
    pub source: String,
    pub inherited: bool,
    pub hostname: String,
    pub user: String,
    pub port: Option<u64>,
    pub identity_file: String,
    /// The editor's origin paragraph (MachinesPage.cpp:337-338).
    pub origin: String,
}

/// The selected peer's effective SSH connection (`machine resolve`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Effective {
    pub alias: String,
    pub pending: bool,
    pub error: bool,
    pub text: String,
    /// The inherited values for the field placeholders ("Inherited: ...").
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub key: String,
}

/// The last "Test connection" (`machine check`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckResult {
    pub alias: String,
    pub pending: bool,
    pub error: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MachinesView {
    pub loaded: bool,
    pub refreshing: bool,
    pub error: Option<String>,
    /// This computer's name as `machine ls` reports it.
    pub local: String,
    pub environment_override: bool,
    pub machines: Vec<MachineRow>,
    pub effective: Option<Effective>,
    pub check: Option<CheckResult>,
    /// A list read or a check runs (MachinesPage `busy`).
    pub busy: bool,
}

/// The Machines reads so far.
#[derive(Clone, Debug, Default)]
pub struct MachinesState {
    pub list: Option<Value>,
    pub list_error: Option<String>,
    pub poll: Poll,
    resolve_alias: Option<String>,
    resolve: Option<Result<Value, String>>,
    resolving: bool,
    check_alias: Option<String>,
    check: Option<Result<Value, String>>,
    checking: bool,
}

fn row(machine: &Value, environment: bool) -> MachineRow {
    let connection = machine.get("connection").unwrap_or(&Value::Null);
    let source = text(machine, "source");
    let mut origin = String::new();
    if source == "config" {
        origin.push_str("This alias comes from HGS configuration. ");
    }
    origin.push_str("SSH supplies inherited values from your OpenSSH configuration and defaults. Fields below are Zerus overrides; blank fields keep the inherited value.");
    if environment {
        origin.push_str(" HGS_PEERS in the environment overrides the list of monitored machines.");
    }
    MachineRow {
        alias: text(machine, "alias"),
        enabled: flag(machine, "enabled"),
        inherited: flag(machine, "inherited"),
        hostname: text(connection, "hostname"),
        user: text(connection, "user"),
        port: whole(connection, "port").filter(|port| *port > 0),
        identity_file: text(connection, "identity_file"),
        origin,
        source,
    }
}

fn effective(alias: &str, pending: bool, reply: Option<&Result<Value, String>>) -> Effective {
    let mut out = Effective {
        alias: alias.to_owned(),
        pending,
        error: false,
        text: String::new(),
        hostname: String::new(),
        user: String::new(),
        port: String::new(),
        key: String::new(),
    };
    match reply {
        None => out.text = "Reading effective SSH connection…".into(),
        Some(Err(error)) => {
            out.error = true;
            out.text = format!("Could not read effective SSH settings: {error}\nYour connection overrides are shown below.");
        }
        Some(Ok(value)) => {
            let found = value.get("effective").unwrap_or(&Value::Null);
            let keys: Vec<String> = items(found, "identity_files").iter().filter_map(Value::as_str).map(str::to_owned).collect();
            out.hostname = text(found, "hostname");
            out.user = text(found, "user");
            out.port = text(found, "port");
            out.key = keys.first().cloned().unwrap_or_default();
            let mut lines = vec![format!("Effective connection: {}@{}, port {}", out.user, out.hostname, out.port)];
            if !keys.is_empty() {
                lines.push(format!("Key paths: {}", keys.join(", ")));
            }
            let jump = text(found, "proxy_jump");
            if !jump.is_empty() {
                lines.push(format!("Jump host: {jump}"));
            } else if flag(found, "proxy_command") {
                lines.push("Proxy command configured in OpenSSH".into());
            }
            out.text = lines.join("\n");
        }
    }
    out
}

fn check(alias: &str, pending: bool, reply: Option<&Result<Value, String>>) -> CheckResult {
    let (error, message) = match reply {
        None => (false, "Testing SSH and checking HGS…".to_owned()),
        Some(Err(error)) => (true, error.clone()),
        Some(Ok(data)) if flag(data, "ok") => (
            false,
            format!("Connected: {} {}, {}, {}", text(data, "os"), text(data, "arch"), text(data, "hgs"), text(data, "tmux")),
        ),
        Some(Ok(data)) if flag(data, "ssh_ok") => {
            let mut missing = Vec::new();
            for (key, name) in [("hgs", "HGS"), ("tmux", "tmux"), ("cargo", "Rust")] {
                if text(data, key).is_empty() {
                    missing.push(name);
                }
            }
            let missing = if missing.is_empty() { "a working HGS installation".to_owned() } else { missing.join(", ") };
            let message = if text(data, "os").is_empty() {
                text(data, "error")
            } else {
                format!(
                    "SSH connected: {} {}. Missing: {missing}. Install the newest stable Rust and tmux in the SSH terminal if needed, then use Install / update HGS.",
                    text(data, "os"),
                    text(data, "arch")
                )
            };
            (true, message)
        }
        Some(Ok(data)) => (
            true,
            format!("SSH connection failed: {}\nOpen SSH terminal to check credentials or approve the host key, then test again.", text(data, "error")),
        ),
    };
    CheckResult { alias: alias.to_owned(), pending, error, text: message }
}

impl MachinesState {
    /// `machine ls` on show and on Refresh; false while one runs.
    pub fn start_list(&mut self, now: u64) -> bool {
        self.poll.due(now, true)
    }

    /// A failure keeps the rows and shows the error.
    pub fn finish_list(&mut self, reply: Result<Value, String>) {
        self.poll.finish();
        match reply {
            Ok(value) => {
                self.list = Some(value);
                self.list_error = None;
            }
            Err(error) => self.list_error = Some(error),
        }
    }

    /// The selected peer's effective connection. A new selection replaces an
    /// older one; false when this alias is already being read.
    pub fn start_resolve(&mut self, alias: &str) -> bool {
        if self.resolving && self.resolve_alias.as_deref() == Some(alias) {
            return false;
        }
        self.resolve_alias = Some(alias.to_owned());
        self.resolve = None;
        self.resolving = true;
        true
    }

    /// Files a reply; a reply for an older selection is dropped.
    pub fn finish_resolve(&mut self, alias: &str, reply: Result<Value, String>) {
        if self.resolve_alias.as_deref() == Some(alias) {
            self.resolving = false;
            self.resolve = Some(reply);
        }
    }

    /// "Test connection": one at a time, never while the list reads.
    pub fn start_check(&mut self, alias: &str) -> bool {
        if self.busy() {
            return false;
        }
        self.check_alias = Some(alias.to_owned());
        self.check = None;
        self.checking = true;
        true
    }

    pub fn finish_check(&mut self, alias: &str, reply: Result<Value, String>) {
        if self.check_alias.as_deref() == Some(alias) {
            self.checking = false;
            self.check = Some(reply);
        }
    }

    pub fn busy(&self) -> bool {
        self.poll.pending() || self.checking
    }

    pub fn view(&self) -> MachinesView {
        let list = self.list.as_ref();
        let environment = list.is_some_and(|list| flag(list, "environment_override"));
        MachinesView {
            loaded: list.is_some(),
            refreshing: self.poll.pending(),
            error: self.list_error.clone(),
            local: list.map(|list| text(list, "local")).unwrap_or_default(),
            environment_override: environment,
            machines: list
                .map(|list| items(list, "machines").iter().take(MACHINES).map(|m| row(m, environment)).collect())
                .unwrap_or_default(),
            effective: self.resolve_alias.as_deref().map(|alias| effective(alias, self.resolving, self.resolve.as_ref())),
            check: self.check_alias.as_deref().map(|alias| check(alias, self.checking, self.check.as_ref())),
            busy: self.busy(),
        }
    }
}
```

`client/core/src/views/projects.rs` (tests at its end):

```rust
//! The Projects screen from `hgs swarm get`: the shared organization, its
//! order, folders and default project, with SwarmController's status line
//! (SwarmController.cpp:191-219) and ProjectsDialog's unconnected-computer
//! notice (ProjectsDialog.cpp:251-263).
use super::schedule::{Poll, SWARM_INTERVAL};
use super::{items, text};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

/// The most projects, and folders per project, the screen lists.
const PROJECTS: usize = 500;
const FOLDERS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Folder {
    pub id: String,
    /// `name`, else the path's last part (ProjectsDialog.cpp:300).
    pub name: String,
    pub path: String,
    /// The Computer column: `machine_name`, else `machine` (ProjectsDialog.cpp:303).
    pub computer: String,
    pub machine: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// The project's colour (hex); None uses the palette slot.
    pub color: Option<String>,
    /// False for a project on a computer without a configured connection ("All swarm projects").
    pub accessible: bool,
    pub is_default: bool,
    /// The palette slot by list position, until colour preferences arrive (3c).
    pub palette: usize,
    pub folders: Vec<Folder>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProjectsView {
    pub loaded: bool,
    pub refreshing: bool,
    pub status: String,
    pub status_error: bool,
    pub conflicts: usize,
    pub unconnected: Vec<String>,
    pub notice: Option<String>,
    pub projects: Vec<Project>,
}

/// The shared catalog's reads so far.
#[derive(Clone, Debug)]
pub struct SwarmState {
    pub poll: Poll,
    valid: Option<Value>,
    status: Option<(String, bool)>,
}

impl Default for SwarmState {
    fn default() -> Self {
        Self { poll: Poll::new(SWARM_INTERVAL), valid: None, status: None }
    }
}

/// The last part of a path, with either separator.
pub fn leaf(path: &str) -> String {
    path.split(['/', '\\']).rfind(|part| !part.is_empty()).unwrap_or(path).to_owned()
}

fn status_of(snapshot: &Value) -> (String, bool) {
    let conflicts = items(snapshot, "conflicts").len();
    let peers = snapshot.get("peers").and_then(Value::as_object);
    let total = peers.map_or(0, |peers| peers.len());
    let offline = peers.map_or(0, |peers| peers.values().filter(|peer| !text(peer, "error").is_empty()).count());
    if conflicts > 0 {
        (format!("{conflicts} project conflict(s) need review in Swarm."), true)
    } else if offline > 0 {
        (format!("{offline} connection(s) unavailable. Changes are saved here and will sync when reachable."), true)
    } else if total == 0 {
        ("Local catalog. Connect another computer to sync projects.".into(), false)
    } else {
        (format!("Project sync connected to {total} computer(s)"), false)
    }
}

fn folder(value: &Value) -> Folder {
    let path = text(value, "path");
    let name = text(value, "name");
    let machine = text(value, "machine");
    let machine_name = text(value, "machine_name");
    Folder {
        id: text(value, "id"),
        name: if name.is_empty() { leaf(&path) } else { name },
        computer: if machine_name.is_empty() { machine.clone() } else { machine_name },
        machine,
        path,
    }
}

impl SwarmState {
    /// Files a `swarm get` reply; anything but a schema 1 catalog keeps the last good one.
    pub fn finish(&mut self, reply: Result<Value, String>) {
        self.poll.finish();
        match reply {
            Err(error) => self.status = Some((format!("Project sync unavailable: {error}"), true)),
            Ok(value)
                if value.get("schema").and_then(Value::as_i64) != Some(1)
                    || !value.get("organization").is_some_and(Value::is_object) =>
            {
                self.status = Some(("Project sync needs an updated HGS installation.".into(), true));
            }
            Ok(value) => {
                self.status = Some(status_of(&value));
                self.valid = Some(value);
            }
        }
    }

    /// The screen; `peers` are the fleet's peer aliases.
    pub fn view(&self, peers: &[String]) -> ProjectsView {
        let (status, status_error) = self.status.clone().unwrap_or_default();
        let Some(snapshot) = &self.valid else {
            return ProjectsView { refreshing: self.poll.pending(), status, status_error, ..ProjectsView::default() };
        };
        let organization = &snapshot["organization"];
        let default = text(organization, "default_project");
        let listed: Vec<&Value> = items(organization, "projects").iter().take(PROJECTS).collect();
        // The shared order first (SwarmController::presentation), then the rest as listed.
        let mut seen = BTreeSet::new();
        let mut ordered: Vec<&Value> = Vec::new();
        for id in items(organization, "project_order").iter().filter_map(Value::as_str) {
            if let Some(project) = listed.iter().find(|p| text(p, "id") == id) {
                if seen.insert(id.to_owned()) {
                    ordered.push(project);
                }
            }
        }
        for project in &listed {
            if seen.insert(text(project, "id")) {
                ordered.push(project);
            }
        }
        let projects = ordered
            .iter()
            .enumerate()
            .map(|(index, p)| Project {
                id: text(p, "id"),
                name: text(p, "name"),
                color: Some(text(p, "color")).filter(|color| !color.is_empty()),
                accessible: p.get("accessible").and_then(Value::as_bool).unwrap_or(true),
                is_default: text(p, "id") == default,
                palette: index % 8,
                folders: items(p, "folders").iter().take(FOLDERS).map(folder).collect(),
            })
            .collect();
        // A configured SSH connection permits session access; it does not join
        // independent project catalogs. Only the swarm snapshot proves that.
        let known: BTreeSet<&str> = snapshot
            .get("peers")
            .and_then(Value::as_object)
            .map(|peers| peers.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let unconnected: Vec<String> = if text(snapshot, "node_id").is_empty() {
            Vec::new()
        } else {
            peers.iter().filter(|peer| !known.contains(peer.as_str())).cloned().collect()
        };
        let notice = (!unconnected.is_empty()).then(|| {
            format!(
                "Session connections for {} do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping.",
                unconnected.join(", ")
            )
        });
        ProjectsView {
            loaded: true,
            refreshing: self.poll.pending(),
            status,
            status_error,
            conflicts: items(snapshot, "conflicts").len(),
            unconnected,
            notice,
            projects,
        }
    }
}
```

In `client/core/src/machine.rs`, add to `MachineMeta` after `ok`:

```rust
    /// `local_sessions`: false only on a control console (the key is missing otherwise).
    pub local_sessions: Option<bool>,
    /// `local_host`: this Windows computer's session host.
    pub local_host: Option<LocalHost>,
```

the struct after `Presence`:

```rust
/// This Windows computer's session host (`catalog.meta.local_host`), as
/// MachinesPage reads it; Qt reads `conpty` and `integrity` as text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalHost {
    pub state: Option<String>,
    pub pid: Option<u64>,
    pub conpty: Option<String>,
    pub integrity: Option<String>,
    pub windows_session: Option<u64>,
    pub error: Option<String>,
}
```

and in `MachineMeta::project`, after `ok: flag(meta, "ok"),`:

```rust
            local_sessions: flag(meta, "local_sessions"),
            local_host: meta.get("local_host").filter(|host| host.is_object()).map(|host| LocalHost {
                state: text(host, "state"),
                pid: count(host, "pid"),
                conpty: text(host, "conpty"),
                integrity: text(host, "integrity"),
                windows_session: count(host, "windows_session"),
                error: text(host, "error"),
            }),
```

PR #391's batch and pump tests build `MachineMeta { .., ..Default::default() }`, so they compile unchanged.

- [ ] **Step 5: Run** the core tests, clippy and fmt as in Task 1 Step 5. Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add client/core/src/views client/core/src/machine.rs client/core/tests/fixtures/views
git commit -m "feat(client): read timing, Machines and Projects views, console fields of the machine card"
```

---

### Task 3: The screens hub, its commands and the replay guard (`client/src-tauri/src/screens.rs`)

**Files:**
- Create: `client/src-tauri/src/screens.rs`
- Modify: `client/src-tauri/src/pump.rs` (`Pump::fleet()` and a test)
- Modify: `client/src-tauri/src/commands.rs` (`AppState::pump()`)
- Modify: `client/src-tauri/src/lib.rs` (`#[cfg(feature = "live")] mod screens;`, manage the hub in `setup`, register the six commands in the live `handler()`)
- Modify: `client/core/tests/support/fake_hgs_reads.py` (a reply may `sleep`)
- Test: unit tests in `screens.rs` and `pump.rs`

**Interfaces:**
- Consumes:
  - `reads::read(hgs: &Path, computer: &ComputerId, read: &Read) -> Result<Value, ReadError>` and `Read::{Accounts, AccountUsage { id }, Swarm, Machines, MachineResolve { alias }, MachineCheck { alias }}` (PR #390);
  - Tasks 1 and 2: `accounts::{hosts, refs, reachable, project}`, `AccountsState`'s timing methods, `MachinesState`, `SwarmState`, `Screen`, `FleetEntry`;
  - `crate::actions::actions_allowed(replay, hgs)` (phase 2);
  - the pump's mirror with PR #391's `machine` per computer.
- Produces (frontend contract, snake_case JSON; event `screens-changed` with payload `"overview" | "projects" | "machines" | "accounts"`):

```rust
#[tauri::command(async)] pub fn screen_visible(state: State<'_, Screens>, screen: Screen, visible: bool) -> Result<(), String>;
#[tauri::command(async)] pub fn accounts_view(state: State<'_, Screens>, force: bool) -> Result<AccountsView, String>;
#[tauri::command(async)] pub fn machines_view(state: State<'_, Screens>, force: bool) -> Result<MachinesView, String>;
#[tauri::command(async)] pub fn machine_resolve(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String>;
#[tauri::command(async)] pub fn machine_check(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String>;
#[tauri::command(async)] pub fn projects_view(state: State<'_, Screens>) -> Result<ProjectsView, String>;
impl Pump { pub fn fleet(&self) -> Vec<FleetEntry> }
impl AppState { pub fn pump(&self) -> crate::pump::Pump }
pub const REPLAY_NEEDS_HGS: &str = "Reads in replay mode need ZERUS_CLIENT_HGS.";
```

Behaviour:
- Every command answers at once with the current view and starts what is due on Tauri's blocking pool (`tauri::async_runtime::spawn_blocking`). A one-second ticker thread re-reads the fleet (`Pump::fleet()`), warms new account hosts and keeps visible screens' reads due.
- **Showing a screen:**
  - Machines starts `machine ls` (skipped while one runs).
  - Projects starts `swarm get`.
  - Overview and Accounts make the account reads due: `account ls` per host (30 s), then each installed profile's `inspect` (5 min, two at once).
- **Hiding a screen** stops its reads; only the account warm-up runs hidden.
- **Refresh:**
  - `accounts_view(true)` forces `ls` and inspects, ignored while account reads run (Qt `reload`).
  - `machines_view(true)` forces `machine ls`.
- **On selection and on request:** `machine_resolve` runs on selection; `machine_check` only on "Test connection", never while the list reads or another check runs.
- **Events:** after a read finishes, each visible screen whose view changed since the webview last saw it gets one `screens-changed` event.
- **Replay** (`ZERUS_CLIENT_REPLAY` set) without `ZERUS_CLIENT_HGS`: no read ever runs. The views carry `REPLAY_NEEDS_HGS`: as the first accounts notice, as the Machines error and as the Projects status.

- [ ] **Step 1: Let the fake sleep.** In `client/core/tests/support/fake_hgs_reads.py`, document `"sleep": seconds` in the docstring's reply keys, add `import time` to the imports, and right after the `if reply is None:` block insert:

```python
time.sleep(reply.get("sleep", 0))
```

- [ ] **Step 2: Write the failing tests.** In `pump.rs` tests:

```rust
#[test]
fn the_fleet_lists_unended_computers_with_their_cards_this_computer_first() {
    let pump = Pump::default();
    for (computer, status) in [
        ("studio", ComputerStatus::Reconnecting { attempt: 2 }),
        ("", ComputerStatus::Live),
        ("old", ComputerStatus::Ended),
    ] {
        pump.push(Change::Computer { computer: ComputerId(computer.into()), status });
    }
    let card = MachineMeta {
        host: Some("workstation".into()),
        platform: Some("windows".into()),
        local_sessions: Some(false),
        local_host: Some(zerus_client_core::machine::LocalHost::default()),
        ..MachineMeta::default()
    };
    pump.push(Change::Machine { computer: ComputerId(String::new()), machine: card });
    let fleet = pump.fleet();
    let rows: Vec<(&str, &str, u32)> = fleet.iter().map(|e| (e.computer.0.as_str(), e.status.as_str(), e.attempt)).collect();
    assert_eq!(rows, [("", "live", 0), ("studio", "reconnecting", 2)]);
    assert!(!fleet[0].local_sessions && fleet[0].local_host);
    assert_eq!((fleet[0].host.as_deref(), fleet[0].platform.as_deref()), (Some("workstation"), Some("windows")));
    assert!(fleet[1].local_sessions && fleet[1].host.is_none(), "no card yet: local sessions assumed");
}
```

In `screens.rs` (`#[cfg(test)] mod tests`, Unix only like the core's read tests):

```rust
use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The core's fake hgs (client/core/tests/support/fake_hgs_reads.py) with the view fixtures.
struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    fn new(replies: Value) -> Self {
        let dir = std::env::temp_dir().join(format!("zerus-screens-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests");
        let hgs = dir.join("hgs");
        std::os::unix::fs::symlink(core.join("support/fake_hgs_reads.py"), &hgs).unwrap();
        let config = json!({"fixtures": core.join("fixtures/views"), "replies": replies});
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        Self { dir, hgs }
    }

    /// Every call so far, as its argv joined by spaces.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("log.ndjson"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let call: Value = serde_json::from_str(line).unwrap();
                call["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect::<Vec<_>>().join(" ")
            })
            .collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Harness {
    screens: Screens,
    clock: Arc<AtomicU64>,
    notified: Arc<Mutex<Vec<Screen>>>,
    fake: Fake,
}

impl Harness {
    fn at(&self, ms: u64) {
        self.clock.store(ms, Ordering::SeqCst);
    }

    fn count(&self, argv: &str) -> usize {
        self.fake.calls().iter().filter(|call| call.as_str() == argv).count()
    }
}

fn local() -> FleetEntry {
    FleetEntry {
        computer: ComputerId::local(),
        status: "live".into(),
        attempt: 0,
        local_sessions: true,
        local_host: false,
        host: Some("workstation".into()),
        platform: Some("linux".into()),
    }
}

fn harness(replies: Value, allowed: bool) -> Harness {
    let fake = Fake::new(replies);
    let clock = Arc::new(AtomicU64::new(0));
    let notified = Arc::new(Mutex::new(Vec::new()));
    let screens = Screens::new(
        fake.hgs.clone(),
        allowed,
        Arc::new(|job: Job| {
            std::thread::spawn(job);
        }),
        {
            let notified = notified.clone();
            Arc::new(move |screen: Screen| notified.lock().unwrap().push(screen))
        },
        Arc::new(|| vec![local()]),
        {
            let clock = clock.clone();
            Arc::new(move || clock.load(Ordering::SeqCst))
        },
    );
    Harness { screens, clock, notified, fake }
}

fn eventually(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Review Focus 5.
#[test]
fn projects_read_only_while_shown_every_five_seconds_and_never_twice_at_once() {
    let h = harness(json!({"swarm get": {"file": "swarm.json", "sleep": 1.0}}), true);
    for second in 1..=12 {
        h.at(second * 1000);
        h.screens.tick();
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.count("swarm get"), 0, "hidden: no read");
    h.at(13_000);
    h.screens.set_visible(Screen::Projects, true);
    eventually("the first read starts", || h.count("swarm get") == 1);
    h.at(18_100);
    h.screens.tick();
    assert_eq!(h.count("swarm get"), 1, "the first read still runs");
    eventually("the catalog arrives", || h.screens.projects().loaded);
    eventually("a change event", || h.notified.lock().unwrap().contains(&Screen::Projects));
    h.screens.tick();
    eventually("due 5 s after the last start", || h.count("swarm get") == 2);
    h.at(30_000);
    h.screens.tick();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.count("swarm get"), 2, "never two at once");
    eventually("the second read ends", || !h.screens.projects().refreshing);
    h.screens.set_visible(Screen::Projects, false);
    h.at(60_000);
    h.screens.tick();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.count("swarm get"), 2, "hidden again: no read");
}

// Review Focus 4.
#[test]
fn a_failing_or_slow_inspect_leaves_the_others_and_waits_five_minutes() {
    let h = harness(json!({
        "account ls": {"file": "accounts-local.json"},
        "account inspect claude-work --refresh": {"file": "inspect-claude-work.json"},
        "account inspect native-claude --refresh": {"stderr": "hgs: inspect failed\n", "exit": 1},
        "account inspect native-codex --refresh": {"file": "inspect-native-codex.json", "sleep": 0.5},
    }), true);
    h.screens.set_visible(Screen::Accounts, true);
    eventually("every inspect answered", || {
        let view = h.screens.accounts(false);
        view.pending == 0 && !view.busy && view.snapshot
    });
    let view = h.screens.accounts(false);
    let member = |id: &str| view.accounts.iter().flat_map(|a| &a.members).find(|m| m.id == id).cloned().unwrap();
    assert_eq!(member("claude-work").usage.status, "ok", "the others still load");
    assert_eq!(member("native-codex").usage.status, "signed_out", "the slow one answers too");
    let failed = member("native-claude");
    assert!(failed.usage.refresh_error);
    assert_eq!(failed.usage.status, "unavailable");
    for call in ["account inspect claude-work --refresh", "account inspect native-claude --refresh", "account inspect native-codex --refresh"] {
        assert_eq!(h.count(call), 1, "{call}");
    }
    // A minute later the catalog is read again (30 s), the inspects are not (5 min).
    h.at(60_000);
    h.screens.tick();
    eventually("the second account ls", || h.count("account ls") == 2);
    eventually("idle again", || !h.screens.accounts(false).busy);
    assert_eq!(h.count("account inspect native-claude --refresh"), 1, "no retry inside five minutes");
    // Refresh forces both.
    h.screens.accounts(true);
    eventually("the forced inspect", || h.count("account inspect native-claude --refresh") == 2);
    eventually("the forced ls", || h.count("account ls") == 3);
}

#[test]
fn machines_list_on_show_resolve_on_selection_and_check_only_on_request() {
    let h = harness(json!({
        "machine ls": {"file": "machines.json"},
        "machine resolve studio": {"file": "resolve-studio.json"},
        "machine check studio": {"file": "check-studio.json", "sleep": 0.5},
    }), true);
    h.screens.set_visible(Screen::Machines, true);
    eventually("the list arrives", || h.screens.machines(false).loaded);
    assert_eq!(h.screens.machines(false).machines.len(), 3);
    h.screens.resolve("studio".into());
    eventually("the effective connection", || h.screens.machines(false).effective.is_some_and(|e| !e.pending));
    assert_eq!(h.count("machine check studio"), 0, "a check runs only on request");
    h.screens.check("studio".into());
    h.screens.check("studio".into());
    eventually("the check answers", || h.screens.machines(false).check.is_some_and(|c| !c.pending));
    assert_eq!(h.count("machine check studio"), 1, "one check at a time");
    h.screens.machines(true);
    eventually("Refresh reads the list again", || h.count("machine ls") == 2);
    eventually("a change event", || h.notified.lock().unwrap().contains(&Screen::Machines));
}

#[test]
fn a_replay_without_hgs_never_reads() {
    let h = harness(json!({}), false);
    for screen in [Screen::Overview, Screen::Machines, Screen::Projects] {
        h.screens.set_visible(screen, true);
    }
    h.at(40_000);
    h.screens.tick();
    assert_eq!(h.screens.resolve("studio".into()).error.as_deref(), Some(REPLAY_NEEDS_HGS));
    h.screens.check("studio".into());
    let accounts = h.screens.accounts(true);
    std::thread::sleep(Duration::from_millis(300));
    assert!(h.fake.calls().is_empty(), "{:?}", h.fake.calls());
    assert_eq!(accounts.notices, [REPLAY_NEEDS_HGS]);
    assert!(accounts.snapshot && accounts.pending == 0 && !accounts.busy);
    let projects = h.screens.projects();
    assert_eq!((projects.status.as_str(), projects.status_error), (REPLAY_NEEDS_HGS, true));
}
```

- [ ] **Step 3: Run** `cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml screens:: pump::`. Expected: compile errors (`Screens`, `fleet` not found).

- [ ] **Step 4: Implement.**

`client/src-tauri/src/pump.rs`: add `use zerus_client_core::views::FleetEntry;` and to `impl Pump`:

```rust
    /// The fleet as the screens see it: every computer whose status has not
    /// ended, this computer first (the mirror is keyed by the id's string,
    /// and "" sorts first), with its machine card's host, platform and the
    /// control-console fields. Without a card a computer keeps local sessions.
    pub fn fleet(&self) -> Vec<FleetEntry> {
        let inner = self.lock();
        inner
            .mirror
            .computers
            .iter()
            .filter_map(|(computer, rows)| {
                let status = rows.status?;
                if status == ComputerStatus::Ended {
                    return None;
                }
                let machine = rows.machine.as_ref();
                Some(FleetEntry {
                    computer: ComputerId(computer.clone()),
                    status: status.name().to_owned(),
                    attempt: status.attempt(),
                    local_sessions: machine.and_then(|m| m.local_sessions).unwrap_or(true),
                    local_host: machine.is_some_and(|m| m.local_host.is_some()),
                    host: machine.and_then(|m| m.host.clone()),
                    platform: machine.and_then(|m| m.platform.clone()),
                })
            })
            .collect()
    }
```

`client/src-tauri/src/commands.rs`, after `pub struct AppState`:

```rust
#[cfg(feature = "live")]
impl AppState {
    /// The change pump, whose mirror is the screens hub's view of the fleet.
    pub fn pump(&self) -> crate::pump::Pump {
        self.live.pump.clone()
    }
}
```

`client/src-tauri/src/lib.rs`:
- Add `#[cfg(feature = "live")] mod screens;` after `mod replay;`.
- In `setup`, inside the `#[cfg(feature = "live")]` block after `app.manage(actions::Actions::new(...));`:

```rust
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump));
```

- In the live `handler()`, add after `actions::pending_read_marks,`:

```rust
        screens::screen_visible,
        screens::accounts_view,
        screens::machines_view,
        screens::machine_resolve,
        screens::machine_check,
        screens::projects_view,
```

`client/src-tauri/src/screens.rs` (tests of Step 2 at its end, under `#[cfg(all(test, unix))]`):

```rust
//! The reads behind the Overview, Projects, Machines and Accounts screens:
//! one hub that knows which screens are visible, runs the due `hgs` reads off
//! the main thread (`zerus_client_core::reads`, bounded and timed out there),
//! keeps the replies and hands the webview projected view models
//! (`zerus_client_core::views`). Which read runs when is decided in
//! `views::schedule`; this file wires the clock, the threads and the event.
//!
//! Every command answers at once with the current view and starts what is
//! due in the background. A changed view emits `screens-changed` with the
//! screen's name; the frontend then pulls it again. A replay reads only with
//! an explicit `ZERUS_CLIENT_HGS`, as the actions do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::State;
use zerus_client_core::reads::{self, Read, ReadError};
use zerus_client_core::views::accounts::{self, AccountsDisplay, AccountsState, AccountsView, UsageRef};
use zerus_client_core::views::machines::{MachinesState, MachinesView};
use zerus_client_core::views::projects::{ProjectsView, SwarmState};
use zerus_client_core::views::schedule::Screen;
use zerus_client_core::views::FleetEntry;
use zerus_client_core::ComputerId;

/// What a replay without an explicit `hgs` shows instead of reading.
pub const REPLAY_NEEDS_HGS: &str = "Reads in replay mode need ZERUS_CLIENT_HGS.";

/// One read, run off the UI thread.
pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn(Screen) + Send + Sync>;
pub type FleetSource = Arc<dyn Fn() -> Vec<FleetEntry> + Send + Sync>;
/// Milliseconds since the hub started.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Default)]
struct Hub {
    visible: BTreeSet<Screen>,
    fleet: Vec<FleetEntry>,
    accounts: AccountsState,
    display: AccountsDisplay,
    machines: MachinesState,
    swarm: SwarmState,
    /// The view each visible screen last showed, so a change emits once.
    shown: BTreeMap<Screen, Value>,
}

struct Shared {
    hub: Mutex<Hub>,
    hgs: PathBuf,
    allowed: bool,
    spawn: Spawn,
    notify: Notify,
    fleet: FleetSource,
    clock: Clock,
}

/// The managed state of the screen commands; clones share it.
#[derive(Clone)]
pub struct Screens(Arc<Shared>);

/// A read the hub decided to start.
enum Run {
    Catalog(ComputerId),
    Usage(UsageRef),
    Swarm,
    Machines,
    Resolve(String),
    Check(String),
}

fn account_hosts(fleet: &[FleetEntry]) -> Vec<ComputerId> {
    accounts::hosts(fleet).into_iter().map(|host| host.computer).collect()
}

fn message(reply: Result<Value, ReadError>) -> Result<Value, String> {
    reply.map_err(|error| error.to_string())
}

/// Records a view the webview now holds for the visible screens that show it.
fn remember<T: Serialize>(hub: &mut Hub, screens: &[Screen], view: &T) {
    if let Ok(value) = serde_json::to_value(view) {
        for screen in screens {
            if hub.visible.contains(screen) {
                hub.shown.insert(*screen, value.clone());
            }
        }
    }
}

impl Screens {
    pub fn new(hgs: PathBuf, allowed: bool, spawn: Spawn, notify: Notify, fleet: FleetSource, clock: Clock) -> Self {
        Self(Arc::new(Shared { hub: Mutex::new(Hub::default()), hgs, allowed, spawn, notify, fleet, clock }))
    }

    /// The app's hub: `default_hgs()` on Tauri's blocking pool, the fleet from
    /// the pump's mirror, `screens-changed` to the webview, and a ticker.
    pub fn tauri(app: tauri::AppHandle, pump: crate::pump::Pump) -> Self {
        use tauri::Emitter;
        let var = |name: &str| std::env::var(name).ok();
        let allowed = crate::actions::actions_allowed(var("ZERUS_CLIENT_REPLAY").as_deref(), var("ZERUS_CLIENT_HGS").as_deref());
        let started = Instant::now();
        let screens = Self::new(
            zerus_client_core::default_hgs(),
            allowed,
            Arc::new(|job: Job| {
                tauri::async_runtime::spawn_blocking(job);
            }),
            Arc::new(move |screen: Screen| {
                let _ = app.emit("screens-changed", screen.name());
            }),
            Arc::new(move || pump.fleet()),
            Arc::new(move || started.elapsed().as_millis() as u64),
        );
        screens.start_ticker();
        screens
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.hub.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        (self.0.clock)()
    }

    fn refresh_fleet(&self) {
        let fleet = (self.0.fleet)();
        let mut hub = self.lock();
        if hub.fleet != fleet {
            hub.fleet = fleet;
        }
    }

    fn start_ticker(&self) {
        let screens = self.clone();
        std::thread::Builder::new()
            .name("zerus-screens".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                screens.tick();
            })
            .expect("the screens ticker starts");
    }

    /// One policy step, every second: follow the fleet, warm new account
    /// hosts once, keep the visible screens' reads due, report changes.
    pub fn tick(&self) {
        self.refresh_fleet();
        self.step();
        self.publish();
    }

    fn step(&self) {
        let runs = {
            let now = self.now();
            let mut hub = self.lock();
            self.due(&mut hub, now, false)
        };
        self.run(runs);
    }

    /// The reads due now; `force` is an account Refresh.
    fn due(&self, hub: &mut Hub, now: u64, force: bool) -> Vec<Run> {
        if !self.0.allowed {
            return Vec::new();
        }
        let hosts = account_hosts(&hub.fleet);
        let mut runs: Vec<Run> = hub.accounts.warm(&hosts, now).into_iter().map(Run::Catalog).collect();
        if hub.visible.contains(&Screen::Accounts) || hub.visible.contains(&Screen::Overview) {
            runs.extend(hub.accounts.ensure_catalogs(&hosts, now, force).into_iter().map(Run::Catalog));
            for (r, reachable) in accounts::refs(&hub.fleet, &hub.accounts) {
                hub.accounts.ensure_usage(&r, reachable, now, force);
            }
        }
        let fleet = hub.fleet.clone();
        let reachable = move |host: &ComputerId| fleet.iter().any(|entry| &entry.computer == host && accounts::reachable(entry));
        runs.extend(hub.accounts.dispatch(&reachable).into_iter().map(Run::Usage));
        if hub.visible.contains(&Screen::Projects) && hub.swarm.poll.due(now, false) {
            runs.push(Run::Swarm);
        }
        runs
    }

    fn run(&self, runs: Vec<Run>) {
        for job in runs {
            let screens = self.clone();
            (self.0.spawn)(Box::new(move || screens.read(job)));
        }
    }

    /// Runs one read (blocking, off the UI thread) and files its reply.
    fn read(&self, job: Run) {
        let hgs = &self.0.hgs;
        let local = ComputerId::local();
        match job {
            Run::Catalog(host) => {
                let reply = message(reads::read(hgs, &host, &Read::Accounts));
                let now = self.now();
                let again = {
                    let mut hub = self.lock();
                    let hosts = account_hosts(&hub.fleet);
                    hub.accounts.finish_catalog(&host, reply, now, &hosts)
                };
                if again {
                    self.run(vec![Run::Catalog(host)]);
                }
                self.step();
            }
            Run::Usage(r) => {
                let reply = reads::read(hgs, &r.host, &Read::AccountUsage { id: r.id.clone() }).ok();
                self.lock().accounts.finish_usage(&r, reply);
                self.step();
            }
            Run::Swarm => {
                let reply = message(reads::read(hgs, &local, &Read::Swarm));
                self.lock().swarm.finish(reply);
            }
            Run::Machines => {
                let reply = message(reads::read(hgs, &local, &Read::Machines));
                self.lock().machines.finish_list(reply);
            }
            Run::Resolve(alias) => {
                let reply = message(reads::read(hgs, &local, &Read::MachineResolve { alias: alias.clone() }));
                self.lock().machines.finish_resolve(&alias, reply);
            }
            Run::Check(alias) => {
                let reply = message(reads::read(hgs, &local, &Read::MachineCheck { alias: alias.clone() }));
                self.lock().machines.finish_check(&alias, reply);
            }
        }
        self.publish();
    }

    /// One `screens-changed` per visible screen whose view changed.
    fn publish(&self) {
        let changed: Vec<Screen> = {
            let mut hub = self.lock();
            let visible: Vec<Screen> = hub.visible.iter().copied().collect();
            let mut changed = Vec::new();
            for screen in visible {
                let value = self.view_of(&mut hub, screen);
                if hub.shown.get(&screen) != Some(&value) {
                    hub.shown.insert(screen, value);
                    changed.push(screen);
                }
            }
            changed
        };
        for screen in changed {
            (self.0.notify)(screen);
        }
    }

    fn view_of(&self, hub: &mut Hub, screen: Screen) -> Value {
        match screen {
            Screen::Overview | Screen::Accounts => serde_json::to_value(self.accounts_of(hub)),
            Screen::Machines => serde_json::to_value(self.machines_of(hub)),
            Screen::Projects => serde_json::to_value(self.projects_of(hub)),
        }
        .unwrap_or(Value::Null)
    }

    fn accounts_of(&self, hub: &mut Hub) -> AccountsView {
        let mut view = accounts::project(&hub.fleet, &hub.accounts, &mut hub.display);
        if !self.0.allowed {
            view.notices.insert(0, REPLAY_NEEDS_HGS.into());
            view.pending = 0;
            view.busy = false;
            view.snapshot = true;
        }
        view
    }

    fn machines_of(&self, hub: &Hub) -> MachinesView {
        let mut view = hub.machines.view();
        if !self.0.allowed {
            view.error = Some(REPLAY_NEEDS_HGS.into());
        }
        view
    }

    fn projects_of(&self, hub: &Hub) -> ProjectsView {
        let peers: Vec<String> = hub.fleet.iter().filter_map(|entry| entry.computer.alias().map(str::to_owned)).collect();
        let mut view = hub.swarm.view(&peers);
        if !self.0.allowed {
            view.status = REPLAY_NEEDS_HGS.into();
            view.status_error = true;
        }
        view
    }

    /// A screen shown or hidden. Showing Machines reads the list, showing
    /// Projects the catalog; Overview and Accounts make the account reads due.
    pub fn set_visible(&self, screen: Screen, visible: bool) {
        self.refresh_fleet();
        let runs = {
            let now = self.now();
            let mut hub = self.lock();
            let mut runs = Vec::new();
            if visible {
                let shown = hub.visible.insert(screen);
                if shown && self.0.allowed {
                    if screen == Screen::Machines && hub.machines.start_list(now) {
                        runs.push(Run::Machines);
                    }
                    if screen == Screen::Projects && hub.swarm.poll.due(now, true) {
                        runs.push(Run::Swarm);
                    }
                }
            } else {
                hub.visible.remove(&screen);
                hub.shown.remove(&screen);
            }
            runs.extend(self.due(&mut hub, now, false));
            runs
        };
        self.run(runs);
    }

    /// The accounts view; `force` is Refresh (forced `ls` and inspects,
    /// ignored while account reads run, as Qt's `reload`).
    pub fn accounts(&self, force: bool) -> AccountsView {
        self.refresh_fleet();
        let runs = {
            let now = self.now();
            let mut hub = self.lock();
            if force && !hub.accounts.reading() { self.due(&mut hub, now, true) } else { Vec::new() }
        };
        self.run(runs);
        let mut hub = self.lock();
        let view = self.accounts_of(&mut hub);
        remember(&mut hub, &[Screen::Overview, Screen::Accounts], &view);
        view
    }

    fn machines_answer(&self) -> MachinesView {
        let mut hub = self.lock();
        let view = self.machines_of(&hub);
        remember(&mut hub, &[Screen::Machines], &view);
        view
    }

    /// The Machines view; `force` is Refresh (skipped while a list read runs).
    pub fn machines(&self, force: bool) -> MachinesView {
        let start = {
            let now = self.now();
            force && self.0.allowed && self.lock().machines.start_list(now)
        };
        if start {
            self.run(vec![Run::Machines]);
        }
        self.machines_answer()
    }

    /// The selected peer's effective connection.
    pub fn resolve(&self, alias: String) -> MachinesView {
        let start = self.0.allowed && self.lock().machines.start_resolve(&alias);
        if start {
            self.run(vec![Run::Resolve(alias)]);
        }
        self.machines_answer()
    }

    /// "Test connection" (`machine check`, up to 25 s).
    pub fn check(&self, alias: String) -> MachinesView {
        let start = self.0.allowed && self.lock().machines.start_check(&alias);
        if start {
            self.run(vec![Run::Check(alias)]);
        }
        self.machines_answer()
    }

    pub fn projects(&self) -> ProjectsView {
        let mut hub = self.lock();
        let view = self.projects_of(&hub);
        remember(&mut hub, &[Screen::Projects], &view);
        view
    }
}

#[tauri::command(async)]
pub fn screen_visible(state: State<'_, Screens>, screen: Screen, visible: bool) -> Result<(), String> {
    state.set_visible(screen, visible);
    Ok(())
}

#[tauri::command(async)]
pub fn accounts_view(state: State<'_, Screens>, force: bool) -> Result<AccountsView, String> {
    Ok(state.accounts(force))
}

#[tauri::command(async)]
pub fn machines_view(state: State<'_, Screens>, force: bool) -> Result<MachinesView, String> {
    Ok(state.machines(force))
}

#[tauri::command(async)]
pub fn machine_resolve(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String> {
    Ok(state.resolve(alias))
}

#[tauri::command(async)]
pub fn machine_check(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String> {
    Ok(state.check(alias))
}

#[tauri::command(async)]
pub fn projects_view(state: State<'_, Screens>) -> Result<ProjectsView, String> {
    Ok(state.projects())
}
```

- [ ] **Step 5: Run** the tests of Step 3, then `cargo +1.99.1 clippy --locked --manifest-path client/src-tauri/Cargo.toml --all-targets -- -D warnings`, the same with `--no-default-features`, and the core tests (`fake_hgs_reads.py` changed). Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add client/src-tauri/src client/core/tests/support/fake_hgs_reads.py
git commit -m "feat(client): the screens hub runs due hgs reads and reports changed views"
```

---

### Task 4: Frontend types, bridge, stores, Overview selectors and the `usage.ts` gaps

**Files:**
- Modify: `client/src/types.ts` (machine cards, `Batch.machines`, the view types, `Screen`, empty views)
- Modify: `client/src/bridge.ts` (machines kept in batches; six read commands; `onScreensChanged`)
- Modify: `client/src/dev/replay.ts` (replays `machines` when the fixture has them)
- Create: `client/src/store/latest.ts`, `attention.ts`, `machines.ts`, `accounts.ts`, `projects.ts`, `screens.ts`, `overview.ts`, `testing.ts`
- Modify: `client/src/store/ui.ts` (`needsAttention` and `isActive` move to `attention.ts`, re-exported), `client/src/store/catalog.ts` (`createCatalog(onMachines)`), `client/src/store/session.ts` (starts the screens wiring)
- Modify: `client/src/ui/lib/usage.ts` (gaps 1-4 and 7, new helpers), `client/src/ui/components/UsageMeter.tsx` (nullable resets)
- Test: `client/src/ui/lib/usage.test.ts`, `client/src/store/machines.test.ts`, `client/src/store/overview.test.ts`, `client/src/store/screens.test.ts`, one case in `client/src/bridge.test.ts`

**Interfaces:**
- Consumes: the Task 3 commands and event; `Batch.machines` (PR #391); the catalog store (phase 1).
- Produces:

```ts
// bridge.ts
export async function screenVisible(screen: Screen, visible: boolean): Promise<void>;
export async function accountsView(force?: boolean): Promise<AccountsView>;
export async function machinesView(force?: boolean): Promise<MachinesView>;
export async function machineResolve(alias: string): Promise<MachinesView>;
export async function machineCheck(alias: string): Promise<MachinesView>;
export async function projectsView(): Promise<ProjectsView>;
export function onScreensChanged(listener: (screen: Screen) => void): () => void;
// store/machines.ts
export function createMachineCards(): { cards: Record<ComputerId, MachineMeta>; applyMachines(b: Batch): void };
export const machineCards, applyMachines, machinesPage: Accessor<MachinesView>, loadMachines(force?), resolveMachine(alias), testConnection(alias);
// store/accounts.ts
export const accountsPage: Accessor<AccountsView>, loadAccounts(force?), accountMachine/setAccountMachine ("*" or a computer id),
             requestedProfile/setRequestedProfile (a profile key);
// store/projects.ts
export const projectsPage: Accessor<ProjectsView>, loadProjects();
// store/screens.ts
export const screenOf: (page: Page, hidden: boolean) => Screen | undefined;
export function reload(screen: Screen): Promise<void>;
export function startScreens(page: Accessor<Page>, visibility: PageVisibility): () => void;
// store/overview.ts
export interface Connection { tile: MachineState; text: string; fresh: boolean }
export function connectionOf(status: string | undefined): Connection;
export interface MachineCardModel { computer: ComputerId; name: string; filter: string; local: boolean; platform?: OsKind; connection: Connection; meta?: MachineMeta }
export function fleetCards(computers: Record<ComputerId, ComputerState>, cards: Record<ComputerId, MachineMeta>): MachineCardModel[];
export interface OverviewStats { all: number; active: number; attention: number; offline: number; per: Record<ComputerId, { sessions: number; attention: number }> }
export function overviewStats(rows: readonly Session[], computers: Record<ComputerId, ComputerState>): OverviewStats;
export function liveSessions(rows: readonly Session[]): Session[];
export function createOverview(rows, computers, cards, compute?): { stats: Accessor<OverviewStats>; live: Accessor<Session[]>; cards: Accessor<MachineCardModel[]> };
// ui/lib/usage.ts (additions; UsageWindow.minutes and resetsIn become nullable, label optional)
export const fromProjected: (w: ProjectedWindow, now: number) => UsageWindow;
export const windowsAt: (windows: readonly ProjectedWindow[], now: number) => UsageWindow[];
export const thresholdTone: (used: number) => Tone;
export function severest(windows: readonly UsageWindow[]): Tone | undefined;
export const providerName: (provider: string) => string;
export const usageLoading: (u: Usage) => boolean;
export function summaryText(u: Usage): string;
export function stateText(u: Usage): string;
export function dayTime(seconds: number): string;   // "28 Oct 17:05", local time
export function clock(seconds: number): string;     // "17:05"
export function accountTooltip(u: Usage, now: number): string;
```

Rules:
- Machine cards apply inside the catalog's batch transaction, right after the computer statuses. An `ended` computer drops its card.
- **Screen visibility:**
  - The visible screen is the page's (`dashboard` is `overview`), and none while the window is hidden.
  - A change hides the previous screen and shows the next. The next screen's view is pulled once `screen_visible` answers.
  - A `screens-changed` event for the visible screen pulls its view again. `overview` and `accounts` pull the same accounts view.
- **Pulls:** only the answer to the latest pull of a view is applied.
- **Overview selectors** run while the Overview exists: one pass over the catalog per batch, never on a timer. Only a `live` stream is fresh.
- **Session order** in the live list: needs attention first, then the latest event, then the row key.

- [ ] **Step 1: Write the shared test helpers** (`client/src/store/testing.ts`; not a test file itself):

```ts
import type { ComputerId, MachineMeta, ProjectedWindow, RowOut, Usage, ViewRow } from "../types";

// Synthetic data for the store and screen tests.

export const NOW = 1_793_000_000;

export function machineMeta(host: string, metrics: Partial<MachineMeta["metrics"]> = {}, extra: Partial<MachineMeta> = {}): MachineMeta {
  return {
    host, hgs_version: "1.56.0", platform: "linux", ok: true, local_sessions: null, local_host: null,
    metrics: {
      cpu_percent: 12, cpu_count: 8, cpu_state: "available", cpu_interval_seconds: 5, load_1: 0.5, load_5: 0.4, load_15: 0.3,
      memory_used_bytes: 4 * 2 ** 30, memory_total_bytes: 16 * 2 ** 30, memory_available_bytes: 12 * 2 ** 30,
      uptime_seconds: 3600, sampled_at: NOW, ...metrics,
    },
    presence: { idle_seconds: null, locked: null, locked_seconds: null },
    ...extra,
  };
}

export const rowOut = (computer: ComputerId, i: number, extra: Partial<ViewRow> = {}): RowOut => ({
  computer, key: `run:${i}`,
  row: { name: `claude/sample-project/t${i}`, agent: "claude", status: "ready", phase: "idle", unread: false,
         machine: computer || "workstation", project: "sample-project", updated_at: NOW + i, ...extra },
});

export const projected = (extra: Partial<ProjectedWindow> = {}): ProjectedWindow =>
  ({ id: "five_hour", label: "", used: 34, minutes: 300, resets_at: NOW + 120 * 60, ...extra });

export const usageOf = (extra: Partial<Usage> = {}): Usage => ({
  status: "ok", provider: "claude", label: "Work", auth_status: "", checked_at: null,
  identity: { email: "", name: "", plan: "", organization: "", account_id: "", auth_method: "" },
  windows: [], balances: [], credits: null, refreshing: false, refresh_error: false, offline: false, ...extra,
});
```

- [ ] **Step 2: Write the failing tests.**

`client/src/ui/lib/usage.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { accountTooltip, current, dayTime, forecast, fromProjected, pace, period, remaining, severest, stateText, summaryText, windowText } from "./usage";
import { NOW, projected, usageOf } from "../../store/testing";

describe("usage windows (inventory 8.4, gaps 1-4 and 7)", () => {
  it("1: adapts a projected window to minutes from now", () => {
    expect(fromProjected(projected(), NOW)).toEqual({ minutes: 300, used: 34, label: "", resetsIn: 120 });
    expect(fromProjected(projected({ resets_at: null }), NOW).resetsIn).toBeNull();
  });

  it("2: a missing reset counts as current and keeps the threshold tone", () => {
    const w = fromProjected(projected({ resets_at: null, used: 75 }), NOW);
    expect(current(w)).toBe(true);
    expect(pace(w)).toMatchObject({ known: false, tone: "warning" });
    expect(windowText(w)).toBe("5h 75%");
    expect(current(fromProjected(projected({ resets_at: NOW - 1 }), NOW))).toBe(false);
  });

  it("3: period reads the label and a missing length", () => {
    const at = (minutes: number | null, label = "") => period({ minutes, used: 0, resetsIn: null, label });
    expect([at(300), at(10080), at(2880), at(45), at(null), at(null, "Monthly"), at(10080, "Sonnet"), at(0)])
      .toEqual(["5h", "7d", "2d", "45m", "Usage", "Monthly", "Sonnet 7d", "Usage"]);
  });

  it("4: an unknown reset reads 'reset unknown'", () => {
    expect(remaining(null)).toBe("reset unknown");
    expect(windowText(fromProjected(projected({ label: "Monthly", minutes: null, used: 100, resets_at: null }), NOW))).toBe("Monthly 100% (reset unknown)");
    expect(remaining(0)).toBe("refresh needed");
    expect(remaining(61)).toBe("1h 1m");
  });

  // Review Focus 3.
  it("ISO-parsed, null and unlengthed windows never show NaN", () => {
    for (const w of [projected({ resets_at: null }), projected({ minutes: null, label: "Monthly" }), projected({ minutes: null, resets_at: null }), projected({ used: 100 })]) {
      const at = fromProjected(w, NOW);
      expect(`${windowText(at)} ${forecast(at)} ${period(at)}`).not.toMatch(/NaN|undefined|null/);
    }
  });

  it("7: the tooltip has identity lines, windows, balances and the refresh notes", () => {
    const u = usageOf({
      identity: { email: "person@example.com", name: "Example Person", plan: "max", organization: "Example Org", account_id: "acct-1", auth_method: "oauth" },
      windows: [projected({ resets_at: null }), projected({ id: "m", label: "Monthly", minutes: null, used: 50, resets_at: null })],
      balances: [{ kind: "Topped up", balance: "18.40", currency: "USD" }],
      checked_at: new Date(2026, 9, 28, 17, 5).getTime() / 1000, refresh_error: true, offline: true,
    });
    expect(accountTooltip(u, NOW).split("\n")).toEqual([
      "Claude", "Work", "Example Person", "person@example.com", "max", "Example Org",
      "5h: 34% used", "Monthly: 50% used", "Topped up: 18.40 USD", "Updated 17:05",
      "Update failed. Showing the last available values.", "Machine is offline.", "Updates every 5 minutes. Open account details.",
    ]);
    const resetting = accountTooltip(usageOf({ windows: [projected()] }), NOW).split("\n");
    expect(resetting[3]).toBe(`Resets ${dayTime(NOW + 7200)} (in 2h 0m)`);
    expect(accountTooltip(usageOf({ status: "signed_out", refreshing: true, label: "Claude" }), NOW).split("\n")).toEqual([
      "Claude", "Updating… Previous values remain visible.",
      "No native subscription sign-in found. API-key sessions may still work.", "Updates every 5 minutes. Open account details.",
    ]);
  });

  it("severest takes the worst pace tone of the current windows", () => {
    const fast = fromProjected(projected({ used: 60, resets_at: NOW + 240 * 60 }), NOW);
    const calm = fromProjected(projected({ used: 10, resets_at: NOW + 60 * 60 }), NOW);
    expect(pace(fast).tone).toBe("critical");
    expect(severest([calm, fast])).toBe("critical");
    expect(severest([calm])).toBe("normal");
    expect(severest([fromProjected(projected({ resets_at: NOW - 60 }), NOW)])).toBeUndefined();
    expect(severest([])).toBeUndefined();
  });

  it("summaries and states follow Qt", () => {
    expect(summaryText(usageOf({ balances: [{ kind: "Topped up", balance: "18.40", currency: "USD" }] }))).toBe("18.40 USD");
    expect(summaryText(usageOf({ status: "configured", provider: "dsh" }))).toBe("API key");
    expect(summaryText(usageOf({ status: "configured", provider: "opencode" }))).toBe("Configured");
    expect(summaryText(usageOf({ status: "signed_out" }))).toBe("Sign in required");
    expect(summaryText(usageOf({ status: "expired" }))).toBe("Sign-in expired");
    expect(summaryText(usageOf({ status: "loading" }))).toBe("Unavailable");
    expect(stateText(usageOf({ status: "loading" }))).toBe("Loading account details…");
    expect(stateText(usageOf({ status: "ok", offline: true }))).toBe("Machine is offline. Showing the last available account data.");
    expect(stateText(usageOf({ status: "signed_out", provider: "dsh" })))
      .toBe("DeepSeek account is signed out. API-key connections are configured separately in the native settings.");
  });

  it("formats local dates as Qt's d MMM HH:mm", () => {
    expect(dayTime(new Date(2026, 9, 28, 7, 5).getTime() / 1000)).toBe("28 Oct 07:05");
  });
});
```

`client/src/store/machines.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog } from "./catalog";
import { createMachineCards } from "./machines";
import { machineMeta } from "./testing";
import { emptyBatch } from "../types";

describe("machine cards", () => {
  it("apply right after the computer statuses and drop with an ended computer", () => {
    createRoot((dispose) => {
      const machines = createMachineCards();
      const seen: string[] = [];
      const { catalog, applyBatch } = createCatalog((b) => {
        seen.push(Object.keys(catalog.computers).join(","));
        machines.applyMachines(b);
      });
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }, { computer: "studio", status: "live", attempt: 0 }],
        machines: [{ computer: "", machine: machineMeta("workstation") }, { computer: "studio", machine: machineMeta("studio") }] });
      expect(seen).toEqual([",studio"]);
      expect(machines.cards[""].host).toBe("workstation");
      applyBatch({ ...emptyBatch(), machines: [{ computer: "", machine: machineMeta("workstation", { cpu_percent: 40 }) }] });
      expect(machines.cards[""].metrics.cpu_percent).toBe(40);
      applyBatch({ ...emptyBatch(), computers: [{ computer: "studio", status: "ended", attempt: 0 }] });
      expect(machines.cards.studio).toBeUndefined();
      expect(machines.cards[""]).toBeDefined();
      dispose();
    });
  });
});
```

`client/src/store/overview.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog, toSession } from "./catalog";
import { connectionOf, createOverview, fleetCards, liveSessions, overviewStats } from "./overview";
import { machineMeta, rowOut } from "./testing";
import { emptyBatch } from "../types";

const live = (id: string) => ({ id, status: "live", attempt: 0 });

describe("overview selectors", () => {
  it("map the stream states as the rulings say", () => {
    expect(["live", "probing", "reconnecting", "unsupported", "ended", undefined].map((s) => connectionOf(s))).toEqual([
      { tile: "online", text: "Connected", fresh: true },
      { tile: "connecting", text: "Connecting", fresh: false },
      { tile: "connecting", text: "Reconnecting", fresh: false },
      { tile: "offline", text: "Update hgs", fresh: false },
      { tile: "offline", text: "Offline", fresh: false },
      { tile: "offline", text: "Offline", fresh: false },
    ]);
  });

  it("list this computer unless it is a control console, then the peers that have not ended", () => {
    const computers = { "": live(""), studio: { id: "studio", status: "reconnecting", attempt: 1 }, old: { id: "old", status: "ended", attempt: 0 }, alpha: { id: "alpha", status: "probing", attempt: 0 } };
    const cards = fleetCards(computers, { "": machineMeta("workstation"), studio: machineMeta("studio", {}, { platform: "macos" }) });
    expect(cards.map((c) => [c.name, c.filter, c.local, c.platform])).toEqual([
      ["workstation", "workstation", true, "linux"], ["alpha", "alpha", false, undefined], ["studio", "studio", false, "macos"],
    ]);
    const console = fleetCards(computers, { "": machineMeta("workstation", {}, { local_sessions: false }) });
    expect(console.map((c) => c.name)).toEqual(["alpha", "studio"]);
  });

  it("order the live sessions attention first, then the latest event, then the key", () => {
    const rows = [
      toSession("", "run:1", rowOut("", 1, { status: "working", updated_at: 10 }).row),
      toSession("", "run:2", rowOut("", 2, { status: "attention", phase: "approval", updated_at: 5 }).row),
      toSession("", "run:3", rowOut("", 3, { status: "working", updated_at: 20 }).row),
      toSession("", "run:4", rowOut("", 4, { status: "ready" }).row),
      toSession("", "run:0", rowOut("", 0, { status: "working", updated_at: 20 }).row),
    ];
    expect(liveSessions(rows).map((s) => s.catalogKey)).toEqual(["run:2", "run:0", "run:3", "run:1"]);
  });

  it("count 20,000 rows once per batch, with no timer", () => {
    createRoot((dispose) => {
      const { catalog, applyBatch } = createCatalog();
      const rows = Array.from({ length: 20_000 }, (_, i) => rowOut(i % 2 ? "studio" : "", i));
      applyBatch({ ...emptyBatch(), computers: [live(""), { id: "studio", status: "reconnecting", attempt: 1 }].map((c) => ({ computer: c.id, status: c.status, attempt: c.attempt })),
        replaces: [{ computer: "", rows: rows.filter((r) => r.computer === "") }, { computer: "studio", rows: rows.filter((r) => r.computer === "studio") }] });
      const compute = vi.fn(overviewStats);
      const ov = createOverview(() => catalog.order.map((id) => catalog.rows[id]), () => catalog.computers, () => ({}), compute);
      expect(ov.stats()).toMatchObject({ all: 20_000, attention: 0, offline: 10_000 });
      expect(compute).toHaveBeenCalledTimes(1);
      applyBatch({ ...emptyBatch(), upserts: Array.from({ length: 100 }, (_, i) => rowOut("", i * 2, { status: "attention", phase: "approval" })) });
      expect(ov.stats().attention).toBe(100);
      expect(ov.stats().per[""]).toEqual({ sessions: 10_000, attention: 100 });
      expect(ov.stats().per.studio).toEqual({ sessions: 10_000, attention: 0 });
      expect(compute).toHaveBeenCalledTimes(2);
      dispose();
    });
  });
});
```

`client/src/store/screens.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { screenOf, startScreens } from "./screens";
import { latest } from "./latest";
import { emptyAccounts, emptyMachines, emptyProjects } from "../types";
import type { Page } from "./ui";

vi.mock("../bridge", () => ({
  screenVisible: vi.fn(async () => {}),
  onScreensChanged: vi.fn(() => () => {}),
  accountsView: vi.fn(async () => emptyAccounts()),
  machinesView: vi.fn(async () => emptyMachines()),
  machineResolve: vi.fn(async () => emptyMachines()),
  machineCheck: vi.fn(async () => emptyMachines()),
  projectsView: vi.fn(async () => emptyProjects()),
}));
const mocked = vi.mocked(bridge);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("screens wiring", () => {
  beforeEach(() => vi.clearAllMocks());

  it("maps pages to screens and hides them with the window", () => {
    expect(screenOf("dashboard", false)).toBe("overview");
    expect(screenOf("machines", false)).toBe("machines");
    expect(screenOf("sessions", false)).toBeUndefined();
    expect(screenOf("accounts", true)).toBeUndefined();
  });

  it("tells the hub which screen shows and pulls it", async () => {
    let hidden = false;
    const listeners: (() => void)[] = [];
    const [page, setPage] = createSignal<Page>("sessions");
    const stop = startScreens(page, { hidden: () => hidden, onChange: (l) => { listeners.push(l); return () => {}; } });
    setPage("dashboard");
    await settle();
    expect(mocked.screenVisible.mock.calls).toEqual([["overview", true]]);
    expect(mocked.accountsView).toHaveBeenCalledTimes(1);
    setPage("machines");
    await settle();
    expect(mocked.screenVisible.mock.calls.slice(1)).toEqual([["overview", false], ["machines", true]]);
    expect(mocked.machinesView).toHaveBeenCalledWith(false);
    hidden = true;
    listeners.forEach((l) => l());
    await settle();
    expect(mocked.screenVisible.mock.calls.at(-1)).toEqual(["machines", false]);
    stop();
  });

  it("re-pulls the visible screen on screens-changed", async () => {
    let fire: (screen: "projects" | "accounts") => void = () => {};
    mocked.onScreensChanged.mockImplementation((listener) => { fire = listener; return () => {}; });
    const [page] = createSignal<Page>("projects");
    const stop = startScreens(page, { hidden: () => false, onChange: () => () => {} });
    await settle();
    mocked.projectsView.mockClear();
    fire("accounts");
    fire("projects");
    await settle();
    expect(mocked.projectsView).toHaveBeenCalledTimes(1);
    expect(mocked.accountsView).not.toHaveBeenCalled();
    stop();
  });

  it("applies only the answer to the latest pull", async () => {
    const got: number[] = [];
    const pull = latest<number>((value) => got.push(value));
    let first!: (n: number) => void;
    const slow = pull(() => new Promise<number>((resolve) => { first = resolve; }));
    await pull(async () => 2);
    first(1);
    await slow;
    expect(got).toEqual([2]);
  });
});
```

In `client/src/bridge.test.ts`, inside `describe("pump", ...)`:

```ts
  it("keeps machine cards and counts a batch of only cards as content", async () => {
    const src = source();
    const h = host();
    const got: Batch[] = [];
    pump(src.s, (b) => got.push(b), h.h);
    src.answer({ machines: [{ computer: "", machine: { host: "workstation" } }] } as unknown as Batch);
    await settle();
    h.runFrames();
    expect(got).toHaveLength(1);
    expect(got[0].machines).toHaveLength(1);
    expect(got[0].upserts).toEqual([]);
  });
```

- [ ] **Step 3: Run** `npx vitest run src/ui/lib/usage.test.ts src/store/machines.test.ts src/store/overview.test.ts src/store/screens.test.ts src/bridge.test.ts` in `client`. Expected: FAIL (missing modules and exports).

- [ ] **Step 4: Implement.**

`client/src/types.ts`: `Batch` gains `machines?: MachineOut[];` after `computers`. `emptyBatch` returns `machines: []` too, and `isEmptyBatch` also checks `!b.machines?.length`. Append:

```ts
/** A computer's machine card from its `catalog.meta` (client/core `machine.rs`); an older host leaves fields null. */
export interface MachineMetrics {
  cpu_percent: number | null;
  cpu_count: number | null;
  /** `available`, `warming` or `unavailable`. */
  cpu_state: string | null;
  cpu_interval_seconds: number | null;
  load_1: number | null;
  load_5: number | null;
  load_15: number | null;
  memory_used_bytes: number | null;
  memory_total_bytes: number | null;
  memory_available_bytes: number | null;
  uptime_seconds: number | null;
  /** Unix seconds. */
  sampled_at: number | null;
}
/** This Windows computer's session host. */
export interface LocalHost { state: string | null; pid: number | null; conpty: string | null; integrity: string | null; windows_session: number | null; error: string | null }
export interface MachineMeta {
  host: string | null;
  hgs_version: string | null;
  platform: string | null;
  ok: boolean | null;
  metrics: MachineMetrics;
  presence: { idle_seconds: number | null; locked: boolean | null; locked_seconds: number | null };
  /** False only on a control console. */
  local_sessions: boolean | null;
  local_host: LocalHost | null;
}
export interface MachineOut { computer: ComputerId; machine: MachineMeta }

// The screen views the Rust side projects (client/core `views`, client/src-tauri `screens.rs`).

export type Screen = "overview" | "projects" | "machines" | "accounts";
export interface ProjectedWindow { id: string; label: string; used: number; minutes: number | null; resets_at: number | null }
export interface Identity { email: string; name: string; plan: string; organization: string; account_id: string; auth_method: string }
export interface Usage {
  status: string;
  provider: string;
  label: string;
  auth_status: string;
  checked_at: number | null;
  identity: Identity;
  windows: ProjectedWindow[];
  balances: { kind: string; balance: string; currency: string }[];
  credits: { balance: string; unlimited: boolean } | null;
  refreshing: boolean;
  refresh_error: boolean;
  offline: boolean;
}
export type MemberState = "ready" | "offline" | "not-installed" | "apikey" | "signed-out" | "keychain" | "desktop" | "unavailable" | "expired" | "checking" | "unknown";
export interface OpenCodeInfo { providers: { label: string; connection: string; models: string }[]; truncated: boolean; stale_catalog_at: number | null; stopped: boolean }
export interface Profile {
  key: string; host: ComputerId; machine: string; id: string; label: string; provider: string;
  native: boolean; installed: boolean; is_default: boolean; home: string; auth_revision: string; online: boolean;
  state: MemberState; detail: string; signed_in: boolean;
  permission: string; permission_label: string; permission_hint: string; model_supported: boolean; model_text: string;
  usage: Usage; opencode: OpenCodeInfo | null;
}
export interface Account {
  key: string; label: string; provider: string; installed: boolean; machines: string[]; members: Profile[];
  usage: Usage; permission: string; identity_lines: string[]; identity_note: string;
}
export interface AccountHost { computer: ComputerId; name: string; label: string; online: boolean }
export interface AccountsView {
  hosts: AccountHost[]; accounts: Account[]; overview: Account[]; snapshot: boolean; loading: boolean;
  pending: number; loaded_machines: number; busy: boolean; notices: string[]; no_hosts: string | null;
}
export interface MachineRow { alias: string; enabled: boolean; source: string; inherited: boolean; hostname: string; user: string; port: number | null; identity_file: string; origin: string }
export interface Effective { alias: string; pending: boolean; error: boolean; text: string; hostname: string; user: string; port: string; key: string }
export interface CheckResult { alias: string; pending: boolean; error: boolean; text: string }
export interface MachinesView {
  loaded: boolean; refreshing: boolean; error: string | null; local: string; environment_override: boolean;
  machines: MachineRow[]; effective: Effective | null; check: CheckResult | null; busy: boolean;
}
export interface Folder { id: string; name: string; path: string; computer: string; machine: string }
export interface Project { id: string; name: string; color: string | null; accessible: boolean; is_default: boolean; palette: number; folders: Folder[] }
export interface ProjectsView {
  loaded: boolean; refreshing: boolean; status: string; status_error: boolean; conflicts: number;
  unconnected: string[]; notice: string | null; projects: Project[];
}

export const emptyAccounts = (notices: string[] = []): AccountsView => ({
  hosts: [], accounts: [], overview: [], snapshot: notices.length > 0, loading: false, pending: 0, loaded_machines: 0, busy: false, notices, no_hosts: null,
});
export const emptyMachines = (error: string | null = null): MachinesView => ({
  loaded: false, refreshing: false, error, local: "", environment_override: false, machines: [], effective: null, check: null, busy: false,
});
export const emptyProjects = (status = ""): ProjectsView => ({
  loaded: false, refreshing: false, status, status_error: status !== "", conflicts: 0, unconnected: [], notice: null, projects: [],
});
```

`client/src/bridge.ts`:
- Add `import { listen } from "@tauri-apps/api/event";`, and add `emptyAccounts`, `emptyMachines`, `emptyProjects`, `AccountsView`, `MachinesView`, `ProjectsView` and `Screen` to the `./types` import.
- In `normalize`, add `machines: b.machines ?? []` to the returned object.
- Append:

```ts
/** What the browser build shows instead of the screen reads. */
const DESKTOP_ONLY = "Reads need the desktop app.";

/** Tells the screens hub which screen shows; its reads run only while it does. */
export async function screenVisible(screen: Screen, visible: boolean): Promise<void> {
  if (!inTauri()) return;
  await invoke("screen_visible", { screen, visible }).catch((error) => console.warn("Zerus Next: screen_visible failed", error));
}

/** The Accounts and Overview account data; `force` is Refresh. */
export async function accountsView(force = false): Promise<AccountsView> {
  if (!inTauri()) return emptyAccounts([DESKTOP_ONLY]);
  return invoke<AccountsView>("accounts_view", { force });
}

/** The Machines view; `force` is Refresh. */
export async function machinesView(force = false): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machines_view", { force });
}

/** Reads the selected peer's effective SSH connection. */
export async function machineResolve(alias: string): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machine_resolve", { alias });
}

/** "Test connection": `machine check`, up to 25 s. */
export async function machineCheck(alias: string): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machine_check", { alias });
}

export async function projectsView(): Promise<ProjectsView> {
  if (!inTauri()) return emptyProjects(DESKTOP_ONLY);
  return invoke<ProjectsView>("projects_view");
}

/** Calls `listener` with the screen whose view changed (`screens-changed`); returns the stop. */
export function onScreensChanged(listener: (screen: Screen) => void): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  let stopped = false;
  void listen<string>("screens-changed", (event) => listener(event.payload as Screen)).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
  return () => {
    stopped = true;
    stop?.();
  };
}
```

`client/src/dev/replay.ts`: `Fixture` gains `machines?: MachineOut[];` (import the type), and the first push becomes `push({ computers: fixture.computers, machines: fixture.machines ?? [], replaces: fixture.snapshot });`.

`client/src/store/latest.ts`:

```ts
/** Applies only the answer to the latest request, so a slow older pull never overwrites a newer view. */
export function latest<T>(set: (value: T) => void) {
  let sequence = 0;
  return async (request: () => Promise<T>) => {
    const mine = ++sequence;
    const value = await request();
    if (mine === sequence) set(value);
  };
}
```

`client/src/store/attention.ts`: move the two definitions out of `store/ui.ts` unchanged:

```ts
import type { Session } from "../ui/sessions/types";

// Shared by the session list and the Overview; pure, so tests can import it
// without the page state of store/ui.ts.

/** Needs attention: actionable, unread reply or review later (muted rows are listed, not counted). */
export const needsAttention = (s: Session) => s.status === "attention" || (s.unread && s.status !== "offline") || !!s.reviewLater;

/** Active = current work OR Scheduled OR Needs attention OR a local draft. */
export const isActive = (s: Session) =>
  s.status === "working" || s.status === "background" || s.status === "scheduled" || s.status === "waiting" || needsAttention(s) || s.draft;
```

In `store/ui.ts`, delete those two definitions and add `import { isActive, needsAttention } from "./attention";` plus `export { isActive, needsAttention };` after the imports.

`client/src/store/machines.ts`:

```ts
import { createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { machineCheck, machineResolve, machinesView } from "../bridge";
import { emptyMachines, type Batch, type ComputerId, type MachineMeta, type MachinesView } from "../types";
import { latest } from "./latest";

// Machine cards: each computer's latest catalog.meta projection
// (Batch.machines). The catalog applies them right after the computer
// statuses (the core's apply order); an ended computer drops its card.
// Also the Machines page's view from the screens hub.

export function createMachineCards() {
  const [cards, setCards] = createStore<Record<ComputerId, MachineMeta>>({});
  function applyMachines(b: Batch) {
    const ended = b.computers.filter((c) => c.status === "ended" && cards[c.computer]).map((c) => c.computer);
    if (ended.length) setCards(produce((all) => { for (const id of ended) delete all[id]; }));
    for (const m of b.machines ?? []) setCards(m.computer, reconcile(m.machine));
  }
  return { cards, applyMachines };
}

export const { cards: machineCards, applyMachines } = createMachineCards();

export const [machinesPage, setMachinesPage] = createSignal<MachinesView>(emptyMachines());
const pull = latest(setMachinesPage);
export const loadMachines = (force = false) => pull(() => machinesView(force));
export const resolveMachine = (alias: string) => pull(() => machineResolve(alias));
export const testConnection = (alias: string) => pull(() => machineCheck(alias));
```

`client/src/store/accounts.ts`:

```ts
import { createSignal } from "solid-js";
import { accountsView } from "../bridge";
import { emptyAccounts, type AccountsView } from "../types";
import { latest } from "./latest";

// The Accounts screen and the Overview's account cards share one view from
// the screens hub. The machine filter and the profile to select are UI state.

export const [accountsPage, setAccountsPage] = createSignal<AccountsView>(emptyAccounts());
const pull = latest(setAccountsPage);
export const loadAccounts = (force = false) => pull(() => accountsView(force));
/** The machine the Accounts list shows: a computer id, or "*" for all machines. */
export const [accountMachine, setAccountMachine] = createSignal("*");
/** The profile key an Overview card asks Accounts to select. */
export const [requestedProfile, setRequestedProfile] = createSignal<string | undefined>();
```

`client/src/store/projects.ts`:

```ts
import { createSignal } from "solid-js";
import { projectsView } from "../bridge";
import { emptyProjects, type ProjectsView } from "../types";
import { latest } from "./latest";

export const [projectsPage, setProjectsPage] = createSignal<ProjectsView>(emptyProjects());
const pull = latest(setProjectsPage);
export const loadProjects = () => pull(() => projectsView());
```

`client/src/store/screens.ts`:

```ts
import { createEffect, createRoot, createSignal, on, type Accessor } from "solid-js";
import { onScreensChanged, screenVisible } from "../bridge";
import type { Screen } from "../types";
import type { PageVisibility } from "./visibility";
import type { Page } from "./ui";
import { loadAccounts } from "./accounts";
import { loadMachines } from "./machines";
import { loadProjects } from "./projects";

// The screens hub runs a screen's reads only while it shows: this keeps
// `screen_visible` in step with the page and the window, and pulls a view
// again when the hub reports it changed.

const SCREEN: Partial<Record<Page, Screen>> = { dashboard: "overview", projects: "projects", machines: "machines", accounts: "accounts" };

/** The screen whose reads run: the page's, while the window shows. */
export const screenOf = (page: Page, hidden: boolean): Screen | undefined => (hidden ? undefined : SCREEN[page]);

/** Pulls a screen's view again; Overview and Accounts share the accounts view. */
export function reload(screen: Screen): Promise<void> {
  if (screen === "overview" || screen === "accounts") return loadAccounts();
  if (screen === "machines") return loadMachines();
  return loadProjects();
}

/** Starts the wiring; returns its stop. */
export function startScreens(page: Accessor<Page>, visibility: PageVisibility): () => void {
  return createRoot((dispose) => {
    const [hidden, setHidden] = createSignal(visibility.hidden());
    const stopVisibility = visibility.onChange(() => setHidden(visibility.hidden()));
    const stopEvents = onScreensChanged((screen) => {
      if (screen === screenOf(page(), hidden())) void reload(screen);
    });
    createEffect(on(() => screenOf(page(), hidden()), (next, previous) => {
      if (previous) void screenVisible(previous, false);
      if (next) void screenVisible(next, true).then(() => reload(next));
    }));
    return () => {
      stopVisibility();
      stopEvents();
      dispose();
    };
  });
}
```

`client/src/store/overview.ts`:

```ts
import { createMemo, type Accessor } from "solid-js";
import type { ComputerState } from "./catalog";
import { isActive, needsAttention } from "./attention";
import type { ComputerId, MachineMeta } from "../types";
import type { Session } from "../ui/sessions/types";
import type { MachineState } from "../ui/components/MachineTile";
import type { OsKind } from "../ui/lib/os-icons";

// The Overview's selectors (DashboardPage::render, DashboardPage.cpp:401-456).
// The Overview screen creates them, so they cost nothing on other pages; each
// is one pass over the catalog per batch and none runs on a timer.

/** A machine's connection as the Overview and Machines show it (Decisions note). */
export interface Connection { tile: MachineState; text: string; fresh: boolean }

/** Only a live stream is fresh: the "Machines online" tile, the load colours and the offline notice count it. */
export function connectionOf(status: string | undefined): Connection {
  switch (status) {
    case "live": return { tile: "online", text: "Connected", fresh: true };
    case "probing": return { tile: "connecting", text: "Connecting", fresh: false };
    case "reconnecting": return { tile: "connecting", text: "Reconnecting", fresh: false };
    case "unsupported": return { tile: "offline", text: "Update hgs", fresh: false };
    default: return { tile: "offline", text: "Offline", fresh: false };
  }
}

export const osOf = (platform: string | null | undefined): OsKind | undefined =>
  platform === "linux" || platform === "macos" || platform === "windows" ? platform : undefined;

export interface MachineCardModel {
  computer: ComputerId;
  /** The badge: this computer's host name, a peer's alias. */
  name: string;
  /** The Sessions machine filter's value for this computer's rows. */
  filter: string;
  local: boolean;
  platform?: OsKind;
  connection: Connection;
  meta?: MachineMeta;
}

/** This computer unless it is a control console (DashboardPage.cpp:407-409), then every peer that has not ended. */
export function fleetCards(computers: Record<ComputerId, ComputerState>, cards: Record<ComputerId, MachineMeta>): MachineCardModel[] {
  const out: MachineCardModel[] = [];
  const localMeta = cards[""];
  if ((computers[""] || localMeta) && localMeta?.local_sessions !== false) {
    out.push({
      computer: "", name: localMeta?.host || "This machine", filter: localMeta?.host || "this computer", local: true,
      platform: osOf(localMeta?.platform), connection: connectionOf(computers[""]?.status), meta: localMeta,
    });
  }
  for (const id of Object.keys(computers).filter((id) => id !== "" && computers[id].status !== "ended").sort()) {
    out.push({ computer: id, name: id, filter: id, local: false, platform: osOf(cards[id]?.platform), connection: connectionOf(computers[id].status), meta: cards[id] });
  }
  return out;
}

export interface OverviewStats {
  all: number;
  active: number;
  attention: number;
  /** Rows on machines whose stream is not live (the offline notice). */
  offline: number;
  per: Record<ComputerId, { sessions: number; attention: number }>;
}

export function overviewStats(rows: readonly Session[], computers: Record<ComputerId, ComputerState>): OverviewStats {
  const stats: OverviewStats = { all: 0, active: 0, attention: 0, offline: 0, per: {} };
  for (const s of rows) {
    const per = (stats.per[s.computer] ??= { sessions: 0, attention: 0 });
    stats.all++;
    per.sessions++;
    if (!s.muted && needsAttention(s)) { stats.attention++; per.attention++; }
    if (isActive(s)) stats.active++;
    if (computers[s.computer]?.status !== "live") stats.offline++;
  }
  return stats;
}

/** Working, attention and draft sessions: attention first, then the latest event, then the key (DashboardPage.cpp:452-456). */
export function liveSessions(rows: readonly Session[]): Session[] {
  return rows.filter(isActive).sort((a, b) =>
    Number(needsAttention(b)) - Number(needsAttention(a))
    || (b.updatedAt ?? 0) - (a.updatedAt ?? 0)
    || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

export function createOverview(
  rows: Accessor<readonly Session[]>,
  computers: Accessor<Record<ComputerId, ComputerState>>,
  cards: Accessor<Record<ComputerId, MachineMeta>>,
  compute: typeof overviewStats = overviewStats,
) {
  return {
    stats: createMemo(() => compute(rows(), computers())),
    live: createMemo(() => liveSessions(rows())),
    cards: createMemo(() => fleetCards(computers(), cards())),
  };
}
```

`client/src/store/catalog.ts`: `export function createCatalog(onMachines?: (b: Batch) => void)`. In `applyBatch`, right after the `for (const c of b.computers) ...` line, add `onMachines?.(b);`. The module's own catalog becomes `createCatalog(applyMachines)`, with `import { applyMachines } from "./machines";`.

`client/src/store/session.ts`: add `import { startScreens } from "./screens";`, `import { page } from "./ui";` (beside the existing `./ui` import) and `import { documentVisibility } from "./visibility";`, then at the end:

```ts
// The Overview, Projects, Machines and Accounts reads run while their page shows.
startScreens(page, documentVisibility);
```

`client/src/ui/lib/usage.ts` (whole file):

```ts
// Account limit windows and their pace, ported from tray/src/AccountUsage.h.
// Pace compares used quota with the share of the window that has passed:
// spending faster than that share exhausts the limit before it resets.
// The core projects resets as Unix seconds; `fromProjected` turns them into
// minutes from `now` on each 15 s render, so nothing else runs on a timer.
import type { ProjectedWindow, Usage } from "../../types";

export interface UsageWindow {
  /** Window length in minutes (300 = 5 h, 10080 = 7 d); null when the provider reports none. */
  minutes: number | null;
  /** Used share, 0..100. */
  used: number;
  /** Minutes until reset; null when unknown (it counts as current, as in Qt); not positive once ended. */
  resetsIn: number | null;
  /** The provider's name for the window ("Sonnet", "Monthly"). */
  label?: string;
}

export type Tone = "normal" | "warning" | "critical";

export interface Pace {
  /** The window length and a future reset are known. */
  known: boolean;
  /** The first 8% of a window is too short to extrapolate. */
  early: boolean;
  used: number;
  /** Share of the window that has passed, 0..1. */
  elapsed: number;
  minutesLeft: number;
  /** Minutes until 100% at the average pace; -1 when it lasts until reset. */
  runsOutIn: number;
  /** Percent at reset at the average pace; -1 when unknown. */
  projected: number;
  tone: Tone;
}

/** A projected window at `now` (Unix seconds). */
export const fromProjected = (w: ProjectedWindow, now: number): UsageWindow => ({
  minutes: w.minutes, used: w.used, label: w.label, resetsIn: w.resets_at === null ? null : (w.resets_at - now) / 60,
});

export const windowsAt = (windows: readonly ProjectedWindow[], now: number) => windows.map((w) => fromProjected(w, now));

export const thresholdTone = (used: number): Tone => (used >= 90 ? "critical" : used >= 70 ? "warning" : "normal");

/** AccountUsage::current: a reset that is unknown or still ahead. */
export const current = (w: UsageWindow) => w.resetsIn === null || w.resetsIn > 0;

export function pace(w: UsageWindow): Pace {
  const used = Math.max(0, Math.min(100, w.used));
  const result: Pace = { known: false, early: false, used, elapsed: 0, minutesLeft: -1, runsOutIn: -1, projected: -1, tone: thresholdTone(used) };
  if (!w.minutes || w.minutes <= 0 || w.resetsIn === null || w.resetsIn <= 0) return result;
  result.known = true;
  result.minutesLeft = w.resetsIn;
  const passed = Math.max(0, w.minutes - w.resetsIn);
  result.elapsed = passed / w.minutes;
  result.early = result.elapsed < 0.08;
  // Exhausted and fresh windows keep the fixed 70/90% thresholds.
  if (used >= 100 || result.early) return result;
  result.projected = used / result.elapsed;
  if (result.projected > 100) result.runsOutIn = ((100 - used) * passed) / used;
  result.tone = result.runsOutIn < 0 ? "normal" : result.runsOutIn >= result.minutesLeft / 2 ? "warning" : "critical";
  if (used >= 90 && result.tone === "normal") result.tone = "warning";
  return result;
}

/** AccountUsage::period: the duration after the provider's name; "Usage" when neither is known. */
export function period(w: UsageWindow) {
  const m = w.minutes ?? 0;
  const duration = m === 10080 ? "7d" : m && m % 1440 === 0 ? `${m / 1440}d` : m && m % 60 === 0 ? `${m / 60}h` : m ? `${m}m` : "";
  const name = w.label ?? "";
  return name ? (duration ? `${name} ${duration}` : name) : duration || "Usage";
}

export function remaining(minutes: number | null) {
  if (minutes === null) return "reset unknown";
  const m = Math.max(0, Math.ceil(minutes));
  if (!m) return "refresh needed";
  if (m >= 1440) return `${Math.floor(m / 1440)}d ${Math.floor((m % 1440) / 60)}h`;
  if (m >= 60) return `${Math.floor(m / 60)}h ${m % 60}m`;
  return `${m}m`;
}

/** Footer countdown: only the largest whole unit fits next to the percentage. */
export function countdown(minutes: number) {
  const m = Math.max(1, Math.ceil(minutes));
  return m < 60 ? `${m}m` : m < 2880 ? `${Math.floor(m / 60)}h` : `${Math.floor(m / 1440)}d`;
}

export function windowText(w: UsageWindow, withReset = true) {
  if (!current(w)) return `${period(w)} refresh`;
  return `${period(w)} ${Math.round(w.used)}%${withReset && w.used >= 100 ? ` (${remaining(w.resetsIn)})` : ""}`;
}

export function approximately(minutes: number) {
  const m = Math.max(1, Math.floor(minutes)), d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), r = m % 60;
  if (d) return h ? `${d}d ${h}h` : `${d}d`;
  if (h) return r ? `${h}h ${r}m` : `${h}h`;
  return `${m}m`;
}

export function forecast(w: UsageWindow) {
  if (!current(w)) return "";
  const p = pace(w);
  if (!p.known || p.early || p.used >= 100) return "";
  return p.runsOutIn >= 0 ? `At the current pace, the limit runs out in about ${approximately(p.runsOutIn)}`
    : `At the current pace, about ${Math.round(p.projected)}% by reset`;
}

const TONES: Tone[] = ["normal", "warning", "critical"];

/** AccountUsage::severest: the most urgent pace tone of the current windows; undefined when none reports usage. */
export function severest(windows: readonly UsageWindow[]): Tone | undefined {
  let tone: Tone | undefined;
  for (const w of windows) {
    if (!current(w)) continue;
    const next = pace(w).tone;
    if (!tone || TONES.indexOf(next) > TONES.indexOf(tone)) tone = next;
  }
  return tone;
}

const PROVIDERS: Record<string, string> = { claude: "Claude", codex: "Codex", kimi: "Kimi", opencode: "OpenCode", dsh: "DeepSeek", deepseek: "DeepSeek", sh: "Shell" };
/** IdentityBadges::providerName. */
export const providerName = (provider: string) => PROVIDERS[provider] ?? provider;

/** No window to draw yet: the meters show their placeholders. */
export const usageLoading = (u: Usage) => u.status === "loading" && !u.windows.length;

/** What replaces the meters without windows (AccountUsage::Button::summaryText). */
export function summaryText(u: Usage) {
  const b = u.balances[0];
  if (b) return `${b.balance} ${b.currency}`;
  if (u.status === "configured") return u.provider === "opencode" ? "Configured" : "API key";
  if (u.status === "signed_out") return "Sign in required";
  if (u.status === "expired") return "Sign-in expired";
  return "Unavailable";
}

/** AccountUsage::state: what the usage panel says without windows or balances. */
export function stateText(u: Usage) {
  const s = u.status;
  if (!s || s === "loading") return "Loading account details…";
  if (s === "configured" && u.provider === "opencode") return "OpenCode uses its native provider configuration. Usage is not available here.";
  if (s === "configured") return "API key configured. Usage is available in the DeepSeek console.";
  if (u.offline || s === "offline") return "Machine is offline. Showing the last available account data.";
  if (s === "expired") return "Native sign-in has expired. Open the agent or sign in again, then refresh usage.";
  if (s === "signed_out" && u.provider === "dsh") return "DeepSeek account is signed out. API-key connections are configured separately in the native settings.";
  if (s === "signed_out") return "No native subscription sign-in found. API-key sessions may still work.";
  if (s === "credentials_locked") return "Saved sign-in is in the locked macOS Keychain. Unlock it in Keychain Access, then refresh.";
  if (s === "desktop_session_unavailable") return "Saved sign-in requires the Mac desktop session. Log in to macOS and check the HGS installation, then refresh.";
  if (s === "credentials_unavailable") return "The saved Keychain sign-in could not be used. Check account access or sign in again.";
  return "Usage is unavailable from this provider. Open its native account page or refresh after signing in.";
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const two = (n: number) => String(n).padStart(2, "0");

/** Qt's "d MMM HH:mm" in local time. */
export function dayTime(seconds: number) {
  const d = new Date(seconds * 1000);
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** Qt's "HH:mm" in local time. */
export function clock(seconds: number) {
  const d = new Date(seconds * 1000);
  return `${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** AccountUsage::tooltip (AccountUsage.h:197-219). */
export function accountTooltip(u: Usage, now: number) {
  const lines: string[] = [];
  const provider = providerName(u.provider);
  if (provider) lines.push(provider);
  if (u.label && u.label !== provider) lines.push(u.label);
  for (const value of [u.identity.name, u.identity.email, u.identity.plan, u.identity.organization]) if (value) lines.push(value);
  for (const p of u.windows) {
    const w = fromProjected(p, now);
    lines.push(`${period(w)}: ${Math.round(w.used)}% used`);
    if (p.resets_at !== null) lines.push(`Resets ${dayTime(p.resets_at)}${current(w) ? ` (in ${remaining(w.resetsIn)})` : ""}`);
    const f = forecast(w);
    if (f) lines.push(f);
  }
  for (const b of u.balances) lines.push(`${b.kind}: ${b.balance} ${b.currency}`);
  if (u.checked_at !== null) lines.push(`Updated ${clock(u.checked_at)}`);
  if (u.refreshing) lines.push("Updating… Previous values remain visible.");
  else if (u.refresh_error) lines.push("Update failed. Showing the last available values.");
  if (u.offline) lines.push("Machine is offline.");
  if (!u.windows.length && !u.balances.length) lines.push(stateText(u));
  lines.push("Updates every 5 minutes. Open account details.");
  return lines.join("\n");
}
```

`client/src/ui/components/UsageMeter.tsx`: a countdown needs a known reset. Change `countdownShown` and the countdown call:

```tsx
  const countdownShown = (w?: UsageWindow) => indicator() === "marker-countdown" && !props.compact && !!w && current(w) && w.resetsIn !== null;
```

```tsx
                    {countdown(w!.resetsIn!)}
```

- [ ] **Step 5: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py` in `client`. Expected: PASS (all earlier tests too).

- [ ] **Step 6: Commit**

```bash
git add client/src
git commit -m "feat(client): screen stores, Overview selectors and Qt's usage rules for projected windows"
```

---

### Task 5: The Overview screen

**Files:**
- Create: `client/src/ui/screens/overview-ui.ts`, `client/src/ui/screens/overview-ui.test.ts`, `client/src/ui/screens/Overview.tsx`
- Create: `client/src/ui/styles/screens.css` (a verbatim copy, Step 4)
- Modify: `client/src/App.tsx` (route `dashboard`), `client/src/main.tsx` (import `screens.css`)
- Modify: `client/scripts/make-fixture.py`; regenerate `client/fixtures/replay.ndjson`, `replay-mac.ndjson`, `replay-batches.json`

**Interfaces:**
- Consumes: `createOverview`, `connectionOf`, `MachineCardModel`, `OverviewStats` (Task 4); `machineCards`; `accountsPage`, `loadAccounts`, `setAccountMachine`, `setRequestedProfile`; `windowsAt`, `severest`, `summaryText`, `usageLoading`, `accountTooltip`, `dayTime` (Task 4); `MachineTile`, `UsageMeters`, `AgentBadge`, `MachineBadge`, `SessionCard`; `setPage`, `setMachineFilter`, `setProjectFilter`, `setStateFilter`, `setSelectedKey` (`store/ui.ts`).
- Produces: `export function OverviewScreen(): JSX.Element` and, in `overview-ui.ts`:

```ts
export type Level = "unknown" | "normal" | "warning" | "critical";
export function level(percent: number | undefined, fresh: boolean): Level;
export function loadTip(percent: number | undefined, fresh: boolean): string;
export const bytes: (value: number) => string;
export interface Figure { value: string; percent?: number; detail: string; tip: string }
export function cpuFigure(meta: MachineMeta | undefined): Figure;
export function memoryFigure(meta: MachineMeta | undefined): Figure;
export function sampleNote(meta: MachineMeta | undefined, c: Connection, now: number): string;
export const sampleTip: (c: Connection) => string;
export const sessionsLabel: (sessions: number, attention: number) => string;
export function accountIdentity(a: Account): string;
export function accountTone(a: Account, now: number): Tone | "none";
export function accountsNote(view: AccountsView): string | undefined;
```

Rules (Qt `DashboardPage.cpp` and the Decisions note):
- **Tiles:**
  - Needs attention (`!muted && needsAttention`, "hot" above 0) goes to Sessions with the attention filter.
  - Active goes to the active filter.
  - "Machines online" (`fresh / cards`) goes to Machines.
  - Sessions goes to all sessions.
  - Every tile clears the machine and project filters.
- **Machine card:**
  - It shows the tile with its state dot, the badge, "This machine", the status text and the settings button (Machines).
  - CPU and memory percentages are the main figures, with the load colour from `level`.
  - The details are "N cores, load X.XX" (Qt prints `0.00` without a load) and "A of B" (`bytes`). Without a sample: "Update hgs for metrics" (empty metrics), "CPU metrics unavailable" or "Waiting for second sample", and "Memory unavailable".
  - The tooltips are Qt's.
  - The sessions link reads "N sessions, M need attention" and opens Sessions filtered to that machine. Its tooltip is "View sessions on X" or "View last known sessions on X".
  - The sample note shows only when not current.
- **Account cards:**
  - `view.overview` in order. A card shows the label, provider badge, machine badges, the identity line (email or name, " / " and the plan, or "Agent not installed") and the compact meters.
  - Without windows a card shows `summaryText`; an agent that is not installed reads as unavailable.
  - The stripe takes the pace tone. The tooltip is `accountTooltip`.
  - A click opens Accounts on that account's first profile.
  - Empty texts come from `accountsNote`.
- **Sessions:** the offline notice when `stats.offline > 0`, then the virtualised live cards. A click opens the session; an empty list reads "No sessions are working, need attention or have drafts.".
- **Refresh** forces the account reads and is disabled and spinning while they run.
- **Width:** both edges drag the centred column (720-2400, default 1200), double-click resets, and the width is kept in `localStorage` (`zerus-client-overview-width`). "Full-width Overview" arrives with Settings in 3b.
- **Lists:** machine and account cards render with `<Index>`, so a batch updates bound text in place instead of rebuilding cards.

- [ ] **Step 1: Write the failing tests** (`client/src/ui/screens/overview-ui.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { connectionOf, fleetCards, overviewStats } from "../../store/overview";
import { toSession } from "../../store/catalog";
import { NOW, machineMeta, projected, rowOut, usageOf } from "../../store/testing";
import { accountIdentity, accountTone, accountsNote, bytes, cpuFigure, level, memoryFigure, sampleNote, sessionsLabel } from "./overview-ui";
import { emptyAccounts, type Account } from "../../types";

const account = (extra: Partial<Account> = {}): Account => ({
  key: "k", label: "Work", provider: "claude", installed: true, machines: ["workstation"], members: [], usage: usageOf(),
  permission: "Default", identity_lines: [], identity_note: "", ...extra,
});

describe("overview cards", () => {
  // Review Focus 1.
  it("a reconnecting peer keeps its last values in the unknown colour, leaves Machines online and counts as offline", () => {
    const computers = { "": { id: "", status: "live", attempt: 0 }, studio: { id: "studio", status: "reconnecting", attempt: 1 } };
    const cards = fleetCards(computers, { "": machineMeta("workstation"), studio: machineMeta("studio", { cpu_percent: 42, sampled_at: NOW - 30 }) });
    const studio = cards.find((c) => c.computer === "studio")!;
    expect(studio.connection).toEqual({ tile: "connecting", text: "Reconnecting", fresh: false });
    const cpu = cpuFigure(studio.meta);
    expect(cpu.value).toBe("42%");
    expect(level(cpu.percent, studio.connection.fresh)).toBe("unknown");
    expect(cards.filter((c) => c.connection.fresh)).toHaveLength(1);
    expect(sampleNote(studio.meta, studio.connection, NOW)).toMatch(/^Last sample /);
    const rows = [rowOut("", 1), rowOut("studio", 2), rowOut("studio", 3)].map((r) => toSession(r.computer, r.key, r.row));
    expect(overviewStats(rows, computers).offline).toBe(2);
  });

  it("show percentages as the main figures with Qt's details", () => {
    const live = connectionOf("live");
    const meta = machineMeta("workstation", { cpu_percent: 71.6, cpu_count: 16, load_1: 2.5, memory_used_bytes: 7.5 * 2 ** 30, memory_total_bytes: 31.3 * 2 ** 30 });
    expect(cpuFigure(meta)).toMatchObject({ value: "72%", detail: "16 cores, load 2.50" });
    expect(level(71.6, true)).toBe("warning");
    expect(level(95, true)).toBe("critical");
    expect(level(undefined, true)).toBe("unknown");
    expect(memoryFigure(meta)).toMatchObject({ value: "24%", detail: "7.5 GiB of 31.3 GiB" });
    expect(bytes(512 * 2 ** 20)).toBe("512 MiB");
    expect(cpuFigure(undefined).detail).toBe("Update hgs for metrics");
    expect(cpuFigure(machineMeta("w", { cpu_percent: null, cpu_state: "warming" })).detail).toBe("Waiting for second sample");
    expect(cpuFigure(machineMeta("w", { cpu_percent: null, cpu_state: "unavailable" })).detail).toBe("CPU metrics unavailable");
    expect(memoryFigure(machineMeta("w", { memory_total_bytes: null })).detail).toBe("Memory unavailable");
    expect(sampleNote(meta, live, NOW + 59)).toBe("");
    expect(sampleNote(meta, live, NOW + 60)).toMatch(/^Last sample /);
    expect(sampleNote(undefined, connectionOf("unsupported"), NOW)).toBe("No sample yet");
    expect(sessionsLabel(1, 0)).toBe("1 session");
    expect(sessionsLabel(3, 2)).toBe("3 sessions, 2 need attention");
  });

  it("word the account cards as Qt does", () => {
    const identity = { ...usageOf().identity, email: "person@example.com", plan: "max" };
    expect(accountIdentity(account({ usage: usageOf({ identity }) }))).toBe("person@example.com / max");
    expect(accountIdentity(account({ installed: false }))).toBe("Agent not installed");
    expect(accountTone(account({ usage: usageOf({ windows: [projected({ used: 95, resets_at: null })] }) }), NOW)).toBe("critical");
    expect(accountTone(account(), NOW)).toBe("none");
    expect(accountsNote(emptyAccounts())).toBe("Loading accounts…");
    expect(accountsNote({ ...emptyAccounts(), snapshot: true, loading: true })).toBe("Loading account details…");
    expect(accountsNote({ ...emptyAccounts(), snapshot: true })).toBe("Open Manage accounts to view or set up accounts.");
    expect(accountsNote({ ...emptyAccounts(), snapshot: true, overview: [account()] })).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run** `npx vitest run src/ui/screens/overview-ui.test.ts` in `client`. Expected: FAIL (module missing).

- [ ] **Step 3: Implement `overview-ui.ts`:**

```ts
import type { Account, AccountsView, MachineMeta } from "../../types";
import type { Connection } from "../../store/overview";
import { dayTime, severest, windowsAt, type Tone } from "../lib/usage";

// Pure texts and levels of the Overview cards (DashboardPage.cpp:29-138,
// 255-279, 341-360), with the owner's round 7-8 choices: percentages are the
// main figures and the sample time shows only when it is not current.

export type Level = "unknown" | "normal" | "warning" | "critical";

/** updateLoadBar (DashboardPage.cpp:60-71): unknown without a live stream or a sample. */
export function level(percent: number | undefined, fresh: boolean): Level {
  if (!fresh || percent === undefined || !Number.isFinite(percent) || percent < 0) return "unknown";
  return percent >= 90 ? "critical" : percent >= 70 ? "warning" : "normal";
}

export function loadTip(percent: number | undefined, fresh: boolean) {
  return percent === undefined ? "No sample available" : !fresh ? "Last known value; waiting for a fresh sample." : "Orange from 70%; red from 90%.";
}

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
/** DashboardPage.cpp:29-34. */
export const bytes = (value: number) => (value >= GIB ? `${(value / GIB).toFixed(1)} GiB` : `${(value / MIB).toFixed(0)} MiB`);

export interface Figure { value: string; percent?: number; detail: string; tip: string }

/** No metrics at all: an older hgs, or no card. */
const noMetrics = (meta?: MachineMeta) =>
  !meta || (meta.metrics.sampled_at === null && meta.metrics.cpu_state === null && meta.metrics.cpu_percent === null);

export function cpuFigure(meta: MachineMeta | undefined): Figure {
  const m = meta?.metrics;
  if (m && m.cpu_percent !== null) {
    return {
      value: `${Math.round(m.cpu_percent)}%`,
      percent: m.cpu_percent,
      detail: `${m.cpu_count ?? 0} cores, load ${(m.load_1 ?? 0).toFixed(2)}`,
      tip: `Average CPU use over the last ${(m.cpu_interval_seconds ?? 0).toFixed(1)} seconds, across all logical cores.`,
    };
  }
  const detail = noMetrics(meta) ? "Update hgs for metrics" : m?.cpu_state === "unavailable" ? "CPU metrics unavailable" : "Waiting for second sample";
  return { value: "—", detail, tip: "CPU use needs two samples. Refresh again after a moment." };
}

export function memoryFigure(meta: MachineMeta | undefined): Figure {
  const m = meta?.metrics;
  const total = m?.memory_total_bytes ?? 0;
  const used = m?.memory_used_bytes ?? 0;
  const tip = meta?.platform === "macos"
    ? "App, wired and compressed memory. Reclaimable file cache is excluded."
    : "Total memory minus available memory, including reclaimable cache.";
  if (total <= 0) return { value: "—", detail: "Memory unavailable", tip };
  const percent = (100 * used) / total;
  return { value: `${Math.round(percent)}%`, percent, detail: `${bytes(used)} of ${bytes(total)}`, tip };
}

/** The sample time when it is not current; a live stream with a sample under 60 s old is current. */
export function sampleNote(meta: MachineMeta | undefined, c: Connection, now: number): string {
  const at = meta?.metrics.sampled_at ?? null;
  if (c.fresh && at !== null && now - at < 60) return "";
  return at !== null && at > 0 ? `Last sample ${dayTime(at)}` : "No sample yet";
}

export const sampleTip = (c: Connection) =>
  c.fresh ? "Last known value; waiting for a fresh sample." : "Last known values. This machine is unavailable or has not replied recently.";

export const sessionsLabel = (sessions: number, attention: number) =>
  `${sessions === 1 ? "1 session" : `${sessions} sessions`}${attention ? `, ${attention} need attention` : ""}`;

/** The card's identity line (DashboardPage.cpp:263-265). */
export function accountIdentity(a: Account): string {
  const identity = a.usage.identity;
  let text = identity.email || identity.name;
  if (identity.plan) text += `${text ? " / " : ""}${identity.plan}`;
  return text || (a.installed ? "" : "Agent not installed");
}

/** The stripe: the pace tone of the current windows (AccountUsage::severest), grey without one. */
export function accountTone(a: Account, now: number): Tone | "none" {
  return severest(windowsAt(a.usage.windows, now)) ?? "none";
}

/** The Accounts section's text when it shows no card (DashboardPage.cpp:314, 360). */
export function accountsNote(view: AccountsView): string | undefined {
  if (!view.snapshot && !view.accounts.length) return "Loading accounts…";
  if (view.overview.length) return undefined;
  return view.loading ? "Loading account details…" : "Open Manage accounts to view or set up accounts.";
}
```

- [ ] **Step 4: Copy the approved Overview and Projects styles** (one command from the repository root; lines 2-154 are the `@layer` opening, the page shell, Overview, Projects and the shared table):

```bash
{ printf '/* Overview and Projects, copied from design/webui/src/styles/setup.css lines 1-154 (the approved mockup). Skin tokens only. */\n'; sed -n '2,154p' design/webui/src/styles/setup.css; printf '}\n'; } > client/src/ui/styles/screens.css
```

Add `import "./ui/styles/screens.css";` to `client/src/main.tsx` after the `client.css` import.

- [ ] **Step 5: Write `Overview.tsx`** (the mockup's markup and classes; its mock imports replaced by the stores):

```tsx
import { For, Index, Show, createSignal, onCleanup, onMount } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon } from "../lib/icons";
import { MachineTile } from "../components/MachineTile";
import { UsageMeters } from "../components/UsageMeter";
import { AgentBadge, MachineBadge, SessionCard } from "../sessions/SessionCard";
import type { Session } from "../sessions/types";
import type { Account } from "../../types";
import { agentKind, catalog, orderedRows } from "../../store/catalog";
import { machineCards } from "../../store/machines";
import { accountsPage, loadAccounts, setAccountMachine, setRequestedProfile } from "../../store/accounts";
import { createOverview, type MachineCardModel, type OverviewStats } from "../../store/overview";
import { setMachineFilter, setPage, setProjectFilter, setSelectedKey, setStateFilter, type StateFilter } from "../../store/ui";
import { accountTooltip, summaryText, usageLoading, windowsAt } from "../lib/usage";
import { accountIdentity, accountTone, accountsNote, cpuFigure, level, loadTip, memoryFigure, sampleNote, sampleTip, sessionsLabel } from "./overview-ui";

// Overview (DashboardPage.cpp), ported from design/webui/src/screens/Dashboard.tsx:
// summary tiles, a card per machine from the stream's machine cards, account
// cards from the screens hub, then the sessions that are working, need
// attention or have drafts. A sleeping peer is a pause, not a death: its last
// known sessions keep counting and its card says its stream is not live.

const CARD_W = 340;
const CARD_H = 104;
const GAP = 8;

function LoadBar(props: { percent?: number; fresh: boolean }) {
  return (
    <div class="ov-load" data-level={level(props.percent, props.fresh)} title={loadTip(props.percent, props.fresh)}>
      <i style={{ width: `${props.percent === undefined ? 0 : Math.max(0, Math.min(100, props.percent))}%` }} />
    </div>
  );
}

function MachineCard(props: { m: MachineCardModel; stats: OverviewStats; now: number }) {
  const c = () => props.m.connection;
  const cpu = () => cpuFigure(props.m.meta);
  const memory = () => memoryFigure(props.m.meta);
  const counts = () => props.stats.per[props.m.computer] ?? { sessions: 0, attention: 0 };
  const note = () => sampleNote(props.m.meta, c(), props.now);
  const openSessions = () => { setMachineFilter([props.m.filter]); setProjectFilter([]); setStateFilter("all"); setPage("sessions"); };
  return (
    <article class="ov-mcard box" classList={{ stale: !c().fresh }} aria-label={props.m.local ? "This machine" : props.m.name}>
      <header class="ov-mtop">
        <MachineTile os={props.m.platform} state={c().tile} size={26} />
        <MachineBadge alias={props.m.name} computer={props.m.computer} />
        <span class="ov-mos ell"><Show when={props.m.local}><b>This machine</b></Show></span>
        <span class="ov-mstatus" classList={{ online: c().fresh }}>{c().text}</span>
        <button class="icon-btn small" data-tip="Machine settings" aria-label="Machine settings" onClick={() => setPage("machines")}><Icon name="settings" size={14} /></button>
      </header>
      <div class="ov-mmeters">
        <div class="ov-mm" title={cpu().tip}>
          <span class="ov-mlabel">CPU</span>
          <span class="ov-mline">
            <b class="ov-mval tabular" data-level={level(cpu().percent, c().fresh)}>{cpu().value}</b>
            <LoadBar percent={cpu().percent} fresh={c().fresh} />
          </span>
          <span class="ov-mdetail">{cpu().detail}</span>
        </div>
        <div class="ov-mm" title={memory().tip}>
          <span class="ov-mlabel">Memory</span>
          <span class="ov-mline">
            <b class="ov-mval tabular" data-level={level(memory().percent, c().fresh)}>{memory().value}</b>
            <LoadBar percent={memory().percent} fresh={c().fresh} />
          </span>
          <span class="ov-mdetail">{memory().detail}</span>
        </div>
      </div>
      <footer class="ov-mfoot">
        <button class="su-link" title={c().fresh ? `View sessions on ${props.m.name}` : `View last known sessions on ${props.m.name}`} onClick={openSessions}>
          {sessionsLabel(counts().sessions, counts().attention)}
        </button>
        <span class="grow" />
        <Show when={note()}><span class="ov-mdetail" title={sampleTip(c())}>{note()}</span></Show>
      </footer>
    </article>
  );
}

function AccountCard(props: { a: Account; now: number }) {
  // An agent that is not installed reads as unavailable (DashboardPage.cpp:262).
  const usage = () => (props.a.installed ? props.a.usage : { ...props.a.usage, status: "unavailable" });
  const hosts = () => props.a.members.filter((m, i, all) => all.findIndex((x) => x.host === m.host) === i);
  const open = () => {
    setRequestedProfile(props.a.members[0]?.key);
    setAccountMachine("*");
    setPage("accounts");
  };
  return (
    <button class="ov-acard box" data-tone={accountTone(props.a, props.now)} title={accountTooltip(usage(), props.now)} onClick={open}>
      <span class="ov-al1">
        <b class="ell">{props.a.label}</b>
        <span class="grow" />
        <AgentBadge agent={agentKind(props.a.provider)} />
        <For each={hosts()}>{(m) => <MachineBadge alias={m.machine} computer={m.host} />}</For>
      </span>
      <span class="ov-al2">
        <span class="ell muted">{accountIdentity(props.a)}</span>
        <span class="grow" />
        <span class="ov-ausage tabular">
          <UsageMeters windows={windowsAt(usage().windows, props.now)} loading={usageLoading(usage())} compact summary={summaryText(usage())} />
        </span>
      </span>
    </button>
  );
}

function LiveSessions(props: { rows: Session[]; scroller: () => HTMLElement | undefined }) {
  let grid!: HTMLDivElement;
  const [width, setWidth] = createSignal(1000);
  const [top, setTop] = createSignal(0);
  const cols = () => Math.max(1, Math.floor((width() + GAP) / (CARD_W + GAP)));
  const lines = () => Math.ceil(props.rows.length / cols());
  // The page scroller is attached only after this grid mounts; reading it
  // earlier would measure a detached element as 0 px high.
  let attached = false;
  const virtualizer = createVirtualizer({
    get count() { return lines(); },
    getScrollElement: () => (attached ? props.scroller() ?? null : null),
    estimateSize: () => CARD_H + GAP,
    overscan: 4,
    get scrollMargin() { return top(); },
  });
  attached = true;
  onMount(() => {
    const measure = () => { setWidth(grid.clientWidth); setTop(grid.offsetTop); };
    const ro = new ResizeObserver(measure);
    ro.observe(grid);
    measure();
    onCleanup(() => ro.disconnect());
  });
  const open = (s: Session) => { setSelectedKey(s.key); setPage("sessions"); };
  return (
    <div class="ov-live" ref={grid} style={{ height: `${virtualizer.getTotalSize()}px` }}>
      <For each={virtualizer.getVirtualItems()}>
        {(line) => (
          <div class="ov-line" style={{ transform: `translateY(${line.start - top()}px)`, "grid-template-columns": `repeat(${cols()}, minmax(0, 1fr))` }}>
            <For each={props.rows.slice(line.index * cols(), line.index * cols() + cols())}>
              {(s) => (
                <div class="ov-cell rowhit" role="button" tabindex="0" aria-label={`Open ${s.name}`} onClick={() => open(s)}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(s); } }}>
                  <SessionCard session={s} selected={false} expandable={false} expanded={false} onToggleChildren={() => open(s)} />
                </div>
              )}
            </For>
          </div>
        )}
      </For>
    </div>
  );
}

// A centred column like Activity: both edges drag together, double-click
// returns to the default, and the width is kept for this viewer.
const OVERVIEW = { min: 720, max: 2400, edge: 12, fallback: 1200 } as const;
const WIDTH_KEY = "zerus-client-overview-width";
const clampWidth = (width: number) => Math.round(Math.max(OVERVIEW.min, Math.min(OVERVIEW.max, width)));
const savedWidth = (() => { try { return Number(localStorage.getItem(WIDTH_KEY)) || 0; } catch { return 0; } })();
const [overviewWidth, setOverviewWidthRaw] = createSignal(savedWidth ? clampWidth(savedWidth) : OVERVIEW.fallback);
function setOverviewWidth(width: number) {
  setOverviewWidthRaw(clampWidth(width));
  try { localStorage.setItem(WIDTH_KEY, String(overviewWidth())); } catch { /* storage may be unavailable */ }
}

function OverviewEdge(props: { right?: boolean }) {
  const [dragging, setDragging] = createSignal(false);
  let origin = 0, start = 0, widest = 0;
  return (
    <div class="col-edge ov-edge" classList={{ right: props.right, dragging: dragging() }} role="separator" aria-orientation="vertical" tabindex="0"
      aria-label="Overview width" aria-valuemin={OVERVIEW.min} aria-valuemax={OVERVIEW.max} aria-valuenow={overviewWidth()}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        const el = e.currentTarget;
        el.setPointerCapture(e.pointerId);
        origin = e.clientX;
        start = el.parentElement!.getBoundingClientRect().width;
        widest = (el.closest(".ov-page") as HTMLElement).clientWidth - 2 * OVERVIEW.edge;
        setDragging(true);
      }}
      onPointerMove={(e) => {
        if (!dragging()) return;
        const distance = (e.clientX - origin) * (props.right ? 1 : -1);
        setOverviewWidth(Math.min(widest, start + 2 * distance));
      }}
      onPointerUp={() => setDragging(false)} onPointerCancel={() => setDragging(false)}
      onDblClick={() => setOverviewWidth(OVERVIEW.fallback)}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") { e.preventDefault(); setOverviewWidth(overviewWidth() + ((e.key === "ArrowRight") === !!props.right ? 40 : -40)); }
        else if (e.key === "Home") { e.preventDefault(); setOverviewWidth(OVERVIEW.fallback); }
      }} />
  );
}

export function OverviewScreen() {
  let scroller: HTMLDivElement | undefined;
  const ov = createOverview(orderedRows, () => catalog.computers, () => machineCards);
  // Ages, resets and pace re-render every 15 s while shown (DashboardPage.cpp:322); nothing else runs on a timer.
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), 15_000);
  onCleanup(() => clearInterval(timer));
  const view = accountsPage;
  const online = () => ov.cards().filter((m) => m.connection.fresh).length;
  const go = (filter: StateFilter) => { setMachineFilter([]); setProjectFilter([]); setStateFilter(filter); setPage("sessions"); };
  return (
    <div class="su-page ov-page" ref={scroller}>
      <div class="ov-content" style={{ "--ovw": `${overviewWidth()}px` }}>
        <OverviewEdge /><OverviewEdge right />
        <header class="su-head">
          <h1>Overview</h1>
          <span class="grow" />
          <button class="icon-btn" data-tip="Refresh overview" aria-label="Refresh overview" disabled={view().busy} onClick={() => void loadAccounts(true)}>
            <Icon name="refresh-cw" size={16} class={view().busy ? "su-spin" : undefined} />
          </button>
        </header>

        <div class="ov-tiles">
          <button class="ov-tile" classList={{ hot: ov.stats().attention > 0 }} onClick={() => go("attention")}>
            <b class="tabular">{ov.stats().attention}</b><span>Needs attention</span>
          </button>
          <button class="ov-tile" onClick={() => go("active")}><b class="tabular">{ov.stats().active}</b><span>Active</span></button>
          <button class="ov-tile" onClick={() => setPage("machines")}><b class="tabular">{`${online()} / ${ov.cards().length}`}</b><span>Machines online</span></button>
          <button class="ov-tile" onClick={() => go("all")}><b class="tabular">{ov.stats().all}</b><span>Sessions</span></button>
        </div>

        <h2 class="section-title ov-h">Machines</h2>
        <div class="ov-mgrid">
          <Index each={ov.cards()}>{(m) => <MachineCard m={m()} stats={ov.stats()} now={now()} />}</Index>
        </div>

        <div class="ov-h row">
          <h2 class="section-title">Accounts</h2>
          <span class="grow" />
          <button class="su-link" onClick={() => { setAccountMachine("*"); setPage("accounts"); }}>Manage accounts <Icon name="arrow-right" size={12} /></button>
        </div>
        <Show when={accountsNote(view())} fallback={<div class="ov-agrid"><Index each={view().overview}>{(a) => <AccountCard a={a()} now={now()} />}</Index></div>}>
          {(note) => <p class="ov-empty">{note()}</p>}
        </Show>

        <div class="ov-h row">
          <h2 class="section-title">Attention &amp; in progress</h2>
          <span class="grow" />
          <button class="su-link" onClick={() => go("all")}>All sessions <Icon name="arrow-right" size={12} /></button>
        </div>
        <Show when={ov.stats().offline > 0}>
          <div class="notice warn ov-note"><Icon name="wifi-off" size={14} />
            <span>{ov.stats().offline} sessions are on offline or stale machines. Their last known status is shown on the machine cards.</span>
          </div>
        </Show>
        <Show when={ov.live().length} fallback={<p class="ov-empty">No sessions are working, need attention or have drafts.</p>}>
          <LiveSessions rows={ov.live()} scroller={() => scroller} />
        </Show>
      </div>
    </div>
  );
}
```

In `client/src/App.tsx`, import `OverviewScreen` from `./ui/screens/Overview` and add, before the `page() === "settings"` match:

```tsx
          <Match when={page() === "dashboard"}><OverviewScreen /></Match>
```

- [ ] **Step 6: Give the replay fixtures machine cards.** In `client/scripts/make-fixture.py`, add after `deltas_for`:

```python
METRIC_KEYS = ("cpu_percent", "cpu_count", "cpu_state", "cpu_interval_seconds", "load_1", "load_5", "load_15",
               "memory_used_bytes", "memory_total_bytes", "memory_available_bytes", "uptime_seconds", "sampled_at")


def metrics_for(host):
    """A synthetic sample with the keys of src/metrics.rs; the peer "mac" is a Mac."""
    mac = host == "mac"
    total = (32 if mac else 64) * 1024 ** 3
    return {"sampled_at": BASE_AT, "platform": "macos" if mac else "linux", "cpu_percent": 31.0 if mac else 12.0,
            "cpu_interval_seconds": 2.0, "cpu_state": "available", "cpu_count": 10 if mac else 16,
            "load_1": 2.4 if mac else 1.1, "load_5": 2.0 if mac else 0.9, "load_15": 1.8 if mac else 0.8,
            "memory_total_bytes": total, "memory_used_bytes": int(total * (0.74 if mac else 0.38)),
            "memory_available_bytes": int(total * (0.26 if mac else 0.62)), "uptime_seconds": 86400.0}


def machine_card(host):
    """The same sample as the core's MachineMeta projects it, for the browser replay."""
    metrics = metrics_for(host)
    return {"host": host, "hgs_version": "1.56.0", "platform": metrics["platform"], "ok": True,
            "metrics": {key: metrics[key] for key in METRIC_KEYS},
            "presence": {"idle_seconds": None, "locked": None, "locked_seconds": None},
            "local_sessions": None, "local_host": None}
```

In `stream`, the `catalog.meta` frame becomes:

```python
    frame("snapshot", topic="catalog.meta", value={"host": host, "ok": True, "hgs_version": "1.56.0", "projects": {}, "peers": [],
                                                   "metrics": metrics_for(host)}, page=0, last=True, cursor=1)
```

In `main`, the batches start with `"machines": []` beside `"computers": []`, and after the `batches["computers"].append(...)` line add `batches["machines"].append({"computer": computer, "machine": machine_card(host)})`. Then run `npm run fixture` in `client`. Expected: the three fixture files are written. `git diff --stat client/fixtures` shows only the two `catalog.meta` lines and `replay-batches.json` changed, since the meta frame draws no random numbers.

- [ ] **Step 7: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py` and `cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml replay::` in `client`. Expected: PASS. Then `npm run dev` with `?page=dashboard` in a browser shows the two machine cards from `replay-batches.json`, the tiles and the live sessions (look only, no screenshot committed).

- [ ] **Step 8: Commit**

```bash
git add client/src client/scripts/make-fixture.py client/fixtures
git commit -m "feat(client): the Overview screen on live machine cards, accounts and sessions"
```

---

### Task 6: The Machines screen

**Files:**
- Create: `client/src/ui/components/MachineLabel.tsx`, `client/src/ui/components/Status.tsx`, `client/src/ui/screens/reasons.ts`
- Create: `client/src/ui/screens/machines-ui.ts`, `client/src/ui/screens/machines-ui.test.ts`, `client/src/ui/screens/Machines.tsx`
- Create: `client/src/ui/styles/machines.css` (a verbatim copy, Step 4)
- Modify: `client/src/App.tsx` (route `machines`), `client/src/main.tsx` (import `machines.css`)

**Interfaces:**
- Consumes: `machinesPage`, `loadMachines`, `resolveMachine`, `testConnection`, `machineCards` (Task 4); `connectionOf`, `osOf`; `setAccountMachine`; `catalog`; `paletteIndex` (`ui/sessions/SessionCard.tsx`).
- Produces:

```ts
// components/MachineLabel.tsx
export function MachineLabel(props: { alias: string; class?: string; dimmed?: boolean }): JSX.Element;
// components/Status.tsx (design/webui settings/ui.tsx, unchanged)
export function Status(props: { kind?: "ok" | "warn" | "error" | "busy" | "muted"; children: JSX.Element; class?: string }): JSX.Element;
export function Spinner(props: { size?: number }): JSX.Element;
export function Lines(props: { text: string; class?: string }): JSX.Element;
export function Actions(props: { children: JSX.Element; class?: string }): JSX.Element;
// screens/reasons.ts
export const MACHINE_EDITING, ACCOUNT_MANAGEMENT, PROJECT_EDITING, NEW_SESSION, WORKTREE_FLOWS, COPY_ACTIONS: string;
// screens/machines-ui.ts
export const LOCAL = "@local";
export interface PeerState { tile: MachineState; line: string; live: boolean }
export function peerState(row: MachineRow, computer: ComputerState | undefined, sessions: number): PeerState;
export interface LocalState { name: string; console: boolean; line: string; origin: string; canEnable: boolean; windows: boolean; sessionHost?: string; formHidden: boolean }
export function localState(meta: MachineMeta | undefined, listedName: string): LocalState;
export function sessionHostLine(host: LocalHost): string;
export function summary(local: { console: boolean; live: boolean }, peers: { live: boolean }[]): string;
export function placeholders(effective: Effective | null | undefined, alias: string): { hostname: string; user: string; port: string; key: string };
export function testEnabled(selected: string, view: MachinesView): boolean;
// screens/Machines.tsx
export function MachinesScreen(): JSX.Element;
```

Rules (`MachinesPage.cpp`):
- **The list** shows this computer, then the saved peers of `machine ls`.
- **State lines:**
  - A peer's line is "Disabled" (`enabled: false`, grey tile), "Connected, 1 session" or "Connected, N sessions" (live), or the stream's state text: "Connecting", "Reconnecting" or "Update hgs". Otherwise it is "Not connected".
  - This computer reads "This machine" or "This computer: control console".
  - The card's meta line shows the OS and `hgs <version>` from the machine card.
- **Summary:** "N connected, 1 machine" or "N connected, M machines". A control console is not a machine of the fleet (`renderRows`, 301-322).
- **Editor:**
  - It shows the title and the state chip, the origin paragraph and, on Windows, the session host line.
  - Appearance: "Label color…", Soft or Bright, "Automatic" (disabled), and a preview with the default palette slot.
  - Connection: the effective connection (pending, failed or its lines) and the fields as read-only values with "Inherited: …" placeholders once the resolve answered.
  - Save is disabled, and so is Open SSH terminal. Test connection is enabled for a selected saved peer while nothing runs.
  - Setup: "Enable local sessions" (Windows console, disabled), "Install / update HGS…" (disabled), and "View sessions" and "Agent accounts…" (navigation).
  - The check result shows under Setup for the selected peer.
  - The footer has "Reset SSH overrides" and "Remove from Zerus" (disabled).
  - The connection form, Test, SSH, Install, Reset and Remove are hidden for a console or a Windows local computer.
- **Selection:** selecting a peer resolves its effective connection. A selection that left the list falls back to this computer.
- **Header:** Refresh reads the list again (disabled while busy); "Add machine" is disabled.
- **Reasons:** every disabled write carries `MACHINE_EDITING` as its title.

- [ ] **Step 1: Write the failing tests** (`client/src/ui/screens/machines-ui.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { LOCAL, localState, peerState, placeholders, sessionHostLine, summary, testEnabled } from "./machines-ui";
import { machineMeta } from "../../store/testing";
import { emptyMachines, type MachineRow } from "../../types";

const row = (extra: Partial<MachineRow> = {}): MachineRow => ({
  alias: "studio", enabled: true, source: "local", inherited: false, hostname: "", user: "", port: null, identity_file: "", origin: "", ...extra,
});
const at = (status: string, attempt = 0) => ({ id: "studio", status, attempt });

describe("machines page", () => {
  it("words a peer's state line as Qt, with the stream's states", () => {
    expect(peerState(row({ enabled: false }), at("live"), 2)).toEqual({ tile: "disabled", line: "Disabled", live: false });
    expect(peerState(row(), at("live"), 1)).toEqual({ tile: "online", line: "Connected, 1 session", live: true });
    expect(peerState(row(), at("live"), 3).line).toBe("Connected, 3 sessions");
    expect(peerState(row(), at("reconnecting", 2), 3)).toEqual({ tile: "connecting", line: "Reconnecting", live: false });
    expect(peerState(row(), at("probing"), 0)).toEqual({ tile: "connecting", line: "Connecting", live: false });
    expect(peerState(row(), at("unsupported"), 0)).toEqual({ tile: "offline", line: "Update hgs", live: false });
    expect(peerState(row(), undefined, 0)).toEqual({ tile: "offline", line: "Not connected", live: false });
  });

  it("counts like Qt: a control console is no machine of the fleet", () => {
    expect(summary({ console: false, live: true }, [{ live: true }, { live: false }])).toBe("2 connected, 3 machines");
    expect(summary({ console: true, live: true }, [{ live: true }])).toBe("1 connected, 1 machine");
    expect(summary({ console: false, live: false }, [])).toBe("0 connected, 1 machine");
  });

  it("describes this computer, a control console and a Windows session host", () => {
    expect(localState(machineMeta("workstation"), "")).toMatchObject({
      name: "workstation", console: false, line: "This machine", formHidden: false, canEnable: false,
      origin: "This device runs Zerus. Add a remote machine to manage its agents over SSH.",
    });
    expect(localState(undefined, "workstation").name).toBe("workstation");
    const host = { state: "running", pid: 4120, conpty: "2", integrity: "Medium", windows_session: 1, error: null };
    const console = localState(machineMeta("win-lab", {}, { platform: "windows", local_sessions: false, local_host: host }), "");
    expect(console).toMatchObject({
      console: true, canEnable: true, formHidden: true, line: "This computer: control console",
      origin: "This computer is a control console: agents run on the remote machines you add here, over SSH. Enable local sessions to run them on this computer too.",
      sessionHost: "Session host: running, PID 4120, 2 ConPTY, Medium integrity, Windows session 1",
    });
    expect(sessionHostLine({ state: "not-installed", pid: null, conpty: null, integrity: null, windows_session: null, error: "access denied" }))
      .toBe("Session host: not installed: access denied");
  });

  it("shows inherited values once the effective connection is known, and tests only an idle saved peer", () => {
    const e = { alias: "studio", pending: false, error: false, text: "", hostname: "studio.example", user: "person", port: "2222", key: "/home/example/.ssh/id_ed25519" };
    expect(placeholders(e, "studio")).toEqual({
      hostname: "Inherited: studio.example", user: "Inherited: person", port: "Inherited: 2222", key: "Inherited: /home/example/.ssh/id_ed25519",
    });
    expect(placeholders({ ...e, pending: true }, "studio").hostname).toBe("Address or existing SSH alias");
    expect(placeholders(e, "build-01").user).toBe("Use SSH configuration");
    expect(placeholders(null, "studio")).toEqual({
      hostname: "Address or existing SSH alias", user: "Use SSH configuration", port: "SSH configuration (usually 22)", key: "Use SSH agent / configuration",
    });
    const view = { ...emptyMachines(), loaded: true, machines: [row()] };
    expect(testEnabled("studio", view)).toBe(true);
    expect(testEnabled(LOCAL, view)).toBe(false);
    expect(testEnabled("studio", { ...view, busy: true })).toBe(false);
    expect(testEnabled("gone", view)).toBe(false);
  });
});
```

- [ ] **Step 2: Run** `npx vitest run src/ui/screens/machines-ui.test.ts` in `client`. Expected: FAIL (module missing).

- [ ] **Step 3: Implement the helpers.**

`client/src/ui/screens/reasons.ts`:

```ts
// Why an entry that writes, launches or copies is disabled in phase 3a; each
// names the phase that brings it (Beads zerus-5wmh.25, .10, .19).
export const MACHINE_EDITING = "Arrives with machine editing.";
export const ACCOUNT_MANAGEMENT = "Arrives with account management.";
export const PROJECT_EDITING = "Arrives with project editing.";
export const NEW_SESSION = "Arrives with the New session dialog.";
export const WORKTREE_FLOWS = "Arrives with the worktree flows.";
export const COPY_ACTIONS = "Arrives with the copy actions.";
```

`client/src/ui/screens/machines-ui.ts`:

```ts
import type { ComputerState } from "../../store/catalog";
import { connectionOf } from "../../store/overview";
import type { Effective, LocalHost, MachineMeta, MachineRow, MachinesView } from "../../types";
import type { MachineState } from "../components/MachineTile";

// Pure texts and enablement of the Machines screen (MachinesPage.cpp).

/** The selection key of this computer. */
export const LOCAL = "@local";

export interface PeerState { tile: MachineState; line: string; live: boolean }

/** A saved peer's state line (renderRows, MachinesPage.cpp:308-311) with the stream's states of the Decisions note. */
export function peerState(row: MachineRow, computer: ComputerState | undefined, sessions: number): PeerState {
  if (!row.enabled) return { tile: "disabled", line: "Disabled", live: false };
  const c = connectionOf(computer?.status);
  if (c.fresh) return { tile: "online", line: sessions === 1 ? "Connected, 1 session" : `Connected, ${sessions} sessions`, live: true };
  return { tile: c.tile, line: c.text === "Offline" ? "Not connected" : c.text, live: false };
}

export interface LocalState {
  name: string;
  console: boolean;
  line: string;
  origin: string;
  /** A Windows control console whose CLI reports a session host (LocalSessionHost::canEnable). */
  canEnable: boolean;
  windows: boolean;
  sessionHost?: string;
  /** The connection form and the peer actions are hidden (updateControls, 400-405). */
  formHidden: boolean;
}

/** MachinesPage::updateControls, 408-419. */
export function sessionHostLine(host: LocalHost): string {
  const state = host.state ?? "";
  const facts = [state === "not-installed" ? "not installed" : state];
  if (host.pid !== null) facts.push(`PID ${host.pid}`);
  if (host.conpty) facts.push(`${host.conpty} ConPTY`);
  if (host.integrity) facts.push(`${host.integrity} integrity`);
  if (host.windows_session !== null) facts.push(`Windows session ${host.windows_session}`);
  return `Session host: ${facts.join(", ")}${host.error ? `: ${host.error}` : ""}`;
}

/** This computer (loadSelection, MachinesPage.cpp:328-336); `listedName` is `machine ls .local`. */
export function localState(meta: MachineMeta | undefined, listedName: string): LocalState {
  const console = meta?.local_sessions === false;
  const windows = meta?.platform === "windows";
  const canEnable = console && windows && !!meta?.local_host;
  return {
    name: meta?.host || listedName || "This machine",
    console,
    line: console ? "This computer: control console" : "This machine",
    origin: console
      ? `This computer is a control console: agents run on the remote machines you add here, over SSH.${canEnable ? " Enable local sessions to run them on this computer too." : ""}`
      : "This device runs Zerus. Add a remote machine to manage its agents over SSH.",
    canEnable,
    windows,
    sessionHost: windows && meta?.local_host ? sessionHostLine(meta.local_host) : undefined,
    formHidden: console || windows,
  };
}

/** "N connected, M machines": this computer counts unless it is a control console (renderRows, 302-321). */
export function summary(local: { console: boolean; live: boolean }, peers: { live: boolean }[]): string {
  const online = (local.live && !local.console ? 1 : 0) + peers.filter((p) => p.live).length;
  const saved = peers.length + (local.console ? 0 : 1);
  return saved === 1 ? `${online} connected, 1 machine` : `${online} connected, ${saved} machines`;
}

/** The field placeholders: inherited values once the selected peer's resolve answered (showEffectiveConnection). */
export function placeholders(effective: Effective | null | undefined, alias: string) {
  const known = !!effective && effective.alias === alias && !effective.pending && !effective.error;
  return {
    hostname: known ? `Inherited: ${effective!.hostname}` : "Address or existing SSH alias",
    user: known ? `Inherited: ${effective!.user}` : "Use SSH configuration",
    port: known ? `Inherited: ${effective!.port}` : "SSH configuration (usually 22)",
    key: known && effective!.key ? `Inherited: ${effective!.key}` : "Use SSH agent / configuration",
  };
}

/** Test connection: a saved peer is selected and nothing of the page runs (no draft or edits exist in 3a). */
export const testEnabled = (selected: string, view: MachinesView) =>
  selected !== LOCAL && view.loaded && !view.busy && view.machines.some((m) => m.alias === selected);
```

`client/src/ui/components/MachineLabel.tsx`:

```tsx
import { paletteIndex } from "../sessions/SessionCard";

// The coloured machine label (IdentityBadges::Machine). Label colours and
// styles are this device's preferences and arrive with machine editing
// (3c); until then every machine shows its default palette slot.
export function MachineLabel(props: { alias: string; class?: string; dimmed?: boolean }) {
  return (
    <span class={`entity mach-label ${props.class ?? ""}`} classList={{ dimmed: props.dimmed }} style={{ "--entity": `var(--z-entity-palette-${paletteIndex(props.alias)})` }}>
      {props.alias}
    </span>
  );
}
```

`client/src/ui/components/Status.tsx` (the mockup's `screens/settings/ui.tsx` helpers, unchanged):

```tsx
import { For, Show, type JSX } from "solid-js";
import { Icon } from "../lib/icons";

// Status rows and action groups shared by Machines and Accounts (design/webui
// screens/settings/ui.tsx). Settings (3b) uses them too.

export function Status(props: { kind?: "ok" | "warn" | "error" | "busy" | "muted"; children: JSX.Element; class?: string }) {
  return (
    <div class={`set-status k-${props.kind ?? "muted"} ${props.class ?? ""}`} role="status">
      <Show when={props.kind === "busy"}><span class="set-spinner" aria-hidden="true" /></Show>
      <Show when={props.kind === "ok"}><Icon name="check" size={13} /></Show>
      <Show when={props.kind === "warn"}><Icon name="triangle-alert" size={13} /></Show>
      <Show when={props.kind === "error"}><Icon name="circle-alert" size={13} /></Show>
      <span class="set-status-text">{props.children}</span>
    </div>
  );
}

export function Spinner(props: { size?: number }) {
  return <span class="set-spinner" style={{ width: `${props.size ?? 14}px`, height: `${props.size ?? 14}px` }} aria-hidden="true" />;
}

/** A multi-line plain-text value, as Qt labels with \n show it. */
export function Lines(props: { text: string; class?: string }) {
  return <div class={`set-lines ${props.class ?? ""}`}><For each={props.text.split("\n")}>{(line) => <div>{line}</div>}</For></div>;
}

export function Actions(props: { children: JSX.Element; class?: string }) {
  return <div class={`set-actions ${props.class ?? ""}`}>{props.children}</div>;
}
```

- [ ] **Step 4: Copy the approved Machines and Accounts styles** (repository root):

```bash
{ printf '/* Machines, Accounts and their status rows, copied from design/webui/src/styles/settings.css (the approved mockup): lines 3-5, 57, 60-61, 76-82, 223-297 and 344-374. Skin tokens only. */\n'; sed -n '3,5p;57p;60,61p;76,82p;223,297p;344,374p' design/webui/src/styles/settings.css; printf '}\n'; } > client/src/ui/styles/machines.css
```

Add `import "./ui/styles/machines.css";` to `client/src/main.tsx` after `screens.css`.

- [ ] **Step 5: Write `Machines.tsx`:**

```tsx
import { Index, Show, createEffect, createMemo, createSignal, on, type JSX } from "solid-js";
import { Icon } from "../lib/icons";
import { OS_NAME } from "../lib/os-icons";
import { MachineTile } from "../components/MachineTile";
import { MachineLabel } from "../components/MachineLabel";
import { Actions, Lines, Spinner, Status } from "../components/Status";
import { paletteIndex } from "../sessions/SessionCard";
import type { MachineMeta, MachineRow } from "../../types";
import { catalog } from "../../store/catalog";
import { loadMachines, machineCards, machinesPage, resolveMachine, testConnection } from "../../store/machines";
import { setAccountMachine } from "../../store/accounts";
import { connectionOf, osOf } from "../../store/overview";
import { setMachineFilter, setPage } from "../../store/ui";
import { MACHINE_EDITING } from "./reasons";
import { LOCAL, localState, peerState, placeholders, summary, testEnabled, type LocalState, type PeerState } from "./machines-ui";

// Machines (MachinesPage.cpp), ported from design/webui/src/screens/Machines.tsx:
// this computer and the saved peers of `hgs machine ls`, each with its stream
// state, and an editor with the Zerus overrides and the effective SSH
// connection. Editing, installing and removing arrive with machine editing
// (3c) and keep their place disabled; Test connection is a read and works.

const [selected, setSelected] = createSignal(new URLSearchParams(location.search).get("machine") ?? LOCAL);

function MetaLine(props: { meta: MachineMeta }) {
  return (
    <span class="mach-meta tabular parts">
      <Show when={osOf(props.meta.platform)}>{(os) => <span>{OS_NAME[os()]}</span>}</Show>
      <Show when={props.meta.hgs_version}><span>hgs {props.meta.hgs_version}</span></Show>
    </span>
  );
}

function LocalCard(props: { local: LocalState }) {
  const meta = () => machineCards[""];
  const on = () => selected() === LOCAL;
  return (
    <button class="mach-card" classList={{ on: on() }} aria-current={on()} onClick={() => setSelected(LOCAL)}>
      <MachineTile os={osOf(meta()?.platform)} state={connectionOf(catalog.computers[""]?.status).tile} />
      <span class="mach-col">
        <span class="row"><MachineLabel alias={props.local.name} /></span>
        <span class="mach-state">{props.local.line}</span>
        <Show when={!props.local.console && meta()}>{(m) => <MetaLine meta={m()} />}</Show>
      </span>
    </button>
  );
}

function PeerCard(props: { row: MachineRow; state: PeerState }) {
  const meta = () => machineCards[props.row.alias];
  const on = () => selected() === props.row.alias;
  return (
    <button class="mach-card" classList={{ on: on(), offline: !props.state.live }} aria-current={on()} onClick={() => setSelected(props.row.alias)}>
      <MachineTile os={osOf(meta()?.platform)} state={props.state.tile} />
      <span class="mach-col">
        <span class="row"><MachineLabel alias={props.row.alias} /></span>
        <span class="mach-state">{props.state.line}</span>
        <Show when={meta()}>{(m) => <MetaLine meta={m()} />}</Show>
      </span>
    </button>
  );
}

function Field(props: { label: string; id: string; children: JSX.Element }) {
  return (
    <div class="form-row mach-form-row">
      <label for={props.id}>{props.label}</label>
      <div class="col" style={{ gap: "4px" }}>{props.children}</div>
    </div>
  );
}

function Editor(props: { local: LocalState; peers: { row: MachineRow; state: PeerState }[] }) {
  const view = machinesPage;
  const isLocal = () => selected() === LOCAL;
  const peer = () => props.peers.find((p) => p.row.alias === selected());
  const row = () => peer()?.row;
  const name = () => (isLocal() ? props.local.name : selected());
  const effective = () => (view().effective?.alias === selected() ? view().effective : null);
  const check = () => (view().check?.alias === selected() ? view().check : null);
  const hint = () => placeholders(effective(), selected());
  const formHidden = () => isLocal() && props.local.formHidden;
  return (
    <div class="mach-editor">
      <div class="mach-title">
        <h2>{name()}</h2>
        <span class="grow" />
        <Show when={peer()}>{(p) => <span class="chip" classList={{ "mach-chip-ok": p().state.live }}>{p().state.line}</span>}</Show>
      </div>
      <p class="muted mach-origin">{isLocal() ? props.local.origin : row()?.origin}</p>
      <Show when={isLocal() && props.local.sessionHost}><p class="mach-host selectable">{props.local.sessionHost}</p></Show>

      <section class="mach-section">
        <h3>Appearance</h3>
        <div class="row mach-appearance">
          <button class="btn" disabled title={MACHINE_EDITING}>
            <span class="mach-swatch" style={{ "--entity": `var(--z-entity-palette-${paletteIndex(name())})` }} />Label color…
          </button>
          <select class="field" aria-label="Label style" disabled title={MACHINE_EDITING}>
            <option value="soft">Soft</option><option value="bright">Bright</option>
          </select>
          <button class="btn ghost" disabled title={MACHINE_EDITING}>Automatic</button>
        </div>
        <div class="row mach-preview">
          <span class="dim">Preview</span>
          <MachineLabel alias={name()} />
          <span class="dim">Saved on this device</span>
        </div>
      </section>

      <Show when={!formHidden()}>
        <section class="mach-section">
          <h3>Connection</h3>
          <Show when={!isLocal() && effective()}>{(e) => (
            <Show when={!e().pending} fallback={<Status kind="busy">{e().text}</Status>}>
              <Show when={!e().error} fallback={<Status kind="error"><Lines text={e().text} /></Status>}>
                <div class="mach-effective selectable"><Lines text={e().text} /></div>
              </Show>
            </Show>
          )}</Show>
          <Field label="Name / SSH alias" id="mach-alias">
            <input id="mach-alias" class="field" value={name()} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Hostname / IP" id="mach-host">
            <input id="mach-host" class="field" placeholder={hint().hostname} value={row()?.hostname ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="SSH user" id="mach-user">
            <input id="mach-user" class="field" placeholder={hint().user} value={row()?.user ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Port" id="mach-port">
            <input id="mach-port" class="field tabular mach-port" placeholder={hint().port} value={row()?.port ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Private key path" id="mach-key">
            <div class="row">
              <input id="mach-key" class="field mono grow" placeholder={hint().key} value={row()?.identity_file ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
              <button class="btn" disabled title={MACHINE_EDITING}>Browse…</button>
            </div>
          </Field>
          <div class="form-row mach-form-row">
            <span />
            <label class="check" title={MACHINE_EDITING}>
              <input type="checkbox" checked={isLocal() || (row()?.enabled ?? true)} disabled />Show sessions and include in background polling
            </label>
          </div>
          <Actions>
            <button class="btn primary" disabled title={MACHINE_EDITING}>Save connection</button>
            <button class="btn" disabled={!testEnabled(selected(), view())} onClick={() => void testConnection(selected())}><Icon name="plug" size={13} />Test connection</button>
            <button class="btn" disabled title={MACHINE_EDITING}><Icon name="square-terminal" size={13} />Open SSH terminal</button>
          </Actions>
        </section>
      </Show>

      <section class="mach-section">
        <h3>Setup</h3>
        <Actions>
          <Show when={isLocal() && props.local.canEnable}>
            <button class="btn primary" disabled title={MACHINE_EDITING}>Enable local sessions</button>
          </Show>
          <Show when={!formHidden()}>
            <button class="btn" disabled title={MACHINE_EDITING}><Icon name="download" size={13} />Install / update HGS…</button>
          </Show>
          <Show when={!(isLocal() && props.local.console)}>
            <button class="btn" disabled={view().busy} onClick={() => { setMachineFilter([name()]); setPage("sessions"); }}>
              <Icon name="rows-3" size={13} />View sessions
            </button>
            <button class="btn" disabled={view().busy} onClick={() => { setAccountMachine(isLocal() ? "" : selected()); setPage("accounts"); }}>
              <Icon name="user-round-cog" size={13} />Agent accounts…
            </button>
          </Show>
        </Actions>
        <Show when={check()}>{(c) => <Status kind={c().pending ? "busy" : c().error ? "error" : "muted"}><Lines text={c().text} /></Status>}</Show>
      </section>

      <Show when={!formHidden()}>
        <div class="mach-foot">
          <button class="btn" disabled title={MACHINE_EDITING}>Reset SSH overrides</button>
          <button class="btn danger" disabled title={MACHINE_EDITING}><Icon name="trash" size={13} />Remove from Zerus</button>
        </div>
      </Show>
    </div>
  );
}

export function MachinesScreen() {
  const view = machinesPage;
  // Rows per computer from the row ids alone (computer + "\n" + key): one pass per batch.
  const sessions = createMemo(() => {
    const per: Record<string, number> = {};
    for (const id of catalog.order) {
      const computer = id.slice(0, id.indexOf("\n"));
      per[computer] = (per[computer] ?? 0) + 1;
    }
    return per;
  });
  const local = () => localState(machineCards[""], view().local);
  const peers = createMemo(() => view().machines.map((row) => ({ row, state: peerState(row, catalog.computers[row.alias], sessions()[row.alias] ?? 0) })));
  // A selection that left the list falls back to this computer (loadSelection).
  createEffect(() => {
    if (view().loaded && selected() !== LOCAL && !view().machines.some((m) => m.alias === selected())) setSelected(LOCAL);
  });
  // Selecting a peer reads its effective connection (resolveConnection).
  createEffect(on(selected, (key) => { if (key !== LOCAL) void resolveMachine(key); }));
  return (
    <div class="mach-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Machines</h1>
          <button class="icon-btn" title="Refresh machines" aria-label="Refresh machines" disabled={view().busy} onClick={() => void loadMachines(true)}>
            <Show when={!view().refreshing} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled title={MACHINE_EDITING}><Icon name="plus" size={14} />Add machine</button>
        </header>
        <p class="muted mach-summary">{summary({ console: local().console, live: catalog.computers[""]?.status === "live" }, peers().map((p) => p.state))}</p>
        <Show when={view().error}>{(error) => <Status kind="error"><Lines text={error()} /></Status>}</Show>
        <div class="mach-body">
          <div class="mach-list" role="list" aria-label="Machines">
            <LocalCard local={local()} />
            <Index each={peers()}>{(p) => <PeerCard row={p().row} state={p().state} />}</Index>
          </div>
          <Editor local={local()} peers={peers()} />
        </div>
      </div>
    </div>
  );
}
```

In `client/src/App.tsx`, import `MachinesScreen` from `./ui/screens/Machines` and add after the dashboard match:

```tsx
          <Match when={page() === "machines"}><MachinesScreen /></Match>
```

- [ ] **Step 6: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py` in `client`. Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add client/src
git commit -m "feat(client): the Machines screen on machine ls, the stream states and Test connection"
```

---

### Task 7: The Accounts screen and its usage panel

**Files:**
- Create: `client/src/ui/screens/accounts-ui.ts`, `client/src/ui/screens/accounts-ui.test.ts`, `client/src/ui/screens/Accounts.tsx`
- Modify: `client/src/App.tsx` (route `accounts`)

**Interfaces:**
- Consumes: `accountsPage`, `loadAccounts`, `accountMachine`, `setAccountMachine`, `requestedProfile`, `setRequestedProfile` (Task 4); `fromProjected`, `current`, `pace`, `period`, `forecast`, `thresholdTone`, `dayTime`, `clock`, `providerName`, `stateText` (Task 4); `MachineLabel`, `Status`, `Spinner`, `Actions`, `ACCOUNT_MANAGEMENT` (Task 6); `machineCards`, `connectionOf`, `osOf`.
- Produces:

```ts
export function AccountsScreen(): JSX.Element;
// accounts-ui.ts
export const tone: (s: MemberState) => "ok" | "busy" | "muted" | "warn";
export function loginLabel(p: Profile): string | undefined;
export function memberOf(a: Account, key: string | undefined, machine: string): Profile | undefined;
export function visibleMembers(a: Account, selectedKey: string | undefined, max?: number): Profile[];
export function visibleAccounts(view: AccountsView, machine: string): Account[];
export function summaryLine(view: AccountsView, visible: number): string;
export interface PanelWindow { period: string; usedText: string; used: number; tone: Tone; elapsed?: number; resets?: string; forecast: string }
export function panelWindow(p: ProjectedWindow, now: number): PanelWindow;
export function creditsText(credits: { balance: string; unlimited: boolean } | null): string | undefined;
```

Rules (`AccountsPage.cpp`, the mockup, the Decisions note):
- **Header and filter:**
  - Refresh forces the account reads and is disabled and spinning while they run. Its tooltip is "Updating accounts… N remaining" or "Refresh account usage (automatically every 5 minutes)".
  - "Add account" is disabled with the no-hosts hint or `ACCOUNT_MANAGEMENT`.
  - The machine filter offers "All machines" and every host's label, and filters the list to accounts with a member there.
- **Summary:** `summaryLine` (Qt `updateRefreshState`). "Loading accounts…" with "Checking accounts across your machines." until the first snapshot. Without hosts, the mockup's empty card with "Open Machines".
- **List rows:** label, provider badge, up to three machine labels (dimmed offline) with "+N", and the permission chip (Varies when the members differ). The tooltip lists each machine's mode.
- **Selection:**
  - The requested profile (Overview card) is selected once listed.
  - Otherwise the profile on the filtered machine, else the first (`AccountsPage::selected`).
- **Detail:**
  - The title with "Rename…" (disabled), and the identity lines or note.
  - The Machines card: at most four chips keeping the selected one, "+N more" with a searchable list (it only selects), the "Default" chip, and "Make default" beside other online, ready machines (disabled). "Add to machine…" is disabled.
  - The status line (`detail`, toned by state), and the sign-in, install and OpenCode start buttons (disabled).
  - The OpenCode providers table and its notes.
  - The permissions line with its hint and "Change…" (disabled), and the model line (when supported) with "Model and effort…" (disabled).
  - "Make default on X" or "Default on X" (not for DeepSeek or OpenCode) and "Remove from X…" (disabled).
- **Usage panel:**
  - "Updated HH:mm", "Shared by sessions using this account.", the offline note and Qt's refreshing or failed notes.
  - Without windows or balances it shows `stateText`.
  - Then every window: period, "N% used" (or "Window ended; refresh for current usage"), the bar in Qt's threshold colour with the elapsed rail, "Resets d MMM HH:mm" and the forecast.
  - Balances and credits follow.
  - Time texts re-render every 15 s while shown.

- [ ] **Step 1: Write the failing tests** (`client/src/ui/screens/accounts-ui.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { creditsText, loginLabel, memberOf, panelWindow, summaryLine, visibleAccounts, visibleMembers } from "./accounts-ui";
import { NOW, projected, usageOf } from "../../store/testing";
import { dayTime } from "../lib/usage";
import { emptyAccounts, type Account, type Profile } from "../../types";

const member = (extra: Partial<Profile> = {}): Profile => ({
  key: "\u001fclaude-work", host: "", machine: "workstation", id: "claude-work", label: "Work", provider: "claude", native: false,
  installed: true, is_default: false, home: "", auth_revision: "", online: true, state: "ready", detail: "Ready on workstation.",
  signed_in: true, permission: "auto", permission_label: "Auto", permission_hint: "", model_supported: true, model_text: "",
  usage: usageOf(), opencode: null, ...extra,
});
const account = (members: Profile[], key = "a"): Account => ({
  key, label: "Work", provider: members[0].provider, installed: true, machines: members.map((m) => m.machine), members,
  usage: usageOf(), permission: "Auto", identity_lines: [], identity_note: "",
});

describe("accounts page", () => {
  it("labels the sign-in button as Qt does", () => {
    expect(loginLabel(member())).toBe("Sign in again on workstation…");
    expect(loginLabel(member({ signed_in: false }))).toBe("Sign in on workstation…");
    expect(loginLabel(member({ provider: "dsh", usage: usageOf({ status: "configured" }) }))).toBe("Change API key on workstation…");
    expect(loginLabel(member({ provider: "dsh", usage: usageOf({ status: "signed_out" }) }))).toBe("Set API key on workstation…");
    expect(loginLabel(member({ provider: "opencode" }))).toBeUndefined();
    expect(loginLabel(member({ installed: false }))).toBeUndefined();
  });

  it("selects the requested profile, else the filtered machine, else the first", () => {
    const studio = member({ key: "studio\u001fclaude-studio", host: "studio", machine: "studio" });
    const a = account([member(), studio]);
    expect(memberOf(a, "studio\u001fclaude-studio", "*")).toBe(studio);
    expect(memberOf(a, undefined, "studio")).toBe(studio);
    expect(memberOf(a, undefined, "*")?.machine).toBe("workstation");
    const other = account([member({ key: "mac\u001fx", host: "mac", machine: "mac" })], "b");
    const view = { ...emptyAccounts(), accounts: [a, other] };
    expect(visibleAccounts(view, "*")).toHaveLength(2);
    expect(visibleAccounts(view, "mac")).toEqual([other]);
  });

  it("shows at most four machine chips, keeping the selected one", () => {
    const many = account(Array.from({ length: 7 }, (_, i) => member({ key: `m${i}\u001fx`, host: `m${i}`, machine: `m${i}` })));
    expect(visibleMembers(many, "m6\u001fx").map((m) => m.machine)).toEqual(["m0", "m1", "m2", "m6"]);
    expect(visibleMembers(many, "m1\u001fx").map((m) => m.machine)).toEqual(["m0", "m1", "m2", "m3"]);
    expect(visibleMembers(account(many.members.slice(0, 5)), undefined)).toHaveLength(5);
  });

  it("words the summary as Qt does", () => {
    const view = { ...emptyAccounts(), snapshot: true, loaded_machines: 2, pending: 3 };
    expect(summaryLine(view, 1)).toBe("1 account, 2 machines loaded (Updating… 3 remaining)");
    expect(summaryLine({ ...view, pending: 0, loaded_machines: 1 }, 4)).toBe("4 accounts, 1 machine loaded");
    expect(summaryLine(emptyAccounts(), 0)).toBe("Loading accounts…");
    const none = "Add a remote machine in Machines to manage agent accounts; this computer runs no agents.";
    expect(summaryLine({ ...view, no_hosts: none }, 0)).toBe(none);
  });

  it("draws a panel window with Qt's threshold colour, its reset and the forecast", () => {
    const w = panelWindow(projected({ used: 75, resets_at: NOW + 150 * 60 }), NOW);
    expect(w).toMatchObject({ period: "5h", usedText: "75% used", used: 75, tone: "warning", resets: `Resets ${dayTime(NOW + 9000)}` });
    expect(w.elapsed).toBeCloseTo(50);
    expect(w.forecast).toBe("At the current pace, the limit runs out in about 50m");
    expect(panelWindow(projected({ resets_at: NOW - 60 }), NOW)).toMatchObject({ usedText: "Window ended; refresh for current usage", used: 0, forecast: "" });
    expect(panelWindow(projected({ label: "Monthly", minutes: null, resets_at: null, used: 50 }), NOW))
      .toMatchObject({ period: "Monthly", usedText: "50% used", elapsed: undefined, resets: undefined, forecast: "" });
    expect(creditsText({ balance: "", unlimited: true })).toBe("Credits: unlimited");
    expect(creditsText({ balance: "lots", unlimited: false })).toBe("Credits available: —");
    expect(creditsText({ balance: "12.5", unlimited: false })).toMatch(/^Credits available: 12[.,]50$/);
    expect(creditsText(null)).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run** `npx vitest run src/ui/screens/accounts-ui.test.ts` in `client`. Expected: FAIL (module missing).

- [ ] **Step 3: Implement `accounts-ui.ts`:**

```ts
import type { Account, AccountsView, MemberState, Profile, ProjectedWindow } from "../../types";
import { current, dayTime, forecast, fromProjected, pace, period, thresholdTone, type Tone } from "../lib/usage";

// Pure texts and selection rules of the Accounts screen (AccountsPage.cpp).

/** The status line's tone (the mockup's statusText tones). */
export const tone = (s: MemberState): "ok" | "busy" | "muted" | "warn" =>
  s === "ready" || s === "apikey" ? "ok" : s === "checking" ? "busy" : s === "offline" ? "muted" : "warn";

/** The sign-in button (AccountsPage.cpp:639-641); none for OpenCode or an agent that is not installed. */
export function loginLabel(p: Profile): string | undefined {
  if (p.provider === "opencode" || !p.installed) return undefined;
  if (p.provider === "dsh") return p.usage.status === "configured" ? `Change API key on ${p.machine}…` : `Set API key on ${p.machine}…`;
  return p.signed_in ? `Sign in again on ${p.machine}…` : `Sign in on ${p.machine}…`;
}

/** The member the detail shows (AccountsPage::selected): the chosen profile, else the filtered machine's, else the first. */
export function memberOf(a: Account, key: string | undefined, machine: string): Profile | undefined {
  return a.members.find((m) => m.key === key) ?? a.members.find((m) => m.host === machine) ?? a.members[0];
}

/** At most `max` machine chips, keeping the selected one (spec round 6); five or fewer all show. */
export function visibleMembers(a: Account, selectedKey: string | undefined, max = 4): Profile[] {
  const all = a.members;
  if (all.length <= max + 1) return all;
  const first = all.slice(0, max);
  const chosen = all.find((m) => m.key === selectedKey);
  return chosen && !first.includes(chosen) ? [...first.slice(0, max - 1), chosen] : first;
}

/** The accounts with a member on `machine` ("*" is every machine). */
export const visibleAccounts = (view: AccountsView, machine: string): Account[] =>
  machine === "*" ? view.accounts : view.accounts.filter((a) => a.members.some((m) => m.host === machine));

/** updateRefreshState (AccountsPage.cpp:440-447); `visible` counts the listed accounts. */
export function summaryLine(view: AccountsView, visible: number): string {
  if (view.no_hosts) return view.no_hosts;
  if (!view.snapshot) return "Loading accounts…";
  const accounts = visible === 1 ? "1 account" : `${visible} accounts`;
  const machines = view.loaded_machines === 1 ? "1 machine loaded" : `${view.loaded_machines} machines loaded`;
  return `${accounts}, ${machines}${view.pending ? ` (Updating… ${view.pending} remaining)` : ""}`;
}

export interface PanelWindow {
  period: string;
  usedText: string;
  /** The bar's share, 0 once the window ended. */
  used: number;
  /** Qt's threshold colour (AccountUsage::color). */
  tone: Tone;
  /** The elapsed share in percent, when the length and reset are known. */
  elapsed?: number;
  resets?: string;
  forecast: string;
}

/** One window of the usage panel (AccountUsage::Panel::setData, AccountUsage.h:368-384, plus the forecast). */
export function panelWindow(p: ProjectedWindow, now: number): PanelWindow {
  const w = fromProjected(p, now);
  const fresh = current(w);
  const known = pace(w);
  return {
    period: period(w),
    usedText: fresh ? `${Math.round(w.used)}% used` : "Window ended; refresh for current usage",
    used: fresh ? Math.max(0, Math.min(100, w.used)) : 0,
    tone: thresholdTone(w.used),
    elapsed: known.known ? known.elapsed * 100 : undefined,
    resets: p.resets_at !== null ? `Resets ${dayTime(p.resets_at)}` : undefined,
    forecast: forecast(w),
  };
}

/** AccountUsage::credits and the panel's credits line. */
export function creditsText(credits: { balance: string; unlimited: boolean } | null): string | undefined {
  if (!credits) return undefined;
  if (credits.unlimited) return "Credits: unlimited";
  const n = Number(credits.balance);
  const text = credits.balance.trim() !== "" && Number.isFinite(n)
    ? n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })
    : "—";
  return `Credits available: ${text}`;
}
```

- [ ] **Step 4: Write `Accounts.tsx`:**

```tsx
import { For, Index, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { MachineTile } from "../components/MachineTile";
import { MachineLabel } from "../components/MachineLabel";
import { Actions, Spinner, Status } from "../components/Status";
import { AgentBadge } from "../sessions/SessionCard";
import type { Account } from "../../types";
import { agentKind, catalog } from "../../store/catalog";
import { machineCards } from "../../store/machines";
import { accountMachine, accountsPage, loadAccounts, requestedProfile, setAccountMachine, setRequestedProfile } from "../../store/accounts";
import { connectionOf, osOf } from "../../store/overview";
import { setPage } from "../../store/ui";
import { clock, providerName, stateText } from "../lib/usage";
import { ACCOUNT_MANAGEMENT } from "./reasons";
import { creditsText, loginLabel, memberOf, panelWindow, summaryLine, tone, visibleAccounts, visibleMembers } from "./accounts-ui";

// Agent accounts (AccountsPage.cpp), ported from design/webui/src/screens/Accounts.tsx:
// profiles grouped by provider identity across machines; choosing a machine
// tag selects that machine's profile. Every write (rename, defaults,
// permissions, sign-in, install, remove, add) arrives with account management
// (3c) and keeps its place disabled.

const [selectedAccount, setSelectedAccount] = createSignal<string | undefined>();
const [selectedProfile, setSelectedProfile] = createSignal<string | undefined>();

function AccountRow(props: { a: Account }) {
  const on = () => selectedAccount() === props.a.key;
  const pick = () => {
    setSelectedAccount(props.a.key);
    setSelectedProfile(memberOf(props.a, undefined, accountMachine())?.key);
  };
  return (
    <button class="acct-row" classList={{ on: on() }} aria-current={on()} title={props.a.members.map((m) => `${m.machine}: ${m.permission_label}`).join("\n")} onClick={pick}>
      <span class="row">
        <b class="ell grow">{props.a.label}</b>
        <AgentBadge agent={agentKind(props.a.provider)} />
      </span>
      <span class="row acct-row-tags">
        <span class="row acct-machines">
          <For each={props.a.members.slice(0, 3)}>{(m) => <MachineLabel alias={m.machine} dimmed={!m.online} />}</For>
          <Show when={props.a.members.length > 3}><span class="dim">+{props.a.members.length - 3}</span></Show>
        </span>
        <span class="grow" />
        <span class="acct-perm" classList={{ bypass: props.a.permission === "Bypass" }}>{props.a.permission}</span>
      </span>
    </button>
  );
}

function UsagePanel(props: { a: Account }) {
  // Resets and pace re-render every 15 s while shown; nothing else runs on a timer.
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), 15_000);
  onCleanup(() => clearInterval(timer));
  const u = () => props.a.usage;
  return (
    <section class="acct-usage box">
      <div class="row">
        <h3>Account usage</h3>
        <span class="grow" />
        <Show when={u().checked_at !== null}><span class="dim">Updated {clock(u().checked_at!)}</span></Show>
      </div>
      <p class="muted">Shared by sessions using this account.</p>
      <Show when={u().offline}><Status>Machine is offline. Showing the last available account data.</Status></Show>
      <Show when={u().refreshing}><Status kind="busy">Updating… Previous values remain visible.</Status></Show>
      <Show when={!u().refreshing && u().refresh_error}><Status kind="warn">Update failed. Showing the last available values.</Status></Show>
      <Show when={!u().windows.length && !u().balances.length && !u().offline}><p>{stateText(u())}</p></Show>
      <For each={u().windows}>{(p) => {
        const w = () => panelWindow(p, now());
        return (
          <div class="acct-window">
            <div class="row"><b class="tabular">{w().period}</b><span class="tabular">{w().usedText}</span></div>
            <div class="acct-meter rails">
              <div class="acct-bar" classList={{ warn: w().tone === "warning", danger: w().tone === "critical" }}><i style={{ "--w": `${w().used}%` }} /></div>
              <Show when={w().elapsed !== undefined}>
                <div class="acct-bar elapsed" title={`${Math.round(w().elapsed!)}% of the window elapsed`}><i style={{ "--w": `${w().elapsed}%` }} /></div>
              </Show>
            </div>
            <div class="row dim">
              <Show when={w().resets}><span>{w().resets}</span></Show>
              <span class="grow" />
              <span class="acct-pace">{w().forecast}</span>
            </div>
          </div>
        );
      }}</For>
      <For each={u().balances}>{(b) => <p>{`${b.kind}: ${b.balance} ${b.currency}`}</p>}</For>
      <Show when={creditsText(u().credits)}>{(text) => <p>{text()}</p>}</Show>
    </section>
  );
}

function Detail(props: { a: Account }) {
  const m = () => memberOf(props.a, selectedProfile(), accountMachine())!;
  const [moreOpen, setMoreOpen] = createSignal(false);
  const [moreQuery, setMoreQuery] = createSignal("");
  const shown = () => visibleMembers(props.a, m().key);
  const nativeConfig = () => props.a.provider === "opencode";
  const defaultable = () => props.a.provider !== "dsh" && !nativeConfig();
  const matches = () => props.a.members.filter((x) => x.machine.toLowerCase().includes(moreQuery().trim().toLowerCase()));
  return (
    <div class="acct-detail">
      <div class="acct-title">
        <h2 class="ell">{props.a.label}</h2>
        <span class="grow" />
        <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="pencil" size={13} />Rename…</button>
      </div>
      <div class="row acct-identity">
        <AgentBadge agent={agentKind(props.a.provider)} />
        <div class="col selectable">
          <For each={props.a.identity_lines}>{(line) => <span>{line}</span>}</For>
          <Show when={props.a.identity_note}><span class="muted">{props.a.identity_note}</span></Show>
        </div>
      </div>

      <section class="acct-card box">
        <h3>Machines</h3>
        <div class="acct-tags" role="radiogroup" aria-label="Machine profile">
          <For each={shown()}>{(x) => (
            <span class="acct-tag-wrap">
              <button class="acct-tag" role="radio" aria-checked={x.key === m().key} classList={{ on: x.key === m().key, offline: !x.online }} onClick={() => setSelectedProfile(x.key)}>
                <MachineLabel alias={x.machine} />{x.online ? "" : " (offline)"}
                <Show when={x.is_default}>
                  <span class="acct-default" title={`Default for new ${providerName(props.a.provider)} sessions on ${x.machine} when no account is chosen`}>Default</span>
                </Show>
              </button>
              <Show when={!x.is_default && x.online && x.state === "ready" && defaultable()}>
                <button class="btn small ghost acct-make-default" disabled title={ACCOUNT_MANAGEMENT}>Make default</button>
              </Show>
            </span>
          )}</For>
          <Show when={props.a.members.length > shown().length}>
            <span class="acct-more-anchor">
              <button class="acct-tag more" aria-haspopup="dialog" aria-expanded={moreOpen()} onClick={() => { setMoreQuery(""); setMoreOpen(!moreOpen()); }}>
                +{props.a.members.length - shown().length} more
              </button>
              <Show when={moreOpen()}>
                <div class="acct-more" role="dialog" aria-label={`Machines with ${props.a.label}`}
                  onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setMoreOpen(false); } }}>
                  <input class="field" type="search" placeholder={`Search ${props.a.members.length} machines`} value={moreQuery()}
                    onInput={(e) => setMoreQuery(e.currentTarget.value)} autofocus aria-label="Search machines" />
                  <div class="acct-more-list">
                    <For each={matches()} fallback={<div class="dim acct-more-empty">No machine matches.</div>}>
                      {(x) => (
                        <button class="acct-more-row" classList={{ on: x.key === m().key }} onClick={() => { setSelectedProfile(x.key); setMoreOpen(false); }}>
                          <MachineTile os={osOf(machineCards[x.host]?.platform)} state={x.online ? connectionOf(catalog.computers[x.host]?.status).tile : "offline"} size={22} />
                          <MachineLabel alias={x.machine} />
                          <span class="grow" />
                          <Show when={x.is_default}><span class="acct-default">Default</span></Show>
                          <span class="dim">{x.state === "ready" ? "" : x.state === "signed-out" ? "Sign-in needed" : x.state}</span>
                        </button>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </span>
          </Show>
          <button class="btn small acct-add" disabled title={ACCOUNT_MANAGEMENT}><Icon name="plus" size={12} />Add to machine…</button>
        </div>
        <div class="acct-status">
          <Status kind={tone(m().state)}>{m().detail}</Status>
          <Actions>
            <Show when={loginLabel(m())}>{(label) => <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="key-round" size={13} />{label()}</button>}</Show>
            <Show when={!m().installed}><button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="download" size={13} />Install agent…</button></Show>
            <Show when={m().opencode?.stopped}>
              <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="play" size={13} />Start OpenCode on {m().machine}</button>
            </Show>
          </Actions>
        </div>
        <Show when={m().opencode}>{(oc) => (
          <>
            <Show when={oc().providers.length}>
              <div class="acct-oc" aria-label="OpenCode providers">
                <For each={oc().providers}>{(p) => (
                  <div class="acct-oc-row"><span class="grow">{p.label}</span><span class="muted tabular">{p.models}</span><span class="dim">{p.connection}</span></div>
                )}</For>
              </div>
            </Show>
            <Show when={oc().truncated}><p class="dim">Showing the first {oc().providers.length} providers.</p></Show>
            <Show when={oc().stale_catalog_at !== null}>
              <p class="dim">Last known catalog from {new Date(oc().stale_catalog_at! * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" })}.</p>
            </Show>
          </>
        )}</Show>
        <div class="acct-setting">
          <span title={m().permission_hint}>
            Permissions on {m().machine}: <b classList={{ danger: m().permission === "bypass" }}>{m().permission_label}</b>
            <span class="dim acct-src">{m().permission === "bypass" ? "set in Zerus" : "from provider settings"}</span>
          </span>
          <button class="btn small" disabled title={ACCOUNT_MANAGEMENT}>Change…</button>
        </div>
        <Show when={m().model_supported}>
          <div class="acct-setting">
            <span>{m().model_text}</span>
            <button class="btn small" disabled title={ACCOUNT_MANAGEMENT}>Model and effort…</button>
          </div>
        </Show>
        <Actions>
          <Show when={defaultable()}>
            <button class="btn" disabled title={ACCOUNT_MANAGEMENT}>{m().is_default ? `Default on ${m().machine}` : `Make default on ${m().machine}`}</button>
          </Show>
          <button class="btn danger" disabled title={ACCOUNT_MANAGEMENT}><Icon name="trash" size={13} />Remove from {m().machine}…</button>
        </Actions>
      </section>
      <UsagePanel a={props.a} />
    </div>
  );
}

export function AccountsScreen() {
  const view = accountsPage;
  const visible = createMemo(() => visibleAccounts(view(), accountMachine()));
  // An Overview card asks for a profile: select its account once it is listed.
  createEffect(() => {
    const key = requestedProfile();
    const account = key ? view().accounts.find((a) => a.members.some((m) => m.key === key)) : undefined;
    if (!account) return;
    setSelectedAccount(account.key);
    setSelectedProfile(key);
    setRequestedProfile(undefined);
  });
  const current = () => visible().find((a) => a.key === selectedAccount()) ?? visible()[0];
  const filterName = () => view().hosts.find((h) => h.computer === accountMachine())?.name ?? accountMachine();
  return (
    <div class="mach-page acct-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Agent accounts</h1>
          <button class="icon-btn" aria-label="Refresh accounts" disabled={view().busy} onClick={() => void loadAccounts(true)}
            title={view().busy ? `Updating accounts… ${view().pending} remaining` : "Refresh account usage (automatically every 5 minutes)"}>
            <Show when={!view().busy} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled title={view().no_hosts ?? ACCOUNT_MANAGEMENT}><Icon name="plus" size={14} />Add account</button>
        </header>
        <p class="muted mach-summary">Your accounts across all machines. Choose an account to manage where it is available.</p>
        <For each={view().notices}>{(notice) => <Status kind="warn">{notice}</Status>}</For>
        <Show when={!view().no_hosts} fallback={
          <div class="empty acct-empty"><Icon name="user-round-cog" size={28} /><h3>No machine keeps agent accounts</h3>
            <p>{view().no_hosts}</p>
            <button class="btn" onClick={() => setPage("machines")}>Open Machines</button></div>
        }>
          <div class="row acct-filter">
            <select class="field" aria-label="Machine" value={accountMachine()} onChange={(e) => setAccountMachine(e.currentTarget.value)}>
              <option value="*">All machines</option>
              <For each={view().hosts}>{(h) => <option value={h.computer}>{h.label}</option>}</For>
            </select>
            <Show when={view().snapshot}>
              <Show when={view().pending}><Spinner size={12} /></Show>
              <span class="muted">{summaryLine(view(), visible().length)}</span>
            </Show>
          </div>
          <Show when={view().snapshot} fallback={
            <div class="acct-loading"><div class="row"><Spinner /><span>Loading accounts…</span></div><p class="muted">Checking accounts across your machines.</p></div>
          }>
            <div class="mach-body acct-body">
              <div class="mach-list acct-list" role="list" aria-label="Agent accounts">
                <Show when={visible().length} fallback={<p class="muted acct-none">No accounts on {filterName()}.</p>}>
                  <Index each={visible()}>{(a) => <AccountRow a={a()} />}</Index>
                </Show>
              </div>
              <Show when={current()} fallback={<div class="acct-detail"><h2 class="muted">Choose an account</h2></div>}>
                {(a) => <Detail a={a()} />}
              </Show>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
}
```

In `client/src/App.tsx`, import `AccountsScreen` from `./ui/screens/Accounts` and add after the machines match:

```tsx
          <Match when={page() === "accounts"}><AccountsScreen /></Match>
```

- [ ] **Step 5: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py` in `client`. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/src
git commit -m "feat(client): the Accounts screen with Qt's grouping, states and usage panel"
```

---

### Task 8: The Projects screen

**Files:**
- Create: `client/src/ui/screens/projects-ui.ts`, `client/src/ui/screens/projects-ui.test.ts`, `client/src/ui/screens/Projects.tsx`
- Modify: `client/src/App.tsx` (route `projects`; `LaterScreen` and its `SCREEN` map go, since every page now has its screen), `client/src/ui/styles/client.css` (drop the unused `.later-screen` rule)

**Interfaces:**
- Consumes: `projectsPage` (Task 4); `MachineLabel`, the reasons (Task 6); `setMenu`, `MenuEntry` (`store/ui.ts`).
- Produces:

```ts
export function ProjectsScreen(): JSX.Element;
// projects-ui.ts
export type SortColumn = "name" | "machine" | "path";
export interface Sort { column: SortColumn; ascending: boolean }
export function sortFolders(folders: readonly Folder[], sort: Sort): { folder: Folder; index: number }[];
export const nextSort: (sort: Sort, column: SortColumn) => Sort;
export const shownProjects: (view: ProjectsView, all: boolean) => Project[];
export const projectColor: (p: Project) => string;
```

Rules (`ProjectsDialog.cpp`, `SwarmController.cpp`, the mockup):
- **Header:** "Worktrees…" (disabled, `WORKTREE_FLOWS`) and "Swarm…" (disabled, `PROJECT_EDITING`), then the hint line.
- **Notice:** the unconnected-computer notice with "Connect project catalogs…" (disabled) when the view has one.
- **Swarm line:** "All swarm projects" works for the session and lists the inaccessible projects as plain rows. The status line uses the error style when `status_error`.
- **List:**
  - The shared order. Up and Down move the selection; there is no reordering.
  - The context menu lists "New project…", "Move project up" and "Move project down", disabled with their reason and no keyboard hints.
  - The tooltip names the default project.
- **Right pane:**
  - The name (read-only), the ID, and the colour preview with "Edit color…" (disabled).
  - "Default for sessions started outside Zerus" is checked for the default project and disabled.
  - The folders table sorts by Name, Computer or Folder (click a heading; its tooltip "Click a column heading to sort folders.").
  - The folder buttons are disabled: "Clone repository…", "New session…" (`NEW_SESSION`), "Add folder…", "Edit folder…", "Remove folder" and "Open folder".
  - The folder context menu has Qt's entries, each disabled with its reason. "Copy folder path" uses `COPY_ACTIONS`.
  - "Delete project…" is disabled.
  - With no project the pane is hidden, as in Qt.
- **Out of 3a:** the Worktrees panel under the folders (phase 5).

- [ ] **Step 1: Write the failing tests** (`client/src/ui/screens/projects-ui.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { nextSort, projectColor, shownProjects, sortFolders } from "./projects-ui";
import { emptyProjects, type Folder, type Project } from "../../types";

const folder = (name: string, computer: string, path: string): Folder => ({ id: name, name, computer, machine: computer, path });
const project = (id: string, accessible: boolean, color: string | null = null): Project =>
  ({ id, name: id, color, accessible, is_default: false, palette: 3, folders: [] });

describe("projects page", () => {
  it("sorts folders by a column, case-insensitively, keeping their indices", () => {
    const folders = [folder("docs", "Windows lab", "C:\\docs"), folder("Api", "workstation", "/home/example/api"), folder("web", "studio", "/home/example/web")];
    expect(sortFolders(folders, { column: "name", ascending: true }).map((x) => x.index)).toEqual([1, 0, 2]);
    expect(sortFolders(folders, { column: "machine", ascending: true }).map((x) => x.folder.computer)).toEqual(["studio", "Windows lab", "workstation"]);
    expect(sortFolders(folders, { column: "path", ascending: false }).map((x) => x.index)).toEqual([0, 2, 1]);
    expect(nextSort({ column: "name", ascending: true }, "name")).toEqual({ column: "name", ascending: false });
    expect(nextSort({ column: "name", ascending: false }, "path")).toEqual({ column: "path", ascending: true });
  });

  it("lists remote projects only with All swarm projects and colours by the project", () => {
    const view = { ...emptyProjects(), loaded: true, projects: [project("a", true, "#3fb950"), project("b", false)] };
    expect(shownProjects(view, false).map((p) => p.id)).toEqual(["a"]);
    expect(shownProjects(view, true).map((p) => p.id)).toEqual(["a", "b"]);
    expect(projectColor(view.projects[0])).toBe("#3fb950");
    expect(projectColor(view.projects[1])).toBe("var(--z-entity-palette-3)");
  });
});
```

- [ ] **Step 2: Run** `npx vitest run src/ui/screens/projects-ui.test.ts` in `client`. Expected: FAIL (module missing).

- [ ] **Step 3: Implement `projects-ui.ts`:**

```ts
import type { Folder, Project, ProjectsView } from "../../types";

// Pure rules of the Projects screen (ProjectsDialog.cpp).

export type SortColumn = "name" | "machine" | "path";
export interface Sort { column: SortColumn; ascending: boolean }

/** The folders table sorted by a column, case-insensitively; each row keeps its index in the project's folders. */
export function sortFolders(folders: readonly Folder[], sort: Sort): { folder: Folder; index: number }[] {
  const key = (f: Folder) => (sort.column === "name" ? f.name : sort.column === "machine" ? f.computer : f.path).toLowerCase();
  const direction = sort.ascending ? 1 : -1;
  return folders
    .map((folder, index) => ({ folder, index }))
    .sort((a, b) => {
      const ka = key(a.folder), kb = key(b.folder);
      return (ka < kb ? -1 : ka > kb ? 1 : a.index - b.index) * direction;
    });
}

/** A heading click: the same column flips, another one sorts ascending. */
export const nextSort = (sort: Sort, column: SortColumn): Sort => ({ column, ascending: sort.column === column ? !sort.ascending : true });

/** "All swarm projects" adds the projects of computers without a configured connection. */
export const shownProjects = (view: ProjectsView, all: boolean) => (all ? view.projects : view.projects.filter((p) => p.accessible));

/** The project's own colour, else its palette slot (this device's colours arrive with project editing). */
export const projectColor = (p: Project) => p.color ?? `var(--z-entity-palette-${p.palette})`;
```

- [ ] **Step 4: Write `Projects.tsx`:**

```tsx
import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { MachineLabel } from "../components/MachineLabel";
import type { Project } from "../../types";
import { projectsPage } from "../../store/projects";
import { setMenu, type MenuEntry } from "../../store/ui";
import { COPY_ACTIONS, NEW_SESSION, PROJECT_EDITING, WORKTREE_FLOWS } from "./reasons";
import { nextSort, projectColor, shownProjects, sortFolders, type Sort, type SortColumn } from "./projects-ui";

// Projects (ProjectsDialog.cpp), ported from design/webui/src/screens/Projects.tsx:
// the shared project catalog of `hgs swarm get`, its folders on each computer
// and the swarm's status. Editing (names, colours, defaults, folders, order,
// deleting, the swarm connection) arrives with project editing (3c), New
// session with phase 5, and the Worktrees panel with phase 5.

function ProjectLabel(props: { project: Project; class?: string }) {
  return (
    <span class={`su-plabel entity ${props.class ?? ""}`} style={{ "--entity": projectColor(props.project) }}>
      <Icon name={props.project.id === "ungrouped" ? "layers" : "folder"} size={14} stroke={1.8} />
      <span class="ell">{props.project.name}</span>
      <Show when={props.project.is_default}><span class="su-pldefault">Default</span></Show>
    </span>
  );
}

const [selectedProject, setSelectedProject] = createSignal<string | undefined>();
const [selectedFolder, setSelectedFolder] = createSignal(0);
const [allSwarm, setAllSwarm] = createSignal(false);

const disabled = (label: string, icon: string, hint: string): MenuEntry => ({ label, icon, disabled: true, hint });

export function ProjectsScreen() {
  const view = projectsPage;
  const [sort, setSort] = createSignal<Sort>({ column: "name", ascending: true });
  const projects = createMemo(() => shownProjects(view(), allSwarm()));
  const project = () => projects().find((p) => p.id === selectedProject()) ?? projects().find((p) => p.accessible);
  const folders = createMemo(() => sortFolders(project()?.folders ?? [], sort()));
  onCleanup(() => setMenu(undefined));
  const choose = (p: Project) => { setSelectedProject(p.id); setSelectedFolder(0); };

  const folderMenu = (e: MouseEvent, index: number) => {
    e.preventDefault();
    setSelectedFolder(index);
    const others = projects().filter((p) => p.accessible && p.id !== project()?.id);
    setMenu({ x: e.clientX, y: e.clientY, items: [
      disabled("New session…", "plus", NEW_SESSION),
      disabled("Open folder", "folder-open", PROJECT_EDITING),
      disabled("Copy folder path", "copy", COPY_ACTIONS),
      { separator: true },
      { heading: "Copy to" },
      ...others.map((p) => disabled(p.name, "folder", PROJECT_EDITING)),
      { heading: "Move to" },
      ...others.map((p) => disabled(p.name, "arrow-right", PROJECT_EDITING)),
      { separator: true },
      disabled("Edit folder…", "pencil", PROJECT_EDITING),
      disabled("Remove folder from project", "minus", PROJECT_EDITING),
      disabled("Add folder…", "plus", PROJECT_EDITING),
    ] });
  };

  const listMenu = (e: MouseEvent, id?: string) => {
    e.preventDefault();
    const p = projects().find((x) => x.id === id);
    if (p) choose(p);
    setMenu({ x: e.clientX, y: e.clientY, items: [
      disabled("New project…", "plus", PROJECT_EDITING),
      ...(p ? [{ separator: true } as const, disabled("Move project up", "arrow-up", PROJECT_EDITING), disabled("Move project down", "arrow-down", PROJECT_EDITING)] : []),
    ] });
  };

  const onListKey = (e: KeyboardEvent) => {
    if (e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const list = projects().filter((p) => p.accessible);
    const i = list.findIndex((p) => p.id === project()?.id);
    const next = list[Math.min(list.length - 1, Math.max(0, i + (e.key === "ArrowUp" ? -1 : 1)))];
    if (!next) return;
    choose(next);
    (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-id="${CSS.escape(next.id)}"]`)?.focus();
  };

  const header = (column: SortColumn, label: string) => (
    <th aria-sort={sort().column === column ? (sort().ascending ? "ascending" : "descending") : "none"}>
      <button class="su-th" title="Click a column heading to sort folders." onClick={() => setSort((s) => nextSort(s, column))}>
        {label}<Show when={sort().column === column}><Icon name={sort().ascending ? "chevron-down" : "arrow-up"} size={11} /></Show>
      </button>
    </th>
  );

  return (
    <div class="su-page projects">
      <div class="proj-content">
        <header class="su-head">
          <h1>Projects</h1>
          <span class="grow" />
          <button class="btn" disabled title={WORKTREE_FLOWS}><Icon name="folder-git-2" size={14} />Worktrees…</button>
          <button class="btn" disabled title={PROJECT_EDITING}><Icon name="network" size={14} />Swarm…</button>
        </header>
        <p class="muted proj-hint">Organize sessions and folders across your computers. A folder can be used by several projects.</p>

        <Show when={view().notice}>{(notice) => (
          <div class="notice proj-guide">
            <Icon name="link" size={14} />
            <span class="grow">{notice()}</span>
            <button class="btn small" disabled title={PROJECT_EDITING}>Connect project catalogs…</button>
          </div>
        )}</Show>

        <div class="row proj-swarm">
          <label class="check" title="Include projects on computers without a configured connection here.">
            <input type="checkbox" checked={allSwarm()} onChange={(e) => setAllSwarm(e.currentTarget.checked)} />All swarm projects
          </label>
          <span class="muted" classList={{ danger: view().status_error }}>{view().status}</span>
        </div>

        <div class="proj-split">
          <aside class="proj-left">
            <ul class="proj-list" role="listbox" aria-label="Projects" tabindex="-1" onKeyDown={onListKey}
              onContextMenu={(e) => { if (e.target === e.currentTarget) listMenu(e); }}>
              <For each={projects()}>{(p) => (
                <li role="option" aria-selected={p.id === project()?.id}>
                  <Show when={p.accessible} fallback={<div class="proj-item remote" title={p.name}><ProjectLabel project={p} /></div>}>
                    <button class="proj-item" classList={{ on: p.id === project()?.id }} data-id={p.id} tabindex={p.id === project()?.id ? 0 : -1}
                      title={p.is_default ? `${p.name}, default project for sessions started outside Zerus` : p.name}
                      onClick={() => choose(p)} onContextMenu={(e) => listMenu(e, p.id)}>
                      <ProjectLabel project={p} />
                    </button>
                  </Show>
                </li>
              )}</For>
            </ul>
            <button class="btn" disabled title={PROJECT_EDITING}><Icon name="plus" size={14} />New project…</button>
          </aside>

          <Show when={project()}>{(p) => (
            <section class="proj-right" aria-label="Project">
              <div class="form-row">
                <label for="proj-name">Name</label>
                <div class="row">
                  <input id="proj-name" class="field proj-name" value={p().name} readOnly title={PROJECT_EDITING} />
                  <span class="dim proj-id">ID</span>
                  <span class="mono proj-id-val" title="Project ID">{p().id}</span>
                </div>
              </div>
              <div class="form-row">
                <label>Color</label>
                <div class="row">
                  <ProjectLabel project={p()} class="proj-preview" />
                  <button class="btn" disabled title={PROJECT_EDITING}><Icon name="palette" size={14} />Edit color…</button>
                </div>
              </div>
              <label class="check proj-default" title={PROJECT_EDITING}>
                <input type="checkbox" checked={p().is_default} disabled />Default for sessions started outside Zerus
              </label>

              <div class="proj-folders">
                <div class="row proj-fhead">
                  <h2 class="section-title">Folders</h2>
                  <span class="grow" />
                  <button class="btn small" disabled title={PROJECT_EDITING}><Icon name="download" size={12} />Clone repository…</button>
                  <button class="btn small" disabled title={NEW_SESSION}><Icon name="plus" size={12} />New session…</button>
                </div>
                <div class="su-tablewrap">
                  <table class="su-table proj-table">
                    <thead><tr>{header("name", "Name")}{header("machine", "Computer")}{header("path", "Folder")}</tr></thead>
                    <tbody>
                      <For each={folders()}>{(row) => (
                        <tr classList={{ on: selectedFolder() === row.index }} tabindex="0" aria-selected={selectedFolder() === row.index}
                          onClick={() => setSelectedFolder(row.index)} onContextMenu={(e) => folderMenu(e, row.index)}>
                          <td class="ell" title={row.folder.name}>{row.folder.name}</td>
                          <td><MachineLabel alias={row.folder.computer} /></td>
                          <td class="mono ell" title={row.folder.path}>{row.folder.path}</td>
                        </tr>
                      )}</For>
                    </tbody>
                  </table>
                </div>
                <div class="row proj-factions">
                  <button class="btn small" disabled title={PROJECT_EDITING}>Add folder…</button>
                  <button class="btn small" disabled title={PROJECT_EDITING}>Edit folder…</button>
                  <button class="btn small" disabled title={PROJECT_EDITING}>Remove folder</button>
                  <span class="grow" />
                  <button class="btn small" disabled title={PROJECT_EDITING}><Icon name="folder-open" size={12} />Open folder</button>
                </div>
              </div>
              <div class="row proj-foot">
                <button class="btn danger" disabled title={PROJECT_EDITING}><Icon name="trash" size={14} />Delete project…</button>
              </div>
            </section>
          )}</Show>
        </div>
      </div>
    </div>
  );
}
```

In `client/src/App.tsx`:
- import `ProjectsScreen` from `./ui/screens/Projects` and add `<Match when={page() === "projects"}><ProjectsScreen /></Match>` after the accounts match;
- delete `SCREEN`, `LaterScreen` and the last `<Match when={page() !== "sessions"}>` (every page now has a screen), and drop the `Icon` and `type Page` imports they used.

In `client/src/ui/styles/client.css`, delete the `.later-screen { flex: 1; align-self: center; }` line.

- [ ] **Step 5: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py` in `client`. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/src
git commit -m "feat(client): the Projects screen on the shared project catalog"
```

---

### Task 9: Documentation, the follow-up task, the headless smoke and the checks

**Files:**
- Modify: `client/README.md` (new "Screens" section after "Actions"; the build list mentions the new tests)
- Modify: `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md` (the later-phases table)
- Modify: `client/src/ui/PerfHost.tsx` (the `screens-smoke=1` hook)
- Modify: `.beads/issues.jsonl` (export after the new task)

**Interfaces:**
- Consumes: everything above; `perf_config().actions_smoke` (phase 2: replay, an explicit `ZERUS_CLIENT_HGS` and `ZERUS_CLIENT_PERF`), `exit_app`, `perf_log`.
- Produces: the `screens-smoke=1` hook, the README section and the phase table rows.

- [ ] **Step 1: File the polling fallback** (the Decisions note; no child of `zerus-5wmh` covers it; Beads text carries no absolute paths):

```bash
bd create "Tauri client: poll peers without the watch-v1 stream" --parent zerus-5wmh -t task -p 2 \
  -d "Peers whose hgs predates the event stream (status unsupported) show no machine card, metrics or sessions in the new client. The spec keeps today's polling for them: hgs @alias ls --json on the Qt cadence (60 s per peer, 15 s timeout, one read in flight), inspect --after for the followed session, and the same view rows and machine card projection as the stream. Found while planning phase 3a (zerus-5wmh.8); 3a shows such peers as 'Update hgs'."
bd export -o .beads/issues.jsonl
python3 scripts/check-source-privacy.py
```

Expected: `bd` prints the new ID (`zerus-5wmh.N`), and the privacy check reports 0 findings. Use that ID in Step 3's table.

- [ ] **Step 2: The screens smoke hook.** In `client/src/ui/PerfHost.tsx`, import `setPage` and `type Page` from `../store/ui`, `accountsPage` from `../store/accounts`, `machinesPage` from `../store/machines` and `projectsPage` from `../store/projects`. Add before `onScenarioStart`:

```ts
// `?screens-smoke=1` (ZERUS_CLIENT_QUERY=screens-smoke=1, in the environment
// the actions smoke needs) shows Overview, Machines, Accounts and Projects in
// turn on the replay catalog with a fake hgs answering the reads. It waits
// until each view loaded, logs "screens-smoke <screen>: shown", holds the page
// 3 s (for a screenshot) and exits 0, or 1 when a view did not load in 30 s.
async function screensSmoke(allowed: boolean) {
  const exit = (code: number) => invoke("exit_app", { code }).catch(() => {});
  if (!allowed) {
    log("screens-smoke not started: it needs ZERUS_CLIENT_REPLAY, ZERUS_CLIENT_HGS and ZERUS_CLIENT_PERF");
    return;
  }
  const steps: [Page, string, () => boolean][] = [
    ["dashboard", "overview", () => accountsPage().snapshot && accountsPage().overview.length > 0],
    ["machines", "machines", () => machinesPage().loaded],
    ["accounts", "accounts", () => accountsPage().snapshot && accountsPage().pending === 0],
    ["projects", "projects", () => projectsPage().loaded],
  ];
  try {
    for (const [target, name, ready] of steps) {
      setPage(target);
      await until(ready, 30_000, `${name} did not load`);
      log(`screens-smoke ${name}: shown`);
      await new Promise((resolve) => setTimeout(resolve, 3000));
    }
    await exit(0);
  } catch (error) {
    log(`screens-smoke failed: ${String(error)}`);
    await exit(1);
  }
}
```

and in `PerfHost`'s `onMount`, after the actions smoke line:

```ts
    if (new URLSearchParams(location.search).get("screens-smoke") === "1" && inTauri()) void screensSmoke(!!config().actions_smoke);
```

- [ ] **Step 3: Documentation.**

In `client/README.md`, add after the "Actions" section (before "Measurement run"):

````markdown
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
````

In `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`, replace the phase 3 row of "Later phases" with:

```markdown
| 3a | Overview, Projects, Machines and Accounts on live data, reads only: machine cards from `catalog.meta`, account, machine and swarm reads through the screens hub; writes disabled with their phase; plan: `docs/superpowers/plans/2026-10-28-tauri-client-phase3a-screens.md` | zerus-5wmh.8 |
| 3b | Settings, including the Phones panel the mockup shows under Machines | zerus-5wmh.24 |
| 3c | Machines, Accounts and Projects writes (connections, install, accounts, defaults, permissions, models, projects, folders, swarm, the legacy `project ls` import, Open folder, local label and project colours) | zerus-5wmh.25 |
| 3 | Peers without the watch-v1 stream: Qt's polling fallback (`hgs @alias ls --json`) | the ID from Task 9 Step 1 |
```

and extend the phase 5 row's scope with "; the Projects Worktrees panel and its `worktrees` read; New session from a project folder".

In the README's "Build and run" block, change the `npm test` comment to `# Vitest: stores, transcript, screens`.

- [ ] **Step 4: Run every check** (from `client`, except the first and last):
  - `cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core` and the clippy and fmt checks of Task 1 Step 5 (repository root);
  - `cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml`, and `cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, with and without `--no-default-features`;
  - `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py`;
  - `python3 scripts/check-source-privacy.py` (repository root).

  Expected: all pass. Fix any warning here (this is the task's lint debt); never silence one with `allow` unless the reason is written beside it.

- [ ] **Step 5: Headless smoke.**
  - Build the release app (`RUSTUP_TOOLCHAIN=1.99.1 npx tauri build --no-bundle` in `client`) and run the README's "Screens smoke run" script.
  - Expected:
    - `exit 0`, and four lines `screens-smoke overview: shown` through `screens-smoke projects: shown`;
    - the call counts show `account ls`, `@mac account ls`, the inspects, `machine ls` and `swarm get`, and never `machine check`, `machine set`, `account` writes or `swarm apply`;
    - four screenshots in `$S`.
  - Look at each screenshot against the mockup (`design/webui`, `?page=` with the same screen). Record the command, the log lines and the counts in the PR description, and keep the screenshots in the scratch folder only.

- [ ] **Step 6: Commit**

```bash
git add client/README.md client/src/ui/PerfHost.tsx docs/superpowers/plans/2026-10-28-tauri-client-phase1.md .beads/issues.jsonl
git commit -m "docs(client): the screens, their reads and the screens smoke run"
```

---

## Phases of the screen entries not in 3a

| Entry | Phase and Beads task |
|---|---|
| Machines: Add machine, Save connection, Discard draft, Browse…, Open SSH terminal, Install / update HGS…, Enable local sessions, Reset SSH overrides, Remove from Zerus, label colour and style | 3c, `zerus-5wmh.25` |
| Accounts: Add account, Rename…, Make default, Add to machine… (the three-step dialog), Sign in, Set API key, Install agent…, Start OpenCode, Permissions Change…, Model and effort…, Remove | 3c, `zerus-5wmh.25` |
| Projects: New project, rename, colour, default, reorder, folders (add, edit, remove, copy and move), Clone repository…, Delete project…, Swarm…, Connect project catalogs…, the legacy `project ls` import, Open folder | 3c, `zerus-5wmh.25` |
| Projects: New session… from a folder, Worktrees… and the Worktrees panel | Phase 5, `zerus-5wmh.10` |
| Projects: Copy folder path | `zerus-5wmh.19` |
| The Phones panel (shown under Machines in the mockup) | 3b, `zerus-5wmh.24` |
| Full-width Overview and the default Overview width | 3b, `zerus-5wmh.24` |
| Peers without watch-v1: metrics, sessions and accounts through polling | the task filed in Task 9 Step 1 |
