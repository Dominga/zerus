# Tauri Client: Sessions Grouped by the Shared Organization, and the Project Organisation Actions

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Sessions list groups sessions by the shared project organization, with Qt's order, Inbox and unassigned rules and collapsed state (Beads `zerus-5wmh.31`). Its project menus run Qt's organisation actions: Move to project (and New project), Rename project, Project settings, Move project up and down, Sort sessions by name and Delete project (`zerus-5wmh.18`). Every write is a guarded `hgs swarm apply {base, desired, versions}` with SwarmController's rules, so the Qt GUI and this client can edit the same replicated organization side by side.

**Architecture:**
- **Source:** the core's `organization` topic consumer (PR #395, `zerus-5wmh.30`). It delivers `Change::Organization { computer, update: OrganizationUpdate::State { value, digest, membership } | Failed | Ended }`, and `Fleet::organization_snapshot(computer)` gives the `OrganizationSnapshot { digest, base, versions }` of one digest. The core polls `swarm get` itself when a stream lacks the topic. This client never polls.
- **Pure core, `client/core/src/swarm/`:**
  - `mod.rs`: member keys, membership and the default project;
  - `edit.rs`: the menus' edits as changes of the `organization` object, which become the desired side of `swarm apply`;
  - `layout.rs`: this device's collapsed projects, session order and default project;
  - `grouping.rs`: each row's project and rank, and the project headers, sent only when they change;
  - `sync.rs`: the write controller, a port of `tray/src/SwarmController.cpp`.
- **Tauri:**
  - `pump.rs` owns the grouping beside its row mirror and puts `Batch.groups` and `Batch.grouping` on each batch. It strips `Batch.organizations`, so the organization value never reaches the webview.
  - `organization.rs` feeds the controller from the topic and runs `hgs swarm apply` on its own thread. It saves the layout and exposes the edit commands.
- **Frontend:**
  - `store/organization.ts` holds the per-row assignments and the headers.
  - `model.ts` groups rows with Qt's visibility rules.
  - `OrganizationDialogs.tsx` holds Move to project, New project, Rename project and Delete project.
  - The header menu and the session menu's Move to project entry use them.

> **Decisions (this plan's, 2026-10-28; Qt wins wherever it is not named here).**
> - **Membership** comes from the catalog as `organization.projects[].sessions[]`. Its keys are `"<host>\n<session name>"`, where the host is this computer's host name or the peer's alias, which equals `ViewRow::machine`. A row that no project lists is an implicit member of the default project (Qt's "unassigned" fallback row). Such a row is never sent as membership, until an edit moves it.
> - **Default project:**
>   - this device's choice when it is listed;
>   - else the catalog's `default_project` when it is listed;
>   - else the first project;
>   - else the Inbox (`ungrouped`).
>
>   This device's choice changes only when Delete project removes the default project (`SessionOrganization::removeGroup`). Setting a default is 3c.
> - **Local layout:** the collapsed state, the order of sessions inside a project and the default project are local, as in Qt. They live in `organization-layout.json` in the app data directory, because Qt's QSettings cannot be shared (deviation 1 below). The project order is shared (`project_order`).
> - **Edits run while the catalog is ready.** Before that, the entries show disabled with the reason. Qt queues edits made before its first reply. This client refuses them until its first state, also on top of a draft restored from the last run (deviation 4).
> - **Never `swarm initialize`.** This client has no GUI cache to import. An initialize from it would mark the catalog imported and skip the Qt GUI's one-time import (`src/swarm/mod.rs:212-215`, `SwarmController.cpp:86-88`). With `initialized: false`, edits stay disabled. Task 10 files the follow-up for computers without the Qt GUI.
> - **A failed or timed-out apply is sent again, as in Qt** (coordinator ruling, 2026-10-28; it withdraws this plan's first "never sent again" rule):
>   - **Qt:** a failure leaves `m_base`, `m_desired` and `m_versions` as they were (`SwarmController.cpp:67-73`), and so does a reply of another schema (194-196). The next 5 s `refresh` sends `apply` again from them (55, 96-101; `send` 102-110). An edit made meanwhile restarts the 120 ms flush (94), which sends the same base and versions with the newer desired state.
>   - **Why this is not a replay:** it is reconciliation toward a desired state, not the repeat of a side effect. `swarm apply` writes each field that differs between base and desired, with the observed versions as parents (`src/swarm/model.rs:126-201`; `catalog.rs:418-444`). A duplicate of a diff that already landed adds a head with the same value, and the snapshot reports conflicts only for differing values (`model.rs:298-312`), so it is a no-op for the list. A stale base surfaces as a conflict instead of clobbering anything. The spec's never-replay rule covers non-idempotent actions (send, interrupt, terminate), and dropping the user's move on a network blip would regress from Qt.
>   - **Here:** the edits stay pending and the list keeps showing them. The next tick sends one apply with the same base, desired and versions. The tick is 5 s after the failure (Qt's poll), or the organization source's next state, whichever comes first. After a resend fails again, only the 5 s tick sends it, so a refused apply goes out at most once per 5 s. That source is this client's read tick: the topic, or the core's fallback `swarm get` poll, which delivers its reads as the same states. One apply runs at a time, and no state is adopted under pending edits (204-206).
>   - **Answers:** each edit is answered by the first apply that carries it, with `Done`, or `Kept` and Qt's status line. A later resend shows its result in the status line, as Qt's `statusChanged` does (Global Constraints).
>   - **Nothing pending is dropped.** Qt keeps every failed draft, and hgs exits 1 for every swarm error (`src/swarm/mod.rs:168-173`), so a validation refusal cannot be told from a transient failure. The client's own checks (`edit::apply`) refuse an invalid edit before it is queued, and the reason is the notice. Pending edits end in two ways: an apply succeeds, or they return to the base, which settles them without a call (Qt's `m_base==m_desired`, 100). Quitting does not end them (next rule).
>   - As in Qt, a draft whose versions the catalog no longer knows fails on every tick ("unknown field version; reload the catalog", `src/swarm/catalog.rs:431`), and the status line shows it.
> - **The pending draft persists, as in Qt** (coordinator ruling, 2026-10-28):
>   - **Qt:** `saveOrganization` saves the draft with every change (`SessionsWindow.cpp:2496`), and `persist` saves the observed base and versions (`SwarmController.cpp:111-130`, called at 72, 205, 206 and 209). At start Qt loads the draft (`SessionsWindow.cpp:275-276`) and calls `start` with it (1661). `start` takes the draft as `m_desired` and the saved base and versions (`SwarmController.cpp:78, 81-83`), and its first call is a `get` (88), never an `apply`. That reply makes it ready (197) and keeps the base, since it was a read (203). With the draft pending, the reply is not adopted, and the flush sends the draft 120 ms later from the saved base and versions (204-205).
>   - **Here:** the controller's base, desired state and versions go to `organization-draft.json` in the app data directory, beside `organization-layout.json` (`organization-draft-replay.json` in a replay). The file is written atomically, through a temporary file and a rename, as `client/core/src/readmarks.rs:117-140` does. The hub writes it whenever pending edits change, before their apply runs, and removes it when they settle.
>   - **On start:** the hub loads the draft, and the list shows it at once. Nothing is sent before the first organization state. That state is not adopted under the draft; it makes the controller ready and starts the 120 ms flush. Then one apply carries the saved base, desired state and versions, so a catalog that changed meanwhile surfaces as a conflict through `swarm apply`'s merge, never as an overwrite. After that, the rules above apply.
>   - A replay starts from the fixture's catalog, so it removes a leftover replay draft at start.
> - **A rename keeps the project.** While the client runs, the same run under a new name moves the old name's explicit membership to the new name, as Qt's `renameSession` does. It is one apply, and only when the old name was explicit. With Qt and this client side by side, both migrate the same rename (Qt on its own rename, `SessionsWindow.cpp:1942`, and on observed ones by run token, `SessionOrganization::observe`, `SessionOrganization.cpp:223-230`). Task 4 tests that the second migration is a no-op or a clean merge. Qt also follows renames made while it was closed, by persisted run tokens; that is a follow-up (Task 10).
> - **Move to project takes a set of members.** It is one `swarm apply` from one snapshot. The session menu passes one member; the batch menu of `zerus-5wmh.20` passes the selection.
> - **Sort sessions by name** compares titles case-folded, then as written, then by member (deviation 2).
> - **Headers and colours:** a header's colour is the project's `color`, else its palette slot, as on 3a's Projects screen. The vivid style and local colours are 3c.
> - **Header counts stay in the webview** (a deviation from the spec, ruling F19, 2026-10-28). The spec has Rust project the list rows and their header counters ("Parsing never happens in the webview", spec lines 79-82). Here Rust sends each row's project and rank and the headers (`Batch.groups`, `Batch.grouping`), and `groupRows` counts each header's attention, unread and working members in the webview, as main does. The counts follow the webview's state and machine filters, which live there; counting in Rust would need those filters there too. The counts are one memoized pass over the listed rows (`listRows`); the members are grouped again only when row ids or assignments change, never on a status-only batch.
> - **Deviations from Qt.** The coordinator accepted 1 and 2 as deviations, and 3 to 5 are part of the approved plan (2026-10-28):
>   1. This device's layout (collapsed projects, the order inside each project, the default project, and since `zerus-5wmh.40` the run memory below) lives in `organization-layout.json` in the app data directory. Qt keeps it in QSettings (`workspace/organization`, `SessionsWindow.cpp:2486-2500`).
>      - **Run memory (coordinator ruling, 2026-10-29, Qt parity).** Qt's `run_groups` (`SessionOrganization.cpp:60, 193, 234-242, 336`) maps `machine\nrun` to the project a live session of that run was last placed in. A live, assigned session updates its run's entry; an unassigned one removes it. An archived row that the shared organization does not list goes to its run's remembered project when that project still exists, otherwise to the default project, unassigned. The client keeps the same map in `organization-layout.json`. Qt never evicts it; the client caps it at 20,000 entries and drops the oldest written first (repository rule: no unbounded per-session state).
>      - **Seed at the Mac switch.** On the switch identity's first start the map is seeded once, read-only, from Qt's organization JSON `run_groups`, as the settings seed is, so existing archives keep their projects. Qt's file is never written.
>   2. Sort sessions by name folds case, then compares as written, then by member. Qt uses `QString::localeAwareCompare` (`SessionsWindow.cpp:2603-2608`), which has no equivalent in the core.
>   3. No `swarm initialize` (above).
>   4. Edits made before the first ready catalog are refused, not queued (above).
>   5. Renames made while the client was closed are not followed yet (Task 10 follow-up).
>   6. Closed by `zerus-5wmh.40` (coordinator ruling, 2026-10-29): archived sessions are placed as Qt places them. An archived row is keyed by its archive identity, `machine\narchive\n<archive id>` (`SessionOrganization.cpp:225`); its project is the shared organization's entry for that identity, else its run's remembered project (deviation 1's run memory), else the default project, unassigned. After a verified Terminate whose run is archived, the archive identity moves into the live session's project in one swarm apply (`SessionsWindow.cpp:3106-3125`). The Archive view groups by project (newest archived first, `3238-3242`), the project filter and counts apply under Archive, Clear archive works under a project filter, and Move to project moves the archive identity. Split: the placement, the run memory, the post-Terminate intent and the `archive_page` group and rank go to the core owner; the Archive view and `zerus-5wmh.45` to the UI. Until it lands the interim refusals stay:
>      - the Archive view and its count ignore the project filter (`store/archive.ts` `inScope`);
>      - Clear archive takes no row while a project filter is set, and says why;
>      - Move to project is disabled for an archived row, and the store refuses one.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` ("Actions", "Parsing never happens in the webview"). The model for this plan is `docs/superpowers/plans/2026-10-28-tauri-client-phase2-actions.md`. The behavioural reference is the Qt GUI; every rule below names its lines (verified on main `1f388605`):
- `tray/src/SessionOrganization.h:11-12` (missing and offline sessions keep their place), `:16` (`Group.collapsed`).
- `tray/src/SessionOrganization.cpp`:
  - the constructor 24-77: the 80-character name at 29, sessions deduplicated across projects at 46-49, `General` when there are no projects at 52-53, and the default at 55-58;
  - `groupFor` 87-90, `indexSessions` 98-104, `orderedMembers` 114-131;
  - `createGroup` 133-145, `renameGroup` 147-152, `removeGroup` 154-166, `moveGroup` 174-185, `moveSession` 187-201;
  - `renameSession` 203-221, `observe` 223-245, `toJson` 326-338.
- `tray/src/SwarmController.h:17-19` (`busy`, `ready`, `settled`).
- `tray/src/SwarmController.cpp`:
  - `sharedOrganization` 22-51;
  - the 5 s poll 55, 62-63 and 85;
  - the 120 ms flush 64-65;
  - failure 67-73, `start` 76-89, `edit` 90-95, `refresh` 96-101, `send` 102-110;
  - `presentation` 131-189: collapsed 141, fallback rows 142-147, local order 148-150, shared order 166-171, local fields 188;
  - `received` 191-219: the request check 193, schema 194-196, ready 197, `written` 199-202, base 203, pending 204-206, adoption 207-209, status 211-218.
- `tray/src/HgsClient.cpp:706-719` (`requestSwarm`: local only, 10 s, compact JSON input, "Invalid project catalog response").
- `tray/src/SessionsWindow.cpp`:
  - background menu 349-353, header click 518-521, drops 556-561, start 1641-1661 (the controller's status line goes to the Projects page, 1653), rename 1940-1943;
  - `saveOrganization` 2486-2500 (the draft saved at 2496), `revealSession` 2547-2556, `createGroup` 2558-2566;
  - `showGroupMenu` 2568-2616, with Rename 2582-2587, Project settings 2588-2589, up and down 2591-2600, Sort 2601-2611 and Delete 2613-2614;
  - entries 3206-3215, the sort 3238-3243, observe 3244, the archive placement 3106-3125;
  - Move to project 5363-5372.
- `tray/src/SessionsWindowRows.cpp`: `hideEmptyProjects` 109, the project of a row 279, row building 289-338 (`hasGroups` 293, `filtering` 294, the empty-project rule 323, the header 324, hidden members 326), and header counts 393-411.
- `tray/src/ProjectsDialog.cpp:367-381` (`deleteProject`), `:173` (the default checkbox).
- `tray/src/SessionPresentation.h:203-207` (`sessionLabel`).
- hgs:
  - `src/swarm/mod.rs:206-233`: read stdin, apply, save, `written`; `:168-173`: every swarm error exits 1 with `hgs: <message>`;
  - `src/swarm/model.rs`: `flatten` 39-124, `apply` 126-201 (the explicit-assignment rule 146-157, the write with the observed parents 197-198), conflicts only for differing head values 298-312, the snapshot 324-336 (`default_project` is always `ungrouped`, ordered by `project_order` then ID);
  - `src/swarm/catalog.rs:369-394` (concurrent Inbox and default markers against a concrete project), `:418-444` (`write` appends an operation with the given parents).

**Prerequisites, on main before Task 1:**
- PR #391 (machine cards), PR #393 (the server topic) and PR #395 (the core consumer: `Change::Organization`, `OrganizationUpdate`, `OrganizationSnapshot`, `Fleet::follow_organization`, `Fleet::organization_snapshot` and `Batch.organizations`).
- The phase 3a base commit: `client/core/src/views/mod.rs` and `client/core/src/views/projects.rs`, as `docs/superpowers/plans/2026-10-28-tauri-client-phase3a-screens.md` Tasks 1-2 specify them.
- Task 9 also needs 3a's hub (`client/src-tauri/src/screens.rs`, 3a Task 3) and Projects screen (3a Task 8). If they are not on the branch when Task 9 runs, do the rest and hand Task 9 to the 3a session.
- If a name differs on main at execution time, follow main.

## Global Constraints

- "Actions keep today's guarded CLI calls and their rules: request identities, expected run and conversation, destructive confirmations with the safe choice focused, and **unknown outcomes are never replayed automatically**. The client adds no new action semantics." (spec, Actions)
  - That rule covers non-idempotent actions (send, interrupt, terminate). `swarm apply` is a declarative three-way merge guarded by versions, and Qt sends a failed one again on its next refresh. The client does the same (coordinator ruling, 2026-10-28; Decisions).
- Rust toolchain 1.99.1 (`cargo +1.99.1`, `rust-version = "1.99"`); `client/core` stays pure Rust without Tauri.
- The new UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); `scripts/check-ui-separators.py` enforces it, also in `.rs` files under `client/`.
- No obvious keyboard hints in the UI (owner rule, round 6): no `kbd` on menu entries, no "Enter to …" texts.
- Never run the real `hgs` against live sessions or the real catalog in tests; tests use the fake `hgs` scripts in `client/core/tests/support`.
- Headless GUI runs only with `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...` (`client/README.md`, "Headless runs").
- Repository text in English; fixtures synthetic (reserved example domains, `/home/example/...`, `claude/sample-project/review`).
- **Scale:** session counts reach tens of thousands.
  - The grouping is computed in Rust from the topic's `membership`: one pass over the members and the rows per presented organization.
  - The webview gets only the rows whose project or rank changed (`Batch.groups`) and the headers when they changed (`Batch.grouping`). The organization value never goes to the webview.
  - Bounds: at most 500 projects (3a's bound), the layout remembers at most 100,000 members, and there are no per-row timers.
  - The frontend groups rows once per change of row ids or assignments, never on a status update.
  - Task 3 sizes and tests it for 20,000 sessions.
- **The 20,000-row reconcile cost must not regress noticeably.**
  - Measure it as phase 2 did: one replace of 20,000 rows into `createCatalog()`, wall time and heap after GC. Phase 2 measured 277 ms and +49 MB.
  - Report the grouping's own cost beside it (Task 10). Grouping state stays out of the catalog's row objects.
- **SwarmController's apply rules, ported exactly** (`client/core/src/swarm/sync.rs`; owner coordinator ruling, 2026-10-28):
  - **Ready and busy gating** (`SwarmController.h:17-19`; `.cpp:65, 94, 98, 104`). One `swarm apply` at a time. Nothing is sent before a ready catalog, and nothing while one runs. Once ready, the controller stays ready, as `m_ready` does. The end of the topic refuses new edits, but pending edits are still sent.
  - **The 120 ms debounced flush** (`.cpp:64-65`). Each edit restarts it while idle (94), also after a failure, and a reply with pending edits starts it again (205).
  - **A failure keeps the edits, and the next tick sends them again** (`.cpp:67-73, 96-101, 194-196`):
    - a non-zero exit, a timeout, a signal, an unreadable reply or a reply of another schema changes only the status. Base, desired and versions stay, and so does the presented organization;
    - the next tick sends one apply from them, exactly as Qt's `refresh` would. The tick is `REFRESH_MS` (5 s, `.cpp:55`) after the failure, or the organization topic's next state, whichever comes first. After a resend fails again, only the 5 s tick sends it, so a refused apply goes out at most once per 5 s;
    - edits made during the failed apply ride on that resend. Edits made after it flush 120 ms later, from the same base and versions.
  - **The pending draft persists** (`SessionsWindow.cpp:275-276, 1661, 2496`; `SwarmController.cpp:76-89, 111-130, 191-206`):
    - base, desired and versions are saved in `organization-draft.json` whenever pending edits change, through a temporary file and a rename (`client/core/src/readmarks.rs:117-140`), and removed when they settle;
    - a restart restores them. Nothing is sent before the first organization state, which is not adopted under the draft: it makes the controller ready and starts the flush, as Qt's first `get` reply does (197, 203-205);
    - the apply then carries the saved base and versions, never the newer state's, so a catalog that changed meanwhile gives a conflict, not an overwrite.
  - **`initialize` versus `get`** (`.cpp:81-88, 100, 197, 203`). Qt's first call is `initialize` when it holds no observed baseline. This client never initializes (Decisions). Its baseline is the first observed state, and an uninitialized catalog keeps edits disabled.
  - **Base, desired and versions observed together** (`.cpp:105, 108, 207-209`). Base and versions come from one `OrganizationSnapshot` of the update's digest, or from one `swarm apply` or `swarm get` reply. They are never combined across states. The request's base is what the user saw: a newer state is not merged under a pending edit.
  - **Never apply from a stale base:**
    - a state is not adopted while edits are pending or running (`.cpp:204-206`);
    - after an acknowledged write, a state that does not yet show the written operations is older than the base held, and is not adopted for 10 s (the topic can deliver a read made before the write after its reply; Qt's sequential requests drop such answers at 193);
    - a failed apply leaves the base the edits started from, so its resend carries the base the user saw.
  - **`written` advances only acknowledged fields** (`.cpp:199-202`). Fields another writer changed meanwhile keep the versions the user saw, so hgs reports a conflict instead of an overwrite. A failure advances nothing.
  - **Conflict and review states** (`.cpp:211-218`). The status keeps Qt's precedence and texts. An apply's outcome counts the conflicts on the fields it wrote, and the client shows "N project conflict(s) need review in Swarm.". Resolving them is the Swarm dialog's (3c).
  - **The status line keeps Qt's texts verbatim**, with Qt's literal "(s)" plurals (only a Russian translation exists, `tray/translations/zerus_ru.ts`):
    - from `received()` (212-218), through 3a's `status_of`, by precedence: "N project conflict(s) need review in Swarm.", then "N connection(s) unavailable. Changes are saved here and will sync when reachable.", then "Local catalog. Connect another computer to sync projects." without peers, else "Project sync connected to N computer(s)" (no final full stop, as in Qt). Conflicts and unavailable connections are errors;
    - the schema check (195): "Project sync needs an updated HGS installation.";
    - the failure path (69): "Project sync unavailable: <error>". The error is the client runner's text where Qt's runner has its own (`failureDetail`, `HgsClient.cpp:466-479`): the standard error trimmed to 4000 characters, "hgs exited with code N", "hgs did not answer in time", "hgs ended abnormally", "hgs did not take the request", the start error, or Qt's "Invalid project catalog response" for unreadable output (`HgsClient.cpp:713`);
    - the status reaches the Projects screen, as Qt's `statusChanged` reaches the Projects page (`SessionsWindow.cpp:1653`; Task 9), and each edit's answer (Task 7).
- **Send intent; hgs resolves it.** The client edits only the lists the user asked for. It never decides placement, concurrent Inbox and default markers or causal heads: those stay in hgs and the replica, which have had subtle bugs (`src/swarm/catalog.rs:369-394`; project-placement memory, 2026-10-22).
- Writes run on this computer only: `hgs swarm apply` without `@host` (`HgsClient.cpp:708-711`), since the catalog itself is shared.
- **Shared files** (the menu-parity session, `claude/zerus-work-1a04708c/menu-parity`, edits the other session-menu entries in parallel): changes to them are additive only, as new functions, entries and enum variants, never restructuring. See "Shared files" below.

## Review Focus

1. **A second writer between the client's read and its apply** (Task 4, `a_second_writer_between_read_and_apply_gives_a_conflict_not_an_overwrite`):
   - the apply carries the base and versions the user saw;
   - the newer state that arrived meanwhile is not merged under the edit;
   - the outcome reports the conflict hgs recorded, and the list shows where hgs placed the session;
   - a late older state is not adopted, the next read is, and the next edit applies from it.
2. **An apply that times out stays pending, and the next tick sends it once more** (Task 4, `a_timed_out_apply_stays_pending_and_the_next_tick_sends_it_once_more`):
   - the edit is answered `Kept` with "Project sync unavailable: hgs did not answer in time", the status line shows that text as an error, and the list keeps showing the move;
   - nothing is sent before the tick (`REFRESH_MS` after the failure). At the tick, exactly one apply goes out, with the same base, desired and versions as the first;
   - while it runs, a newer state is not adopted and sends nothing, and `due` sends nothing;
   - the fake's log is exactly `get, apply, apply`, with no apply in between and none while the second one is in flight;
   - beside it: the topic's next state as the tick (`a_state_after_a_failed_apply_is_the_tick_that_sends_it_again`), a refusal sent once per tick until it lands (`a_refused_apply_is_kept_and_sent_once_per_tick_until_it_lands`), an edit after a failure (`an_edit_after_a_failure_flushes_120_ms_later_from_the_same_base`), and the hub's resend on the next state (Task 6, `a_failed_apply_answers_kept_and_the_next_state_sends_it_once_more`).
3. **Edits during a running apply** (Task 4 unit tests): one follow-up apply from what was sent. Only the written fields advance their versions, and edits 120 ms apart are one apply. Edits made during a failed apply ride on its resend.
4. **No `swarm initialize` ever:** a catalog with `initialized: false` keeps every write entry disabled with its reason (Task 4).
5. **20,000 sessions** (Task 3):
   - the first organization sends 20,000 assignments once;
   - moving one session sends one, with no headers;
   - the batch JSON never contains the organization;
   - a status-only batch does not regroup the frontend (Task 7).
6. **Sort sessions by name on the Inbox** makes every listed fallback row an explicit Inbox member: one apply with all of them. This is Qt's behaviour (`moveSession` clears the unassigned marker, `SessionOrganization.cpp:192`; `SessionsWindow.cpp:2609`). Approved by the coordinator (2026-10-28).
7. **A rename observed in the catalog** writes the old name's explicit project for the new name, without a person's click, as Qt does (approved, 2026-10-28). A rename of a fallback row writes nothing. **Two writers migrating the same rename** (Qt and this client side by side; Task 4, `a_rename_the_other_writer_already_migrated_sends_nothing` and `a_rename_the_other_writer_migrates_between_read_and_apply_merges_cleanly`):
   - when the catalog already shows the migration, the client's migration changes nothing shared and sends no apply;
   - when the other writer migrates between the client's read and its apply, the apply carries the base and versions the client read, and hgs's reply holds same-value concurrent heads;
   - either way there is no conflict, and the new name is listed once, in the old name's project.
8. **Texts without a Qt source** (the coordinator approved them on 2026-10-28; the two that served only the withdrawn no-resend design, "Checking the last project change…" and "The change may have been saved. The list shows the projects as they are now.", are gone):
   - "Project sync is starting.";
   - "Project sync is not set up on this computer yet. Start Zerus here once to import its projects.";
   - "This project is no longer listed. Open the menu again.";
   - "Choose the sessions to move.";
   - "Project sync stopped." (a worker that is gone, and now also the reason new edits are refused after the topic ended);
   - "Enter a project name.".
9. **Failure answers use Qt's texts:** a kept edit's answer and the status line say "Project sync unavailable: <error>" or "Project sync needs an updated HGS installation.", as Qt's failure path and schema check do. The Projects screen shows the controller's status line (Task 9).
10. **The pending draft survives a restart** (coordinator ruling, 2026-10-28; Task 4, `a_pending_draft_survives_a_restart_and_is_sent_once_after_the_first_state` and `a_restored_draft_against_a_newer_catalog_gives_a_conflict_not_an_overwrite`):
    - an edit is pending and its draft is on disk; the client stops, and a new controller loads the draft and shows it;
    - no apply goes out before the first state, also after 20 s;
    - after the first state, exactly one apply goes out, with the saved base, desired state and versions, and the settled draft file is removed;
    - when the first state shows a newer catalog, the apply still carries the saved base and versions, and the outcome is hgs's conflict status, "1 project conflict(s) need review in Swarm.", not an overwrite;
    - beside them: the draft follows pending edits and is removed when they settle (`the_draft_follows_pending_edits_and_is_removed_when_they_settle`), a restored draft waits for the first state (`a_restored_draft_is_shown_and_waits_for_the_first_state`), the file is replaced whole (`draft.rs` tests), and the hub loads it at start and sends it once after the first state (Task 6, `a_saved_draft_waits_for_the_first_state_then_one_apply_sends_it`).

---

## File Structure

```
client/core/src/swarm/mod.rs          member(), projects(), members(), default_project(), INBOX
client/core/src/swarm/edit.rs         Intent, apply(), shared(), changes_shared(), clean_name()
client/core/src/swarm/layout.rs       Layout: collapsed, ranks, default project; JSON load and save
client/core/src/swarm/grouping.rs     Grouping, GroupOut, HeaderOut, GroupingOut, SyncView, by_name()
client/core/src/swarm/sync.rs         Controller (SwarmController), Observation, EditOutcome, Due, Reply, classify(), invocation()
client/core/src/swarm/draft.rs        Draft, DraftWrite, load(), store(), write(): the pending draft file, written atomically
client/core/src/batch.rs              Batch.groups, Batch.grouping (additive)
client/core/src/lib.rs                pub mod swarm; re-exports
client/core/src/views/projects.rs     3a extensions: pub(crate) status_of; SwarmState::set_default, set_status
client/core/tests/swarm_sync.rs       Controller against a fake hgs: a second writer, a timed-out apply and its resend,
                                      a refused apply per tick, a rename both writers migrate, and a draft across a restart
client/core/tests/support/fake_hgs_swarm.py   scripted swarm get and swarm apply replies, logged
client/src-tauri/src/pump.rs          grouping beside the mirror; strips organizations; drain_with()
client/src-tauri/src/organization.rs  Organization hub, worker thread, commands, layout and draft files
client/src-tauri/src/commands.rs      follows the organization, routes its changes, the snapshot source
client/src-tauri/src/replay.rs        follows the replay's organization; organization_snapshot()
client/src-tauri/src/lib.rs           manages the hub, registers the commands
client/src-tauri/src/screens.rs       3a change: Projects shows the hub's organization and status line, no swarm get
client/src/types.ts                   GroupOut, HeaderOut, GroupingOut, EditOutcome; Batch fields
client/src/bridge.ts                  moveToProject, createProject, renameProject, deleteProject, moveProject,
                                      sortProject, setProjectCollapsed
client/src/store/organization.ts      createOrganization(): assignments, headers, edits, notices
client/src/store/organization.test.ts
client/src/store/organization.perf.test.ts   the 20,000-row measurement (ZERUS_CLIENT_MEASURE=1)
client/src/store/session.ts           receive() applies the grouping
client/src/store/actions.ts           exports notify (additive)
client/src/store/ui.ts                drops the name-derived projectOrder and collapsed stores
client/src/ui/sessions/model.ts       groupMembers(), groupRows(), listRows, projectChoices
client/src/ui/sessions/model.test.ts
client/src/ui/sessions/organization-ui.ts      headerMenuState(), deleteTargets(), moveChoices(), projectColor(), nameProblem()
client/src/ui/sessions/organization-ui.test.ts
client/src/ui/sessions/OrganizationDialogs.tsx Move to project, New project, Rename project, Delete project; moveEntry()
client/src/ui/sessions/SessionList.tsx         headers, header menu, the Move to project entry
client/src/ui/sessions/organize.ts             drops: project moves and session moves through the organization
client/src/ui/screens/Projects.tsx             3a change: shows the project Project settings asks for
client/src/App.tsx                    mounts OrganizationDialogs
client/src/dev/replay.ts              the browser replay's grouping
client/src/ui/PerfHost.tsx            the organization-smoke=1 hook
client/scripts/make-fixture.py, client/fixtures/*   the replay organization
client/README.md                      "Projects in the Sessions list" section
```

## Shared files

Every change below is additive or confined to code this plan owns. A merge with the menu-parity branch touches different hunks.

| File | Change | Owner of the rest |
|---|---|---|
| `client/src/ui/sessions/SessionList.tsx` | imports; the `Header` and `StripBand` bodies; `headerMenu` (owned); the local `moveProject` and `projectColor` helpers go; in `sessionMenu`, the one line `{ label: "Move to project…", icon: "folder", disabled: LATER },` becomes `moveEntry(s),`; FilterChips shows a project chip by its name. The SelectionBar's Move to project button stays as it is, for `zerus-5wmh.20` | menu-parity: the other `sessionMenu` entries, `batchMenu`, SelectionBar |
| `client/src/store/actions.ts` | `notify` added to the object `createActions` returns and to the export line, one entry each. If menu-parity merged an exported `notify` first, reuse it | menu-parity |
| `client/src/ui/sessions/ActionDialogs.tsx` | none (the organization dialogs live in `OrganizationDialogs.tsx`) | menu-parity |
| `client/src/ui/sessions/actions-ui.ts` | none | menu-parity |
| `client/core/src/actions.rs` | none (`swarm::sync` reuses `Exit` and `Invocation`) | menu-parity |
| `client/src-tauri/src/actions.rs` | none | menu-parity |
| `client/src/ui/sessions/organize.ts` | `placement` reads a row's project with `groupOf`; `applyDrop` sends drops through the organization; the bulk-selection half is untouched | menu-parity (`zerus-5wmh.20`) |
| `client/src/ui/PerfHost.tsx` | a new `organization-smoke=1` hook beside `actions-smoke` | both |
| `client/src/App.tsx` | one import and `<OrganizationDialogs />` after `<ActionDialogs />` | both |
| `client/core/src/batch.rs`, `client/core/src/lib.rs`, `client/src-tauri/src/pump.rs`, `client/src-tauri/src/commands.rs` | additive fields, modules and methods on top of PR #395 | events session |
| 3a files: `client/core/src/views/projects.rs`, `client/src-tauri/src/screens.rs`, `client/src/ui/screens/Projects.tsx` | the extensions named in Tasks 4 and 9 | phase 3a |

Interfaces for `zerus-5wmh.20` (batch Move to project, disabled with "Arrives with project organisation." until this plan merges):

```rust
// client/src-tauri/src/organization.rs
#[tauri::command]
pub async fn move_to_project(state: State<'_, Organization>, members: Vec<String>, project: String, before: Option<String>) -> Result<EditOutcome, String>;
```

```ts
// client/src/bridge.ts
export async function moveToProject(members: string[], project: string, before: string | null): Promise<EditOutcome>;
// client/src/store/organization.ts
export const memberOf: (s: Pick<Session, "machine" | "name">) => string;   // "<host>\n<session name>"
export function moveToProject(rows: readonly Pick<Session, "machine" | "name">[], projectId: string, before?: Pick<Session, "machine" | "name">): Promise<EditOutcome>;
export const grouping: Accessor<GroupingOut>;   // grouping().ready, grouping().reason for the disabled entry
// client/src/ui/sessions/OrganizationDialogs.tsx
export function openMoveDialog(rows: readonly Session[]): void;   // the project picker, then one moveToProject(rows, id)
```

---

### Task 1: Members, the default project and the edits (`client/core/src/swarm/{mod,edit}.rs`)

**Files:**
- Create: `client/core/src/swarm/mod.rs`, `client/core/src/swarm/edit.rs`
- Create (one line each, `//! Task 2.`, `//! Task 3.`, `//! Task 4.`, `//! Task 4.`, so this task compiles alone): `client/core/src/swarm/layout.rs`, `grouping.rs`, `sync.rs`, `draft.rs`
- Modify: `client/core/src/lib.rs` (`pub mod swarm;` after `pub mod state;`)
- Test: unit tests in `mod.rs` and `edit.rs`

**Interfaces:**
- Consumes: the `organization` object of `swarm get` (`OrganizationSnapshot::base`, or `value["organization"]` of an `OrganizationUpdate::State`).
- Produces:

```rust
// swarm/mod.rs
pub const INBOX: &str = "ungrouped";
pub const INBOX_NAME: &str = "General";
pub const INBOX_COLOR: &str = "#8d9baa";
pub const MAX_PROJECTS: usize = 500;
pub fn member(machine: &str, name: &str) -> String;
pub fn projects(organization: &Value) -> &[Value];
pub fn project_id(project: &Value) -> &str;
pub fn sessions(project: &Value) -> impl Iterator<Item = &str>;
pub fn listed(organization: &Value, id: &str) -> bool;
pub fn default_project(organization: &Value, local: Option<&str>) -> String;
pub fn members(organization: &Value) -> BTreeMap<String, String>;
// swarm/edit.rs
pub enum Intent { Move { members, project, before }, Create { id, name, members }, Rename { project, name },
                  Delete { project, destination, members }, Reorder { project, before }, Sort { project, members },
                  Renamed { from, to } }
pub fn apply(organization: &Value, intent: &Intent) -> Result<Value, String>;
pub fn shared(organization: &Value) -> Value;
pub fn changes_shared(base: &Value, desired: &Value) -> bool;
pub fn clean_name(name: &str) -> Option<String>;
```

- [ ] **Step 1: Write the failing tests.**

At the end of `swarm/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn organization() -> Value {
        json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "sessions": ["workstation\nclaude/sample-project/review", "studio\ncodex/aurora/orders-v3"]},
            {"id": "beta", "name": "Beta", "sessions": ["workstation\nclaude/sample-project/review"]},
            {"id": "ungrouped", "name": "Inbox", "sessions": []}]})
    }

    #[test]
    fn a_member_is_the_machine_and_the_session_name() {
        assert_eq!(member("workstation", "claude/sample-project/review"), "workstation\nclaude/sample-project/review");
    }

    #[test]
    fn the_first_project_that_lists_a_member_keeps_it() {
        let members = members(&organization());
        assert_eq!(members["workstation\nclaude/sample-project/review"], "alpha");
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn the_default_project_follows_qt() {
        let org = organization();
        assert_eq!(default_project(&org, None), "ungrouped");
        assert_eq!(default_project(&org, Some("beta")), "beta");
        assert_eq!(default_project(&org, Some("gone")), "ungrouped");
        let without = json!({"default_project": "ungrouped", "projects": [{"id": "alpha"}, {"id": "beta"}]});
        assert_eq!(default_project(&without, None), "alpha");
        assert_eq!(default_project(&json!({"projects": []}), None), INBOX);
        assert_eq!(default_project(&json!(null), None), INBOX);
    }
}
```

At the end of `swarm/edit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::{project_id, projects, sessions};
    use serde_json::json;

    const R: &str = "workstation\nclaude/sample-project/review";
    const O: &str = "studio\ncodex/aurora/orders-v3";
    /// A fallback row: no project lists it.
    const N: &str = "workstation\nkimi/docs-site/landing";

    fn org() -> Value {
        json!({"version": 2, "default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true,
             "folders": [{"id": "f1", "machine": "workstation", "machine_id": "m-1", "path": "/home/example/src/sample-project", "name": ""}],
             "sessions": [R]},
            {"id": "beta", "name": "Beta", "color": "#64b5f6", "accessible": true, "folders": [], "sessions": [O]},
            {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": []}]})
    }

    fn lists(org: &Value) -> Vec<(String, Vec<String>)> {
        projects(org).iter().map(|p| (project_id(p).to_owned(), sessions(p).map(str::to_owned).collect())).collect()
    }

    fn ids(org: &Value) -> Vec<String> {
        projects(org).iter().map(|p| project_id(p).to_owned()).collect()
    }

    fn moving(members: &[&str], project: &str) -> Intent {
        Intent::Move { members: members.iter().map(|m| (*m).to_owned()).collect(), project: project.into(), before: None }
    }

    #[test]
    fn a_move_takes_every_member_out_of_its_project_and_appends_them_in_order() {
        let next = apply(&org(), &moving(&[O, N, R, O], "alpha")).unwrap();
        assert_eq!(lists(&next), [
            ("alpha".to_owned(), vec![O.to_owned(), N.to_owned(), R.to_owned()]),
            ("beta".to_owned(), vec![]),
            ("ungrouped".to_owned(), vec![]),
        ]);
        assert!(changes_shared(&org(), &next));
        let same = apply(&org(), &moving(&[R], "alpha")).unwrap();
        assert!(!changes_shared(&org(), &same), "a move inside its own project changes only the local order");
        assert_eq!(apply(&org(), &moving(&[R], "gone")), Err(GONE.to_owned()));
        assert_eq!(apply(&org(), &moving(&[], "alpha")), Err(NOTHING.to_owned()));
    }

    #[test]
    fn a_new_project_goes_before_the_inbox_and_takes_its_members() {
        let id = "0f0e0d0c-0b0a-4908-8706-050403020100";
        let next = apply(&org(), &Intent::Create { id: id.into(), name: "  Release work  ".into(), members: vec![N.into()] }).unwrap();
        assert_eq!(ids(&next), ["alpha", "beta", id, "ungrouped"]);
        let created = &projects(&next)[2];
        assert_eq!(created["name"], "Release work");
        assert_eq!(created["color"], NEW_COLOR);
        assert_eq!(created["sessions"], json!([N]));
        let long = apply(&org(), &Intent::Create { id: "id-2".into(), name: "x".repeat(90), members: vec![] }).unwrap();
        assert_eq!(projects(&long)[2]["name"].as_str().unwrap().chars().count(), NAME_CHARS);
        assert_eq!(apply(&org(), &Intent::Create { id: "id-3".into(), name: "   ".into(), members: vec![] }), Err(NAME_NEEDED.to_owned()));
        let no_inbox = json!({"projects": [{"id": "alpha", "sessions": []}]});
        let last = apply(&no_inbox, &Intent::Create { id: "id-4".into(), name: "Last".into(), members: vec![] }).unwrap();
        assert_eq!(ids(&last), ["alpha", "id-4"]);
        assert!(changes_shared(&org(), &next));
        assert_eq!(shared(&next)["project_order"], json!(["alpha", "beta", id, "ungrouped"]));
    }

    #[test]
    fn rename_trims_and_keeps_eighty_characters() {
        let next = apply(&org(), &Intent::Rename { project: "beta".into(), name: " Orders ".into() }).unwrap();
        assert_eq!(projects(&next)[1]["name"], "Orders");
        assert_eq!(apply(&org(), &Intent::Rename { project: "beta".into(), name: " ".into() }), Err(NAME_NEEDED.to_owned()));
        assert_eq!(apply(&org(), &Intent::Rename { project: "gone".into(), name: "x".into() }), Err(GONE.to_owned()));
    }

    #[test]
    fn delete_moves_its_members_and_the_listed_rows_into_the_destination() {
        let next = apply(&org(), &Intent::Delete { project: "beta".into(), destination: "alpha".into(), members: vec![O.into(), N.into()] }).unwrap();
        assert_eq!(lists(&next), [
            ("alpha".to_owned(), vec![R.to_owned(), O.to_owned(), N.to_owned()]),
            ("ungrouped".to_owned(), vec![]),
        ]);
        assert_eq!(projects(&next)[0]["folders"], org()["projects"][0]["folders"], "the receiver keeps its folders");
        for (project, destination) in [("beta", "beta"), ("beta", "gone"), ("gone", "alpha")] {
            let intent = Intent::Delete { project: project.into(), destination: destination.into(), members: vec![] };
            assert_eq!(apply(&org(), &intent), Err(GONE.to_owned()), "{project} into {destination}");
        }
    }

    #[test]
    fn reorder_follows_move_group() {
        let up = apply(&org(), &Intent::Reorder { project: "beta".into(), before: Some("alpha".into()) }).unwrap();
        assert_eq!(ids(&up), ["beta", "alpha", "ungrouped"]);
        let last = apply(&org(), &Intent::Reorder { project: "alpha".into(), before: None }).unwrap();
        assert_eq!(ids(&last), ["beta", "ungrouped", "alpha"]);
        let unknown = apply(&org(), &Intent::Reorder { project: "alpha".into(), before: Some("gone".into()) }).unwrap();
        assert_eq!(ids(&unknown), ["alpha", "beta", "ungrouped"], "moveGroup ignores an unknown anchor");
        assert!(changes_shared(&org(), &up));
        assert_eq!(shared(&up)["project_order"], json!(["beta", "alpha", "ungrouped"]));
    }

    #[test]
    fn sort_makes_every_listed_member_explicit() {
        let inbox = apply(&org(), &Intent::Sort { project: "ungrouped".into(), members: vec![N.into()] }).unwrap();
        assert_eq!(lists(&inbox)[2], ("ungrouped".to_owned(), vec![N.to_owned()]));
        assert!(changes_shared(&org(), &inbox));
    }

    #[test]
    fn a_rename_keeps_an_explicit_members_project() {
        const R2: &str = "workstation\nclaude/sample-project/review-2";
        let next = apply(&org(), &Intent::Renamed { from: R.into(), to: R2.into() }).unwrap();
        assert_eq!(lists(&next)[0], ("alpha".to_owned(), vec![R2.to_owned()]));
        let implicit = apply(&org(), &Intent::Renamed { from: N.into(), to: R2.into() }).unwrap();
        assert!(!changes_shared(&org(), &implicit), "a fallback row's rename changes nothing shared");
        // Qt migrated it first: the old name is no member, so the second migration changes nothing.
        let again = apply(&next, &Intent::Renamed { from: R.into(), to: R2.into() }).unwrap();
        assert!(!changes_shared(&next, &again), "a migration another writer made is a no-op");
        // The old explicit name wins over the new name's own project (SessionOrganization.cpp:208-215).
        let both = apply(&org(), &Intent::Renamed { from: R.into(), to: O.into() }).unwrap();
        assert_eq!(lists(&both), [
            ("alpha".to_owned(), vec![O.to_owned()]),
            ("beta".to_owned(), vec![]),
            ("ungrouped".to_owned(), vec![]),
        ]);
    }

    #[test]
    fn the_request_marks_membership_explicit_and_carries_the_shared_order() {
        let request = shared(&org());
        assert_eq!(request["session_assignments_explicit"], true);
        assert_eq!(request["project_order"], json!(["alpha", "beta", "ungrouped"]));
        assert_eq!(request["projects"], org()["projects"]);
        let mut cached = org();
        cached["unassigned_sessions"] = json!([N]);
        assert!(shared(&cached).get("unassigned_sessions").is_none());
    }
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core swarm::`. Expected: compile errors (`swarm` not found).

- [ ] **Step 3: Implement.** `client/core/src/lib.rs` gains `pub mod swarm;` after `pub mod state;`.

`client/core/src/swarm/mod.rs`:

```rust
//! The shared project organization in the Sessions list (`hgs swarm`), on
//! top of the core's `organization` topic (`crate::organization`):
//!
//! - [`member`], [`members`] and [`default_project`]: Qt's membership rules
//!   (tray/src/SessionOrganization.cpp);
//! - [`edit`]: the menus' edits, as the desired side of `swarm apply`;
//! - [`layout`]: this device's collapsed projects, session order and default;
//! - [`grouping`]: each row's project and rank and the headers, sent to the
//!   webview only when they change;
//! - [`sync`]: the write controller, a port of tray/src/SwarmController.cpp;
//! - [`draft`]: its pending edits on disk, so a restart sends them again.
//!
//! Pure Rust: the Tauri hub (client/src-tauri/src/organization.rs) runs
//! `hgs swarm apply` and passes the clock in. Placement itself (concurrent
//! Inbox and default markers against a concrete project, causal heads) is
//! resolved by `hgs swarm apply` and the replica (src/swarm/model.rs:126-201,
//! src/swarm/catalog.rs:369-394), never here.
pub mod draft;
pub mod edit;
pub mod grouping;
pub mod layout;
pub mod sync;

use serde_json::Value;
use std::collections::BTreeMap;

/// The Inbox: the project of sessions no project lists, unless this device
/// or the catalog names another (SessionOrganization.cpp:52-58).
pub const INBOX: &str = "ungrouped";
/// The Inbox's name and colour when the catalog lists no project at all
/// (SessionOrganization.cpp:53).
pub const INBOX_NAME: &str = "General";
pub const INBOX_COLOR: &str = "#8d9baa";
/// The most projects the list shows (phase 3a's bound).
pub const MAX_PROJECTS: usize = 500;

/// A session as `organization.projects[].sessions[]` names it: the machine
/// (this computer's host name or the peer's alias, as `ViewRow::machine`)
/// and the session name (SessionOrganization.cpp:225).
pub fn member(machine: &str, name: &str) -> String {
    format!("{machine}\n{name}")
}

/// The listed projects, in the catalog's order.
pub fn projects(organization: &Value) -> &[Value] {
    organization
        .get("projects")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub fn project_id(project: &Value) -> &str {
    project.get("id").and_then(Value::as_str).unwrap_or("")
}

/// A project's explicit members.
pub fn sessions(project: &Value) -> impl Iterator<Item = &str> {
    project
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

pub fn listed(organization: &Value, id: &str) -> bool {
    projects(organization).iter().any(|project| project_id(project) == id)
}

/// The project of sessions no project lists (SessionOrganization.cpp:55-58):
/// this device's choice when listed, then the catalog's `default_project`
/// when listed, then the first project, then the Inbox.
pub fn default_project(organization: &Value, local: Option<&str>) -> String {
    if let Some(id) = local.filter(|id| listed(organization, id)) {
        return id.to_owned();
    }
    let catalog = organization
        .get("default_project")
        .and_then(Value::as_str)
        .unwrap_or(INBOX);
    if listed(organization, catalog) {
        return catalog.to_owned();
    }
    projects(organization)
        .iter()
        .map(project_id)
        .find(|id| !id.is_empty())
        .unwrap_or(INBOX)
        .to_owned()
}

/// Every explicit member's project. A member listed by two projects stays in
/// the first (indexSessions, SessionOrganization.cpp:98-104); `swarm apply`
/// refuses such a catalog (src/swarm/model.rs:107-110), so a valid snapshot
/// lists each member once.
pub fn members(organization: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for project in projects(organization) {
        for member in sessions(project) {
            out.entry(member.to_owned())
                .or_insert_with(|| project_id(project).to_owned());
        }
    }
    out
}
```

`client/core/src/swarm/edit.rs`:

```rust
//! The organisation edits of the Sessions list, as changes of `swarm get`'s
//! `organization` object: the desired side of `hgs swarm apply {base,
//! desired, versions}`. Each follows SessionOrganization's rule for the
//! shared fields (tray/src/SessionOrganization.cpp). `swarm apply` diffs base
//! and desired field by field and resolves concurrent heads
//! (src/swarm/model.rs:126-201), so an edit only changes the lists the user
//! asked for. The order of sessions inside a project is not shared
//! (SwarmController.cpp:148-150): it lives in [`super::layout`].
use super::{listed, members, project_id, projects, sessions, INBOX};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The longest project name (SessionOrganization.cpp:29, 135, 150).
pub const NAME_CHARS: usize = 80;
/// A new project's colour (SessionOrganization.cpp:142).
pub const NEW_COLOR: &str = "#64b5f6";
pub const GONE: &str = "This project is no longer listed. Open the menu again.";
pub const NAME_NEEDED: &str = "Enter a project name.";
pub const NOTHING: &str = "Choose the sessions to move.";

/// One edit. Members are `"<host>\n<session name>"` keys ([`super::member`]).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intent {
    /// Move to project, a drop and a selection's move: the members, in order,
    /// into `project` as explicit members (moveSession, 187-201). `before`
    /// places them in the local order only.
    Move { members: Vec<String>, project: String, before: Option<String> },
    /// New project before the Inbox (createGroup, 133-145), then the members
    /// into it (SessionsWindow::createGroup, SessionsWindow.cpp:2558-2566).
    Create { id: String, name: String, members: Vec<String> },
    /// renameGroup (147-152).
    Rename { project: String, name: String },
    /// removeGroup (154-166): its members and the listed rows it showed go
    /// to `destination`.
    Delete { project: String, destination: String, members: Vec<String> },
    /// moveGroup (174-185): before `before`, or last.
    Reorder { project: String, before: Option<String> },
    /// Sort sessions by name: every listed member, already in order, as an
    /// explicit member (SessionsWindow.cpp:2601-2611).
    Sort { project: String, members: Vec<String> },
    /// The same run under a new name (renameSession, 203-221). When another
    /// writer migrated it already, `from` is no member and nothing changes.
    Renamed { from: String, to: String },
}

/// The name SessionOrganization stores: trimmed, then at most 80 characters.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(NAME_CHARS).collect();
    (!name.is_empty()).then_some(name)
}

fn projects_mut(organization: &mut Value) -> Option<&mut Vec<Value>> {
    organization.get_mut("projects").and_then(Value::as_array_mut)
}

fn position(organization: &Value, id: &str) -> Option<usize> {
    projects(organization).iter().position(|project| project_id(project) == id)
}

/// Takes members out of every project (moveSession's removal, 193-195).
fn take(organization: &mut Value, gone: &BTreeSet<&str>) {
    for project in projects_mut(organization).into_iter().flatten() {
        if let Some(list) = project.get_mut("sessions").and_then(Value::as_array_mut) {
            list.retain(|member| !member.as_str().is_some_and(|member| gone.contains(member)));
        }
    }
}

/// The members, in order, into the project at `at` as explicit members.
fn move_into(organization: &mut Value, members: &[String], project: &str) -> Result<(), String> {
    let at = position(organization, project).ok_or(GONE)?;
    let mut seen = BTreeSet::new();
    let unique: Vec<&str> = members
        .iter()
        .map(String::as_str)
        .filter(|member| !member.is_empty() && seen.insert(*member))
        .collect();
    take(organization, &seen);
    let target = &mut projects_mut(organization).ok_or(GONE)?[at];
    if !target.get("sessions").is_some_and(Value::is_array) {
        target["sessions"] = json!([]);
    }
    let list = target["sessions"].as_array_mut().ok_or(GONE)?;
    list.extend(unique.into_iter().map(|member| Value::String(member.to_owned())));
    Ok(())
}

/// The organization after `intent`, or why it cannot be made.
pub fn apply(organization: &Value, intent: &Intent) -> Result<Value, String> {
    let mut next = organization.clone();
    match intent {
        Intent::Move { members, project, .. } => {
            if members.is_empty() {
                return Err(NOTHING.into());
            }
            move_into(&mut next, members, project)?;
        }
        Intent::Create { id, name, members } => {
            let name = clean_name(name).ok_or(NAME_NEEDED)?;
            if !listed(&next, id) {
                let at = position(&next, INBOX).unwrap_or(projects(&next).len());
                let project = json!({"id": id, "name": name, "color": NEW_COLOR, "accessible": true,
                                     "folders": [], "sessions": []});
                match projects_mut(&mut next) {
                    Some(list) => list.insert(at, project),
                    None => next["projects"] = json!([project]),
                }
            }
            if !members.is_empty() {
                move_into(&mut next, members, id)?;
            }
        }
        Intent::Rename { project, name } => {
            let name = clean_name(name).ok_or(NAME_NEEDED)?;
            let at = position(&next, project).ok_or(GONE)?;
            projects_mut(&mut next).ok_or(GONE)?[at]["name"] = json!(name);
        }
        Intent::Delete { project, destination, members } => {
            if project == destination || !listed(&next, destination) {
                return Err(GONE.into());
            }
            let at = position(&next, project).ok_or(GONE)?;
            let removed = projects_mut(&mut next).ok_or(GONE)?.remove(at);
            let mut displaced: Vec<String> = sessions(&removed).map(str::to_owned).collect();
            for member in members {
                if !displaced.contains(member) {
                    displaced.push(member.clone());
                }
            }
            if !displaced.is_empty() {
                move_into(&mut next, &displaced, destination)?;
            }
        }
        Intent::Reorder { project, before } => {
            let anchor = before.as_deref();
            if anchor == Some(project.as_str()) || anchor.is_some_and(|id| !listed(&next, id)) {
                return Ok(next);
            }
            let from = position(&next, project).ok_or(GONE)?;
            let list = projects_mut(&mut next).ok_or(GONE)?;
            let moving = list.remove(from);
            let to = anchor
                .and_then(|id| list.iter().position(|p| project_id(p) == id))
                .unwrap_or(list.len());
            list.insert(to, moving);
        }
        Intent::Sort { project, members } => move_into(&mut next, members, project)?,
        Intent::Renamed { from, to } => {
            if from == to || from.is_empty() || to.is_empty() || !members(&next).contains_key(from) {
                return Ok(next);
            }
            take(&mut next, &BTreeSet::from([to.as_str()]));
            for project in projects_mut(&mut next).into_iter().flatten() {
                if let Some(list) = project.get_mut("sessions").and_then(Value::as_array_mut) {
                    for member in list.iter_mut().filter(|member| member.as_str() == Some(from.as_str())) {
                        *member = json!(to);
                    }
                }
            }
        }
    }
    Ok(next)
}

/// What `swarm apply` carries for an organization (sharedOrganization,
/// SwarmController.cpp:22-51): membership is explicit and the shared project
/// order is the listed one. This client never lists fallback rows as
/// members, and the catalog never holds this computer's Windows folders, so
/// nothing else is filtered.
pub fn shared(organization: &Value) -> Value {
    let mut out = organization.clone();
    if let Some(object) = out.as_object_mut() {
        object.remove("unassigned_sessions");
        object.insert("session_assignments_explicit".into(), json!(true));
        let order: Vec<Value> = projects(organization).iter().map(|p| json!(project_id(p))).collect();
        object.insert("project_order".into(), Value::Array(order));
    }
    out
}

type Shape = (Vec<(String, Value, Value, Value)>, BTreeMap<String, String>);

fn shape(organization: &Value) -> Shape {
    let fields = projects(organization)
        .iter()
        .map(|p| (project_id(p).to_owned(), p["name"].clone(), p["color"].clone(), p["folders"].clone()))
        .collect();
    (fields, members(organization))
}

/// Whether two organizations differ in what `swarm apply` writes: the
/// projects, their names, colours and folders, their order and the
/// membership. The order of sessions inside a project is local.
pub fn changes_shared(base: &Value, desired: &Value) -> bool {
    shape(base) != shape(desired)
}
```

- [ ] **Step 4: Run** the tests, then `cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core --all-targets -- -D warnings` and `cargo +1.99.1 fmt --manifest-path client/core/Cargo.toml -- --check`. Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/swarm client/core/src/lib.rs
git commit -m "feat(client): organization members, the default project and Qt's project edits"
```

---

### Task 2: This device's layout of the list (`client/core/src/swarm/layout.rs`)

**Files:**
- Modify: `client/core/src/swarm/layout.rs` (replacing the stub)
- Test: unit tests in `layout.rs`

**Interfaces:**
- Produces:

```rust
pub const GAP: u64 = 1 << 20;
pub const MAX_MEMBERS: usize = 100_000;
pub struct Layout;
impl Layout {
    pub fn from_json(bytes: &[u8]) -> Self;            // unreadable: empty
    pub fn to_json(&self) -> Vec<u8>;
    pub fn take_dirty(&mut self) -> Option<Vec<u8>>;   // serialized once after a change
    pub fn collapsed(&self, project: &str) -> bool;
    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) -> bool;
    pub fn default_project(&self) -> Option<&str>;
    pub fn set_default_project(&mut self, project: Option<String>);
    pub fn rank(&mut self, project: &str, member: &str) -> u64;
    pub fn place(&mut self, project: &str, members: &[String], before: Option<&str>);
    pub fn sort(&mut self, project: &str, members: &[String]);
    pub fn rename(&mut self, from: &str, to: &str);
    pub fn forget(&mut self, project: &str);
    pub fn prune(&mut self, keep: impl Fn(&str) -> bool);
    pub fn len(&self) -> usize;
}
```

Rules:
- **Ranks:** a member's rank is its place inside its project, with gaps of `GAP`.
  - A member new to a project goes last (`observe`, `SessionOrganization.cpp:236`; `presentation`, `SwarmController.cpp:149-150`).
  - A member keeps its rank while it stays in the project, also while its row is missing or offline (`SessionOrganization.h:11-12`).
  - An insert before a member takes the middle of the gap; without room, the project is renumbered first. Only moved members change rank otherwise, so a move sends few rows.
- **Collapsed** projects and the **default project** are this device's (`SwarmController.cpp:141, 188`).
- Past `MAX_MEMBERS`, the members that are neither listed nor in the catalog are forgotten.

- [ ] **Step 1: Write the failing tests** (at the end of `layout.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|m| (*m).to_owned()).collect()
    }

    fn order(layout: &mut Layout, project: &str, members: &[&str]) -> Vec<String> {
        let mut ranked: Vec<(u64, String)> = members.iter().map(|m| (layout.rank(project, m), (*m).to_owned())).collect();
        ranked.sort();
        ranked.into_iter().map(|(_, m)| m).collect()
    }

    #[test]
    fn new_members_go_last_and_keep_their_rank() {
        let mut layout = Layout::default();
        assert_eq!(layout.rank("alpha", "a"), GAP);
        assert_eq!(layout.rank("alpha", "b"), 2 * GAP);
        assert_eq!(layout.rank("alpha", "a"), GAP, "a member keeps its place");
        assert_eq!(layout.rank("beta", "a"), GAP, "moved by the catalog: last in its new project");
        assert_eq!(layout.rank("alpha", "c"), 3 * GAP, "gaps stay; nothing is renumbered");
    }

    #[test]
    fn a_drop_takes_the_middle_and_renumbers_only_without_room() {
        let mut layout = Layout::default();
        for m in ["a", "b", "c"] {
            layout.rank("alpha", m);
        }
        layout.place("alpha", &names(&["c"]), Some("b"));
        assert_eq!(order(&mut layout, "alpha", &["a", "b", "c"]), ["a", "c", "b"]);
        assert_eq!(layout.rank("alpha", "c"), GAP + GAP / 2);
        for n in 0..40 {
            layout.place("alpha", &[format!("x{n}")], Some("c"));
        }
        assert_eq!(order(&mut layout, "alpha", &["a", "x39", "c", "b"]), ["a", "x39", "c", "b"]);
        layout.place("beta", &names(&["a"]), Some("missing"));
        assert_eq!(layout.rank("beta", "a"), GAP, "an anchor outside the project places last");
    }

    #[test]
    fn sort_appends_in_the_given_order() {
        let mut layout = Layout::default();
        for m in ["c", "a", "b", "kept"] {
            layout.rank("alpha", m);
        }
        layout.sort("alpha", &names(&["a", "b", "c"]));
        assert_eq!(order(&mut layout, "alpha", &["a", "b", "c", "kept"]), ["kept", "a", "b", "c"]);
    }

    #[test]
    fn a_rename_keeps_the_old_place() {
        let mut layout = Layout::default();
        for m in ["a", "b"] {
            layout.rank("alpha", m);
        }
        layout.rename("a", "a2");
        assert_eq!(order(&mut layout, "alpha", &["b", "a2"]), ["a2", "b"]);
    }

    #[test]
    fn the_layout_survives_a_restart_and_an_unreadable_file_starts_empty() {
        let mut layout = Layout::default();
        layout.rank("alpha", "b");
        layout.rank("alpha", "a");
        assert!(layout.set_collapsed("alpha", true));
        assert!(!layout.set_collapsed("alpha", true), "no change, nothing to save");
        layout.set_default_project(Some("alpha".into()));
        let saved = layout.take_dirty().unwrap();
        assert!(layout.take_dirty().is_none(), "saved once");
        let mut again = Layout::from_json(&saved);
        assert!(again.collapsed("alpha"));
        assert_eq!(again.default_project(), Some("alpha"));
        assert_eq!(order(&mut again, "alpha", &["a", "b"]), ["b", "a"]);
        assert!(again.take_dirty().is_none(), "loading is not a change");
        assert_eq!(Layout::from_json(b"not json").len(), 0);
    }

    #[test]
    fn forget_and_prune_bound_the_layout() {
        let mut layout = Layout::default();
        layout.rank("gone", "a");
        layout.set_collapsed("gone", true);
        layout.forget("gone");
        assert!(!layout.collapsed("gone"));
        assert_eq!(layout.len(), 0);
        for n in 0..MAX_MEMBERS + 10 {
            layout.rank("alpha", &format!("m{n}"));
        }
        layout.prune(|m| m == "m0");
        assert_eq!(layout.len(), 1);
        let mut small = Layout::default();
        small.rank("alpha", "a");
        small.prune(|_| false);
        assert_eq!(small.len(), 1, "under the cap nothing is pruned");
    }
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core swarm::layout`. Expected: compile errors.

- [ ] **Step 3: Implement** `client/core/src/swarm/layout.rs`:

```rust
//! This device's layout of the Sessions list: collapsed projects, the order
//! of sessions inside each project and the default project. Qt keeps them in
//! its local organization (`workspace/organization`, SessionsWindow.cpp:
//! 2486-2500; collapsed, SwarmController.cpp:141; order, 148-150; default,
//! 188) and never shares them. The Tauri side saves the JSON.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The space between neighbouring ranks; an insert takes the middle.
pub const GAP: u64 = 1 << 20;
/// The most members the layout remembers.
pub const MAX_MEMBERS: usize = 100_000;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    collapsed: Vec<String>,
    default_project: Option<String>,
    /// Each project's members in list order.
    order: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Layout {
    collapsed: BTreeSet<String>,
    default_project: Option<String>,
    order: HashMap<String, BTreeMap<u64, String>>,
    index: HashMap<String, (String, u64)>,
    dirty: bool,
}

impl Layout {
    pub fn from_json(bytes: &[u8]) -> Self {
        let saved: Saved = serde_json::from_slice(bytes).unwrap_or_default();
        let mut layout = Self {
            collapsed: saved.collapsed.into_iter().collect(),
            default_project: saved.default_project,
            ..Self::default()
        };
        for (project, members) in saved.order {
            for member in members {
                layout.append(&project, &member);
            }
        }
        layout.dirty = false;
        layout
    }

    pub fn to_json(&self) -> Vec<u8> {
        let order = self
            .order
            .iter()
            .map(|(project, ranks)| (project.clone(), ranks.values().cloned().collect()))
            .collect();
        let saved = Saved {
            version: 1,
            collapsed: self.collapsed.iter().cloned().collect(),
            default_project: self.default_project.clone(),
            order,
        };
        serde_json::to_vec(&saved).expect("the layout serializes")
    }

    /// The layout to save, once after each change.
    pub fn take_dirty(&mut self) -> Option<Vec<u8>> {
        std::mem::take(&mut self.dirty).then(|| self.to_json())
    }

    pub fn collapsed(&self, project: &str) -> bool {
        self.collapsed.contains(project)
    }

    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) -> bool {
        let changed = if collapsed {
            self.collapsed.insert(project.to_owned())
        } else {
            self.collapsed.remove(project)
        };
        self.dirty |= changed;
        changed
    }

    pub fn default_project(&self) -> Option<&str> {
        self.default_project.as_deref()
    }

    pub fn set_default_project(&mut self, project: Option<String>) {
        if self.default_project != project {
            self.default_project = project;
            self.dirty = true;
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// The member's rank in `project`: where it is, else last.
    pub fn rank(&mut self, project: &str, member: &str) -> u64 {
        if let Some((owner, rank)) = self.index.get(member) {
            if owner == project {
                return *rank;
            }
        }
        self.append(project, member)
    }

    fn remove(&mut self, member: &str) {
        if let Some((project, rank)) = self.index.remove(member) {
            if let Some(ranks) = self.order.get_mut(&project) {
                ranks.remove(&rank);
                if ranks.is_empty() {
                    self.order.remove(&project);
                }
            }
            self.dirty = true;
        }
    }

    fn append(&mut self, project: &str, member: &str) -> u64 {
        self.remove(member);
        let ranks = self.order.entry(project.to_owned()).or_default();
        let rank = ranks.keys().next_back().map_or(GAP, |last| last + GAP);
        ranks.insert(rank, member.to_owned());
        self.index.insert(member.to_owned(), (project.to_owned(), rank));
        self.dirty = true;
        rank
    }

    fn rank_in(&self, project: &str, member: &str) -> Option<u64> {
        self.index
            .get(member)
            .filter(|(owner, _)| owner == project)
            .map(|(_, rank)| *rank)
    }

    fn renumber(&mut self, project: &str) {
        let members: Vec<String> = self
            .order
            .remove(project)
            .map(|ranks| ranks.into_values().collect())
            .unwrap_or_default();
        let ranks = self.order.entry(project.to_owned()).or_default();
        for (at, member) in members.into_iter().enumerate() {
            let rank = (at as u64 + 1) * GAP;
            ranks.insert(rank, member.clone());
            self.index.insert(member, (project.to_owned(), rank));
        }
    }

    fn insert_before(&mut self, project: &str, member: &str, anchor: &str) {
        let Some(next) = self.rank_in(project, anchor) else {
            self.append(project, member);
            return;
        };
        let previous = self
            .order
            .get(project)
            .and_then(|ranks| ranks.range(..next).next_back().map(|(rank, _)| *rank))
            .unwrap_or(0);
        if next - previous < 2 {
            self.renumber(project);
            return self.insert_before(project, member, anchor);
        }
        let rank = previous + (next - previous) / 2;
        self.order.entry(project.to_owned()).or_default().insert(rank, member.to_owned());
        self.index.insert(member.to_owned(), (project.to_owned(), rank));
    }

    /// A drop or Move to project: the members, in order, before `before`
    /// when it is in `project` and not moving itself, else last
    /// (moveSession, SessionOrganization.cpp:196-199).
    pub fn place(&mut self, project: &str, members: &[String], before: Option<&str>) {
        let anchor = before.filter(|anchor| !members.iter().any(|m| m == anchor));
        for member in members {
            self.remove(member);
        }
        for member in members {
            match anchor {
                Some(anchor) => self.insert_before(project, member, anchor),
                None => {
                    self.append(project, member);
                }
            }
        }
        self.dirty = true;
    }

    /// Sort sessions by name: the members leave their places and follow in
    /// this order, last (moveSession for each, SessionsWindow.cpp:2609).
    pub fn sort(&mut self, project: &str, members: &[String]) {
        self.place(project, members, None);
    }

    /// The new name takes the old one's place (renameSession, 214).
    pub fn rename(&mut self, from: &str, to: &str) {
        let Some((project, rank)) = self.index.get(from).cloned() else {
            return;
        };
        self.remove(to);
        self.remove(from);
        self.order.entry(project.clone()).or_default().insert(rank, to.to_owned());
        self.index.insert(to.to_owned(), (project, rank));
        self.dirty = true;
    }

    /// A deleted project's order and collapsed state go.
    pub fn forget(&mut self, project: &str) {
        if let Some(ranks) = self.order.remove(project) {
            for member in ranks.into_values() {
                self.index.remove(&member);
            }
            self.dirty = true;
        }
        self.dirty |= self.collapsed.remove(project);
    }

    /// Past [`MAX_MEMBERS`], forgets every member `keep` refuses.
    pub fn prune(&mut self, keep: impl Fn(&str) -> bool) {
        if self.index.len() <= MAX_MEMBERS {
            return;
        }
        let gone: Vec<String> = self.index.keys().filter(|member| !keep(member)).cloned().collect();
        for member in gone {
            self.remove(&member);
        }
    }
}
```

- [ ] **Step 4: Run** the tests, clippy and fmt as in Task 1. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/swarm/layout.rs
git commit -m "feat(client): this device's project layout: collapsed state, session order, default"
```

---

### Task 3: Rows to projects, and only what changed to the webview (`client/core/src/swarm/grouping.rs`, `batch.rs`)

**Files:**
- Modify: `client/core/src/swarm/grouping.rs` (replacing the stub)
- Modify: `client/core/src/batch.rs` (two `Batch` fields and `is_empty`)
- Modify: `client/core/src/lib.rs` (`pub use swarm::grouping::{GroupOut, GroupingOut, HeaderOut, SyncView};`)
- Test: unit tests in `grouping.rs`, including 20,000 sessions

**Interfaces:**
- Consumes: `Change::{Upsert, Remove, Replace}` with `ViewRow::{machine, name, title}`; the presented organization and its membership (Task 4's `Controller::presented`, or the topic's `membership`); `Layout` (Task 2); `Intent` (Task 1).
- Produces (all `Serialize`, snake_case JSON):

```rust
pub struct GroupOut { pub computer: ComputerId, pub key: String, pub group: String, pub rank: u64 }
pub struct HeaderOut { pub id: String, pub name: String, pub color: Option<String>, pub accessible: bool,
                       pub collapsed: bool, pub is_default: bool, pub palette: usize }
pub struct SyncView { pub ready: bool, pub reason: Option<String>, pub busy: bool, pub status: String,
                      pub status_error: bool, pub conflicts: usize }
pub struct GroupingOut { pub loaded: bool, pub default_project: String, pub headers: Vec<HeaderOut>,
                         #[serde(flatten)] pub sync: SyncView }
pub struct Grouping;
impl Grouping {
    pub fn new(layout: Layout) -> Self;
    pub fn observe(&mut self, change: &Change);
    pub fn present(&mut self, organization: &Value, membership: Arc<BTreeMap<String, String>>, revision: u64);
    pub fn set_sync(&mut self, sync: SyncView);
    pub fn set_collapsed(&mut self, project: &str, collapsed: bool);
    pub fn edited(&mut self, intent: &Intent);
    pub fn members_of(&self, project: &str) -> Vec<(String, String)>;   // (member, title) in list order
    pub fn default_project(&self) -> &str;
    pub fn take(&mut self) -> (Vec<GroupOut>, Option<GroupingOut>);
    pub fn resend(&mut self);
    pub fn take_renames(&mut self) -> Vec<(String, String)>;
    pub fn take_layout(&mut self) -> Option<Vec<u8>>;
}
pub fn by_name(rows: Vec<(String, String)>) -> Vec<String>;
// batch.rs
pub struct Batch { ..., pub groups: Vec<GroupOut>, pub grouping: Option<GroupingOut> }
```

Rules:
- **A row's project** is its member's project in the membership. A member that no listed project names belongs to the default project (`groupFor`, `SessionOrganization.cpp:87-90`). An unknown project counts as unlisted.
- **Rank:** the layout rank in that project (Task 2). Catalog members that are new to their project are ranked in catalog order first (`SwarmController.cpp:148-150`). Rows the catalog does not name come after them, in arrival order (`SessionOrganization.cpp:236`).
- **Headers:**
  - every listed project in order, at most 500;
  - `color` is the project's own, else none (the palette slot);
  - `accessible` defaults to true; `collapsed` comes from the layout; `is_default` marks the resolved default; `palette` is the index modulo 8.
  - With no project listed, one header: `ungrouped`, "General", `#8d9baa` (`SessionOrganization.cpp:52-53`).
- **Sent only when changed:** `take` returns the rows whose project or rank changed since the last take, and `GroupingOut` only when it differs from the last one sent. `resend` marks everything, for a reloaded webview.
- **Renames:** an upsert of the same catalog key `run:<id>` under a new member is a rename. The layout moves the old place to the new member, and `take_renames` reports `(old, new)` for the hub (Qt's `renameSession`). Before the row's machine is known (an empty `ViewRow::machine`), nothing is reported.
- **Sort sessions by name:** titles (Qt's `sessionLabel`, which equals `ViewRow::title`) compared case-folded, then as written, then by member (`SessionsWindow.cpp:2603-2608`).

- [ ] **Step 1: Write the failing tests** (at the end of `grouping.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::members;
    use crate::ViewRow;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn row(machine: &str, name: &str) -> ViewRow {
        ViewRow {
            machine: machine.into(),
            name: name.into(),
            title: name.rsplit('/').next().unwrap_or(name).into(),
            ..ViewRow::default()
        }
    }

    fn upsert(key: &str, machine: &str, name: &str) -> Change {
        Change::Upsert { computer: ComputerId::local(), key: key.into(), row: row(machine, name) }
    }

    fn organization(alpha: &[&str], beta: &[&str]) -> Value {
        json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "color": "#3fb950", "sessions": alpha},
            {"id": "beta", "name": "Beta", "color": "", "accessible": false, "sessions": beta},
            {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "sessions": []}]})
    }

    fn present(grouping: &mut Grouping, org: &Value, revision: u64) {
        grouping.present(org, Arc::new(members(org)), revision);
    }

    fn assigned(groups: &[GroupOut]) -> Vec<(&str, &str)> {
        groups.iter().map(|g| (g.key.as_str(), g.group.as_str())).collect()
    }

    #[test]
    fn rows_join_their_project_or_the_default_and_are_sent_when_that_changes() {
        let mut grouping = Grouping::new(Layout::default());
        grouping.observe(&upsert("run:1", "workstation", "claude/sample-project/review"));
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        let (groups, out) = grouping.take();
        assert_eq!(assigned(&groups), [("run:1", "ungrouped"), ("run:2", "ungrouped")], "before the catalog: the Inbox");
        assert!(!out.unwrap().loaded);
        present(&mut grouping, &organization(&["workstation\nclaude/sample-project/review"], &[]), 1);
        let (groups, out) = grouping.take();
        assert_eq!(assigned(&groups), [("run:1", "alpha")], "only the row that changed");
        let out = out.unwrap();
        assert!(out.loaded);
        assert_eq!(out.headers.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "ungrouped"]);
        assert_eq!((out.headers[1].color.clone(), out.headers[1].accessible, out.headers[1].palette), (None, false, 1));
        assert!(out.headers[2].is_default);
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        assert_eq!(grouping.take(), (vec![], None), "an unchanged row sends nothing");
        grouping.observe(&Change::Remove { computer: ComputerId::local(), key: "run:2".into(), reason: None });
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        assert_eq!(assigned(&grouping.take().0), [("run:2", "ungrouped")], "a returning row is sent again");
    }

    #[test]
    fn ranks_keep_the_catalog_order_then_arrivals_and_collapsing_sends_the_headers() {
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:1", "claude/p/one"), ("run:2", "claude/p/two"), ("run:3", "claude/p/three")] {
            grouping.observe(&upsert(key, "workstation", name));
        }
        present(&mut grouping, &organization(&["workstation\nclaude/p/three", "workstation\nclaude/p/one"], &[]), 1);
        let (mut groups, _) = grouping.take();
        groups.sort_by_key(|g| g.rank);
        let keys = |project: &str| groups.iter().filter(|g| g.group == project).map(|g| g.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys("alpha"), ["run:3", "run:1"], "the catalog's order");
        assert_eq!(keys("ungrouped"), ["run:2"], "the row no project lists stays in the Inbox");
        grouping.set_collapsed("alpha", true);
        let (groups, out) = grouping.take();
        assert!(groups.is_empty());
        assert!(out.unwrap().headers[0].collapsed);
        assert!(grouping.take_layout().is_some());
    }

    #[test]
    fn no_projects_give_qt_s_general_project() {
        let mut grouping = Grouping::new(Layout::default());
        grouping.present(&json!({"default_project": "ungrouped", "projects": []}), Arc::default(), 1);
        let out = grouping.take().1.unwrap();
        assert_eq!(out.headers.len(), 1);
        assert_eq!((out.headers[0].id.as_str(), out.headers[0].name.as_str()), ("ungrouped", "General"));
        assert_eq!(out.default_project, "ungrouped");
    }

    #[test]
    fn a_rename_under_the_same_run_is_reported_once_and_keeps_its_place() {
        let mut grouping = Grouping::new(Layout::default());
        grouping.observe(&upsert("run:1", "workstation", "claude/p/old"));
        grouping.observe(&upsert("name:x", "workstation", "sh/p"));
        grouping.observe(&upsert("run:1", "workstation", "claude/p/new"));
        grouping.observe(&upsert("name:x", "workstation", "sh/q"));
        assert_eq!(grouping.take_renames(), [("workstation\nclaude/p/old".to_owned(), "workstation\nclaude/p/new".to_owned())]);
        assert!(grouping.take_renames().is_empty());
        grouping.observe(&upsert("run:2", "", "claude/p/early"));
        grouping.observe(&upsert("run:2", "workstation", "claude/p/early"));
        assert!(grouping.take_renames().is_empty(), "no machine yet: no rename");
    }

    #[test]
    fn edits_reorder_locally_and_delete_moves_the_default() {
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:1", "claude/p/b"), ("run:2", "claude/p/a")] {
            grouping.observe(&upsert(key, "workstation", name));
        }
        let org = organization(&[], &[]);
        present(&mut grouping, &org, 1);
        let _ = grouping.take();
        let members = by_name(grouping.members_of("ungrouped"));
        assert_eq!(members, ["workstation\nclaude/p/a", "workstation\nclaude/p/b"]);
        grouping.edited(&Intent::Sort { project: "ungrouped".into(), members: members.clone() });
        present(&mut grouping, &org, 2);
        let mut groups = grouping.take().0;
        groups.sort_by_key(|g| g.rank);
        assert_eq!(groups.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["run:2", "run:1"]);
        grouping.edited(&Intent::Delete { project: "ungrouped".into(), destination: "alpha".into(), members: vec![] });
        assert_eq!(grouping.layout.default_project(), Some("alpha"), "removeGroup: the receiver becomes the default");
    }

    #[test]
    fn sort_by_name_folds_case_then_keeps_the_written_form_then_the_member() {
        let rows = vec![("m3".into(), "beta".into()), ("m1".into(), "Alpha".into()), ("m2".into(), "alpha".into()), ("m0".into(), "alpha".into())];
        assert_eq!(by_name(rows), ["m1", "m0", "m2", "m3"]);
    }

    // Review Focus 5.
    #[test]
    fn twenty_thousand_sessions_cost_one_pass_and_a_move_sends_one_row() {
        let names: Vec<String> = (0..20_000).map(|n| format!("claude/sample-project/s{n:05}")).collect();
        let mut grouping = Grouping::new(Layout::default());
        let started = Instant::now();
        grouping.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: names.iter().enumerate().map(|(n, name)| (format!("run:{n}"), row("workstation", name))).collect(),
        });
        let catalog = |moved: bool| {
            let mut alpha: Vec<String> = names[..10_000].iter().map(|n| crate::swarm::member("workstation", n)).collect();
            let mut beta: Vec<String> = names[10_000..].iter().map(|n| crate::swarm::member("workstation", n)).collect();
            if moved {
                beta.push(alpha.remove(0));
            }
            json!({"default_project": "ungrouped", "projects": [
                {"id": "alpha", "name": "Alpha", "sessions": alpha},
                {"id": "beta", "name": "Beta", "sessions": beta},
                {"id": "ungrouped", "name": "Inbox", "sessions": []}]})
        };
        let first = catalog(false);
        present(&mut grouping, &first, 1);
        let (groups, headers) = grouping.take();
        assert_eq!(groups.len(), 20_000);
        assert!(headers.is_some());
        let bytes = serde_json::to_vec(&groups).unwrap().len();
        assert!(bytes < 3 << 20, "{bytes} bytes for 20,000 rows");
        let moved = catalog(true);
        present(&mut grouping, &moved, 2);
        let (groups, headers) = grouping.take();
        assert_eq!(groups.len(), 1, "only the moved row");
        assert_eq!(groups[0].group, "beta");
        assert!(headers.is_none(), "the headers did not change");
        let took = started.elapsed();
        eprintln!("grouping: 20,000 rows, two catalogs in {took:?}");
        assert!(took < Duration::from_secs(5), "one bounded pass per catalog");
    }
}
```

In `batch.rs` tests:

```rust
#[test]
fn groups_and_grouping_are_left_out_when_empty() {
    let value = serde_json::to_value(Batch::default()).unwrap();
    assert!(value.get("groups").is_none() && value.get("grouping").is_none());
    assert!(Batch::default().is_empty());
    let batch = Batch { groups: vec![crate::GroupOut::default()], ..Batch::default() };
    assert!(!batch.is_empty());
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core --lib -- swarm::grouping batch::`. Expected: compile errors.

- [ ] **Step 3: Implement.**

`client/core/src/batch.rs`:
- Add `use crate::swarm::grouping::{GroupOut, GroupingOut};`.
- Add the two fields at the end of `Batch`:

```rust
    /// Each row's project and rank in the Sessions list, for the rows whose
    /// grouping changed. The Coalescer leaves it empty; the Tauri pump fills
    /// it from `swarm::grouping` (zerus-5wmh.31).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupOut>,
    /// The ordered project headers and whether edits can run, when they changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<GroupingOut>,
```

- `is_empty` gains `&& self.groups.is_empty() && self.grouping.is_none()`. The doc of `Batch` adds "then `groups` and `grouping`" to the apply order.

`client/core/src/swarm/grouping.rs`:

```rust
//! The Sessions list's grouping for the webview: each row's project and
//! rank, and the ordered project headers, sent only when they change
//! (`Batch.groups`, `Batch.grouping`). The Tauri pump feeds it every row
//! change and the hub the presented organization. Qt: SessionsWindowRows.cpp
//! 289-338 (headers and members), SessionOrganization.cpp:87-90 (groupFor)
//! and 114-131 (orderedMembers).
use super::edit::Intent;
use super::layout::Layout;
use super::{default_project, project_id, projects, sessions, INBOX, INBOX_COLOR, INBOX_NAME, MAX_PROJECTS};
use crate::{Change, ComputerId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// One row's place: its project and its rank inside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupOut {
    pub computer: ComputerId,
    pub key: String,
    pub group: String,
    pub rank: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderOut {
    pub id: String,
    pub name: String,
    /// The project's colour; None uses the palette slot.
    pub color: Option<String>,
    pub accessible: bool,
    pub collapsed: bool,
    pub is_default: bool,
    pub palette: usize,
}

/// Whether edits can run, and the catalog's status line (from `sync::Controller`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncView {
    pub ready: bool,
    /// Why edits cannot run, for the disabled entries.
    pub reason: Option<String>,
    pub busy: bool,
    pub status: String,
    pub status_error: bool,
    pub conflicts: usize,
}

impl Default for SyncView {
    fn default() -> Self {
        Self {
            ready: false,
            reason: Some(super::sync::STARTING.into()),
            busy: false,
            status: String::new(),
            status_error: false,
            conflicts: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupingOut {
    /// A catalog is presented; before it, the list shows no headers.
    pub loaded: bool,
    pub default_project: String,
    pub headers: Vec<HeaderOut>,
    #[serde(flatten)]
    pub sync: SyncView,
}

#[derive(Debug)]
struct Slot {
    member: String,
    title: String,
    group: String,
    rank: u64,
}

type At = (ComputerId, String);

#[derive(Debug)]
pub struct Grouping {
    pub(crate) layout: Layout,
    loaded: bool,
    revision: Option<u64>,
    /// id, name, colour, accessible.
    projects: Vec<(String, String, Option<String>, bool)>,
    listed: HashSet<String>,
    default: String,
    membership: Arc<BTreeMap<String, String>>,
    rows: HashMap<At, Slot>,
    dirty: BTreeSet<At>,
    sent: Option<GroupingOut>,
    sync: SyncView,
    renames: Vec<(String, String)>,
}

impl Default for Grouping {
    fn default() -> Self {
        Self::new(Layout::default())
    }
}

/// Sort sessions by name (SessionsWindow.cpp:2603-2608): titles case-folded,
/// then as written, then by member. Qt compares with localeAwareCompare,
/// which has no equivalent here.
pub fn by_name(mut rows: Vec<(String, String)>) -> Vec<String> {
    rows.sort_by_cached_key(|(member, title)| (title.to_lowercase(), title.clone(), member.clone()));
    rows.into_iter().map(|(member, _)| member).collect()
}

impl Grouping {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            loaded: false,
            revision: None,
            projects: Vec::new(),
            listed: HashSet::new(),
            default: INBOX.into(),
            membership: Arc::default(),
            rows: HashMap::new(),
            dirty: BTreeSet::new(),
            sent: None,
            sync: SyncView::default(),
            renames: Vec::new(),
        }
    }

    fn group_of(&self, member: &str) -> String {
        self.membership
            .get(member)
            .filter(|project| self.listed.contains(project.as_str()))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }

    fn place(&mut self, at: At, member: String, title: String) {
        let group = self.group_of(&member);
        let rank = self.layout.rank(&group, &member);
        let changed = self
            .rows
            .get(&at)
            .is_none_or(|slot| slot.group != group || slot.rank != rank);
        if changed {
            self.dirty.insert(at.clone());
        }
        self.rows.insert(at, Slot { member, title, group, rank });
    }

    fn upsert(&mut self, computer: &ComputerId, key: &str, machine: &str, name: &str, title: &str) {
        let at = (computer.clone(), key.to_owned());
        let member = super::member(machine, name);
        if let Some(old) = self.rows.get(&at).map(|slot| slot.member.clone()) {
            // The catalog keys a run by its ID: the same key under a new name is a rename.
            if old != member && key.starts_with("run:") && !machine.is_empty() && !old.starts_with('\n') {
                self.layout.rename(&old, &member);
                self.renames.push((old, member.clone()));
            }
        }
        self.place(at, member, title.to_owned());
    }

    /// A row change from the pump.
    pub fn observe(&mut self, change: &Change) {
        match change {
            Change::Upsert { computer, key, row } => {
                self.upsert(computer, key, &row.machine, &row.name, &row.title)
            }
            Change::Remove { computer, key, .. } => {
                let at = (computer.clone(), key.clone());
                self.rows.remove(&at);
                self.dirty.remove(&at);
            }
            Change::Replace { computer, rows } => {
                let keep: HashSet<&str> = rows.iter().map(|(key, _)| key.as_str()).collect();
                self.rows.retain(|(owner, key), _| owner != computer || keep.contains(key.as_str()));
                self.dirty.retain(|(owner, key)| owner != computer || keep.contains(key.as_str()));
                for (key, row) in rows {
                    self.upsert(computer, key, &row.machine, &row.name, &row.title);
                }
            }
            _ => {}
        }
    }

    /// The organization the list shows; `revision` changes whenever it does.
    pub fn present(&mut self, organization: &Value, membership: Arc<BTreeMap<String, String>>, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        self.loaded = true;
        self.projects = projects(organization)
            .iter()
            .filter(|project| !project_id(project).is_empty())
            .take(MAX_PROJECTS)
            .map(|project| {
                (
                    project_id(project).to_owned(),
                    project["name"].as_str().unwrap_or("").to_owned(),
                    project["color"].as_str().filter(|color| !color.is_empty()).map(str::to_owned),
                    project["accessible"].as_bool().unwrap_or(true),
                )
            })
            .collect();
        if self.projects.is_empty() {
            self.projects.push((INBOX.into(), INBOX_NAME.into(), Some(INBOX_COLOR.into()), true));
        }
        self.listed = self.projects.iter().map(|(id, ..)| id.clone()).collect();
        self.default = default_project(organization, self.layout.default_project());
        if !self.listed.contains(&self.default) {
            self.default = self.projects[0].0.clone();
        }
        self.membership = membership;
        for project in projects(organization).iter().take(MAX_PROJECTS) {
            for member in sessions(project) {
                self.layout.rank(project_id(project), member);
            }
        }
        if self.layout.len() > super::layout::MAX_MEMBERS {
            let listed: HashSet<String> = self.rows.values().map(|slot| slot.member.clone()).collect();
            let membership = Arc::clone(&self.membership);
            self.layout.prune(|member| listed.contains(member) || membership.contains_key(member));
        }
        let slots: Vec<(At, String, String)> = self
            .rows
            .iter()
            .map(|(at, slot)| (at.clone(), slot.member.clone(), slot.title.clone()))
            .collect();
        for (at, member, title) in slots {
            self.place(at, member, title);
        }
    }

    pub fn set_sync(&mut self, sync: SyncView) {
        self.sync = sync;
    }

    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) {
        self.layout.set_collapsed(project, collapsed);
    }

    /// The local side of an edit; the hub presents the edited organization next.
    pub fn edited(&mut self, intent: &Intent) {
        match intent {
            Intent::Move { members, project, before } => self.layout.place(project, members, before.as_deref()),
            Intent::Create { id, members, .. } => self.layout.place(id, members, None),
            Intent::Sort { project, members } => self.layout.sort(project, members),
            Intent::Delete { project, destination, .. } => {
                if self.default == *project {
                    self.layout.set_default_project(Some(destination.clone()));
                }
                self.layout.forget(project);
            }
            Intent::Rename { .. } | Intent::Reorder { .. } | Intent::Renamed { .. } => {}
        }
        self.revision = None;
    }

    /// A project's listed rows in list order, with their titles.
    pub fn members_of(&self, project: &str) -> Vec<(String, String)> {
        let mut rows: Vec<&Slot> = self.rows.values().filter(|slot| slot.group == project).collect();
        rows.sort_by_key(|slot| slot.rank);
        rows.into_iter().map(|slot| (slot.member.clone(), slot.title.clone())).collect()
    }

    pub fn default_project(&self) -> &str {
        &self.default
    }

    fn out(&self) -> GroupingOut {
        let headers = if self.loaded {
            self.projects
                .iter()
                .enumerate()
                .map(|(at, (id, name, color, accessible))| HeaderOut {
                    id: id.clone(),
                    name: name.clone(),
                    color: color.clone(),
                    accessible: *accessible,
                    collapsed: self.layout.collapsed(id),
                    is_default: *id == self.default,
                    palette: at % 8,
                })
                .collect()
        } else {
            Vec::new()
        };
        GroupingOut { loaded: self.loaded, default_project: self.default.clone(), headers, sync: self.sync.clone() }
    }

    /// What changed since the last take.
    pub fn take(&mut self) -> (Vec<GroupOut>, Option<GroupingOut>) {
        let groups = std::mem::take(&mut self.dirty)
            .into_iter()
            .filter_map(|at| {
                let slot = self.rows.get(&at)?;
                Some(GroupOut { computer: at.0, key: at.1, group: slot.group.clone(), rank: slot.rank })
            })
            .collect();
        let out = self.out();
        let headers = (self.sent.as_ref() != Some(&out)).then(|| {
            self.sent = Some(out.clone());
            out
        });
        (groups, headers)
    }

    /// A reloaded webview: everything again.
    pub fn resend(&mut self) {
        self.dirty = self.rows.keys().cloned().collect();
        self.sent = None;
    }

    pub fn take_renames(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.renames)
    }

    pub fn take_layout(&mut self) -> Option<Vec<u8>> {
        self.layout.take_dirty()
    }
}
```

`Change::Remove` field names follow `client/core/src/lib.rs` (`computer`, `key`, `reason`). `Change` derives `PartialEq`, and the tests compare `take()` tuples, so `GroupOut` and `GroupingOut` derive `PartialEq`.

- [ ] **Step 4: Run** the tests (the 20,000-row test prints its time; record it), clippy and fmt. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/swarm/grouping.rs client/core/src/batch.rs client/core/src/lib.rs
git commit -m "feat(client): rows to projects in Rust, sent to the webview only when they change"
```

---

### Task 4: The write controller, SwarmController ported (`client/core/src/swarm/sync.rs`)

This task owns the coordinator's binding requirements for `zerus-5wmh.18` (2026-10-28): Qt's apply rules with their lines, intent sent for hgs to resolve, the second-writer test, the resend of a failed or timed-out apply on the next tick, and the pending draft that survives a restart (2026-10-28). Rename migrations are written through this controller, so the two-writer rename tests are here too: the hub (Task 6) only turns an observed rename into `Intent::Renamed`.

**Files:**
- Modify: `client/core/src/swarm/sync.rs` and `client/core/src/swarm/draft.rs` (replacing the stubs)
- Modify (3a extension): `client/core/src/views/projects.rs` makes `fn status_of` `pub(crate)`, so Qt's status precedence has one implementation.
- Create: `client/core/tests/support/fake_hgs_swarm.py`, `client/core/tests/swarm_sync.rs`
- Test: unit tests in `sync.rs` and `draft.rs`; the integration tests against the fake

**Interfaces:**
- Consumes: `Intent`, `edit::{apply, shared, changes_shared}` (Task 1); `members` (Task 1); `SyncView` (Task 3); `Exit`, `Invocation` (phase 2, `client/core/src/actions.rs`); `views::projects::status_of` (3a).
- Produces:

```rust
// swarm/draft.rs
pub const FILE: &str = "organization-draft.json";
pub const REPLAY_FILE: &str = "organization-draft-replay.json";
pub struct Draft { pub version: u32, pub base: Value, pub desired: Value, pub versions: Map<String, Value> }
impl Draft { pub fn new(base: Value, desired: Value, versions: Map<String, Value>) -> Self;
             pub fn to_json(&self) -> Vec<u8>; pub fn from_json(bytes: &[u8]) -> Option<Self> }
pub enum DraftWrite { Save(Vec<u8>), Remove }
pub fn load(path: &Path) -> Option<Draft>;
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()>;      // temporary file, then rename
pub fn store(path: &Path, change: &DraftWrite) -> io::Result<()>;
// swarm/sync.rs
pub const FLUSH_MS: u64 = 120;
pub const REFRESH_MS: u64 = 5_000;
pub const GUARD_MS: u64 = 10_000;
pub const APPLY_TIMEOUT: Duration = Duration::from_secs(10);
pub const STARTING: &str; pub const NOT_SET_UP: &str; pub const STOPPED: &str;
pub const OUTDATED: &str; pub const INVALID: &str;
pub fn unavailable(detail: &str) -> String;
pub struct Observation { pub digest: String, pub value: Arc<Value>, pub versions: Value, pub membership: Arc<BTreeMap<String, String>> }
impl Observation { pub fn from_reply(reply: Value) -> Option<Self> }
pub type Ticket = u64;
pub enum EditOutcome { Done { conflicts: usize }, Kept { detail: String }, Refused { detail: String } }
pub enum Due { Nothing, Apply(Invocation), Settled(Vec<(Ticket, EditOutcome)>) }
pub enum Reply { Done(Value), Outdated, Failed(String) }
pub fn classify(exit: Exit) -> Reply;
pub fn invocation(input: &Value) -> Invocation;
pub struct Controller;
impl Controller {
    pub fn ready(&self) -> bool; pub fn busy(&self) -> bool; pub fn pending(&self) -> bool;
    pub fn revision(&self) -> u64; pub fn digest(&self) -> &str; pub fn next_due(&self) -> Option<u64>;
    pub fn presented(&self) -> Option<(&Value, Arc<BTreeMap<String, String>>)>;
    pub fn presented_value(&self) -> Option<Arc<Value>>;
    pub fn view(&self) -> SyncView;
    pub fn observe(&mut self, observation: Observation, now: u64) -> bool;
    pub fn source_failed(&mut self, message: &str);
    pub fn source_ended(&mut self);
    pub fn edit(&mut self, intent: &Intent, now: u64) -> Result<Ticket, String>;
    pub fn due(&mut self, now: u64) -> Due;
    pub fn finish(&mut self, exit: Exit, now: u64) -> Vec<(Ticket, EditOutcome)>;
    pub fn restore(&mut self, draft: Draft);
    pub fn take_draft(&mut self) -> Option<DraftWrite>;
}
```

Rules, each with its Qt source (the Global Constraints quote the binding list):

| Rule | Qt | Here |
|---|---|---|
| Busy and ready gate every send | `SwarmController.h:17-19`; `.cpp:65, 94, 98, 104` | `due` returns `Nothing` while busy or before the first initialized state; `edit` is refused while not ready. Once set, `ready` stays, as `m_ready` does; the end of the topic refuses new edits (`STOPPED`) and pending ones are still sent |
| The flush runs 120 ms after the last edit | `.cpp:64-65, 94, 205` | `edit` sets `flush_at = now + 120` while idle, also after a failure; `acknowledged` sets it again when edits are pending |
| The 5 s tick sends pending edits | `.cpp:55, 62-63, 85, 96-101` | after a failure `refresh_at = now + REFRESH_MS`; a state from the topic while it is armed moves it to `now`; `due` sends at `flush_at` or `refresh_at`, whichever comes first. Without a failure Qt's tick could only fire inside a 120 ms flush window, where it would send the same apply a little earlier, so the client arms it only after a failure |
| The first call when there is no observed baseline | `.cpp:81-88, 100, 197, 203` | no `initialize` ever; the first adopted state is the baseline; `initialized: false` keeps edits refused (`NOT_SET_UP`) |
| Base, desired and versions observed together | `.cpp:105, 108, 207-209` | `Observation` pairs the update's value with `OrganizationSnapshot::versions` of the same digest, or splits one `swarm apply` or `swarm get` reply; `due` sends `{base: shared(base), desired: shared(desired), versions}` and keeps what it sent |
| Never apply from a stale base | `.cpp:193, 204-206` | `observe` adopts nothing while pending or busy; after an acknowledged write, a state lacking the written operations is not adopted for `GUARD_MS` |
| `written` advances only acknowledged fields | `.cpp:199-202` | the written tokens take the written operations; with pending edits the reply is not adopted, so other fields keep the versions the user saw |
| Conflict and review states | `.cpp:211-218` | the status comes from 3a's `status_of`; `Done { conflicts }` counts the reply's conflicts whose `key` was written |
| Failure keeps the edits | `.cpp:67-73` (failure), `194-196` (schema) | only the status changes: `unavailable(error)` or `OUTDATED`, as an error. Base, desired, versions and the presented organization stay. The tickets the apply carried are answered `Kept` with that status; edits queued meanwhile stay queued and ride on the resend |
| The resend | `.cpp:96-110` | the next `due` sends `{base, desired, versions}` from the held state, which is exactly what Qt's `refresh` sends; nothing pending is ever dropped (Decisions) |
| The draft is saved | `SessionsWindow.cpp:2496`; `.cpp:111-130` (persist, called at 72, 205, 206, 209) | `edit`, `acknowledged` and a `due` that settles without a call mark the draft changed; `take_draft` gives `Save` with base, desired and versions while edits are pending, else `Remove`. A failure changes nothing pending, so it writes nothing. The hub writes the file with `draft::store` |
| A restart sends the draft after the first read | `SessionsWindow.cpp:275-276, 1661`; `.cpp:76-89` (start), `191-206` | `restore` takes the saved base, desired and versions and leaves the controller not ready, so `due` sends nothing and edits are refused (`STARTING`). The first state is not adopted under the draft: it sets `ready` from `initialized` (197) and starts the flush (204-205). Then one apply carries the saved base and versions |

`classify` follows Qt's reply handling: `parseObject` and "Invalid project catalog response" (`HgsClient.cpp:711-715`), then the schema check (`SwarmController.cpp:194`). Whether the apply landed does not matter, since a failure is sent again either way.
- exit 0 with a schema 1 object that has `organization` is `Done`;
- another exit 0 object is `Outdated`, and output that is not a JSON object is `Failed(INVALID)`;
- a non-zero exit is `Failed` with its standard error (trimmed, at most 4000 characters, as `HgsClient.cpp:475`), else "hgs exited with code N";
- `StartFailed(e)` is `Failed(e)`;
- `TimedOut`, `Abnormal` and `WriteFailed` are `Failed`, with the texts of `reads.rs`.

- [ ] **Step 1: Write the fake** `client/core/tests/support/fake_hgs_swarm.py` (stdlib only, never the real CLI):

```python
#!/usr/bin/env python3
"""A fake `hgs swarm` for the organization tests. Reads `config.json` beside itself:
{"get": [reply, ...], "apply": [reply, ...]} with replies
{"exit": 0, "stdout": <object>, "stderr": "", "sleep": 0}. Each call of a
subcommand takes its next reply and the last one repeats, so a list models
another writer changing the catalog between two calls. Appends
{"argv": [...], "stdin": "..."} to `log.ndjson` beside itself."""
import json
import os
import sys
import time

here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
data = sys.stdin.read()
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": args, "stdin": data}) + "\n")
if len(args) != 2 or args[0] != "swarm" or args[1] not in ("get", "apply"):
    sys.stderr.write("fake hgs: unexpected call\n")
    sys.exit(2)
action = args[1]
with open(os.path.join(here, "config.json")) as config_file:
    replies = json.load(config_file)[action]
counts_path = os.path.join(here, "counts.json")
counts = {}
if os.path.exists(counts_path):
    with open(counts_path) as counts_file:
        counts = json.load(counts_file)
n = counts.get(action, 0)
counts[action] = n + 1
with open(counts_path, "w") as counts_file:
    json.dump(counts, counts_file)
reply = replies[min(n, len(replies) - 1)]
time.sleep(reply.get("sleep", 0))
if reply.get("stdout") is not None:
    sys.stdout.write(json.dumps(reply["stdout"]))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
```

- [ ] **Step 2: Write the failing unit tests** (at the end of `sync.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::edit::Intent;
    use serde_json::json;

    const R: &str = "workstation\nclaude/sample-project/review";
    const LOCAL: &str = "Local catalog. Connect another computer to sync projects.";

    fn review() -> String {
        json!({"kind": "session", "machine": "m-1", "session": "claude/sample-project/review"}).to_string()
    }

    fn alpha_name() -> String {
        json!({"kind": "project", "id": "alpha", "field": "name"}).to_string()
    }

    /// `swarm get` with the review session in `project` and its heads; alpha's name has heads `op-a1`.
    fn state(digest: &str, project: &str, heads: &[&str]) -> Value {
        let list = |id: &str| if id == project { json!([R]) } else { json!([]) };
        let mut versions = Map::new();
        versions.insert(review(), json!(heads));
        versions.insert(alpha_name(), json!(["op-a1"]));
        json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": [],
               "organization": {"version": 2, "default_project": "ungrouped", "projects": [
                   {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": list("alpha")},
                   {"id": "beta", "name": "Beta", "color": "#64b5f6", "accessible": true, "folders": [], "sessions": list("beta")},
                   {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": list("ungrouped")}]},
               "versions": versions})
    }

    fn observation(value: Value) -> Observation {
        Observation::from_reply(value).unwrap()
    }

    fn to(project: &str) -> Intent {
        Intent::Move { members: vec![R.into()], project: project.into(), before: None }
    }

    fn ready() -> Controller {
        let mut sync = Controller::default();
        assert!(sync.observe(observation(state("d1", "alpha", &["op-1"])), 0));
        sync
    }

    fn input(due: Due) -> Value {
        match due {
            Due::Apply(invocation) => {
                assert_eq!(invocation.args, ["swarm", "apply"]);
                assert_eq!(invocation.timeout, APPLY_TIMEOUT);
                serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap()
            }
            _ => panic!("no apply"),
        }
    }

    fn reply(mut value: Value, written: &[&str]) -> Exit {
        let mut map = Map::new();
        if !written.is_empty() {
            map.insert(review(), json!(written));
        }
        value["written"] = Value::Object(map);
        Exit::Code { code: 0, stdout: serde_json::to_vec(&value).unwrap(), stderr: String::new() }
    }

    fn place(sync: &Controller) -> String {
        crate::swarm::members(sync.presented().unwrap().0)[R].clone()
    }

    #[test]
    fn edits_wait_for_an_initialized_catalog_and_nothing_initializes_it() {
        let mut sync = Controller::default();
        assert_eq!(sync.edit(&to("beta"), 0), Err(STARTING.into()));
        let mut fresh = state("d0", "alpha", &["op-1"]);
        fresh["initialized"] = json!(false);
        assert!(sync.observe(observation(fresh), 0));
        assert_eq!(sync.edit(&to("beta"), 0), Err(NOT_SET_UP.into()));
        assert!(matches!(sync.due(10_000), Due::Nothing));
        assert_eq!(sync.view().reason.as_deref(), Some(NOT_SET_UP));
        assert!(sync.presented().is_some(), "the list still groups by it");
    }

    #[test]
    fn edits_flush_together_120_ms_after_the_last_one() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        sync.edit(&Intent::Rename { project: "beta".into(), name: "Beta two".into() }, 100).unwrap();
        assert!(matches!(sync.due(219), Due::Nothing));
        let sent = input(sync.due(220));
        let seen = state("d1", "alpha", &["op-1"]);
        assert_eq!(sent["base"], crate::swarm::edit::shared(&seen["organization"]));
        assert_eq!(sent["versions"], seen["versions"]);
        assert_eq!(crate::swarm::members(&sent["desired"])[R], "beta");
        assert_eq!(crate::swarm::projects(&sent["desired"])[1]["name"], "Beta two");
        assert!(sync.busy() && sync.view().busy);
        assert!(matches!(sync.due(10_000), Due::Nothing), "one apply at a time");
    }

    #[test]
    fn edits_during_an_apply_follow_from_what_was_sent_with_only_written_fields_advanced() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let first = input(sync.due(120));
        let second_ticket = sync.edit(&to("ungrouped"), 500).unwrap();
        assert_eq!(sync.next_due(), None, "no timer while busy");
        // Another writer renamed alpha meanwhile: the reply shows it; the user has not seen it.
        let mut answer = state("d2", "beta", &["op-2"]);
        answer["versions"][alpha_name().as_str()] = json!(["op-a2"]);
        assert_eq!(sync.finish(reply(answer, &["op-2"]), 900), [(1, EditOutcome::Done { conflicts: 0 })]);
        assert_eq!(sync.next_due(), Some(1020));
        assert_eq!(place(&sync), "ungrouped", "the list keeps showing the pending edit");
        let second = input(sync.due(1020));
        assert_eq!(second["base"], first["desired"]);
        assert_eq!(second["versions"][review().as_str()], json!(["op-2"]));
        assert_eq!(second["versions"][alpha_name().as_str()], json!(["op-a1"]), "an unseen field keeps the version the user saw");
        let settled = sync.finish(reply(state("d3", "ungrouped", &["op-4"]), &["op-4"]), 1500);
        assert_eq!(settled, [(second_ticket, EditOutcome::Done { conflicts: 0 })]);
        assert_eq!(sync.digest(), "d3", "nothing pending: the reply is the base");
    }

    #[test]
    fn states_wait_for_pending_edits_and_the_reply_is_adopted() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        assert!(!sync.observe(observation(state("d2", "ungrouped", &["op-9"])), 50), "never merged under an edit");
        assert_eq!(place(&sync), "beta");
        let _ = input(sync.due(120));
        assert!(!sync.observe(observation(state("d2", "ungrouped", &["op-9"])), 200));
        sync.finish(reply(state("d3", "beta", &["op-2"]), &["op-2"]), 300);
        assert_eq!(sync.digest(), "d3");
        assert_eq!(place(&sync), "beta");
    }

    #[test]
    fn a_state_without_our_write_is_older_and_waits_for_one_that_shows_it() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let _ = input(sync.due(120));
        sync.finish(reply(state("d3", "beta", &["op-2"]), &["op-2"]), 300);
        assert!(!sync.observe(observation(state("d1", "alpha", &["op-1"])), 400), "read before the write");
        assert!(!sync.observe(observation(state("d2", "ungrouped", &["op-9"])), 500), "another writer's state without it");
        assert_eq!(sync.digest(), "d3");
        assert!(sync.observe(observation(state("d4", "beta", &["op-2"])), 600));
        assert!(sync.observe(observation(state("d5", "alpha", &["op-5"])), 700), "the guard ended with the state that showed the write");
    }

    #[test]
    fn the_guard_ends_after_ten_seconds() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let _ = input(sync.due(120));
        sync.finish(reply(state("d3", "beta", &["op-2"]), &["op-2"]), 300);
        assert!(!sync.observe(observation(state("d6", "alpha", &["op-6"])), 300 + GUARD_MS - 1));
        assert!(sync.observe(observation(state("d6", "alpha", &["op-6"])), 300 + GUARD_MS), "a later writer superseded ours");
    }

    // SwarmController.cpp:67-73 (failure), 96-101 (refresh), 102-110 (send).
    #[test]
    fn a_failed_apply_keeps_its_edits_and_the_next_tick_sends_them_again() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let first = input(sync.due(120));
        let queued = sync.edit(&to("ungrouped"), 130).unwrap();
        let exit = Exit::Code { code: 1, stdout: vec![], stderr: " hgs: session belongs to more than one project \n".into() };
        let status = "Project sync unavailable: hgs: session belongs to more than one project";
        assert_eq!(sync.finish(exit, 200), [(1, EditOutcome::Kept { detail: status.into() })], "the queued edit rides on the resend");
        assert!(sync.pending() && sync.ready(), "nothing was dropped and the base stays valid");
        assert_eq!(place(&sync), "ungrouped", "the list keeps showing the edits");
        let view = sync.view();
        assert_eq!((view.status.as_str(), view.status_error), (status, true));
        let tick = 200 + REFRESH_MS;
        assert_eq!(sync.next_due(), Some(tick));
        for now in (200..tick).step_by(100) {
            assert!(matches!(sync.due(now), Due::Nothing), "nothing before the tick, at {now}");
        }
        let again = input(sync.due(tick));
        assert_eq!(again["base"], first["base"], "the base the edits started from");
        assert_eq!(again["versions"], first["versions"], "a failure advances no version");
        assert_eq!(crate::swarm::members(&again["desired"])[R], "ungrouped", "the desired state now");
        assert!(matches!(sync.due(tick + 100), Due::Nothing), "one apply at a time");
        let settled = sync.finish(reply(state("d3", "ungrouped", &["op-3"]), &["op-3"]), tick + 200);
        assert_eq!(settled, [(queued, EditOutcome::Done { conflicts: 0 })]);
        assert!(!sync.pending());
        assert_eq!(sync.next_due(), None, "the tick has nothing left to send");
        assert_eq!(sync.view().status, LOCAL);
    }

    // SwarmController.cpp:90-95: an edit restarts the flush, also after a failure.
    #[test]
    fn an_edit_after_a_failure_flushes_120_ms_later_from_the_same_base() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let first = input(sync.due(120));
        sync.finish(Exit::TimedOut, 10_120);
        let ticket = sync.edit(&Intent::Rename { project: "beta".into(), name: "Beta two".into() }, 10_200).unwrap();
        assert!(matches!(sync.due(10_319), Due::Nothing));
        let again = input(sync.due(10_320));
        assert_eq!((&again["base"], &again["versions"]), (&first["base"], &first["versions"]));
        assert_eq!(crate::swarm::members(&again["desired"])[R], "beta", "the kept edit");
        assert_eq!(crate::swarm::projects(&again["desired"])[1]["name"], "Beta two", "and the new one");
        let settled = sync.finish(reply(state("d2", "beta", &["op-2"]), &["op-2"]), 10_500);
        assert_eq!(settled, [(ticket, EditOutcome::Done { conflicts: 0 })]);
    }

    // The topic's state is this client's read tick, where Qt's refresh would send the apply.
    #[test]
    fn a_state_after_a_failed_apply_is_the_tick_that_sends_it_again() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let first = input(sync.due(120));
        sync.finish(Exit::TimedOut, 10_120);
        // The topic shows that the first apply landed; it is not adopted under the pending edit.
        assert!(!sync.observe(observation(state("d2", "beta", &["op-2"])), 10_300));
        assert_eq!(input(sync.due(10_300)), first, "sent again at once, unchanged");
        assert!(!sync.observe(observation(state("d2", "beta", &["op-2"])), 10_400), "nothing while it runs");
        assert!(matches!(sync.due(10_400), Due::Nothing));
        // hgs adds a head with the same value: no conflict (src/swarm/model.rs:298-312).
        let settled = sync.finish(reply(state("d3", "beta", &["op-2", "op-3"]), &["op-3"]), 10_600);
        assert!(settled.is_empty(), "the edit was answered by the first apply");
        assert_eq!((sync.digest(), sync.view().conflicts), ("d3", 0));
    }

    // SwarmController.cpp:194-196.
    #[test]
    fn an_outdated_reply_keeps_the_edits_with_qt_s_schema_text() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let _ = input(sync.due(120));
        let exit = Exit::Code { code: 0, stdout: br#"{"schema":2}"#.to_vec(), stderr: String::new() };
        assert_eq!(sync.finish(exit, 300), [(1, EditOutcome::Kept { detail: OUTDATED.into() })]);
        let view = sync.view();
        assert_eq!((view.status.as_str(), view.status_error), ("Project sync needs an updated HGS installation.", true));
        assert!(sync.pending());
        assert_eq!(sync.next_due(), Some(300 + REFRESH_MS));
    }

    // Qt's m_base == m_desired (SwarmController.cpp:65, 100): nothing is left to send.
    #[test]
    fn an_edit_back_to_the_base_after_a_failure_ends_the_resend() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let _ = input(sync.due(120));
        sync.finish(Exit::TimedOut, 10_120);
        let back = sync.edit(&to("alpha"), 10_200).unwrap();
        match sync.due(10_320) {
            Due::Settled(settled) => assert_eq!(settled, [(back, EditOutcome::Done { conflicts: 0 })]),
            _ => panic!("nothing left to send"),
        }
        assert!(!sync.pending());
        assert_eq!(sync.next_due(), None);
        assert!(sync.observe(observation(state("d2", "alpha", &["op-1"])), 10_400), "states are adopted again");
    }

    #[test]
    fn the_end_of_the_source_refuses_new_edits_and_still_sends_pending_ones() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        sync.source_ended();
        assert_eq!(sync.edit(&to("ungrouped"), 50), Err(STOPPED.into()));
        assert_eq!(sync.view().reason.as_deref(), Some(STOPPED));
        let _ = input(sync.due(120));
        sync.finish(reply(state("d2", "beta", &["op-2"]), &["op-2"]), 300);
        assert_eq!(place(&sync), "beta");
        assert!(sync.observe(observation(state("d3", "beta", &["op-2"])), 400), "a state: the source is back");
        assert!(sync.ready());
    }

    // Qt saves its draft with every change (SessionsWindow.cpp:2496) and its base
    // and versions with persist (SwarmController.cpp:111-130, called at 205 and 209).
    #[test]
    fn the_draft_follows_pending_edits_and_is_removed_when_they_settle() {
        let saved = |change: Option<DraftWrite>| match change {
            Some(DraftWrite::Save(bytes)) => Draft::from_json(&bytes).unwrap(),
            other => panic!("no draft saved: {other:?}"),
        };
        let mut sync = ready();
        assert_eq!(sync.take_draft(), None, "nothing pending");
        sync.edit(&to("beta"), 0).unwrap();
        let seen = state("d1", "alpha", &["op-1"]);
        let draft = saved(sync.take_draft());
        assert_eq!(draft.base, seen["organization"], "the base the edit started from");
        assert_eq!(Value::Object(draft.versions), seen["versions"]);
        assert_eq!(crate::swarm::members(&draft.desired)[R], "beta");
        assert_eq!(sync.take_draft(), None, "written once");
        let _ = input(sync.due(120));
        sync.edit(&to("ungrouped"), 200).unwrap();
        assert_eq!(crate::swarm::members(&saved(sync.take_draft()).desired)[R], "ungrouped", "an edit during the apply");
        sync.finish(reply(state("d2", "beta", &["op-2"]), &["op-2"]), 300);
        let draft = saved(sync.take_draft());
        assert_eq!(crate::swarm::members(&draft.base)[R], "beta", "the acknowledged write is the new base");
        assert_eq!(draft.versions[review().as_str()], json!(["op-2"]), "with its written version");
        let _ = input(sync.due(420));
        sync.finish(Exit::TimedOut, 10_420);
        assert_eq!(sync.take_draft(), None, "a failure changes nothing pending");
        let _ = input(sync.due(10_420 + REFRESH_MS));
        sync.finish(reply(state("d3", "ungrouped", &["op-3"]), &["op-3"]), 15_600);
        assert_eq!(sync.take_draft(), Some(DraftWrite::Remove), "settled");
    }

    // Qt's start with a saved draft (SwarmController.cpp:76-89) and its first reply (191-206).
    #[test]
    fn a_restored_draft_is_shown_and_waits_for_the_first_state() {
        let seen = state("d1", "alpha", &["op-1"]);
        let mut desired = seen["organization"].clone();
        desired["projects"][0]["sessions"] = json!([]);
        desired["projects"][1]["sessions"] = json!([R]);
        let versions = seen["versions"].as_object().unwrap().clone();
        let mut sync = Controller::default();
        sync.restore(Draft::new(seen["organization"].clone(), desired, versions));
        assert_eq!(place(&sync), "beta", "the list shows the draft");
        assert_eq!(sync.edit(&to("ungrouped"), 0), Err(STARTING.into()));
        for now in (0..30_000).step_by(500) {
            assert!(matches!(sync.due(now), Due::Nothing), "nothing before the first state, at {now}");
        }
        assert_eq!(sync.take_draft(), None, "the file already holds it");
        assert!(!sync.observe(observation(state("d2", "alpha", &["op-1"])), 30_000), "not adopted under the draft");
        assert!(sync.ready() && sync.pending());
        assert_eq!(sync.next_due(), Some(30_000 + FLUSH_MS));
        let sent = input(sync.due(30_000 + FLUSH_MS));
        assert_eq!(sent["base"], crate::swarm::edit::shared(&seen["organization"]));
        assert_eq!(sent["versions"], seen["versions"]);
        assert_eq!(crate::swarm::members(&sent["desired"])[R], "beta");
    }

    #[test]
    fn conflicts_on_written_fields_are_the_outcome_and_the_status_follows_qt() {
        let mut sync = ready();
        sync.edit(&to("beta"), 0).unwrap();
        let _ = input(sync.due(120));
        let mut answer = state("d3", "beta", &["op-2", "op-3"]);
        answer["conflicts"] = json!([
            {"key": review(), "field": {"kind": "session"}, "selected": "op-3", "variants": []},
            {"key": alpha_name(), "field": {"kind": "project"}, "selected": null, "variants": []}]);
        assert_eq!(sync.finish(reply(answer, &["op-3"]), 300), [(1, EditOutcome::Done { conflicts: 1 })]);
        let view = sync.view();
        assert_eq!((view.status.as_str(), view.status_error, view.conflicts), ("2 project conflict(s) need review in Swarm.", true, 2));
    }

    #[test]
    fn an_edit_that_changes_nothing_shared_sends_nothing() {
        let mut sync = ready();
        let ticket = sync.edit(&Intent::Move { members: vec![R.into()], project: "alpha".into(), before: None }, 0).unwrap();
        match sync.due(120) {
            Due::Settled(settled) => assert_eq!(settled, [(ticket, EditOutcome::Done { conflicts: 0 })]),
            _ => panic!("nothing to send"),
        }
        assert!(!sync.busy());
    }

    #[test]
    fn a_source_failure_keeps_the_base_and_shows_qt_s_text() {
        let mut sync = ready();
        sync.source_failed("catalog unreadable");
        assert_eq!(sync.view().status, "Project sync unavailable: catalog unreadable");
        assert!(sync.ready());
    }

    #[test]
    fn exits_classify_as_qt_s_reply_schema_check_or_failure() {
        let code = |code: i32, stdout: &[u8], stderr: &str| Exit::Code { code, stdout: stdout.to_vec(), stderr: stderr.into() };
        assert!(matches!(classify(code(0, br#"{"schema":1,"organization":{}}"#, "")), Reply::Done(_)));
        assert_eq!(classify(code(0, br#"{"schema":2}"#, "")), Reply::Outdated);
        assert_eq!(classify(code(0, br#"{"schema":1}"#, "")), Reply::Outdated);
        assert_eq!(classify(code(0, b"", "")), Reply::Failed(INVALID.into()));
        assert_eq!(classify(code(0, b"[]", "")), Reply::Failed(INVALID.into()));
        assert_eq!(classify(code(3, b"", "")), Reply::Failed("hgs exited with code 3".into()));
        assert_eq!(classify(Exit::StartFailed("No such file or directory".into())), Reply::Failed("No such file or directory".into()));
        assert_eq!(classify(Exit::TimedOut), Reply::Failed("hgs did not answer in time".into()));
        assert_eq!(classify(Exit::Abnormal { stderr: String::new() }), Reply::Failed("hgs ended abnormally".into()));
        assert_eq!(classify(Exit::WriteFailed), Reply::Failed("hgs did not take the request".into()));
    }
}
```

- [ ] **Step 3: Write the failing integration tests** `client/core/tests/swarm_sync.rs`. These are the coordinator's second-writer test, the resend tests and the two-writer rename tests, with the real runner against the fake:

```rust
//! The organization's write controller (`swarm::sync`) with the real runner
//! against a fake `hgs` (tests/support/fake_hgs_swarm.py) whose `swarm get`
//! and `swarm apply` replies model a second writer, a timed-out apply and its
//! resend, Qt migrating the same rename, and a catalog that changed while the
//! client was stopped with a draft. Never the real CLI.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use zerus_client_core::reads::{self, Read};
use zerus_client_core::swarm::draft;
use zerus_client_core::swarm::edit::{shared, Intent};
use zerus_client_core::swarm::sync::{Controller, Due, EditOutcome, Observation, REFRESH_MS};
use zerus_client_core::swarm::{members, projects, sessions};
use zerus_client_core::{run_limited, ComputerId, Exit};

const R: &str = "workstation\nclaude/sample-project/review";
const R2: &str = "workstation\nclaude/sample-project/review-2";
const OLD: &str = "claude/sample-project/review";
const NEW: &str = "claude/sample-project/review-2";
const LOCAL: &str = "Local catalog. Connect another computer to sync projects.";
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    fn new(config: &Value) -> Self {
        let dir = std::env::temp_dir().join(format!("zerus-swarm-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hgs = dir.join("hgs");
        std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake_hgs_swarm.py"), &hgs).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dir.join("config.json"), serde_json::to_vec(config).unwrap()).unwrap();
        Self { dir, hgs }
    }

    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("log.ndjson"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn applies(&self) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|entry| entry["argv"] == json!(["swarm", "apply"]))
            .map(|entry| serde_json::from_str(entry["stdin"].as_str().unwrap()).unwrap())
            .collect()
    }

    fn calls(&self) -> Vec<String> {
        self.log().iter().map(|entry| entry["argv"][1].as_str().unwrap().to_owned()).collect()
    }

    /// One `swarm get`, as the topic or the core's poll would deliver it.
    fn read(&self) -> Observation {
        Observation::from_reply(reads::read(&self.hgs, &ComputerId::local(), &Read::Swarm).unwrap()).unwrap()
    }

    fn run(&self, due: Due) -> Exit {
        let Due::Apply(invocation) = due else { panic!("no apply") };
        run_limited(&self.hgs, &invocation, &[], Read::Swarm.limit()).unwrap()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A session field's version token, as `swarm get` keys it.
fn token(session: &str) -> String {
    json!({"kind": "session", "machine": "m-1", "session": session}).to_string()
}

fn review() -> String {
    token(OLD)
}

/// `swarm get` with the review session in `project`, its heads, conflicts and what the call wrote.
fn snapshot(digest: &str, project: &str, heads: &[&str], conflicts: Value, written: &[&str]) -> Value {
    let list = |id: &str| if id == project { json!([R]) } else { json!([]) };
    let mut versions = Map::new();
    versions.insert(review(), json!(heads));
    let mut wrote = Map::new();
    if !written.is_empty() {
        wrote.insert(review(), json!(written));
    }
    json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": conflicts,
           "organization": {"version": 2, "default_project": "ungrouped", "projects": [
               {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": list("alpha")},
               {"id": "beta", "name": "Beta", "color": "#64b5f6", "accessible": true, "folders": [], "sessions": list("beta")},
               {"id": "gamma", "name": "Gamma", "color": "#d29922", "accessible": true, "folders": [], "sessions": list("gamma")},
               {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": list("ungrouped")}]},
           "versions": versions, "written": wrote})
}

/// Each session field's heads, by session name.
fn heads(pairs: &[(&str, &[&str])]) -> Map<String, Value> {
    pairs.iter().map(|(session, ops)| (token(session), json!(ops))).collect()
}

/// `swarm get` with alpha listing `sessions`, the session fields' heads and what the call wrote.
fn renaming(digest: &str, sessions: &[&str], versions: &[(&str, &[&str])], written: &[(&str, &[&str])]) -> Value {
    json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": [],
           "organization": {"version": 2, "default_project": "ungrouped", "projects": [
               {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": sessions},
               {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": []}]},
           "versions": heads(versions), "written": heads(written)})
}

fn reply(value: Value) -> Value {
    json!({"exit": 0, "stdout": value})
}

fn to(project: &str) -> Intent {
    Intent::Move { members: vec![R.into()], project: project.into(), before: None }
}

fn renamed() -> Intent {
    Intent::Renamed { from: R.into(), to: R2.into() }
}

fn place(sync: &Controller) -> String {
    members(sync.presented().unwrap().0)[R].clone()
}

/// What the hub does after each change: the draft file follows the pending edits.
fn persist(sync: &mut Controller, path: &Path) {
    if let Some(change) = sync.take_draft() {
        draft::store(path, &change).unwrap();
    }
}

/// A new instance of the client: it loads the draft the last one saved.
fn restarted(path: &Path) -> Controller {
    let mut sync = Controller::default();
    sync.restore(draft::load(path).expect("the last run saved a draft"));
    sync
}

// Review Focus 1 (coordinator ruling, 2026-10-28).
#[test]
fn a_second_writer_between_read_and_apply_gives_a_conflict_not_an_overwrite() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    // The second writer moved the session to gamma after the user's read.
    let other = snapshot("d2", "gamma", &["op-2"], json!([]), &[]);
    // hgs: the client's write has parents op-1, so op-2 and op-3 are concurrent heads.
    let conflict = json!([{"key": review(), "field": {"kind": "session"}, "selected": "op-3", "variants": []}]);
    let answer = snapshot("d3", "beta", &["op-2", "op-3"], conflict.clone(), &["op-3"]);
    let after = snapshot("d3", "beta", &["op-2", "op-3"], conflict, &[]);
    let next = snapshot("d4", "alpha", &["op-4"], json!([]), &["op-4"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone()), reply(other.clone()), reply(after.clone())],
                                 "apply": [reply(answer), reply(next)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    sync.edit(&to("beta"), 0).unwrap();
    assert!(!sync.observe(fake.read(), 60), "the newer state is not merged under the edit");
    assert_eq!(place(&sync), "beta");

    let exit = fake.run(sync.due(120));
    let settled = sync.finish(exit, 400);
    let sent = &fake.applies()[0];
    assert_eq!(sent["base"], shared(&seen["organization"]), "the base the user saw");
    assert_eq!(sent["versions"], seen["versions"], "with the versions of that same read");
    assert_eq!(members(&sent["desired"])[R], "beta");
    assert_eq!(settled, [(1, EditOutcome::Done { conflicts: 1 })]);
    assert_eq!(place(&sync), "beta", "where hgs placed it");
    assert_eq!(sync.view().status, "1 project conflict(s) need review in Swarm.");

    assert!(!sync.observe(Observation::from_reply(other).unwrap(), 500), "a late older state is not adopted");
    assert!(sync.observe(fake.read(), 600), "the next read shows the write and is the base");
    assert_eq!(sync.digest(), "d3");
    sync.edit(&to("alpha"), 1000).unwrap();
    let exit = fake.run(sync.due(1120));
    assert_eq!(sync.finish(exit, 1500), [(2, EditOutcome::Done { conflicts: 0 })]);
    let second = &fake.applies()[1];
    assert_eq!(second["base"], shared(&after["organization"]));
    assert_eq!(second["versions"], after["versions"]);
    assert_eq!(fake.calls(), ["get", "get", "apply", "get", "apply"]);
}

// Review Focus 2 (coordinator ruling, 2026-10-28): SwarmController.cpp:67-73, 96-110.
#[test]
fn a_timed_out_apply_stays_pending_and_the_next_tick_sends_it_once_more() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    // The first apply landed before the timeout (op-2); the topic shows it while the resend runs.
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &[]);
    // hgs writes the resend's diff again with the parents the user saw (src/swarm/model.rs:197-198):
    // a second head with the same value, which is no conflict (298-312).
    let again = snapshot("d3", "beta", &["op-2", "op-3"], json!([]), &["op-3"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone())],
                                 "apply": [{"exit": 0, "sleep": 5, "stdout": null}, reply(again)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&to("beta"), 0).unwrap();
    let Due::Apply(mut invocation) = sync.due(120) else { panic!("no apply") };
    invocation.timeout = Duration::from_millis(300);
    let exit = run_limited(&fake.hgs, &invocation, &[], Read::Swarm.limit()).unwrap();
    assert!(matches!(exit, Exit::TimedOut));
    let failed = 1000;
    let status = "Project sync unavailable: hgs did not answer in time";
    assert_eq!(sync.finish(exit, failed), [(ticket, EditOutcome::Kept { detail: status.into() })]);
    assert!(sync.pending(), "the edit stays");
    assert_eq!(place(&sync), "beta", "the list keeps showing it");
    let view = sync.view();
    assert_eq!((view.status.as_str(), view.status_error, view.ready), (status, true, true));
    for now in (failed..failed + REFRESH_MS).step_by(250) {
        assert!(matches!(sync.due(now), Due::Nothing), "nothing before the tick, at {now}");
    }
    let tick = failed + REFRESH_MS;
    let resend = sync.due(tick);
    assert!(matches!(sync.due(tick), Due::Nothing), "no duplicate while one is in flight");
    assert!(!sync.observe(Observation::from_reply(landed).unwrap(), tick + 50), "not adopted under the edit");
    assert!(matches!(sync.due(tick + 100), Due::Nothing), "and a state sends nothing while the resend runs");
    let exit = fake.run(resend);
    assert!(sync.finish(exit, tick + 400).is_empty(), "the edit was answered by the first apply");
    assert!(!sync.pending());
    assert_eq!((sync.digest(), sync.view().conflicts), ("d3", 0));
    assert_eq!(sync.view().status, LOCAL);
    let applies = fake.applies();
    assert_eq!(applies.len(), 2);
    assert_eq!(applies[1], applies[0], "the same base, desired and versions");
    assert_eq!(applies[0]["base"], shared(&seen["organization"]));
    assert_eq!(applies[0]["versions"], seen["versions"]);
    assert_eq!(members(&applies[0]["desired"])[R], "beta");
    assert_eq!(fake.calls(), ["get", "apply", "apply"]);
}

// Qt never drops a failed draft, and hgs exits 1 for every swarm error
// (src/swarm/mod.rs:168-173), so a refusal is sent again on each tick.
#[test]
fn a_refused_apply_is_kept_and_sent_once_per_tick_until_it_lands() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &["op-2"]);
    let refused = json!({"exit": 1, "stderr": "hgs: session belongs to more than one project\n"});
    let fake = Fake::new(&json!({"get": [reply(seen)], "apply": [refused.clone(), refused, reply(landed)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&to("beta"), 0).unwrap();
    let mut now = 120;
    let exit = fake.run(sync.due(now));
    let status = "Project sync unavailable: hgs: session belongs to more than one project";
    assert_eq!(sync.finish(exit, now), [(ticket, EditOutcome::Kept { detail: status.into() })]);
    for _ in 0..2 {
        assert!(matches!(sync.due(now + REFRESH_MS - 1), Due::Nothing), "one apply per tick");
        now += REFRESH_MS;
        let exit = fake.run(sync.due(now));
        assert!(sync.finish(exit, now).is_empty());
    }
    assert!(!sync.pending(), "the third one landed");
    assert_eq!(place(&sync), "beta");
    let applies = fake.applies();
    assert_eq!(applies.len(), 3);
    assert!(applies.iter().all(|sent| *sent == applies[0]), "the same request each time");
    assert_eq!(fake.calls(), ["get", "apply", "apply", "apply"]);
}

// Two writers migrate one rename: Qt and this client side by side both observe it
// (SessionsWindow.cpp:1942; SessionOrganization.cpp:223-230). Here Qt was first.
#[test]
fn a_rename_the_other_writer_already_migrated_sends_nothing() {
    let migrated = renaming("d2", &[R2], &[(OLD, &["op-q1"]), (NEW, &["op-q2"])], &[]);
    let fake = Fake::new(&json!({"get": [reply(migrated)], "apply": [reply(json!(null))]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&renamed(), 0).unwrap();
    match sync.due(120) {
        Due::Settled(settled) => assert_eq!(settled, [(ticket, EditOutcome::Done { conflicts: 0 })]),
        _ => panic!("the old name is no member any more: nothing to write"),
    }
    assert!(!sync.pending());
    let presented = sync.presented().unwrap().0;
    assert_eq!(sessions(&projects(presented)[0]).collect::<Vec<_>>(), [R2], "listed once, in alpha");
    assert_eq!(sync.view().conflicts, 0);
    assert_eq!(fake.calls(), ["get"], "no apply");
}

// Two writers migrate one rename: Qt writes it between the client's read and its apply.
#[test]
fn a_rename_the_other_writer_migrates_between_read_and_apply_merges_cleanly() {
    let seen = renaming("d1", &[R], &[(OLD, &["op-1"])], &[]);
    let by_qt = renaming("d2", &[R2], &[(OLD, &["op-q1"]), (NEW, &["op-q2"])], &[]);
    // hgs writes the client's diff with the parents it read (src/swarm/model.rs:197-198): the old
    // name's removal beside Qt's, the new name's alpha beside Qt's. Equal values are no conflict
    // (298-312), and each session field holds one value, so membership cannot duplicate.
    let merged = renaming("d3", &[R2], &[(OLD, &["op-q1", "op-c1"]), (NEW, &["op-q2", "op-c2"])],
                          &[(OLD, &["op-c1"]), (NEW, &["op-c2"])]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone()), reply(by_qt)], "apply": [reply(merged)]}));
    let mut sync = Controller::default();
    assert!(sync.observe(fake.read(), 0));
    let ticket = sync.edit(&renamed(), 0).unwrap();
    assert!(!sync.observe(fake.read(), 60), "Qt's migration is not merged under the client's");
    let exit = fake.run(sync.due(120));
    assert_eq!(sync.finish(exit, 400), [(ticket, EditOutcome::Done { conflicts: 0 })]);
    let sent = &fake.applies()[0];
    assert_eq!(sent["base"], shared(&seen["organization"]), "the base the client read");
    assert_eq!(sent["versions"], seen["versions"]);
    assert_eq!(members(&sent["desired"]), BTreeMap::from([(R2.to_owned(), "alpha".to_owned())]));
    let presented = sync.presented().unwrap().0;
    assert_eq!(sessions(&projects(presented)[0]).collect::<Vec<_>>(), [R2], "listed once, in alpha");
    assert!(!members(presented).contains_key(R));
    let view = sync.view();
    assert_eq!((view.conflicts, view.status.as_str(), view.status_error), (0, LOCAL, false));
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}

// Review Focus 10 (coordinator ruling, 2026-10-28). Qt saves its draft
// (SessionsWindow.cpp:2496) and, after a restart, sends it once its first read
// answered (SwarmController.cpp:76-89, 191-206).
#[test]
fn a_pending_draft_survives_a_restart_and_is_sent_once_after_the_first_state() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let landed = snapshot("d2", "beta", &["op-2"], json!([]), &["op-2"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone())], "apply": [reply(landed)]}));
    let path = fake.dir.join(draft::FILE);
    // The first run: the edit is pending, and its draft is on disk before any apply.
    let mut first = Controller::default();
    assert!(first.observe(fake.read(), 0));
    first.edit(&to("beta"), 0).unwrap();
    persist(&mut first, &path);
    assert!(first.pending() && path.exists());
    drop(first); // the client stops (before its flush; during or after a failed apply the draft is the same)

    let mut sync = restarted(&path);
    assert_eq!(place(&sync), "beta", "the new instance shows the draft");
    assert!(!sync.ready(), "edits wait for the first state");
    for now in (0..20_000).step_by(250) {
        assert!(matches!(sync.due(now), Due::Nothing), "nothing before the first state, at {now}");
    }
    assert!(fake.applies().is_empty());
    // The first state: not adopted under the draft; the flush sends it (197, 203-205).
    assert!(!sync.observe(fake.read(), 20_000));
    assert!(matches!(sync.due(20_000 + 119), Due::Nothing));
    let exit = fake.run(sync.due(20_000 + 120));
    assert!(matches!(sync.due(20_300), Due::Nothing), "one apply at a time");
    assert!(sync.finish(exit, 20_400).is_empty(), "nobody waits for a restored edit");
    let applies = fake.applies();
    assert_eq!(applies.len(), 1, "exactly one apply");
    assert_eq!(applies[0]["base"], shared(&seen["organization"]), "the saved base");
    assert_eq!(applies[0]["versions"], seen["versions"], "the saved versions");
    assert_eq!(members(&applies[0]["desired"])[R], "beta", "the saved desired state");
    assert!(!sync.pending());
    assert_eq!(place(&sync), "beta");
    persist(&mut sync, &path);
    assert!(!path.exists(), "a settled draft is removed");
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}

// While the client was stopped, another writer changed the catalog: the draft
// goes out with the saved base and versions, and hgs records both writes.
#[test]
fn a_restored_draft_against_a_newer_catalog_gives_a_conflict_not_an_overwrite() {
    let seen = snapshot("d1", "alpha", &["op-1"], json!([]), &[]);
    let newer = snapshot("d2", "gamma", &["op-2"], json!([]), &[]);
    // hgs: the draft's write has parents op-1, so op-2 and op-3 are concurrent heads.
    let conflict = json!([{"key": review(), "field": {"kind": "session"}, "selected": "op-3", "variants": []}]);
    let answer = snapshot("d3", "beta", &["op-2", "op-3"], conflict, &["op-3"]);
    let fake = Fake::new(&json!({"get": [reply(seen.clone()), reply(newer.clone())], "apply": [reply(answer)]}));
    let path = fake.dir.join(draft::FILE);
    let mut first = Controller::default();
    assert!(first.observe(fake.read(), 0));
    first.edit(&to("beta"), 0).unwrap();
    persist(&mut first, &path);
    drop(first);

    let mut sync = restarted(&path);
    assert!(!sync.observe(fake.read(), 1000), "the newer catalog is not merged under the draft");
    let exit = fake.run(sync.due(1120));
    let sent = &fake.applies()[0];
    assert_eq!(sent["base"], shared(&seen["organization"]), "the saved base, not the newer catalog");
    assert_eq!(sent["versions"], seen["versions"], "the saved versions, so hgs keeps both writes");
    assert_ne!(sent["versions"], newer["versions"]);
    assert!(sync.finish(exit, 1400).is_empty());
    let view = sync.view();
    assert_eq!((view.status.as_str(), view.status_error, view.conflicts), ("1 project conflict(s) need review in Swarm.", true, 1));
    assert_eq!(place(&sync), "beta", "where hgs placed it");
    assert_eq!(fake.calls(), ["get", "get", "apply"]);
}
```

`Due` and `Reply` derive `Debug` and `PartialEq`, since the tests compare them.

- [ ] **Step 4: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core --lib -- swarm::sync swarm::draft`, then `--test swarm_sync`. Expected: compile errors.

- [ ] **Step 5: Implement.** In `client/core/src/views/projects.rs` change `fn status_of` to `pub(crate) fn status_of` (3a extension 1).

`client/core/src/swarm/sync.rs`:

```rust
//! The write side of the organization, a port of tray/src/SwarmController.cpp:
//! one `hgs swarm apply {base, desired, versions}` at a time, base and
//! versions from one observed state, edits flushed 120 ms after the last
//! one, and only acknowledged fields advanced. Pure: the hub runs the
//! invocations and passes the clock in (milliseconds).
//!
//! A failed or timed-out apply keeps its edits, and the next tick sends the
//! same base, desired state and versions again, as Qt's `refresh` does
//! (67-73, 96-110). `swarm apply` is a declarative three-way merge guarded by
//! versions: a resend of a diff that already landed adds a head with the same
//! value, which is no conflict, and a stale base surfaces as a conflict
//! (src/swarm/model.rs:126-201, 298-312; coordinator ruling, 2026-10-28).
//!
//! Pending edits survive a restart as Qt's do: [`Controller::take_draft`]
//! gives the draft file's next content ([`super::draft`]), and
//! [`Controller::restore`] takes it back at start. Nothing is sent before the
//! first state, and the draft then goes out from its saved base and versions
//! (SwarmController.cpp:76-89, 191-206).
//!
//! One rule differs from Qt on purpose. Qt initializes an empty local catalog
//! with its own cache (81-88). This client has none, and an initialize from it
//! would mark the catalog imported and skip the Qt GUI's one-time import
//! (src/swarm/mod.rs:212-215): edits wait until the catalog reports
//! `initialized`.
use super::draft::{Draft, DraftWrite};
use super::edit::{self, Intent};
use super::grouping::SyncView;
use super::members;
use crate::views::projects::status_of;
use crate::{Exit, Invocation};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// SwarmController's flush delay (SwarmController.cpp:64).
pub const FLUSH_MS: u64 = 120;
/// SwarmController's poll interval (SwarmController.cpp:55): the tick that
/// sends pending edits again after a failed apply.
pub const REFRESH_MS: u64 = 5_000;
/// How long a state without this client's last write counts as older.
pub const GUARD_MS: u64 = 10_000;
/// HgsClient::requestSwarm's timeout for local catalog commands (HgsClient.cpp:711).
pub const APPLY_TIMEOUT: Duration = Duration::from_secs(10);
pub const STARTING: &str = "Project sync is starting.";
pub const NOT_SET_UP: &str =
    "Project sync is not set up on this computer yet. Start Zerus here once to import its projects.";
/// The organization topic ended, or the hub's worker is gone.
pub const STOPPED: &str = "Project sync stopped.";
/// SwarmController.cpp:195.
pub const OUTDATED: &str = "Project sync needs an updated HGS installation.";
/// HgsClient.cpp:713.
pub const INVALID: &str = "Invalid project catalog response";

/// SwarmController.cpp:69.
pub fn unavailable(detail: &str) -> String {
    format!("Project sync unavailable: {detail}")
}

/// One observed catalog: the `swarm get` object without `versions`, its
/// digest, the versions observed with it and its membership.
#[derive(Clone, Debug)]
pub struct Observation {
    pub digest: String,
    pub value: Arc<Value>,
    pub versions: Value,
    pub membership: Arc<BTreeMap<String, String>>,
}

impl Observation {
    /// A `swarm get` or `swarm apply` reply, with its versions taken out.
    pub fn from_reply(mut reply: Value) -> Option<Self> {
        if reply["schema"] != 1 || !reply["organization"].is_object() {
            return None;
        }
        let object = reply.as_object_mut()?;
        let versions = object.remove("versions").unwrap_or_else(|| json!({}));
        object.remove("written");
        let digest = reply["digest"].as_str().unwrap_or("").to_owned();
        let membership = Arc::new(members(&reply["organization"]));
        Some(Self { digest, value: Arc::new(reply), versions, membership })
    }
}

pub type Ticket = u64;

/// How an edit ended, for the webview: the outcome of the first apply that carried it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum EditOutcome {
    /// Saved; `conflicts` counts the conflicts on the fields it wrote.
    Done { conflicts: usize },
    /// The apply failed or timed out. The edit stays pending, the list keeps
    /// showing it, and the next tick sends it again. `detail` is Qt's status
    /// line: "Project sync unavailable: …" or the schema text.
    Kept { detail: String },
    /// Not queued: the catalog is not ready, or the edit cannot be made.
    Refused { detail: String },
}

#[derive(Debug, PartialEq)]
pub enum Due {
    Nothing,
    Apply(Invocation),
    /// Edits that changed nothing shared: done without a call.
    Settled(Vec<(Ticket, EditOutcome)>),
}

#[derive(Debug, PartialEq)]
pub enum Reply {
    Done(Value),
    /// A reply of another schema (SwarmController.cpp:194-196).
    Outdated,
    /// Qt's `swarmFailed` (HgsClient.cpp:713-715), with the error.
    Failed(String),
}

fn message(stderr: &str) -> Option<String> {
    let text: String = stderr.trim().chars().take(4000).collect();
    (!text.is_empty()).then_some(text)
}

/// How an apply ended, read as Qt reads it: a JSON object, else "Invalid
/// project catalog response" (HgsClient.cpp:711-715), then the schema check
/// (SwarmController.cpp:194). Every failure is sent again, so whether a
/// failed apply landed does not change what happens next.
pub fn classify(exit: Exit) -> Reply {
    match exit {
        Exit::Code { code: 0, stdout, .. } => match serde_json::from_slice::<Value>(&stdout) {
            Ok(reply) if reply["schema"] == 1 && reply["organization"].is_object() => Reply::Done(reply),
            Ok(reply) if reply.is_object() => Reply::Outdated,
            _ => Reply::Failed(INVALID.into()),
        },
        Exit::Code { code, stderr, .. } => {
            Reply::Failed(message(&stderr).unwrap_or_else(|| format!("hgs exited with code {code}")))
        }
        Exit::StartFailed(error) => Reply::Failed(error),
        Exit::TimedOut => Reply::Failed("hgs did not answer in time".into()),
        Exit::Abnormal { stderr } => Reply::Failed(message(&stderr).unwrap_or_else(|| "hgs ended abnormally".into())),
        Exit::WriteFailed => Reply::Failed("hgs did not take the request".into()),
    }
}

/// `hgs swarm apply` with compact JSON input, this computer only (HgsClient.cpp:706-719).
pub fn invocation(input: &Value) -> Invocation {
    Invocation {
        args: vec!["swarm".into(), "apply".into()],
        stdin: Some(serde_json::to_vec(input).expect("the request serializes")),
        timeout: APPLY_TIMEOUT,
        request_id: None,
    }
}

fn saved(tickets: Vec<Ticket>) -> Vec<(Ticket, EditOutcome)> {
    tickets.into_iter().map(|ticket| (ticket, EditOutcome::Done { conflicts: 0 })).collect()
}

#[derive(Debug)]
struct Sent {
    desired: Value,
    /// The edits this apply carries; each is answered by it.
    tickets: Vec<Ticket>,
}

/// SwarmController's state (SwarmController.h:37-46).
#[derive(Debug, Default)]
pub struct Controller {
    /// m_base: the organization the edits start from.
    base: Option<Value>,
    value: Option<Arc<Value>>,
    membership: Arc<BTreeMap<String, String>>,
    /// m_versions.
    versions: Map<String, Value>,
    digest: String,
    /// m_desired, when it differs from the base.
    desired: Option<Value>,
    desired_membership: Option<Arc<BTreeMap<String, String>>>,
    /// Edits no apply has carried yet.
    queued: Vec<Ticket>,
    /// m_sent and the request (busy while Some).
    sent: Option<Sent>,
    /// m_flush (SwarmController.cpp:64-65).
    flush_at: Option<u64>,
    /// The next `refresh` that sends `apply` (m_poll, 55, 96-101); a failure arms it.
    refresh_at: Option<u64>,
    /// m_ready: an initialized catalog was observed. Once set, it stays.
    ready: bool,
    /// The organization topic ended: new edits wait for its next state.
    ended: bool,
    /// A draft of the last run waits for the first state (Qt's `start`, 76-89).
    restored: bool,
    /// Pending edits changed since the draft file was last given out.
    draft_changed: bool,
    /// The operations the last acknowledged write made, per token, until a state shows them.
    guard: BTreeMap<String, Value>,
    guard_until: u64,
    status: Option<(String, bool)>,
    conflicts: usize,
    source_error: Option<String>,
    revision: u64,
    tickets: Ticket,
}

impl Controller {
    /// Whether edits are accepted.
    pub fn ready(&self) -> bool {
        self.ready && !self.ended
    }

    pub fn busy(&self) -> bool {
        self.sent.is_some()
    }

    pub fn pending(&self) -> bool {
        self.desired.is_some()
    }

    /// Changes whenever the presented organization does.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// When `due` next has something to do: the flush or the tick.
    pub fn next_due(&self) -> Option<u64> {
        match (self.flush_at, self.refresh_at) {
            (Some(flush), Some(refresh)) => Some(flush.min(refresh)),
            (flush, refresh) => flush.or(refresh),
        }
    }

    /// What the list shows: the desired organization while edits are
    /// pending (Qt shows its m_organization), else the base.
    pub fn presented(&self) -> Option<(&Value, Arc<BTreeMap<String, String>>)> {
        match (&self.desired, &self.base) {
            (Some(desired), _) => Some((desired, self.desired_membership.clone().unwrap_or_default())),
            (None, Some(base)) => Some((base, Arc::clone(&self.membership))),
            (None, None) => None,
        }
    }

    /// The `swarm get` object of what the list shows, for the Projects screen.
    pub fn presented_value(&self) -> Option<Arc<Value>> {
        let value = self.value.as_ref()?;
        match &self.desired {
            None => Some(Arc::clone(value)),
            Some(desired) => {
                let mut copy = (**value).clone();
                copy["organization"] = desired.clone();
                Some(Arc::new(copy))
            }
        }
    }

    fn reason(&self) -> String {
        if let (None, Some(error)) = (&self.base, &self.source_error) {
            return unavailable(error);
        }
        match &self.value {
            Some(value) if value["initialized"] != true => NOT_SET_UP.into(),
            Some(_) if self.ended => STOPPED.into(),
            _ => STARTING.into(),
        }
    }

    pub fn view(&self) -> SyncView {
        let (status, status_error) = self.status.clone().unwrap_or_default();
        SyncView {
            ready: self.ready(),
            reason: (!self.ready()).then(|| self.reason()),
            busy: self.busy(),
            status,
            status_error,
            conflicts: self.conflicts,
        }
    }

    fn adopt(&mut self, observation: Observation) {
        self.status = Some(status_of(&observation.value));
        self.conflicts = observation.value["conflicts"].as_array().map_or(0, Vec::len);
        self.ready |= observation.value["initialized"] == true;
        self.base = Some(observation.value["organization"].clone());
        self.versions = observation.versions.as_object().cloned().unwrap_or_default();
        self.digest = observation.digest;
        self.membership = observation.membership;
        self.value = Some(observation.value);
        self.desired = None;
        self.desired_membership = None;
        self.source_error = None;
        self.revision += 1;
    }

    /// A state from the source (the topic, or the core's `swarm get` poll).
    /// Not adopted while an apply runs or edits are pending
    /// (SwarmController.cpp:204-206), or while it lacks this client's last
    /// write. After a failed apply it is the tick that sends it again, where
    /// Qt's `refresh` would send `apply` instead of reading (96-101).
    pub fn observe(&mut self, observation: Observation, now: u64) -> bool {
        self.ended = false;
        if self.busy() {
            return false;
        }
        if self.pending() {
            if self.restored {
                // Qt's first reply after a start with a draft: ready (197), the
                // base kept since it was a read (203), and the flush sends the
                // draft from the saved base and versions (204-205).
                self.ready |= observation.value["initialized"] == true;
                if self.ready {
                    self.restored = false;
                    self.flush_at = Some(now + FLUSH_MS);
                }
            } else if self.refresh_at.is_some() {
                self.refresh_at = Some(now);
            }
            return false;
        }
        if now < self.guard_until {
            let shows = |token: &String, ops: &Value| {
                let heads = observation.versions.get(token).and_then(Value::as_array);
                ops.as_array().into_iter().flatten().all(|op| heads.is_some_and(|heads| heads.contains(op)))
            };
            if !self.guard.iter().all(|(token, ops)| shows(token, ops)) {
                return false;
            }
        }
        self.guard.clear();
        self.adopt(observation);
        true
    }

    pub fn source_failed(&mut self, message: &str) {
        self.source_error = Some(message.to_owned());
        self.status = Some((unavailable(message), true));
    }

    /// The topic ended (`OrganizationUpdate::Ended`). New edits wait for its
    /// next state; pending ones are still sent, since applies are local.
    pub fn source_ended(&mut self) {
        self.ended = true;
    }

    /// An edit (Qt's `edit`, SwarmController.cpp:90-95) on what the list shows.
    pub fn edit(&mut self, intent: &Intent, now: u64) -> Result<Ticket, String> {
        if !self.ready() {
            return Err(self.reason());
        }
        let current = self.desired.as_ref().or(self.base.as_ref()).ok_or_else(|| self.reason())?;
        let next = edit::apply(current, intent)?;
        self.desired_membership = Some(Arc::new(members(&next)));
        self.desired = Some(next);
        self.draft_changed = true;
        self.revision += 1;
        self.tickets += 1;
        self.queued.push(self.tickets);
        if !self.busy() {
            // Restarted by each edit while idle (94), also after a failure.
            self.flush_at = Some(now + FLUSH_MS);
        }
        Ok(self.tickets)
    }

    /// The flush (SwarmController.cpp:65) or the tick (`refresh`, 96-101).
    pub fn due(&mut self, now: u64) -> Due {
        let reached = |at: Option<u64>| at.is_some_and(|at| now >= at);
        if self.busy() || !self.ready || !(reached(self.flush_at) || reached(self.refresh_at)) {
            return Due::Nothing;
        }
        self.flush_at = None;
        self.refresh_at = None;
        let tickets = std::mem::take(&mut self.queued);
        let (Some(base), Some(desired)) = (&self.base, &self.desired) else {
            return Due::Settled(saved(tickets));
        };
        if !edit::changes_shared(base, desired) {
            // m_base == m_desired: nothing to send (65, 100).
            self.desired = None;
            self.desired_membership = None;
            self.draft_changed = true;
            return Due::Settled(saved(tickets));
        }
        // send (102-110): the held base, desired and versions. After a
        // failure they are unchanged, so the resend is the same request.
        let input = json!({"base": edit::shared(base), "desired": edit::shared(desired),
                           "versions": Value::Object(self.versions.clone())});
        self.sent = Some(Sent { desired: desired.clone(), tickets });
        Due::Apply(invocation(&input))
    }

    /// The apply's exit (Qt's `received` and `swarmFailed`, SwarmController.cpp:191-219, 67-73):
    /// the answers of the edits it carried.
    pub fn finish(&mut self, exit: Exit, now: u64) -> Vec<(Ticket, EditOutcome)> {
        let Some(sent) = self.sent.take() else {
            return Vec::new();
        };
        match classify(exit) {
            Reply::Done(reply) => self.acknowledged(sent, reply, now),
            Reply::Outdated => self.kept(sent, OUTDATED.into(), now),
            Reply::Failed(error) => self.kept(sent, unavailable(&error), now),
        }
    }

    fn acknowledged(&mut self, sent: Sent, reply: Value, now: u64) -> Vec<(Ticket, EditOutcome)> {
        self.refresh_at = None;
        // A new base and versions for the edits made meanwhile, or none left (persist, 205, 209).
        self.draft_changed = true;
        let written = reply["written"].as_object().cloned().unwrap_or_default();
        for (token, ops) in &written {
            self.versions.insert(token.clone(), ops.clone());
            self.guard.insert(token.clone(), ops.clone());
        }
        if !written.is_empty() {
            self.guard_until = now + GUARD_MS;
        }
        let conflicts = reply["conflicts"].as_array().map_or(0, |list| {
            list.iter()
                .filter(|conflict| conflict["key"].as_str().is_some_and(|key| written.contains_key(key)))
                .count()
        });
        let outcome = EditOutcome::Done { conflicts };
        let settled = sent.tickets.iter().map(|ticket| (*ticket, outcome.clone())).collect();
        if self.desired.as_ref() == Some(&sent.desired) {
            self.desired = None;
            self.desired_membership = None;
        }
        if self.desired.is_some() {
            // Edits made meanwhile go next, from what was sent (203-205).
            self.base = Some(sent.desired);
            self.flush_at = Some(now + FLUSH_MS);
        } else if let Some(observation) = Observation::from_reply(reply) {
            let guard = std::mem::take(&mut self.guard);
            self.adopt(observation);
            self.guard = guard;
        } else {
            self.base = Some(sent.desired);
        }
        settled
    }

    /// Qt's failure path (67-73) and schema check (194-196): only the status
    /// changes. Base, desired and versions stay, so the next tick sends the
    /// same apply (refresh, 96-101), and edits queued meanwhile ride on it.
    fn kept(&mut self, sent: Sent, status: String, now: u64) -> Vec<(Ticket, EditOutcome)> {
        self.status = Some((status.clone(), true));
        self.refresh_at = Some(now + REFRESH_MS);
        sent.tickets.into_iter().map(|ticket| (ticket, EditOutcome::Kept { detail: status.clone() })).collect()
    }

    /// The draft of the last run (Qt's `start`, SwarmController.cpp:76-89):
    /// the saved base, desired state and versions. The list shows the draft at
    /// once. The controller stays not ready, so nothing is sent and no edit is
    /// taken before the first state (`observe`). Ignored once a base is held.
    pub fn restore(&mut self, draft: Draft) {
        if self.base.is_some() {
            return;
        }
        self.membership = Arc::new(members(&draft.base));
        self.desired_membership = Some(Arc::new(members(&draft.desired)));
        self.base = Some(draft.base);
        self.desired = Some(draft.desired);
        self.versions = draft.versions;
        self.restored = true;
        self.revision += 1;
    }

    /// The draft file's next content, once after pending edits changed: the
    /// base, desired state and versions while edits are pending, else its
    /// removal. Qt saves its draft with every change (SessionsWindow.cpp:2496)
    /// and its base and versions with `persist` (SwarmController.cpp:111-130).
    pub fn take_draft(&mut self) -> Option<DraftWrite> {
        if !std::mem::take(&mut self.draft_changed) {
            return None;
        }
        Some(match (&self.base, &self.desired) {
            (Some(base), Some(desired)) => {
                DraftWrite::Save(Draft::new(base.clone(), desired.clone(), self.versions.clone()).to_json())
            }
            _ => DraftWrite::Remove,
        })
    }
}
```

`client/core/src/swarm/draft.rs`:

```rust
//! The pending organization edits on disk, so a restart sends them again, as
//! Qt does: it saves its draft with every change (`workspace/organization`,
//! tray/src/SessionsWindow.cpp:2496) and its observed base and versions
//! (SwarmController.cpp:111-130). After a restart it sends the draft from them
//! once its first read answered (SwarmController.cpp:76-89, 191-206). The
//! controller ([`super::sync::Controller::take_draft`]) says what the file
//! holds; the Tauri hub keeps it beside `organization-layout.json`.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The draft file in the app data directory; a replay keeps its own.
pub const FILE: &str = "organization-draft.json";
pub const REPLAY_FILE: &str = "organization-draft-replay.json";
const VERSION: u32 = 1;

/// Pending edits: the base they start from, the organization they make and
/// the versions observed with the base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub version: u32,
    pub base: Value,
    pub desired: Value,
    pub versions: Map<String, Value>,
}

impl Draft {
    pub fn new(base: Value, desired: Value, versions: Map<String, Value>) -> Self {
        Self { version: VERSION, base, desired, versions }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("the draft serializes")
    }

    /// A draft of this version with both organizations; anything else is none.
    pub fn from_json(bytes: &[u8]) -> Option<Self> {
        let draft: Self = serde_json::from_slice(bytes).ok()?;
        (draft.version == VERSION && draft.base.is_object() && draft.desired.is_object()).then_some(draft)
    }
}

/// What the draft file becomes.
#[derive(Debug, PartialEq)]
pub enum DraftWrite {
    Save(Vec<u8>),
    Remove,
}

/// The saved draft; a missing or unreadable file is none.
pub fn load(path: &Path) -> Option<Draft> {
    std::fs::read(path).ok().and_then(|bytes| Draft::from_json(&bytes))
}

/// Writes a temporary file beside the target, then renames it over it, as
/// client/core/src/readmarks.rs:117-140 does. A failure keeps the old file.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}-{}.tmp", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    let temporary = PathBuf::from(temporary);
    if let Some(folder) = path.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder)?;
    }
    let written = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// Saves or removes the draft; removing a missing file is no error.
pub fn store(path: &Path, change: &DraftWrite) -> io::Result<()> {
    match change {
        DraftWrite::Save(bytes) => write(path, bytes),
        DraftWrite::Remove => match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-draft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_draft_file_is_replaced_whole_and_removed_when_settled() {
        let dir = scratch("store");
        let path = dir.join(FILE);
        let draft = Draft::new(json!({"projects": []}), json!({"projects": [{"id": "alpha"}]}), Map::new());
        store(&path, &DraftWrite::Save(draft.to_json())).unwrap();
        assert_eq!(load(&path), Some(draft.clone()));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from(FILE)], "no temporary file is left");
        let newer = Draft::new(draft.desired.clone(), json!({"projects": []}), Map::new());
        store(&path, &DraftWrite::Save(newer.to_json())).unwrap();
        assert_eq!(load(&path), Some(newer));
        store(&path, &DraftWrite::Remove).unwrap();
        assert!(!path.exists());
        store(&path, &DraftWrite::Remove).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_or_other_draft_is_none() {
        assert_eq!(Draft::from_json(b"not json"), None);
        assert_eq!(Draft::from_json(br#"{"version": 2, "base": {}, "desired": {}, "versions": {}}"#), None);
        assert_eq!(Draft::from_json(br#"{"version": 1, "base": null, "desired": {}, "versions": {}}"#), None);
        assert!(Draft::from_json(br#"{"version": 1, "base": {}, "desired": {}, "versions": {}}"#).is_some());
        assert_eq!(load(&scratch("missing").join(FILE)), None);
    }
}
```

- [ ] **Step 6: Run** the unit tests, `--test swarm_sync`, all core tests, clippy and fmt. Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add client/core/src/swarm/sync.rs client/core/src/swarm/draft.rs client/core/src/views/projects.rs client/core/tests/swarm_sync.rs client/core/tests/support/fake_hgs_swarm.py
git commit -m "feat(client): SwarmController's apply rules for the organization, with Qt's resend and saved draft"
```

---

### Task 5: The pump carries the grouping, never the organization (`client/src-tauri/src/pump.rs`)

**Files:**
- Modify: `client/src-tauri/src/pump.rs` (on top of PR #395's mirror)
- Test: unit test in `pump.rs`

**Interfaces:**
- Consumes: `Grouping`, `Layout`, `SyncView`, `Intent` (Tasks 1-3).
- Produces:

```rust
impl Pump {
    pub fn drain_with(&self, changes: Receiver<Change>, observe: impl Fn(&Change) + Send + 'static);
    pub fn present(&self, organization: &Value, membership: Arc<BTreeMap<String, String>>, revision: u64, sync: SyncView);
    pub fn set_sync(&self, sync: SyncView);
    pub fn edited(&self, intent: &Intent);
    pub fn set_collapsed(&self, project: &str, collapsed: bool);
    pub fn members_of(&self, project: &str) -> Vec<(String, String)>;
    pub fn default_project(&self) -> String;
    pub fn take_renames(&self) -> Vec<(String, String)>;
    pub fn take_layout(&self) -> Option<Vec<u8>>;
    pub fn load_layout(&self, bytes: &[u8]);
}
```

- [ ] **Step 1: Write the failing test** (in `pump.rs` tests):

```rust
#[test]
fn the_batch_carries_each_rows_project_and_the_headers_never_the_organization() {
    use zerus_client_core::swarm::grouping::SyncView;
    let local = || ComputerId(String::new());
    let pump = Pump::default();
    pump.push(Change::Replace {
        computer: local(),
        rows: vec![("run:a".into(), ViewRow { machine: "workstation".into(), name: "claude/p/a".into(), ..ViewRow::default() })],
    });
    let organization = serde_json::json!({"default_project": "ungrouped", "projects": [
        {"id": "alpha", "name": "Alpha", "sessions": ["workstation\nclaude/p/a"]},
        {"id": "ungrouped", "name": "Inbox", "sessions": []}]});
    pump.push(Change::Organization {
        computer: local(),
        update: OrganizationUpdate::State {
            value: Arc::new(serde_json::json!({"organization": organization.clone()})),
            digest: "d1".into(),
            membership: Arc::default(),
        },
    });
    pump.present(&organization, Arc::new(zerus_client_core::swarm::members(&organization)), 1, SyncView::default());
    let batch = pump.take(PARK);
    assert!(batch.organizations.is_empty(), "the organization stays in Rust");
    assert_eq!(batch.groups.len(), 1);
    assert_eq!(batch.groups[0].group, "alpha");
    assert_eq!(batch.grouping.as_ref().unwrap().headers.len(), 2);
    let json = serde_json::to_string(&batch).unwrap();
    assert!(!json.contains("organizations") && !json.contains("\"sessions\""));
    pump.resend();
    let again = pump.take(PARK);
    assert_eq!(again.groups.len(), 1, "a reloaded webview gets every row's project again");
    assert!(again.grouping.is_some());
    pump.set_collapsed("alpha", true);
    assert!(pump.take(PARK).grouping.unwrap().headers[0].collapsed, "a collapse wakes a parked take");
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --locked` in `client/src-tauri`. Expected: compile errors.

- [ ] **Step 3: Implement:**
- `Inner` gains `grouping: zerus_client_core::swarm::grouping::Grouping` (its `Default` is an empty layout).
- `push` calls `inner.grouping.observe(&change)` after `inner.mirror.observe(&change)`.
- `take`:

```rust
        let mut batch = inner.coalescer.take();
        // The organization stays in Rust; the webview gets each row's project
        // and the headers (zerus-5wmh.31), only when they changed.
        batch.organizations.clear();
        let (groups, grouping) = inner.grouping.take();
        batch.groups = groups;
        batch.grouping = grouping;
        batch
```

- `resend` calls `inner.grouping.resend()` before queuing the mirror's changes.
- `drain` becomes `self.drain_with(changes, |_| {})`. `drain_with` runs `observe(&change)` before `pump.push(change)` for each change, on the same thread.
- A private `fn wake(&self, mut inner: MutexGuard<'_, Inner>)` sets `dirty` and notifies. `present`, `set_sync` and `set_collapsed` end with it, so the next `take` returns at once.
- `present` calls `grouping.present` then `grouping.set_sync`. The other methods forward to the grouping under the lock. `load_layout` replaces the grouping with `Grouping::new(Layout::from_json(bytes))`.
- **Lock order:** the organization hub's lock, then the pump's. No pump method calls the hub.

- [ ] **Step 4: Run** the tests and `cargo +1.99.1 clippy --locked --all-targets -- -D warnings` in `client/src-tauri`, plus the same with `--no-default-features`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/pump.rs
git commit -m "feat(client): the pump sends each row's project and the headers, never the organization"
```

---

### Task 6: The organization hub, its commands and the sources (`client/src-tauri/src/organization.rs`)

**Files:**
- Create: `client/src-tauri/src/organization.rs`
- Modify: `client/src-tauri/src/commands.rs`:
  - `Live` gains `organization: OnceLock<Organization>` and `AppState::set_organization`;
  - `Live::start(self: &Arc<Self>)` sets the snapshot source, follows the organization and drains with the hub's observer;
  - `Live::organization_snapshot()`;
  - `AppState::pump()`, unless 3a's is on the branch.
- Modify: `client/src-tauri/src/replay.rs` (the local computer follows the organization; `Replay::organization_snapshot()`)
- Modify: `client/src-tauri/src/lib.rs` (`#[cfg(feature = "live")] mod organization;`, the hub in `setup`, seven commands in the live `handler()`)
- Test: unit tests in `organization.rs`

**Interfaces:**
- Consumes:
  - `Controller`, `Observation`, `EditOutcome`, `Due`, `Intent`, `by_name`, `STOPPED`, `swarm::draft` (Tasks 1-4) and the Pump methods (Task 5);
  - `Fleet::follow_organization`, `Fleet::organization_snapshot`, `OrganizationUpdate`, `OrganizationSnapshot` (PR #395);
  - `run_limited`, `Read::Swarm` (its reply limit; phase 2 and PR #390);
  - `actions::actions_allowed` (phase 2).
- Produces (frontend contract, snake_case JSON):

```rust
pub type SnapshotSource = Arc<dyn Fn() -> Option<OrganizationSnapshot> + Send + Sync>;
pub type Presented = Arc<dyn Fn(Arc<Value>, String, (String, bool)) + Send + Sync>;
pub fn layout_file(replay: bool) -> &'static str;
pub fn draft_file(replay: bool) -> &'static str;
#[derive(Clone)] pub struct Organization;
impl Organization {
    pub fn new(pump: Pump, hgs: PathBuf, allowed: bool, layout: PathBuf, draft_path: PathBuf) -> Self;
    pub fn set_source(&self, source: SnapshotSource);
    pub fn set_presented(&self, presented: Presented);      // Task 9
    pub fn observe(&self, change: &Change);
    pub fn submit(&self, intent: Intent) -> EditOutcome;    // blocking
}
#[tauri::command] pub async fn move_to_project(state: State<'_, Organization>, members: Vec<String>, project: String, before: Option<String>) -> Result<EditOutcome, String>;
#[tauri::command] pub async fn create_project(state: State<'_, Organization>, name: String, members: Vec<String>) -> Result<EditOutcome, String>;
#[tauri::command] pub async fn rename_project(state: State<'_, Organization>, project: String, name: String) -> Result<EditOutcome, String>;
#[tauri::command] pub async fn delete_project(state: State<'_, Organization>, project: String, destination: String) -> Result<EditOutcome, String>;
#[tauri::command] pub async fn move_project(state: State<'_, Organization>, project: String, before: Option<String>) -> Result<EditOutcome, String>;
#[tauri::command] pub async fn sort_project(state: State<'_, Organization>, project: String) -> Result<EditOutcome, String>;
#[tauri::command] pub fn set_project_collapsed(state: State<'_, Organization>, project: String, collapsed: bool);
```

Behaviour:
- **Observe** (from the drain thread, before the pump):
  - only `Change::Organization` of this computer acts;
  - for a `State`, the source's `OrganizationSnapshot` must carry the update's digest; otherwise a newer state is on its way, and this one is skipped. The `Observation` takes the update's `value` and `membership` and the snapshot's `versions`;
  - `Failed` goes to `source_failed`, and `Ended` to `source_ended`;
  - every observed state wakes the worker: after a failed apply it is the tick that sends the apply again (`Controller::observe`; `SwarmController.cpp:96-101`).
- **Publish** after every change: the presented organization goes to the pump with the `SyncView`. In replay mode without `ZERUS_CLIENT_HGS`, the view is not ready, with "Actions in replay mode need ZERUS_CLIENT_HGS." (phase 2's text). Task 9's `Presented` callback runs when the revision or the status line changed, as Qt's `statusChanged` reaches the Projects page (`SessionsWindow.cpp:1653`).
- **Submit:** refused in a blocked replay; otherwise `Controller::edit`, then `pump.edited`, publish and wake the worker. It waits for the ticket's answer, which the first apply that carries the edit gives: `Done`, or `Kept` with Qt's status line when that apply failed, while the edit stays pending. A worker that is gone answers `Kept("Project sync stopped.")`.
- **The worker thread** (`zerus-organization`):
  - it turns the pump's renames into `Intent::Renamed` edits while ready;
  - it runs `due`. An `Apply` runs `run_limited(hgs, invocation, &[], Read::Swarm.limit())` without the hub's lock, then `finish`. A too large reply counts as `Abnormal`: a failure, kept for the next tick like any other;
  - before `due`, it writes the draft file when pending edits changed (`take_draft`, `draft::store`), so an edit is on disk before its apply runs, and the file goes when the edits settled (`SessionsWindow.cpp:2496`);
  - it saves a changed layout at most once a second, through a temporary file and a rename (`draft::write`, as `readmarks.rs:117-140`);
  - it then waits until the next flush or tick (`next_due`), at most 1 s, or until a state or an edit wakes it.
- **Start:** `new` loads the draft file into the controller (`Controller::restore`) before the worker starts, and publishes it, so the list shows the pending edits at once. Nothing is sent before the first organization state (Qt's `start`, `SwarmController.cpp:76-89`, `SessionsWindow.cpp:1661`).
- **Delete project** passes the project's listed rows (`pump.members_of`), and **Sort sessions by name** passes them through `by_name`. **New project** makes its ID with `uuid::Uuid::new_v4()`.
- **Replay:**
  - `Replay::start` calls `organization_mut().follow()` on this computer's `CatalogState` before the first frame;
  - `Replay::organization_snapshot()` returns its `organization().snapshot()`;
  - the layout file is `organization-layout-replay.json` and the draft file `organization-draft-replay.json`, so replays never touch the real ones;
  - a replay starts from the fixture's catalog, so `setup` removes a leftover replay draft before `new`.

- [ ] **Step 1: Write the failing tests** (in `organization.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zerus_client_core::swarm::draft::Draft;
    use zerus_client_core::swarm::edit::shared;
    use zerus_client_core::{ComputerId, ViewRow};

    const R: &str = "workstation\nclaude/sample-project/review";

    fn full(digest: &str) -> Value {
        json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": [],
               "organization": {"version": 2, "default_project": "ungrouped", "projects": [
                   {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": [R]},
                   {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": []}]},
               "versions": {}})
    }

    /// The fake hgs, its `swarm apply` answering `apply` in turn.
    fn fake_with(dir: &std::path::Path, apply: Value) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let hgs = dir.join("hgs");
        std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/support/fake_hgs_swarm.py"), &hgs).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dir.join("config.json"), serde_json::to_vec(&json!({"get": [{"stdout": full("d1")}], "apply": apply})).unwrap()).unwrap();
        hgs
    }

    fn fake(dir: &std::path::Path) -> PathBuf {
        fake_with(dir, json!([{"stdout": full("d2")}]))
    }

    /// The `swarm apply` lines of the fake's log.
    fn applies(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("log.ndjson"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("\"apply\""))
            .map(str::to_owned)
            .collect()
    }

    fn state(digest: &str) -> Change {
        let value = full(digest);
        let mut without = value.clone();
        without.as_object_mut().unwrap().remove("versions");
        Change::Organization {
            computer: ComputerId::local(),
            update: OrganizationUpdate::State {
                value: Arc::new(without),
                digest: digest.into(),
                membership: Arc::new(zerus_client_core::swarm::members(&value["organization"])),
            },
        }
    }

    fn snapshot(digest: &'static str) -> SnapshotSource {
        Arc::new(move || Some(OrganizationSnapshot { digest: digest.into(), base: full(digest)["organization"].clone(), versions: json!({}) }))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-organization-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_blocked_replay_refuses_edits_with_the_replay_text() {
        let dir = scratch("replay");
        let organization = Organization::new(Pump::default(), fake(&dir), false, dir.join("layout.json"), dir.join("draft.json"));
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(Intent::Move { members: vec![R.into()], project: "ungrouped".into(), before: None });
        assert_eq!(outcome, EditOutcome::Refused { detail: REPLAY_NEEDS_HGS.into() });
    }

    #[test]
    fn a_state_of_another_digest_than_the_snapshot_is_skipped() {
        let dir = scratch("digest");
        let pump = Pump::default();
        let organization = Organization::new(pump.clone(), fake(&dir), true, dir.join("layout.json"), dir.join("draft.json"));
        organization.set_source(snapshot("d2"));
        organization.observe(&state("d1"));
        assert!(!pump.take(crate::pump::PARK).grouping.is_some_and(|g| g.loaded), "versions of another state are never paired");
        organization.observe(&state("d2"));
        assert!(pump.take(crate::pump::PARK).grouping.unwrap().loaded);
    }

    #[test]
    fn an_edit_runs_one_apply_and_answers_with_its_outcome() {
        let dir = scratch("apply");
        let pump = Pump::default();
        pump.push(Change::Upsert { computer: ComputerId::local(), key: "run:1".into(),
            row: ViewRow { machine: "workstation".into(), name: "claude/sample-project/review".into(), ..ViewRow::default() } });
        let organization = Organization::new(pump.clone(), fake(&dir), true, dir.join("layout.json"), dir.join("draft.json"));
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(Intent::Move { members: vec![R.into()], project: "ungrouped".into(), before: None });
        assert_eq!(outcome, EditOutcome::Done { conflicts: 0 });
        assert_eq!(applies(&dir).len(), 1);
        std::thread::sleep(Duration::from_millis(2200));
        assert!(dir.join("layout.json").exists(), "the layout is saved within two seconds");
    }

    // SwarmController.cpp:67-73, 96-101: the edit stays, and the topic's next state sends it again.
    #[test]
    fn a_failed_apply_answers_kept_and_the_next_state_sends_it_once_more() {
        let dir = scratch("resend");
        let refused = json!({"exit": 1, "stderr": "hgs: session belongs to more than one project\n"});
        let organization = Organization::new(Pump::default(), fake_with(&dir, json!([refused, {"stdout": full("d2")}])), true, dir.join("layout.json"), dir.join("draft.json"));
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(Intent::Move { members: vec![R.into()], project: "ungrouped".into(), before: None });
        assert_eq!(outcome, EditOutcome::Kept { detail: "Project sync unavailable: hgs: session belongs to more than one project".into() });
        assert_eq!(applies(&dir).len(), 1);
        organization.observe(&state("d1"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while applies(&dir).len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
        let log = applies(&dir);
        assert_eq!(log.len(), 2, "one resend, and no more once it landed");
        let stdin = |line: &str| serde_json::from_str::<Value>(line).unwrap()["stdin"].clone();
        assert_eq!(stdin(&log[0]), stdin(&log[1]), "the same base, desired and versions");
    }

    // Qt loads its draft at start (SessionsWindow.cpp:275-276, 1661) and sends it
    // once its first read answered (SwarmController.cpp:76-89, 197, 203-205).
    #[test]
    fn a_saved_draft_waits_for_the_first_state_then_one_apply_sends_it() {
        let dir = scratch("draft");
        let hgs = fake(&dir);
        let base = full("d1")["organization"].clone();
        let mut desired = base.clone();
        desired["projects"][0]["sessions"] = json!([]);
        desired["projects"][1]["sessions"] = json!([R]);
        let versions = serde_json::Map::new();
        draft::write(&dir.join("draft.json"), &Draft::new(base.clone(), desired.clone(), versions.clone()).to_json()).unwrap();
        let pump = Pump::default();
        let organization = Organization::new(pump.clone(), hgs, true, dir.join("layout.json"), dir.join("draft.json"));
        organization.set_source(snapshot("d1"));
        assert!(pump.take(crate::pump::PARK).grouping.is_some_and(|g| g.loaded), "the list shows the draft before any state");
        std::thread::sleep(Duration::from_millis(400));
        assert!(applies(&dir).is_empty(), "nothing before the first state");
        assert!(dir.join("draft.json").exists());
        organization.observe(&state("d1"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while (applies(&dir).is_empty() || dir.join("draft.json").exists()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
        let log = applies(&dir);
        assert_eq!(log.len(), 1, "exactly one apply");
        let entry: Value = serde_json::from_str(&log[0]).unwrap();
        let stdin: Value = serde_json::from_str(entry["stdin"].as_str().unwrap()).unwrap();
        assert_eq!(stdin, json!({"base": shared(&base), "desired": shared(&desired), "versions": versions}), "the saved draft");
        assert!(!dir.join("draft.json").exists(), "removed once it landed");
    }
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --locked` in `client/src-tauri`. Expected: compile errors.

- [ ] **Step 3: Implement** `client/src-tauri/src/organization.rs`:

```rust
//! The shared project organization on the Tauri side (zerus-5wmh.31, .18).
//! This computer's `organization` topic feeds the core's write controller
//! (`swarm::sync::Controller`). The presented organization goes to the pump's
//! grouping, which sends the webview each row's project and the headers.
//! Edits run as one `hgs swarm apply` at a time on this module's thread,
//! which also saves the local layout. A failed apply stays pending in the
//! controller, and this thread sends it again on the next tick: 5 s later, or
//! when the topic's next state arrives (SwarmController.cpp:67-73, 96-101).
//! Pending edits are kept in a draft file, written before their apply and
//! removed once they settled; a restart loads it and sends it after the first
//! state, as Qt does (SessionsWindow.cpp:2496, 1661; SwarmController.cpp:76-89).
//! The webview never receives the organization itself.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::State;
use zerus_client_core::reads::Read;
use zerus_client_core::swarm::draft;
use zerus_client_core::swarm::edit::Intent;
use zerus_client_core::swarm::grouping::{by_name, SyncView};
use zerus_client_core::swarm::sync::{Controller, Due, EditOutcome, Observation, Ticket, STOPPED};
use zerus_client_core::{run_limited, Change, Exit, OrganizationSnapshot, OrganizationUpdate};

use crate::pump::Pump;

/// Phase 2's refusal of a replay that would reach the real CLI.
pub const REPLAY_NEEDS_HGS: &str = "Actions in replay mode need ZERUS_CLIENT_HGS.";
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// The base and versions of the latest state: `Fleet::organization_snapshot`
/// live, the replay's own catalog in a replay.
pub type SnapshotSource = Arc<dyn Fn() -> Option<OrganizationSnapshot> + Send + Sync>;
/// Hands the presented `swarm get` object, the default project and the
/// controller's status line to the Projects screen (Task 9), as Qt's
/// `statusChanged` reaches its Projects page (SessionsWindow.cpp:1653).
pub type Presented = Arc<dyn Fn(Arc<Value>, String, (String, bool)) + Send + Sync>;

/// The layout file in the app data directory; a replay keeps its own.
pub fn layout_file(replay: bool) -> &'static str {
    if replay {
        "organization-layout-replay.json"
    } else {
        "organization-layout.json"
    }
}

/// The pending draft beside the layout; a replay keeps its own.
pub fn draft_file(replay: bool) -> &'static str {
    if replay {
        draft::REPLAY_FILE
    } else {
        draft::FILE
    }
}

#[derive(Default)]
struct Hub {
    sync: Controller,
    source: Option<SnapshotSource>,
    waiting: HashMap<Ticket, SyncSender<EditOutcome>>,
    presented: Option<Presented>,
    /// The revision and status line Projects last received.
    shown: Option<(u64, String, bool)>,
}

struct Shared {
    hub: Mutex<Hub>,
    wake: Condvar,
    pump: Pump,
    hgs: PathBuf,
    allowed: bool,
    layout: PathBuf,
    draft: PathBuf,
    clock: Instant,
}

/// The managed state of the organization commands; clones share it.
#[derive(Clone)]
pub struct Organization(Arc<Shared>);

impl Organization {
    pub fn new(pump: Pump, hgs: PathBuf, allowed: bool, layout: PathBuf, draft_path: PathBuf) -> Self {
        if let Ok(bytes) = std::fs::read(&layout) {
            pump.load_layout(&bytes);
        }
        let saved = draft::load(&draft_path);
        let organization = Self(Arc::new(Shared {
            hub: Mutex::default(),
            wake: Condvar::new(),
            pump,
            hgs,
            allowed,
            layout,
            draft: draft_path,
            clock: Instant::now(),
        }));
        {
            let mut hub = organization.lock();
            // The last run's pending edits (Qt's start, SwarmController.cpp:76-89):
            // shown at once, sent after the first state.
            if let Some(saved) = saved {
                hub.sync.restore(saved);
            }
            organization.publish(&mut hub);
        }
        let worker = organization.clone();
        std::thread::Builder::new()
            .name("zerus-organization".into())
            .spawn(move || worker.work())
            .expect("the organization thread starts");
        organization
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.hub.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        self.0.clock.elapsed().as_millis() as u64
    }

    pub fn set_source(&self, source: SnapshotSource) {
        self.lock().source = Some(source);
    }

    pub fn set_presented(&self, presented: Presented) {
        let mut hub = self.lock();
        hub.presented = Some(presented);
        hub.shown = None;
        self.publish(&mut hub);
    }

    /// This computer's organization changes, from the drain thread.
    pub fn observe(&self, change: &Change) {
        let Change::Organization { computer, update } = change else {
            return;
        };
        if !computer.is_local() {
            return;
        }
        let mut hub = self.lock();
        match update {
            OrganizationUpdate::State { value, digest, membership } => {
                let Some(snapshot) = hub.source.as_ref().and_then(|source| source()) else {
                    return;
                };
                // Base and versions of one digest; a newer state is on its way otherwise.
                if snapshot.digest != *digest {
                    return;
                }
                let observation = Observation {
                    digest: digest.clone(),
                    value: Arc::clone(value),
                    versions: snapshot.versions,
                    membership: Arc::clone(membership),
                };
                let now = self.now();
                if hub.sync.observe(observation, now) {
                    self.publish(&mut hub);
                }
                // After a failed apply, a state is the tick that sends it again.
                self.0.wake.notify_all();
            }
            OrganizationUpdate::Failed { message, .. } => {
                hub.sync.source_failed(message);
                self.publish(&mut hub);
            }
            OrganizationUpdate::Ended => {
                hub.sync.source_ended();
                self.publish(&mut hub);
            }
        }
    }

    fn view(&self, hub: &Hub) -> SyncView {
        let mut view = hub.sync.view();
        if !self.0.allowed {
            view.ready = false;
            view.reason = Some(REPLAY_NEEDS_HGS.into());
        }
        view
    }

    fn publish(&self, hub: &mut Hub) {
        let view = self.view(hub);
        let shown = (hub.sync.revision(), view.status.clone(), view.status_error);
        match hub.sync.presented() {
            Some((organization, membership)) => self.0.pump.present(organization, membership, hub.sync.revision(), view),
            None => self.0.pump.set_sync(view),
        }
        if hub.shown.as_ref() != Some(&shown) {
            if let (Some(presented), Some(value)) = (hub.presented.clone(), hub.sync.presented_value()) {
                presented(value, self.0.pump.default_project(), (shown.1.clone(), shown.2));
                hub.shown = Some(shown);
            }
        }
    }

    /// Runs one edit; answers once the first apply that carries it ended.
    /// A failed one answers `Kept`, and the edit stays pending for the next tick.
    pub fn submit(&self, intent: Intent) -> EditOutcome {
        let answer = {
            let mut hub = self.lock();
            if !self.0.allowed {
                return EditOutcome::Refused { detail: REPLAY_NEEDS_HGS.into() };
            }
            let ticket = match hub.sync.edit(&intent, self.now()) {
                Ok(ticket) => ticket,
                Err(detail) => return EditOutcome::Refused { detail },
            };
            self.0.pump.edited(&intent);
            let (sender, receiver) = sync_channel(1);
            hub.waiting.insert(ticket, sender);
            self.publish(&mut hub);
            self.0.wake.notify_all();
            receiver
        };
        answer.recv().unwrap_or(EditOutcome::Kept { detail: STOPPED.into() })
    }

    fn settle(hub: &mut Hub, settled: Vec<(Ticket, EditOutcome)>) {
        for (ticket, outcome) in settled {
            if let Some(sender) = hub.waiting.remove(&ticket) {
                let _ = sender.send(outcome);
            }
        }
    }

    fn work(&self) {
        let mut saved = Instant::now();
        let mut hub = self.lock();
        loop {
            // A rename seen in the catalog keeps its project (renameSession).
            for (from, to) in self.0.pump.take_renames() {
                let intent = Intent::Renamed { from, to };
                let now = self.now();
                if self.0.allowed && hub.sync.ready() && hub.sync.edit(&intent, now).is_ok() {
                    self.0.pump.edited(&intent);
                    self.publish(&mut hub);
                }
            }
            // The draft on disk before its apply runs, and gone once it settled.
            if let Some(change) = hub.sync.take_draft() {
                if let Err(error) = draft::store(&self.0.draft, &change) {
                    eprintln!("zerus-client: the pending project changes were not saved: {error}");
                }
            }
            match hub.sync.due(self.now()) {
                Due::Apply(invocation) => {
                    self.publish(&mut hub);
                    drop(hub);
                    let exit = run_limited(&self.0.hgs, &invocation, &[], Read::Swarm.limit())
                        .unwrap_or_else(|too| Exit::Abnormal { stderr: format!("the reply passed {} bytes", too.cap) });
                    hub = self.lock();
                    let now = self.now();
                    // A failure keeps the edits; the controller arms the next tick.
                    let settled = hub.sync.finish(exit, now);
                    Self::settle(&mut hub, settled);
                    self.publish(&mut hub);
                    continue;
                }
                Due::Settled(settled) => {
                    Self::settle(&mut hub, settled);
                    self.publish(&mut hub);
                    continue;
                }
                Due::Nothing => {}
            }
            if saved.elapsed() >= SAVE_EVERY {
                if let Some(bytes) = self.0.pump.take_layout() {
                    if let Err(error) = draft::write(&self.0.layout, &bytes) {
                        eprintln!("zerus-client: the project layout was not saved: {error}");
                    }
                }
                saved = Instant::now();
            }
            let now = self.now();
            let wait = hub.sync.next_due().map_or(1000, |at| at.saturating_sub(now)).clamp(1, 1000);
            hub = self
                .0
                .wake
                .wait_timeout(hub, Duration::from_millis(wait))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

async fn edit(
    state: State<'_, Organization>,
    intent: impl FnOnce(&Organization) -> Intent + Send + 'static,
) -> Result<EditOutcome, String> {
    let organization = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let intent = intent(&organization);
        organization.submit(intent)
    })
    .await
    .map_err(|error| error.to_string())
}

/// Move to project for one session or a selection: one `swarm apply` with
/// every member, from one snapshot. Members are `"<host>\n<session name>"`;
/// `before` places them before that member (a drop).
#[tauri::command]
pub async fn move_to_project(
    state: State<'_, Organization>,
    members: Vec<String>,
    project: String,
    before: Option<String>,
) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Move { members, project, before }).await
}

/// New project…, with the sessions it takes (none from the list's menu).
#[tauri::command]
pub async fn create_project(state: State<'_, Organization>, name: String, members: Vec<String>) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Create { id: uuid::Uuid::new_v4().to_string(), name, members }).await
}

#[tauri::command]
pub async fn rename_project(state: State<'_, Organization>, project: String, name: String) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Rename { project, name }).await
}

/// Delete project…: its members and the rows it lists go to `destination`.
#[tauri::command]
pub async fn delete_project(state: State<'_, Organization>, project: String, destination: String) -> Result<EditOutcome, String> {
    edit(state, move |organization| {
        let members = organization.0.pump.members_of(&project).into_iter().map(|(member, _)| member).collect();
        Intent::Delete { project, destination, members }
    })
    .await
}

#[tauri::command]
pub async fn move_project(state: State<'_, Organization>, project: String, before: Option<String>) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Reorder { project, before }).await
}

#[tauri::command]
pub async fn sort_project(state: State<'_, Organization>, project: String) -> Result<EditOutcome, String> {
    edit(state, move |organization| {
        let members = by_name(organization.0.pump.members_of(&project));
        Intent::Sort { project, members }
    })
    .await
}

/// Expand or collapse a project: this device's layout, no `hgs`.
#[tauri::command]
pub fn set_project_collapsed(state: State<'_, Organization>, project: String, collapsed: bool) {
    state.0.pump.set_collapsed(&project, collapsed);
}
```

`client/src-tauri/src/commands.rs`, inside `mod live`:
- `Live` gains `organization: std::sync::OnceLock<crate::organization::Organization>`. `AppState` gains `pub fn set_organization(&self, organization: crate::organization::Organization)` (`let _ = self.live.organization.set(organization);`), and `pub fn pump(&self) -> crate::pump::Pump`, unless 3a's is on the branch.
- `Live::organization_snapshot(&self) -> Option<OrganizationSnapshot>` locks `source` and asks the Fleet (`fleet.organization_snapshot(&ComputerId::local())`) or the Replay (`replay.organization_snapshot()`).
- `pub fn start(self: &Arc<Self>)`:
  - first, before locking `source`, when `self.organization.get()` is set, it calls `organization.set_source(Arc::new(move || weak.upgrade()?.organization_snapshot()))` with `weak = Arc::downgrade(self)`;
  - after starting a `Fleet`, it calls `let _ = fleet.follow_organization(&ComputerId::local());`;
  - then `self.pump.drain_with(changes, move |change| if let Some(o) = &organization { o.observe(change) })`.
- Lock order: hub, then `Live::source`. `start` never holds `source` while it takes the hub's lock.

`client/src-tauri/src/lib.rs` (live `setup`, after `Actions`):

```rust
                let pump = app.state::<commands::AppState>().pump();
                let allowed = actions::actions_allowed(std::env::var("ZERUS_CLIENT_REPLAY").ok().as_deref(),
                                                       std::env::var("ZERUS_CLIENT_HGS").ok().as_deref());
                let draft_path = dir.join(organization::draft_file(replay));
                if replay {
                    // A replay starts from the fixture's catalog: no draft of an earlier replay.
                    let _ = zerus_client_core::swarm::draft::store(&draft_path, &zerus_client_core::swarm::draft::DraftWrite::Remove);
                }
                let organization = organization::Organization::new(pump, zerus_client_core::default_hgs(), allowed,
                                                                   dir.join(organization::layout_file(replay)), draft_path);
                app.state::<commands::AppState>().set_organization(organization.clone());
                app.manage(organization);
```

Then register `organization::move_to_project`, `create_project`, `rename_project`, `delete_project`, `move_project`, `sort_project` and `set_project_collapsed` in the live `handler()`. The non-live handler is unchanged.

`client/src-tauri/src/replay.rs`: in `Replay::start`, call `computer.catalog.lock().0.organization_mut().follow()` on the first (local) computer before any frame plays. Add:

```rust
    /// The base and versions of the replayed organization, for edits against a fake hgs.
    pub fn organization_snapshot(&self) -> Option<zerus_client_core::OrganizationSnapshot> {
        let local = self.computers.first()?;
        let guard = local.catalog.lock().unwrap_or_else(PoisonError::into_inner);
        guard.0.organization().snapshot()
    }
```

- [ ] **Step 4: Run** the tests, `cargo +1.99.1 clippy --locked --all-targets -- -D warnings` (and `--no-default-features`), then all core tests. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock
git commit -m "feat(client): the organization hub follows the topic and runs guarded project edits"
```

---

### Task 7: The Sessions list grouped by the organization (frontend)

**Files:**
- Modify:
  - `client/src/types.ts`: `GroupOut`, `HeaderOut`, `GroupingOut`, `EditOutcome`; `Batch.groups?` and `Batch.grouping?`; `isEmptyBatch` covers them;
  - `client/src/bridge.ts`: the seven wrappers. Outside Tauri, edits resolve `{outcome: "refused", detail: "Project changes need the desktop app."}`, and `setProjectCollapsed` resolves;
  - `client/src/store/actions.ts`: `notify` returned and exported (Shared files);
  - `client/src/store/session.ts`: `receive` calls `applyGrouping(b)` right after `applyBatch(b)`;
  - `client/src/store/ui.ts`: delete `projectMoves`, `projectOrder`, `setProjectOrder`, `collapsed` and `setCollapsed`, and fix every import (`grep -rn "projectOrder\|setProjectOrder\|setCollapsed\|collapsed\[" client/src`);
  - `client/src/ui/sessions/model.ts`: `groupMembers`, `groupRows`, `listRows`, `projectChoices`, and the header `Row` fields;
  - `client/src/ui/sessions/organize.ts`: drops;
  - `client/src/ui/sessions/SessionList.tsx`: `Header`, `StripBand` and FilterChips;
  - `client/src/dev/replay.ts`: the fixture's `groups` and `grouping` go into the first batch.
- Create: `client/src/store/organization.ts`, `client/src/store/organization.test.ts`, `client/src/ui/sessions/model.test.ts`

**Interfaces:**
- Consumes: the Task 6 commands; `Batch.groups` and `Batch.grouping` (Tasks 3, 5); `rowId`, `catalog` (`store/catalog.ts`).
- Produces:

```ts
// types.ts
export interface GroupOut { computer: ComputerId; key: string; group: string; rank: number }
export interface HeaderOut { id: string; name: string; color: string | null; accessible: boolean; collapsed: boolean; is_default: boolean; palette: number }
export interface GroupingOut { loaded: boolean; default_project: string; headers: HeaderOut[]; ready: boolean; reason: string | null;
                               busy: boolean; status: string; status_error: boolean; conflicts: number }
export type EditOutcome = { outcome: "done"; conflicts: number } | { outcome: "kept"; detail: string }
                        | { outcome: "refused"; detail: string };
// store/organization.ts
export function createOrganization(say: Say): Organization;
export const memberOf: (s: Pick<Session, "machine" | "name">) => string;
export const { grouping, groups, defaultProject, applyGrouping, groupOf, headerOf, refusal, moveToProject, createProject,
               renameProject, deleteProject, moveProject, sortProject, setProjectCollapsed } = app;
export const [projectToShow, setProjectToShow]: Signal<string | undefined>;
// ui/sessions/model.ts
export function groupMembers(order: readonly string[], rows: Record<string, Session>, assigned: Record<string, { group: string; rank: number } | undefined>, fallback: string): Map<string, Session[]>;
export interface ListInput { grouping: GroupingOut; members: Map<string, Session[]>; visible: (s: Session) => boolean; projects: readonly string[];
                             expanded: (key: string) => boolean; compact: (key: string) => boolean }
export function groupRows(input: ListInput): Row[];
export const membersByGroup: Accessor<Map<string, Session[]>>;
```

Rules (`SessionsWindowRows.cpp:289-338, 393-411`):
- **Members:** each row's project is `groups[id].group`, else the default project. Inside a project, rows follow `rank`, and ties keep arrival order. `membersByGroup` reads only row ids and assignments, so a status upsert does not regroup.
- **One project, or none presented yet** (`hasGroups`, 293): no header and nothing collapses. The project filter still applies.
- **Several projects:**
  - headers follow the organization's order; a project left out by the project filter is skipped (304);
  - a project with no visible member is not shown (323; "Hide projects without active sessions" defaults on, 109, and its setting is 3b);
  - a collapsed project shows its header only (326).
- **Header counts** (393-411): `total` counts the visible members. `attention` counts members that are not muted and are attention or review later. `unread` counts members that are not muted, unread and not offline. `working` counts working or background members. These counts include members the filters hide.
- The project filter, its chips and its choices use project IDs, with header names as labels.
- **Drops:**
  - a project dropped before another is `moveProject(id, before)`;
  - a session dropped into a project, before a row or at its end, is `moveToProject([row], project, before)`, inside its own project too, as Qt's `sessionMoved` (`SessionsWindow.cpp:556-561`).
- **Notices** (the action store's `notify`):
  - `kept`: a sticky error with its detail, which is Qt's status line ("Project sync unavailable: {error}", or "Project sync needs an updated HGS installation."). The list keeps showing the change, and the Rust side sends it again on the next tick;
  - `done` with conflicts: a sticky warning "N project conflict(s) need review in Swarm.";
  - `refused`: an info notice with the reason.
  - A rejected call shows "Project sync unavailable: {error}" once and becomes `kept`. The webview never repeats a call; only the Rust controller sends an apply again.

- [ ] **Step 1: Write the failing tests.**

`client/src/store/organization.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import * as bridge from "../bridge";
import { createOrganization, memberOf } from "./organization";
import { emptyBatch, type Batch, type GroupingOut } from "../types";

vi.mock("../bridge", () => ({
  moveToProject: vi.fn(), createProject: vi.fn(), renameProject: vi.fn(), deleteProject: vi.fn(),
  moveProject: vi.fn(), sortProject: vi.fn(), setProjectCollapsed: vi.fn(() => Promise.resolve()),
}));
vi.mock("./actions", () => ({ notify: vi.fn() }));

const mocked = vi.mocked(bridge);
const header = (id: string, name: string) => ({ id, name, color: null, accessible: true, collapsed: false, is_default: id === "ungrouped", palette: 0 });
const ready = (extra: Partial<GroupingOut> = {}): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers: [header("alpha", "Alpha"), header("ungrouped", "Inbox")],
  ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0, ...extra,
});
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const rows = [{ machine: "workstation", name: "claude/sample-project/review" }, { machine: "studio", name: "codex/aurora/orders-v3" }];

function setup(grouping = ready()) {
  const say = vi.fn();
  const org = createOrganization(say);
  org.applyGrouping(batchOf({ grouping }));
  return { org, say };
}

beforeEach(() => { for (const fn of Object.values(mocked)) fn.mockReset?.(); });

describe("the organization store", () => {
  it("assigns rows from batches and falls back to the default project", () => createRoot((dispose) => {
    const { org } = setup();
    org.applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1048576 }] }));
    expect(org.groupOf("\nrun:1")).toBe("alpha");
    expect(org.groupOf("\nrun:2")).toBe("ungrouped");
    org.applyGrouping(batchOf({ removes: [{ computer: "", key: "run:1", reason: null }] }));
    expect(org.groupOf("\nrun:1")).toBe("ungrouped");
    expect(org.headerOf("alpha")?.name).toBe("Alpha");
    dispose();
  }));

  it("moves a set of rows with one call carrying their members", async () => {
    const { org, say } = setup();
    mocked.moveToProject.mockResolvedValue({ outcome: "done", conflicts: 0 });
    const out = await org.moveToProject(rows, "alpha");
    expect(out).toEqual({ outcome: "done", conflicts: 0 });
    expect(mocked.moveToProject).toHaveBeenCalledTimes(1);
    expect(mocked.moveToProject).toHaveBeenCalledWith(["workstation\nclaude/sample-project/review", "studio\ncodex/aurora/orders-v3"], "alpha", null);
    expect(say).not.toHaveBeenCalled();
    expect(memberOf(rows[0])).toBe("workstation\nclaude/sample-project/review");
  });

  it("refuses before the catalog is ready and for a project that is gone, without a call", async () => {
    const { org, say } = setup(ready({ ready: false, reason: "Project sync is starting." }));
    expect(await org.moveToProject(rows, "alpha")).toEqual({ outcome: "refused", detail: "Project sync is starting." });
    org.applyGrouping(batchOf({ grouping: ready() }));
    expect(await org.renameProject("gone", "x")).toEqual({ outcome: "refused", detail: "This project is no longer listed. Open the menu again." });
    expect(mocked.moveToProject).not.toHaveBeenCalled();
    expect(mocked.renameProject).not.toHaveBeenCalled();
    expect(say).toHaveBeenCalledWith("info", "Project sync is starting.");
  });

  it("shows a rejected call once with Qt's failure text and never repeats it", async () => {
    vi.useFakeTimers();
    const { org, say } = setup();
    mocked.sortProject.mockRejectedValue("lost the answer");
    const out = await org.sortProject("alpha");
    expect(out).toEqual({ outcome: "kept", detail: "Project sync unavailable: lost the answer" });
    vi.advanceTimersByTime(60_000);
    expect(mocked.sortProject).toHaveBeenCalledTimes(1);
    expect(say).toHaveBeenCalledTimes(1);
    expect(say).toHaveBeenCalledWith("error", "Project sync unavailable: lost the answer", true);
    vi.useRealTimers();
  });

  it("warns about conflicts on the change and shows a kept edit's status line", async () => {
    const { org, say } = setup();
    mocked.deleteProject.mockResolvedValue({ outcome: "done", conflicts: 2 });
    await org.deleteProject("alpha", "ungrouped");
    expect(say).toHaveBeenCalledWith("warn", "2 project conflict(s) need review in Swarm.", true);
    mocked.moveProject.mockResolvedValue({ outcome: "kept", detail: "Project sync unavailable: hgs did not answer in time" });
    await org.moveProject("alpha", undefined);
    expect(mocked.moveProject).toHaveBeenCalledWith("alpha", null);
    expect(mocked.moveProject).toHaveBeenCalledTimes(1);
    expect(say).toHaveBeenCalledWith("error", "Project sync unavailable: hgs did not answer in time", true);
    mocked.renameProject.mockResolvedValue({ outcome: "kept", detail: "Project sync needs an updated HGS installation." });
    await org.renameProject("alpha", "Alpha two");
    expect(say).toHaveBeenCalledWith("error", "Project sync needs an updated HGS installation.", true);
  });
});
```

`client/src/ui/sessions/model.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createMemo, createRoot } from "solid-js";
import { createStore } from "solid-js/store";
import { toSession } from "../../store/catalog";
import { groupMembers, groupRows, type ListInput } from "./model";
import type { GroupingOut, ViewRow } from "../../types";
import type { Session } from "./types";

const session = (n: number, extra: Partial<ViewRow> = {}): Session =>
  toSession("", `run:${n}`, { name: `claude/sample-project/s${n}`, machine: "workstation", status: "ready", ...extra });
const header = (id: string, collapsed = false) => ({ id, name: id.toUpperCase(), color: null, accessible: true, collapsed, is_default: id === "ungrouped", palette: 0 });
const grouping = (headers: ReturnType<typeof header>[]): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers, ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
});
const input = (g: GroupingOut, members: Map<string, Session[]>, extra: Partial<ListInput> = {}): ListInput => ({
  grouping: g, members, visible: () => true, projects: [], expanded: () => false, compact: () => false, ...extra,
});
const shape = (rows: ReturnType<typeof groupRows>) => rows.map((r) => (r.type === "header" ? `#${r.project}` : r.type === "card" ? r.session.name.slice(-2) : "child"));

describe("grouped rows", () => {
  it("one project shows no header and nothing collapses", () => {
    const members = new Map([["alpha", [session(1), session(2)]]]);
    expect(shape(groupRows(input(grouping([header("alpha", true)]), members)))).toEqual(["s1", "s2"]);
  });

  it("follows the organization's order, hides empty projects and collapses only members", () => {
    const members = new Map([["beta", [session(3)]], ["alpha", [session(1), session(2)]]]);
    const g = grouping([header("alpha"), header("ungrouped"), header("beta", true)]);
    expect(shape(groupRows(input(g, members)))).toEqual(["#alpha", "s1", "s2", "#beta"]);
    expect(shape(groupRows(input(g, members, { projects: ["beta"] })))).toEqual(["#beta"]);
  });

  it("counts every member of a project in its header, never muted ones", () => {
    const members = new Map([["alpha", [session(1, { status: "attention" }), session(2, { unread: true }), session(3, { status: "working" }), session(4, { status: "attention", muted: true })]], ["ungrouped", [session(5)]]]);
    const rows = groupRows(input(grouping([header("alpha"), header("ungrouped")]), members, { visible: (s) => s.name.endsWith("s3") || s.name.endsWith("s5") }));
    const alpha = rows[0];
    expect(alpha.type === "header" && [alpha.total, alpha.attention, alpha.unread, alpha.working]).toEqual([1, 1, 1, 1]);
  });

  it("orders members by rank, keeps arrival order for ties and puts unassigned rows in the default project", () => {
    const all = [session(1), session(2), session(3), session(4)];
    const rows = Object.fromEntries(all.map((s) => [s.key, s]));
    const members = groupMembers(all.map((s) => s.key), rows, {
      "\nrun:1": { group: "alpha", rank: 3 }, "\nrun:2": { group: "alpha", rank: 1 }, "\nrun:3": { group: "alpha", rank: 1 },
    }, "ungrouped");
    expect(members.get("alpha")?.map((s) => s.name.slice(-2))).toEqual(["s2", "s3", "s1"]);
    expect(members.get("ungrouped")?.map((s) => s.name.slice(-2))).toEqual(["s4"]);
  });

  it("does not regroup when only a row's status changes", () => createRoot((dispose) => {
    const [rows, setRows] = createStore<Record<string, Session>>({ "\nrun:1": session(1) });
    const [order] = createStore(["\nrun:1"]);
    let runs = 0;
    const members = createMemo(() => { runs++; return groupMembers(order, rows, {}, "ungrouped"); });
    members();
    setRows("\nrun:1", "status", "working");
    members();
    expect(runs).toBe(1);
    dispose();
  }));
});
```

- [ ] **Step 2: Run** `npx vitest run src/store/organization.test.ts src/ui/sessions/model.test.ts` in `client`. Expected: FAIL (modules and exports missing).

- [ ] **Step 3: Implement.**

`client/src/store/organization.ts`:

```ts
import { createMemo, createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import * as bridge from "../bridge";
import { rowId } from "./catalog";
import { notify } from "./actions";
import type { Batch, EditOutcome, GroupingOut, HeaderOut } from "../types";
import type { Session } from "../ui/sessions/types";

// The shared project organization in the Sessions list: each row's project
// and rank and the project headers, as the Rust side sends them (it keeps the
// organization itself), and the organisation edits. Every edit is one call,
// which the Rust side sends as one `hgs swarm apply`; its answer is shown
// once. A failed apply stays pending on the Rust side, which sends it again
// on the next tick as Qt's SwarmController does; the webview never repeats a
// call, also not a rejected one.

export const STARTING = "Project sync is starting.";
export const GONE = "This project is no longer listed. Open the menu again.";
export const conflictsText = (n: number) => `${n} project conflict(s) need review in Swarm.`;
export const unavailable = (detail: string) => `Project sync unavailable: ${detail}`;

export const emptyGrouping = (): GroupingOut => ({
  loaded: false, default_project: "ungrouped", headers: [], ready: false, reason: STARTING,
  busy: false, status: "", status_error: false, conflicts: 0,
});

/** A session's member key, as the catalog names it (`swarm::member`). */
export const memberOf = (s: Pick<Session, "machine" | "name">) => `${s.machine}\n${s.name}`;

type Say = (tone: "error" | "warn" | "info", text: string, sticky?: boolean) => void;
type Row = Pick<Session, "machine" | "name">;

export function createOrganization(say: Say) {
  const [grouping, setGrouping] = createSignal<GroupingOut>(emptyGrouping());
  const [groups, setGroups] = createStore<Record<string, { group: string; rank: number } | undefined>>({});
  const defaultProject = createMemo(() => grouping().default_project);

  function applyGrouping(b: Batch) {
    if (b.grouping) setGrouping(b.grouping);
    const assigned = b.groups ?? [];
    if (!assigned.length && !b.removes.length) return;
    setGroups(produce((s) => {
      for (const k of b.removes) delete s[rowId(k.computer, k.key)];
      for (const g of assigned) s[rowId(g.computer, g.key)] = { group: g.group, rank: g.rank };
    }));
  }

  const groupOf = (id: string) => groups[id]?.group ?? defaultProject();
  const headerOf = (project: string): HeaderOut | undefined => grouping().headers.find((h) => h.id === project);

  /** Why an edit cannot run now (the disabled entries' reason). */
  function refusal(...projects: (string | undefined)[]): string | undefined {
    const g = grouping();
    if (!g.ready) return g.reason ?? STARTING;
    if (projects.some((p) => p !== undefined && !headerOf(p))) return GONE;
    return undefined;
  }

  function report(out: EditOutcome) {
    switch (out.outcome) {
      case "done": if (out.conflicts > 0) say("warn", conflictsText(out.conflicts), true); return;
      // Qt's status line for the failure (SwarmController.cpp:69, 195); the Rust side resends.
      case "kept": say("error", out.detail, true); return;
      case "refused": say("info", out.detail); return;
    }
  }

  async function run(why: string | undefined, call: () => Promise<EditOutcome>): Promise<EditOutcome> {
    let out: EditOutcome;
    if (why) out = { outcome: "refused", detail: why };
    else {
      try { out = await call(); } catch (error) {
        const detail = typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
        out = { outcome: "kept", detail: unavailable(detail) };
      }
    }
    report(out);
    return out;
  }

  /** Move to project: one call for every row, as one `swarm apply` (zerus-5wmh.18, .20). */
  function moveToProject(rows: readonly Row[], projectId: string, before?: Row): Promise<EditOutcome> {
    if (!rows.length) return Promise.resolve({ outcome: "refused", detail: "Choose the sessions to move." });
    return run(refusal(projectId), () => bridge.moveToProject(rows.map(memberOf), projectId, before ? memberOf(before) : null));
  }
  const createProject = (name: string, rows: readonly Row[] = []) => run(refusal(), () => bridge.createProject(name, rows.map(memberOf)));
  const renameProject = (projectId: string, name: string) => run(refusal(projectId), () => bridge.renameProject(projectId, name));
  const deleteProject = (projectId: string, destination: string) =>
    run(refusal(projectId, destination), () => bridge.deleteProject(projectId, destination));
  const moveProject = (projectId: string, before: string | undefined) => run(refusal(projectId), () => bridge.moveProject(projectId, before ?? null));
  const sortProject = (projectId: string) => run(refusal(projectId), () => bridge.sortProject(projectId));
  /** Expand or collapse: this device's layout; the next batch shows it. */
  const setProjectCollapsed = (projectId: string, collapsed: boolean) => { void bridge.setProjectCollapsed(projectId, collapsed).catch(() => {}); };

  return { grouping, groups, defaultProject, applyGrouping, groupOf, headerOf, refusal, moveToProject, createProject,
           renameProject, deleteProject, moveProject, sortProject, setProjectCollapsed };
}

const app = createOrganization(notify);
export const { grouping, groups, defaultProject, applyGrouping, groupOf, headerOf, refusal, moveToProject, createProject,
               renameProject, deleteProject, moveProject, sortProject, setProjectCollapsed } = app;

/** Project settings… asks the Projects screen to show this project (Task 9). */
export const [projectToShow, setProjectToShow] = createSignal<string | undefined>();
```

`client/src/ui/sessions/model.ts`:
- The header row becomes `{ type: "header"; id: string; project: string; name: string; color: string | null; palette: number; collapsed: boolean; total: number; attention: number; unread: number; working: number }`. `project` is the project ID.
- Add, replacing the name-derived grouping inside `listRows`:

```ts
/**
 * Each project's rows in list order: by rank, ties in arrival order (the
 * sort is stable); a row without an assignment is in `fallback`. It reads
 * only row ids and assignments, so a status upsert does not run it again.
 */
export function groupMembers(order: readonly string[], rows: Record<string, Session>,
                             assigned: Record<string, { group: string; rank: number } | undefined>, fallback: string): Map<string, Session[]> {
  const map = new Map<string, Session[]>();
  const rank = new Map<string, number>();
  for (const id of order) {
    const s = rows[id];
    if (!s) continue;
    const a = assigned[id];
    rank.set(id, a?.rank ?? Number.MAX_SAFE_INTEGER);
    const group = a?.group ?? fallback;
    const list = map.get(group);
    if (list) list.push(s);
    else map.set(group, [s]);
  }
  for (const list of map.values()) list.sort((x, y) => rank.get(x.key)! - rank.get(y.key)!);
  return map;
}

export interface ListInput {
  grouping: GroupingOut;
  /** Each project's rows in list order (groupMembers). */
  members: Map<string, Session[]>;
  visible: (s: Session) => boolean;
  /** The project filter's project IDs; empty is every project. */
  projects: readonly string[];
  expanded: (key: string) => boolean;
  compact: (key: string) => boolean;
}

/** Qt's grouped rows (SessionsWindowRows.cpp:289-338, header counts 393-411). */
export function groupRows(input: ListInput): Row[] {
  const out: Row[] = [];
  const cards = (list: readonly Session[]) => {
    for (const s of list) {
      const kids = s.subagents ?? [];
      const expanded = kids.length > 0 && input.expanded(s.key);
      out.push({ type: "card", id: s.key, session: s, expandable: kids.length > 0, expanded, compactRow: input.compact(s.key) });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: s, child, first: i === 0, last: i === kids.length - 1 }));
    }
  };
  const wanted = (id: string) => !input.projects.length || input.projects.includes(id);
  const headers = input.grouping.loaded ? input.grouping.headers : [];
  if (headers.length <= 1) {
    // One project, or none presented yet: no header, and nothing collapses (293, 324, 326).
    for (const [id, list] of input.members) if (wanted(id)) cards(list.filter(input.visible));
    return out;
  }
  for (const h of headers) {
    if (!wanted(h.id)) continue;
    const all = input.members.get(h.id) ?? [];
    const list = all.filter(input.visible);
    // A project without visible members stays hidden (323; "Hide projects without active sessions" defaults on, 109).
    if (!list.length) continue;
    let attention = 0, unread = 0, working = 0;
    for (const s of all) {
      if (!s.muted && (s.reviewLater || s.status === "attention")) attention++;
      if (!s.muted && s.unread && s.status !== "offline") unread++;
      if (s.status === "working" || s.status === "background") working++;
    }
    out.push({ type: "header", id: `h:${h.id}`, project: h.id, name: h.name, color: h.color, palette: h.palette,
               collapsed: h.collapsed, total: list.length, attention, unread, working });
    if (!h.collapsed) cards(list);
  }
  return out;
}
```

- `membersByGroup = createMemo(() => groupMembers(catalog.order, catalog.rows, groups, defaultProject()))`.
- `listRows = createMemo(() => groupRows({ grouping: grouping(), members: membersByGroup(), visible, projects: projectFilter(), expanded: (k) => !!expandedChildren[k], compact: (k) => !!compactRows[k] }))`. Here `visible` is the current filter, machine and search test.
- `projectChoices` lists `grouping().headers` that have members, with their names and member counts.

`client/src/ui/sessions/organize.ts`:
- Import `grouping`, `groupOf`, `moveProject` and `moveToProject` from `../../store/organization`.
- Drop `moveRow`, `projectOrder`, `setProjectOrder` and `toast` from the imports.
- `defaultProject = () => grouping().default_project`. In `placement`, a card's group is `groupOf(over.session.key)` (a child's, `groupOf(over.parent.key)`), and the next card's group test is `groupOf(next.session.key) === group`.
- `applyDrop` becomes:

```ts
/** Applies a drop: a project moves before another (SessionList::groupMoved, moveGroup), a session into a project before a row or at its end (sessionMoved, moveSession; SessionsWindow.cpp:556-561). */
export function applyDrop(item: DragItem, d: Drop) {
  if (d.before === item.id) return;
  if (item.kind === "group") { void moveProject(item.id, d.before); return; }
  const moving = catalog.rows[item.id];
  if (!moving) return;
  void moveToProject([moving], d.group, d.before ? catalog.rows[d.before] : undefined);
}
```

`client/src/ui/sessions/SessionList.tsx`:
- `Header` shows `props.row.name`, colours with `projectColor(props.row)` (Task 8's `organization-ui.ts`; until then `props.row.color ?? \`var(--z-entity-palette-${props.row.palette})\``), reads `props.row.collapsed` and toggles with `setProjectCollapsed(props.row.project, !props.row.collapsed)`. The `StripBand` changes the same way.
- FilterChips labels a project chip with `headerOf(id)?.name ?? id`.
- Remove the imports of the deleted `ui.ts` stores. The menus are Task 8.

`client/src/dev/replay.ts`: `Fixture` gains `groups?: GroupOut[]; grouping?: GroupingOut`. When the fixture loads, set `pending.groups = fixture.groups ?? []` and `pending.grouping = fixture.grouping` before the first `push`.

- [ ] **Step 4: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py` in `client`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src
git commit -m "feat(client): the Sessions list groups by the shared organization with Qt's rules"
```

---

### Task 8: The project menus and dialogs (Move to project, New, Rename, Settings, order, Sort, Delete)

**Files:**
- Create: `client/src/ui/sessions/organization-ui.ts`, `client/src/ui/sessions/organization-ui.test.ts`, `client/src/ui/sessions/OrganizationDialogs.tsx`
- Modify: `client/src/ui/sessions/SessionList.tsx` (`headerMenu`, the `sessionMenu` line), `client/src/App.tsx` (mount)

**Interfaces:**
- Consumes: the Task 7 store; `setPage`, `setProjectFilter` and `MenuEntry` (`store/ui.ts`).
- Produces:

```ts
// organization-ui.ts
export interface Entry { enabled: boolean; reason?: string }
export interface HeaderMenuState { collapse: "Expand project" | "Collapse project"; edit: Entry; up: Entry & { before?: string };
                                   down: Entry & { before?: string }; remove: Entry }
export function headerMenuState(g: GroupingOut, project: string): HeaderMenuState | undefined;
export function deleteTargets(g: GroupingOut, project: string): { options: HeaderOut[]; selected: string };
export function moveChoices(g: GroupingOut, current: readonly string[]): (HeaderOut & { current: boolean })[];
export const projectColor: (h: Pick<HeaderOut, "color" | "palette">) => string;
export const nameProblem: (name: string) => string | undefined;
export const NAME_CHARS = 80;
// OrganizationDialogs.tsx
export function openMoveDialog(rows: readonly Session[]): void;
export function openNewProjectDialog(rows?: readonly Session[]): void;
export function openRenameProjectDialog(project: string): void;
export function openDeleteProjectDialog(project: string): void;
export function moveEntry(s: Session): MenuEntry;
export function OrganizationDialogs(): JSX.Element;
```

Rules:
- **The header menu** (`SessionsWindow.cpp:2568-2616`), in Qt's order:
  - "New session…" stays disabled (phase 5), then a separator;
  - "Filter this project" (2575-2576), then a separator;
  - "Expand project" or "Collapse project" (2578-2581) is always enabled and local;
  - "Rename project…" (2582-2587) and "Project settings…" (2588-2589: `setProjectToShow(id)`, then `setPage("projects")`), then a separator;
  - "Move project up" (enabled when not first; before the previous project) and "Move project down" (enabled when not last; before the project after the next one, else last) (2591-2600);
  - "Sort sessions by name" (2601-2611), then a separator;
  - "Delete project…" (2613-2614; enabled with more than one project).

  Every entry that writes is disabled while `grouping().ready` is false, with `grouping().reason` as its tooltip, as Qt's `hint`.
- **Move to project** (5363-5372): the mockup's dialog (`design/webui/src/dialogs/SessionDialogs.tsx`, `MoveProject`), titled "Move to project". It lists every project in order, with the current one checked; with several rows, a project is checked only when every row is in it. A separator follows, then "New project…". Choosing a project closes the dialog and calls `moveToProject(rows, id)` once.
- **New project** (2558-2566): title "New project", the label "Project name", at most 80 characters, OK disabled while the name is blank. It calls `createProject(name, rows)`, so from Move to project the session goes into the new project in the same apply.
- **Rename project** (2582-2587): title "Rename project", the label "Project name", the current name selected. OK is disabled while the name is blank, and it calls `renameProject`.
- **Delete project** (`ProjectsDialog.cpp:367-381`):
  - title "Delete project", and the text `Delete “{name}”? Choose the project that will receive its sessions. Files and running agents stay in place.`;
  - a list of every other project, with the default project chosen, else the first;
  - "Move sessions and delete" (danger) calls `deleteProject(id, target)`;
  - Cancel is focused and the default button; Escape cancels.
- The disabled reasons and texts have no dot separators and no keyboard hints.

- [ ] **Step 1: Write the failing tests** (`client/src/ui/sessions/organization-ui.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { deleteTargets, headerMenuState, moveChoices, nameProblem, projectColor } from "./organization-ui";
import type { GroupingOut } from "../../types";

const header = (id: string, extra = {}) => ({ id, name: id, color: null, accessible: true, collapsed: false, is_default: false, palette: 2, ...extra });
const g = (extra: Partial<GroupingOut> = {}): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers: [header("alpha"), header("ungrouped", { is_default: true }), header("beta", { collapsed: true })],
  ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0, ...extra,
});

describe("project menu rules", () => {
  it("moves up before the previous project and down before the one after the next", () => {
    const first = headerMenuState(g(), "alpha")!;
    expect([first.up.enabled, first.down.enabled, first.down.before]).toEqual([false, true, "beta"]);
    const middle = headerMenuState(g(), "ungrouped")!;
    expect([middle.up.before, middle.down.enabled, middle.down.before]).toEqual(["alpha", true, undefined]);
    const last = headerMenuState(g(), "beta")!;
    expect([last.down.enabled, last.collapse]).toEqual([false, "Expand project"]);
    expect(headerMenuState(g(), "gone")).toBeUndefined();
  });

  it("disables every write with the reason while the catalog is not ready, and Delete with one project", () => {
    const waiting = headerMenuState(g({ ready: false, reason: "Project sync is starting." }), "alpha")!;
    expect([waiting.edit, waiting.remove, waiting.down.enabled]).toEqual([
      { enabled: false, reason: "Project sync is starting." }, { enabled: false, reason: "Project sync is starting." }, false]);
    expect(waiting.collapse).toBe("Collapse project");
    const single = headerMenuState(g({ headers: [header("alpha")] }), "alpha")!;
    expect(single.remove.enabled).toBe(false);
  });

  it("offers every other project for the sessions of a deleted one, the default first", () => {
    expect(deleteTargets(g(), "alpha")).toMatchObject({ selected: "ungrouped" });
    expect(deleteTargets(g(), "ungrouped").options.map((h) => h.id)).toEqual(["alpha", "beta"]);
    expect(deleteTargets(g(), "ungrouped").selected).toBe("alpha");
  });

  it("checks the project every chosen row is in", () => {
    expect(moveChoices(g(), ["beta"]).filter((c) => c.current).map((c) => c.id)).toEqual(["beta"]);
    expect(moveChoices(g(), ["beta", "alpha"]).some((c) => c.current)).toBe(false);
  });

  it("colours by the project, else its palette slot, and wants a name", () => {
    expect(projectColor({ color: "#3fb950", palette: 1 })).toBe("#3fb950");
    expect(projectColor({ color: null, palette: 4 })).toBe("var(--z-entity-palette-4)");
    expect(nameProblem("  ")).toBe("Enter a project name.");
    expect(nameProblem(" Release ")).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run** `npx vitest run src/ui/sessions/organization-ui.test.ts`. Expected: FAIL.

- [ ] **Step 3: Implement.**

`client/src/ui/sessions/organization-ui.ts`:

```ts
import type { GroupingOut, HeaderOut } from "../../types";

// Pure rules of the project menus and dialogs (SessionsWindow.cpp:2568-2616,
// 5363-5372; ProjectsDialog.cpp:367-381).

export interface Entry { enabled: boolean; reason?: string }
export interface HeaderMenuState {
  collapse: "Expand project" | "Collapse project";
  edit: Entry;
  up: Entry & { before?: string };
  down: Entry & { before?: string };
  remove: Entry;
}

export function headerMenuState(g: GroupingOut, project: string): HeaderMenuState | undefined {
  const at = g.headers.findIndex((h) => h.id === project);
  if (at < 0) return undefined;
  const reason = g.ready ? undefined : g.reason ?? undefined;
  const several = g.headers.length > 1;
  return {
    collapse: g.headers[at].collapsed ? "Expand project" : "Collapse project",
    edit: { enabled: g.ready, reason },
    up: { enabled: g.ready && at > 0, before: g.headers[at - 1]?.id, reason },
    down: { enabled: g.ready && at + 1 < g.headers.length, before: g.headers[at + 2]?.id, reason },
    remove: { enabled: g.ready && several, reason: several ? reason : undefined },
  };
}

/** Every other project; the default one is chosen, else the first. */
export function deleteTargets(g: GroupingOut, project: string): { options: HeaderOut[]; selected: string } {
  const options = g.headers.filter((h) => h.id !== project);
  return { options, selected: options.find((h) => h.id === g.default_project)?.id ?? options[0]?.id ?? "" };
}

/** Every project, checked when every chosen row is in it. */
export function moveChoices(g: GroupingOut, current: readonly string[]): (HeaderOut & { current: boolean })[] {
  return g.headers.map((h) => ({ ...h, current: current.length > 0 && current.every((p) => p === h.id) }));
}

export const projectColor = (h: Pick<HeaderOut, "color" | "palette">) => h.color ?? `var(--z-entity-palette-${h.palette})`;
export const NAME_CHARS = 80;
/** createGroup and renameGroup ignore a blank name (SessionOrganization.cpp:135-136, 149). */
export const nameProblem = (name: string) => (name.trim() ? undefined : "Enter a project name.");
```

`client/src/ui/sessions/OrganizationDialogs.tsx` (the mockup's dialog classes and skin variables, one modal host as in `ActionDialogs.tsx`):

```tsx
import { For, Show, createEffect, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { createProject, deleteProject, groupOf, grouping, headerOf, moveToProject, renameProject } from "../../store/organization";
import type { MenuEntry } from "../../store/ui";
import type { Session } from "./types";
import { NAME_CHARS, deleteTargets, moveChoices, nameProblem, projectColor } from "./organization-ui";

// The organisation dialogs: Move to project (also for a selection), New
// project, Rename project and Delete project.

type Open = { kind: "move"; rows: Session[] } | { kind: "new"; rows: Session[] } | { kind: "rename"; project: string } | { kind: "delete"; project: string };
const [open, setOpen] = createSignal<Open | undefined>(undefined, { equals: false });
const close = () => setOpen(undefined);

export const openMoveDialog = (rows: readonly Session[]) => setOpen({ kind: "move", rows: [...rows] });
export const openNewProjectDialog = (rows: readonly Session[] = []) => setOpen({ kind: "new", rows: [...rows] });
export const openRenameProjectDialog = (project: string) => setOpen({ kind: "rename", project });
export const openDeleteProjectDialog = (project: string) => setOpen({ kind: "delete", project });

/** The session menu's Move to project entry. */
export function moveEntry(s: Session): MenuEntry {
  const g = grouping();
  return { label: "Move to project…", icon: "folder", disabled: !g.ready, hint: g.ready ? undefined : g.reason ?? undefined, action: () => openMoveDialog([s]) };
}

function MoveDialog(props: { rows: Session[] }) {
  const choices = () => moveChoices(grouping(), props.rows.map((s) => groupOf(s.key)));
  const pick = (project: string) => { close(); void moveToProject(props.rows, project); };
  return (
    <>
      <div class="dlg-head"><Icon name="folder" size={18} class="accent" /><h2 id="org-dlg-title">Move to project</h2></div>
      <div class="dlg-body col" style={{ gap: "4px" }}>
        <For each={choices()}>{(p) => (
          <button type="button" class="menu-item entity" data-initial={p.current ? "" : undefined} style={{ "--entity": projectColor(p) }}
            aria-pressed={p.current} onClick={() => pick(p.id)}>
            <Icon name="folder" size={14} /><span class="ell">{p.name}</span>
            <Show when={p.current}><span class="grow" /><Icon name="check" size={14} /></Show>
          </button>
        )}</For>
        <div class="menu-sep" />
        <button type="button" class="menu-item" onClick={() => openNewProjectDialog(props.rows)}><Icon name="plus" size={14} />New project…</button>
      </div>
      <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn" onClick={close}>Cancel</button></div>
    </>
  );
}

function NameDialog(props: { title: string; initial: string; submit: (name: string) => void }) {
  const [value, setValue] = createSignal(props.initial);
  const done = (e: Event) => { e.preventDefault(); if (nameProblem(value())) return; close(); props.submit(value()); };
  return (
    <form onSubmit={done}>
      <div class="dlg-head"><Icon name="folder" size={18} class="accent" /><h2 id="org-dlg-title">{props.title}</h2></div>
      <div class="dlg-body">
        <label class="dlg-label" for="org-name">Project name</label>
        <input id="org-name" data-initial class="dlg-input" maxLength={NAME_CHARS} value={value()} onInput={(e) => setValue(e.currentTarget.value)} />
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={close}>Cancel</button>
        <button type="submit" class="btn primary" disabled={!!nameProblem(value())}>OK</button>
      </div>
    </form>
  );
}

function DeleteDialog(props: { project: string }) {
  const name = headerOf(props.project)?.name ?? props.project;
  const targets = deleteTargets(grouping(), props.project);
  const [target, setTarget] = createSignal(targets.selected);
  return (
    <>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="org-dlg-title">Delete project</h2></div>
      <div class="dlg-body">
        <p class="confirm-body" id="org-dlg-body">Delete “{name}”? Choose the project that will receive its sessions. Files and running agents stay in place.</p>
        <select class="field" aria-label="Project that receives the sessions" value={target()} onChange={(e) => setTarget(e.currentTarget.value)}>
          <For each={targets.options}>{(p) => <option value={p.id}>{p.name}</option>}</For>
        </select>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial autofocus onClick={close}>Cancel</button>
        <button type="button" class="btn solid-danger" disabled={!target()} onClick={() => { close(); void deleteProject(props.project, target()); }}>Move sessions and delete</button>
      </div>
    </>
  );
}

export function OrganizationDialogs() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    const state = open();
    if (!el) return;
    if (!state) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => {
      const initial = el?.querySelector<HTMLElement>("[data-initial]") ?? el?.querySelector<HTMLElement>(".menu-item");
      initial?.focus();
      if (initial instanceof HTMLInputElement) initial.select();
    });
  });
  return (
    <dialog class="dlg" ref={el} role={open()?.kind === "delete" ? "alertdialog" : "dialog"} aria-labelledby="org-dlg-title"
      aria-describedby={open()?.kind === "delete" ? "org-dlg-body" : undefined} style={{ width: open()?.kind === "move" ? "380px" : "460px" }} onClose={close}>
      <Show when={open()} keyed>
        {(p) => p.kind === "move" ? <MoveDialog rows={p.rows} />
          : p.kind === "new" ? <NameDialog title="New project" initial="" submit={(name) => void createProject(name, p.rows)} />
          : p.kind === "rename" ? <NameDialog title="Rename project" initial={headerOf(p.project)?.name ?? ""} submit={(name) => void renameProject(p.project, name)} />
          : <DeleteDialog project={p.project} />}
      </Show>
    </dialog>
  );
}
```

`client/src/ui/sessions/SessionList.tsx`:
- In `sessionMenu`, replace `{ label: "Move to project…", icon: "folder", disabled: LATER },` with `moveEntry(s),`.
- Replace `headerMenu`:

```tsx
function headerMenu(project: string): MenuEntry[] {
  const m = headerMenuState(grouping(), project);
  if (!m) return [];
  const hint = (e: { enabled: boolean; reason?: string }) => (e.enabled ? undefined : e.reason);
  return [
    { label: "New session…", icon: "plus", disabled: LATER },
    { separator: true },
    { label: "Filter this project", icon: "funnel", action: () => setProjectFilter([project]) },
    { separator: true },
    { label: m.collapse, icon: "list-tree", action: () => setProjectCollapsed(project, m.collapse === "Collapse project") },
    { label: "Rename project…", icon: "pencil", disabled: !m.edit.enabled, hint: hint(m.edit), action: () => openRenameProjectDialog(project) },
    { label: "Project settings…", icon: "settings", action: () => { setProjectToShow(project); setPage("projects"); } },
    { separator: true },
    { label: "Move project up", icon: "arrow-up", disabled: !m.up.enabled, hint: hint(m.up), action: () => void moveProject(project, m.up.before) },
    { label: "Move project down", icon: "arrow-down", disabled: !m.down.enabled, hint: hint(m.down), action: () => void moveProject(project, m.down.before) },
    { label: "Sort sessions by name", icon: "list-filter", disabled: !m.edit.enabled, hint: hint(m.edit), action: () => void sortProject(project) },
    { separator: true },
    { label: "Delete project…", icon: "trash", danger: true, disabled: !m.remove.enabled, hint: hint(m.remove), action: () => openDeleteProjectDialog(project) },
  ];
}
```

- `Header` and `StripBand` colour with `projectColor(props.row)`.
- `client/src/App.tsx`: `import { OrganizationDialogs } from "./ui/sessions/OrganizationDialogs";` and `<OrganizationDialogs />` after `<ActionDialogs />`.

- [ ] **Step 4: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src
git commit -m "feat(client): Qt's project menus: Move to project, New, Rename, Settings, order, Sort and Delete"
```

---

### Task 9: The Projects screen shows the same organization (3a changes)

Runs after 3a's hub (Task 3) and Projects screen (Task 8) are on the branch. Otherwise hand it to the 3a session with this text.

**Files:**
- Modify (3a extension 2): `client/core/src/views/projects.rs`:
  - `SwarmState` gains `default: Option<String>` and `pub fn set_default(&mut self, default: Option<String>)`. `view()` sets `is_default` from `crate::swarm::default_project(organization, self.default.as_deref())` instead of the catalog's `default_project` alone;
  - `SwarmState` gains `pub fn set_status(&mut self, status: String, error: bool)`, which replaces the status line `finish` derived. It carries the controller's line, which also holds Qt's failure and schema texts (`SwarmController.cpp:69, 195`) that no snapshot carries.
- Modify (3a change): `client/src-tauri/src/screens.rs`:
  - drop `Run::Swarm`, its `read` arm and both `hub.swarm.poll.due(...)` uses (in `due` and `set_visible`);
  - add `pub fn organization(&self, value: Arc<Value>, default_project: String, status: (String, bool))`. It calls `set_default`, then `finish(Ok((*value).clone()))`, then `set_status(status.0, status.1)` when the status is not empty, then `publish`.
- Modify: `client/src-tauri/src/lib.rs`: `organization.set_presented(Arc::new(move |value, default, status| screens.organization(value, default, status)))`.
- Modify (3a change): `client/src/ui/screens/Projects.tsx` shows the project that Project settings asks for:

```tsx
createEffect(() => {
  const id = projectToShow();
  if (!id) return;
  const p = view().projects.find((x) => x.id === id);
  if (!p) return;
  if (p.accessible) choose(p);
  setProjectToShow(undefined);
});
```

Rules:
- Projects reads no `swarm get` of its own. It shows the hub's presented organization, the one the Sessions list groups by, with the same default project.
- Its status line is the controller's, as Qt's `SwarmController::statusChanged` feeds `ProjectsDialog::setSwarmStatus` (`SessionsWindow.cpp:1653`). After a failed apply it says "Project sync unavailable: <error>" until an apply lands.
- 3a's Review Focus 5 ("Projects hidden: no `swarm get` runs") becomes "never".
- 3a's test `projects_read_only_while_shown_every_five_seconds_and_never_twice_at_once` is replaced by:

```rust
#[test]
fn projects_show_the_organization_they_are_handed_and_never_read_it() {
    let h = harness(json!({"swarm get": {"file": "swarm.json"}}), true);
    h.screens.set_visible(Screen::Projects, true);
    for second in 1..=12 {
        h.at(second * 1000);
        h.screens.tick();
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.count("swarm get"), 0, "the organization topic feeds Projects");
    assert!(!h.screens.projects().loaded);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/views/swarm.json");
    let swarm: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let failed = "Project sync unavailable: hgs did not answer in time";
    h.screens.organization(Arc::new(swarm), "sample".into(), (failed.into(), true));
    let view = h.screens.projects();
    assert!(view.loaded);
    assert!(view.projects.iter().any(|p| p.id == "sample" && p.is_default));
    assert_eq!((view.status.as_str(), view.status_error), (failed, true), "the controller's status line");
    eventually("a change event", || h.notified.lock().unwrap().contains(&Screen::Projects));
}
```

- And in `projects.rs` tests:

```rust
#[test]
fn this_devices_default_project_is_marked_when_listed() {
    let mut swarm = SwarmState::default();
    swarm.finish(Ok(fixture("swarm.json")));
    swarm.set_default(Some("sample".into()));
    let view = swarm.view(&peers());
    assert!(view.projects.iter().any(|p| p.id == "sample" && p.is_default));
    swarm.set_default(Some("gone".into()));
    assert!(swarm.view(&peers()).projects.iter().any(|p| p.id == "ungrouped" && p.is_default));
}

#[test]
fn the_controllers_status_line_replaces_the_snapshots() {
    let mut swarm = SwarmState::default();
    swarm.finish(Ok(fixture("swarm.json")));
    swarm.set_status("Project sync needs an updated HGS installation.".into(), true);
    let view = swarm.view(&peers());
    assert_eq!((view.status.as_str(), view.status_error), ("Project sync needs an updated HGS installation.", true));
    assert!(view.loaded, "the organization stays");
}
```

- [ ] **Step 1: Write the failing tests above.**
- [ ] **Step 2: Run** the core tests and `cargo +1.99.1 test --locked` in `client/src-tauri`. Expected: FAIL.
- [ ] **Step 3: Implement** as listed.
- [ ] **Step 4: Run** all core, Tauri and frontend checks. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/core/src/views/projects.rs client/src-tauri/src client/src/ui/screens/Projects.tsx
git commit -m "feat(client): Projects shows the organization the Sessions list groups by"
```

---

### Task 10: The replay organization, the smoke, the measurement, documentation and the follow-ups

**Files:**
- Modify: `client/scripts/make-fixture.py`, regenerating `client/fixtures/replay.ndjson` and `client/fixtures/replay-batches.json`, and adding `client/fixtures/replay-organization.json`
- Modify: `client/src/ui/PerfHost.tsx` (`organization-smoke=1`)
- Create: `client/src/store/organization.perf.test.ts`
- Modify: `client/README.md` ("Projects in the Sessions list"), `docs/superpowers/plans/2026-10-28-tauri-client-phase2-actions.md` (the phase table row for `zerus-5wmh.18` names this plan)
- Modify: `.beads/issues.jsonl` (export after the follow-ups)

- [ ] **Step 1: The replay organization.** In `make-fixture.py`:
  - `organization(computers)` builds the `swarm get` object of the fleet:
    - one project `p-<name>` per name-derived project, in first-seen order, with colours from a fixed palette;
    - every row of that project is a member, `"workstation\n<name>"` locally and `"mac\n<name>"` for the peer;
    - the rows of `infra` stay unassigned, so they show in the Inbox;
    - the Inbox `ungrouped`/"Inbox" comes last;
    - `project_order` lists all projects; `versions` has one `project:<id>:name` token per project (`json.dumps` of `{"kind":"project","id":...,"field":"name"}`); `initialized` is true and the `digest` is `replay-1`.
  - `organization_rows(full)` writes the topic's rows (`src/watch/organization.rs`):
    - the summary without `versions`, its `organization.projects` the IDs;
    - `project:<id>` without `sessions`, with `session_chunks`;
    - `members:<id>:<n>` in chunks of 1000;
    - `version:<token>`.
  - The local stream's hello lists the `organization` topic, and one `snapshot` frame of these rows follows `catalog.meta`.
  - `replay-organization.json` is the full object (the smoke's fake replies). `replay-batches.json` gains `grouping`, a loaded and not-ready view with the reason "Project changes need the desktop app.", and `groups`, with ranks `(index in its project + 1) << 20`.
  - Run `python3 client/scripts/make-fixture.py`, then check that `git diff --stat client/fixtures` shows only the additions.

- [ ] **Step 2: The smoke hook.** `?organization-smoke=1` uses the actions smoke's environment and guard. It waits until `grouping().ready` and the replay session `claude/sample-project/review` is listed. Then it runs one edit each, and each step's project exists in the fixture whatever the fake answers:
  - `moveToProject([row], "p-aurora")`;
  - `createProject("Smoke project", [row])`;
  - `renameProject("p-docs-site", "Docs renamed")`;
  - `sortProject("p-sample-project")`;
  - `moveProject("p-orders-api", "p-sample-project")`;
  - `deleteProject("p-docs-site", "ungrouped")`.

  It logs `organization-smoke <step>: <outcome>` and exits 0 only when every outcome is `done`.
  - Run it headless, with the Global Constraints' environment, `ZERUS_CLIENT_REPLAY` on the fixtures, `ZERUS_CLIENT_PERF=smoke` and `ZERUS_CLIENT_QUERY=organization-smoke=1`. `ZERUS_CLIENT_HGS` points at a scratch copy of `fake_hgs_swarm.py` whose `config.json` answers `get` and `apply` with `replay-organization.json`.
  - Afterwards `log.ndjson` holds exactly six `swarm apply` calls and no `swarm get`. Each call's `base` is the fixture's organization in `shared` form and its `versions` are the fixture's. Record the command and the six argv and stdin heads in the PR description.

- [ ] **Step 3: The measurement** (`client/src/store/organization.perf.test.ts`, skipped unless `ZERUS_CLIENT_MEASURE=1`):
  - Build 20,000 `RowOut`s with phase 2's `ViewRow` fields (name, title, agent, status, caption, machine, project, folder, branch, model, run and conversation IDs, state, activity, `allowed_actions`).
  - Measure, after `globalThis.gc?.()`:
    1. `createCatalog().applyBatch` of one replace with them: milliseconds and heap growth. This is phase 2's reconcile number.
    2. Then `createOrganization(() => {}).applyGrouping` with 20,000 `GroupOut`s in 50 projects, `groupMembers` and `groupRows`: milliseconds and heap growth.
    3. Then a batch of 1,000 status upserts: its reconcile time, and that `groupMembers` did not run again.
  - Run `ZERUS_CLIENT_MEASURE=1 NODE_OPTIONS=--expose-gc npx vitest run src/store/organization.perf.test.ts` on main before this plan and on the branch, on the same machine. Put the four numbers in the PR. The reconcile is expected within 10% of main's (phase 2: 277 ms, +49 MB). A larger regression is a finding to fix before merging.

- [ ] **Step 4: Documentation.** In `client/README.md`, add "Projects in the Sessions list":
  - the data path: the topic, the controller, the pump's per-row deltas, no organization in the webview;
  - Qt's grouping rules, the local layout file, the pending draft file and their replay twins;
  - the edit commands, with the `move_to_project` contract for selections;
  - the apply rules, the resend of a failed or timed-out apply on the next tick (5 s, or the topic's next state), the draft that a restart sends after the first state, and the deviations from Qt listed in this plan's Decisions;
  - the smoke, and the measurement command.

- [ ] **Step 5: Follow-ups in Beads** (`bd create` under `zerus-5wmh`; no absolute paths in the text; then `bd export -o .beads/issues.jsonl` and `python3 scripts/check-source-privacy.py`):
  - "Tauri client: follow session renames made while the client was closed (Qt's persisted run tokens, SessionOrganization.cpp:223-245)";
  - "Tauri client: a project catalog on computers without the Qt GUI (swarm initialize without a GUI cache)";
  - "Tauri client: session cards take their project's colour (SessionsWindowRows.cpp:198-200), and the list background menu gets New project and Expand or Collapse all projects (SessionsWindow.cpp:349-353)".

- [ ] **Step 6: Run all checks:**
  - `cargo +1.99.1 test`, clippy with `-D warnings` and fmt in `client/core`;
  - the same in `client/src-tauri`, also with `--no-default-features`;
  - then `npx vitest run`, `npm run check`, `python3 scripts/check-ui-separators.py` and `python3 scripts/check-source-privacy.py`.

- [ ] **Step 7: Commit**

```bash
git add client docs/superpowers/plans .beads/issues.jsonl
git commit -m "docs(client): the shared organization in the Sessions list, its smoke and measurement"
```

---

## Phases of the remaining organization entries

| Entry | Phase and Beads task |
|---|---|
| Batch Move to project from the selection bar and the batch menu | `zerus-5wmh.20` (menu-parity), with `moveToProject` and `openMoveDialog` from this plan |
| The Projects screen's New project, Delete project, rename, colour, default, reorder and folders; the Swarm dialog and conflict review; the legacy `project ls` import | 3c, `zerus-5wmh.25`; New, Rename, Delete and reorder reuse this plan's commands |
| Archived identities (`machine\narchive\nID`), the run memory and its seed, the Archive view grouped by project and the archive placement after Terminate (`SessionsWindow.cpp:3106-3125`) | `zerus-5wmh.40` (core: placement, run memory, Intent, `archive_page` group and rank; UI: the view) and `zerus-5wmh.45` (deviation 6, closed by them) |
| "Hide projects without active sessions" as a setting (default on here) | 3b Settings, `zerus-5wmh.24` |
| The vivid style and this device's project colours | 3c, `zerus-5wmh.25` |
| New session… in the header menu | phase 5 |
| Renames while the client was closed, a catalog without the Qt GUI, card colours and the background menu | the Task 10 follow-ups |
