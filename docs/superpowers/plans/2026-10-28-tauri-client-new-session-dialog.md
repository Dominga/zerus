# Tauri Client: New Session Dialog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client opens Qt's New session dialog (`tray/src/NewSessionDialog.cpp`) from every place Qt opens it. It has Qt's inputs, enablement, texts and outcome rules, and it hands its plan to background creation as Qt's generic New session does (`SessionsWindow.cpp:2910-2919`). Each admitted creation then shows as a creation card in Sessions, with Qt's progress view and actions. Beads `zerus-5wmh.10.10`, the view part of `zerus-5wmh.10`. In scope:
- five of the six launch options of the core section's N1 list (`2026-10-28-tauri-client-phase5-core-section.md`, Task N1), with their texts (ruling D2):
  - the direct-folder launch, "Start directly in the working folder." (208);
  - the explicit Worktree options, existing or manual (210-216);
  - the outside-project warning (683);
  - the generated worktree's tooltip (704);
  - the offline folder (719);
- None, Auto, and the explicit existing (Choose existing…) or custom (Create manually…) worktree;
- the Git repository source and New project…;
- account and agent choice with limits, and Run shell;
- OpenCode's model picker and effort, and its explicit service start;
- the local Windows-session warning (`LocalLaunchWarning.h`);
- the expected-machine guard: the plan carries the selected computer's identity, and every creation call checks it;
- opening or inspecting the dialog never creates a worktree, a folder, a clone or a session;
- the creation cards: rows, the progress view, Continue setup, Check result, Stop, Hide status, Edit choices…, and the creation status menu (ruling D1).

Not in this plan:
- the foreground path that task executors use, with the sixth launch option "Project changed. Select the checkout again before starting." (737). It arrives with the Tasks view (ruling D2).
- the Worktrees panel's standard mode and cleanup (`zerus-5wmh.10.5` and the worktree flows);
- Settings.

**Architecture:** The events stream's core is used as it is:
- N1 `launch.rs`: form rules, Start, the plan, the Windows warning, the generated branch, folder listings;
- N2 `repository.rs`: suggested name, name validity, destination;
- N3 `creation.rs`: admission, the journal, the creation steps, captions and details;
- W1 `worktrees.rs`: catalogs, snapshots, manual creation, the machine guard;
- `paths.rs`: folder paths in their computer's form.

This plan adds to the core, as the events stream agreed on 2026-10-28:
- four small fields and helpers (G1-G4);
- OpenCode's account calls and folder creation, beside `launch.rs`'s invocations (Q1, Q2);
- the listing parser for launch binding in `creation.rs` (Q3).

Project and folder edits go through the organization stream's edit Controller (Q4).
- **`client/src-tauri`:**
  - `newsession.rs` (new) exposes the dialog's commands;
  - `creations.rs` (new) hosts `creation::Creations` on its own worker, carries out its effects and exposes the cards' reads and actions;
  - `screens.rs` gains the per-computer account read `launch_accounts`.
- **SolidJS:**
  - `store/newSession.ts` (new) holds the one open dialog and ports the dialog's members and slots;
  - `ui/sessions/newsession-ui.ts` and `ui/sessions/opencode-models.ts` (new) hold the view rules the core does not have;
  - `NewSessionDialog.tsx` and its child dialogs render them;
  - `store/creations.ts` (new) holds the creations, their notices, the card selection and the rows the list inserts;
  - `CreationCard.tsx` and `CreationPanel.tsx` (new) render them.

**Rules the core already has are called, never rewritten in TypeScript.** The form's derived state, Start, the plan, repository fields, the Windows warning, creation captions and details go through commands. TypeScript holds only what Qt keeps in its views: account ranking and labels, model lists, row building, card visibility and the request bookkeeping.

One scoped exception (coordinator ruling, 2026-10-28): the pure path rules of `paths.rs` (`absolute`, `clean`, `same`, `within`, `leaf`, `native`, and the style with the build platform) and three helpers of `launch.rs` (`matching_project_folder`, `prefill_target`, `restored_folder`) are ported line for line to `client/src/ui/sessions/paths.ts`. Qt changes the dialog's folder synchronously (`selectPath` → `loadFolders` → `updateWorktrees` in one turn), and its tests depend on the reset and fill in the same turn. The shared golden fixture `client/core/tests/fixtures/folder-paths.json` pins both copies: `client/core/tests/folder_paths.rs` checks the core's answers and `paths.test.ts` the port's.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2; `tauri-plugin-dialog` 2 for the local folder picker, ruling D3), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Base:** branch `work-1a04708c-newsession`, cut from `origin/integrate/client-core-2` at `0eaeb07f`. That integration (PR #424) carries N1-N3 and W1 (`f64925cc`, `a149081c`, `b86f7914`, `875cc6b1`, `91af1c07`). It is now at `f104a9b5`: `0eaeb07f` plus a draft import. This plan needs no rebase onto it.
- Tasks 1-4 start on `integrate/client-core-2`.
- Tasks 5 and 6 start on main after the organization stream's milestone PR (`work-e94411a3-org`) lands. webui says when.

Rebase onto main once #424 lands, and onto the session-menu parity pieces as they land, since they touch `SessionList.tsx`, `ActionDialogs.tsx` and `store/actions.ts`. The events stream's next core branch is Settings: new `client/core/src/settings*` and one `lib.rs` mod line. It touches neither fleet, state, launch, creation, organization nor accounts, so G1-G4 and the `launch.rs` and `creation.rs` additions do not collide with it.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`:
- Owner constraints: Parity, Usage limits (round 5: New session uses the compact meters), Machines (round 6: the OS tile in the New session computer chooser), Text separators (round 8), Hint text (round 6);
- "Actions".

Also `docs/superpowers/specs/2026-10-26-session-creation-progress-design.md`, the design behind Qt's creation cards. Pattern: `docs/superpowers/plans/2026-10-28-tauri-client-session-menu-parity.md`. Visual base: `design/webui/src/dialogs/setup/NewSession.tsx`, `ModelPicker.tsx`, `Clone.tsx` (its folder browser), `WorktreeDialogs.tsx` and `Creation.tsx`, where Qt does not contradict them.

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
  - the creation status menu (463-480) and the creation row menu (502-511);
  - the creation panel and its wiring (1509-1537);
  - placement 1539-1575, the change handler and failure notice 1571-1586, the launch 1588-1593;
  - `placeLaunchedSessions` (3020-3075) and the bound selection (3253-3255);
  - `selectedHostTarget` (3128-3131);
  - `selectSession` (3434-3442), `selectCreation` and `renderCreation` (3717-3756);
  - filter changes that end a reveal (385, 408, 416-419);
  - the open dialog's fleet updates (2692).
- `tray/src/SessionsWindowRows.cpp` (creation rows: 259-263, 269, 300-341, 376-397, 474-479).
- `tray/src/SessionSearch.cpp:287-310` (creations in search).
- `tray/src/SessionCreationController.cpp` (`details` 430-443, the 1 s clock 65).
- `tray/src/SessionOrganization.cpp:258-269` (`addFolder`).
- `tray/src/SessionsWindowShortcuts.cpp:222` and `tray/src/Shortcuts.cpp:29, 58` (`session.new`, Ctrl+N, a Window shortcut).
- `tray/src/ProjectsDialog.cpp` (180, 501-541, 552-560).
- `tray/src/HgsClient.cpp`:
  - the listing's `launch_id` (184) and `ended_launches` (279), and the `ls --json` calls (816, 847);
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
- **Add nothing Qt does not have.** Where the client lacks a Qt prerequisite, the plan says so and leaves it to its owner.
- **Nothing is created on open or inspect.** Opening, browsing, choosing a project, computer, folder, account, agent, source or worktree mode, and reading catalogs run only reads:
  - `account ls` and the account usage reads;
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
  - The creation journal (N3) owns everything after admission. Check result reads the original result and never sends a step again.
- **Confirmations:** the Windows warning is an alert dialog with Launch anyway and Cancel. Cancel is the default and has the focus, Escape is Cancel (`LocalLaunchWarning.h:25-28`). After Cancel nothing starts, and the form is kept.
- **Expected machine:** Start records the selected computer's identity (its machine card's `host`, else the alias) as the plan's `machine` (754). The creation passes it to every call as `--expect-machine` through `worktrees::run_guarded`. The manual Create worktree sends no expected machine, as Qt does (`NewWorktreeDialog.h:53`).
- **Late answers change nothing.** Each read carries its context: computer, folder, request number, and for OpenCode the newest request per folder. An answer for another context is dropped:
  - the folder check, accounts and the service start (344-345, 380-396);
  - folder catalogs (397-422);
  - favorites (423-453);
  - worktree catalogs and the prefill (454-465);
  - the folder browser (`DirectoryDialog.cpp:127-157`).
- **No dots as separators** (`scripts/check-ui-separators.py`). The dialog's Qt texts with " · " follow ruling D4. The creation cards' texts with " · " wait for D6.
- **No obvious keyboard hints.** Shortcut names stay in tooltips only where the client already shows them.
- **Rules from the client owner session (webui, 2026-10-28):**
  1. **Shortcut.** Ctrl/Cmd+N is Qt's `session.new` (`tray/src/Shortcuts.cpp:29, 58`, a Window shortcut). Bind it as `session.new` through the events stream's client shortcut registry built on Qt's ids (Settings > Keyboard). If the registry is not on the branch yet, bind a fixed default under that id. The Mac app menu has no Cmd+N item, so the webview receives the key.
  2. **Disk paths.** Any state `newsession.rs` or `creations.rs` keeps on disk (pending creations, worktree snapshots, last choices) goes under `crate::data_dir(app)`, never `app_data_dir()` directly. `data_dir` comes with webui's phase 4 Task 3a (`fb11c50e`, `integrate/client-webui`). The modules take their directory as a parameter, and `lib.rs` passes `data_dir(app)`.
  3. **Exit.** State that must reach disk is flushed from the one exit hook: `exit(app)` on main; `lifecycle::exit` before `state.release()` on the Mac branch. Nothing runs on the signal thread.
  4. **Settings.** Qt's settings domain is read-only. The remembered choices (agent, account per computer and agent, Open terminal) go into the events core settings store (`<data_dir>/settings.json`, Qt key names: `workspace/launchAgent`, `workspace/launchAccounts`, `workspace/launchOpenTerminal`), never NSUserDefaults or QSettings. Seeding them once from Qt's values is the design session's call (D7).
- **A creation card never opens a composer or touches drafts.** The composer opens through the owner session's hub only once a creation becomes a live session row (webui, 2026-10-28).
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
  - the creation journal: 128 operations in 8 MiB; the creation-notice memory 128 entries (1583); `creations_view` at most 128 rows; details at most 65 536 characters (`creation::details`);
  - binding listings: read at most `reads::READ_LIMIT`, at most 32 ended launches kept per computer, and only while one of the computer's operations waits for binding;
  - projects 500 with 200 folders each (`views/projects.rs`), and saved-project placement 512 (`launch::SAVED_PROJECTS`).
- **All `hgs` calls run off the UI thread**, as async commands.
- **Shared files:** the touch points with the client owner session (`claude/zerus-work-e94411a3/webui`) are listed under "Touch points"; webui approved them on 2026-10-28. Change only those lines, additively, and send the final list before the PR.
- **One milestone PR** (Beads `zerus-5wmh.10.10`):
  - after the local gates, rebased on main;
  - merged when the "CI gate" check is SUCCESS;
  - after the merge, message the owner session with the merge commit; it rebuilds the owner's Mac dev app.
- **Tests never run the real `hgs`.** Rust tests use `client/core/tests/support/fake_hgs_creation.py`; it answers by the first command word, which covers `account`, `worktrees`, `dirs`, `clone`, `ls` and the agent launch. Store tests mock the bridge and record every command they receive.

## Review Focus

1. A full open, inspect, browse and Cancel run sends only reads. The fake `hgs` log has no write, clone, worktree or launch: no `worktrees create`, `dirs --create`, `clone`, `account start`, `account favorite`, `swarm` or agent launch.
2. No worktree is created before Start: every `worktrees create` comes after the Start that admitted its creation.
3. Start submits once. A refused admission shows the reason and keeps every choice. The dialog closes only after admission (777).
4. Late answers for another computer, folder or request never change the form: account rows, worktree catalogs, the prefill, OpenCode catalogs, favorites and the service start.
5. Account binding:
   - an explicit or preset account is never silently replaced (941-943);
   - a catalog still loading never selects Run shell for another agent (944-950);
   - a remembered account falls back to the provider's default only when it no longer exists.
6. The plan equals Qt's for the same choices (`launch::plan`): model only for OpenCode, effort only with a model, terminal only for a detached agent, destination only for Auto with a folder.
7. The Windows warning: Cancel focused and default; after Cancel, no call.
8. Creation cards:
   - each action acts on the exact operation it shows;
   - Check never re-sends;
   - a bound creation's selection moves to its session;
   - a card never opens a composer.
9. Every list stays within the bounds above.

---

## File Structure

```
client/core/src/view.rs, details.rs          G1: git_root and cwd_canonical (additive)
client/core/src/views/projects.rs            G2: ProjectsView.swarm_id (additive)
client/core/src/views/accounts.rs            G3: pub launchable, pub host_profiles (additive)
client/core/src/launch.rs (+ Cargo.toml)     G4: own_windows_session; Q1: OpenCode models, favorite, start;
                                             Q2: directory_create_invocation and its receipt (additive)
client/core/src/creation.rs                  Q3: binding_invocation and binding_listing (additive)
client/core/src/swarm/edit.rs                Q4: Intent::AddFolder (organization branch, additive)
client/src-tauri/src/newsession.rs           the dialog's commands (Task 1)
client/src-tauri/src/creations.rs            the creation host: worker, journal, effects, binding, cards' reads and actions (Task 5)
client/src-tauri/src/screens.rs              launch_accounts on the hub (additive)
client/src-tauri/src/lib.rs                  registers the commands; manages NewSession and CreationHost; data_dir; the exit hook
client/src-tauri/Cargo.toml, Cargo.lock      tauri-plugin-dialog 2, pinned (ruling D3)
client/src/types.ts, bridge.ts               the new types and commands (additive)
client/src/store/newSession.ts (+test)       the open dialog: state, reads, tickets, openNewSession, openFolderSession
client/src/store/creations.ts (+test)        creations, notices, card selection and reveal, withCreations, the status menu
client/src/ui/sessions/newsession-ui.ts (+test)  rows: computers, projects, folders, accounts; usage text; folderLaunchProblem
client/src/ui/sessions/opencode-models.ts (+test)  OpenCodeModels and the picker's list rules, OpenCodeEffort
client/src/ui/sessions/paths.ts (+test)      the port of paths.rs and three launch.rs folder helpers (the scoped exception)
client/core/tests/folder_paths.rs, tests/fixtures/folder-paths.json  the shared golden fixture both copies check
client/src/ui/sessions/NewSessionDialog.tsx  the dialog and the Windows warning
client/src/ui/sessions/ModelPicker.tsx       OpenCode's grouped picker and its popover
client/src/ui/sessions/WorktreePicker.tsx    Choose an existing worktree (WorktreePanel, picker mode)
client/src/ui/sessions/NewWorktreeDialog.tsx Create manually
client/src/ui/sessions/CreationCard.tsx      a creation row in Sessions
client/src/ui/sessions/CreationPanel.tsx     the creation progress view in the detail pane
client/src/ui/components/FolderBrowser.tsx (+ folder-browser.ts, test)  DirectoryDialog for a peer
client/src/ui/styles/newsession.css          the dialog's and cards' styles (ported from design/webui setup styles)
client/src/App.tsx, ui/components/Rail.tsx, ui/sessions/SessionList.tsx, ui/sessions/model.ts,
client/src/ui/screens/Projects.tsx, ui/screens/reasons.ts, store/projects.ts, store/actions.ts,
client/src/store/catalog.ts                  touch points (below)
client/README.md                             "New session" and its smoke run (Task 7)
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
| Creation steps and the launch | `SessionCreationController`, SessionsWindow 1588-1593 | `Creations` effects, `creation::run_call`, `launch::launch_invocation`, `worktrees::run_guarded`, `terminal_launch::Launcher::open_new_session` | ready (host: Task 5) |
| Launch binding | SessionsWindow 3020-3070, HgsClient 184, 279 | `creation::binding_invocation`, `binding_listing`; `Creations::for_launch`, `complete`, `ended` | Q3: Task 5 adds |
| Placement and the bound session's project | SessionsWindow 1539-1575, 3043-3054 | `creation::place`; the organization Controller with `Intent::Create`, `Intent::Move`, `Intent::AddFolder` | Q4: Task 5 adds `AddFolder` |
| Browse, this computer | `DirectoryDialog.cpp:37-45` | `pick_local_folder` | D3 |
| Browse, a peer | `DirectoryDialog.cpp:46-326` | `launch::directories_invocation`, `run_directories`, `directories`; New folder: `launch::directory_create_invocation` | Q2: Task 1 adds |
| Accounts of the computer | 865-952 | `reads::Read::Accounts` on the screens hub, `views::accounts::group` | G3 |
| Limits | 954-1023 | the hub's usage slots (the shared `AccountUsageStore`) | ready |
| OpenCode models, favorites, service | 1145-1313, HgsClient 607-668 | `launch::opencode_models_invocation`, `opencode_favorite_invocation`, `opencode_start_invocation` and their parsers | Q1: Task 1 adds |
| Repository fields | 289-307, 590-639 | `repository::suggested_name`, `Clone::new`, `set_source`, `can_submit`, `destination` through `repository_fields` | ready |
| Choose existing… | 1114-1124, `WorktreePanel.h` | `worktree_catalog`; rows' `git_root` and `cwd_canonical` for the session counts | G1 |
| Create manually… | 1125-1143, `NewWorktreeDialog.h` | `worktrees::create_invocation`, `created`, `Catalogs::invalidate`, `launch::created_catalog` | ready |
| Remembered agent, account and terminal | 221-224, 263, 897, creation `Remember` | the events core settings store; `launch::remember_account`, `creation::Effect::Write { remember }` | waits for the store |
| Creation rows, panel, menu | SessionsWindowRows 300-397, SessionsWindow 463-511, 1509-1537, 3717-3756 | `creation::caption`, `details`, `can_resume`, `Creations::can_edit`, `resume`, `check`, `stop`, `hide`, `show` through `creations.rs` | Task 5 exposes, Task 6 renders |
| Edit choices… | 1525-1528, 787-808 | `creation_plan`, `launch::restored_folder` | Tasks 5 and 6 |

---

## Task 1: the dialog's Rust surface (`client/src-tauri/src/newsession.rs`)

**Files:**
- Create `client/src-tauri/src/newsession.rs`.
- Modify:
  - `client/src-tauri/src/lib.rs`: register the commands; manage `NewSession` with the worktree `Catalogs` loaded from `<data_dir>/worktree-snapshots.json` (`worktree-snapshots-replay.json` in replay mode, as `actions::marks_file`). `data_dir` is not on `integrate/client-core-2`, so until the rebase `NewSession` keeps its snapshots in memory (`NewSession::tauri(None)`); it takes the file as a parameter;
  - `client/src-tauri/src/screens.rs`: `launch_accounts`;
  - `client/src-tauri/Cargo.toml` and `Cargo.lock`: `tauri-plugin-dialog` (ruling D3);
  - `client/src/types.ts`, `client/src/bridge.ts`. In the browser build every command answers a failure, as the copy commands do.
- Core, additive, with tests:
  - G1-G4;
  - in `client/core/src/launch.rs`, beside `directories_invocation`: the OpenCode calls (Q1) and folder creation (Q2). The events stream does not edit `launch.rs`, so there is no collision;
  - `AccountsState::ensure_catalog` (`views/schedule.rs`), one computer's `account ls` on the hub (G3);
  - `tests/support/fake_hgs_creation.py`: two-word replies (`worktrees create`, `account favorite`), `sleep`, `echo` of a fresh identity and `raw` output.

**Interfaces:**

```rust
/// What the form passes; the command derives the rest with the core and answers both.
#[derive(Deserialize)] pub struct FormInput {
    pub form: launch::Form,          // without automatic_available, automatic_path, project_worktree (filled here)
    pub catalog: Value,              // m_worktreeCatalog
    pub verified: Value,             // m_verifiedWorktrees[host\npath], or null
    pub project: Option<launch::Project>,
    pub host: String,                // this computer "" or the alias, as Project folders name it
}                                    // the form's own catalog_pending is m_catalogRequest
#[derive(Serialize)] pub struct FormView { pub shown: launch::Shown, pub automatic_available: bool, pub automatic_path: String, pub project_worktree: bool }

#[tauri::command] pub fn new_session_context() -> Context;      // { branch, own_windows_session, remembered: { agent, accounts, open_terminal } }
#[tauri::command] pub fn new_session_form(input: FormInput) -> FormView;
#[tauri::command] pub fn new_session_start(input: FormInput, submitted: bool) -> launch::Start;   // background = true
#[tauri::command] pub fn new_session_plan(input: FormInput, choices: launch::Choices) -> launch::Plan; // the form's fields from input, as Start derives them
#[tauri::command] pub fn launch_warning(local: MachineMeta, computer: String) -> Option<String>; // warning_text(other_windows_session(..)), own from G4
#[tauri::command] pub fn repository_fields(url: String, destination: String, style: Style, custom: bool, fresh: Option<String>) -> RepositoryFields;
#[tauri::command] pub fn worktree_snapshot(state: State<NewSession>, computer: String, path: String) -> Value;
#[tauri::command] pub async fn worktree_catalog(state: State<'_, NewSession>, computer: String, path: String, refresh: bool) -> Result<Value, String>;
#[tauri::command] pub async fn create_worktree(state: State<'_, NewSession>, computer: String, path: String, common_dir: String, branch: String, base: String, destination: String) -> Result<String, String>;
#[tauri::command] pub async fn list_directories(state: State<'_, NewSession>, computer: String, path: String, hidden: bool) -> Result<Value, String>;
#[tauri::command] pub async fn create_directory(state: State<'_, NewSession>, computer: String, path: String) -> Result<Value, String>;
#[tauri::command] pub async fn pick_local_folder(app: AppHandle, start: String) -> Option<String>;
#[tauri::command] pub async fn local_folder_exists(path: String) -> bool;                                   // ProjectsDialog.cpp:530, on the blocking pool
#[tauri::command] pub async fn opencode_models(state: State<'_, NewSession>, computer: String, folder: String, name: String) -> Result<Value, String>;
#[tauri::command] pub async fn opencode_favorite(state: State<'_, NewSession>, computer: String, value: String, favorite: bool, name: String) -> Result<Value, String>;
#[tauri::command] pub async fn opencode_start(state: State<'_, NewSession>, computer: String) -> Result<Value, String>;
// screens.rs
#[tauri::command] pub async fn launch_accounts(state: State<'_, Screens>, computer: String, refresh: bool, agent: Option<String>) -> Result<LaunchAccounts, String>;
#[tauri::command] pub fn launch_account_rows(state: State<'_, Screens>, computer: String) -> LaunchAccounts;  // reads nothing
#[tauri::command] pub fn launch_accounts_close(state: State<'_, Screens>);  // the dialog closed
// event "launch-accounts-changed" (payload: the computer)
```

Rules:
- **`new_session_context`:**
  - `branch` is `launch::generated_branch` of a fresh UUID: `work-` and eight lower-case hex digits (151);
  - `remembered` reads `workspace/launchAgent`, `workspace/launchAccounts` (`{host: {agent: account}}`) and `workspace/launchOpenTerminal` (default false) from the events core settings store (`<data_dir>/settings.json`). Task 5 writes them.

  Until the store is on the base, `remembered` is empty: Qt's defaults apply (agent `codex`, no remembered account, Open terminal off). Nothing is written anywhere else (webui rule 4).
- **`new_session_form`:**
  - fills `automatic_available` with `automatic_worktree_available(catalog, path, style, catalog_pending)`, and `automatic_path` with `automatic_worktree_path(catalog, branch, style)`;
  - fills `project_worktree` with `project_worktree(path, catalog, …) || project_worktree(path, verified, …)` (1110-1113);
  - answers `launch::shown`.

  `new_session_start` answers `launch::start(form, true, submitted)` on the same derived form. The view asks it again at the click, so a stale `Shown` never starts anything.
- **`repository_fields`** builds a fresh `repository::Clone::new(style, None, None)`. The client never saves a pending clone record (deviation 2).
  - A URL change without a custom destination suggests `{parent}/{suggested_name(url)}` in native form (289-295).
  - The destination splits into parent and name as `editRepositorySource` does (628-639). On Windows `\` reads as `/`; separator 0 gives the parent `/`, and a parent `X:` becomes `X:/`.
  - With `fresh` (a new computer or style, `syncRepositoryComputer` 608-627) the destination starts in `~` with the old record's folder name, kept even when empty (613); the view then clears its custom flag. The first record (no `fresh`, an empty field) is named after the URL.
  - It answers `{url, parent, name, field, destination, suggested, can_submit}`: `field` is the Clone to field's text, `destination` the clone's `PARENT/NAME` that Start and the plan take (767-768).
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
- **`create_directory`** (Q2, `createDirectory` 1810-1870). The core gains `directory_create_invocation` and `directory_created` in `launch.rs`, next to `directories_invocation` and `creation::creation_directory_invocation`:
  - first `hgs [@host] --help` must list `dirs --create <path>` (5 s here, 15 s on a peer; HgsClient 1842-1850), else "Update hgs on {computer|this computer} to create folders. You can still choose existing folders.", and nothing is created;
  - the canonical parent is pinned before the creation (`dirs PARENT`; an unconfirmed one gives "The computer did not confirm the parent folder. Open it before creating a folder.");
  - then `hgs [@host] dirs --create PATH --request-id ID`, 5 s here and 15 s on a peer;
  - the receipt must be `created` with the same `request_id` and `requested_path`, an absolute `path` equal to the cleaned `{parent}/{leaf}` and the same `parent`; otherwise Qt's failure text.
- **`launch_accounts`** reads one computer, as the dialog does (865-874, 956-963):
  - It runs that computer's `account ls` on the hub (25 s) on every open, computer change and Refresh, and waits for it; a read already running is shared, without a follow-up. An open or another computer (`refresh` false) starts with no rows until a reply is filed (Qt clears `m_accounts`, 869-871); Refresh keeps the rows on failure. A failure reads "Could not load accounts: %1" with Qt's `failureDetail` (`launch::read_accounts`, which the Accounts page now uses too).
  - The rows are grouped when a good catalog is filed (or the folder style changes, 883); later limits only refresh the members and each row's best usage (`views::accounts::refresh_groups`, `refreshUsage` 990-1000).
  - `agent` is the dialog's requested agent: it joins the provider order after `launch::AGENTS` (221-222). `launch_accounts_close` ends the watch when the dialog closes.
  - It ensures usage for the computer's installed profiles; `refresh` forces it.
  - `launch_account_rows` answers the rows again and reads nothing. Each catalog or limits reply for the dialog's computer emits `launch-accounts-changed`; the view then pulls the rows, as Qt follows its usage store (340-342).
  - It answers `LaunchAccounts {loaded, error, profiles, groups}`:
    - `profiles`: the raw rows, at most 256, for the OpenCode fields (`model_options`, `model_options_truncated`, `catalog_state`, `catalog_reason`, `installed`, `install_reason`);
    - `groups`: `views::accounts::group` over that computer's launchable profiles only (883-896); each member carries its `Profile` projection with the cached usage (971-978), and each group the best usage of its members (980-988).
  - "Invalid account catalog response" for a bad reply.
- **The OpenCode commands** (Q1). The core gains `opencode_models_invocation`, `opencode_favorite_invocation` and `opencode_start_invocation` in `launch.rs`, with their parsers, following Qt's calls:
  - `account models native-opencode --location F --json`: 30 s; over 5 MiB gives "The OpenCode model catalog is too large", a bad reply "Invalid OpenCode model catalog response";
  - `account favorite native-opencode --model V --favorite on|off --request-id ID --json`: 25 s, one fresh identity per call; the receipt must be `applied` with the same `request_id`, `model` and `favorite`, otherwise "Invalid favorite response";
  - `account start native-opencode --json`: 60 s;
  - `OpenCodeModels::errorText`'s usage-line rule, read in the first 16 lines of at most 4000 characters, which turns an older CLI's usage text into "Update Zerus on {computer} to search all models and star favorites." (`OpenCodeModels.cpp:148-162`). `opencode_models` and `opencode_favorite` apply it with `name`, the computer as the dialog names it (this computer's host name, 416, 451-452).
- **`pick_local_folder`** opens the system folder dialog titled "Choose a folder" at `start`, or the home folder for `~` or empty (`DirectoryDialog.cpp:40-45`). Ruling D3 applies:
  - It is the official `tauri-plugin-dialog`, opened from Rust only, pinned in `Cargo.toml` and `Cargo.lock`. `integrate/client-webui` already pins it at 2.8.4 for the composer's picker (`fb11c50e`); reuse that pin after the rebase, else add the same.
  - `capabilities/default.json` keeps `core:default` alone: the webview never gets the plugin's JavaScript permission.
  - The license check is `python3 scripts/collect-licenses.py "$TMP/client-licenses" --manifest-path client/src-tauri/Cargo.toml`. It must find a notice for every shipped crate. The CI step "Rust license notices" runs the script for the root workspace only, so this is a local gate in Step 4 and in the PR description.
- **`launch_warning`** answers `warning_text(session)` when `other_windows_session(local, computer != "", own_windows_session())` names a session, else None.

- [ ] **Step 1: Write the failing tests:**
  - **core:**
    - G1-G4;
    - the OpenCode invocations and receipts;
    - `directory_create_invocation` and the receipt checks;
  - **`newsession.rs` units:**
    - the Windows warning: a running host of session 2 with own session 1 asks; same session, a remote launch, a stopped host, no own session and a non-Windows computer do not (`launchIntoAnotherWindowsSessionAsksFirst`, `launchWithoutAnotherWindowsSessionDoesNotAsk_data`);
    - `repository_fields`: URL to destination, custom destination kept, Windows parents (`repositorySourceKeepsCommonChoicesAndManualDestination`);
    - `new_session_form` derives Auto and the project worktree from the catalog or the verified one;
  - **integration through `fake_hgs_creation.py`:**
    - a fresh snapshot answers without a call; `refresh` calls; two reads share one call; a generation change gives "Folder changed. Refresh its catalog.";
    - `create_worktree`'s confirmed, mismatched and failed answers with their texts;
    - `list_directories` with an invalid reply;
    - `create_directory` confirmed and mismatched;
    - `launch_accounts`:
      - the local Windows computer hides `dsh` and `opencode` (`localWindowsHidesDeepSeekAndOpenCode`);
      - grouping matches the Accounts page for each computer (`chooserAndAccountsPageShowSameAccountsForEachComputer`, `accountAliasesMatchAccountsGroupingAndKeepExactBinding`);
      - an empty catalog gives no groups (`emptyComputerCatalogOffersOnlyShell`);
    - the OpenCode receipts (`openCodeStarSendsOneFavoriteRequestWithIdentity`) and the start call (`openCodeStartsStoppedServiceExplicitly`).
- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core` and `cargo +1.99.1 test --locked --manifest-path client/src-tauri/Cargo.toml newsession::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, clippy (`-D warnings`) for both packages and the license check above. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): New session's commands over the launch, worktree and account cores`.

## Task 2: the dialog store and its rules

**Files:**
- Create `client/src/store/newSession.ts` (+test), `client/src/ui/sessions/newsession-ui.ts` (+test), `client/src/ui/sessions/opencode-models.ts` (+test), `client/src/ui/sessions/paths.ts` (+test).
- Create the shared fixture `client/core/tests/fixtures/folder-paths.json` and its core test `client/core/tests/folder_paths.rs` (regenerate with `cargo test --test folder_paths -- --ignored`).
- Touch points: `client/src/store/projects.ts` (`requestedProject`, `requestProject`), `client/src/store/actions.ts` (export `notify`), `client/src/store/catalog.ts` and `client/src/ui/sessions/types.ts` (`gitRoot`, `cwdCanonical`), `client/src/bridge.ts` (`creationSubmit`, Task 5's `creation_submit`).
- Rust, additive:
  - `newsession.rs`: `Context.build_style` (`paths::build`, this computer's style before it reports a platform) and `FormInput.local` (this computer's card, from which the command derives `can_enable_local_sessions`, `LocalSessionHost::canEnable` 729);
  - `views/projects.rs`: `Folder.machine_id`, so the computer list adds an identified folder's computer only when it is configured (495).

**Interfaces:**

```ts
export interface NewSessionOptions { agent?: string; project?: string; computer?: string; folder?: string; path?: string; account?: string | null; recipe?: Record<string, unknown> }
export function openNewSession(options?: NewSessionOptions): Promise<void>;           // SessionsWindow::showNewSession (2895-2928); computer defaults to selectedHostTarget()
export function openFolderSession(project: string, folder: Folder): Promise<string>; // ProjectsDialog::newFolderSession (533-541): "" when it opens, else the status text
export function selectedHostTarget(): string;                                        // SessionsWindow::selectedHostTarget (3128-3131)
export const newSession: () => NewSessionState | undefined;                          // one dialog at a time
export const newSessionActions: NewSessionActions;                                   // the controls' slots, no-ops without a dialog
export function closeNewSession(): void;                                             // reject (476)
export function startNewSession(confirm: (text: string) => Promise<boolean>): Promise<void>;
export function onCreationAdmitted(listener: (id: string, plan: LaunchPlan) => void): () => void; // store/creations.ts reveals the card
```

The state mirrors `NewSessionDialog.h:98-175`; drop only the members of the foreground path (ruling D2).

Rules (each with its Qt lines; port the logic, not the widget code):
- **Opening** (`showNewSession`, 2895-2928):
  - The project is the current row's project, else the selected session's, else the default project. A `project` option matching an id or name wins, and only it makes the project explicit (2921).
  - The selected session is context only for a generic open (no project, folder, path or account option) on a session row (2902-2906). It sets the computer and prefills its folder:
    - the folder is the details' `cwd`, else the row's;
    - the canonical form is the details' `cwd_canonical`, else the row's.
  - Then, in order: `folder` (`selectFolder`; failure: the sticky notice "This project folder changed. Select it again.", and the dialog does not open), `path` (`selectPath(path, false)`), `account` (`selectAccount`), `recipe` (`restoreCreationChoices`, 787-808, with `launch::restored_folder`; Task 6's Edit choices… opens with it).
  - The swarm is the projects view's `swarm_id` (2922, G2).
- **Initial choices:**
  - the agents are codex, claude, kimi, dsh, opencode, plus an unknown requested agent (221-222);
  - the agent is the option's, else the remembered one when listed or `sh`, else `codex` (223-224);
  - the remembered account is per computer and agent (897, 922-923); Open terminal is the remembered value (263). Both come from `new_session_context` (webui rule 4);
  - the name is the generated branch (260).
- **Computers** (`loadMachines`, 487-510):
  - this computer when it runs local sessions, then the peers, the previous choice, and the project's folders' computers (local only with local sessions; a folder with a machine id only when its computer is configured);
  - the label is the computer, plus " (offline)" when its card is not ok (ruling D5);
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
  - `OpenCodeModels`: `parse`, `providers`, `group`, `filter`, `marks` ("Default, Favorite, Free", ruling D4), `catalogSupported` (`OpenCodeModels.cpp`);
  - `OpenCodeEffort.valid`;
  - the dialog's `addModelOptions` (51-71) and the favorite marking (74-105);
  - `updateModels` (1148-1207) and `updateEfforts` (1209-1219): a changed model keeps the effort only when it offers it;
  - `openCodeProfile` (1220-1228) and the service row (1229-1254);
  - folder catalogs: at most 4, the current one kept; only the newest request per folder answers; a failed refresh keeps a ready catalog (397-422, 1277-1294);
  - favorites: one request per star and late answers ignored; after a confirmed change, the computer's other folders' catalogs are dropped (423-453, 1306-1313);
  - the picker's All models ask (328-337);
  - "Use native default model" (338).
- **The form:** the store assembles `FormInput` from its state and keeps the latest `FormView` for the latest revision. The account status ("Loading accounts…", "Could not load accounts: %1", "Could not start OpenCode: %1") stays its own line beside Refresh, as Qt's `launchAccountStatus` label.
  - The error line takes `shown.notice` when there is one, which drops the error shown before (core `Shown.notice`, 719-731).
  - Otherwise it shows the last transient error: a refused admission, a favorite failure, "Worktree created at %1 on %2. Select that folder to start a session.", and the account status texts.
- **Start** (`startNewSession`, 743-786):
  1. Ask `new_session_start(input, submitted)`; `nothing` ends here.
  2. With `launch_warning(...)` text, await `confirm(text)`; Cancel ends here and changes nothing.
  3. Ask `new_session_start` again (748).
  4. Set `submitted`, then `creation_submit(new_session_plan(input, choices))`, with the input and choices read in the same turn as the click's last `new_session_start` (751-771).
  5. `{admitted}` closes the dialog (777) and hands the id to `store/creations.ts`, which reveals its card (Task 6), also when the dialog was closed meanwhile. `{refused}` clears `submitted` and shows the text (783-786). A rejected call keeps `submitted` and shows the error: an unknown outcome is never sent again.

  No creating command is sent before step 4. The choices are `launch::Choices` from the state (751-771):
  - the computer's identity as `machine`;
  - the swarm;
  - the project's name as `project_name` (765);
  - `listed_folder` when the folder row has an id;
  - `predecessor` for an Edit choices… recipe (776).
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
  - no `creation_submit`, `create_worktree` or other creating command before `startNewSession` reaches step 4 (Review Focus 2);
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
| Target | project choice (accessible "Project", project colour), the manage button (tooltip and accessible "Manage this project's folders"), computer choice (accessible "Computer", with the OS tile and state dot, spec round 6, and Qt's " (offline)", ruling D5) | 146-157 |
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
- the buttons: "Open Machines" (only without a computer), Cancel, then the primary Start button. Its text is `shown.start_text`; here "Start session" or "Clone and start" (ruling D2 leaves the others to the Tasks view).

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
| `session.new`, Ctrl or Cmd+N (webui rule 1) | `Shortcuts.cpp:29, 58`, `SessionsWindowShortcuts.cpp:222` | same |
| Project header menu "New session…" (`SessionList.tsx:141`, `LATER` today) | `showGroupMenu` 2572-2573 | `openNewSession({ project, computer: selectedHostTarget() })` |
| Projects folder menu "New session…" (`Projects.tsx:47`) | `ProjectsDialog.cpp:558-560` | `openFolderSession(project.id, folder)`, disabled with `folderLaunchProblem` as hint, otherwise the folder's native path as hint |
| Projects Folders "New session…" button (`Projects.tsx:163`) | `ProjectsDialog.cpp:180, 520-521, 222` | same; its tooltip is "New session in {path} on {machine}" or the problem |

`session.new` is a Window shortcut: it works on every page, as Qt's does.
- When the events stream's shortcut registry is on the branch, register the id `session.new` with Qt's default Ctrl+N (Cmd+N on macOS) and its Settings > Keyboard label "New session" in the group "Sessions" (`Shortcuts.cpp:29`).
- Without the registry, `App.tsx`'s key handler binds the fixed default under the id `session.new`, so the registry later takes it over unchanged.

`SessionDetail.tsx` has no New session entry: Qt's detail pane has none, and its panel button is the list header's. `reasons.ts` loses `NEW_SESSION` once nothing uses it.

- [ ] **Step 1: Write the failing tests:**
  - the entry points' option objects (with a mocked `openNewSession`);
  - `session.new` opens the dialog on every page;
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
    - the creation itself is `create_directory` (Task 1).

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
    - grouped list (accessible "Models"), with a folder's group headed "{provider} (this folder)" (ruling D4);
    - "All models" (tooltip "Search every model OpenCode offers", or the reason);
    - "Unavailable favorites";
    - row marks "Default, Favorite, Free" (ruling D4) and "Unavailable";
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

Based on main after the organization stream's milestone PR lands (`work-e94411a3-org`; webui says when).

**Files:**
- Create `client/src-tauri/src/creations.rs`, `client/src/store/creations.ts` (+test).
- Modify:
  - `client/src-tauri/src/lib.rs`: manage the host; the catalog hook; `quit` and the journal flush in the one exit hook (webui rule 3);
  - `client/src/types.ts`, `client/src/bridge.ts`.
- Core, additive: `client/core/src/creation.rs` (`binding_invocation`, `binding_listing`, Q3); `client/core/src/swarm/edit.rs` (`Intent::AddFolder`, Q4).

**Interfaces:**

```rust
#[tauri::command] pub async fn creation_submit(state: State<'_, CreationHost>, plan: Value) -> Admission; // {"admitted": id} | {"refused": text}
#[tauri::command] pub fn creations_view(state: State<CreationHost>) -> Vec<CreationCard>;  // not bound, newest first, at most 128
#[tauri::command] pub fn creation_details(state: State<CreationHost>, id: String) -> Option<String>; // creation::details with Qt's stamps
#[tauri::command] pub fn creation_action(state: State<CreationHost>, id: String, action: CreationAction) -> Result<(), String>;
#[tauri::command] pub fn creation_plan(state: State<CreationHost>, id: String) -> Option<Value>;  // Edit choices: the operation, when can_edit
pub enum CreationAction { Resume, Check, Stop, Hide, Show }
/// A card's fields, copied from the operation; the captions come from the core.
pub struct CreationCard {
    id: String, tag: String, state: String, caption: String, error: String, created: i64,
    group: String, host: String, machine: String, cmd: String, account: String, target: String,
    show_pending: bool, launch_dispatched: bool, stop_requested: bool, worktree_receipts: bool,
    can_resume: bool, can_edit: bool,
}
// event "creations": the changed card, or { id, removed: true }, per Effect::Changed / Removed
```

Rules:
- **One worker owns `Creations::new(clock, uuid)`.** At start it loads the journal `<data_dir>/creations.json` (`creations-replay.json` in replay mode) as `Journal::Missing`, `Unreadable` or `Bytes`. Effects are carried out in order:
  - **`Write`:** an ordered writer thread writes the bytes atomically (temporary file, sync, rename) and answers `written(write, ok)`. After a good write, a `remember` saves to the events core settings store (webui rule 4):
    - `workspace/launchAgent`;
    - `workspace/launchAccounts` through `launch::remember_account`;
    - `workspace/launchOpenTerminal`.

    Until the store is on the base, the remember is dropped.
  - **`Call`:** a pool thread runs `creation::run_call(hgs, &call)`, then the worker answers `answered(flight, answer, &mut placement)`.
  - **`Launch`:**
    - without `terminal`: `launch_invocation(computer, &Launch {…}, expected_machine)` through `worktrees::run_guarded` (90 s, the `guarded-create-v1` probe), then `launched`, then `launch_finished(launch_id, result)`;
    - with `terminal` (Q5): the core's `terminal_launch::Launcher::open_new_session`, Qt's `makeLaunchTerminal` choice (Konsole tab or raise, xdg-terminal-exec, Terminal.app, Windows Terminal), with its advisory text, as `TrayAgent::launchSession` (669-688). There is no stand-in.
      - A failure answers `launch_finished(id, Err(error))`, as `newSessionLaunchFailed` does (3004).
      - It shows Qt's notice "Zerus: the terminal did not open" with the error, a blank line and the command (684-685).
      - The launcher runs on a worker thread, since a Konsole call may take 3 s.
  - **`Changed` and `Removed`:** emit the `creations` event.
- **Admission** (2911-2913): `creation_submit` answers `{"admitted": id}` from `Creations::submit(plan)`, or `{"refused": text}` with `UNSAFE_URL`, `TOO_MANY`, `ADMISSION_FULL` or `admission_error()`, as `CreationAdmission` in `types.ts` (the contract the store already follows, Task 2 fix round 1). A refusal clears the dialog's `submitted` and shows the text on its error line. A rejected call is not a refusal: its outcome is unknown, so the dialog stays submitted, shows the error and never sends it again. The command therefore never answers a refusal as `Err`.
- **Placement** (Q4). Qt goes through the organization model, not raw `requestSwarm`:
  - `SessionsWindow.cpp:1564` and `:3054` call `m_organization.addFolder(group, machine, path, {}, style)`;
  - project creation and the move are `SessionOrganization` edits;
  - `SwarmController` sends them all as one declarative, version-guarded `swarm apply`.

  The client port is the organization stream's: `client/core/src/swarm/edit.rs` `Intent`, `client/core/src/swarm/sync.rs` `Controller` (one apply at a time, 120 ms flush, a draft that survives restarts), and `src-tauri/src/organization.rs` (the `edit` command, `Pump::edited(&Intent)`). A second writer calling `swarm apply` or project commands would race the Controller, so the host sends nothing else. `creation::Placement::resolve` follows Qt 1539-1575 with `creation::place`:
  - a cloned repository with New project… creates the project with `Intent::Create { id: reserved_project, name: project_name, members: [] }`;
  - a cloned checkout joins its project with `Intent::AddFolder { project, machine, path, name: "", style }`;
  - every edit waits for `Controller::ready()`, as every edit does: before the first state, edits are refused or held, never sent.

  After binding (`placeLaunchedSessions` 3043-3054):
  - an untouched implicit Inbox stays as it is;
  - otherwise the folder to add (`add_folder`: the source folder, else the target) joins with `Intent::AddFolder`;
  - the session moves with `Intent::Move { members: [session], project }` once its member id exists in the organization.

  `Intent::AddFolder` is new in `edit.rs`, a port of `SessionOrganization::addFolder` (`tray/src/SessionOrganization.cpp:258-269`):
  - no blank machine, an absolute path without a line break, the path cleaned;
  - an existing folder of the same machine and path is kept;
  - otherwise a new folder id with the name trimmed to 80.

  It has its own tests and goes through the same Controller.
- **Binding** (Q3, `placeLaunchedSessions` 3020-3070). The watch stream carries neither `launch_id` nor `ended_launches`; Qt reads both from `hgs [@host] ls --json` (`HgsClient.cpp:184, 279`; `src/state/launches.rs`). The host reads that listing for a computer only while one of its operations is `launching` or `confirming`:
  - **the call:** `creation::binding_invocation` is `ls --json --local --no-archives` here and `@host ls --json --no-archives` on a peer, 15 s (`HgsClient.cpp:816, 847`).
  - **the parser:** `creation::binding_listing` reads at most `reads::READ_LIMIT`. It keeps, per session, only `launch_id`, `cmd`, `name` and `state`, and at most 32 `ended_launches` (`{launch_id, name, code, output}`).
  - **the cadence:** Qt's poll cadence is every 2 s here and 60 s on a peer (`AppConfig.h:13-14`), plus one read right after the launch call ends and one after Check result (1534, 1592).
  - **completion:** an online, live session with the operation's launch id (`Creations::for_launch`), the same agent and the same tag completes it (`complete`).
  - **an ended launch:** an `ended_launches` entry with no session for it ends the operation (`ended`) with "Session {tag} ended before it appeared (exit code {code})." or "Session {tag} ended before it appeared.", a line break and at most 4000 characters of its output (3061-3068).
  - **a missing row** in one listing proves nothing; the operation keeps waiting.

  Carrying both fields on watch-v1 is filed by the events stream as a follow-up.
- **Notices** (1577-1586): `store/creations.ts` shows the sticky error notice "Creation {tag}: {caption}\n{error}" once per state and error, for `failed`, `unknown` and `storage_error`. The memory keeps 128 entries.
- **The cards' surface** (read by Task 6):
  - `creations_view` and the `creations` event carry every unbound operation's card;
  - `creation_details` formats `creation::details(operation, now, time)` with Qt's stamps: `Created:` in ISO date form, steps as `HH:mm:ss` (`SessionCreationController.cpp:440-442`);
  - `creation_action` calls `resume`, `check` (then the binding read), `stop`, `hide` or `show` on the exact operation id, and answers an error when the operation is gone or bound;
  - `creation_plan` answers the operation only while `can_edit` holds.
- **Exit** (webui rule 3): the one exit hook calls `quit()`, then waits at most 2 s for the writer, as the drafts flush does. Nothing runs on the signal thread.

- [ ] **Step 1: Write the failing tests:**
  - core: `binding_listing` (bounds, missing fields, 33 ended launches keep 32) and `Intent::AddFolder` (`SessionOrganization::addFolder`'s cases);
  - src-tauri integration with `fake_hgs_creation.py`:
    - an ordinary folder with None: `dirs --creation`, then the launch with `--expect-machine`, and no `worktrees create`;
    - Auto: the verified catalog, `worktrees create`, its receipt, then the launch in the new checkout;
    - the repository source: the access check, then `clone`, then `Intent::Create` and `Intent::AddFolder` after `ready()`;
    - a failed journal write: "Could not save creation progress. No further setup step was sent." and no call;
    - a restart with an interrupted operation: `recover`, and no call until Continue setup;
    - a launch exiting 0 stays `confirming`; the binding read with its launch id binds it, then `Intent::Move`; an `ended_launches` entry ends it with Qt's text;
    - no binding read for a computer without a waiting operation;
    - the admission limits' texts;
  - store: the notice deduplication.
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement.**
- [ ] **Step 4: Run** src-tauri and core tests, clippy, `npx vitest run`, `npm run check`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): background session creation host and New session admission`.

## Task 6: the creation cards

Based on main after the organization stream's milestone PR lands, after Task 5.

**Files:**
- Create `client/src/ui/sessions/CreationCard.tsx`, `client/src/ui/sessions/CreationPanel.tsx`.
- Extend `client/src/store/creations.ts` (+test) and `client/src/ui/styles/newsession.css`.
- Touch points (the smallest agreed insertions, below): `client/src/ui/sessions/model.ts`, `client/src/ui/sessions/SessionList.tsx`, `client/src/App.tsx`.

It reads Task 5's surface: `creations_view`, the `creations` event, `creation_details`, `creation_action` and `creation_plan`.

**Interfaces (store):**

```ts
export const creations: () => CreationCard[];                 // by id, newest first
export const selectedCreation: () => string | undefined;      // m_selectedCreation
export function selectCreation(id: string): void;             // selectCreation (3717-3728)
export function revealCreation(id: string): void;             // admission and the status menu (475, 2916-2919)
export function withCreations(rows: Row[], scope: ListScope): Row[];   // the creation rows and the adopted session (SessionsWindowRows 269-341)
export function creationStatusMenu(): MenuEntry[];            // 463-480
export function creationFooter(): string;                     // ", 1 creation" or ", {n} creations" (474-479)
```

Rules, each Qt's:
- **Which cards show** (`SessionsWindowRows.cpp:300-323`). A card shows under its destination project: its `group` when that project exists, else the default project.
  - The revealed card always shows. Otherwise:
    - the state filter is All or Active;
    - the machine filter, if any, holds its computer;
    - with a search query, the card shows when its details contain the query, case-insensitively, as Qt's search lists creations (`SessionSearch.cpp:287-298`, deviation 6);
    - it is not hidden (`show_pending`).
  - A project with only cards still gets its header (323). A collapsed project hides its cards (337).
  - The cards follow the project's sessions (337). The header's total counts them (397).
- **The adopted session** (269, 300, 3036-3039, 3253-3255): when the selected creation binds, the selection moves to its session's row. That row shows through every filter until a filter or search changes or Return to filters is pressed (385, 408, 416-419, 1529-1532).
- **The row** (376-391):
  - the title is the tag;
  - the status is `caption`;
  - the detail is the error, else "Preparing the session in the background" (384);
  - also the folder (`target`), the agent's glyph and the computer with its colour;
  - the accessible text is "{tag}, {computer}, {caption}" (390).

  The row is selectable, with the session card's height (94).
  - A click selects it (`selectSession` 3434-3438).
  - Its context menu has "Hide launch status", which hides it (502-511).
  - A card never opens a composer or touches drafts (webui).
- **Selection** (3717-3728, 3441-3442):
  - selecting a card clears the session selection and any open subagent, so the composer and Activity show no session;
  - selecting a session clears the card, and the adoption unless it is the adopted row;
  - a card that becomes hidden or bound loses its selection and reveal (1571-1576).
- **The panel** (`renderCreation` 3729-3756, the panel 1509-1537), in the detail pane while a card is selected:
  - the title: tag, caption and the elapsed time `{m}:{ss}`, laid out per D6;
  - the details text (read-only, accessible "Session creation progress", 1514), from `creation_details`, keeping its scroll on refresh;
  - the buttons, in Qt's grid (1522-1524):

    | Button | Shows when | Calls |
    |---|---|---|
    | "Continue setup" | `can_resume` | `resume` |
    | "Check result" | the state is `unknown`, `recover` or `confirming` | `check`, then the binding read |
    | "Stop creating", or "Stop after preparation" while `cloning`, or `worktree` without `worktree_receipts` (3755) | no `launch_dispatched` and the state is not `stopped`, `failed` or `storage_error`; enabled without `stop_requested` | `stop` |
    | "Edit choices…" | `can_edit` | opens New session with `openNewSession({ agent: cmd, project: group, computer: host, account, recipe: creation_plan(id) })` (1525-1528); its admission hides the predecessor when `can_edit` still holds (2914-2915) |
    | "Hide status" | always | `hide` |
    | "Return to filters" | always | ends the reveal and the adoption (1529-1532) |
  - the panel re-renders every second while shown, for the elapsed time and "Taking longer than usual…" (1537, controller 65).
- **Reveal after admission** (2914-2919):
  - select the new card and reveal it;
  - expand its project;
  - show the Sessions page and scroll the card into view.
- **The status menu** ("Session creation status", the footer clock, 463-480):
  - one entry per unbound operation, newest first by `created`, then id;
  - a heading "Older creations…" before every further 50 (471, D6);
  - each entry's text per D6;
  - choosing one that is gone or bound shows "This creation has already been resolved." (474);
  - otherwise it reveals, selects and shows that card (`show`) on the Sessions list (475-476);
  - an empty menu has the disabled "No pending session creations" (479).
- **The footer** (474-479): the count text gains ", 1 creation" or ", {n} creations" (only "1 creation" or "{n} creations" when no session shows).

Texts, verbatim:
- the captions of `creation::caption`: "Saving request…", "Queued", "Checking folder…", "Checking repository access…", "Cloning repository…", "Creating worktree…", "Verifying checkout…", "Starting agent…", "Confirming session…", "Stopping setup…", "Setup stopped", "Continue setup", "Creation result unknown", "Could not save creation", "Creation failed", "Session created";
- "Preparing the session in the background" (`SessionsWindowRows.cpp:384`);
- "Hide launch status" (505);
- "Session creation status" (464), "Older creations…" (471), "This creation has already been resolved." (474), "No pending session creations" (479);
- "Session creation progress", "Continue setup", "Check result", "Stop creating", "Stop after preparation", "Hide status", "Return to filters", "Edit choices…" (1514-1521, 3755);
- "1 creation", "{n} creations" (`SessionsWindowRows.cpp:477`);
- the details lines come from the core (`creation::details`).

- [ ] **Step 1: Write the failing vitest tests:**
  - `withCreations`:
    - each filter rule;
    - the revealed card through filters;
    - hidden cards;
    - the default-project fallback;
    - a project with only cards;
    - a collapsed project;
    - the header total;
    - the adopted session through filters and its end on a filter change;
  - the footer text;
  - the status menu's order, headings, resolved notice and empty entry;
  - the panel's button visibility and Stop text for each state;
  - selection exclusivity, and the move to the bound session;
  - Edit choices… opens the dialog with the recipe and hides the predecessor on admission;
  - no composer or draft command is sent from a card (bridge mock).
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): creation cards, progress and the creation status menu`.

## Task 7: README and smoke steps

**Files:** `client/README.md` ("New session"), `client/src/ui/PerfHost.tsx` (the `newsession-smoke` hook, beside `actions-smoke`), `client/core/tests/support/fake_hgs_creation.py` (an optional stamped log).

- [ ] **Step 1: README:**
  - the commands and their `hgs` calls;
  - what opening reads and what only a click creates;
  - the creation cards and their actions;
  - where state lives, all under `data_dir`: `worktree-snapshots.json`, `creations.json`, and the remembered choices in the settings store;
  - the bounds;
  - the questions still open (D6, D7, Q6).
- [ ] **Step 2: Stamped fake log.** `fake_hgs_creation.py` gains an optional `"stamp": true` in `config.json`. It appends `[unix_ms, argv]` to `stamped.ndjson` beside `log.ndjson`, which keeps its format for the existing tests.
- [ ] **Step 3: Smoke hook.** `ZERUS_CLIENT_QUERY=newsession-smoke=1` starts only in replay mode with an explicit `ZERUS_CLIENT_HGS` and `ZERUS_CLIENT_PERF`, as `actions-smoke`. It runs three passes through the store the dialog uses. For each pass it logs `zerus-client perf: newsession-smoke <pass> begin <unix_ms>` and `… end <unix_ms>`; for passes 2 and 3 also `… start <unix_ms>` right before `creation_submit`:
  1. open, choose the replay's project folder, switch to Git repository and back, choose an account, open Choose existing…, then Cancel;
  2. open, None, Start; wait until the card reads `confirming` or binds, logging its captions;
  3. open, Auto on a Git catalog, wait until Start is enabled, Start; wait as in pass 2.
- [ ] **Step 4: Smoke run**, documented in the README with the headless environment of the actions smoke. Put `fake_hgs_creation.py` as `hgs` in a scratch folder with a `config.json` with `"stamp": true` and `replies` answering:
  - `account` with one claude profile;
  - `worktrees` with an `ok` catalog of one main checkout;
  - `dirs` with the folder;
  - `ls` with a listing whose session carries the launch id the run reports;
  - `--help` with `guarded-create-v1` and `repository-clone-v1`;
  - the agent with exit 0.

  The run must assert, with a short Python check over `stamped.ndjson` and the hook's stderr lines:
  - **open plus Cancel sends only reads.** Between pass 1's begin and end there is no write, no clone, no worktree and no launch. Only `account …` reads (`ls` and usage), `worktrees --path …` and `--help` lines appear. In particular there is no `worktrees create`, `dirs --create`, `clone`, `account start`, `account favorite`, `swarm` or argv containing `--new`.
  - **no worktree is created before Start.** Every `worktrees create` line is stamped after pass 3's `start`, and there is exactly one. Pass 2 has none. Pass 2 adds `dirs --creation`, one launch with `--expect-machine` and the binding `ls --json`. Pass 3 adds `worktrees --creation`, one `worktrees create`, its receipt read, one launch and the binding `ls --json`.

  Record the executed counts, the assertion output and the logs' hashes in the PR description.
- [ ] **Step 5: Run every check:**
  - `cargo +1.99.1 test` and clippy (`-D warnings`) in `client/core` and `client/src-tauri`;
  - `python3 scripts/collect-licenses.py "$TMP/client-licenses" --manifest-path client/src-tauri/Cargo.toml`;
  - `npx vitest run`, `npm run check`, `python3 scripts/check-ui-separators.py`, `python3 scripts/check-source-privacy.py`.
- [ ] **Step 6: Commit** `docs(client): New session commands, cards, state and smoke run`.

---

## Touch points with the client owner session (webui)

All additive; approved by webui on 2026-10-28 for the dialog. Send the creation-card rows of this table and the final list before the PR.

| File | Change |
|---|---|
| `client/src/ui/components/Rail.tsx` | the New button (32) calls `openNewSession({ computer: selectedHostTarget() })` instead of the toast |
| `client/src/ui/sessions/SessionList.tsx` | the dialog: the header add button (398) is enabled with the same call; `headerMenu(project)`'s "New session…" (141) calls `openNewSession({ project, computer: selectedHostTarget() })` |
| `client/src/ui/sessions/SessionList.tsx` | the cards: (a) in `ListRow` (516), one `<Show when={row()?.type === "creation"}>` branch rendering `<CreationCard>` (ours), the single insertion point for rows; (b) the footer's "Session creation status" button (440) is enabled and opens `creationStatusMenu()`; the footer count (437) appends `creationFooter()`. Nothing else |
| `client/src/ui/sessions/model.ts` | the cards: the `Row` union gains `{ type: "creation"; id; creation }`; `rowHeight` gives it the card height; `listRows` ends with `return withCreations(out, scope)` instead of `return out` |
| `client/src/App.tsx` | mounts `<NewSessionDialog />` beside `<ActionDialogs />`; binds `session.new` when the shortcut registry is absent (webui rule 1); the detail pane shows `<CreationPanel />` instead of `<SessionDetail />` while a card is selected (one `<Show>`) |
| `client/src/ui/screens/Projects.tsx` | the folder menu entry (47) and the Folders button (163) call `openFolderSession`, with `folderLaunchProblem` as their reason; the screen takes `requestedProject` once, as Machines takes `requestedMachine` |
| `client/src/ui/screens/reasons.ts` | `NEW_SESSION` goes when unused |
| `client/src/store/projects.ts` | `requestedProject` and `requestProject(id)` |
| `client/src/store/actions.ts` | exports `notify` (the notices' own function) for "This project folder changed. Select it again." and the creation notices |
| `client/src/store/catalog.ts`, `types.ts` | `Session.gitRoot`, `Session.cwdCanonical`, `Details.cwd_canonical` (G1) |
| `client/src/bridge.ts`, `types.ts` | the new commands and types |
| `client/src-tauri/src/screens.rs` | `launch_accounts` on the hub; the hub's account reads and usage queue are shared, not duplicated |
| `client/src-tauri/src/lib.rs` | registers the commands; manages `NewSession` and `CreationHost` with `data_dir(app)` (webui rule 2); the journal flush in the one exit hook (webui rule 3) |
| `client/core/src/swarm/edit.rs` (organization stream) | `Intent::AddFolder`, sent through its Controller |

New modules owned by this plan:
- `store/newSession.ts`, `store/creations.ts`;
- `ui/sessions/newsession-ui.ts`, `ui/sessions/opencode-models.ts`;
- `NewSessionDialog.tsx`, `ModelPicker.tsx`, `WorktreePicker.tsx`, `NewWorktreeDialog.tsx`, `CreationCard.tsx`, `CreationPanel.tsx`;
- `ui/components/FolderBrowser.tsx`, `ui/styles/newsession.css`;
- `src-tauri/src/newsession.rs`, `src-tauri/src/creations.rs`.

## Core additions, answers and open questions

**Trivial and additive, done in this plan** (the events stream reviews the core lines in the PR):
- **G1:** `ViewRow.git_root`, `ViewRow.cwd_canonical` (`view.rs`) and `Details.cwd_canonical` (`details.rs`). The prefill uses the canonical folder (2924), and the worktree picker counts sessions by them (`WorktreePanel.h:135-139`).
- **G2:** `ProjectsView.swarm_id` (`views/projects.rs`), the swarm the folder check's project inference must match (2922, 355-361); `Folder.machine_id` (Task 2), the folder's computer identity that `loadMachines` reads (495).
- **G3:** `views::accounts::launchable` public (this computer without a platform takes the build's, `MachinePlatform::of`), `host_profiles(entry, catalog, state, agents)` returning one computer's launchable profiles with their raw rows, and `refresh_groups`, for `launch_accounts`.
- **G4:** `launch::own_windows_session()`, this process's Windows session (`ProcessIdToSessionId`, `LocalSessionHost.cpp`; windows-sys feature `Win32_System_RemoteDesktop`), None on other systems. `other_windows_session` takes it as `own`.

**Answered by the events stream (2026-10-28):**
- **Q1, OpenCode model, favorite and service start:** not in the core. Task 1 adds them beside `launch.rs`'s invocations, following Qt's calls; the events stream does not edit `launch.rs`.
- **Q2, `dirs --create` with its receipt:** the same. Task 1 adds it next to `launch::directories_invocation` and `creation::creation_directory_invocation`.
- **Q3, launch binding:** the watch stream carries neither `launch_id` nor `ended_launches`. Qt takes both from `hgs [@host] ls --json` (`HgsClient.cpp:184, 279`; `src/state/launches.rs`). The queue already has the inputs (`Creations::for_launch`, `complete`, `launch_finished`, `ended`). Task 5 adds a bounded `ls --json` read for a computer only while one of its operations waits for binding, with the parser in `creation.rs`. The events stream files carrying both on watch-v1 as a follow-up.
- **Q4, project and folder edits** (answered by webui): Qt's creation goes through the organization model, not raw `requestSwarm`.
  - `SessionsWindow.cpp:1564` and `:3054` call `m_organization.addFolder(group, machine, path, {}, style)`.
  - Project creation and the move are `SessionOrganization` edits.
  - `SwarmController` sends them all as one declarative, version-guarded `swarm apply`; raw `requestSwarm` is only `SwarmDialog`'s preview, sync, join and resolve. A second writer calling `swarm apply` or project commands would race the controller.

  The client port lives on the organization branch `work-e94411a3-org`:
  - `client/core/src/swarm/edit.rs`: `Intent::{Move, Create{id, name, members}, Rename, Delete, Reorder, Sort, Renamed}`;
  - `client/core/src/swarm/sync.rs`: the Controller (one apply at a time, 120 ms flush, a draft that survives restarts);
  - `src-tauri/src/organization.rs`: the `edit` command and `Pump::edited(&Intent)`.

  So the creation host:
  1. creates a project with `Intent::Create`, and moves the new session with `Intent::Move` once its member id exists;
  2. adds `Intent::AddFolder { project, machine, path, name, style }` in `edit.rs`, a port of `SessionOrganization::addFolder` (`tray/src/SessionOrganization.cpp:258`), with its tests, sent through the same Controller;
  3. waits for `Controller::ready()` as every edit does: before the first state, edits are refused or held, never sent.
- **Q5, the terminal window:** use the core's `terminal_launch::Launcher::open_new_session` (Qt's `makeLaunchTerminal` choice: Konsole tab or raise, xdg-terminal-exec, Terminal.app, Windows Terminal) with its advisory text; no stand-in.

**New questions:**
- **Q6 (webui, organization):** `creation::Placement::resolve` blocks until it can answer, and treats an edit whose outcome is unknown as an error (`creation.rs`). With the Controller's durable draft, is an edit's outcome:
  - the `edit` command's acceptance into the draft, so the creation continues;
  - or the applied `swarm apply`, so the creation waits for it?

  Proposal: acceptance into the draft, since the draft survives restarts and the Controller owns delivery. A refused intent is an error.
- **D6 (design session):** Qt texts with dots in the creation cards:
  - the panel title "{tag} · {caption} · {m}:{ss}" (`SessionsWindow.cpp:3742`). Proposal: the tag as the title, with "{caption} ({m}:{ss})" under it;
  - the status menu entries "{tag} · {caption}" (472). Proposal: "{tag}: {caption}".

  Also two presentation gaps:
  - Qt pages older creations into an "Older creations…" submenu every 50 entries (471). The client's menus have no submenus. Proposal: a heading "Older creations…" before each further 50, as the batch menu shows machines as headings.
  - Qt lists matching creations in its separate search results (`SessionSearch.cpp:287-310`). The client filters the list in place. Proposal: a card stays in the list when its details contain the query (Task 6).
- **D7 (design session):** Should the remembered choices (agent, account per computer and agent, Open terminal) be seeded once from Qt's `workspace/launchAgent`, `workspace/launchAccounts` and `workspace/launchOpenTerminal`, read-only? Or does the client start from Qt's defaults? (webui rule 4.)

## Owner rulings and deviations from Qt

Ruled by the coordinator on 2026-10-28 (Task 2): option B, the scoped port of the pure path rules and the three folder helpers to `paths.ts`, pinned by a shared golden fixture that a core Rust test and vitest both check, with `Context.build_style` added in Rust; `creationSubmit(plan)` in `bridge.ts` calling `creation_submit` (Task 5 registers it), and a hook in `newSession.ts` that hands the admitted id to `store/creations.ts`.

Ruled by the design session on 2026-10-28, verbatim:
> D1 yes, the creation cards (rows, progress, Continue/Check/Stop/Hide/Edit, the status menu) go in the same milestone PR, since without them an admitted creation is invisible; D2 yes, five launch options now, the executor-only "Project changed…" path arrives with the Tasks view; D3 approved: the official tauri-plugin-dialog, Rust side only, pinned in Cargo.toml and Cargo.lock, with the license check passing (scripts/collect-licenses.py if it covers src-tauri, say which check), the dialog opened from Rust, never by giving the webview the plugin's JS permission; D4 approved: "Default, Favorite, Free" and "{provider} (this folder)"; D5 yes, keep " (offline)".

How the plan applies them:
- **D1:** Task 6.
- **D2:** the Goal, and Task 3's Start texts. The foreground path's texts wait for the Tasks view:
  - "The computer did not confirm this folder. Try again." (348);
  - "The repository could not be confirmed. Choose a working folder again." (350);
  - "Creating worktree…" and "Checking folder…" on the error line and Start (372, 695, 781);
  - "Cloning…", "Checking…" and "Use checkout and start" (696-697);
  - "The project changed. The checkout is available at {path}. Choose a project to use it." (647);
  - "Checkout saved. Select its computer and folder to start a session." (654);
  - "Project changed. Select the checkout again before starting." (737);
  - the clone recovery controls (196).
- **D3:** Task 1. The license check is `scripts/collect-licenses.py` with `--manifest-path client/src-tauri/Cargo.toml`. It covers `src-tauri` only with that argument: CI's "Rust license notices" step runs it for the root workspace.
- **D4:** Tasks 2 and 4.
- **D5:** Tasks 2 and 3.

Deviations from Qt:
1. Background creation only (ruling D2).
2. No pending clone record exists in the client, because only the foreground clone saves one (`RepositoryClone.cpp:83-93`). So "A previous clone is unconfirmed. Check its original result before continuing." (`RepositoryClone.cpp:37`), Check result and Choose another destination… never show in the dialog (196, 711-713).
3. New worktree's absolute-path check and its destination suggestion use the selected computer's path style (core `paths.rs`). Qt's `QDir::isAbsolutePath`, `QDir::filePath` and `QFileInfo` use this computer's style (`NewWorktreeDialog.h:33, 67, 71`). N1 made the same choice for Auto (`launch::automatic_worktree_available`).
4. The remembered launch choices live in the events core settings store under Qt's key names, never in QSettings (webui rule 4). Until that store is on the base, they are not remembered.
5. The dot separators of ruling D4, and of D6 once ruled.
6. Creation cards under a search query: kept in place when their details match (D6), since the client has no separate search results list.
7. The folder path rules and three folder helpers have a TypeScript copy (`paths.ts`), the scoped exception above; the shared fixture keeps it equal to the core.
8. The dialog keeps at most 64 verified worktree catalogs (Qt keeps them for the dialog's life without a bound).

## Order and handoff

| Order | Task | Base | Depends on | Waits for |
|---|---|---|---|---|
| 1 | Rust surface (`newsession.rs`, G1-G4, Q1, Q2) | `integrate/client-core-2` | — | the settings store for `remembered` only |
| 2 | Store and rules | `integrate/client-core-2` | 1 | — |
| 3 | The dialog and entry points | `integrate/client-core-2` | 2 | the shortcut registry, else the fixed default |
| 4 | Child dialogs | `integrate/client-core-2` | 2 (parallel with 3) | — |
| 5 | Creation host and admission | main after the organization PR lands (webui says when) | 1 | Q6 |
| 6 | Creation cards | main after the organization PR lands | 5 | D6 for the title and menu texts |
| 7 | README and smoke | main | 3, 4, 6 | D7 for the README's settings line |

Each task gets a spec review against the Qt lines quoted for it, then a quality review; the branch gets a whole-branch review before the PR. One milestone PR (`zerus-5wmh.10.10`), rebased on main after #424 and the organization PR, merged on a green "CI gate". After the merge:
- the merge commit goes to the owner session, which rebuilds the Mac dev app;
- the design session hears that New session is ready for the owner to try, with D6, D7 and Q6 if still open.
