# Tauri Client: New Session Dialog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client opens Qt's New session dialog (`tray/src/NewSessionDialog.cpp`) from every place Qt opens it. It has Qt's inputs, enablement, texts and outcome rules, and it hands its plan to background creation as Qt's generic New session does (`SessionsWindow.cpp:2910-2919`). Beads `zerus-5wmh.10.10`, the view part of `zerus-5wmh.10`. In scope:
- the six launch options of the core section's N1 list (`2026-10-28-tauri-client-phase5-core-section.md`, Task N1), with their texts:
  - the direct-folder launch, "Start directly in the working folder." (208);
  - the explicit Worktree options, existing or manual (210-216);
  - the outside-project warning (683);
  - the generated worktree's tooltip (704);
  - the clone checkout's project change (737, see D2);
  - the offline folder (719);
- None, Auto, and the explicit existing (Choose existing…) or custom (Create manually…) worktree;
- the Git repository source and New project…;
- account and agent choice with limits, and Run shell;
- OpenCode's model picker and effort, and its explicit service start;
- the local Windows-session warning (`LocalLaunchWarning.h`);
- the expected-machine guard: the plan carries the selected computer's identity, and every creation call checks it;
- opening or inspecting the dialog never creates a worktree, a folder, a clone or a session.

Not in this plan: the creation cards and their actions (D1); the foreground path that task executors use (D2); the Worktrees panel's standard mode and cleanup (`zerus-5wmh.10.5` and the worktree flows); Settings.

**Architecture:** The events stream's core is used as it is:
- N1 `launch.rs`: form rules, Start, the plan, the Windows warning, the generated branch, folder listings;
- N2 `repository.rs`: suggested name, name validity, destination;
- N3 `creation.rs`: admission, the journal, the creation steps;
- W1 `worktrees.rs`: catalogs, snapshots, manual creation, the machine guard;
- `paths.rs`: folder paths in their computer's form.

This plan adds four small, additive core fields and helpers (G1-G4). Calls the core lacks are questions for the events stream (Q1-Q5).
- **`client/src-tauri`:**
  - `newsession.rs` (new) exposes the dialog's commands;
  - `creations.rs` (new) hosts `creation::Creations` on its own worker and carries out its effects;
  - `screens.rs` gains the per-computer account read `launch_accounts`.
- **SolidJS:**
  - `store/newSession.ts` (new) holds the one open dialog and ports the dialog's members and slots;
  - `ui/sessions/newsession-ui.ts` and `ui/sessions/opencode-models.ts` (new) hold the view rules the core does not have;
  - `NewSessionDialog.tsx` and its child dialogs render them;
  - `store/creations.ts` (new) follows admitted creations for Qt's failure notice.

**Rules the core already has are called, never rewritten in TypeScript.** The form's derived state, Start, the plan, repository fields and the Windows warning go through commands. TypeScript holds only what Qt keeps in the dialog itself: account ranking and labels, model lists, row building and the request bookkeeping.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2; `tauri-plugin-dialog` 2 for the local folder picker, D3), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Base:** branch `work-1a04708c-newsession`, cut from `origin/integrate/client-core-2` at `0eaeb07f`. That integration (PR #424) carries N1-N3 and W1 (`f64925cc`, `a149081c`, `b86f7914`, `875cc6b1`, `91af1c07`). Rebase onto main once #424 lands, and onto the session-menu parity pieces as they land, since they touch `SessionList.tsx`, `ActionDialogs.tsx` and `store/actions.ts`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`:
- Owner constraints: Parity, Usage limits (round 5: New session uses the compact meters), Machines (round 6: the OS tile in the New session computer chooser), Text separators (round 8), Hint text (round 6);
- "Actions".

Also `docs/superpowers/specs/2026-10-26-session-creation-progress-design.md` ("Admission and immediate feedback"). Pattern: `docs/superpowers/plans/2026-10-28-tauri-client-session-menu-parity.md`. Visual base: `design/webui/src/dialogs/setup/NewSession.tsx`, `ModelPicker.tsx`, `Clone.tsx` (its folder browser) and `WorktreeDialogs.tsx`, where Qt does not contradict them.

**Behavioural reference (Qt).** Read these lines before each task; this plan quotes the rules it depends on.
- `tray/src/NewSessionDialog.cpp`, the whole file:
  - the constructor (136-475);
  - `setFleet` and `loadMachines` (478-510);
  - `setGroups`, `selectFolder`, `loadFolders`, `browseFolder` (512-588);
  - the repository source (590-658);
  - `updateForm` and `start` (660-786);
  - `restoreCreationChoices` (787-808);
  - Auto (809-842);
  - accounts and usage (857-1023);
  - paths and prefill (1025-1091);
  - worktrees (1092-1143);
  - OpenCode (1145-1313).
- `tray/src/NewSessionDialog.h`, `NewWorktreeDialog.h`, `LocalLaunchWarning.h`, `LocalSessionHost.h` and `.cpp`.
- `tray/src/WorktreePanel.h`, picker mode.
- `tray/src/DirectoryDialog.cpp` and `.h`.
- `tray/src/OpenCodeModelPicker.cpp` and `.h`, `OpenCodeModels.cpp` and `.h`, `OpenCodeEffort.h`.
- `tray/src/AccountChoice.h`, `AccountCatalog.h`, `AccountUsage.h` (`launchSummary` 130-142), `RepositoryClone.cpp`, `SessionTag.h`.
- `tray/src/SessionsWindow.cpp`:
  - `showNewSession` (2895-2958);
  - the entry points (295-297, 347-348, 377-378, 2572-2573, 1664-1665);
  - the creation wiring: placement 1539-1575, the failure notice 1577-1586, the launch 1588-1593;
  - `placeLaunchedSessions` (3020-3075);
  - `selectedHostTarget` (3128-3131);
  - the open dialog's fleet updates (2692).
- `tray/src/SessionsWindowShortcuts.cpp:222` and `tray/src/Shortcuts.cpp:58` (Ctrl+N).
- `tray/src/ProjectsDialog.cpp` (180, 501-541, 552-560).
- `tray/src/HgsClient.cpp`:
  - `requestAccounts` (607-620), `requestOpenCodeModels` (622-646), `requestOpenCodeFavorite` (648-668);
  - `launchDetachedSession` (1136-1149);
  - `requestWorktrees` (1515-1563), `createWorktree` (1565-1590);
  - `requestDirectories` (1790-1808), `createDirectory` (1810-1870).
- `tray/src/TrayAgent.cpp:669-688` (`launchSession`, the terminal launch).
- The Qt tests `tray/tests/test_newsessiondialog.cpp`: each task names the cases it ports.

## Global Constraints

Everything in the phase 2 plan's Global Constraints applies. In addition:
- **Qt wins.** Where this plan and Qt disagree, Qt is right; fix the plan in the same PR. Where the design prototype adds something Qt lacks, drop it. Examples:
  - the prototype's "Reading folder…" and "Ordinary folder (no Git)." hints on the Worktree row;
  - "OpenCode is not running on {machine}. Showing the last known models.";
  - the "Previous clone" switch;
  - a second Use folder footer under the worktree picker.
- **Add nothing Qt does not have.** Where the client lacks a Qt prerequisite, the plan says so (D1-D5, Q1-Q5) and leaves it to its owner.
- **Nothing is created on open or inspect.** Opening, browsing, choosing a project, computer, folder, account, agent, source or worktree mode, and reading catalogs run only reads:
  - `account ls`;
  - `worktrees --path P --json [--refresh]`;
  - `dirs PATH` in the folder browser;
  - `account models` once the model picker is opened.

  The only creating calls are the ones a click names:
  - `worktrees create` (Create worktree);
  - `dirs --create` (Create in the folder browser);
  - `account start` (Start OpenCode on …);
  - `account favorite` (a star);
  - the creation's steps after Start session.

  A generated worktree is created only by the admitted creation, and only for a catalog that verified a Git checkout (`launch::automatic_worktree_available`). Ordinary folders start directly; nothing runs `git init`.
- **Unknown outcomes are never replayed:**
  - One Start submits once: `submitted` is set before `creation_submit`, as `m_creationSubmitted` (745, 750), and cleared only by a refused admission (783-786).
  - Create worktree, Create folder and a favorite change each send one request identity and show their failure; nothing retries them. A lost folder creation is re-read, never re-sent (`DirectoryDialog.cpp:156-160`).
  - The creation journal (N3) owns everything after admission.
- **Confirmations:** the Windows warning is an alert dialog with Launch anyway and Cancel. Cancel is the default and has the focus, Escape is Cancel (`LocalLaunchWarning.h:25-28`). After Cancel nothing starts, and the form is kept.
- **Expected machine:** Start records the selected computer's identity (its machine card's `host`, else the alias) as the plan's `machine` (754). The creation passes it to every call as `--expect-machine` through `worktrees::run_guarded`. The manual Create worktree sends no expected machine, as Qt does (`NewWorktreeDialog.h:53`).
- **Late answers change nothing.** Each read carries its context: computer, folder, request number, and for OpenCode the newest request per folder. An answer for another context is dropped:
  - the folder check, accounts and the service start (344-345, 380-396);
  - folder catalogs (397-422);
  - favorites (423-453);
  - worktree catalogs and the prefill (454-465);
  - the folder browser (`DirectoryDialog.cpp:127-157`).
- **No dots as separators** (`scripts/check-ui-separators.py`). The Qt texts with " · " are listed in D4.
- **No obvious keyboard hints.** Shortcut names stay in tooltips only where the client already shows them.
- **Bounded data:**
  - account rows: at most 256 profiles per computer (`views::accounts`);
  - OpenCode:
    - short list 256;
    - catalog 10 000 models, read at most 5 MiB;
    - 64 favorites;
    - 500 rows shown (`OpenCodeModelPicker.cpp:20`);
    - catalogs kept for at most 4 folders (1286-1294);
  - folder listings: at most 8 MiB (`launch::DIRECTORIES_LIMIT`), and `hgs` lists 2,000 folders;
  - worktree catalogs: at most 2 MiB; 64 snapshots in 8 MiB;
  - the creation journal: 128 operations in 8 MiB; the creation-notice memory 128 entries (1583);
  - projects 500 with 200 folders each (`views/projects.rs`), and saved-project placement 512 (`launch::SAVED_PROJECTS`).
- **All `hgs` calls run off the UI thread**, as async commands.
- **Shared files:** the touch points with the client owner session (`claude/zerus-work-e94411a3/webui`) are listed under "Touch points". Change only those lines, additively; send webui the list before the PR (agreement of 2026-10-28 for archived Activity, same practice).
- **One milestone PR** (Beads `zerus-5wmh.10.10`):
  - after the local gates, rebased on main;
  - merged when the "CI gate" check is SUCCESS;
  - after the merge, message the owner session with the merge commit; it rebuilds the owner's Mac dev app.
- **Tests never run the real `hgs`.** Rust tests use `client/core/tests/support/fake_hgs_creation.py`; it answers by the first command word, which covers `account`, `worktrees`, `dirs`, `clone` and the agent launch. Store tests mock the bridge and record every command they receive.

## Review Focus

1. A full open, inspect, browse and Cancel run sends only reads: the fake `hgs` log has no `worktrees create`, `dirs --create`, `clone`, `account start`, `account favorite` or launch.
2. Start submits once. A refused admission shows the reason and keeps every choice. The dialog closes only after admission (777).
3. Late answers for another computer, folder or request never change the form: account rows, worktree catalogs, the prefill, OpenCode catalogs, favorites and the service start.
4. Account binding:
   - an explicit or preset account is never silently replaced (941-943);
   - a catalog still loading never selects Run shell for another agent (944-950);
   - a remembered account falls back to the provider's default only when it no longer exists.
5. The plan equals Qt's for the same choices (`launch::plan`): model only for OpenCode, effort only with a model, terminal only for a detached agent, destination only for Auto with a folder.
6. The Windows warning: Cancel focused and default; after Cancel, no call.
7. Every list stays within the bounds above.

---

## File Structure

```
client/core/src/view.rs, details.rs          G1: git_root and cwd_canonical (additive)
client/core/src/views/projects.rs            G2: ProjectsView.swarm_id (additive)
client/core/src/views/accounts.rs            G3: pub launchable, pub host_profiles (additive)
client/core/src/launch.rs (+ Cargo.toml)     G4: own_windows_session (cfg(windows); windows-sys Win32_System_RemoteDesktop)
client/src-tauri/src/newsession.rs           the dialog's commands (Task 1)
client/src-tauri/src/creations.rs            the creation host: worker, journal, effects, binding (Task 5)
client/src-tauri/src/screens.rs              launch_accounts on the hub (additive)
client/src-tauri/src/lib.rs                  registers the commands; manages NewSession and CreationHost; flushes on exit
client/src-tauri/Cargo.toml                  tauri-plugin-dialog = "2" (Rust side only, D3)
client/src/types.ts, bridge.ts               the new types and commands (additive)
client/src/store/newSession.ts (+test)       the open dialog: state, reads, tickets, openNewSession, openFolderSession
client/src/store/creations.ts (+test)        admitted creations and Qt's failure notice
client/src/ui/sessions/newsession-ui.ts (+test)  rows: computers, projects, folders, accounts; usage text; folderLaunchProblem
client/src/ui/sessions/opencode-models.ts (+test)  OpenCodeModels and the picker's list rules, OpenCodeEffort
client/src/ui/sessions/NewSessionDialog.tsx  the dialog and the Windows warning
client/src/ui/sessions/ModelPicker.tsx       OpenCode's grouped picker and its popover
client/src/ui/sessions/WorktreePicker.tsx    Choose an existing worktree (WorktreePanel, picker mode)
client/src/ui/sessions/NewWorktreeDialog.tsx Create manually
client/src/ui/components/FolderBrowser.tsx (+ folder-browser.ts, test)  DirectoryDialog for a peer
client/src/ui/styles/newsession.css          the dialog's styles (ported from design/webui setup styles)
client/src/App.tsx, ui/components/Rail.tsx, ui/sessions/SessionList.tsx, ui/screens/Projects.tsx,
client/src/ui/screens/reasons.ts, store/projects.ts, store/actions.ts, store/catalog.ts   touch points (below)
client/README.md                             "New session" and its smoke run (Task 6)
```

## The core surface per Qt step

| Qt step | Qt lines | Surface the view calls | State |
|---|---|---|---|
| Generated branch | 151 | `launch::generated_branch` through `new_session_context` | ready |
| Computers, their style and identity | 106-110, 487-510, 754 | machine cards (`machineCards`), `paths::of` | ready |
| Projects and their folders | 512-580 | `projectsView` (`hgs swarm get`), plus `swarm_id` | G2 |
| Folder catalog | 1081-1091, 454-465 | `worktrees::Catalogs`, `catalog_invocation`, `run_catalog` through `worktree_snapshot` and `worktree_catalog` | ready |
| Prefill from the selected session | 1045-1080, SessionsWindow 2924 | `launch::matching_project_folder`, `prefill_target`; the row's and details' `cwd_canonical` | G1 |
| Project worktree, Auto and its path | 809-838, 1092-1113 | `launch::project_worktree`, `automatic_worktree_available`, `automatic_worktree_path` inside `new_session_form` | ready |
| Form state | 660-742 | `launch::shown` inside `new_session_form` | ready |
| Start and the plan | 743-778 | `launch::start`, `launch::plan` | ready |
| Windows warning | 746-748, `LocalLaunchWarning.h` | `launch::other_windows_session`, `warning_text`, and this client's own session | G4 |
| Admission | SessionsWindow 2911-2919 | `creation::Creations::submit`, `admission_error` through `creation_submit` | ready (host: Task 5) |
| Creation steps and the launch | `SessionCreationController`, SessionsWindow 1588-1593 | `Creations` effects, `creation::run_call`, `launch::launch_invocation`, `worktrees::run_guarded` | ready; binding, placement and terminal launches: Q3-Q5 |
| Browse, this computer | `DirectoryDialog.cpp:37-45` | `pick_local_folder` | D3 |
| Browse, a peer | `DirectoryDialog.cpp:46-326` | `launch::directories_invocation`, `run_directories`, `directories`; New folder needs `dirs --create` | Q2 |
| Accounts of the computer | 865-952 | `reads::Read::Accounts` on the screens hub, `views::accounts::group` | G3 |
| Limits | 954-1023 | the hub's usage slots (the shared `AccountUsageStore`) | ready |
| OpenCode models, favorites, service | 1145-1313, HgsClient 607-668 | none yet | Q1 |
| Repository fields | 289-307, 590-639 | `repository::suggested_name`, `Clone::new`, `set_source`, `can_submit`, `destination` through `repository_fields` | ready |
| Choose existing… | 1114-1124, `WorktreePanel.h` | `worktree_catalog`; rows' `git_root` and `cwd_canonical` for the session counts | G1 |
| Create manually… | 1125-1143, `NewWorktreeDialog.h` | `worktrees::create_invocation`, `created`, `Catalogs::invalidate`, `launch::created_catalog` | ready |
| Restored choices | 787-808 | `launch::restored_folder` | ready (used by D1's Edit choices) |
| Remembered agent, account and terminal | 221-224, 263, 897, creation `Remember` | `launch::remember_account`, `creation::Effect::Write { remember }` | ready |

---

## Task 1: the dialog's Rust surface (`client/src-tauri/src/newsession.rs`)

**Files:**
- Create `client/src-tauri/src/newsession.rs`.
- Modify:
  - `client/src-tauri/src/lib.rs`: register the commands; manage `NewSession` with the worktree `Catalogs` loaded from `worktree-snapshots.json` in the app data directory, a separate file in replay mode, as `actions::marks_file`;
  - `client/src-tauri/src/screens.rs`: `launch_accounts`;
  - `client/src-tauri/Cargo.toml`: `tauri-plugin-dialog`;
  - `client/src/types.ts`, `client/src/bridge.ts`. In the browser build every command answers a failure, as the copy commands do.
- Core, additive, with tests: G1-G4.

**Interfaces:**

```rust
/// What the form passes; the command derives the rest with the core and answers both.
#[derive(Deserialize)] pub struct FormInput {
    pub form: launch::Form,          // without automatic_available, automatic_path, project_worktree (filled here)
    pub catalog: Value,              // m_worktreeCatalog
    pub verified: Value,             // m_verifiedWorktrees[host\npath], or null
    pub project: Option<launch::Project>,
    pub host: String,                // this computer "" or the alias, as Project folders name it
    pub catalog_pending: bool,
}
#[derive(Serialize)] pub struct FormView { pub shown: launch::Shown, pub automatic_available: bool, pub automatic_path: String, pub project_worktree: bool }

#[tauri::command] pub fn new_session_context(state: State<NewSession>) -> Context;      // { branch, own_windows_session, remembered: { agent, accounts, open_terminal } }
#[tauri::command] pub fn new_session_form(input: FormInput) -> FormView;
#[tauri::command] pub fn new_session_start(input: FormInput, submitted: bool) -> launch::Start;   // background = true
#[tauri::command] pub fn new_session_plan(choices: launch::Choices) -> launch::Plan;
#[tauri::command] pub fn launch_warning(local: MachineMeta, computer: String) -> Option<String>; // warning_text(other_windows_session(..)), own from G4
#[tauri::command] pub fn repository_fields(url: String, destination: String, style: Style, custom: bool) -> RepositoryFields;
#[tauri::command] pub fn worktree_snapshot(state: State<NewSession>, computer: String, path: String) -> Value;
#[tauri::command] pub async fn worktree_catalog(state: State<'_, NewSession>, computer: String, path: String, refresh: bool) -> Result<Value, String>;
#[tauri::command] pub async fn create_worktree(state: State<'_, NewSession>, computer: String, path: String, common_dir: String, branch: String, base: String, destination: String) -> Result<String, String>;
#[tauri::command] pub async fn list_directories(computer: String, path: String, hidden: bool) -> Result<Value, String>;
#[tauri::command] pub async fn create_directory(computer: String, path: String) -> Result<Value, String>;      // Q2
#[tauri::command] pub async fn pick_local_folder(app: AppHandle, start: String) -> Option<String>;          // D3
#[tauri::command] pub fn local_folder_exists(path: String) -> bool;                                         // ProjectsDialog.cpp:530
#[tauri::command] pub async fn opencode_models(computer: String, folder: String) -> Result<Value, String>;   // Q1
#[tauri::command] pub async fn opencode_favorite(computer: String, value: String, favorite: bool) -> Result<Value, String>; // Q1
#[tauri::command] pub async fn opencode_start(computer: String) -> Result<Value, String>;                  // Q1
// screens.rs
#[tauri::command] pub async fn launch_accounts(state: State<'_, Screens>, computer: String, refresh: bool) -> Result<LaunchAccounts, String>;
```

Rules:
- **`new_session_context`:**
  - `branch` is `launch::generated_branch` of a fresh UUID: `work-` and eight lower-case hex digits (151);
  - `remembered` reads `launch-choices.json`: Qt's `workspace/launchAgent`, `workspace/launchAccounts` (`{host: {agent: account}}`) and `workspace/launchOpenTerminal` (default false), under those names. Task 5 writes it.
- **`new_session_form`:**
  - fills `automatic_available` with `automatic_worktree_available(catalog, path, style, catalog_pending)`, and `automatic_path` with `automatic_worktree_path(catalog, branch, style)`;
  - fills `project_worktree` with `project_worktree(path, catalog, …) || project_worktree(path, verified, …)` (1110-1113);
  - answers `launch::shown`.

  `new_session_start` answers `launch::start(form, true, submitted)` on the same derived form. The view asks it again at the click, so a stale `Shown` never starts anything.
- **`repository_fields`** builds a fresh `repository::Clone::new(style, None, None)`. The client never saves a pending clone record (deviation 2).
  - A URL change without a custom destination suggests `{parent}/{suggested_name(url)}` in native form (289-295).
  - The destination splits into parent and name as `editRepositorySource` does (628-639). On Windows `\` reads as `/`; separator 0 gives the parent `/`, and a parent `X:` becomes `X:/`.
  - It answers `{url, parent, name, destination, suggested, can_submit}`.
- **`worktree_catalog`** ports `requestWorktrees` (1515-1563) over W1:
  - a fresh snapshot (under 30 s, not stale) answers without a call unless `refresh`;
  - reads of the same computer and path in flight share one call;
  - the answer passes `Catalogs::accept` with the stamp taken before the call;
  - after each accepted answer the snapshots are saved off the UI thread;
  - a failure answers the error; the store then marks its catalog stale (462-464).

  `worktree_snapshot` answers the saved snapshot, or `{}` (1087).
- **`create_worktree`** ports `NewWorktreeDialog` with `createWorktree` (1565-1590). It calls `create_invocation(computer, path, common_dir, branch, base, destination, fresh UUID, false, "")` (330 s), checks the answer with `created(exit, id, branch, common_dir, false, destination)` and answers the new path. Qt's texts come from the core:
  - "Creation could not be confirmed. Refresh the catalog and check the destination before retrying.";
  - on failure, the error, then "\nCheck {destination} before retrying. Any created worktree is kept.".

  The repository's catalogs are invalidated on both outcomes.
- **`list_directories`** is `directories_invocation`, `run_directories` and `directories`: 5 s here, 15 s on a peer; "Invalid directory response".
- **`launch_accounts`** reads one computer, as the dialog does (865-874, 956-963):
  - It runs that computer's `account ls` on the hub (25 s), unless the hub has it and `refresh` is false. A read already running is shared.
  - It ensures usage for the computer's installed profiles; `refresh` forces it.
  - It answers `LaunchAccounts {loaded, error, profiles, groups}`:
    - `profiles`: the raw rows, at most 256, for the OpenCode fields (`model_options`, `model_options_truncated`, `catalog_state`, `catalog_reason`, `installed`, `install_reason`);
    - `groups`: `views::accounts::group` over that computer's launchable profiles only (883-896); each member carries its `Profile` projection with the cached usage (971-978), and each group the best usage of its members (980-988).
  - "Invalid account catalog response" for a bad reply.
- **The OpenCode commands** (Q1) are Qt's calls:
  - `account models native-opencode --location F --json`: 30 s; over 5 MiB gives "The OpenCode model catalog is too large", a bad reply "Invalid OpenCode model catalog response";
  - `account favorite native-opencode --model V --favorite on|off --request-id ID --json`: 25 s, one fresh identity per call; the receipt must be `applied` with the same `request_id`, `model` and `favorite`, otherwise "Invalid favorite response";
  - `account start native-opencode --json`: 60 s.
- **`pick_local_folder`** opens the system folder dialog titled "Choose a folder" at `start`, or the home folder for `~` or empty (`DirectoryDialog.cpp:40-45`). It runs from Rust, so no JavaScript permission is added.
- **`launch_warning`** answers `warning_text(session)` when `other_windows_session(local, computer != "", own_windows_session())` names a session, else None.

- [ ] **Step 1: Write the failing tests:**
  - **G1-G4** in their core files;
  - **`newsession.rs` units:**
    - the Windows warning: a running host of session 2 with own session 1 asks; same session, a remote launch, a stopped host, no own session and a non-Windows computer do not (`launchIntoAnotherWindowsSessionAsksFirst`, `launchWithoutAnotherWindowsSessionDoesNotAsk_data`);
    - `repository_fields`: URL to destination, custom destination kept, Windows parents (`repositorySourceKeepsCommonChoicesAndManualDestination`);
    - `new_session_form` derives Auto and the project worktree from the catalog or the verified one;
  - **integration through `fake_hgs_creation.py`:**
    - a fresh snapshot answers without a call; `refresh` calls; two reads share one call; a generation change gives "Folder changed. Refresh its catalog.";
    - `create_worktree`'s confirmed, mismatched and failed answers with their texts;
    - `list_directories` with an invalid reply;
    - `launch_accounts`:
      - the local Windows computer hides `dsh` and `opencode` (`localWindowsHidesDeepSeekAndOpenCode`);
      - grouping matches the Accounts page for each computer (`chooserAndAccountsPageShowSameAccountsForEachComputer`, `accountAliasesMatchAccountsGroupingAndKeepExactBinding`);
      - an empty catalog gives no groups (`emptyComputerCatalogOffersOnlyShell`);
    - the OpenCode receipts (`openCodeStarSendsOneFavoriteRequestWithIdentity`) and the start call (`openCodeStartsStoppedServiceExplicitly`).
- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core` and `cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml newsession::`. Expected: FAIL.
- [ ] **Step 3: Implement.** Commands that wait on Q1 or Q2 are added with their signatures and answer "Arrives with …" failures until the core calls exist. The view disables their controls with the same text.
- [ ] **Step 4: Run** the tests and clippy (`-D warnings`) for both packages. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): New session's commands over the launch, worktree and account cores`.

## Task 2: the dialog store and its rules

**Files:**
- Create `client/src/store/newSession.ts` (+test), `client/src/ui/sessions/newsession-ui.ts` (+test), `client/src/ui/sessions/opencode-models.ts` (+test).
- Touch points: `client/src/store/projects.ts` (`requestedProject`, `requestProject`), `client/src/store/actions.ts` (export `notify`), `client/src/store/catalog.ts` (`gitRoot`, `cwdCanonical`).

**Interfaces:**

```ts
export interface NewSessionOptions { agent?: string; project?: string; computer?: string; folder?: string; path?: string; account?: string | null; recipe?: Record<string, unknown> }
export function openNewSession(options?: NewSessionOptions): void;        // SessionsWindow::showNewSession (2895-2928)
export function openFolderSession(project: string, folder: Folder): void; // ProjectsDialog::newFolderSession (533-541)
export function selectedHostTarget(): string;                             // SessionsWindow::selectedHostTarget (3128-3131)
export const newSession: () => NewSessionState | undefined;               // one dialog at a time
export function closeNewSession(): void;                                  // reject (476)
export function startNewSession(confirm: (text: string) => Promise<boolean>): Promise<void>;
```

The state mirrors `NewSessionDialog.h:98-175`; drop only the members of the foreground path (D2).

Rules (each with its Qt lines; port the logic, not the widget code):
- **Opening** (`showNewSession`, 2895-2928):
  - The project is the current row's project, else the selected session's, else the default project. A `project` option matching an id or name wins, and only it makes the project explicit (2921).
  - The selected session is context only for a generic open (no project, folder, path or account option) on a session row (2902-2906). It sets the computer and prefills its folder:
    - the folder is the details' `cwd`, else the row's;
    - the canonical form is the details' `cwd_canonical`, else the row's.
  - Then, in order: `folder` (`selectFolder`; failure: the sticky notice "This project folder changed. Select it again.", and the dialog does not open), `path` (`selectPath(path, false)`), `account` (`selectAccount`), `recipe` (`restoreCreationChoices`, 787-808, with `launch::restored_folder`).
  - The swarm is the projects view's `swarm_id` (2922, G2).
- **Initial choices:**
  - the agents are codex, claude, kimi, dsh, opencode, plus an unknown requested agent (221-222);
  - the agent is the option's, else the remembered one when listed or `sh`, else `codex` (223-224);
  - the remembered account is per computer and agent (897, 922-923); Open terminal is the remembered value (263);
  - the name is the generated branch (260).
- **Computers** (`loadMachines`, 487-510):
  - this computer when it runs local sessions, then the peers, the previous choice, and the project's folders' computers (local only with local sessions; a folder with a machine id only when its computer is configured);
  - the label is the computer, plus " (offline)" when its card is not ok;
  - fleet updates relabel in place and keep the choice (503-509, 2692).
- **Projects** (`setGroups`, 512-521):
  - accessible projects with their colours, plus "New project…" for the repository source;
  - the choice order: keep New project…, the selected one, the initial name, the default project.
- **Folders** (`loadFolders`, 536-580):
  - the project's folders on the computer, then a custom path (the selected one, the previous one in the same project and computer, or the folder last browsed on this computer) when it is not listed;
  - each row carries its computer's availability;
  - a change of rows or index clears the error line (569);
  - `selectFolder` (522-535), `selectPath` (1025-1033), Browse (581-588: the answer applies only if computer and project are unchanged).
- **Prefill** (1045-1080): select `launch::matching_project_folder(path, canonical)`, then read `worktree_catalog(refresh)` of the session's folder. On the answer, with an unchanged context, `launch::prefill_target` moves the choice to the main checkout's project folder and keeps the catalog as that folder's verified one. While it runs the preview is "Checking the session's main working folder…" and Start is disabled.
- **Worktree catalog** (1081-1091):
  - the context is computer, path and online;
  - the snapshot first, then a read when the folder is set and online;
  - Choose existing… is shown when the catalog lists worktrees, and Create manually… when its `state` is `ok` (459-460, 1088).
- **Repository source** (590-639):
  - `setRepositorySource` keeps New project… until the user picks a project in folder mode (602-604);
  - a new computer or style starts fresh fields with the URL and name kept (608-627);
  - the URL field takes the focus (606);
  - the new project's name follows the folder name until edited (618, 637);
  - the project name is at most 80 characters (158).
- **Accounts** (865-952), in `newsession-ui.ts` as `accountRows(groups, choice)` returning rows and the selected index or none:
  - ranks inside a group: preset (5), current (4), remembered (3), default (2), native (1) (901-911);
  - the selection: the preset one only; else the current one, the remembered one, the agent's default, the first (941-943);
  - none while the computer's catalog loads with no profiles and the agent is not `sh` (944-950);
  - the last row is "Run shell", or "Run PowerShell" on a Windows computer, with "Interactive terminal session" (936-940).

  Rows are grouped once per arriving catalog; later limits change rows in place (883, 990-1000). Opening and Refresh ask for limits (`requestUsage`; Refresh forces them and re-asks folders whose OpenCode answer was not ready, 317-322). A 60 s timer and the hub's usage changes refresh rows in place (340-342).
- **Row texts** (1002-1023):
  - the name is the usage identity's email, else the cached one, else the label; empty, "Default account" and "Native account" read "Account n/a";
  - the summary is `AccountUsage::launchSummary` (`AccountUsage.h:130-142`): each window "{period} {n}% used", joined by two spaces; balances "Balance: {amount} {currency}"; otherwise "limits n/a";
  - the tone is the severest window's unless offline or a refresh error;
  - the accessible text is "{provider} {name}\n{summary}".
- **OpenCode** (`opencode-models.ts` with the shared fixtures `tests/fixtures/opencode-provider-catalog.json` and `opencode-effort-ids.json`):
  - `OpenCodeModels`: `parse`, `providers`, `group`, `filter`, `marks` (D4), `catalogSupported`, `updateText`, `errorText` (`OpenCodeModels.cpp`);
  - `OpenCodeEffort.valid`;
  - the dialog's `addModelOptions` (51-71) and the favorite marking (74-105);
  - `updateModels` (1148-1207) and `updateEfforts` (1209-1219): a changed model keeps the effort only when it offers it;
  - `openCodeProfile` (1220-1228) and the service row (1229-1254);
  - folder catalogs: at most 4, the current one kept; only the newest request per folder answers; a failed refresh keeps a ready catalog (397-422, 1277-1294);
  - favorites: one request per star and late answers ignored; after a confirmed change, the computer's other folders' catalogs are dropped (423-453, 1306-1313);
  - the picker's All models ask (328-337);
  - "Use native default model" (338).
- **The form:** the store assembles `FormInput` from its state and keeps the latest `FormView` for the latest revision.
  - The error line takes `shown.notice` when there is one, which drops the error shown before (core `Shown.notice`, 719-731).
  - Otherwise it shows the last transient error: a refused admission, a favorite failure, "Worktree created at %1 on %2. Select that folder to start a session.", and the account status texts.
- **Start** (`startNewSession`, 743-786):
  1. Ask `new_session_start(input, submitted)`; `nothing` ends here.
  2. With `launch_warning(...)` text, await `confirm(text)`; Cancel ends here and changes nothing.
  3. Ask `new_session_start` again (748).
  4. Set `submitted`, then `creation_submit(new_session_plan(choices))`.
  5. Ok closes the dialog (777) and hands the id to `store/creations.ts`. An error clears `submitted` and shows the error (783-786).

  The choices are `launch::Choices` from the state (751-771):
  - the computer's identity as `machine`;
  - the swarm;
  - the project's name as `project_name` (765);
  - `listed_folder` when the folder row has an id.
- **Closing:** Cancel, Escape, Manage this project's folders and Open Machines close. With no foreground work, closing is always allowed.
  - Manage this project's folders closes, then opens Projects on that project (281, 2955);
  - Open Machines closes, then opens Machines on this computer (282, 2956).
- **`folderLaunchProblem`** (`ProjectsDialog.cpp:523-532`), for the Projects entries:
  - "Select a folder for the new session.";
  - "Waiting for this computer's identity.";
  - "Reconnect %1 to start a session in this folder.";
  - "This folder is unavailable on this computer." (`local_folder_exists` here; an absolute path in the computer's style elsewhere).

  `openFolderSession` checks the folder is still the project's, with "This project folder changed. Select it again." (533-541).

- [ ] **Step 1: Write the failing vitest tests** with a mocked bridge that records each command. Port these Qt cases:
  - accounts: `focusWhileCatalogLoadsKeepsRememberedAgent`, `unavailableRememberedAgentSelectsFirstAgent`, `rememberedAndExplicitMissingAccounts`, `refreshKeepsSelectionAndLastReportedLimits`, `lateUsageCannotRetargetAnotherComputer`, `selectedComputerCatalogRejectsLateOtherComputerRows`, `pendingAccountsAllowShellOnlyWhenChosen`, `pairsAndLimitsLaunchExactProfile`, `openingRefreshesLimitsInPlace`, `unknownAndEndedWindowsStayUnavailable`;
  - repository: `repositorySourceKeepsNewProjectIntent`, `repositorySourceKeepsOpaqueProjectId`, `repositorySourceIgnoresLateFolderPrefill`, `sourceSwitchKeepsControls`;
  - placement: `untouchedInboxUsesNativeFolderInferenceAndExplicitInboxWins` (the plan's `implicit_inbox` and `swarm`);
  - OpenCode: `openCodeEffortFollowsSelectedModel`, `openCodeEffortIdsFollowSharedRule`, `openCodeStartNeedsOnlineComputer`, `openCodePickerUsesTheSelectedFolderCatalog`, `openCodeChoicesOutsideTheRowSurviveAnAccountRefresh`, `openCodeStoppedServiceKeepsTheRowListAndNeverStarts`, `openCodeOlderComputerOffersNoStarsOrAllModels`, `openCodeLateFolderCatalogStaysWithItsFolder`, `openCodeLateFavoriteAnswerStaysWithItsComputer`, `openCodeCurrentModelSurvivesItsFavoriteChanges`, `openCodeOlderFolderAnswerNeverReplacesANewerOne`, `openCodeFolderChangeKeepsOnlyAChoiceTheNewFolderOffers`;
  - the Windows warning: Cancel sends nothing (`launchIntoAnotherWindowsSessionAsksFirst`).

  Add:
  - opening, choosing and Cancel send only reads (Review Focus 1);
  - two Start clicks submit once;
  - a refused admission keeps the choices;
  - the prefill's late answer after a folder change does nothing;
  - `folderLaunchProblem`'s four texts;
  - `openNewSession`'s project and context rules.
- [ ] **Step 2: Run** `npx vitest run src/store/newSession.test.ts src/ui/sessions` in `client`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): New session's store and rules`.

## Task 3: the dialog and its entry points

**Files:**
- Create `client/src/ui/sessions/NewSessionDialog.tsx`, `client/src/ui/styles/newsession.css`.
- Touch points: `client/src/App.tsx`, `client/src/ui/components/Rail.tsx`, `client/src/ui/sessions/SessionList.tsx`, `client/src/ui/screens/Projects.tsx`, `client/src/ui/screens/reasons.ts`.

**Layout** (142-266): a modal `dialog` titled "New session", 660 px wide, at least 520, with the heading "New session". The form rows in Qt's order:

| Row label | Content | Qt lines |
|---|---|---|
| Target | project choice (accessible "Project", project colour), the manage button (tooltip and accessible "Manage this project's folders"), computer choice (accessible "Computer", with the OS tile and state dot, spec round 6) | 146-157 |
| New project | only for New project… with the repository source: "Project name" placeholder, at most 80 | 158-160, 667-668 |
| Source | "Working folder" and "Git repository" toggles | 161-165 |
| Working folder, or Repository | folder: the choice (accessible "Working folder", placeholder "Choose a folder…") and "Browse…"; repository: the URL field (placeholder `https://github.com/owner/repository.git`, tooltip "An SSH or HTTPS repository URL, on GitHub or another Git server.", at most 4096) | 166-174, 188, 670 |
| (none), or Clone to | folder: the path in native form (selectable) and the two-line preview; repository: the destination (tooltip "The complete new checkout path. Edit its folder name or choose a parent folder with Browse.", at most 8192) and "Browse…" | 175-197, 670-685 |
| Worktree | "Auto" (tooltip "Create a generated branch and worktree when you start the session."), "None" (tooltip "Start directly in the working folder."), the "Worktree options" menu ("Choose existing…", "Create manually…"), and the target path line (tooltip "New branch: {branch}\nStart from: HEAD") | 198-216, 700-705 |
| Account and agent | the account list (accessible "Account and agent"; rows: provider logo, bold name, compact meters or the summary in its tone), the status text and "Refresh" (accessible "Refresh accounts") | 225-234 |
| Model | OpenCode only: the model button ("Native default model", or the model's label; tooltip "Models OpenCode offers on this computer. Native default uses OpenCode's configuration.") opening the picker popover | 235-247, 1256-1265 |
| (none) | OpenCode service: the reason from `hgs` and "Start OpenCode on {computer}" or "Starting OpenCode…" (tooltip "Start the shared OpenCode service on this computer. Zerus never stops it.", or "{computer} is offline.") | 248-255, 1229-1246 |
| Effort | OpenCode with a model that has variants: "Default" and the variants (tooltip "Variants of the selected model. Default uses the model's own setting.") | 256-259 |
| Session name | the name, then its problem line (`SessionTag::problem`: "Enter a session name.", "Remove spaces at the beginning or end.", "Names cannot contain /, : or . or control characters.") | 260-261, `SessionTag.h:12-15` |
| (none) | "Open terminal window" (tooltips "DeepSeek opens in the workspace's Native UI.", "OpenCode runs through its native server and opens in Activity.", "Also open an external terminal for this session.") | 262-263, 717-718 |

Below the form:
- the error line;
- the buttons: "Open Machines" (only without a computer), Cancel, then the primary Start button. Its text is `shown.start_text`; here "Start session" or "Clone and start" (D2 lists the others).

Rows keep their height while texts change (171-186, 685, 705).

**Texts.** Every other text the dialog shows, verbatim:
- Previews (680-683):
  - "Checking the session's main working folder…";
  - "Browse to choose a folder on this computer.";
  - "A matching project is detected automatically.\nOtherwise this session stays in Inbox.";
  - "This folder is outside the project.\nStarting here will add it to this project.", in the warning colour (678-679).
- Standing notices on the error line (719-730):
  - "This computer is unavailable. Choose another folder or reconnect it.";
  - "Restore or add an account in Agent accounts to start this agent.";
  - "Loading this session's account…";
  - "This session's account is unavailable. Choose another account.";
  - "Add a remote machine in Machines to start sessions from this computer.";
  - "Enable local sessions for this computer in Machines, or add a remote machine.".
- Account status:
  - "Loading accounts…" (872);
  - "Could not load accounts: {error}" (395);
  - "Could not start OpenCode: {error}" (391).
- Account rows: "Run shell" or "Run PowerShell", "Interactive terminal session" (937-938), "Account n/a" (1012), "limits n/a" (`AccountUsage.h:141`).
- OpenCode:
  - "Use native default model" (246);
  - "Choose a working folder to list all of its models." (333);
  - "{computer} is offline." (334, 1201);
  - "Could not change the favorite on {computer}: {error}" (452);
  - "Update Zerus on {computer} to search all models and star favorites." (`OpenCodeModels.cpp:148`).
- Computers: " (offline)" (501). Projects: "New project…" (518, 597). Admission: the core's texts, shown on the error line (785).
- The Windows warning (`LocalLaunchWarning.h:18-28`):
  - title "Launch in another Windows session?";
  - text "The session host on this computer runs in another Windows session ({n}), for example one started over SSH. Agents it starts cannot show windows on this desktop, such as a browser sign-in. Launch anyway?";
  - buttons "Launch anyway" and Cancel, with Cancel the default and focused.

**Enablement**, all from the store's `FormView` and state:
- `fields_enabled` covers project, computer, folder, account, model, effort, name, the manage button, the worktree options and the source toggles (698-700);
- Browse follows `browse_enabled`, Auto `auto_enabled`, None `none_enabled`;
- the Worktree options button, Choose existing… and Create manually… follow their flags (714-716);
- Refresh is enabled when idle, with a computer and no account read (699);
- the repository fields follow `fields_enabled`, unlocked fields and an online computer (707-708);
- the model button needs OpenCode with its row (1262);
- Start OpenCode needs an online computer, not starting and no account read (1243);
- Open terminal follows `open_terminal_enabled`, Cancel `cancel_enabled`, Start `start_enabled`.

**Entry points**, each Qt's call:

| Client entry | Qt | Call |
|---|---|---|
| Rail New (`Rail.tsx:32`, today a toast) | rail button 295-297 | `openNewSession({ computer: selectedHostTarget() })` |
| Sessions header add button (`SessionList.tsx:398`, disabled today) | panel button 377-378 | same |
| Ctrl or Cmd+N (`App.tsx` keys) | `Shortcuts.cpp:58`, `SessionsWindowShortcuts.cpp:222` | same |
| Project header menu "New session…" (`SessionList.tsx:141`, `LATER` today) | `showGroupMenu` 2572-2573 | `openNewSession({ project, computer: selectedHostTarget() })` |
| Projects folder menu "New session…" (`Projects.tsx:47`) | `ProjectsDialog.cpp:558-560` | `openFolderSession(project.id, folder)`, disabled with `folderLaunchProblem` as hint, otherwise the folder's native path as hint |
| Projects Folders "New session…" button (`Projects.tsx:163`) | `ProjectsDialog.cpp:180, 520-521, 222` | same; its tooltip is "New session in {path} on {machine}" or the problem |

`SessionDetail.tsx` has no New session entry: Qt's detail pane has none, and its panel button is the list header's. The footer's "Session creation status" (`SessionList.tsx:440`) stays disabled until D1. `reasons.ts` loses `NEW_SESSION` once nothing uses it.

- [ ] **Step 1: Write the failing tests:**
  - the entry points' option objects (with a mocked `openNewSession`);
  - the Projects entries' enablement and hints;
  - the warning's focus and default (a DOM test of `NewSessionDialog`'s confirm: Cancel has `data-initial` and `autofocus`; Escape answers false).
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement** the dialog from the prototype's layout and classes, with Qt's rows, texts and rules. The account list keeps arrow-key navigation, and focusing it never selects a row (947-949).
- [ ] **Step 4: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): the New session dialog and its entry points`.

## Task 4: the child dialogs

**Files:** Create `client/src/ui/components/FolderBrowser.tsx` with `folder-browser.ts` (+test), `client/src/ui/sessions/WorktreePicker.tsx`, `client/src/ui/sessions/NewWorktreeDialog.tsx`, `client/src/ui/sessions/ModelPicker.tsx` (+test of its list state).

Each child dialog opens over New session in the same modal layer. Its answer applies only while the computer and project are what they were when it opened (585, 1123, 1134).

- **Browse…** (`browseFolder` 581-588, `DirectoryDialog`):
  - this computer: `pick_local_folder`;
  - a peer: `FolderBrowser`, a port of the peer branch of `DirectoryDialog.cpp`:
    - title "Choose folder — {computer}"; heading "Choose a folder"; "Folders on {computer}";
    - "Up", "Home", the path field (accessible "Directory path") and "Go";
    - the filter (placeholder "Filter folders in this directory…"); the list (accessible "Directories"); "Show hidden folders";
    - "New folder" and "Dialog…" (accessible and tooltip "New folder dialog…");
    - "Use this folder", or "Use selected folder" with a selection (106);
    - status texts:
      - "Loading folders…" (190);
      - "The folder is available. Press Use this folder to select it.", "Folder created. Press Use this folder to select it.", "Showing the first 2,000 folders. You can also enter an exact path above.", "Double-click to open a folder, or select it and press Use selected folder.", "No subfolders. You can use this folder." (138-142);
      - "Press Go to open this path, or New folder to create it." (111);
    - New folder inline (placeholder "Folder name", accessible "New folder name", "Create", Cancel with accessible "Cancel new folder", 222-228);
    - New folder dialog: "New folder — {computer}", "Create a folder in {parent} on {computer}.", "Folder name", "Create folder" (297-306); "Creating folder on {computer}…" (324);
    - folder names: not empty, not `.` or `..`, no `/`, no control character, the whole path at most 4096 bytes (25-30);
    - closing is refused while a folder is being created (195-198);
    - a failed creation re-reads the typed path and shows the error, never re-sending (156-160);
    - New folder waits for Q2; until then it is disabled with "Arrives with folder creation.".

  `FolderBrowser` lives in `ui/components` because Clone repository and Projects Add folder use the same dialog later.
- **Choose existing…** (`chooseWorktree` 1114-1124): a dialog "Choose an existing worktree" with `WorktreePicker`, a port of `WorktreePanel` in picker mode:
  - "Folder and worktrees" and "Refresh";
  - the status:
    - "Select a folder.", "Offline. Showing the last snapshot.", "Catalog unavailable: {error}", "Reading folder…", "Ordinary folder (no Git).";
    - "Folder is missing or inaccessible.", "Git information unavailable. You can still use an accessible folder.", "Partial catalog (first 512 worktrees).", "Existing working folders on {computer}" (122-129);
    - plus "\nSnapshot: {d MMM HH:mm:ss}" and " (outdated)" (130);
  - row labels:
    - "Main checkout", "Linked worktree", "Bare repository", "Folder";
    - " / {branch}" or " / detached {head}";
    - " (locked)", " (prunable)", " (unavailable)";
    - " (1 session)" or " (N sessions)", counted from listed rows whose `git_root` (checkouts) or `cwd_canonical` (folders) is the row's path (135-150, G1);
  - "Use folder" (enabled for an available, non-bare, current row, 117) and Cancel.

  No All sessions, Clean up or context menu (picker mode). Using a folder sets None and selects it (1123).
- **Create manually…** (`createWorktree` 1125-1143, `NewWorktreeDialog.h`):
  - title and heading "New worktree"; "Computer: {computer}\nRepository: {source}";
  - "New branch": the session name, selected and focused; placeholder "feature/my-task";
  - "Start from": editable, HEAD first, then the catalog's branches; tooltip "Local branch, tag, commit or an already fetched remote branch (for example origin/main).";
  - "New folder": the suggestion `{parent of selected_root}/{repository}-{branch with runs outside letters, digits, ._- as -}` until edited; "Parent folder…" opens Browse for the parent;
  - "Creates a new branch and working folder. It remains on this computer if you cancel or cannot start the session.";
  - the error line, "Create worktree", Cancel; "Creating worktree…" while it runs; every control is disabled and closing is refused meanwhile (52-72);
  - Create worktree is enabled with a branch, a base and an absolute destination in the computer's style (deviation 3).

  On success:
  - if the computer or project changed, the error line reads "Worktree created at {path} on {computer}. Select that folder to start a session." (1135);
  - otherwise the new checkout joins the catalog as verified (`launch::created_catalog`); None is set and the folder is selected (1139-1142).
- **The model popover** (`openModelPicker` 1266-1274):
  - at least 440 × 420 under the model button;
  - opening asks the folder's catalog once (`requestFolderCatalog(false)`);
  - `ModelPicker` ports `OpenCodeModelPicker`:
    - search: placeholder and accessible "Search models", at most 200 characters, 150 ms debounce;
    - grouped list (accessible "Models");
    - "All models" (tooltip "Search every model OpenCode offers", or the reason);
    - "Unavailable favorites";
    - row marks (D4) and "Unavailable";
    - "Add to favorites" or "Remove from favorites";
    - "OpenCode does not offer {model} here. Its star removes the favorite.";
    - notices: "Loading all models…", "Showing the first 500 matches. Search to narrow the list.", "Showing the first {n} models. Search to narrow the list.", "Some models are not in this short list. Use All models to search every model." (252-283, 298-303);
    - a context change resets it first (`OpenCodeModelPicker.h:13-20`);
  - below it, "Use native default model".

- [ ] **Step 1: Write the failing tests:**
  - `folder-browser.ts`: navigation, status order, a late listing after navigation, the name rule, a failed creation that re-reads;
  - the picker list: grouping, search, budget 500, unavailable favorites, notices in Qt's order (`openCodeAllModelsSearchesTheFolderCatalog`);
  - the new worktree suggestion and enablement;
  - the worktree picker's labels and Use folder enablement.
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement.** **Step 4: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): folder browser, worktree choices and model picker for New session`.

## Task 5: the creation host and admission (`client/src-tauri/src/creations.rs`)

**Files:**
- Create `client/src-tauri/src/creations.rs`, `client/src/store/creations.ts` (+test).
- Modify `client/src-tauri/src/lib.rs` (manage, the catalog hook, `quit` and a bounded flush on exit, as the read marks), `client/src/types.ts`, `client/src/bridge.ts`.

**Interfaces:**

```rust
#[tauri::command] pub async fn creation_submit(state: State<'_, CreationHost>, plan: Value) -> Result<String, String>; // Ok(id) admitted; Err(text)
#[tauri::command] pub fn creations_view(state: State<CreationHost>) -> Vec<CreationSummary>;                       // unsettled, newest first, at most 128
// event "creations": { id, tag, state, caption, error, show_pending } per Effect::Changed / Removed
```

Rules:
- **One worker owns `Creations::new(clock, uuid)`.** At start it loads the journal: `creations.json` in the app data directory (a separate file in replay mode), as `Journal::Missing`, `Unreadable` or `Bytes`. Effects are carried out in order:
  - **`Write`:** an ordered writer thread writes the bytes atomically (temporary file, sync, rename) and answers `written(write, ok)`. After a good write, a `remember` saves the agent, `remember_account` for the computer and agent, and Open terminal to `launch-choices.json` (`creation::Remember`).
  - **`Call`:** a pool thread runs `creation::run_call(hgs, &call)`, then the worker answers `answered(flight, answer, &mut placement)`.
  - **`Launch`:**
    - without `terminal`: `launch_invocation(computer, &Launch {…}, expected_machine)` through `worktrees::run_guarded` (90 s, the `guarded-create-v1` probe), then `launched`, then `launch_finished(launch_id, result)`;
    - with `terminal`: Q5 (`TrayAgent::launchSession` 669-688). A failure answers `launch_finished(id, Err(error))`, as `newSessionLaunchFailed` does (3004).
  - **`Changed` and `Removed`:** emit the `creations` event.
- **Admission** (2911-2913): `creation_submit` answers `Ok(id)` from `Creations::submit(plan)`. An error answers its text, which the store puts on the dialog's error line: `UNSAFE_URL`, `TOO_MANY`, `ADMISSION_FULL`, or `admission_error()`. The dialog never retries.
- **Placement** (`creation::Placement`, Qt 1539-1575): `creation::place` decides. Creating the project by its reserved ID and adding the cloned folder are organization edits (Q4). An edit whose outcome is unknown is an error, and the operation fails with it.
- **Binding** (`placeLaunchedSessions` 3020-3070):
  - On each catalog batch, an operation in `launching` or `confirming` whose `launch_id` appears on an online, live row of its computer with the same agent and tag is completed (`complete`).
  - Otherwise, the computer's `ended_launches` for that launch, with no row for it, ends it with "Session {tag} ended before it appeared (exit code {code})." or "Session {tag} ended before it appeared.", a line break and at most 4000 characters of its output (`ended`).
  - Both need Q3. Placing the bound session in its project (3043-3050) needs Q4.
- **Notices** (1577-1586): `store/creations.ts` shows the sticky error notice "Creation {tag}: {caption}\n{error}" once per state and error, for `failed`, `unknown` and `storage_error`. The memory keeps 128 entries.
- **Exit:** `quit()`, then wait at most 2 s for the writer, as the read marks do.

- [ ] **Step 1: Write the failing tests** (src-tauri integration with `fake_hgs_creation.py`):
  - an ordinary folder with None: `dirs --creation`, then the launch with `--expect-machine`, and no `worktrees create`;
  - Auto: the verified catalog, `worktrees create`, its receipt, then the launch in the new checkout;
  - the repository source: the access check, then `clone`;
  - a failed journal write: "Could not save creation progress. No further setup step was sent." and no call;
  - a restart with an interrupted operation: `recover`, and no call until Continue setup;
  - a launch exiting 0 stays `confirming`; a row with its launch id binds it;
  - the admission limits' texts;
  - the store's notice deduplication.
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement** what Q3-Q5 do not block. Blocked parts keep Qt's behaviour of waiting: a creation that cannot bind stays `confirming`, and nothing is replayed.
- [ ] **Step 4: Run** src-tauri tests, clippy, `npx vitest run`, `npm run check`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): background session creation host and New session admission`.

## Task 6: README and smoke steps

**Files:** `client/README.md` ("New session"), `client/src/ui/PerfHost.tsx` (the `newsession-smoke` hook, beside `actions-smoke`).

- [ ] **Step 1: README:**
  - the commands and their `hgs` calls;
  - what opening reads and what only a click creates;
  - where state lives: `worktree-snapshots.json`, `creations.json`, `launch-choices.json`;
  - the bounds;
  - the gaps still open (D1-D5, Q1-Q5).
- [ ] **Step 2: Smoke hook.** `ZERUS_CLIENT_QUERY=newsession-smoke=1` starts only in replay mode with an explicit `ZERUS_CLIENT_HGS` and `ZERUS_CLIENT_PERF`, as `actions-smoke`. It runs three passes through the store the dialog uses, logging `zerus-client perf: newsession-smoke <pass>: <outcome>`:
  1. open, choose the replay's project folder, switch to Git repository and back, choose an account, Cancel;
  2. open, None, Start;
  3. open, Auto on a Git catalog, Start.
- [ ] **Step 3: Smoke run**, documented in the README with the headless environment of the actions smoke. Put `fake_hgs_creation.py` as `hgs` in a scratch folder with a `config.json` whose `replies` answer:
  - `account` with one claude profile;
  - `worktrees` with an `ok` catalog of one main checkout;
  - `dirs` with the folder;
  - `--help` with `guarded-create-v1` and `repository-clone-v1`;
  - the agent with exit 0.

  Expected `log.ndjson`:
  - pass 1 has only reads: `account ls`, the account usage reads and `worktrees --path`;
  - pass 2 adds `dirs --creation` and one launch, without `worktrees create`;
  - pass 3 adds `worktrees --creation`, one `worktrees create` and one launch.

  Record the counts and the log's hash in the PR description.
- [ ] **Step 4: Run every check:**
  - `cargo +1.99.1 test` and clippy (`-D warnings`) in `client/core` and `client/src-tauri`;
  - `npx vitest run`, `npm run check`, `python3 scripts/check-ui-separators.py`, `python3 scripts/check-source-privacy.py`.
- [ ] **Step 5: Commit** `docs(client): New session commands, state and smoke run`.

---

## Touch points with the client owner session (webui)

All additive; send this list to `claude/zerus-work-e94411a3/webui` before the PR.

| File | Change |
|---|---|
| `client/src/ui/components/Rail.tsx` | the New button (32) calls `openNewSession({ computer: selectedHostTarget() })` instead of the toast |
| `client/src/ui/sessions/SessionList.tsx` | the header add button (398) is enabled with the same call; `headerMenu(project)`'s "New session…" (141) calls `openNewSession({ project, computer: selectedHostTarget() })`; nothing else |
| `client/src/ui/screens/Projects.tsx` | the folder menu entry (47) and the Folders button (163) call `openFolderSession`, with `folderLaunchProblem` as their reason; the screen takes `requestedProject` once, as Machines takes `requestedMachine` |
| `client/src/ui/screens/reasons.ts` | `NEW_SESSION` goes when unused |
| `client/src/App.tsx` | mounts `<NewSessionDialog />` beside `<ActionDialogs />`; Ctrl or Cmd+N opens it |
| `client/src/store/projects.ts` | `requestedProject` and `requestProject(id)` |
| `client/src/store/actions.ts` | exports `notify` (the notices' own function) for "This project folder changed. Select it again." and the creation notices |
| `client/src/store/catalog.ts`, `types.ts` | `Session.gitRoot`, `Session.cwdCanonical`, `Details.cwd_canonical` (G1) |
| `client/src/bridge.ts`, `types.ts` | the new commands and types |
| `client/src-tauri/src/screens.rs` | `launch_accounts` on the hub; the hub's account reads and usage queue are shared, not duplicated |
| `client/src-tauri/src/lib.rs` | registers the commands; manages `NewSession` and `CreationHost`; the exit flush |

New modules owned by this plan: `store/newSession.ts`, `store/creations.ts`, `ui/sessions/newsession-ui.ts`, `ui/sessions/opencode-models.ts`, `NewSessionDialog.tsx`, `ModelPicker.tsx`, `WorktreePicker.tsx`, `NewWorktreeDialog.tsx`, `ui/components/FolderBrowser.tsx`, `ui/styles/newsession.css`, `src-tauri/src/newsession.rs`, `src-tauri/src/creations.rs`.

## Core additions and open questions

**Trivial and additive, done in this plan** (the events stream reviews the core lines in the PR):
- **G1:** `ViewRow.git_root`, `ViewRow.cwd_canonical` (`view.rs`) and `Details.cwd_canonical` (`details.rs`). The prefill uses the canonical folder (2924), and the worktree picker counts sessions by them (`WorktreePanel.h:135-139`).
- **G2:** `ProjectsView.swarm_id` (`views/projects.rs`), the swarm the folder check's project inference must match (2922, 355-361).
- **G3:** `views::accounts::launchable` public, and `host_profiles(entry, catalog, state)` returning one computer's launchable profiles with their raw rows, for `launch_accounts`.
- **G4:** `launch::own_windows_session()`, this process's Windows session (`ProcessIdToSessionId`, `LocalSessionHost.cpp`; windows-sys feature `Win32_System_RemoteDesktop`), None on other systems. `other_windows_session` takes it as `own`.

**Questions for the events stream:**
- **Q1:** The OpenCode account calls are not in the core:
  - `account models native-opencode --location F --json`, `account favorite … --request-id ID --json` and `account start native-opencode --json` (HgsClient.cpp 607-668), with their limits, receipt checks and texts;
  - `OpenCodeModels::catalogSupported`, `updateText` and `errorText`.

  Will you add them (for example `client/core/src/opencode.rs`), or may Task 1 add them with Qt-recorded tests for your review? Until then the model button lists only the row's short list. All models, stars and Start OpenCode stay disabled.
- **Q2:** `dirs --create PATH --request-id ID` (`createDirectory`, HgsClient.cpp 1810-1870) is not in the core: its canonical parent pin and receipt checks. The folder browser's New folder needs it. Same question as Q1.
- **Q3:** Binding a creation needs, per computer:
  - each catalog row's `launch_id`;
  - `ended_launches` (`{launch_id, name, code, output}`, `docs/reference.md` "ended_launches");

  in the client's catalog or `catalog.meta` projection (SessionsWindow 3020-3070). Neither reaches the client now. Can the catalog and machine projections carry them, bounded at 32 ended launches?
- **Q4:** Placement edits. `creation::Placement::resolve` must create a project by its reserved ID and add a cloned folder (SessionsWindow 1539-1575). A bound session must join its project and add its folder (3043-3050). In the client these are shared-catalog edits (`swarm apply`), owned by the organization stream (`zerus-5wmh.18`). Which function does the creation host call, and which outcome does an unconfirmed edit report?
- **Q5:** Terminal launches. A plan with `terminal` launches through `terminal_launch::Launcher::open_new_session` (`TrayAgent::launchSession`). The client has no Launcher instance or terminal settings yet; they come with the Open terminal family, `zerus-5wmh.9`. May the creation host build a `Launcher` with the platform's default open mode until then?

**Questions for the design session:**
- **D1:** The creation cards are not in this bead's scope list:
  - the `#launch` rows in Sessions;
  - the preparation view: title, details, Continue setup, Check result, Stop creating or Stop after preparation, Edit choices…, Hide status, Return to filters (SessionsWindow 1509-1537, 3729-3756);
  - the footer's "Session creation status" menu (463-480);
  - "Hide launch status" (502-511);
  - the reveal after admission (2914-2919).

  Without them an admitted creation shows no progress, and an interrupted one cannot continue (it waits safely). Proposal: a follow-up piece of this plan on its own Beads child, landing in the same milestone PR before Start is enabled. Which do you want?
- **D2:** The foreground path. Qt's generic New session creates in the background (2910); task executors use the foreground path. Its texts appear only there:
  - "The computer did not confirm this folder. Try again." (348);
  - "The repository could not be confirmed. Choose a working folder again." (350);
  - "Creating worktree…" and "Checking folder…" on the error line and Start (372, 695, 781);
  - "Cloning…", "Checking…" and "Use checkout and start" (696-697);
  - "The project changed. The checkout is available at {path}. Choose a project to use it." (647);
  - "Checkout saved. Select its computer and folder to start a session." (654);
  - "Project changed. Select the checkout again before starting." (737), the fifth launch option of the N1 list;
  - the clone recovery controls "Check result" and "Choose another destination…" (196).

  Proposal: they arrive with the Tasks view's executor launch, through N1's `Start::CheckFolder` and `Start::Clone`. The store keeps a `background` option that is true for every entry point here. Confirm.
- **D3:** The local Browse uses `tauri-plugin-dialog`, a new dependency on the Rust side only, for Qt's `QFileDialog::getExistingDirectory`. Confirm.
- **D4:** Qt texts with dot separators:
  - `OpenCodeModels::marks` joins "Default · Favorite · Free" (`OpenCodeModels.cpp:138`). Proposal: "Default, Favorite, Free".
  - The picker's folder group "{provider} · This folder" (`OpenCodeModelPicker.cpp:269`). Proposal: "{provider} (this folder)".

  Both follow the round 8 rule (a comma or parentheses). Confirm.
- **D5:** The computer chooser shows the OS tile with its state dot (round 6). Should Qt's " (offline)" suffix (501) stay beside it? Proposal: keep it, as the accessible name needs the state in words.

## Owner rulings and deviations from Qt

Known before the first ruling:
1. Background creation only (D2).
2. No pending clone record exists in the client, because only the foreground clone saves one (`RepositoryClone.cpp:83-93`). So "A previous clone is unconfirmed. Check its original result before continuing." (`RepositoryClone.cpp:37`), Check result and Choose another destination… never show (196, 711-713).
3. New worktree's absolute-path check and its destination suggestion use the selected computer's path style (core `paths.rs`). Qt's `QDir::isAbsolutePath`, `QDir::filePath` and `QFileInfo` use this computer's style (`NewWorktreeDialog.h:33, 67, 71`). N1 made the same choice for Auto (`launch::automatic_worktree_available`).
4. The remembered launch choices live in the client's private `launch-choices.json` under Qt's key names until the QSettings migration (`zerus-5wmh.8`).
5. The dot separators of D4, once ruled.

## Order and handoff

| Order | Task | Depends on | Waits for |
|---|---|---|---|
| 1 | Rust surface (`newsession.rs`, G1-G4) | base | Q1 and Q2 for their commands only |
| 2 | Store and rules | 1 | — |
| 3 | The dialog and entry points | 2 | D4, D5 for texts and the chooser |
| 4 | Child dialogs | 2 (parallel with 3) | Q1 (picker stars, All models), Q2 (New folder), D3 |
| 5 | Creation host and admission | 1 (parallel with 2-4) | Q3, Q4, Q5; D1 for the reveal |
| 6 | README and smoke | 3, 4, 5 | — |

Each task gets a spec review against the Qt lines quoted for it, then a quality review; the branch gets a whole-branch review before the PR. One milestone PR (`zerus-5wmh.10.10`), rebased on main after #424, merged on a green "CI gate". After the merge:
- the merge commit goes to the owner session, which rebuilds the Mac dev app;
- the design session hears that New session is ready for the owner to try, with the open items of D1-D5 and Q1-Q5.
