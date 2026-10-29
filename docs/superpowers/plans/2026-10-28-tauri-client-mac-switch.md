# Tauri Client Mac Switch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Tauri client (`client/`) the native Mac work it needs before it replaces the Qt GUI as Zerus (`com.hgdev.hgs-tray`): the menu-bar item, Qt's single-instance protocol and command line, login through the existing LaunchAgent, quit handling, attention notifications, Qt's main-thread watchdog, a switch bundle, and an updater that switches, verifies and rolls back.

**Architecture:** Decision logic is pure and tested on Linux. The menu-bar model, the tile geometry and a port of Qt's `AttentionTracker` live in `client/core`. The command line, build identity, bundle identity and single-instance protocol are pure modules of `client/src-tauri`. The updater's rules are pure Python functions in `tray/update-installed.py`. Thin macOS glue (objc2, IOKit and Tauri's `tray-icon`) draws the tile, shows the menu, posts notifications, beats the watchdog's heartbeat and keeps the app in the menu bar. The switch bundle is the Tauri build with Qt's identifier, executable name and Info.plist keys. `tray/sign-local-macos.py` signs it, and `tray/install.sh` installs it through `tray/update-installed.py`.

**Tech Stack:** Rust 1.99.1, Tauri 2.12 (`tray-icon` feature), objc2 0.6, objc2-foundation, objc2-app-kit and objc2-user-notifications 0.3.2, block2 0.6, libc, IOKit and libdispatch through a few hand-declared calls; SolidJS and Vitest; Python 3 `unittest`; bash.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`: Platform integrations (rows 289 tray, 290 notifications, 297 watchdog and signals, 298 single instance, 299 autostart, 300 updates), Packaging and signing, Migration. Beads: `zerus-5wmh.44`, `zerus-5wmh.43`, `zerus-5wmh.12.1`, `zerus-5wmh.12.2`, `zerus-5wmh.12.3` (parent `zerus-5wmh.13`). Owner decisions of 2026-10-28 are recorded on `zerus-5wmh.13`; the coordinator's rulings of the same day are on `zerus-5wmh.12.2`, `zerus-5wmh.12.3` and `zerus-5wmh.13.1`.

## Decisions

- **Controller rulings on the plan review (2026-10-28).**
  - Watchdog scope: it runs in the switch bundle, which launchd restarts, and elsewhere only with `ZERUS_CLIENT_WATCHDOG=1`. Accepted, so dev runs on shared machines are never aborted.
  - Client private paths: every path comes from the bundle identity helper (Task 2). The phase 4 plan's Global Constraints now say so, so Task 3's draft store and every later path use the helper.
  - macOS `-D warnings`: zerus-5wmh.51 (PR #426) lands before Task 4 starts. Until then the Mac check expects only main's 20 known warnings.
  - `mac-install.sh` cannot make numbered or preview builds, so on a numbered Dev Mac it updates the CLI and refuses the GUI. Accepted, as AGENTS.md states; numbered switch builds go through `tray/install.sh` from a clean clone.
  - Task 7 stays one commit.

- **Controller rulings on the open questions (2026-10-28).**
  - The primary click shows and raises the window, as Qt does (`TrayAgent::showSessions`). Bead .44's "shows or hides" was wrong.
  - The notification rules apply the host-confirmed session Mute and Qt's optional-question banner setting (`notifications/optionalQuestions`, off by default). There is no "Reduce" notification setting in Qt. Bead .43's wording was wrong.
  - The switch does not wait for M1c. Voice and round video are phase 4 Part B, after the switch. The install enables `--features media` whenever the feature exists.
  - Lazy window: the switch bundle creates its webview on first open, as Qt creates its window lazily. The slower first open is accepted.
- **Coordinator rulings (2026-10-28).**
  - **Rollback slot.** The one kept copy stays the last Qt bundle until the owner retires Qt. A failed start restores automatically. There is no second slot (Decision 12, Task 7).
  - **Numbered Dev guard.** It applies on every platform, as AGENTS.md states. The local preview label (`ZERUS_LOCAL_PREVIEW_LABEL`) is the escape hatch. `docs/reference.md` documents the guard and the label, and the updater's refusal uses the same wording (Decision 13, Task 7). The events and menu-parity sessions are told before this reaches main (runbook, "Before the merge").
  - **Main-thread watchdog (`zerus-5wmh.12.3`).** It lands in this plan, before the bundle task: Task 8 ports Qt's `MainThreadWatchdog` with its thresholds and its exit path (Decision 4).
  - **Data carry-over.** The one-time draft import of `zerus-5wmh.13.1` also reads the dev app's drafts, read-only on the source, the same way it reads Qt's. That import is phase 4's work and is not planned here. Settings start at their defaults, and read marks start fresh (Decision 5). As built: the phase 3b settings ruling seeds settings once, read-only, from Qt's (Task 9's docs).
  - **Update notices.** They are accepted as missing after the switch. The follow-up is `zerus-5wmh.12.4`.
  - **Display windows.** Runbook step 4 needs an owner-given Mac window: the implementer tells the coordinator when the build reaches it. Step 8 is the coordinator's job, done with the owner.
  - **Tasks 3 and 4 reach `integrate/client-webui` together.** Task 3 alone leaves a hidden Mac app with no way back: a closed window or a hidden start leaves no Dock icon and no menu-bar item until Task 4, and only a `sessions` line or a signal reaches the app.

These are settled for this plan. Items 1 to 3 deviate from the spec text and are recorded in the spec's Decisions section by Task 9.

1. **Single instance follows Qt, not `tauri-plugin-single-instance` (deviates from spec row 298).** The client implements Qt's `SingleInstance` protocol (`tray/src/SingleInstance.{h,cpp}`, `tray/src/main.cpp:337-361` and `:424-433`): the per-user start lock `hgs-tray-start.lock` in Qt's runtime location, the socket named `hgs-tray` in the canonical temporary folder, and the one-line `show` and `sessions` activations. `tray/update-installed.py` finds the old GUI's socket by `lsof` (a path ending in `/hgs-tray`), waits on that same path for the new GUI and sends `sessions` (`update-installed.py:187-208`, `:295-315`). The plugin uses another path, so the updater would time out and roll back.
2. **The bundle takes over Qt's identity (supersedes the spec's "A distinct bundle identifier from the Qt app" and "Binary names and paths must not collide with `hgs-tray`", for the Mac only).** These are the owner's decisions of 2026-10-28. The switch bundle is `com.hgdev.hgs-tray`, named Zerus, with executable `hgs-tray`. It replaces `~/Applications/hgs-tray.app` in place, and Qt is not kept beside it on the Mac. The dev app keeps `dev.zerus.next` and its name.
3. **Menu entries: Qt's, except Refresh and the update entries (deviates from spec row 289's "menu with machines and sessions" parity).** Every computer streams through `hgs watch`, and a failed stream restarts on its own schedule, so there is nothing to poll. The client has no update controller yet (spec row 300). The tooltip drops Qt's poll ages for the same reason. The "?" error tile shows while this computer's stream is reconnecting.
4. **The main-thread watchdog follows Qt (spec row 297, `zerus-5wmh.12.3`, coordinator ruling).** Task 8 ports `tray/src/MainThreadWatchdog.{h,cpp}` and `MacSystemPower`: a main-thread heartbeat every 7.5 s, a check every 15 s on a thread named `hgs-watchdog`, a 30 s threshold, uptime that stops during sleep, a hold from the sleep notice until the main thread beats after a wake (10 minutes at most), and `abort()` so launchd's `KeepAlive Crashed` restarts the client and the crash report shows the main thread. The decision logic is pure and tested on Linux; the heartbeat (an `NSTimer` in the common run-loop modes, a GLib timeout on Linux) and the IOKit power notices are thin glue. It runs in the switch bundle, which launchd supervises, and in other builds only with `ZERUS_CLIENT_WATCHDOG=1`, so a dev run on the shared machine is never aborted (Decision 7's rule). Qt's other guard stays too: no status-item update while its menu is open (`QuietTrayIcon`).
5. **The client's private data directory follows the running bundle's identifier (ruling 4).** `identity::Identity::data_dir` is Tauri's `app_data_dir` formula applied to the identifier the running bundle reports (`NSBundle.mainBundle.bundleIdentifier` on macOS). The switch bundle therefore uses `~/Library/Application Support/com.hgdev.hgs-tray/`, which does not collide with Qt's `~/Library/Application Support/hgdev/hgs-tray`. The dev app keeps `~/Library/Application Support/dev.zerus.next/`.
   - **One source for every client path.** Read marks, `menubar.json`, `attention-observed.json` and phase 4's drafts root (`<data dir>/drafts`) all come from `Identity::data_dir_now()`. Tauri's `app.path().app_data_dir()` reports the compile-time identifier, which is `com.hgdev.hgs-tray.local` in a switch build (Decision 6), so the client never calls it. Task 2 manages the `Identity` in the app state for that reason. Phase 4's composer (`DraftStore::open(root.join("drafts"))`) currently takes its root from `app_data_dir`: whichever of phase 4 and this plan lands second routes it through `app.state::<identity::Identity>()`, and `grep -rn "app_data_dir()" client/src-tauri/src` must print nothing (Task 9 checks it). Otherwise the client would write its drafts under the `.local` identifier and the updater would verify an empty drafts root. Phase 4's attachment cache (`app_cache_dir`) is disposable; under the `.local` identifier it is only a misnamed cache, and phase 4 may move it the same way.
   - **Data carry-over (coordinator ruling).** Settings start at their defaults (superseded: the phase 3b ruling seeds them once from Qt's), and read marks start fresh: the hosts' read state stays authoritative. The dev app's webview settings (Animate working sessions, Reduce motion, panel widths) and its pending read marks stay with the dev app. Its durable drafts reach the switched client through the one-time import of `zerus-5wmh.13.1`, which reads the dev app's drafts root (`~/Library/Application Support/dev.zerus.next/drafts`) read-only, the same way it reads Qt's store. That import is phase 4's work; this plan only depends on it (runbook step 8), and until it lands those drafts stay readable in the dev app.
   - **Why not pin:** after the switch the dev app keeps running beside the installed client for testing. A pinned directory would give one drafts root two writers, which the phase 4 core section rules out ("each store has exactly one writer").
6. **The switch build is configured as `com.hgdev.hgs-tray.local` and promoted at signing.** This is Qt's privacy split (`tray/CMakeLists.txt:283-285`, `tray/sign-local-macos.py:134-143`): an unsigned copy in a build folder never registers the public identifier. The runtime identity therefore comes from the bundle, not from Tauri's compile-time configuration (Decision 5).
7. **Only the switch identity claims Qt's names, posts notifications and runs the watchdog.** The dev app gets single-instance only with `ZERUS_CLIENT_SINGLE_INSTANCE=1`, under its own name, notifications only with `ZERUS_CLIENT_NOTIFICATIONS=1`, and the main-thread watchdog only with `ZERUS_CLIENT_WATCHDOG=1`. This keeps parallel headless runs on the shared arch machine working. It also means a dev client never claims Qt's `/tmp/hgs-tray` on arch and never doubles Qt's banners on the Mac. A replay (`ZERUS_CLIENT_REPLAY`) never posts notifications.
8. **Primary click shows and activates the window, as Qt's `TrayAgent::showSessions` does (`TrayAgent.cpp:781-783`).** Qt never hides on click; the controller confirmed it.
9. **`show` changes nothing visible.** Qt's handler acts only on `sessions` (`main.cpp:428`). A launchd or second plain start therefore never pops the window.
10. **The "Open in" choice lives in the client's data directory (`menubar.json`).** It is seeded once from `open-mode` in `~/.config/hgs/tray.conf`, which the client never writes. `icon-color` from the same file is honoured read-only.
11. **The attention state is the client's own file (`attention-observed.json`).** On the switch identity's first start it is seeded from Qt's `attention/observed` value (`TrayAgent.cpp:157`). That value is read from the shared NSUserDefaults domain, read-only. Banners Qt delivered under the same bundle identifier are therefore withdrawn when their attention clears, and stay clickable.
12. **Rollback slot (coordinator ruling).** The one kept copy, `~/.local/share/hgs/desktop-previous/hgs-tray.app`, stays the last Qt bundle until the owner retires Qt. When the client updates itself, the replaced client waits only in the update's own staging folder while that update runs: a new client that does not start is replaced by it automatically, and after any other outcome the staging folder goes with it. There is no second slot and no later rollback to an older client. `--rollback` reinstalls the Qt copy, and only then does the client move into the slot. A copy moved into the slot is unregistered from LaunchServices, so a notification click cannot launch the old app.
13. **Numbered Dev guard, all platforms (coordinator ruling).** `update-installed.py` refuses to replace an installed numbered Dev build with a build that has neither a Dev number nor a local preview label (`previewLabel` in `--build-info`, from `ZERUS_LOCAL_PREVIEW_LABEL`). `HGS_TRAY_RESTART=0` installs, which replace the GUI on disk without the live update, ask the updater first (`--check`), so no path skips the guard. `docs/reference.md` states the rule and the label, and the refusal quotes the same sentence; a test keeps the two equal. The Tauri client embeds the label as Qt does (Task 2).
14. **Mute applies as in Qt.** Muting is the host-confirmed `notification_preferences` (`SessionNotifications::valid`). The optional-questions banner setting (`notifications/optionalQuestions`, default off) is read from Qt's domain until the client has its own Settings entry. Qt has no notification setting named "Reduce"; the controller confirmed it.
15. **Run-event hook merge rule.** All lifecycle handling lives in `client/src-tauri/src/lifecycle.rs`. The run chain in `lib.rs` is `let mut app = …build(context).expect(…);`, then `lifecycle::prepare(&mut app, menu_bar, opens_sessions);` (macOS: the policy the app launches with, Task 3), then `app.run(|app, event| { lifecycle::on_event(app, &event); })`, with `on_event` as the closure's first statement. `RunEvent::Exit` reaches `lifecycle::exit`, the one exit hook. The watchdog stops first (Task 8). Other exit work (phase 4's drafts flush, the media branch's voice close, the organisation and read-mark flushes) runs there next. `state.release()` comes last, so the single-instance socket goes after every flush, as Qt's `QLocalServer` outlives the tray agent whose `aboutToQuit` flushes (`main.cpp:340` vs `:378`). The media branch (`work-ba0ed9fd-media`) adds a `.build(...).expect(...).run(...)` chain with an `if let tauri::RunEvent::Exit` block calling its `exit(app)`. Whichever lands second keeps the chain above and moves that exit work into `lifecycle::exit`, before `state.release()`. The shared `.build(...)` takes a `context` built earlier.
16. **Merge point with M1c (media).** M1c adds `client/src-tauri/Info.plist` (microphone text), `Entitlements.plist` (audio input) and the `media` feature. The switch configuration names its own `Info.switch.plist`, which repeats Qt's microphone text. `tray/install.sh` adds `--features media` whenever the manifest has that feature, so the build order does not matter.
17. **Release packaging stays phase 7.** A notarized Tauri Zerus.app with the CLI, connector, setup scripts and notices is phase 7 work. This plan makes `scripts/check-macos-bundle.py --gui tauri` and `scripts/sign-macos.py` accept the switch bundle (hardened runtime, `packaging/macos/Zerus.entitlements`). Notarization itself runs only in the `macos-signing` environment.

## Global Constraints

- **Rust 1.99.1 only:** `cargo +1.99.1` (or `RUSTUP_TOOLCHAIN=1.99.1`). On the shared machine prefix builds with `nice` and set `CARGO_BUILD_JOBS=4`.
- **No new dependencies.** Allowed: objc2 crates already in `client/src-tauri/Cargo.lock` (`objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-user-notifications`, `block2`), Tauri's existing `tray-icon` feature (its crates are already in the lock) and `libc`, which is already a direct dependency. IOKit and libdispatch, which no crate of the lock binds, are declared by hand in `platform/macos_power.rs` (Task 8). `client/core` keeps the dependencies it has on main (`base64`, `infer`, `serde`, `serde_json`, `sha2`, `zerus-watch-protocol`, `libc`); the attention tokens use its `base64`, and the notice IDs are injected. After every Cargo change, `git diff --stat -- client/*/Cargo.lock` must show no new `[[package]]`.
- **Mac display runs are not allowed** until the owner gives a window, which the coordinator relays: tell the coordinator when a task reaches one (runbook step 4). Builds and display-free tests over `ssh -o BatchMode=yes mac` are fine, only in the persistent checkout `~/zerus-dev/webui`, only on a scratch branch, and only while holding the shared heavy-build lock (**mac-lock** to **mac-done** below). Never open a window or start the app there; never touch `~/zerus-next-p2`, which other sessions use.
- **Never touch, on any machine:** the installed GUI (`~/Applications/hgs-tray.app`) or anything else in `~/Applications`, `~/Library/LaunchAgents`, `~/Library/Preferences/com.hgdev.hgs-tray*` (including through `defaults`), the hgs CLI, `com.hgdev.hgs-recovery`, `com.hgdev.hgs-voice`, native agents, tmux, services, and the mobile connector. Tests use a fake HOME, fake bundles with the identifier `com.example.zerus-switch-test`, and a fake `defaults` executable.
- **Installing on the owner's Mac stays with the coordinator.** This plan ends with a runbook; no task performs the switch.
- **Arch display runs:** any time, under `flock -w 1800 /tmp/zerus-arch-display.lock`, closing every window opened. **Headless runs only with** `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" <cmd>`.
- **Repository text** is English. Fixtures are synthetic: example domains, `/Users/example` or `/home/example` paths. No team IDs, certificate names, device identifiers or other identities go into the repository.
- **The web UI never separates values with dots.** `scripts/check-ui-separators.py` scans `client` (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB). The menu-bar marker U+25CF before an attached session is Qt's status mark, not a separator, and is not in that list.
- **Commits:** every commit message ends with the `Co-Authored-By: Claude Opus 5.5` trailer and address given in the implementer's instructions. The examples below omit the trailer because the source privacy guard rejects its address in repository files. The owner authorized commits, pushes, PRs and merging green, reviewed work to main (2026-10-13). Each task commits; pushing follows the coordinator.
- **Formatting.** The Rust in this plan is not rustfmt-formatted: run `cargo +1.99.1 fmt --manifest-path core/Cargo.toml` and the same for `src-tauri/Cargo.toml` (from `client/`) before the format check.
- **Cargo.lock.** The checks run with `--locked`. A task that adds a direct dependency (Tasks 4 and 6: `block2`, `objc2-app-kit`, `objc2-user-notifications`, all already in the lock) records it once with `cd client && cargo +1.99.1 metadata --offline --format-version 1 --manifest-path src-tauri/Cargo.toml > /dev/null`. Then `git diff client/src-tauri/Cargo.lock` must show only new lines inside existing `dependencies` lists, never a new `[[package]]`. Feature changes never touch the lock.
- **Secrets.** CI's gitleaks scan fails on token-like strings, synthetic ones included. Test values are words (`sample`, `placeholder`) or repeated characters, such as the forty `a` characters that stand for a commit in Task 2.
- **Checks before each task's commit** (from the repository root unless noted; skip the ones a task does not touch):
  - `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 fmt --check --manifest-path core/Cargo.toml && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 fmt --check --manifest-path src-tauri/Cargo.toml`
  - `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 clippy --locked --manifest-path core/Cargo.toml --all-targets -- -D warnings`
  - `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings`
  - `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml` and the same for `src-tauri/Cargo.toml`
  - `cd client && npm run check && npm test`
  - `python3 scripts/check-ui-separators.py && python3 scripts/check-source-privacy.py`
  - `python3 -m unittest discover -s tests -p 'test_macos_*.py' -v` plus the task's own Python tests
- **Shared commands** (used by name in the tasks). Run them from the repository root, in this order, and always end with **mac-done**, also when a step failed:
  - **mac-lock** takes the shared heavy-build lock of the Mac, waiting up to 60 minutes while another session holds it:
    ```sh
    owner="$(basename "$(git rev-parse --show-toplevel)") (mac switch plan)"
    for attempt in $(seq 1 120); do
      ssh -o BatchMode=yes mac "mkdir ~/zerus-dev/.heavy.lock 2>/dev/null && echo '$owner' > ~/zerus-dev/.heavy.lock/owner" && break
      ssh -o BatchMode=yes mac 'echo "held by $(cat ~/zerus-dev/.heavy.lock/owner)"'; sleep 30
    done
    ```
  - **mac-sync** puts this branch's sources on a scratch branch of the persistent checkout `~/zerus-dev/webui`, at the commit of `origin/main` this branch starts from, keeping the checkout's build caches. `tar -m` gives the files the current time, so cargo rebuilds what changed:
    ```sh
    base="$(git merge-base HEAD origin/main)"
    git ls-files -z --cached --others --exclude-standard client crates/watch-protocol tray scripts tests packaging VERSION DEV_VERSION \
      | tar --null -T - -czf - \
      | ssh -o BatchMode=yes mac "cd ~/zerus-dev/webui && git fetch -q origin && git checkout -q -- . \
          && git clean -fdq -- client tray scripts tests packaging && git checkout -q -B scratch/mac-switch $base && tar -xmzf -"
    ```
  - **mac-clippy** runs the macOS lints display-free:
    ```sh
    ssh -o BatchMode=yes mac 'cd ~/zerus-dev/webui && export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH" RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=6 \
      && cargo clippy --locked --keep-going --manifest-path client/core/Cargo.toml --target-dir target/client-core --all-targets -- -D warnings \
      && cargo clippy --locked --keep-going --manifest-path client/src-tauri/Cargo.toml --target-dir target/client-tauri --all-targets \
      && cargo clippy --locked --keep-going --manifest-path client/src-tauri/Cargo.toml --target-dir target/client-tauri --no-default-features --all-targets'
    ```
    Expected: exit 0, and no warning in a file this plan adds or a line it changes. Until `zerus-5wmh.51` is on main, `client/src-tauri` has 20 dead-code warnings of its own on macOS (`display.rs`, `metrics.rs`, and the measurement helpers and `CVDisplayLink` declarations of `platform/macos.rs`); they are the only warnings expected. Once that fix is on main, add `-- -D warnings` to the two src-tauri commands.
  - **mac-test** runs the unit tests display-free:
    ```sh
    ssh -o BatchMode=yes mac 'cd ~/zerus-dev/webui && export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH" RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=6 \
      && cargo test --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core \
      && cargo test --locked --manifest-path client/src-tauri/Cargo.toml --target-dir target/client-tauri'
    ```
  - **mac-done** returns the checkout to `origin/main`, deletes the scratch branch and releases the lock:
    ```sh
    ssh -o BatchMode=yes mac 'cd ~/zerus-dev/webui && git checkout -q -- . && git clean -fdq -- client tray scripts tests packaging \
      && git checkout -q --detach origin/main && git branch -D scratch/mac-switch; rm -rf ~/zerus-dev/.heavy.lock'
    ```

## Review Focus

These are the conditions most likely to hurt a person using this client that the spec implies but does not spell out. Each line names the test that pins it.

1. **A second start while launchd is still starting the client** (KeepAlive restart, Finder open at login, `hgs-tray --sessions` from a launcher) must wait for the service and forward its line, never leave two menu-bar items or exit with code 3. Pinned by `instance::tests::a_start_waits_for_the_starting_service_then_forwards` (Task 2).
2. **A socket or lock left by a crash** (KeepAlive Crashed restarts the client) must be reclaimed, not fail. Pinned by `instance::tests::a_crashed_instance_socket_is_claimed` and `a_dead_holders_lock_is_taken` (Task 2).
3. **The menu open while the fleet changes** must not update the status item; that can deadlock AppKit on macOS 26 (`QuietTrayIcon.h`). The latest state shows once the menu closes. Pinned by `menubar::tests::gate_defers_everything_while_the_menu_is_open` (Task 1).
4. **A notification clicked after its session was renamed, resumed, archived or forgotten** must open the session by conversation or run identity, or say "This session is no longer available: NAME". It must never open another session. Pinned by `attention::tests::targets_match_by_conversation_run_or_name_and_creation` (Task 5) and `attention::tests::a_click_waits_then_reports_a_gone_session` (Task 5).
5. **An update interrupted or refused mid-switch** must leave Qt's drafts and settings untouched and keep Qt as the rollback copy. A failing new client must restore the GUI it replaced, never an unexpected one. Pinned by `test_tray_update_switch.SlotAndRestore` (Task 7) and `test_tray_update.TrayUpdate.test_rollback_reinstalls_the_kept_copy` (Task 7).
6. **An unnumbered build over the owner's numbered Dev install**, through the live update or `HGS_TRAY_RESTART=0`, must be refused before anything is replaced or restarted. Pinned by `test_tray_update.TrayUpdate.test_an_unnumbered_build_never_replaces_a_numbered_dev_install` and `test_tray_update_switch.Switch.test_an_on_disk_install_asks_the_updater_first` (Task 7).
7. **A Mac waking from sleep, or a dark wake on battery,** must not look like a hung main thread, and a main thread that never runs again after a wake must still count. Pinned by `watchdog::tests::a_wake_holds_until_the_main_thread_beats` and `a_main_thread_silent_past_the_wake_grace_is_hung` (Task 8); an open menu is the runbook's step 4 display check.

## File Structure

| Path | Task | Responsibility |
| --- | --- | --- |
| `client/core/src/view.rs` (modify) | 1, 5 | `NotifyFacts`: what the menu bar and notifications read from a row, never serialized |
| `client/core/src/menubar.rs` (new) | 1 | `Desk` mirror of the fleet, `Face`, tooltip, menu `Entry` list, `OpenMode`, `Gate` |
| `client/core/src/trayicon.rs` (new) | 4 | Tile geometry and colours, the Zerus mark path, `unpremultiply`, colour parsing |
| `client/core/src/shell.rs` (modify) | 4 | `terminal_app_script`, `notification_script` (Qt's `MacBackend`), `notification_text` |
| `client/core/src/attention.rs` (new) | 5 | Port of `AttentionTracker`, preference validation, tokens, `Delivery`, click resolution, seeding |
| `client/src-tauri/build_identity.rs` (new) | 2 | Pure build-identity rules shared by `build.rs` and the tests |
| `client/src-tauri/build.rs` (modify) | 2 | Embeds VERSION, DEV_VERSION, the Dev number, the local preview label and the source commit |
| `client/src-tauri/src/cli.rs` (new) | 2 | Qt's command line, `--build-info`, `--version`, `--help` |
| `client/src-tauri/src/identity.rs` (new) | 2, 6, 8 | Running bundle identity, data directory, single-instance name, notifications and watchdog switches |
| `client/src-tauri/src/instance.rs` (new) | 2 | Qt's single-instance protocol on Unix |
| `client/src-tauri/src/lifecycle.rs` (new) | 3 | Window show/hide, activation policy, reopen, run-event hook, socket cleanup |
| `client/src-tauri/src/signals.rs` (new) | 3 | SIGTERM, SIGINT, SIGHUP quit like the menu's Quit |
| `client/src-tauri/src/desk.rs` (new) | 4, 6 | Desk thread: changes into the menu bar and notifications; menu actions; pending opens |
| `client/src-tauri/src/pump.rs` (modify) | 4 | A tap that hands every change to the desk thread |
| `client/src-tauri/src/watchdog.rs` (new) | 8 | Qt's main-thread watchdog: `Pulse`, `Watchdog`, uptime clock, abort |
| `client/src-tauri/src/platform/macos.rs` (modify) | 2, 3, 6, 8 | Bundle identifier, main-thread QoS, Qt defaults reads, the watchdog's heartbeat |
| `client/src-tauri/src/platform/linux.rs` (modify) | 8 | The watchdog's heartbeat |
| `client/src-tauri/src/platform/macos_power.rs` (new) | 8 | IOKit sleep and wake notices for the watchdog |
| `client/src-tauri/src/platform/macos_menubar.rs` (new) | 4 | Status item, tile drawing with AppKit, menu, menu-tracking observers |
| `client/src-tauri/src/platform/macos_notify.rs` (new) | 6 | `UNUserNotificationCenter` glue and its delegate |
| `client/src-tauri/src/lib.rs`, `commands.rs` (modify) | 2, 3, 4, 6, 8 | Startup order, state, command list |
| `client/src-tauri/Cargo.toml`, `Cargo.lock` (modify) | 2, 4, 6, 8 | The `tray-icon` feature, objc2 crates already in the lock, feature lists |
| `client/src/bridge.ts`, `client/src/store/desk.ts` (new), `client/src/main.tsx` | 4 | `open-session` and `desk-notice` events |
| `tray/update-installed.py` (modify) | 7 | Draft roots, GUI kind, slot policy, Dev guard, settings snapshot and check, `--rollback`, `--installed-gui`, `--check` |
| `docs/reference.md` (modify) | 7 | The numbered Dev guard and the local preview label |
| `client/src-tauri/tauri.switch.conf.json`, `Info.switch.plist`, `client/scripts/switch-config.py` (new) | 9 | The switch bundle |
| `tray/install.sh`, `mac-install.sh` (modify) | 7, 9 | `HGS_TRAY_RESTART=0` asks `--check` (7); build and install the client when chosen or already installed (9) |
| `scripts/check-macos-bundle.py`, `scripts/sign-macos.py` (modify) | 9 | Accept the Tauri bundle |
| `docs/macos-distribution.md`, `client/README.md`, the spec (modify) | 2, 9 | Docs and the switch runbook |

---

### Task 1: Core menu-bar model (zerus-5wmh.44)

The pure model of Qt's status item: counters, stale dot and "?" (`tray/src/IconFactory.cpp:57-65`, `:178-186`), counts (`tray/src/FleetState.cpp:392-435`), tooltip (`FleetState.cpp:437-470`), menu (`tray/src/TrayMenu.cpp:23-31`, `:156-246`) and the update gate (`tray/src/QuietTrayIcon.h`).

**Files:**
- Modify: `client/core/src/view.rs` (the `ViewRow` struct and `row()`)
- Create: `client/core/src/menubar.rs`
- Modify: `client/core/src/lib.rs` (add `pub mod menubar;`)
- Test: unit tests inside `client/core/src/menubar.rs` and `client/core/src/view.rs`

**Interfaces:**
- Consumes: `zerus_client_core::{Change, ComputerId, ComputerStatus, ViewRow, MachineMeta}` as on main.
- Produces:
  - `view::NotifyFacts { attached: bool, needs_action: bool, acknowledged: bool }` and `ViewRow::notify: NotifyFacts` (`#[serde(skip)]`). Task 5 adds fields.
  - `menubar::Computer { status, listed, host, local_sessions, rows }` with `fresh()` and `visible()`.
  - `menubar::Desk` with `observe(&Change) -> Option<ComputerId>`, `computers()`, `error()`, `total()`, `attention()`, `stale()`, `face() -> Face`, `badge() -> Option<String>`, `tooltip() -> String`, `menu(OpenMode) -> Vec<Entry>`.
  - `menubar::Face { total, attention, highlighted, dot }` with `counts()` and `error()`; `menubar::count_text(usize) -> String`.
  - `menubar::OpenMode { Terminal, Clipboard }` with `ALL`, `label()` and `from_config()`; `menubar::tray_conf_value(text, key) -> Option<String>`.
  - `menubar::Entry { Open, Separator, Label(String), Session { computer, key, name, text, running }, OpenIn(OpenMode), Quit }`.
  - `menubar::Gate` with `offer(Face, String, Vec<Entry>) -> Vec<Apply>`, `menu_opened()`, `menu_closed() -> Vec<Apply>` and `is_open()`; `menubar::Apply { Face(Face), Tooltip(String), Menu(Vec<Entry>) }`.
  - `menubar::needs_attention(&ViewRow) -> bool`, `menubar::session_text(&ViewRow) -> String`, `menubar::escape(&str) -> String`.

- [ ] **Step 1: Write the failing view test**

Add to the `tests` module of `client/core/src/view.rs`:

```rust
    #[test]
    fn notify_facts_carry_attachment_action_and_acknowledgement() {
        let busy = row(&json!({
            "name": "claude/sample-project/review", "attached": 2, "tracked": true,
            "phase": "approval", "conversation_id": "c", "attention_signature": "sig",
            "read": {"conversation_id": "c", "attention_signature": "sig"}
        }));
        assert!(busy.notify.attached);
        assert!(busy.notify.needs_action);
        assert!(busy.notify.acknowledged);
        let quiet = row(&json!({"name": "codex/sample-project/docs", "attached": 0, "tracked": true}));
        assert!(!quiet.notify.attached && !quiet.notify.needs_action && !quiet.notify.acknowledged);
        // The frontend never receives the facts.
        assert!(serde_json::to_value(&busy).unwrap().get("notify").is_none());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml notify_facts_carry`
Expected: FAIL to compile with "no field `notify` on type `ViewRow`" and "cannot find type `NotifyFacts`".

- [ ] **Step 3: Add `NotifyFacts` to the view**

In `client/core/src/view.rs`, add after the `ViewRow` struct:

```rust
/// What the menu bar and attention notifications read beyond the card. It is
/// never serialized: the frontend does not receive it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotifyFacts {
    /// `attached > 0`: a terminal client is attached (`TrayMenu.cpp:30` shows "● ").
    pub attached: bool,
    /// `SessionInfo::needsAction` (`tray/src/HgsClient.h:109-116`).
    pub needs_action: bool,
    /// The host's read mark names the current attention signature.
    pub acknowledged: bool,
}
```

Add the last field of `ViewRow`, after `terminal_attach_reported`:

```rust
    /// What the menu bar and notifications read; never sent to the frontend.
    #[serde(skip)]
    pub notify: NotifyFacts,
```

In `pub fn row(value: &Value) -> ViewRow`, add the last field of the returned literal:

```rust
        notify: NotifyFacts {
            attached: value.get("attached").and_then(Value::as_i64).unwrap_or(0) > 0,
            needs_action: session.needs_action(),
            acknowledged: session.attention_acknowledged(),
        },
```

Every other `ViewRow { .. }` literal in `client/` already ends with `..ViewRow::default()` or `..Default::default()`. Run `grep -rn "ViewRow {" client --include=*.rs` and confirm that before building.

- [ ] **Step 4: Run it to verify it passes**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml notify_facts_carry`
Expected: PASS.

- [ ] **Step 5: Write the failing menu-bar tests**

Create `client/core/src/menubar.rs` with only the test module below and `pub mod menubar;` in `lib.rs`, so the tests fail to compile:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Change, ComputerId, ComputerStatus, MachineMeta, ViewRow};

    fn row(name: &str, state: &str) -> ViewRow {
        ViewRow { name: name.to_owned(), state: state.to_owned(), ..ViewRow::default() }
    }

    fn unread(name: &str) -> ViewRow {
        ViewRow { unread: true, ..row(name, "running") }
    }

    fn asking(name: &str) -> ViewRow {
        let mut row = row(name, "running");
        row.notify.needs_action = true;
        row
    }

    fn listed(desk: &mut Desk, computer: &str, rows: Vec<(&str, ViewRow)>) {
        let id = ComputerId::from(computer);
        let rows = rows.into_iter().map(|(key, row)| (key.to_owned(), row)).collect();
        desk.observe(&Change::Replace { computer: id.clone(), rows });
        desk.observe(&Change::Computer { computer: id, status: ComputerStatus::Live });
    }

    fn status(desk: &mut Desk, computer: &str, status: ComputerStatus) {
        desk.observe(&Change::Computer { computer: ComputerId::from(computer), status });
    }

    fn host(desk: &mut Desk, computer: &str, name: &str, local_sessions: bool) {
        let machine = MachineMeta {
            host: Some(name.to_owned()),
            local_sessions: Some(local_sessions),
            ..MachineMeta::default()
        };
        desk.observe(&Change::Machine { computer: ComputerId::from(computer), machine });
    }

    #[test]
    fn counts_follow_qt_total_and_attention() {
        let mut desk = Desk::default();
        let mut acknowledged = asking("claude/p/acknowledged");
        acknowledged.notify.acknowledged = true;
        let mut muted = asking("claude/p/muted");
        muted.muted = true;
        listed(&mut desk, "", vec![
            ("a", asking("claude/p/approval")),
            ("b", unread("codex/p/reply")),
            ("c", acknowledged),
            ("d", muted),
            ("e", row("claude/p/old", "archived")),
        ]);
        assert_eq!(desk.total(), 4, "archived sessions are not counted");
        assert_eq!(desk.attention(), 2, "acknowledged and muted sessions do not count");
        assert_eq!(desk.face(), Face { total: "4".into(), attention: "2".into(), highlighted: true, dot: false });
    }

    #[test]
    fn counts_cap_at_ninety_nine() {
        assert_eq!(count_text(0), "0");
        assert_eq!(count_text(99), "99");
        assert_eq!(count_text(100), "99+");
        assert!(!Face::counts(3, 0, false).highlighted);
    }

    #[test]
    fn the_dock_badge_is_the_exact_attention_count() {
        let mut desk = Desk::default();
        assert_eq!(desk.badge(), None);
        listed(&mut desk, "", vec![("a", asking("claude/p/a")), ("b", unread("claude/p/b"))]);
        assert_eq!(desk.badge().as_deref(), Some("2"));
        status(&mut desk, "", ComputerStatus::Reconnecting { attempt: 1 });
        assert_eq!(desk.badge(), None, "no badge while the counters are unknown");
    }

    #[test]
    fn a_peer_that_is_not_current_marks_the_tile_stale_and_counts_no_attention() {
        let mut desk = Desk::default();
        listed(&mut desk, "", vec![("a", row("claude/p/here", "running"))]);
        listed(&mut desk, "mac", vec![("b", asking("claude/p/there"))]);
        assert_eq!(desk.attention(), 1);
        assert!(!desk.stale());
        status(&mut desk, "mac", ComputerStatus::Reconnecting { attempt: 1 });
        assert_eq!(desk.total(), 2, "the last known rows still count");
        assert_eq!(desk.attention(), 0);
        assert!(desk.stale());
        status(&mut desk, "studio", ComputerStatus::Probing);
        assert!(desk.face().dot, "a peer that has not answered yet is stale");
    }

    #[test]
    fn a_reconnecting_local_stream_shows_the_question_mark() {
        let mut desk = Desk::default();
        listed(&mut desk, "", vec![("a", row("claude/p/x", "running"))]);
        status(&mut desk, "", ComputerStatus::Reconnecting { attempt: 2 });
        assert_eq!(desk.face(), Face::error());
        assert_eq!(desk.tooltip(), "Zerus: reconnecting to hgs on this computer");
    }

    #[test]
    fn the_tooltip_has_qt_lines_without_poll_ages() {
        let mut desk = Desk::default();
        host(&mut desk, "", "arch", true);
        listed(&mut desk, "", vec![("a", unread("claude/p/a")), ("b", row("claude/p/b", "paused"))]);
        listed(&mut desk, "mac", vec![("c", row("codex/p/c", "running"))]);
        status(&mut desk, "studio", ComputerStatus::Probing);
        listed(&mut desk, "win", vec![]);
        status(&mut desk, "win", ComputerStatus::Reconnecting { attempt: 1 });
        assert_eq!(
            desk.tooltip(),
            "1 needs attention, 3 sessions total\n2 here\n1 on mac\nstudio: not polled yet\nwin: unreachable"
        );
        let mut empty = Desk::default();
        host(&mut empty, "", "arch", true);
        assert_eq!(empty.tooltip(), "0 need attention, 0 sessions total\n0 here");
    }

    #[test]
    fn the_menu_lists_computers_and_sessions_in_qt_order() {
        let mut desk = Desk::default();
        host(&mut desk, "", "arch", true);
        let mut attached = row("claude/R&D/main", "running");
        attached.notify.attached = true;
        listed(&mut desk, "", vec![
            ("k2", row("codex/p/zeta", "stopped")),
            ("k1", attached),
            ("k3", row("claude/p/old", "archived")),
        ]);
        listed(&mut desk, "mac", vec![]);
        status(&mut desk, "studio", ComputerStatus::Probing);
        listed(&mut desk, "win", vec![("k4", row("claude/p/w", "running"))]);
        status(&mut desk, "win", ComputerStatus::Reconnecting { attempt: 1 });
        let local = ComputerId::local();
        assert_eq!(desk.menu(OpenMode::Clipboard), vec![
            Entry::Open,
            Entry::Separator,
            Entry::Label("arch".into()),
            Entry::Session { computer: local.clone(), key: "k1".into(), name: "claude/R&D/main".into(),
                             text: "\u{25cf} claude/R&&D/main".into(), running: true },
            Entry::Session { computer: local, key: "k2".into(), name: "codex/p/zeta".into(),
                             text: "codex/p/zeta  (stopped)".into(), running: false },
            Entry::Separator,
            Entry::Label("mac".into()),
            Entry::Label("no sessions".into()),
            Entry::Separator,
            Entry::Label("studio".into()),
            Entry::Label("studio: polling\u{2026}".into()),
            Entry::Separator,
            Entry::Label("win".into()),
            Entry::Label("win: unreachable".into()),
            Entry::Separator,
            Entry::OpenIn(OpenMode::Clipboard),
            Entry::Quit,
        ]);
    }

    #[test]
    fn a_local_computer_that_has_not_answered_is_this_box_polling() {
        let desk = Desk::default();
        assert_eq!(&desk.menu(OpenMode::Terminal)[..4], &[
            Entry::Open,
            Entry::Separator,
            Entry::Label("this box".into()),
            Entry::Label("polling\u{2026}".into()),
        ]);
    }

    #[test]
    fn a_control_only_computer_without_peers_says_so() {
        let mut desk = Desk::default();
        host(&mut desk, "", "win", false);
        listed(&mut desk, "", vec![]);
        assert_eq!(desk.menu(OpenMode::Terminal), vec![
            Entry::Open,
            Entry::Separator,
            Entry::Label("No remote machines".into()),
            Entry::Separator,
            Entry::OpenIn(OpenMode::Terminal),
            Entry::Quit,
        ]);
        assert_eq!(desk.tooltip(), "0 need attention, 0 sessions total");
    }

    #[test]
    fn an_ended_computer_leaves_the_desk() {
        let mut desk = Desk::default();
        listed(&mut desk, "mac", vec![("a", row("claude/p/a", "running"))]);
        assert_eq!(
            desk.observe(&Change::Computer { computer: ComputerId::from("mac"), status: ComputerStatus::Ended }),
            Some(ComputerId::from("mac"))
        );
        assert!(desk.computers().is_empty());
    }

    #[test]
    fn session_text_follows_qt_suffixes() {
        assert_eq!(session_text(&row("a/b", "paused")), "a/b  (paused)");
        assert_eq!(session_text(&row("a/b", "stopped")), "a/b  (stopped)");
        assert_eq!(session_text(&row("a/b&c", "running")), "a/b&&c");
        let mut attached = row("a/b", "paused");
        attached.notify.attached = true;
        assert_eq!(session_text(&attached), "a/b  (paused)", "a saved session shows no attachment");
    }

    #[test]
    fn gate_applies_only_changes() {
        let mut gate = Gate::default();
        let face = Face::counts(1, 0, false);
        let applied = gate.offer(face.clone(), "t".into(), vec![Entry::Quit]);
        assert_eq!(applied, vec![
            Apply::Face(face.clone()),
            Apply::Tooltip("t".into()),
            Apply::Menu(vec![Entry::Quit]),
        ]);
        assert!(gate.offer(face.clone(), "t".into(), vec![Entry::Quit]).is_empty());
        assert_eq!(gate.offer(face, "u".into(), vec![Entry::Quit]), vec![Apply::Tooltip("u".into())]);
    }

    #[test]
    fn gate_defers_everything_while_the_menu_is_open() {
        let mut gate = Gate::default();
        gate.offer(Face::counts(1, 0, false), "a".into(), vec![Entry::Open]);
        gate.menu_opened();
        assert!(gate.is_open());
        assert!(gate.offer(Face::counts(2, 1, false), "b".into(), vec![Entry::Quit]).is_empty());
        assert!(gate.offer(Face::counts(3, 1, true), "c".into(), vec![Entry::Quit]).is_empty());
        // Only the latest offer reaches the status item once the menu closes.
        assert_eq!(gate.menu_closed(), vec![
            Apply::Face(Face::counts(3, 1, true)),
            Apply::Tooltip("c".into()),
            Apply::Menu(vec![Entry::Quit]),
        ]);
        assert!(gate.menu_closed().is_empty());
    }

    #[test]
    fn open_mode_reads_tray_conf() {
        let conf = "# comment\nhgs=/opt/example/hgs\nopen-mode = clipboard\n\nopen-mode=terminal\n";
        assert_eq!(tray_conf_value(conf, "open-mode").as_deref(), Some("terminal"));
        assert_eq!(tray_conf_value(conf, "icon-color"), None);
        assert_eq!(OpenMode::from_config("clipboard"), Some(OpenMode::Clipboard));
        assert_eq!(OpenMode::from_config("tab"), None, "Linux modes are not macOS modes");
        assert_eq!(OpenMode::Terminal.label(), "Terminal.app");
    }
}
```

`MachineMeta` already derives `Default` (`client/core/src/machine.rs:10`).

- [ ] **Step 6: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml menubar`
Expected: FAIL to compile: `Desk`, `Face`, `Entry`, `OpenMode`, `Gate` and the functions are not found.

- [ ] **Step 7: Implement the model**

Put this above the test module in `client/core/src/menubar.rs`:

```rust
//! The menu-bar item's model, as the Qt client shows it: two counters with a
//! stale dot or "?" (`tray/src/IconFactory.cpp:57-65`, `:178-186`), the
//! tooltip (`tray/src/FleetState.cpp:437-470`) and the menu
//! (`tray/src/TrayMenu.cpp`). [`Desk`] mirrors the core's changes; [`Gate`]
//! decides what reaches the status item (`tray/src/QuietTrayIcon.h`). Pure:
//! the platform glue draws and shows it.
use crate::{Change, ComputerId, ComputerStatus, ViewRow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One computer as the menu bar knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Computer {
    pub status: ComputerStatus,
    /// A catalog listing arrived at least once (Qt's `everPolled`).
    pub listed: bool,
    /// The host name its machine card reports.
    pub host: Option<String>,
    /// False for a control-only computer (`local_sessions` in its card).
    pub local_sessions: bool,
    pub rows: BTreeMap<String, ViewRow>,
}

impl Default for Computer {
    fn default() -> Self {
        Self {
            status: ComputerStatus::Probing,
            listed: false,
            host: None,
            local_sessions: true,
            rows: BTreeMap::new(),
        }
    }
}

impl Computer {
    /// Its rows are current. Qt counts a peer's attention only within
    /// `kPeerStaleMs` of its last answer (`FleetState.cpp:421-435`); a stream
    /// is current while it is live or polled.
    pub fn fresh(&self) -> bool {
        self.listed && matches!(self.status, ComputerStatus::Live | ComputerStatus::Polled)
    }

    /// Sessions that are not archived (`visibleSessionCount`, `FleetState.cpp:17-21`).
    pub fn visible(&self) -> usize {
        self.rows.values().filter(|row| row.state != "archived").count()
    }
}

/// `SessionInfo::needsAttention` (`tray/src/HgsClient.h:107-108`) without
/// the device-local Review later mark, which this client does not keep yet.
pub fn needs_attention(row: &ViewRow) -> bool {
    (row.notify.needs_action && !row.notify.acknowledged)
        || (row.state != "archived" && row.unread)
}

/// Qt's badge text: "0".."99", then "99+" (`IconFactory::countText`).
pub fn count_text(count: usize) -> String {
    if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    }
}

/// What the tile shows (`IconFactory::forCounts`, `IconFactory::forError`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub total: String,
    pub attention: String,
    /// The attention row is amber.
    pub highlighted: bool,
    /// The dot in the top right corner: a peer is stale.
    pub dot: bool,
}

impl Face {
    pub fn counts(total: usize, attention: usize, stale: bool) -> Self {
        Self {
            total: count_text(total),
            attention: count_text(attention),
            highlighted: attention > 0,
            dot: stale,
        }
    }

    /// No trustworthy snapshot: unknown counters.
    pub fn error() -> Self {
        Self { total: "?".to_owned(), attention: "?".to_owned(), highlighted: false, dot: true }
    }
}

/// What a running session's entry does: Qt's "Open in" submenu with the
/// macOS set of `AppConfig::openModes` (Terminal.app or the clipboard).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenMode {
    #[default]
    Terminal,
    Clipboard,
}

impl OpenMode {
    pub const ALL: [OpenMode; 2] = [OpenMode::Terminal, OpenMode::Clipboard];

    /// Qt's labels (`TrayMenu.cpp:78-81`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal.app",
            Self::Clipboard => "Clipboard",
        }
    }

    /// An `open-mode` value of `~/.config/hgs/tray.conf`; the Linux and
    /// Windows values are not macOS modes.
    pub fn from_config(value: &str) -> Option<Self> {
        match value.trim() {
            "terminal" => Some(Self::Terminal),
            "clipboard" => Some(Self::Clipboard),
            _ => None,
        }
    }
}

/// The value of `key` in a `tray.conf` text: `key=value` lines, `#` comments,
/// the last line winning (`AppConfig.cpp:349-380`).
pub fn tray_conf_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

/// One menu entry, in Qt's order (`TrayMenu::rebuild`, `TrayMenu.cpp:156-246`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// "Open Zerus".
    Open,
    Separator,
    /// A line that cannot be chosen: a computer's header or its state.
    Label(String),
    /// A session; `text` is what the entry shows, with `&` doubled.
    Session { computer: ComputerId, key: String, name: String, text: String, running: bool },
    /// The "Open in" submenu with the chosen mode checked.
    OpenIn(OpenMode),
    Quit,
}

/// Doubles `&`: menus read a single one as a mnemonic marker (`TrayMenu.cpp:18-21`).
pub fn escape(text: &str) -> String {
    text.replace('&', "&&")
}

/// The text Qt shows for a session (`TrayMenu.cpp:23-31`).
pub fn session_text(row: &ViewRow) -> String {
    let label = escape(&row.name);
    match row.state.as_str() {
        "paused" => format!("{label}  (paused)"),
        "stopped" => format!("{label}  (stopped)"),
        _ if row.notify.attached => format!("\u{25cf} {label}"),
        _ => label,
    }
}

/// The fleet as the menu bar sees it, kept from the core's changes.
#[derive(Clone, Debug, Default)]
pub struct Desk {
    computers: BTreeMap<ComputerId, Computer>,
}

impl Desk {
    /// Applies one change; returns the computer whose state it may change.
    pub fn observe(&mut self, change: &Change) -> Option<ComputerId> {
        match change {
            Change::Upsert { computer, key, row } => {
                self.entry(computer).rows.insert(key.clone(), row.clone());
            }
            Change::Remove { computer, key, .. } => {
                self.entry(computer).rows.remove(key);
            }
            Change::Replace { computer, rows } => {
                let entry = self.entry(computer);
                entry.rows = rows.iter().cloned().collect();
                entry.listed = true;
            }
            Change::Computer { computer, status: ComputerStatus::Ended } => {
                self.computers.remove(computer)?;
            }
            Change::Computer { computer, status } => {
                self.entry(computer).status = *status;
            }
            Change::Machine { computer, machine } => {
                let entry = self.entry(computer);
                entry.host.clone_from(&machine.host);
                entry.local_sessions = machine.local_sessions.unwrap_or(true);
            }
            _ => return None,
        }
        Some(change.computer().clone())
    }

    pub fn computers(&self) -> &BTreeMap<ComputerId, Computer> {
        &self.computers
    }

    fn entry(&mut self, computer: &ComputerId) -> &mut Computer {
        self.computers.entry(computer.clone()).or_default()
    }

    fn local(&self) -> Option<&Computer> {
        self.computers.get(&ComputerId::local())
    }

    fn peers(&self) -> impl Iterator<Item = (&str, &Computer)> {
        self.computers.iter().filter_map(|(id, computer)| Some((id.alias()?, computer)))
    }

    /// Why the counters are unknown: this computer's stream failed. Qt shows
    /// "?" after a failed `hgs` call (`TrayAgent.cpp:507-517`).
    pub fn error(&self) -> Option<&'static str> {
        matches!(self.local()?.status, ComputerStatus::Reconnecting { .. })
            .then_some("reconnecting to hgs on this computer")
    }

    pub fn total(&self) -> usize {
        self.computers.values().filter(|c| c.listed).map(Computer::visible).sum()
    }

    pub fn attention(&self) -> usize {
        self.computers
            .values()
            .filter(|c| c.fresh())
            .flat_map(|c| c.rows.values())
            .filter(|row| !row.muted && needs_attention(row))
            .count()
    }

    /// A peer has not answered yet or is not current (`FleetState::isStale`);
    /// this computer is never stale.
    pub fn stale(&self) -> bool {
        self.peers().any(|(_, computer)| !computer.fresh())
    }

    pub fn face(&self) -> Face {
        if self.error().is_some() {
            return Face::error();
        }
        Face::counts(self.total(), self.attention(), self.stale())
    }

    /// The Dock badge: the exact attention count, none at zero or while the
    /// counters are unknown (`TrayAgent.cpp:540-541`, `MacApplicationBadge.mm`).
    pub fn badge(&self) -> Option<String> {
        let count = if self.error().is_some() { 0 } else { self.attention() };
        (count > 0).then(|| count.to_string())
    }

    /// Qt's tooltip; a stream has no poll age to show.
    pub fn tooltip(&self) -> String {
        if let Some(error) = self.error() {
            return format!("Zerus: {error}");
        }
        let (attention, total) = (self.attention(), self.total());
        let mut lines = vec![format!(
            "{}{}",
            if attention == 1 { "1 needs attention".to_owned() } else { format!("{attention} need attention") },
            if total == 1 { ", 1 session total".to_owned() } else { format!(", {total} sessions total") },
        )];
        let local = self.local();
        if local.is_none_or(|computer| computer.local_sessions) {
            let here = local.filter(|computer| computer.listed).map_or(0, Computer::visible);
            lines.push(format!("{here} here"));
        }
        for (alias, computer) in self.peers() {
            lines.push(if !computer.listed {
                format!("{alias}: not polled yet")
            } else if !computer.fresh() {
                format!("{alias}: unreachable")
            } else {
                format!("{} on {alias}", computer.visible())
            });
        }
        lines.join("\n")
    }

    /// The menu Qt builds, without Refresh and the update entries.
    pub fn menu(&self, mode: OpenMode) -> Vec<Entry> {
        let mut menu = vec![Entry::Open, Entry::Separator];
        let local = self.local();
        let local_section = local.is_none_or(|computer| computer.local_sessions);
        if local_section {
            let header = local
                .and_then(|computer| computer.host.clone())
                .filter(|host| !host.is_empty())
                .unwrap_or_else(|| "this box".to_owned());
            menu.push(Entry::Label(escape(&header)));
            match local.filter(|computer| computer.listed) {
                None => menu.push(Entry::Label("polling\u{2026}".to_owned())),
                Some(computer) => sessions(&mut menu, &ComputerId::local(), computer),
            }
        }
        let peers: Vec<(&str, &Computer)> = self.peers().collect();
        if !local_section && peers.is_empty() {
            menu.push(Entry::Label("No remote machines".to_owned()));
        }
        for (index, (alias, computer)) in peers.into_iter().enumerate() {
            if local_section || index > 0 {
                menu.push(Entry::Separator);
            }
            let alias_text = escape(alias);
            menu.push(Entry::Label(alias_text.clone()));
            if !computer.listed {
                menu.push(Entry::Label(format!("{alias_text}: polling\u{2026}")));
            } else if !computer.fresh() {
                menu.push(Entry::Label(format!("{alias_text}: unreachable")));
            } else {
                sessions(&mut menu, &ComputerId::from(alias), computer);
            }
        }
        menu.extend([Entry::Separator, Entry::OpenIn(mode), Entry::Quit]);
        menu
    }
}

/// A computer's sessions by name, or "no sessions" (`TrayMenu.cpp:178-195`).
fn sessions(menu: &mut Vec<Entry>, computer: &ComputerId, state: &Computer) {
    let mut rows: Vec<(&String, &ViewRow)> =
        state.rows.iter().filter(|(_, row)| row.state != "archived").collect();
    if rows.is_empty() {
        menu.push(Entry::Label("no sessions".to_owned()));
        return;
    }
    rows.sort_by(|a, b| a.1.name.cmp(&b.1.name).then_with(|| a.0.cmp(b.0)));
    for (key, row) in rows {
        menu.push(Entry::Session {
            computer: computer.clone(),
            key: key.clone(),
            name: row.name.clone(),
            text: session_text(row),
            running: row.state == "running",
        });
    }
}

/// One change to the status item.
#[derive(Clone, Debug, PartialEq)]
pub enum Apply {
    Face(Face),
    Tooltip(String),
    Menu(Vec<Entry>),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Look {
    face: Option<Face>,
    tooltip: Option<String>,
    menu: Option<Vec<Entry>>,
}

/// What reaches the status item: only changes, and nothing while its menu is
/// open, since an update then can deadlock AppKit on macOS 26
/// (`QuietTrayIcon.h`). The latest offer applies when the menu closes.
#[derive(Debug, Default)]
pub struct Gate {
    open: bool,
    shown: Look,
    wanted: Look,
}

impl Gate {
    pub fn offer(&mut self, face: Face, tooltip: String, menu: Vec<Entry>) -> Vec<Apply> {
        self.wanted = Look { face: Some(face), tooltip: Some(tooltip), menu: Some(menu) };
        if self.open {
            Vec::new()
        } else {
            self.flush()
        }
    }

    pub fn menu_opened(&mut self) {
        self.open = true;
    }

    pub fn menu_closed(&mut self) -> Vec<Apply> {
        self.open = false;
        self.flush()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn flush(&mut self) -> Vec<Apply> {
        let mut apply = Vec::new();
        if self.wanted.face != self.shown.face {
            apply.extend(self.wanted.face.clone().map(Apply::Face));
        }
        if self.wanted.tooltip != self.shown.tooltip {
            apply.extend(self.wanted.tooltip.clone().map(Apply::Tooltip));
        }
        if self.wanted.menu != self.shown.menu {
            apply.extend(self.wanted.menu.clone().map(Apply::Menu));
        }
        self.shown = self.wanted.clone();
        apply
    }
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml menubar`
Expected: PASS (14 tests).

- [ ] **Step 9: Run the task's checks**

Run core fmt and clippy, the src-tauri clippy with and without default features (the new `ViewRow` field must not break src-tauri), `python3 scripts/check-ui-separators.py` and `python3 scripts/check-source-privacy.py`.
Expected: all clean.

- [ ] **Step 10: Commit**

```bash
git add client/core/src/view.rs client/core/src/menubar.rs client/core/src/lib.rs
git commit -m "feat(client): menu-bar model with Qt's counters, tooltip, menu and update gate"
```

---

### Task 2: Command line, build identity, bundle identity and single instance (zerus-5wmh.44)

The start-up contract with the LaunchAgent and the updater. Qt's command line: `--hgs PATH`, `--sessions` and `--background` (`tray/src/AppConfig.cpp:200-320`, `:461-464`). `--build-info` prints Qt's JSON (`tray/src/UpdateController.cpp:118-136`, `tray/cmake/BuildVersion.cmake`). The single-instance protocol is `tray/src/SingleInstance.cpp` with `main.cpp:337-361` and `:424-433`. The data directory follows Decision 5.

**Files:**
- Create: `client/src-tauri/build_identity.rs`
- Modify: `client/src-tauri/build.rs`
- Create: `client/src-tauri/src/cli.rs`, `client/src-tauri/src/identity.rs`, `client/src-tauri/src/instance.rs`
- Modify: `client/src-tauri/src/lib.rs` (modules, `run`, `Startup`, `launch`)
- Modify: `client/src-tauri/src/platform/macos.rs` (add `bundle_identifier`)
- Modify: `client/src-tauri/Cargo.toml` (objc2-foundation feature `NSBundle`)
- Modify: `client/README.md` (the read-marks path)
- Test: unit tests in `cli.rs`, `identity.rs`, `instance.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `cli::{parse(&[String]) -> Result<Command, String>, Command { Run(Options), Help, Version, BuildInfo }, Options { hgs, sessions, background, ignored }, Options::opens_sessions(switch: bool) -> bool, usage() -> &'static str, BuildFacts { version, dev_base, dev_number, source_commit, source_dirty, preview_label }, BUILD, build_info(&BuildFacts) -> serde_json::Value, version_line(&BuildFacts) -> String}`. `build_info` adds Qt's `previewLabel` for a local preview build; Task 7's Dev guard reads it.
  - `build_identity::{release_version, check_dev_base, dev_number, source_commit, check_numbered, preview_label, check_preview}` (shared by `build.rs` and the tests).
  - `identity::{SWITCH_ID, LAUNCHD_LABEL, Platform { Mac, Linux, Windows }, Platform::current(), Identity { bundle_id }, Identity::{new, is_switch, instance_name(requested) -> Option<String>, launchd_label() -> Option<&'static str>, data_dir(platform, home, xdg_data_home, appdata) -> PathBuf, data_dir_now() -> PathBuf}, current(config_identifier) -> Identity, flag(name) -> bool}`.
  - `instance::{Activation { Show, Sessions }, Paths, paths, temp_dir, runtime_dir, LockInfo, is_stale, try_lock, LockAttempt, Service, ServiceState, launchd_service, waits_for_service, Timing, Start, Outcome, start, forward, Listening, Claimed { listening, activations: Receiver<Activation> }, Claim, claim(name, label, activation) -> Claim, claim_at(..), InstanceState(Mutex<Option<Listening>>)}` (Linux and macOS only).
  - `lib::Startup { identity, instance }`; `lib::run()` keeps its name, so `main.rs` stays unchanged. The app state holds the `identity::Identity`, the one source of the client's data directory (Decision 5).

- [ ] **Step 1: Write the failing tests for the command line and the build identity**

Create `client/src-tauri/src/cli.rs` holding only this test module, add `mod cli;` to `lib.rs`, and create `client/src-tauri/build_identity.rs` as an empty file:

```rust
#[cfg(test)]
#[path = "../build_identity.rs"]
mod build_identity;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_launch_agent_command_line_runs() {
        assert_eq!(parse(&args(&[])), Ok(Command::Run(Options::default())));
        assert_eq!(
            parse(&args(&["--hgs", "/Users/example/.local/bin/hgs"])),
            Ok(Command::Run(Options { hgs: Some("/Users/example/.local/bin/hgs".into()), ..Options::default() }))
        );
        assert_eq!(
            parse(&args(&["--sessions", "-psn_0_12345"])),
            Ok(Command::Run(Options { sessions: true, ..Options::default() }))
        );
    }

    #[test]
    fn qt_options_the_client_does_not_use_are_accepted() {
        assert_eq!(
            parse(&args(&["--local-poll", "2000", "--background", "--open-mode", "terminal"])),
            Ok(Command::Run(Options {
                background: true,
                ignored: vec!["--local-poll".into(), "--open-mode".into()],
                ..Options::default()
            }))
        );
    }

    #[test]
    fn identity_and_help_answer_before_anything_starts() {
        assert_eq!(parse(&args(&["--hgs", "x", "--build-info"])), Ok(Command::BuildInfo));
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["-h"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
    }

    #[test]
    fn mistakes_are_refused() {
        assert_eq!(parse(&args(&["--hgs"])), Err("--hgs needs a value".to_owned()));
        assert_eq!(parse(&args(&["--frobnicate"])), Err("unknown option --frobnicate".to_owned()));
        assert_eq!(parse(&args(&["--selftest"])), Err("--selftest is not available in this client".to_owned()));
    }

    #[test]
    fn the_switch_bundle_starts_in_the_menu_bar_like_qt() {
        let plain = Options::default();
        assert!(!plain.opens_sessions(true), "Qt on macOS opens the window only with --sessions");
        assert!(plain.opens_sessions(false), "other builds keep opening their window");
        assert!(Options { sessions: true, ..Options::default() }.opens_sessions(true));
        assert!(!Options { background: true, ..Options::default() }.opens_sessions(false));
    }

    #[test]
    fn build_info_has_qt_shape() {
        let commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let dev = BuildFacts {
            version: "0.47.0", dev_base: "0.47.1", dev_number: Some("1413"),
            source_commit: Some(commit), source_dirty: Some("0"), preview_label: None,
        };
        assert_eq!(build_info(&dev), serde_json::json!({
            "version": "0.47.0", "versionName": "0.47.1-dev.1413", "devBuildNumber": 1413,
            "sourceCommit": commit, "sourceDirty": false,
        }));
        assert_eq!(version_line(&dev), "Zerus 0.47.1-dev.1413\n");
        let local = BuildFacts {
            version: "0.47.0", dev_base: "0.47.1", dev_number: None, source_commit: None, source_dirty: None, preview_label: None,
        };
        assert_eq!(build_info(&local), serde_json::json!({
            "version": "0.47.0", "versionName": "0.47.0", "devBuildNumber": 0,
            "sourceCommit": "", "sourceDirty": null,
        }));
        // A local branch preview (`UpdateController.cpp:130-134`): the label and its own version name.
        let preview = BuildFacts { dev_number: None, preview_label: Some("task-board"), ..dev };
        assert_eq!(build_info(&preview)["versionName"], "0.47.1-preview.task-board.aaaaaaa");
        assert_eq!(build_info(&preview)["previewLabel"], "task-board");
        assert_eq!(build_info(&preview)["devBuildNumber"], 0);
    }

    #[test]
    fn the_embedded_version_is_the_repository_version() {
        assert_eq!(BUILD.version, include_str!("../../../VERSION").trim());
        assert_eq!(BUILD.dev_base, include_str!("../../../DEV_VERSION").trim());
    }

    #[test]
    fn build_identity_follows_the_cmake_rules() {
        use build_identity::*;
        assert_eq!(release_version(" 0.47.0\n").as_deref(), Some("0.47.0"));
        assert_eq!(release_version("0.47"), None);
        assert_eq!(release_version("0.047.0"), None);
        assert!(check_dev_base("0.47.0", "0.47.1").is_ok());
        assert!(check_dev_base("0.47.0", "0.47.0").is_err());
        assert!(check_dev_base("0.47.0", "0.9.9").is_err());
        assert_eq!(dev_number(""), Ok(None));
        assert_eq!(dev_number("1413"), Ok(Some(1413)));
        for bad in ["0", "012", "-1", "1e3", "1234567890123456"] {
            assert!(dev_number(bad).is_err(), "{bad}");
        }
        let commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(source_commit(commit, "/src/example", "/src/example").as_deref(), Some(commit));
        assert_eq!(source_commit(commit, "/src/parent", "/src/example"), None, "a parent repository's commit is not ours");
        assert_eq!(source_commit("ABC", "/src/example", "/src/example"), None);
        assert!(check_numbered(Some(1), Some(commit), Some(false)).is_ok());
        assert!(check_numbered(Some(1), Some(commit), Some(true)).is_err());
        assert!(check_numbered(Some(1), None, None).is_err());
        assert!(check_numbered(None, None, None).is_ok());
        // `ZERUS_LOCAL_PREVIEW_LABEL` (`BuildVersion.cmake:9-18`).
        assert_eq!(preview_label(""), Ok(None));
        assert_eq!(preview_label("task-board"), Ok(Some("task-board".to_owned())));
        for bad in ["Task", "task--board", "-task", "task-", "task_board", &"a".repeat(33)] {
            assert!(preview_label(bad).is_err(), "{bad}");
        }
        assert!(check_preview(Some("task-board"), None, Some(commit), Some(false)).is_ok());
        assert!(check_preview(Some("task-board"), Some(7), Some(commit), Some(false)).is_err(), "never with a Dev number");
        assert!(check_preview(Some("task-board"), None, Some(commit), Some(true)).is_err(), "never from local changes");
        assert!(check_preview(None, None, None, None).is_ok());
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml cli::`
Expected: FAIL to compile: `parse`, `Options`, `Command`, `BuildFacts`, `build_info` and the `build_identity` functions are missing.

`commit` in these tests is forty `a` characters: a valid hex commit that no secret scanner mistakes for a key.

- [ ] **Step 3: Write `build_identity.rs`**

```rust
//! The build's identity, shared by `build.rs` and the crate's tests: Qt's
//! rules in `tray/cmake/BuildVersion.cmake` and `tray/CMakeLists.txt:121-139`.

/// `MAJOR.MINOR.PATCH` without leading zeros, as `VERSION` and `DEV_VERSION` hold it.
pub fn release_version(raw: &str) -> Option<String> {
    let value = raw.trim();
    let parts: Vec<&str> = value.split('.').collect();
    let valid = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        });
    valid.then(|| value.to_owned())
}

fn numbers(version: &str) -> Vec<u64> {
    version.split('.').filter_map(|part| part.parse().ok()).collect()
}

/// `DEV_VERSION` names a release newer than `VERSION` (`BuildVersion.cmake:4-7`).
pub fn check_dev_base(version: &str, dev_base: &str) -> Result<(), String> {
    if numbers(dev_base) > numbers(version) {
        Ok(())
    } else {
        Err("DEV_VERSION must name a release newer than the stable product version".to_owned())
    }
}

/// `ZERUS_DEV_BUILD_NUMBER`: empty, or a positive integer of at most 15 digits.
pub fn dev_number(raw: &str) -> Result<Option<u64>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let valid = raw.len() <= 15 && raw.bytes().all(|byte| byte.is_ascii_digit()) && !raw.starts_with('0');
    if !valid {
        return Err("ZERUS_DEV_BUILD_NUMBER must be a positive integer of at most 15 digits".to_owned());
    }
    raw.parse().map(Some).map_err(|error| format!("ZERUS_DEV_BUILD_NUMBER: {error}"))
}

/// The source commit: a 40-character lowercase hex HEAD, only when Git's top
/// level is this repository (not a parent checkout).
pub fn source_commit(head: &str, toplevel: &str, root: &str) -> Option<String> {
    let head = head.trim();
    let valid = head.len() == 40 && head.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    (valid && toplevel.trim() == root).then(|| head.to_owned())
}

/// A numbered Dev build needs a clean source tree (`BuildVersion.cmake:28-30`).
pub fn check_numbered(number: Option<u64>, commit: Option<&str>, dirty: Option<bool>) -> Result<(), String> {
    if number.is_some() && !(commit.is_some() && dirty == Some(false)) {
        Err("A numbered Dev build requires a clean Git source tree".to_owned())
    } else {
        Ok(())
    }
}

/// `ZERUS_LOCAL_PREVIEW_LABEL`: empty, or at most 32 lowercase letters and
/// digits in hyphen-separated words (`BuildVersion.cmake:9-14`).
pub fn preview_label(raw: &str) -> Result<Option<String>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    let valid = raw.len() <= 32
        && raw.split('-').all(|word| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    if valid {
        Ok(Some(raw.to_owned()))
    } else {
        Err("A local preview label must contain at most 32 lowercase letters, digits or separated hyphens".to_owned())
    }
}

/// A local branch preview has no Dev number and a clean source tree (`BuildVersion.cmake:15-17`).
pub fn check_preview(label: Option<&str>, number: Option<u64>, commit: Option<&str>, dirty: Option<bool>) -> Result<(), String> {
    if label.is_some() && (number.is_some() || !(commit.is_some() && dirty == Some(false))) {
        Err("A local branch preview requires clean Git source and no Dev build number".to_owned())
    } else {
        Ok(())
    }
}
```

- [ ] **Step 4: Embed the identity in `build.rs`**

Replace `client/src-tauri/build.rs` with:

```rust
#[path = "build_identity.rs"]
mod build_identity;

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

fn canonical(path: &str) -> String {
    Path::new(path).canonicalize().map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into_owned())
}

/// `--build-info` and `--version` report Zerus' identity as the Qt GUI does:
/// the product version, the Dev number from `ZERUS_DEV_BUILD_NUMBER`, a local
/// preview label from `ZERUS_LOCAL_PREVIEW_LABEL` and the clean source commit
/// (`tray/cmake/BuildVersion.cmake`).
fn embed_identity() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..").canonicalize().expect("the repository root");
    let read = |name: &str| {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
        std::fs::read_to_string(root.join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let version = build_identity::release_version(&read("VERSION")).expect("VERSION names MAJOR.MINOR.PATCH");
    let dev_base = build_identity::release_version(&read("DEV_VERSION")).expect("DEV_VERSION names MAJOR.MINOR.PATCH");
    build_identity::check_dev_base(&version, &dev_base).unwrap_or_else(|error| panic!("{error}"));
    println!("cargo:rerun-if-env-changed=ZERUS_DEV_BUILD_NUMBER");
    let number = build_identity::dev_number(&std::env::var("ZERUS_DEV_BUILD_NUMBER").unwrap_or_default())
        .unwrap_or_else(|error| panic!("{error}"));
    println!("cargo:rerun-if-env-changed=ZERUS_LOCAL_PREVIEW_LABEL");
    let label = build_identity::preview_label(&std::env::var("ZERUS_LOCAL_PREVIEW_LABEL").unwrap_or_default())
        .unwrap_or_else(|error| panic!("{error}"));
    let root_text = root.to_string_lossy().into_owned();
    let commit = git(&root, &["rev-parse", "HEAD"])
        .zip(git(&root, &["rev-parse", "--show-toplevel"]))
        .and_then(|(head, top)| build_identity::source_commit(&head, &canonical(&top), &root_text));
    let dirty = commit
        .as_ref()
        .and_then(|_| git(&root, &["status", "--porcelain", "--untracked-files=normal"]))
        .map(|changes| !changes.is_empty());
    if let Some(dir) = git(&root, &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/index");
    }
    build_identity::check_numbered(number, commit.as_deref(), dirty).unwrap_or_else(|error| panic!("{error}"));
    build_identity::check_preview(label.as_deref(), number, commit.as_deref(), dirty).unwrap_or_else(|error| panic!("{error}"));
    if let Some(label) = &label {
        println!("cargo:rustc-env=ZERUS_LOCAL_PREVIEW_LABEL={label}");
    }
    println!("cargo:rustc-env=ZERUS_VERSION={version}");
    println!("cargo:rustc-env=ZERUS_DEV_BASE_VERSION={dev_base}");
    if let Some(number) = number {
        println!("cargo:rustc-env=ZERUS_DEV_BUILD_NUMBER={number}");
    }
    if let Some(commit) = &commit {
        println!("cargo:rustc-env=ZERUS_SOURCE_COMMIT={commit}");
        println!("cargo:rustc-env=ZERUS_SOURCE_DIRTY={}", u8::from(dirty == Some(true)));
    }
}

fn main() {
    embed_identity();
    tauri_build::build()
}
```

- [ ] **Step 5: Write `cli.rs` above its tests**

```rust
//! The command line of Qt's `hgs-tray` (`tray/src/AppConfig.cpp`), so the
//! LaunchAgent (`--hgs PATH`), the updater (`--build-info`) and launchers
//! (`--sessions`, `--background`) start this client unchanged.
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--hgs PATH`: the `hgs` to run; the LaunchAgent always passes it.
    pub hgs: Option<PathBuf>,
    /// `--sessions`: open the window, here or in the running instance.
    pub sessions: bool,
    /// `--background`: start in the menu bar only.
    pub background: bool,
    /// Qt options this client accepts and does not use.
    pub ignored: Vec<String>,
}

impl Options {
    /// Whether the window opens: the switch bundle follows Qt's
    /// `AppConfig::opensSessions` on macOS (only `--sessions`); other builds
    /// keep opening their window unless started with `--background`.
    pub fn opens_sessions(&self, switch: bool) -> bool {
        self.sessions || (!switch && !self.background)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
    BuildInfo,
}

/// Qt options with a value that this client does not use.
const IGNORED_WITH_VALUE: [&str; 5] = ["--local-poll", "--peer-poll", "--terminal", "--icon-color", "--open-mode"];

pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-h" | "--help" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            "--build-info" => return Ok(Command::BuildInfo),
            "--sessions" => options.sessions = true,
            "--background" => options.background = true,
            "--selftest" => return Err("--selftest is not available in this client".to_owned()),
            "--hgs" => {
                let value = args.get(index + 1).ok_or_else(|| format!("{arg} needs a value"))?;
                options.hgs = Some(PathBuf::from(value));
                index += 1;
            }
            _ if IGNORED_WITH_VALUE.contains(&arg) => {
                args.get(index + 1).ok_or_else(|| format!("{arg} needs a value"))?;
                options.ignored.push(arg.to_owned());
                index += 1;
            }
            // Older macOS passes a process serial number to apps it opens.
            _ if arg.starts_with("-psn_") => {}
            _ => return Err(format!("unknown option {arg}")),
        }
        index += 1;
    }
    Ok(Command::Run(options))
}

pub fn usage() -> &'static str {
    "Usage: hgs-tray [options]\n\n\
     \x20 --hgs <path>          path to the hgs binary; found in PATH by default\n\
     \x20 --sessions            open the Zerus window\n\
     \x20 --background          start in the menu bar only (the default for Zerus)\n\
     \x20 -h, --help            this text\n\
     \x20 --version             installed release version and exit\n\
     \x20 --build-info          embedded build/source identity as JSON and exit\n\n\
     Accepted for the Qt GUI's command line and not used: --local-poll, --peer-poll,\n\
     --terminal, --icon-color and --open-mode (each takes a value).\n"
}

/// What `build.rs` embedded.
pub struct BuildFacts {
    pub version: &'static str,
    pub dev_base: &'static str,
    pub dev_number: Option<&'static str>,
    pub source_commit: Option<&'static str>,
    pub source_dirty: Option<&'static str>,
    pub preview_label: Option<&'static str>,
}

pub const BUILD: BuildFacts = BuildFacts {
    version: env!("ZERUS_VERSION"),
    dev_base: env!("ZERUS_DEV_BASE_VERSION"),
    dev_number: option_env!("ZERUS_DEV_BUILD_NUMBER"),
    source_commit: option_env!("ZERUS_SOURCE_COMMIT"),
    source_dirty: option_env!("ZERUS_SOURCE_DIRTY"),
    preview_label: option_env!("ZERUS_LOCAL_PREVIEW_LABEL"),
};

/// Qt's `UpdateController::buildInfo` (`tray/src/UpdateController.cpp:118-136`).
pub fn build_info(facts: &BuildFacts) -> Value {
    let number: u64 = facts.dev_number.and_then(|value| value.parse().ok()).unwrap_or(0);
    let commit = facts.source_commit.unwrap_or("");
    let version_name = match facts.preview_label {
        Some(label) => format!("{}-preview.{label}.{}", facts.dev_base, commit.get(..7).unwrap_or(commit)),
        None if number > 0 => format!("{}-dev.{number}", facts.dev_base),
        None => facts.version.to_owned(),
    };
    let mut info = json!({
        "version": facts.version,
        "versionName": version_name,
        "devBuildNumber": number,
        "sourceCommit": commit,
        "sourceDirty": facts.source_commit.map(|_| facts.source_dirty == Some("1")),
    });
    if let Some(label) = facts.preview_label {
        info["previewLabel"] = Value::from(label);
    }
    info
}

/// `hgs-tray --version` (`main.cpp:196-200`).
pub fn version_line(facts: &BuildFacts) -> String {
    format!("Zerus {}\n", build_info(facts)["versionName"].as_str().unwrap_or(facts.version))
}
```

- [ ] **Step 6: Run the command-line tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml cli::`
Expected: PASS (8 tests).

- [ ] **Step 7: Write the failing identity and single-instance tests**

Create `client/src-tauri/src/identity.rs` with only this test module and add `mod identity;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_bundle_takes_qt_names_and_others_keep_their_own() {
        let switch = Identity::new("com.hgdev.hgs-tray");
        assert!(switch.is_switch());
        assert_eq!(switch.instance_name(false).as_deref(), Some("hgs-tray"));
        assert_eq!(switch.launchd_label(), Some("com.hgdev.hgs-tray"));
        for other in ["dev.zerus.next", "com.hgdev.hgs-tray.local"] {
            let identity = Identity::new(other);
            assert!(!identity.is_switch());
            assert_eq!(identity.instance_name(false), None);
            assert_eq!(identity.instance_name(true), Some(format!("zerus-client-{other}")));
            assert_eq!(identity.launchd_label(), None);
        }
    }

    #[test]
    fn the_data_directory_follows_the_running_identifier() {
        let mac = Path::new("/Users/example");
        assert_eq!(
            Identity::new("com.hgdev.hgs-tray").data_dir(Platform::Mac, mac, None, None),
            Path::new("/Users/example/Library/Application Support/com.hgdev.hgs-tray"),
            "never Qt's Application Support/hgdev/hgs-tray"
        );
        let linux = Path::new("/home/example");
        let dev = Identity::new("dev.zerus.next");
        assert_eq!(dev.data_dir(Platform::Linux, linux, None, None), Path::new("/home/example/.local/share/dev.zerus.next"));
        assert_eq!(
            dev.data_dir(Platform::Linux, linux, Some(Path::new("/data/example")), None),
            Path::new("/data/example/dev.zerus.next")
        );
        assert_eq!(
            dev.data_dir(Platform::Windows, Path::new("C:/Users/example"), None, Some(Path::new("C:/Users/example/AppData/Roaming"))),
            Path::new("C:/Users/example/AppData/Roaming/dev.zerus.next")
        );
    }
}
```

Create `client/src-tauri/src/instance.rs` with only this test module, and add `#[cfg(any(target_os = "linux", target_os = "macos"))] mod instance;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zci-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fast() -> Timing {
        Timing { lock_wait: Duration::from_millis(300), service_wait: Duration::from_millis(400), poll: Duration::from_millis(20) }
    }

    #[test]
    fn activation_lines_are_qt_lines() {
        assert_eq!(Activation::Sessions.line(), b"sessions\n");
        assert_eq!(Activation::Show.line(), b"show\n");
        assert_eq!(Activation::parse(b" sessions\r"), Some(Activation::Sessions));
        assert_eq!(Activation::parse(b"show"), Some(Activation::Show));
        assert_eq!(Activation::parse(b"quit"), None);
    }

    #[test]
    fn paths_use_qt_names() {
        let p = paths("hgs-tray", Path::new("/private/var/folders/x/T"), Path::new("/Users/example/Library/Application Support"));
        assert_eq!(p.socket, Path::new("/private/var/folders/x/T/hgs-tray"));
        assert_eq!(p.lock, Path::new("/Users/example/Library/Application Support/hgs-tray-start.lock"));
        assert_eq!(
            runtime_dir(Platform::Mac, Path::new("/Users/example"), None),
            Path::new("/Users/example/Library/Application Support")
        );
        assert_eq!(
            runtime_dir(Platform::Linux, Path::new("/home/example"), Some(Path::new("/run/user/1000"))),
            Path::new("/run/user/1000")
        );
    }

    #[test]
    fn lock_staleness_follows_qlockfile() {
        let info = LockInfo { pid: 42, app: "hgs-tray".into(), host: "mac".into() };
        let young = Duration::from_secs(1);
        assert!(is_stale(Some(&info), young, "mac", |_| None), "the holder exited");
        assert!(is_stale(Some(&info), young, "mac", |_| Some(Some("Safari".into()))), "its PID runs another program");
        assert!(!is_stale(Some(&info), young, "mac", |_| Some(Some("hgs-tray".into()))));
        assert!(!is_stale(Some(&info), young, "mac", |_| Some(None)));
        assert!(!is_stale(Some(&info), young, "other", |_| None), "another host's holder is not judged by PID");
        assert!(is_stale(Some(&info), Duration::from_secs(31), "mac", |_| Some(Some("hgs-tray".into()))), "QLockFile's 30 s");
        assert!(is_stale(None, Duration::from_secs(31), "mac", |_| None));
        assert!(!is_stale(None, young, "mac", |_| None));
        assert_eq!(LockInfo::parse("42\nhgs-tray\nmac\nmachine-id\nboot-id\n"), Some(info.clone()));
        assert_eq!(LockInfo::parse(&info.contents()), Some(info));
        assert_eq!(LockInfo::parse("x\n"), None);
    }

    #[test]
    fn launchd_print_reads_only_the_jobs_own_state() {
        let text = |state: &str, pid: &str| {
            let state = if state.is_empty() { String::new() } else { format!("\tstate = {state}\n") };
            let pid = if pid.is_empty() { String::new() } else { format!("\tpid = {pid}\n") };
            format!(
                "gui/501/com.hgdev.hgs-tray = {{\n\tactive count = 1\n\tpath = /Users/example/Library/LaunchAgents/com.hgdev.hgs-tray.plist\n\ttype = LaunchAgent\n{state}\n\tprogram = /Applications/Zerus.app/Contents/MacOS/hgs-tray\n\tevent triggers = {{\n\t\tcom.example.trigger => {{\n\t\t\tstate = running\n\t\t\tpid = 7\n\t\t}}\n\t}}\n{pid}\tlast exit code = (never exited)\n}}\n"
            )
        };
        assert_eq!(launchd_service(&text("running", "4242")), Service { state: ServiceState::Running, pid: 4242 });
        assert_eq!(launchd_service(&text("spawn scheduled", "")), Service { state: ServiceState::Starting, pid: 0 });
        assert_eq!(launchd_service(&text("not running", "")), Service::default());
        assert_eq!(launchd_service(&text("", "")), Service::default());
        assert_eq!(launchd_service(""), Service::default());
    }

    #[test]
    fn only_another_service_process_keeps_the_socket() {
        assert!(!waits_for_service(&Service::default(), 100));
        assert!(!waits_for_service(&Service { state: ServiceState::Running, pid: 100 }, 100));
        assert!(waits_for_service(&Service { state: ServiceState::Running, pid: 200 }, 100));
        assert!(waits_for_service(&Service { state: ServiceState::Starting, pid: 0 }, 100));
        assert!(waits_for_service(&Service { state: ServiceState::Starting, pid: 200 }, 100));
    }

    /// Scripted effects for `start`.
    #[derive(Default)]
    struct Script {
        lock_error: Option<String>,
        forwards: VecDeque<bool>,
        services: VecDeque<Service>,
        claim_error: Option<String>,
        elapsed: Duration,
        locked: bool,
        log: Vec<String>,
    }

    impl Start for Script {
        fn lock(&mut self, _wait: Duration) -> Result<(), String> {
            self.log.push("lock".into());
            match &self.lock_error {
                Some(error) => Err(error.clone()),
                None => {
                    self.locked = true;
                    Ok(())
                }
            }
        }
        fn unlock(&mut self) {
            if self.locked {
                self.log.push("unlock".into());
            }
            self.locked = false;
        }
        fn forward(&mut self, activation: Activation) -> bool {
            let reached = self.forwards.pop_front().unwrap_or(false);
            self.log.push(format!("forward {activation:?} {reached}"));
            reached
        }
        fn service(&mut self) -> Service {
            self.services.pop_front().unwrap_or_default()
        }
        fn claim(&mut self) -> io::Result<()> {
            self.log.push("claim".into());
            match &self.claim_error {
                Some(error) => Err(io::Error::other(error.clone())),
                None => Ok(()),
            }
        }
        fn pause(&mut self, pause: Duration) {
            self.elapsed += pause;
        }
        fn elapsed(&self) -> Duration {
            self.elapsed
        }
        fn report(&mut self, line: &str) {
            self.log.push(format!("report {line}"));
        }
    }

    #[test]
    fn a_start_waits_for_the_starting_service_then_forwards() {
        let mut script = Script {
            forwards: [false, false, true].into(),
            services: [Service { state: ServiceState::Starting, pid: 0 }, Service { state: ServiceState::Running, pid: 500 }].into(),
            ..Script::default()
        };
        assert_eq!(start(&mut script, Activation::Sessions, 100, &fast()), Outcome::Forwarded);
        assert!(!script.log.iter().any(|line| line == "claim"), "{:?}", script.log);
        assert_eq!(script.log.iter().filter(|line| line.starts_with("report waiting")).count(), 1, "reported once");
        assert!(!script.locked, "the lock is released");
    }

    #[test]
    fn the_services_own_process_claims_at_once() {
        let mut script = Script { services: [Service { state: ServiceState::Running, pid: 100 }].into(), ..Script::default() };
        assert_eq!(start(&mut script, Activation::Show, 100, &fast()), Outcome::Claimed);
        assert_eq!(script.log, ["lock", "forward Show false", "claim", "unlock"]);
    }

    #[test]
    fn a_service_that_never_listens_is_given_up_after_the_wait() {
        let mut script = Script {
            services: std::iter::repeat_n(Service { state: ServiceState::Running, pid: 500 }, 100).collect(),
            ..Script::default()
        };
        assert_eq!(start(&mut script, Activation::Show, 100, &fast()), Outcome::Claimed);
        assert!(
            script.log.iter().any(|line| line == "report the Zerus service did not open its socket within 0.4 s; starting here"),
            "{:?}", script.log
        );
    }

    #[test]
    fn a_lock_that_cannot_be_taken_is_reported_once_and_skipped() {
        let mut script = Script {
            lock_error: Some("another start keeps x locked; starting without it".into()),
            services: [Service { state: ServiceState::Starting, pid: 0 }].into(),
            forwards: [false, true].into(),
            ..Script::default()
        };
        assert_eq!(start(&mut script, Activation::Show, 100, &fast()), Outcome::Forwarded);
        assert_eq!(script.log.iter().filter(|line| *line == "lock").count(), 1);
    }

    #[test]
    fn a_failed_listen_is_reported() {
        let mut script = Script { claim_error: Some("address in use".into()), ..Script::default() };
        assert_eq!(start(&mut script, Activation::Show, 100, &fast()), Outcome::Failed("address in use".into()));
    }

    #[test]
    fn a_second_start_forwards_its_line_to_the_first() {
        let dir = scratch("forward");
        let paths = paths("zerus-test", &dir, &dir);
        let own = LockInfo::own();
        let Claim::Claimed(first) = claim_at(&paths, None, Activation::Show, &own, &fast()) else {
            panic!("the first start claims")
        };
        assert!(matches!(claim_at(&paths, None, Activation::Sessions, &own, &fast()), Claim::Forwarded));
        assert_eq!(first.activations.recv_timeout(Duration::from_secs(5)).unwrap(), Activation::Sessions);
        assert!(!paths.lock.exists(), "the start lock is released");
        drop(first.listening);
        assert!(!paths.socket.exists(), "quitting removes the socket");
    }

    #[test]
    fn a_crashed_instance_socket_is_claimed() {
        let dir = scratch("crashed");
        let paths = paths("zerus-test", &dir, &dir);
        drop(UnixListener::bind(&paths.socket).unwrap());
        assert!(paths.socket.exists(), "a socket file nobody listens on");
        let Claim::Claimed(claimed) = claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast()) else {
            panic!("a crashed instance's socket is replaced")
        };
        assert!(forward(&paths.socket, Activation::Sessions));
        assert_eq!(claimed.activations.recv_timeout(Duration::from_secs(5)).unwrap(), Activation::Sessions);
    }

    #[test]
    fn a_dead_holders_lock_is_taken() {
        let dir = scratch("dead");
        let lock = dir.join("zerus-test-start.lock");
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        let own = LockInfo::own();
        fs::write(&lock, LockInfo { pid, app: own.app.clone(), host: own.host.clone() }.contents()).unwrap();
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Taken(_)));
    }

    #[test]
    fn a_live_holders_lock_is_busy_until_released() {
        let dir = scratch("live");
        let lock = dir.join("zerus-test-start.lock");
        let own = LockInfo::own();
        let LockAttempt::Taken(held) = try_lock(&lock, &own) else { panic!("the first attempt takes it") };
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Busy));
        drop(held);
        assert!(!lock.exists());
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Taken(_)));
    }
}
```

- [ ] **Step 8: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- identity:: instance::`
Expected: FAIL to compile: `Identity`, `Platform`, `paths`, `LockInfo`, `start` and the rest are missing.

- [ ] **Step 9: Write `identity.rs` above its tests**

```rust
//! Who this build is. The switch bundle (`com.hgdev.hgs-tray`) takes over
//! Qt's single-instance names, LaunchAgent label and notifications; every
//! other build (the dev app `dev.zerus.next`, an unpromoted
//! `com.hgdev.hgs-tray.local` build) keeps its own names and stays quiet.
use std::path::{Path, PathBuf};

pub const SWITCH_ID: &str = "com.hgdev.hgs-tray";
/// Qt's LaunchAgent label (`tray/packaging/com.hgdev.hgs-tray.plist`).
pub const LAUNCHD_LABEL: &str = "com.hgdev.hgs-tray";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub bundle_id: String,
}

impl Identity {
    pub fn new(bundle_id: &str) -> Self {
        Self { bundle_id: bundle_id.to_owned() }
    }

    pub fn is_switch(&self) -> bool {
        self.bundle_id == SWITCH_ID
    }

    /// The single-instance name: Qt's `hgs-tray` for the switch bundle; any
    /// other build only on request (`ZERUS_CLIENT_SINGLE_INSTANCE=1`), under
    /// its own name, so parallel dev runs and Qt's socket stay apart.
    pub fn instance_name(&self, requested: bool) -> Option<String> {
        if self.is_switch() {
            Some("hgs-tray".to_owned())
        } else if requested {
            Some(format!("zerus-client-{}", self.bundle_id))
        } else {
            None
        }
    }

    /// The service a second start waits for (`SingleInstance::currentService`).
    pub fn launchd_label(&self) -> Option<&'static str> {
        self.is_switch().then_some(LAUNCHD_LABEL)
    }

    /// Tauri's `app_data_dir` formula, with this identifier.
    pub fn data_dir(&self, platform: Platform, home: &Path, xdg_data_home: Option<&Path>, appdata: Option<&Path>) -> PathBuf {
        match platform {
            Platform::Mac => home.join("Library/Application Support").join(&self.bundle_id),
            Platform::Linux => xdg_data_home
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/share"))
                .join(&self.bundle_id),
            Platform::Windows => appdata
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData/Roaming"))
                .join(&self.bundle_id),
        }
    }

    pub fn data_dir_now(&self) -> PathBuf {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = var("HOME").or_else(|| var("USERPROFILE")).unwrap_or_default();
        self.data_dir(Platform::current(), &home, var("XDG_DATA_HOME").as_deref(), var("APPDATA").as_deref())
    }
}

/// The running bundle's identifier on macOS, since the signer promotes a
/// `.local` build; the configuration's identifier elsewhere.
pub fn current(config_identifier: &str) -> Identity {
    #[cfg(target_os = "macos")]
    if let Some(id) = crate::platform::macos::bundle_identifier() {
        return Identity::new(&id);
    }
    Identity::new(config_identifier)
}

/// `NAME=1` in the environment.
pub fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}
```

Add to `client/src-tauri/src/platform/macos.rs`:

```rust
/// The running bundle's identifier; None outside a bundle (`cargo run`, tests).
pub fn bundle_identifier() -> Option<String> {
    objc2_foundation::NSBundle::mainBundle().bundleIdentifier().map(|id| id.to_string())
}
```

In `client/src-tauri/Cargo.toml`, add `"NSBundle"` to the `objc2-foundation` features of the macOS dependency table.

- [ ] **Step 10: Write `instance.rs` above its tests**

```rust
//! One client per user, by Qt's protocol (`tray/src/SingleInstance.{h,cpp}`,
//! `tray/src/main.cpp:337-361` and `:424-433`). Concurrent starts take a
//! per-user lock in turn; the running instance listens on a Unix socket and
//! reads one line, `sessions` or `show`; a start that reaches it forwards its
//! line and exits. The switch bundle uses Qt's names, so
//! `tray/update-installed.py` finds the socket (a path ending in `/hgs-tray`)
//! and sends `sessions` to the new build.
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::identity::Platform;

/// The line a start sends to the running instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Show,
    Sessions,
}

impl Activation {
    pub fn line(self) -> &'static [u8] {
        match self {
            Self::Show => b"show\n",
            Self::Sessions => b"sessions\n",
        }
    }

    /// Qt reads one line and trims it (`main.cpp:428`).
    pub fn parse(line: &[u8]) -> Option<Self> {
        match line.trim_ascii() {
            b"sessions" => Some(Self::Sessions),
            b"show" => Some(Self::Show),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub socket: PathBuf,
    pub lock: PathBuf,
}

/// QLocalServer puts a relative name in `QDir::tempPath()`; the start lock is
/// `QStandardPaths::RuntimeLocation` + `/<name>-start.lock` (`SingleInstance.cpp:215-218`).
pub fn paths(name: &str, temp: &Path, runtime: &Path) -> Paths {
    Paths { socket: temp.join(name), lock: runtime.join(format!("{name}-start.lock")) }
}

/// `QDir::tempPath()`: `TMPDIR`, else the per-user `confstr` folder on macOS
/// (Rust's `temp_dir` has the same fallback), canonicalized as Qt does.
pub fn temp_dir() -> PathBuf {
    let temp = std::env::temp_dir();
    temp.canonicalize().unwrap_or(temp)
}

/// `QStandardPaths::RuntimeLocation`: `~/Library/Application Support` on
/// macOS, `XDG_RUNTIME_DIR` on Linux.
pub fn runtime_dir(platform: Platform, home: &Path, xdg_runtime: Option<&Path>) -> PathBuf {
    match platform {
        Platform::Mac => home.join("Library/Application Support"),
        _ => xdg_runtime.map(Path::to_path_buf).unwrap_or_else(temp_dir),
    }
}

/// QLockFile's first three lines (`qlockfile_unix.cpp`): PID, process name, host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockInfo {
    pub pid: i64,
    pub app: String,
    pub host: String,
}

impl LockInfo {
    pub fn contents(&self) -> String {
        format!("{}\n{}\n{}\n", self.pid, self.app, self.host)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.split('\n');
        let pid = lines.next()?.trim().parse::<i64>().ok().filter(|pid| *pid > 0)?;
        Some(Self {
            pid,
            app: lines.next().unwrap_or("").to_owned(),
            host: lines.next().unwrap_or("").to_owned(),
        })
    }

    /// This process, named as QLockFile names it.
    pub fn own() -> Self {
        let pid = i64::from(std::process::id());
        Self {
            pid,
            app: process_name(pid).flatten().unwrap_or_else(|| "zerus-client".to_owned()),
            host: host_name(),
        }
    }
}

/// QLockFile's default stale time.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// QLockFile's `isApparentlyStale`: on this host, a holder that has exited
/// or whose PID now runs another program; on any host, a file older than
/// 30 s. `process` answers None for no such process, `Some(None)` for a
/// process whose name is unknown and `Some(Some(name))` otherwise.
pub fn is_stale(info: Option<&LockInfo>, age: Duration, host: &str, process: impl Fn(i64) -> Option<Option<String>>) -> bool {
    if let Some(info) = info {
        if info.host.is_empty() || info.host == host {
            let running = match process(info.pid) {
                None => false,
                Some(Some(name)) if !name.is_empty() => {
                    Path::new(&info.app).file_name().is_some_and(|app| app == name.as_str())
                }
                Some(_) => true,
            };
            if !running {
                return true;
            }
        }
    }
    age > STALE_AFTER
}

/// None when no process has this PID; its name when the system tells it.
pub fn process_name(pid: i64) -> Option<Option<String>> {
    let raw = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(raw, 0) } == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        return None;
    }
    #[cfg(target_os = "macos")]
    let name = {
        let mut buffer = [0u8; 256];
        // SAFETY: the buffer is writable for its length.
        let length = unsafe { libc::proc_name(raw, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        (length > 0).then(|| String::from_utf8_lossy(&buffer[..length as usize]).into_owned())
    };
    #[cfg(not(target_os = "macos"))]
    let name = fs::read_to_string(format!("/proc/{raw}/comm")).ok().map(|name| name.trim_end().to_owned());
    Some(name)
}

/// `QSysInfo::machineHostName()`.
pub fn host_name() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is writable for its length.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return String::new();
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// A taken start lock; dropping it removes the file, as `QLockFile::unlock` does.
pub struct StartLock {
    path: PathBuf,
    _file: File,
}

impl Drop for StartLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub enum LockAttempt {
    Taken(StartLock),
    Busy,
    Unavailable,
}

fn age(path: &Path) -> Duration {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .unwrap_or_default()
}

fn stale_now(path: &Path, host: &str) -> bool {
    let info = fs::read_to_string(path).ok().and_then(|text| LockInfo::parse(&text));
    is_stale(info.as_ref(), age(path), host, process_name)
}

/// Takes QLockFile's advisory lock; false when a live holder keeps it.
fn flock_now(file: &File) -> bool {
    // SAFETY: an advisory lock on an open descriptor.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// QLockFile guards a removal with `<lock>.rmlock` and removes only a file
/// that no live holder keeps locked.
fn remove_stale(path: &Path, host: &str) -> bool {
    let mut guard = OsString::from(path.as_os_str());
    guard.push(".rmlock");
    let guard = PathBuf::from(guard);
    if OpenOptions::new().write(true).create_new(true).mode(0o644).open(&guard).is_err() {
        if age(&guard) > STALE_AFTER {
            let _ = fs::remove_file(&guard);
        }
        return false;
    }
    let removed = stale_now(path, host)
        && OpenOptions::new().write(true).open(path).is_ok_and(|file| flock_now(&file))
        && fs::remove_file(path).is_ok();
    let _ = fs::remove_file(&guard);
    removed
}

/// One attempt to take the lock, removing a stale one first (`QLockFile::tryLock(0)`).
pub fn try_lock(path: &Path, own: &LockInfo) -> LockAttempt {
    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).mode(0o644).open(path) {
            Ok(mut file) => {
                // Held while the file exists, so a remover can tell a live
                // holder from a crashed one (QLockFile's native lock).
                let _ = flock_now(&file);
                if file.write_all(own.contents().as_bytes()).is_err() {
                    let _ = fs::remove_file(path);
                    return LockAttempt::Unavailable;
                }
                return LockAttempt::Taken(StartLock { path: path.to_owned(), _file: file });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !(stale_now(path, &own.host) && remove_stale(path, &own.host)) {
                    return LockAttempt::Busy;
                }
            }
            Err(_) => return LockAttempt::Unavailable,
        }
    }
    LockAttempt::Busy
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServiceState {
    #[default]
    Absent,
    Starting,
    Running,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Service {
    pub state: ServiceState,
    pub pid: i64,
}

/// `launchctl print gui/<uid>/<label>`: only the job's own properties, one
/// level inside its braces (`SingleInstance.cpp:146-177`).
pub fn launchd_service(description: &str) -> Service {
    let (mut state, mut pid, mut depth) = ("", 0_i64, 0_i32);
    for raw in description.lines() {
        let line = raw.trim();
        if line == "}" {
            depth -= 1;
            continue;
        }
        if depth == 1 {
            if let Some((name, value)) = line.split_once(" = ") {
                match name {
                    "state" => state = value,
                    "pid" => pid = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    let state = match state {
        "running" => ServiceState::Running,
        "spawn scheduled" => ServiceState::Starting,
        _ => return Service::default(),
    };
    Service { state, pid: pid.max(0) }
}

/// A start that finds no instance leaves the socket to a service another
/// process runs or is about to run (`SingleInstance::waitsForService`).
pub fn waits_for_service(service: &Service, own_pid: i64) -> bool {
    service.state != ServiceState::Absent && service.pid != own_pid
}

/// `launchctl print`, bounded at 2 s like `SingleInstance::currentService`.
fn launchd_now(label: &str) -> Service {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let Ok(child) = Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{label}")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Service::default();
    };
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) if output.status.success() => launchd_service(&String::from_utf8_lossy(&output.stdout)),
        Ok(_) => Service::default(),
        Err(_) => {
            // SAFETY: the child is ours and still running.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            Service::default()
        }
    }
}

/// `SingleInstance::Startup`'s defaults.
pub struct Timing {
    pub lock_wait: Duration,
    pub service_wait: Duration,
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self { lock_wait: Duration::from_secs(10), service_wait: Duration::from_secs(30), poll: Duration::from_millis(200) }
    }
}

/// What `start` does to the world, so tests can script it.
pub trait Start {
    fn lock(&mut self, wait: Duration) -> Result<(), String>;
    fn unlock(&mut self);
    fn forward(&mut self, activation: Activation) -> bool;
    fn service(&mut self) -> Service;
    fn claim(&mut self) -> io::Result<()>;
    fn pause(&mut self, pause: Duration);
    fn elapsed(&self) -> Duration;
    fn report(&mut self, line: &str);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Forwarded,
    Claimed,
    Failed(String),
}

/// `SingleInstance::start` (`SingleInstance.cpp:220-263`): probing the
/// socket, removing a stale one and listening are one step under the lock; a
/// start that launchd does not run leaves the socket to the service while
/// it runs or is about to.
pub fn start(effects: &mut impl Start, activation: Activation, own_pid: i64, timing: &Timing) -> Outcome {
    let mut lockable = true;
    let mut wait_reported = false;
    loop {
        if lockable {
            if let Err(why) = effects.lock(timing.lock_wait) {
                lockable = false;
                effects.report(&why);
            }
        }
        if effects.forward(activation) {
            effects.unlock();
            return Outcome::Forwarded;
        }
        let service = effects.service();
        if waits_for_service(&service, own_pid) {
            if effects.elapsed() < timing.service_wait {
                if !wait_reported {
                    wait_reported = true;
                    effects.report(&if service.pid > 0 {
                        format!("waiting for the Zerus service (pid {}) to open its socket", service.pid)
                    } else {
                        "waiting for the Zerus service to start".to_owned()
                    });
                }
                effects.unlock();
                effects.pause(timing.poll);
                continue;
            }
            effects.report(&format!(
                "the Zerus service did not open its socket within {} s; starting here",
                timing.service_wait.as_secs_f64()
            ));
        }
        let outcome = match effects.claim() {
            Ok(()) => Outcome::Claimed,
            Err(error) => Outcome::Failed(error.to_string()),
        };
        effects.unlock();
        return outcome;
    }
}

/// Hands the activation to an instance that answers (`SingleInstance.cpp:94-103`).
pub fn forward(socket: &Path, activation: Activation) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.write_all(activation.line());
    true
}

struct UnixStart<'a> {
    paths: &'a Paths,
    own: &'a LockInfo,
    label: Option<&'a str>,
    lock: Option<StartLock>,
    started: Instant,
    listener: Option<UnixListener>,
}

impl Start for UnixStart<'_> {
    fn lock(&mut self, wait: Duration) -> Result<(), String> {
        if let Some(folder) = self.paths.lock.parent() {
            let _ = fs::create_dir_all(folder);
        }
        let deadline = Instant::now() + wait;
        loop {
            match try_lock(&self.paths.lock, self.own) {
                LockAttempt::Taken(lock) => {
                    self.lock = Some(lock);
                    return Ok(());
                }
                LockAttempt::Busy if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                LockAttempt::Busy => {
                    return Err(format!("another start keeps {} locked; starting without it", self.paths.lock.display()))
                }
                LockAttempt::Unavailable => {
                    return Err(format!("cannot create {}; concurrent starts are not serialized", self.paths.lock.display()))
                }
            }
        }
    }

    fn unlock(&mut self) {
        self.lock = None;
    }

    fn forward(&mut self, activation: Activation) -> bool {
        forward(&self.paths.socket, activation)
    }

    fn service(&mut self) -> Service {
        self.label.map(launchd_now).unwrap_or_default()
    }

    fn claim(&mut self) -> io::Result<()> {
        // Nothing answers: the socket, if any, is a crashed instance's.
        match fs::remove_file(&self.paths.socket) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let listener = UnixListener::bind(&self.paths.socket)?;
        fs::set_permissions(&self.paths.socket, Permissions::from_mode(0o600))?;
        self.listener = Some(listener);
        Ok(())
    }

    fn pause(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn report(&mut self, line: &str) {
        eprintln!("zerus-client: {line}");
    }
}

/// The claimed socket; dropping it removes the file it bound, as
/// `QLocalServer::close` does, unless another instance replaced it.
pub struct Listening {
    socket: PathBuf,
    inode: u64,
}

impl Drop for Listening {
    fn drop(&mut self) {
        if fs::metadata(&self.socket).is_ok_and(|metadata| metadata.ino() == self.inode) {
            let _ = fs::remove_file(&self.socket);
        }
    }
}

pub struct Claimed {
    pub listening: Listening,
    /// Lines from later starts, queued until the app takes them.
    pub activations: Receiver<Activation>,
}

pub enum Claim {
    Claimed(Claimed),
    Forwarded,
    Failed(String),
}

/// The claimed socket, kept in the app state until the app exits.
pub struct InstanceState(pub Mutex<Option<Listening>>);

/// Takes the single-instance socket named `name`, or hands `activation` to
/// the instance that holds it.
pub fn claim(name: &str, label: Option<&str>, activation: Activation) -> Claim {
    let var = |key: &str| std::env::var_os(key).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = var("HOME").unwrap_or_default();
    let runtime = runtime_dir(Platform::current(), &home, var("XDG_RUNTIME_DIR").as_deref());
    claim_at(&paths(name, &temp_dir(), &runtime), label, activation, &LockInfo::own(), &Timing::default())
}

pub fn claim_at(paths: &Paths, label: Option<&str>, activation: Activation, own: &LockInfo, timing: &Timing) -> Claim {
    let mut effects = UnixStart { paths, own, label, lock: None, started: Instant::now(), listener: None };
    match start(&mut effects, activation, own.pid, timing) {
        Outcome::Forwarded => Claim::Forwarded,
        Outcome::Failed(error) => Claim::Failed(error),
        Outcome::Claimed => {
            let Some(listener) = effects.listener.take() else {
                return Claim::Failed("the socket was not kept".to_owned());
            };
            let inode = fs::metadata(&paths.socket).map(|metadata| metadata.ino()).unwrap_or(0);
            let (sender, activations) = mpsc::channel();
            listen(listener, move |activation| {
                let _ = sender.send(activation);
            });
            Claim::Claimed(Claimed { listening: Listening { socket: paths.socket.clone(), inode }, activations })
        }
    }
}

/// Reads one line per connection, at most 64 bytes within 1 s (`main.cpp:424-433`).
fn listen(listener: UnixListener, deliver: impl Fn(Activation) + Send + 'static) {
    std::thread::Builder::new()
        .name("zerus-client-instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut line = Vec::new();
                let mut byte = [0u8; 1];
                while line.len() < 64 {
                    match stream.read(&mut byte) {
                        Ok(1) if byte[0] != b'\n' => line.push(byte[0]),
                        _ => break,
                    }
                }
                if let Some(activation) = Activation::parse(&line) {
                    deliver(activation);
                }
            }
        })
        .expect("the instance thread starts");
}
```

- [ ] **Step 11: Run the identity and instance tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- identity:: instance::`
Expected: PASS (2 identity tests, 14 instance tests).

- [ ] **Step 12: Wire the start-up into `lib.rs`**

Add the modules `cli` and `identity`, and `#[cfg(any(target_os = "linux", target_os = "macos"))] mod instance;`. Then replace `pub fn run()` with:

```rust
/// What `run` settled before the app starts.
pub struct Startup {
    pub identity: identity::Identity,
    /// The claimed single-instance socket and the activations it receives.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub instance: Option<instance::Claimed>,
}

/// Parses Qt's command line and answers `--help`, `--version` and
/// `--build-info`. It then takes the single-instance socket, or hands the
/// activation to the running instance and returns, and runs the app.
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match cli::parse(&args) {
        Ok(cli::Command::Run(options)) => options,
        Ok(cli::Command::Help) => {
            print!("{}", cli::usage());
            return;
        }
        Ok(cli::Command::Version) => {
            print!("{}", cli::version_line(&cli::BUILD));
            return;
        }
        Ok(cli::Command::BuildInfo) => {
            println!("{}", cli::build_info(&cli::BUILD));
            return;
        }
        Err(error) => {
            eprint!("zerus-client: {error}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    for option in &options.ignored {
        eprintln!("zerus-client: {option} is accepted for the Qt GUI's command line and not used");
    }
    // Before any thread starts: the core's `default_hgs` reads it.
    if let Some(hgs) = &options.hgs {
        std::env::set_var("ZERUS_CLIENT_HGS", hgs);
    }
    let context = tauri::generate_context!();
    let identity = identity::current(&context.config().identifier);
    let opens_sessions = options.opens_sessions(identity.is_switch());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let instance = match identity.instance_name(identity::flag("ZERUS_CLIENT_SINGLE_INSTANCE")) {
        None => None,
        Some(name) => {
            let activation = if opens_sessions { instance::Activation::Sessions } else { instance::Activation::Show };
            match instance::claim(&name, identity.launchd_label(), activation) {
                instance::Claim::Claimed(claimed) => Some(claimed),
                instance::Claim::Forwarded => {
                    eprintln!("zerus-client: already running");
                    return;
                }
                instance::Claim::Failed(error) => {
                    eprintln!("zerus-client: cannot claim the socket: {error}");
                    std::process::exit(3);
                }
            }
        }
    };
    launch(
        context,
        Startup {
            identity,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            instance,
        },
    );
}

fn launch(context: tauri::Context<tauri::Wry>, startup: Startup) {
    let identity = startup.identity;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (listening, activations) = match startup.instance {
        Some(claimed) => (Some(claimed.listening), Some(claimed.activations)),
        None => (None, None),
    };
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(commands::AppState::default());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let builder = builder.manage(instance::InstanceState(std::sync::Mutex::new(listening)));
    builder
        .invoke_handler(handler())
        .setup(move |app| {
            use tauri::Manager;
            // The one source of the client's data directory (Decision 5).
            app.manage(identity.clone());
            #[cfg(feature = "live")]
            {
                let dir = identity.data_dir_now();
                std::fs::create_dir_all(&dir)?;
                // A replay never touches the real read marks.
                let replay = std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|list| !list.is_empty());
                app.manage(actions::Actions::new(dir.join(actions::marks_file(replay))));
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump));
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if let Some(activations) = activations {
                // Task 3 shows the window on `sessions`.
                std::thread::spawn(move || {
                    for activation in activations {
                        eprintln!("zerus-client: activation {activation:?}");
                    }
                });
            }
            // The headless guard and the window creation stay exactly as on main.
            let display = display::display_name();
            if !display::headless_allowed(std::env::var("ZERUS_CLIENT_HEADLESS").is_ok_and(|v| v == "1"), display.as_deref()) {
                eprintln!("zerus-client: ZERUS_CLIENT_HEADLESS=1 but the display is {display:?}, not an X server; no window opens (set GDK_BACKEND=x11)");
                std::process::exit(3);
            }
            let window = create_main_window(app.handle())?;
            if std::env::var("ZERUS_CLIENT_PERF").is_ok_and(|mode| mode == "auto") {
                if let Err(error) = window.set_always_on_top(true).and_then(|()| window.set_focus()) {
                    eprintln!("zerus-client perf: could not bring the window to the front: {error}");
                }
            }
            Ok(())
        })
        .run(context)
        .expect("Zerus Next failed to start");
}
```

In `client/README.md`, the read-marks paragraph names "the app data directory (`app_data_dir`, ...)": change it to "the client's data directory (Tauri's `app_data_dir` formula with the running bundle's identifier, `identity.rs`, ...)". For the dev app the two are identical (Decision 5). Nothing in `src-tauri` calls `app.path().app_data_dir()` any more. Do not touch `main.rs`.

- [ ] **Step 13: Check the binary's early answers without a display**

Run:
```bash
cd client && npm ci && RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=4 nice npx tauri build --no-bundle
./src-tauri/target/release/zerus-client --build-info
./src-tauri/target/release/zerus-client --version
./src-tauri/target/release/zerus-client --frobnicate; echo "exit $?"
```
Expected:
- `--build-info` prints one JSON object with `version` equal to `VERSION`, `devBuildNumber` 0, and `sourceCommit` set with `sourceDirty: true`, because the work tree carries this task's uncommitted files.
- `--version` prints `Zerus <VERSION>`.
- `--frobnicate` prints the usage and exits with code 2.
- `ZERUS_DEV_BUILD_NUMBER=7 npx tauri build --no-bundle` in this dirty tree fails with "A numbered Dev build requires a clean Git source tree".

- [ ] **Step 14: Check single instance headless on arch**

Run both starts under the headless wrapper, the second while the first runs:
```bash
cd client
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
  ZERUS_CLIENT_SINGLE_INSTANCE=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson \
  xvfb-run -a -s "-screen 0 1440x900x24" sh -c '
    ./src-tauri/target/release/zerus-client 2> first.log & first=$!
    sleep 5
    ./src-tauri/target/release/zerus-client --sessions; echo "second exit $?"
    sleep 1; kill $first; wait $first; grep -E "activation|already" first.log'
rm -f first.log
```
Expected: `zerus-client: already running`, `second exit 0`, then `zerus-client: activation Sessions` from the first.

- [ ] **Step 15: Check on the Mac, display-free**

Run **mac-lock**, **mac-sync**, **mac-clippy**, **mac-test** and **mac-done**.
Expected: clean, with the `instance::` and `identity::` tests passing on macOS (`proc_name` path).

- [ ] **Step 16: Run the task's checks and commit**

Run fmt, both clippy variants, the src-tauri tests, `check-ui-separators.py` and `check-source-privacy.py`.

```bash
git add client/src-tauri/build.rs client/src-tauri/build_identity.rs client/src-tauri/src/cli.rs \
  client/src-tauri/src/identity.rs client/src-tauri/src/instance.rs client/src-tauri/src/lib.rs \
  client/src-tauri/src/platform/macos.rs client/src-tauri/Cargo.toml client/README.md
git commit -m "feat(client): Qt's command line, build identity and single-instance protocol"
```

---

### Task 3: Lifecycle: hidden start, close hides, activation policy, activation lines, signals (zerus-5wmh.44)

The client behaves as Qt's menu-bar app on the Mac:
- **Hidden start.** It starts hidden unless started with `--sessions` (`main.cpp:297`, `AppConfig.cpp:461-464`).
- **Close hides.** Closing the window hides it and the app keeps running (`main.cpp:279`).
- **Dock icon.** The Dock icon shows only while the window is open, shown or minimised (`MacAppVisibility.mm:65-88`).
- **Activation lines.** A `sessions` line opens and raises the window (`main.cpp:424-433`, `TrayAgent.cpp:781-783`).
- **Signals and quit.** SIGTERM, SIGINT and SIGHUP quit like Quit (`TerminationSignals.h`). The Apple Event quit from the updater ends in `RunEvent::Exit` (tao's `applicationWillTerminate`), where the socket is removed. Other flushes (drafts, organization) join that hook under Decision 15.
- **Main-thread QoS.** The main thread gets Qt's QoS (`main.cpp:256-263`).
- **Fleet at start.** The fleet starts from Rust at launch, so the menu bar and notifications work before any window loads.

**Files:**
- Create: `client/src-tauri/src/lifecycle.rs`, `client/src-tauri/src/signals.rs`
- Modify: `client/src-tauri/src/lib.rs` (`Startup.opens_sessions`, `launch`)
- Modify: `client/src-tauri/src/commands.rs` (`AppState::start_source`)
- Modify: `client/src-tauri/src/platform/macos.rs` (`raise_main_thread_qos`)
- Test: unit tests in `lifecycle.rs` and `signals.rs`

**Interfaces:**
- Consumes: `lib::Startup`, `instance::{Activation, Claimed, InstanceState}` (Task 2), `create_main_window`, `MAIN_WINDOW`.
- Produces:
  - `lifecycle::{Lifecycle { menu_bar }, Policy, policy(bool), reopen_restores(bool, bool), close_hides(bool), window_at_start(bool, bool), on_event(&AppHandle, &RunEvent), show_main(&AppHandle) -> tauri::Result<WebviewWindow>, hide_main(&AppHandle), sync_policy(&AppHandle)}`.
  - `signals::{SIGNALS, install(on_quit) -> io::Result<Vec<i32>>}`.
  - `commands::AppState::start_source(&self) -> Result<String, String>` (live feature).
  - `lib::Startup { identity, opens_sessions, instance }`.

- [ ] **Step 1: Write the failing tests**

Create `client/src-tauri/src/lifecycle.rs` with only this test module (`mod lifecycle;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_icon_shows_while_the_window_is_open() {
        assert_eq!(policy(true), Policy::Regular);
        assert_eq!(policy(false), Policy::Accessory);
    }

    #[test]
    fn a_dock_click_restores_only_a_minimised_window() {
        assert!(reopen_restores(true, false));
        assert!(!reopen_restores(true, true));
        assert!(!reopen_restores(false, false), "a closed window stays closed, as in Qt");
    }

    #[test]
    fn the_window_opens_at_start_only_when_asked_in_the_menu_bar() {
        assert!(!window_at_start(false, true), "the LaunchAgent start stays in the menu bar");
        assert!(window_at_start(true, true));
        assert!(window_at_start(false, false), "without a menu-bar item the window is the only way in");
        assert!(close_hides(true));
        assert!(!close_hides(false));
    }
}
```

Create `client/src-tauri/src/signals.rs` with only this test module (`#[cfg(any(target_os = "linux", target_os = "macos"))] mod signals;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_signal_reaches_the_quit_handler() {
        let (sender, receiver) = std::sync::mpsc::channel();
        // Only SIGHUP: a test process that handled SIGINT would ignore Ctrl-C.
        let handled = install_for(&[libc::SIGHUP], move |signal| {
            let _ = sender.send(signal);
        })
        .unwrap();
        if !handled.contains(&libc::SIGHUP) {
            return; // started under nohup: an ignored signal stays ignored
        }
        // SAFETY: raises SIGHUP in this process; the handler above takes it.
        unsafe { libc::raise(libc::SIGHUP) };
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)).unwrap(), libc::SIGHUP);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- lifecycle:: signals::`
Expected: FAIL to compile: `policy`, `Policy`, `reopen_restores`, `window_at_start`, `close_hides` and `install_for` are missing.

- [ ] **Step 3: Write `lifecycle.rs` above its tests**

```rust
//! The menu-bar app's lifecycle, as the Qt GUI behaves on macOS
//! (`tray/src/main.cpp`, `tray/src/MacAppVisibility.mm`): closing the window
//! hides it and the app stays in the menu bar; a Dock icon shows only while
//! the window is open; a Dock click restores a minimised window; quitting
//! removes the single-instance socket. Builds without a menu-bar item close
//! as before. `lib.rs` calls [`on_event`] from its run loop (Decision 15).
use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

use crate::MAIN_WINDOW;

/// The close button only hides the window of an app with a menu-bar item
/// (`QApplication::setQuitOnLastWindowClosed(false)`, `main.cpp:279`).
pub fn close_hides(menu_bar: bool) -> bool {
    menu_bar
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Regular,
    Accessory,
}

/// A normal app while its window is open, shown or minimised, else a
/// menu-bar app (`MacAppVisibility.mm:65-81`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn policy(window_open: bool) -> Policy {
    if window_open {
        Policy::Regular
    } else {
        Policy::Accessory
    }
}

/// A Dock click restores a minimised window when none is visible
/// (`MacAppVisibility.mm:82-88`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn reopen_restores(minimized: bool, visible: bool) -> bool {
    minimized && !visible
}

/// The window opens at start for `--sessions`, and always without a menu-bar item.
pub fn window_at_start(opens_sessions: bool, menu_bar: bool) -> bool {
    opens_sessions || !menu_bar
}

/// Kept in the app state.
pub struct Lifecycle {
    pub menu_bar: bool,
}

fn menu_bar(app: &AppHandle) -> bool {
    app.try_state::<Lifecycle>().is_some_and(|lifecycle| lifecycle.menu_bar)
}

/// The run loop's events this module handles.
pub fn on_event(app: &AppHandle, event: &RunEvent) {
    match event {
        // The last window went away: the app keeps its menu-bar item.
        RunEvent::ExitRequested { code: None, api, .. } if menu_bar(app) => api.prevent_exit(),
        RunEvent::WindowEvent { label, event: WindowEvent::CloseRequested { api, .. }, .. }
            if label == MAIN_WINDOW && close_hides(menu_bar(app)) =>
        {
            api.prevent_close();
            hide_main(app);
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => reopen(app),
        RunEvent::Exit => exit(app),
        _ => {}
    }
}

/// Opens Zerus: Qt's `TrayAgent::showSessions` (`TrayAgent.cpp:781-783`).
pub fn show_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => window,
        None => crate::create_main_window(app)?,
    };
    if window.is_minimized()? {
        window.unminimize()?;
    }
    window.show()?;
    window.set_focus()?;
    sync_policy(app);
    Ok(window)
}

pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    sync_policy(app);
}

#[cfg(target_os = "macos")]
fn window_open(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW).is_some_and(|window| {
        window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false)
    })
}

/// Applies [`policy`] to the app (macOS menu-bar builds only).
pub fn sync_policy(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if menu_bar(app) {
        let policy = match policy(window_open(app)) {
            Policy::Regular => tauri::ActivationPolicy::Regular,
            Policy::Accessory => tauri::ActivationPolicy::Accessory,
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
fn reopen(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let minimized = window.is_minimized().unwrap_or(false);
        let visible = window.is_visible().unwrap_or(false);
        if reopen_restores(minimized, visible) {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

/// Before the process ends: the socket goes, as `QLocalServer::close` removes it.
fn exit(app: &AppHandle) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(state) = app.try_state::<crate::instance::InstanceState>() {
        drop(state.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = app;
}
```

- [ ] **Step 4: Write `signals.rs` above its tests**

```rust
//! SIGTERM, SIGINT and SIGHUP quit like the menu's Quit
//! (`tray/src/TerminationSignals.h`): `launchctl bootout` and `unload` stop
//! the app with SIGTERM, and `RunEvent::Exit` then runs what must happen
//! before the process ends. The handler only writes the signal number to a
//! self-pipe; a thread calls `on_quit`. Each handler resets to the default
//! action when it fires (SA_RESETHAND), so a second signal still ends an app
//! that cannot finish quitting. Signals ignored at start stay ignored.
use std::io;
use std::sync::atomic::{AtomicI32, Ordering};

pub const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

static WRITE_END: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
    let fd = WRITE_END.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; the byte outlives the call.
        unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }
}

/// Installs the handlers for [`SIGNALS`]; returns the signals now handled.
pub fn install(on_quit: impl FnOnce(i32) + Send + 'static) -> io::Result<Vec<i32>> {
    install_for(&SIGNALS, on_quit)
}

fn install_for(signals: &[libc::c_int], on_quit: impl FnOnce(i32) + Send + 'static) -> io::Result<Vec<i32>> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for both ends.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // SAFETY: fd is a descriptor this call just created.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    WRITE_END.store(fds[1], Ordering::Relaxed);
    let mut handled = Vec::new();
    for &signal in signals {
        // SAFETY: plain sigaction(2) calls with zeroed, then filled structs.
        unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(signal, std::ptr::null(), &mut current) != 0 || current.sa_sigaction == libc::SIG_IGN {
                continue;
            }
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESETHAND | libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(signal, &action, std::ptr::null_mut()) == 0 {
                handled.push(signal);
            }
        }
    }
    let read = fds[0];
    std::thread::Builder::new().name("zerus-client-signals".into()).spawn(move || {
        let mut byte = 0u8;
        loop {
            // SAFETY: reads one byte into a local.
            let count = unsafe { libc::read(read, (&mut byte as *mut u8).cast(), 1) };
            if count == 1 {
                on_quit(i32::from(byte));
                return;
            }
            if count < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
    })?;
    Ok(handled)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- lifecycle:: signals::`
Expected: PASS (4 tests).

- [ ] **Step 6: Start the fleet from Rust and raise the main thread's QoS**

In `client/src-tauri/src/commands.rs`, add to `impl AppState`:

```rust
    /// Starts the data source now, before any window loads: the menu bar and
    /// notifications need the fleet while the window is closed. A later
    /// `start` from the webview finds it running and resends the catalog.
    #[cfg(feature = "live")]
    pub fn start_source(&self) -> Result<String, String> {
        self.live.start()
    }
```

In `client/src-tauri/src/platform/macos.rs`, add:

```rust
/// LaunchServices gives an app's main thread this QoS; the LaunchAgent does
/// not, and its `ProcessType Interactive` only lifts the clamp
/// (`tray/src/main.cpp:256-263`). Call on the main thread.
pub fn raise_main_thread_qos() {
    // SAFETY: changes the calling thread's own QoS class.
    unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
}
```

- [ ] **Step 7: Rewrite `launch` with the lifecycle**

In `lib.rs`:
- Add `mod lifecycle;` and the `signals` module from Step 1.
- Add `pub opens_sessions: bool,` to `Startup` and set it in `run()` (`Startup { identity, opens_sessions, instance }`).
- Add `#[cfg(target_os = "macos")] platform::macos::raise_main_thread_qos();` as the first line after the single-instance `match` in `run()`.

Then replace `launch` with:

```rust
fn launch(context: tauri::Context<tauri::Wry>, startup: Startup) {
    let identity = startup.identity;
    let opens_sessions = startup.opens_sessions;
    // Only the macOS build has a menu-bar item in this phase; Linux keeps Qt.
    let menu_bar = cfg!(target_os = "macos");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (listening, activations) = match startup.instance {
        Some(claimed) => (Some(claimed.listening), Some(claimed.activations)),
        None => (None, None),
    };
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(commands::AppState::default())
        .manage(lifecycle::Lifecycle { menu_bar });
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let builder = builder.manage(instance::InstanceState(std::sync::Mutex::new(listening)));
    builder
        .invoke_handler(handler())
        .setup(move |app| {
            use tauri::Manager;
            // The one source of the client's data directory (Decision 5).
            app.manage(identity.clone());
            #[cfg(feature = "live")]
            {
                let dir = identity.data_dir_now();
                std::fs::create_dir_all(&dir)?;
                // A replay never touches the real read marks.
                let replay = std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|list| !list.is_empty());
                app.manage(actions::Actions::new(dir.join(actions::marks_file(replay))));
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump));
                if menu_bar {
                    app.state::<commands::AppState>().start_source()?;
                }
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                if let Some(activations) = activations {
                    forward_activations(app.handle().clone(), activations);
                }
                let handle = app.handle().clone();
                signals::install(move |signal| {
                    eprintln!("zerus-client: quitting on signal {signal}");
                    handle.exit(0);
                })?;
            }
            let display = display::display_name();
            if !display::headless_allowed(std::env::var("ZERUS_CLIENT_HEADLESS").is_ok_and(|v| v == "1"), display.as_deref()) {
                eprintln!("zerus-client: ZERUS_CLIENT_HEADLESS=1 but the display is {display:?}, not an X server; no window opens (set GDK_BACKEND=x11)");
                std::process::exit(3);
            }
            if lifecycle::window_at_start(opens_sessions, menu_bar) {
                let window = lifecycle::show_main(app.handle())?;
                // An unattended measurement needs frames: keep its window in front.
                if std::env::var("ZERUS_CLIENT_PERF").is_ok_and(|mode| mode == "auto") {
                    if let Err(error) = window.set_always_on_top(true).and_then(|()| window.set_focus()) {
                        eprintln!("zerus-client perf: could not bring the window to the front: {error}");
                    }
                }
            }
            lifecycle::sync_policy(app.handle());
            Ok(())
        })
        .build(context)
        .expect("Zerus Next failed to start")
        .run(|app, event| {
            lifecycle::on_event(app, &event);
        });
}

/// A `sessions` line from a later start opens and raises the window; `show`
/// changes nothing visible (`main.cpp:424-433`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn forward_activations(app: tauri::AppHandle, activations: std::sync::mpsc::Receiver<instance::Activation>) {
    std::thread::Builder::new()
        .name("zerus-client-activations".into())
        .spawn(move || {
            for activation in activations {
                eprintln!("zerus-client: activation {activation:?}");
                if activation == instance::Activation::Sessions {
                    let target = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Err(error) = lifecycle::show_main(&target) {
                            eprintln!("zerus-client: could not open the window: {error}");
                        }
                    });
                }
            }
        })
        .expect("the activation thread starts");
}
```

**Merge note (Decision 15).** If the media branch landed first, its closure has an `if let tauri::RunEvent::Exit = event { ... exit(_app); }` block. Move that exit work into `lifecycle::exit`, before `state.release()`, and keep `lifecycle::on_event(app, &event);` as the closure's first statement (Decision 15). Change only `.build(...)` to take `context`.

- [ ] **Step 8: Check the quit path headless on arch**

```bash
cd client && RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=4 nice npx tauri build --no-bundle
socket="$(python3 -c 'import os,tempfile; print(os.path.realpath(tempfile.gettempdir()))')/zerus-client-dev.zerus.next"
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
  ZERUS_CLIENT_SINGLE_INSTANCE=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson SOCKET="$socket" \
  xvfb-run -a -s "-screen 0 1440x900x24" sh -c '
    ./src-tauri/target/release/zerus-client 2> run.log & app=$!
    sleep 5; test -S "$SOCKET" && echo "socket present"
    kill -TERM $app; wait $app; echo "exit $?"
    grep "quitting on signal 15" run.log; test -e "$SOCKET" || echo "socket removed"'
rm -f run.log
```
Expected: `socket present`, `exit 0`, `zerus-client: quitting on signal 15`, `socket removed`.

- [ ] **Step 9: Check on the Mac, display-free**

Run **mac-lock**, **mac-sync**, **mac-clippy**, **mac-test** and **mac-done**. Expected: clean.

- [ ] **Step 10: Run the task's checks and commit**

```bash
git add client/src-tauri/src/lifecycle.rs client/src-tauri/src/signals.rs client/src-tauri/src/lib.rs \
  client/src-tauri/src/commands.rs client/src-tauri/src/platform/macos.rs
git commit -m "feat(client): menu-bar lifecycle, activation lines and quit on signals"
```

---

### Task 4: The menu-bar item on macOS (zerus-5wmh.44)

The status item shows Qt's tile, which AppKit draws from pure geometry (`IconFactory::renderTile`, `IconFactory.cpp:114-159`), and Qt's menu.
- **Clicks.** A primary click opens Zerus; the secondary click shows the menu (`QtTrayIcon.cpp:12-20`).
- **Session entries.** A running session opens in Terminal.app or is copied, per "Open in" (`MacBackend.cpp:27-36`, `ClipboardBackend.cpp:242-265`). A saved session opens the window on its row (`TrayAgent.cpp:214-216`).
- **Notices.** The menu's notices go through `osascript display notification`, as Qt's `TrayAgent::notify` does on macOS (`TrayAgent.cpp:796-830`).
- **Dock badge.** The Dock tile shows the attention count, as Qt's `MacApplicationBadge` does (spec row 291; no bead names it).
- **Menu tracking.** `NSMenuDidBeginTrackingNotification` and `NSMenuDidEndTrackingNotification` drive the `Gate` from Task 1. This task also adds the frontend's `open-session` and `desk-notice` handling, which Task 6 reuses.

**Files:**
- Create: `client/core/src/trayicon.rs`; modify `client/core/src/lib.rs` (`pub mod trayicon;`)
- Modify: `client/core/src/shell.rs` (`apple_script_escape`, `terminal_app_script`, `notification_script`, `notification_text`)
- Modify: `client/src-tauri/src/pump.rs` (tap)
- Create: `client/src-tauri/src/desk.rs`
- Create: `client/src-tauri/src/platform/macos_menubar.rs`; modify `client/src-tauri/src/platform/mod.rs`
- Modify: `client/src-tauri/src/lib.rs` (setup, `handler()`), `client/src-tauri/Cargo.toml`
- Modify: `client/src/bridge.ts`, `client/src/main.tsx`; create `client/src/store/desk.ts`, `client/src/store/desk.test.ts`

**Interfaces:**
- Consumes: `menubar::{Desk, Face, Entry, OpenMode, Gate, Apply, tray_conf_value}` (Task 1); `lifecycle::show_main` (Task 3); `identity::Identity::data_dir_now` (Task 2).
- Produces:
  - `trayicon::{Rgb, Rect, Text, Tile, Step, tile(&Face, f64, Rgb) -> Tile, mark_path() -> Vec<Step>, unpremultiply(&mut [u8]), parse_color(&str) -> Option<Rgb>, foreground(bool, Option<Rgb>) -> Rgb}`.
  - `shell::{apple_script_escape, terminal_app_script, notification_script, notification_text}`.
  - `pump::Tap`, `Pump::tap(Tap)`.
  - `desk::{PACE, DeskInput { Change(Box<Change>), Mode(OpenMode) }, DeskState, OpenSession { computer, key }, Notice { title, text }, Pending { open, notices }, PENDING_NOTICES, MODE_FILE, load_mode(&Path, Option<&str>) -> OpenMode, save_mode, write_private(&Path, &[u8]) -> io::Result<()>, tray_conf() -> Option<String>, menu_bar_change(&Change) -> bool, start(&AppHandle, &Pump, PathBuf, OpenMode), open_session(&AppHandle, &ComputerId, &str), open_running(&AppHandle, &ComputerId, &str), set_mode(&AppHandle, OpenMode), tray_notify(&str, &str), window_notice(&AppHandle, &str, &str), desk_take_pending (command)}`. Task 6 extends `DeskInput` and `run`.
  - `macos_menubar::{TRAY, TILE_PIXELS, MenuAction, MenuBarState, install(&AppHandle, Option<Rgb>), offer(&AppHandle, Face, String, Vec<Entry>, Option<String>), menu_event(&AppHandle, &str), render(&Tile) -> Option<Vec<u8>>}`.
  - Frontend: `bridge.{OpenSession, DeskNotice, DeskPending, onOpenSession, onDeskNotice, deskTakePending}`; `store/desk.{openSession, startDesk}`.
- As shipped: `Inbox` (with the page generation), `store_mode` on the desk thread and `page_loading` supersede these snippets.

- [ ] **Step 1: Write the failing core tests**

Create `client/core/src/trayicon.rs` with only this test module (`pub mod trayicon;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::menubar::Face;

    #[test]
    fn the_tile_follows_qt_geometry_at_36_pixels() {
        let white = Rgb::new(255, 255, 255);
        let tile = tile(&Face::counts(12, 3, true), 36.0, white);
        assert_eq!(tile.upper, Rect { x: 0.0, y: 0.0, w: 36.0, h: 17.0 });
        assert_eq!(tile.lower, Rect { x: 0.0, y: 19.0, w: 36.0, h: 17.0 });
        assert_eq!(tile.radius, 4.0);
        assert_eq!(tile.surface, SURFACE_FOR_LIGHT_TEXT);
        assert_eq!(tile.lower_fill, AMBER);
        assert_eq!(tile.mark, Some(Rect { x: 0.0, y: 0.0, w: 17.0, h: 17.0 }));
        assert_eq!(tile.total, Text { text: "12".into(), rect: Rect { x: 17.0, y: 1.0, w: 17.0, h: 15.0 }, color: white });
        assert_eq!(tile.attention, Text { text: "3".into(), rect: Rect { x: 2.0, y: 20.0, w: 32.0, h: 15.0 }, color: ON_AMBER });
        assert_eq!(tile.font_pixels, 20.0);
        assert_eq!(tile.dot, Some((33.75, 2.25, 2.25)));
    }

    #[test]
    fn a_capped_total_takes_the_whole_row_and_quiet_attention_is_translucent() {
        let black = Rgb::new(0, 0, 0);
        let tile = tile(&Face::counts(120, 0, false), 36.0, black);
        assert_eq!(tile.surface, SURFACE_FOR_DARK_TEXT);
        assert_eq!(tile.mark, None);
        assert_eq!(tile.total.rect, Rect { x: 2.0, y: 1.0, w: 32.0, h: 15.0 });
        assert_eq!(tile.lower_fill, tile.surface);
        assert_eq!(tile.attention.color, Rgb { a: 160, ..black });
        assert_eq!(tile.dot, None);
    }

    #[test]
    fn the_mark_is_three_blades_from_qt_curves() {
        let steps = mark_path();
        assert_eq!(steps.len(), 30);
        assert_eq!(steps.iter().filter(|step| matches!(step, Step::Move(..))).count(), 3);
        assert_eq!(steps.iter().filter(|step| matches!(step, Step::Close)).count(), 3);
        let Step::Move(x, y) = steps[0] else { panic!("a blade starts with a move") };
        assert!((x - 40.5024).abs() < 1e-9 && (y - 9.2112).abs() < 1e-9, "({x}, {y})");
    }

    #[test]
    fn pixels_lose_their_premultiplied_alpha() {
        let mut rgba = [128, 64, 0, 128, 10, 10, 10, 0, 10, 20, 30, 255];
        unpremultiply(&mut rgba);
        assert_eq!(rgba, [255, 128, 0, 128, 0, 0, 0, 0, 10, 20, 30, 255]);
    }

    #[test]
    fn icon_colours_read_like_qt() {
        assert_eq!(parse_color("#ff8000"), Some(Rgb::new(255, 128, 0)));
        assert_eq!(parse_color(" #f80 "), Some(Rgb::new(255, 136, 0)));
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(foreground(true, None), Rgb::new(255, 255, 255));
        assert_eq!(foreground(false, None), Rgb::new(0, 0, 0));
        assert_eq!(foreground(true, Some(Rgb::new(1, 2, 3))), Rgb::new(1, 2, 3));
    }
}
```

Add to the tests of `client/core/src/shell.rs`:

```rust
    #[test]
    fn apple_script_strings_escape_backslash_first() {
        assert_eq!(apple_script_escape(r#"a\b"c"#), r#"a\\b\"c"#);
        assert_eq!(apple_script_escape("one\ntwo"), "one\\ntwo");
        assert_eq!(
            terminal_app_script("hgs @mac a 'claude/R\"D'"),
            "tell application \"Terminal\"\n    do script \"hgs @mac a 'claude/R\\\"D'\"\n    activate\nend tell"
        );
        assert_eq!(
            notification_script("Zerus: copied", "hgs a x\nnext"),
            "display notification \"hgs a x\\nnext\" with title \"Zerus: copied\""
        );
    }

    #[test]
    fn long_copied_commands_are_cut_in_the_middle() {
        assert_eq!(notification_text("hgs a x"), "hgs a x");
        let long = format!("hgs @mac a {}", "x".repeat(200));
        let shown = notification_text(&long);
        assert_eq!(shown.chars().count(), 120);
        assert!(shown.starts_with("hgs @mac a xx"));
        assert!(shown.contains(" \u{2026} "));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml -- trayicon shell::`
Expected: FAIL to compile (missing `tile`, `Rgb`, `mark_path`, `unpremultiply`, `parse_color`, `foreground`, `apple_script_escape`, `terminal_app_script`, `notification_script`, `notification_text`).

- [ ] **Step 3: Write `trayicon.rs` above its tests**

```rust
//! Qt's menu-bar tile (`tray/src/IconFactory.cpp:114-159`) as geometry and
//! colours, and the Zerus mark (`IconFactory.cpp:21-55`). Pure: the platform
//! glue fills the shapes, draws the two counts and reads the pixels back.
use crate::menubar::Face;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// HSL lightness, as `QColor::lightnessF`.
    pub fn lightness(self) -> f64 {
        let max = f64::from(self.r.max(self.g).max(self.b));
        let min = f64::from(self.r.min(self.g).min(self.b));
        (max + min) / 2.0 / 255.0
    }
}

/// A rectangle in pixels, origin at the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// `QRectF::adjusted`.
    pub fn adjusted(self, left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self { x: self.x + left, y: self.y + top, w: self.w - left + right, h: self.h - top + bottom }
    }
}

/// A count and the box it is centred in.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub rect: Rect,
    pub color: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub size: f64,
    pub upper: Rect,
    pub lower: Rect,
    pub radius: f64,
    pub surface: Rgb,
    pub lower_fill: Rgb,
    /// The Zerus mark's box beside a total of at most two characters.
    pub mark: Option<Rect>,
    pub foreground: Rgb,
    pub total: Text,
    pub attention: Text,
    /// The font size tried first; the glue shrinks it until the count fits.
    pub font_pixels: f64,
    /// The stale dot: centre x, centre y, radius.
    pub dot: Option<(f64, f64, f64)>,
}

/// The counter surface behind light text (a dark menu bar) and dark text.
pub const SURFACE_FOR_LIGHT_TEXT: Rgb = Rgb::new(0x29, 0x32, 0x3c);
pub const SURFACE_FOR_DARK_TEXT: Rgb = Rgb::new(0xe3, 0xe9, 0xef);
pub const AMBER: Rgb = Rgb::new(0xf3, 0xbd, 0x64);
pub const ON_AMBER: Rgb = Rgb::new(0x20, 0x25, 0x2c);

/// `IconFactory::renderTile` at `size` pixels.
pub fn tile(face: &Face, size: f64, foreground: Rgb) -> Tile {
    let gap = (size / 22.0).round().max(1.0);
    let upper_height = ((size - gap) / 2.0).floor();
    let upper = Rect { x: 0.0, y: 0.0, w: size, h: upper_height };
    let lower = Rect { x: 0.0, y: upper_height + gap, w: size, h: size - upper_height - gap };
    let surface = if foreground.lightness() > 0.5 { SURFACE_FOR_LIGHT_TEXT } else { SURFACE_FOR_DARK_TEXT };
    // Large capped counts take the whole top row (`IconFactory.cpp:144-146`).
    let mark_size = if face.total.chars().count() <= 2 { upper_height } else { 0.0 };
    let inset = (size / 24.0).round().max(1.0);
    let total_left = if mark_size > 0.0 { mark_size } else { inset };
    Tile {
        size,
        upper,
        lower,
        radius: (size / 9.0).max(1.5),
        surface,
        lower_fill: if face.highlighted { AMBER } else { surface },
        mark: (mark_size > 0.0).then_some(Rect { x: 0.0, y: 0.0, w: mark_size, h: mark_size }),
        foreground,
        total: Text { text: face.total.clone(), rect: upper.adjusted(total_left, 1.0, -inset, -1.0), color: foreground },
        attention: Text {
            text: face.attention.clone(),
            rect: lower.adjusted(inset, 1.0, -inset, -1.0),
            color: if face.highlighted { ON_AMBER } else { Rgb { a: 160, ..foreground } },
        },
        font_pixels: (size * 0.55).round().max(8.0),
        dot: face.dot.then(|| {
            let radius = (size / 16.0).max(1.0);
            (size - radius, radius, radius)
        }),
    }
}

/// One step of a path in a 100-unit box, y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Move(f64, f64),
    Curve { c1: (f64, f64), c2: (f64, f64), to: (f64, f64) },
    Close,
}

/// The selected monochrome Swarm: three separated blades with the curves and
/// transforms of `zerusMark` (`IconFactory.cpp:21-55`).
pub fn mark_path() -> Vec<Step> {
    const START: (f64, f64) = (39.0, 9.0);
    const CURVES: [[f64; 6]; 8] = [
        [54.0, 3.0, 70.0, 13.0, 77.0, 29.0],
        [72.0, 25.0, 69.0, 24.0, 72.0, 32.0],
        [81.0, 44.0, 77.0, 58.0, 65.0, 65.0],
        [70.0, 51.0, 65.0, 36.0, 52.0, 28.0],
        [40.0, 21.0, 30.0, 25.0, 22.0, 34.0],
        [25.0, 21.0, 38.0, 15.0, 51.0, 19.0],
        [57.0, 21.0, 56.0, 16.0, 51.0, 13.0],
        [47.0, 11.0, 43.0, 10.0, 39.0, 9.0],
    ];
    // translate(56, 30) scale(0.88) translate(-56, -30)
    let spacing = |(x, y): (f64, f64)| (56.0 + 0.88 * (x - 56.0), 30.0 + 0.88 * (y - 30.0));
    let mut steps = Vec::with_capacity(30);
    for angle in [0.0_f64, 120.0, 240.0] {
        let (sin, cos) = angle.to_radians().sin_cos();
        // translate(50, 50) scale(1.06) rotate(angle) translate(-50, -50); QTransform::rotate turns clockwise with y down.
        let place = |point: (f64, f64)| {
            let (x, y) = spacing(point);
            let (dx, dy) = (x - 50.0, y - 50.0);
            (50.0 + 1.06 * (dx * cos - dy * sin), 50.0 + 1.06 * (dx * sin + dy * cos))
        };
        let (x, y) = place(START);
        steps.push(Step::Move(x, y));
        for curve in CURVES {
            steps.push(Step::Curve {
                c1: place((curve[0], curve[1])),
                c2: place((curve[2], curve[3])),
                to: place((curve[4], curve[5])),
            });
        }
        steps.push(Step::Close);
    }
    steps
}

/// AppKit bitmaps are premultiplied; a tray image is not.
pub fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

/// `#rgb` or `#rrggbb`, the forms `icon-color` documents (`AppConfig.cpp:296-304`).
pub fn parse_color(value: &str) -> Option<Rgb> {
    let hex: Vec<u8> = value
        .trim()
        .strip_prefix('#')?
        .chars()
        .map(|c| c.to_digit(16).map(|digit| digit as u8))
        .collect::<Option<_>>()?;
    match hex.as_slice() {
        [r, g, b] => Some(Rgb::new(r * 17, g * 17, b * 17)),
        [r1, r2, g1, g2, b1, b2] => Some(Rgb::new(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
        _ => None,
    }
}

/// `IconFactory::foreground`: `icon-color`, else white on a dark menu bar
/// and black on a light one.
pub fn foreground(dark: bool, configured: Option<Rgb>) -> Rgb {
    configured.unwrap_or(if dark { Rgb::new(255, 255, 255) } else { Rgb::new(0, 0, 0) })
}
```

Add to `client/core/src/shell.rs` (above its tests):

```rust
/// An AppleScript string literal's contents: backslash first, then quote and
/// newline (`MacBackend.cpp:7-23`).
pub fn apple_script_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Opens `command` in a new Terminal.app window and brings it to the front
/// (`MacBackend::terminalAppScript`).
pub fn terminal_app_script(command: &str) -> String {
    format!("tell application \"Terminal\"\n    do script \"{}\"\n    activate\nend tell", apple_script_escape(command))
}

/// Qt's tray message on macOS (`MacBackend::notificationScript`).
pub fn notification_script(title: &str, body: &str) -> String {
    format!(
        "display notification \"{}\" with title \"{}\"",
        apple_script_escape(body),
        apple_script_escape(title)
    )
}

/// A copied command as its notice shows it: at most 120 characters, cut in
/// the middle (`ClipboardBackend::notificationText`).
pub fn notification_text(command: &str) -> String {
    const MAX: usize = 120;
    const ELLIPSIS: &str = " \u{2026} ";
    let chars: Vec<char> = command.chars().collect();
    if chars.len() <= MAX {
        return command.to_owned();
    }
    let keep = MAX - ELLIPSIS.chars().count();
    let (head, tail) = (keep / 2, keep - keep / 2);
    let mut shown: String = chars[..head].iter().collect();
    shown.push_str(ELLIPSIS);
    shown.extend(&chars[chars.len() - tail..]);
    shown
}
```

- [ ] **Step 4: Run the core tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml -- trayicon shell::`
Expected: PASS.

The core part stands alone and is the task's first reviewable unit. Run core fmt, core clippy and the core tests, then commit it:

```bash
git add client/core/src/trayicon.rs client/core/src/shell.rs client/core/src/lib.rs
git commit -m "feat(client): Qt's menu-bar tile geometry and AppleScript strings in the core"
```

- [ ] **Step 5: Write the failing src-tauri tests**

In `client/src-tauri/src/pump.rs`, add to its tests:

```rust
    #[test]
    fn the_tap_sees_every_change() {
        let pump = Pump::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        pump.tap(Box::new(move |change: &Change| {
            let _ = sender.send(change.computer().clone());
        }));
        pump.push(Change::Computer { computer: ComputerId::from("mac"), status: ComputerStatus::Live });
        assert_eq!(receiver.try_recv(), Ok(ComputerId::from("mac")));
    }
```

Create `client/src-tauri/src/desk.rs` with only this test module (`#[cfg(feature = "live")] mod desk;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zcd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_open_mode_prefers_the_clients_file_then_tray_conf() {
        let dir = scratch("mode");
        assert_eq!(load_mode(&dir, None), OpenMode::Terminal);
        assert_eq!(load_mode(&dir, Some("open-mode=clipboard\n")), OpenMode::Clipboard);
        assert_eq!(load_mode(&dir, Some("open-mode=tab\n")), OpenMode::Terminal, "a Linux mode is not a macOS mode");
        save_mode(&dir, OpenMode::Terminal).unwrap();
        assert_eq!(load_mode(&dir, Some("open-mode=clipboard\n")), OpenMode::Terminal);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(MODE_FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn pending_notices_are_bounded() {
        let mut pending = Pending::default();
        for index in 0..10 {
            pending.notice(Notice { title: "Zerus".into(), text: index.to_string() });
        }
        assert_eq!(pending.notices.len(), PENDING_NOTICES);
        assert_eq!(pending.notices[0].text, "2");
    }

    #[test]
    fn the_desk_listens_to_rows_statuses_and_cards_only() {
        use zerus_client_core::{ActivityUpdate, ComputerStatus};
        assert!(menu_bar_change(&Change::Computer { computer: ComputerId::local(), status: ComputerStatus::Live }));
        assert!(!menu_bar_change(&Change::Activity {
            computer: ComputerId::local(),
            key: "k".into(),
            update: ActivityUpdate::Ended,
        }));
    }
}
```

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- desk:: the_tap_sees`
Expected: FAIL to compile (`tap`, `load_mode`, `save_mode`, `Pending`, `Notice`, `menu_bar_change`, `MODE_FILE` are missing).

- [ ] **Step 6: Add the pump tap**

In `client/src-tauri/src/pump.rs`:

```rust
/// Receives every change before the coalescer folds it (the desk thread).
pub type Tap = Box<dyn Fn(&Change) + Send>;
```

Add `tap: Option<Tap>,` to `struct Inner`. In `Pump::push`, call it first:

```rust
    pub fn push(&self, change: Change) {
        let mut inner = self.lock();
        if let Some(tap) = &inner.tap {
            tap(&change);
        }
        inner.mirror.observe(&change);
        inner.coalescer.push(change);
        inner.dirty = true;
        drop(inner);
        self.0 .1.notify_all();
    }

    /// Hands every later change to `tap` as well.
    pub fn tap(&self, tap: Tap) {
        self.lock().tap = Some(tap);
    }
```

- [ ] **Step 7: Write `desk.rs` above its tests**

As shipped: `Inbox` (with the page generation), `store_mode` on the desk thread and `page_loading` supersede these snippets.

```rust
//! The menu-bar item's data and actions. The pump taps every change into a
//! thread that keeps a [`Desk`] and, at most every [`PACE`], offers the
//! status item its face, tooltip and menu on the main thread. Menu actions
//! open the window, open a running session in Terminal.app or copy its
//! command (Qt's "Open in"), open a saved session's row, or quit. A session
//! or notice the webview cannot take yet waits in [`Pending`] until it asks.
// The menu bar exists only on macOS in this phase; Linux keeps the Qt GUI.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use zerus_client_core::menubar::{tray_conf_value, Desk, OpenMode};
use zerus_client_core::shell;
use zerus_client_core::{Change, ComputerId};

use crate::MAIN_WINDOW;

/// The status item is offered at most this often.
pub const PACE: Duration = Duration::from_millis(250);
/// The client's own "Open in" choice, in its data directory.
pub const MODE_FILE: &str = "menubar.json";
/// At most this many notices wait for the webview.
pub const PENDING_NOTICES: usize = 8;

pub enum DeskInput {
    Change(Box<Change>),
    Mode(OpenMode),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSession {
    pub computer: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub open: Option<OpenSession>,
    pub notices: Vec<Notice>,
}

impl Pending {
    #[allow(dead_code)] // Task 6's notifier is its first caller.
    pub fn notice(&mut self, notice: Notice) {
        if self.notices.len() == PENDING_NOTICES {
            self.notices.remove(0);
        }
        self.notices.push(notice);
    }
}

/// Kept in the app state.
pub struct DeskState {
    sender: Mutex<Sender<DeskInput>>,
    mode: Mutex<OpenMode>,
    dir: PathBuf,
    pending: Mutex<Pending>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Serialize, Deserialize)]
struct Saved {
    open_mode: OpenMode,
}

/// The "Open in" choice: the client's own file, else `open-mode` from Qt's
/// `tray.conf` (read only), else Terminal.app (Decision 10).
pub fn load_mode(dir: &Path, tray_conf: Option<&str>) -> OpenMode {
    std::fs::read(dir.join(MODE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        .map(|saved| saved.open_mode)
        .or_else(|| {
            tray_conf
                .and_then(|text| tray_conf_value(text, "open-mode"))
                .and_then(|value| OpenMode::from_config(&value))
        })
        .unwrap_or_default()
}

pub fn save_mode(dir: &Path, mode: OpenMode) -> std::io::Result<()> {
    write_private(&dir.join(MODE_FILE), &serde_json::to_vec(&Saved { open_mode: mode })?)
}

/// Writes a private (0600) file through a temporary file and a rename.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// `~/.config/hgs/tray.conf` (`AppConfig::configPath`), read only.
pub fn tray_conf() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    std::fs::read_to_string(Path::new(&home).join(".config/hgs/tray.conf")).ok()
}

/// The changes the menu bar reads.
pub fn menu_bar_change(change: &Change) -> bool {
    matches!(
        change,
        Change::Upsert { .. } | Change::Remove { .. } | Change::Replace { .. } | Change::Computer { .. } | Change::Machine { .. }
    )
}

/// Taps the pump and starts the desk thread. Call before the fleet starts,
/// so the first catalog reaches the menu bar.
pub fn start(app: &AppHandle, pump: &crate::pump::Pump, dir: PathBuf, mode: OpenMode) {
    let (sender, inputs) = mpsc::channel();
    let tap = sender.clone();
    pump.tap(Box::new(move |change: &Change| {
        if menu_bar_change(change) {
            let _ = tap.send(DeskInput::Change(Box::new(change.clone())));
        }
    }));
    app.manage(DeskState {
        sender: Mutex::new(sender),
        mode: Mutex::new(mode),
        dir,
        pending: Mutex::new(Pending::default()),
    });
    let handle = app.clone();
    std::thread::Builder::new()
        .name("zerus-client-desk".into())
        .spawn(move || run(handle, inputs, mode))
        .expect("the desk thread starts");
}

fn run(app: AppHandle, inputs: Receiver<DeskInput>, mut mode: OpenMode) {
    let mut desk = Desk::default();
    while let Ok(first) = inputs.recv() {
        for input in std::iter::once(first).chain(std::iter::from_fn(|| inputs.try_recv().ok())) {
            match input {
                DeskInput::Change(change) => {
                    desk.observe(&change);
                }
                DeskInput::Mode(chosen) => mode = chosen,
            }
        }
        publish(&app, &desk, mode);
        std::thread::sleep(PACE);
    }
}

fn publish(app: &AppHandle, desk: &Desk, mode: OpenMode) {
    #[cfg(target_os = "macos")]
    {
        let (face, tooltip, menu, badge) = (desk.face(), desk.tooltip(), desk.menu(mode), desk.badge());
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || crate::platform::macos_menubar::offer(&handle, face, tooltip, menu, badge));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, desk, mode);
}

/// What the webview could not take yet; it asks once when it loads.
#[tauri::command]
pub fn desk_take_pending(app: AppHandle) -> Pending {
    app.try_state::<DeskState>()
        .map(|state| std::mem::take(&mut *lock(&state.pending)))
        .unwrap_or_default()
}

/// Opens the window on a session's row (`TrayAgent.cpp:214-216`).
pub fn open_session(app: &AppHandle, computer: &ComputerId, key: &str) {
    let target = OpenSession { computer: computer.0.clone(), key: key.to_owned() };
    if app.get_webview_window(MAIN_WINDOW).is_none() {
        if let Some(state) = app.try_state::<DeskState>() {
            lock(&state.pending).open = Some(target.clone());
        }
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(error) = crate::lifecycle::show_main(&handle) {
            eprintln!("zerus-client: could not open the window: {error}");
        }
        let _ = handle.emit("open-session", target);
    });
}

/// A notice inside the window (Qt's `showNotificationNotice`).
#[allow(dead_code)] // Task 6's notifier is its first caller.
pub fn window_notice(app: &AppHandle, title: &str, text: &str) {
    let notice = Notice { title: title.to_owned(), text: text.to_owned() };
    if app.get_webview_window(MAIN_WINDOW).is_some() {
        let _ = app.emit("desk-notice", notice);
    } else if let Some(state) = app.try_state::<DeskState>() {
        lock(&state.pending).notice(notice);
    }
}

/// Qt's `TrayAgent::notify` on macOS: `osascript display notification`,
/// shown at once without the app's own permission (`TrayAgent.cpp:796-830`).
pub fn tray_notify(title: &str, body: &str) {
    let spawned = std::process::Command::new("osascript")
        .arg("-e")
        .arg(shell::notification_script(title, body))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => eprintln!("zerus-client: {title}: {body} ({error})"),
    }
}

/// A running session's entry: Terminal.app or the clipboard, as "Open in"
/// says (`TrayAgent::activateSessionWith`, `TrayAgent.cpp:631-659`).
pub fn open_running(app: &AppHandle, computer: &ComputerId, name: &str) {
    let hgs = zerus_client_core::default_hgs();
    let command = shell::open_command(&hgs.to_string_lossy(), &computer.0, name, None, shell::current_shell());
    let mode = app.try_state::<DeskState>().map(|state| *lock(&state.mode)).unwrap_or_default();
    match mode {
        OpenMode::Terminal => {
            if let Err(error) = open_in_terminal(&command) {
                tray_notify(
                    "Zerus: the terminal did not open",
                    &format!("{error}\n\nCommand: {command}\nRun it in a terminal by hand."),
                );
            }
        }
        OpenMode::Clipboard => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            let copied = shell::copyable(&command)
                .and_then(|()| app.clipboard().write_text(command.clone()).map_err(|error| error.to_string()));
            match copied {
                Ok(()) => tray_notify("Zerus: copied to the clipboard", &shell::notification_text(&command)),
                Err(error) => tray_notify("Zerus: could not copy", &error),
            }
        }
    }
}

/// `osascript` with Qt's Terminal.app script, detached (`MacBackend::open`).
fn open_in_terminal(command: &str) -> std::io::Result<()> {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
    let mut child = std::process::Command::new("osascript")
        .arg("-e")
        .arg(shell::terminal_app_script(command))
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The "Open in" choice: applied at once, then saved (`TrayAgent::setOpenMode`).
pub fn set_mode(app: &AppHandle, mode: OpenMode) {
    let Some(state) = app.try_state::<DeskState>() else { return };
    *lock(&state.mode) = mode;
    if let Err(error) = save_mode(&state.dir, mode) {
        tray_notify(
            "Zerus: mode applied, but not saved",
            &format!(
                "New sessions already open the new way, but the choice could not be written to {}: {error}",
                state.dir.join(MODE_FILE).display()
            ),
        );
    }
    let _ = lock(&state.sender).send(DeskInput::Mode(mode));
}
```

- [ ] **Step 8: Run the src-tauri tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- desk:: the_tap_sees`
Expected: PASS (4 tests).

- [ ] **Step 9: Write the macOS status item**

Create `client/src-tauri/src/platform/macos_menubar.rs` and add `#[cfg(all(target_os = "macos", feature = "live"))] pub mod macos_menubar;` to `platform/mod.rs`. The menu bar drives the desk thread, which exists only with the live data source; without the `live` gate, `--no-default-features` fails to compile on macOS (`crate::desk` and `zerus_client_core` are missing there):

```rust
//! The status item on macOS: Tauri's tray icon carrying Qt's tile and menu
//! (`tray/src/QtTrayIcon.cpp`, `TrayMenu.cpp`). Updates go through
//! [`Gate`]: only changes, and nothing while a menu is open (`QuietTrayIcon.h`).
//! A primary click opens Zerus; the secondary click opens the menu
//! (`QtTrayIcon.cpp:12-20`).
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBezierPath, NSBitmapImageRep, NSColor,
    NSDeviceRGBColorSpace, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext,
    NSMenuDidBeginTrackingNotification, NSMenuDidEndTrackingNotification, NSStringDrawing,
};
use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSDictionary, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString,
};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use zerus_client_core::menubar::{Apply, Entry, Face, Gate, OpenMode};
use zerus_client_core::trayicon::{self, Rgb, Step, Tile};
use zerus_client_core::ComputerId;

/// The tray's id in Tauri.
pub const TRAY: &str = "zerus";
/// 18 points at 2x: tray-icon sizes every image to 18 points high.
pub const TILE_PIXELS: usize = 36;

#[derive(Clone, Debug)]
pub enum MenuAction {
    Open,
    Session { computer: ComputerId, key: String, name: String, running: bool },
    Mode(OpenMode),
    Quit,
}

struct Bar {
    menu: Menu<Wry>,
    gate: Gate,
    actions: HashMap<String, MenuAction>,
    configured: Option<Rgb>,
    /// The Dock badge last set; None before the first offer.
    badge: Option<Option<String>>,
}

pub struct MenuBarState(Mutex<Bar>);

fn lock(state: &MenuBarState) -> std::sync::MutexGuard<'_, Bar> {
    state.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Creates the status item with an empty fleet's tile and an empty menu;
/// the desk thread's first offer fills both.
pub fn install(app: &AppHandle, configured: Option<Rgb>) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    let blank = || Image::new_owned(vec![0; TILE_PIXELS * TILE_PIXELS * 4], TILE_PIXELS as u32, TILE_PIXELS as u32);
    TrayIconBuilder::with_id(TRAY)
        .icon(tile_image(&Face::counts(0, 0, false), configured).unwrap_or_else(blank))
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if let Err(error) = crate::lifecycle::show_main(tray.app_handle()) {
                    eprintln!("zerus-client: could not open the window: {error}");
                }
            }
        })
        .on_menu_event(|app, event| menu_event(app, event.id().as_ref()))
        .build(app)?;
    app.manage(MenuBarState(Mutex::new(Bar { menu, gate: Gate::default(), actions: HashMap::new(), configured, badge: None })));
    observe_tracking(app);
    Ok(())
}

/// The desk thread's latest state, on the main thread.
pub fn offer(app: &AppHandle, face: Face, tooltip: String, menu: Vec<Entry>, badge: Option<String>) {
    let Some(state) = app.try_state::<MenuBarState>() else { return };
    let changes = {
        let mut bar = lock(&state);
        // The Dock badge is not the status item: it may change while the menu is open.
        if bar.badge.as_ref() != Some(&badge) {
            if let Some(mtm) = MainThreadMarker::new() {
                let label = badge.as_deref().map(NSString::from_str);
                NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(label.as_deref());
            }
            bar.badge = Some(badge);
        }
        bar.gate.offer(face, tooltip, menu)
    };
    apply(app, &state, changes);
}

fn apply(app: &AppHandle, state: &MenuBarState, changes: Vec<Apply>) {
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    let mut bar = lock(state);
    for change in changes {
        let result = match change {
            Apply::Face(face) => match tile_image(&face, bar.configured) {
                Some(image) => tray.set_icon(Some(image)),
                None => Ok(()),
            },
            Apply::Tooltip(text) => tray.set_tooltip(Some(text)),
            Apply::Menu(entries) => rebuild(app, &mut bar, &entries),
        };
        if let Err(error) = result {
            eprintln!("zerus-client: menu bar: {error}");
        }
    }
}

/// Rebuilds the one menu in place, so the status item keeps its NSMenu.
fn rebuild(app: &AppHandle, bar: &mut Bar, entries: &[Entry]) -> tauri::Result<()> {
    while bar.menu.remove_at(0)?.is_some() {}
    bar.actions.clear();
    for (index, entry) in entries.iter().enumerate() {
        let id = format!("zerus-{index}");
        match entry {
            Entry::Open => {
                bar.menu.append(&MenuItem::with_id(app, id.as_str(), "Open Zerus", true, None::<&str>)?)?;
                bar.actions.insert(id, MenuAction::Open);
            }
            Entry::Separator => bar.menu.append(&PredefinedMenuItem::separator(app)?)?,
            Entry::Label(text) => bar.menu.append(&MenuItem::with_id(app, id.as_str(), text, false, None::<&str>)?)?,
            Entry::Session { computer, key, name, text, running } => {
                bar.menu.append(&MenuItem::with_id(app, id.as_str(), text, true, None::<&str>)?)?;
                bar.actions.insert(
                    id,
                    MenuAction::Session { computer: computer.clone(), key: key.clone(), name: name.clone(), running: *running },
                );
            }
            Entry::OpenIn(chosen) => {
                let submenu = Submenu::with_id(app, id.as_str(), "Open in", true)?;
                for mode in OpenMode::ALL {
                    let item = format!("{id}-{}", mode.label());
                    submenu.append(&CheckMenuItem::with_id(app, item.as_str(), mode.label(), true, mode == *chosen, None::<&str>)?)?;
                    bar.actions.insert(item, MenuAction::Mode(mode));
                }
                bar.menu.append(&submenu)?;
            }
            Entry::Quit => {
                bar.menu.append(&MenuItem::with_id(app, id.as_str(), "Quit", true, None::<&str>)?)?;
                bar.actions.insert(id, MenuAction::Quit);
            }
        }
    }
    Ok(())
}

pub fn menu_event(app: &AppHandle, id: &str) {
    let action = app.try_state::<MenuBarState>().and_then(|state| lock(&state).actions.get(id).cloned());
    match action {
        Some(MenuAction::Open) => {
            if let Err(error) = crate::lifecycle::show_main(app) {
                eprintln!("zerus-client: could not open the window: {error}");
            }
        }
        Some(MenuAction::Session { computer, key, running: false, .. }) => crate::desk::open_session(app, &computer, &key),
        Some(MenuAction::Session { computer, name, running: true, .. }) => crate::desk::open_running(app, &computer, &name),
        Some(MenuAction::Mode(mode)) => crate::desk::set_mode(app, mode),
        Some(MenuAction::Quit) => app.exit(0),
        None => {}
    }
}

/// NSMenu posts these around every tracking session, the status item's
/// included; another menu of the app only delays an update. The observers
/// live as long as the app.
fn observe_tracking(app: &AppHandle) {
    let depth = Arc::new(AtomicUsize::new(0));
    let (began_depth, began_app) = (Arc::clone(&depth), app.clone());
    let began = RcBlock::new(move |_note: NonNull<NSNotification>| {
        if began_depth.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(state) = began_app.try_state::<MenuBarState>() {
                lock(&state).gate.menu_opened();
            }
        }
    });
    let ended_app = app.clone();
    let ended = RcBlock::new(move |_note: NonNull<NSNotification>| {
        let before = depth.try_update(Ordering::SeqCst, Ordering::SeqCst, |d| Some(d.saturating_sub(1))).unwrap_or(0);
        if before == 1 {
            let handle = ended_app.clone();
            // After the tracking loop has unwound.
            let _ = ended_app.run_on_main_thread(move || {
                if let Some(state) = handle.try_state::<MenuBarState>() {
                    let changes = lock(&state).gate.menu_closed();
                    apply(&handle, &state, changes);
                }
            });
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: AppKit's constant notification names; the blocks capture only Send state.
    unsafe {
        let first = center.addObserverForName_object_queue_usingBlock(Some(NSMenuDidBeginTrackingNotification), None, None, &began);
        let second = center.addObserverForName_object_queue_usingBlock(Some(NSMenuDidEndTrackingNotification), None, None, &ended);
        std::mem::forget((first, second));
    }
}

fn tile_image(face: &Face, configured: Option<Rgb>) -> Option<Image<'static>> {
    let dark = MainThreadMarker::new().is_some_and(dark_appearance);
    let tile = trayicon::tile(face, TILE_PIXELS as f64, trayicon::foreground(dark, configured));
    Some(Image::new_owned(render(&tile)?, TILE_PIXELS as u32, TILE_PIXELS as u32))
}

/// Qt follows the application palette (`IconFactory::foreground`).
fn dark_appearance(mtm: MainThreadMarker) -> bool {
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    // SAFETY: AppKit's constant appearance names.
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    appearance
        .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark]))
        .is_some_and(|name| name.isEqualToString(dark))
}

/// Draws the tile into a fresh bitmap and returns straight RGBA, top row first.
pub fn render(tile: &Tile) -> Option<Vec<u8>> {
    let pixels = tile.size as isize;
    // SAFETY: a bitmap AppKit allocates for our size; we read it after drawing.
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), pixels, pixels, 8, 4, true, false, NSDeviceRGBColorSpace, pixels * 4, 32,
        )?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        draw(tile);
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let data = rep.bitmapData();
        if data.is_null() {
            return None;
        }
        let (stride, width) = (rep.bytesPerRow() as usize, tile.size as usize);
        let mut rgba = Vec::with_capacity(width * width * 4);
        for row in 0..width {
            rgba.extend_from_slice(std::slice::from_raw_parts(data.add(row * stride), width * 4));
        }
        trayicon::unpremultiply(&mut rgba);
        Some(rgba)
    }
}

/// AppKit's y axis points up; the tile's geometry points down.
fn draw(tile: &Tile) {
    let size = tile.size;
    let rect = |r: trayicon::Rect| NSRect::new(NSPoint::new(r.x, size - r.y - r.h), NSSize::new(r.w, r.h));
    fill_rounded(rect(tile.upper), tile.radius, tile.surface);
    fill_rounded(rect(tile.lower), tile.radius, tile.lower_fill);
    if let Some(mark) = tile.mark {
        let point = |(x, y): (f64, f64)| NSPoint::new(mark.x + x * mark.w / 100.0, size - (mark.y + y * mark.h / 100.0));
        let path = NSBezierPath::bezierPath();
        for step in trayicon::mark_path() {
            match step {
                Step::Move(x, y) => path.moveToPoint(point((x, y))),
                Step::Curve { c1, c2, to } => path.curveToPoint_controlPoint1_controlPoint2(point(to), point(c1), point(c2)),
                Step::Close => path.closePath(),
            }
        }
        color(tile.foreground).setFill();
        path.fill();
    }
    draw_text(&tile.total, tile.font_pixels, size);
    draw_text(&tile.attention, tile.font_pixels, size);
    if let Some((x, y, r)) = tile.dot {
        color(tile.foreground).setFill();
        NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(x - r, size - y - r), NSSize::new(2.0 * r, 2.0 * r))).fill();
    }
}

/// Bold, shrunk until the count fits its box, centred on the cap height
/// (`IconFactory.cpp:130-141`).
fn draw_text(text: &trayicon::Text, start_pixels: f64, size: f64) {
    let string = NSString::from_str(&text.text);
    let mut pixels = start_pixels;
    loop {
        let font = NSFont::boldSystemFontOfSize(pixels);
        let attributes = attributes(&font, text.color);
        // SAFETY: the dictionary holds a font and a colour under their keys.
        let width = unsafe { string.sizeWithAttributes(Some(&attributes)) }.width;
        if (width <= text.rect.w && font.capHeight() <= text.rect.h) || pixels <= 5.0 {
            let x = text.rect.x + (text.rect.w - width) / 2.0;
            let baseline = size - (text.rect.y + (text.rect.h + font.capHeight()) / 2.0);
            // SAFETY: as above.
            unsafe { string.drawAtPoint_withAttributes(NSPoint::new(x, baseline + font.descender()), Some(&attributes)) };
            return;
        }
        pixels -= 1.0;
    }
}

fn attributes(font: &NSFont, rgb: Rgb) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let color = color(rgb);
    // SAFETY: AppKit's constant attribute names.
    let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [font, &color];
    NSDictionary::from_slices(&keys, &values)
}

fn color(rgb: Rgb) -> Retained<NSColor> {
    let channel = |value: u8| f64::from(value) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(channel(rgb.r), channel(rgb.g), channel(rgb.b), channel(rgb.a))
}

fn fill_rounded(rect: NSRect, radius: f64, rgb: Rgb) {
    color(rgb).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_draws_amber_attention_and_the_stale_dot() {
        let tile = trayicon::tile(&Face::counts(12, 3, true), TILE_PIXELS as f64, Rgb::new(255, 255, 255));
        let rgba = render(&tile).expect("AppKit draws into a bitmap without a display");
        assert_eq!(rgba.len(), TILE_PIXELS * TILE_PIXELS * 4);
        let pixel = |x: usize, y: usize| rgba[(y * TILE_PIXELS + x) * 4..][..4].to_vec();
        let amber = pixel(3, 33);
        assert!(amber[0] > 200 && amber[1] > 150 && amber[2] < 150 && amber[3] == 255, "{amber:?}");
        assert!(pixel(TILE_PIXELS - 2, 2)[0] > 200, "the white dot on the dark surface");
        assert_eq!(pixel(18, 18)[3], 0, "the gap between the rows stays clear");
    }
}
```

In `client/src-tauri/Cargo.toml`:
- Make `tauri = { version = "2", features = ["tray-icon"] }`.
- In the macOS table, add `block2 = "0.6.2"` and `objc2-app-kit = { version = "0.3.2", default-features = false, features = ["std", "objc2-core-foundation", "objc2-core-graphics", "NSAppearance", "NSApplication", "NSAttributedString", "NSBezierPath", "NSBitmapImageRep", "NSColor", "NSDockTile", "NSFont", "NSGraphics", "NSGraphicsContext", "NSImageRep", "NSMenu", "NSResponder", "NSStringDrawing"] }`.
- Extend the `objc2-foundation` features with `"block2", "NSAttributedString", "NSDictionary", "NSGeometry", "NSNotification", "NSOperation"`.

Record the new direct dependencies in the lock (Global Constraints, "Cargo.lock") and confirm that `git diff client/src-tauri/Cargo.lock` adds no `[[package]]`. If the Mac compiler names another missing feature, add exactly that one; the feature lists above passed `cargo clippy --all-targets` on `aarch64-apple-darwin`.

- [ ] **Step 10: Install the menu bar in `launch`**

In the `#[cfg(feature = "live")]` block of the setup closure, replace the two `pump` lines and the `start_fleet` call with:

```rust
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump.clone()));
                #[cfg(target_os = "macos")]
                {
                    let conf = desk::tray_conf();
                    let configured = conf
                        .as_deref()
                        .and_then(|text| zerus_client_core::menubar::tray_conf_value(text, "icon-color"))
                        .and_then(|value| {
                            let color = zerus_client_core::trayicon::parse_color(&value);
                            if color.is_none() {
                                eprintln!("zerus-client: icon-color {value:?} in tray.conf is not #rgb or #rrggbb; using the menu bar's colour");
                            }
                            color
                        });
                    platform::macos_menubar::install(app.handle(), configured)?;
                    desk::start(app.handle(), &pump, dir.clone(), desk::load_mode(&dir, conf.as_deref()));
                }
                if menu_bar {
                    lifecycle::start_fleet(|| app.state::<commands::AppState>().start_source());
                }
```

Add `desk::desk_take_pending,` to the live `generate_handler!` list in `handler()`.

- [ ] **Step 11: Write the failing frontend test**

Create `client/src/store/desk.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as bridge from "../bridge";
import * as ui from "./ui";
import { startDesk } from "./desk";

vi.mock("../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge")>()),
  onOpenSession: vi.fn(() => () => {}),
  onDeskNotice: vi.fn(() => () => {}),
  deskTakePending: vi.fn(async () => ({ open: null, notices: [] })),
}));
// store/ui reads the page URL when it loads; the wiring needs only its setters.
vi.mock("./ui", () => ({ setPage: vi.fn(), setSelectedKey: vi.fn(), toast: vi.fn() }));
const mocked = vi.mocked(bridge);
const shown = vi.mocked(ui);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("desk wiring", () => {
  beforeEach(() => vi.clearAllMocks());

  it("opens the session the menu bar or a notification names", () => {
    let open: ((target: bridge.OpenSession) => void) | undefined;
    mocked.onOpenSession.mockImplementation((listener) => {
      open = listener;
      return () => {};
    });
    const stop = startDesk();
    open!({ computer: "mac", key: "claude/sample-project/review" });
    expect(shown.setPage).toHaveBeenCalledWith("sessions");
    expect(shown.setSelectedKey).toHaveBeenCalledWith("mac\nclaude/sample-project/review");
    stop();
  });

  it("applies what arrived before the page loaded", async () => {
    mocked.deskTakePending.mockResolvedValueOnce({
      open: { computer: "", key: "k1" },
      notices: [{ title: "Zerus", text: "This session is no longer available: claude/sample-project/x" }],
    });
    startDesk();
    await settle();
    expect(shown.setSelectedKey).toHaveBeenCalledWith("\nk1");
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "This session is no longer available: claude/sample-project/x");
  });

  it("shows notices as toasts", () => {
    let notice: ((value: bridge.DeskNotice) => void) | undefined;
    mocked.onDeskNotice.mockImplementation((listener) => {
      notice = listener;
      return () => {};
    });
    startDesk();
    notice!({ title: "Zerus", text: "System notification was not delivered: denied" });
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "System notification was not delivered: denied");
  });
});
```

Run: `cd client && npx vitest run src/store/desk.test.ts`
Expected: FAIL (`./desk` does not exist; `onOpenSession`, `onDeskNotice`, `deskTakePending` are not exported).

- [ ] **Step 12: Add the bridge events and the desk store**

In `client/src/bridge.ts`, add after `onScreensChanged`:

```ts
/** A session the menu bar or a notification opens: its computer and catalog key. */
export type OpenSession = { computer: ComputerId; key: string };
/** A notice for the window. */
export type DeskNotice = { title: string; text: string };
export type DeskPending = { open: OpenSession | null; notices: DeskNotice[] };

function onEvent<T>(name: string, listener: (payload: T) => void): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  let stopped = false;
  void listen<T>(name, (event) => listener(event.payload)).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
  return () => {
    stopped = true;
    stop?.();
  };
}

/** `open-session`: the menu bar or a notification opens a session; returns the stop. */
export const onOpenSession = (listener: (target: OpenSession) => void) => onEvent<OpenSession>("open-session", listener);
/** `desk-notice`: a notice for the window; returns the stop. */
export const onDeskNotice = (listener: (notice: DeskNotice) => void) => onEvent<DeskNotice>("desk-notice", listener);

/** What the menu bar or a notification asked for before this page loaded. */
export async function deskTakePending(): Promise<DeskPending> {
  if (!inTauri()) return { open: null, notices: [] };
  return invoke<DeskPending>("desk_take_pending");
}
```

Create `client/src/store/desk.ts`:

```ts
import { deskTakePending, onDeskNotice, onOpenSession, type OpenSession } from "../bridge";
import { rowId } from "./catalog";
import { setPage, setSelectedKey, toast } from "./ui";

// The menu bar and attention notifications open sessions and show notices
// through the native side (`src-tauri/src/desk.rs`).

/** Shows the session's row and its Activity (Qt's `showAttentionSession`). */
export function openSession(target: OpenSession): void {
  setPage("sessions");
  setSelectedKey(rowId(target.computer, target.key));
}

/** Starts the wiring; returns its stop. */
export function startDesk(): () => void {
  const stopOpen = onOpenSession(openSession);
  const stopNotice = onDeskNotice((notice) => toast(notice.title, notice.text));
  void deskTakePending().then(
    (pending) => {
      if (pending.open) openSession(pending.open);
      for (const notice of pending.notices) toast(notice.title, notice.text);
    },
    (error) => console.warn("Zerus Next: desk_take_pending failed", error),
  );
  return () => {
    stopOpen();
    stopNotice();
  };
}
```

In `client/src/main.tsx`, import `startDesk` from `./store/desk` and call `startDesk();` after `render(...)`.

- [ ] **Step 13: Run the frontend checks**

Run: `cd client && npm run check && npx vitest run src/store/desk.test.ts && npm test`
Expected: PASS.

- [ ] **Step 14: Check on the Mac, display-free**

Run **mac-lock**, **mac-sync**, **mac-clippy**, **mac-test** and **mac-done**.
Expected: clean, and `macos_menubar::tests::the_tile_draws_amber_attention_and_the_stale_dot` passes over SSH. If AppKit refuses to draw without a window server session, the test fails with `None`. In that case mark it `#[ignore = "needs the owner's display window"]` and list it in the runbook's display checks; do not weaken its assertions.

- [ ] **Step 15: Run the task's checks and commit**

Run fmt, both clippy variants on Linux, the core and src-tauri tests, `npm run check`, `npm test`, `check-ui-separators.py` and `check-source-privacy.py`.

```bash
git add client/src-tauri/src/pump.rs client/src-tauri/src/desk.rs client/src-tauri/src/platform \
  client/src-tauri/src/lib.rs client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock \
  client/src/bridge.ts client/src/main.tsx client/src/store/desk.ts client/src/store/desk.test.ts
git commit -m "feat(client): Qt's menu-bar item on macOS with its tile, menu and Open in"
```

---

### Task 5: Core attention rules: a port of Qt's AttentionTracker (zerus-5wmh.43)

The client decides what to notify, withdraw and open exactly as Qt does. Qt's parity rules, pinned by source line:

| Rule | Qt source |
| --- | --- |
| A request that needs action notifies once per phase and request; reasons "Approval needed", "Input needed", the recovery reason, the activity detail or "Agent needs attention" (concise 240) | `AttentionTracker.cpp:165-173`, `HgsClient.h:109-116` |
| An acknowledged request (the host's read mark names its attention signature) or one in the host's suppressed baseline is recorded silently | `AttentionTracker.cpp:134-140`, `:171`, `:64-68` |
| Replies belong to conversations (`reply:<cmd>:<conversation>`); they notify only when idle (no background job, no active subagent, no coordination wait, nothing that needs action); a reply in progress keeps its entry; routine ticks of a recurring schedule are consumed | `AttentionTracker.cpp:174-192` |
| Children notify only for `approval`, `input` or `attention`, with "Subagent LABEL: Approval needed" or "Input needed", and not for hook profiles, exited, unknown or ended sessions | `AttentionTracker.cpp:193-205`, `SessionNotifications.h:85-96` |
| Optional questions are off by default (`notifications/optionalQuestions`), keep 64 seen IDs per conversation, and announce only IDs not seen; a request that needs action goes first | `AttentionTracker.cpp:207-280`, `QuestionNotificationSettings.h` |
| A focused Zerus consumes edges without banners | `AttentionTracker.cpp:157-160`, `:261` |
| Muted sessions record without tokens; an offline host withdraws only muted banners and keeps every identity | `AttentionTracker.cpp:90-116`, `:134-140` |
| A failed delivery retries 60 s later with a new token | `AttentionTracker.cpp:290-303`, `TrayAgent.cpp:195-201` |
| Titles "Zerus — Needs attention", "Zerus — New reply", "Zerus — Optional question"; body "NAME / COMPUTER\nREASON" (120 and 40 characters) | `AttentionTracker.cpp:155-160`, `:268-270` |
| Tokens: base64url JSON `{host, name, run, created, notice}` plus `reply`/`question`, `conversation`, `agent`; at most 8192 characters | `AttentionTracker.cpp:148-153`, `:305-312` |
| A click opens the session by conversation and agent (replies, questions), by run, or by name and creation time; archived rows never match; otherwise "This session is no longer available: NAME" after a current listing or 16 s | `AttentionTracker.cpp:314-322`, `TrayAgent.cpp:176-193`, `:593-608` |
| Delivery: one authorization check at a time, request it when not determined (alert and sound), "Allow Zerus notifications in System Settings → Notifications." when denied, a late add for a withdrawn token is removed | `MacAttentionNotifier.mm:51-114` |
| Shared notification preferences are validated strictly before they mute or suppress | `SessionNotifications.h:43-80` |

**Device marks in the menu bar's counts (coordinator, 2026-10-28; Task 4 review).** Task 1's `needs_attention` reads host values only, so the tile's attention count, the Dock badge and the tooltip ignore this device's marks. They must apply them the way `FleetState::updateUnread` and `attentionSessions` do, from the Rust `AttentionMarks` (`client/core/src/marks.rs`), so the counts are right while the window is closed and no webview runs `store/marks.ts`:
- **Review later** counts while not archived and not muted, also for a peer past the stale limit, which counts nothing else (`FleetState.cpp:141`, `:421-435`, `HgsClient.h:107-108`).
- **Local acknowledgement**: a mark whose `acknowledged` equals the row's `attention_fingerprint` acknowledges its attention on a host without shared read state, and for recovery-only attention without a host signature (`FleetState.cpp:142`, `:147`, `:160-161`).
- **Read replies and pending read marks**: a host without shared read state keeps the device's read reply per reply key; a queued read mark of the same run and conversation reads the reply or the attention until the host shows it (`FleetState.cpp:143-161`); `visibleUnreadReply` waits while a reply is in progress (`HgsClient.h:104-106`).

Task 5 ports these rules into the core as a pure overlay beside `menubar::needs_attention`. The Rust tests and `store/marks.ts`' tests read one shared JSON fixture, so the two overlays cannot drift. Task 6 wires it (below).

**Files:**
- Modify: `client/core/src/view.rs` (extend `NotifyFacts`, add `ChildFacts` and `Preferences`, fill them in `row()`)
- Create: `client/core/src/attention.rs`; modify `client/core/src/lib.rs` (`pub mod attention;`)
- Test: unit tests in `attention.rs` and `view.rs`

**Interfaces:**
- Consumes: `menubar::{Desk, Computer}` (Task 1).
- Produces:
  - `view::NotifyFacts { attached, needs_action, acknowledged, cmd, created, attention_id, attention_started, recovery_reason, activity_detail, unread_reply, reply_scheduled, recurring_schedules, background, subagents_active, coordination_wait, conversation_state, subagent_source, children: Vec<ChildFacts>, preferences: Option<Preferences>, optional_reported, optional_count, optional_ids }`.
  - `view::ChildFacts { id, label, phase, request, started: Option<f64> }`; `view::Preferences { muted, suppressed: Value }`.
  - `attention::{NEEDS_ATTENTION, NEW_REPLY, OPTIONAL_QUESTION, SEEN_QUESTIONS, RETRY_SECONDS, DENIED, CLICK_WAIT, Host { alias, label, ok, rows }, Notice { token, title, body }, Changes { raised, cleared }, Tracker::{new, set_optional_questions, state, observe(&Host, now: i64, focused: bool, &mut dyn FnMut() -> String) -> Changes, delivery_failed(&str, i64)}, target(&str) -> Option<Map>, matches(&Map, &ViewRow) -> bool, Resolution { Open { computer, key }, Gone(String), Waiting }, resolve(&Map, &Desk, expired: bool) -> Resolution, gone_text(&str) -> String, seed(Option<&[u8]>, Option<&[u8]>) -> Map, valid_preferences(&Value, &str) -> bool, request_json, concise, base64url, from_base64url, encode_token, Delivery::{post, withdraw, authorized, added}, DeliveryStep, request_id(&str) -> String}`. `base64url` and `from_base64url` wrap the core's `base64` crate (URL alphabet, no padding).

- [ ] **Step 1: Write the failing view test**

Add to the tests of `client/core/src/view.rs`:

```rust
    #[test]
    fn notify_facts_carry_what_the_attention_tracker_reads() {
        let view = row(&json!({
            "name": "claude/sample-project/review", "cmd": "dsh", "created": 1700000000.0, "tracked": true,
            "state": "running", "phase": "idle", "activity": "idle", "run_id": "r", "conversation_id": "c",
            "attention_id": "a-1", "attention_started": 1761600000.5,
            "recovery": {"state": "blocked", "reason": "Provider\nfailed"}, "activity_detail": "waiting",
            "reply_id": "rep-2", "attention_signature": "sig", "read": {"conversation_id": "c", "reply_id": "rep-1"},
            "reply_scheduled": true, "schedules": {"count": 2, "recurring": 1}, "process_work": {"count": 0},
            "subagent_active_count": 1, "conversation_state": "active", "subagent_source": "native",
            "subagents": {
                "b": {"state": "running"},
                "a": {"display_state": "approval", "attention_id": "q-1", "label": "Reviewer", "attention_started": 5.0}
            },
            "optional_questions": {"count": 2, "ids": ["q2", "q1", "q2", ""]},
            "notification_preferences": {"conversation_id": "c", "muted": true, "revision": 3, "suppressed": {
                "reply_id": null, "attention_signature": null, "optional_question_ids": [],
                "main_attention": null, "child_attention": {}}}
        }));
        let facts = &view.notify;
        assert_eq!(facts.cmd, "dsh", "conversation keys use the raw cmd");
        assert_eq!(facts.created, 1700000000);
        assert_eq!(facts.attention_id, "a-1");
        assert_eq!(facts.attention_started, 1761600000.5);
        assert!(facts.needs_action, "a blocked recovery needs action");
        assert_eq!(facts.recovery_reason, "Provider\nfailed");
        assert!(facts.unread_reply);
        assert!(facts.reply_scheduled);
        assert_eq!((facts.recurring_schedules, facts.background, facts.subagents_active), (1, 0, 1));
        assert_eq!(facts.children, vec![ChildFacts {
            id: "a".into(), label: "Reviewer".into(), phase: "approval".into(), request: "q-1".into(), started: Some(5.0),
        }]);
        assert!(facts.optional_reported);
        assert_eq!((facts.optional_count, facts.optional_ids.clone()), (2, vec!["q2".to_owned(), "q1".to_owned()]));
        assert!(facts.preferences.as_ref().is_some_and(|preferences| preferences.muted));
        let other = row(&json!({"name": "x", "conversation_id": "c", "notification_preferences": {
            "conversation_id": "other", "muted": true, "revision": 0, "suppressed": {}}}));
        assert_eq!(other.notify.preferences, None, "another conversation's preferences are not this row's");
        let unknown = row(&json!({"name": "x", "optional_questions": {"count": null}}));
        assert_eq!((unknown.notify.optional_reported, unknown.notify.optional_count), (true, -1));
    }
```

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml notify_facts_carry_what`
Expected: FAIL to compile (missing fields, `ChildFacts`, `Preferences`).

- [ ] **Step 2: Extend `NotifyFacts` and its projection**

Replace `NotifyFacts` in `client/core/src/view.rs` with:

```rust
/// What the menu bar and attention notifications read beyond the card,
/// with Qt's field rules (`tray/src/HgsClient.cpp:155-260`). It is never
/// serialized: the frontend does not receive it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotifyFacts {
    /// `attached > 0`: a terminal client is attached (`TrayMenu.cpp:30` shows "● ").
    pub attached: bool,
    /// `SessionInfo::needsAction` (`tray/src/HgsClient.h:109-116`).
    pub needs_action: bool,
    /// The host's read mark names the current attention signature.
    pub acknowledged: bool,
    /// The raw `cmd` (`dsh` stays `dsh`): conversation keys use it.
    pub cmd: String,
    pub created: i64,
    pub attention_id: String,
    /// `attention_started`, 0 when absent.
    pub attention_started: f64,
    /// `recovery.reason`.
    pub recovery_reason: String,
    pub activity_detail: String,
    /// The unread reply before a reply in progress hides it (`unreadReply`).
    pub unread_reply: bool,
    pub reply_scheduled: bool,
    /// `schedules.recurring`, -1 when unknown.
    pub recurring_schedules: i64,
    /// `process_work.count`, -1 when unknown.
    pub background: i64,
    pub subagents_active: i64,
    /// A valid coordination wait of this run and conversation.
    pub coordination_wait: bool,
    pub conversation_state: String,
    pub subagent_source: String,
    /// Children in `approval`, `input` or `attention`, by ID.
    pub children: Vec<ChildFacts>,
    /// Host-confirmed notification preferences, when valid (`SessionNotifications::valid`).
    pub preferences: Option<Preferences>,
    /// The row has `optional_questions`.
    pub optional_reported: bool,
    /// Its whole non-negative count, -1 when unknown.
    pub optional_count: i64,
    pub optional_ids: Vec<String>,
}

/// A child that asks (`SessionNotifications::childPhase`, `childRequest`, `childStarted`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChildFacts {
    pub id: String,
    pub label: String,
    pub phase: String,
    pub request: String,
    pub started: Option<f64>,
}

/// Valid shared notification preferences: muted, and the suppressed baseline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub muted: bool,
    pub suppressed: Value,
}
```

Replace the `notify: NotifyFacts { .. }` field of `row()` with `notify: notify_facts(value, &session),` and add:

```rust
fn positive_time(value: Option<f64>) -> Option<f64> {
    value.filter(|at| at.is_finite() && *at > 0.0)
}

/// The facts `AttentionTracker` and the menu bar read (`HgsClient.cpp:155-260`).
fn notify_facts(value: &Value, session: &Session) -> NotifyFacts {
    let preferences = value
        .get("notification_preferences")
        .filter(|preferences| crate::attention::valid_preferences(preferences, session.conversation_id))
        .map(|preferences| Preferences {
            muted: preferences.get("muted").and_then(Value::as_bool).unwrap_or(false),
            suppressed: preferences.get("suppressed").cloned().unwrap_or(Value::Null),
        });
    // Strict types: anything but a whole non-negative count is unknown, never 0.
    let (optional_count, optional_ids) = match object(value, "optional_questions").and_then(|summary| {
        let count = summary.get("count").and_then(Value::as_f64).filter(|c| *c >= 0.0 && *c <= 1e9 && c.fract() == 0.0)?;
        let mut ids: Vec<String> = Vec::new();
        for id in summary.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if !id.is_empty() && !ids.iter().any(|known| known == id) {
                ids.push(id.to_owned());
            }
        }
        Some((count as i64, ids))
    }) {
        Some(found) => found,
        None => (-1, Vec::new()),
    };
    let mut children: Vec<ChildFacts> = object(value, "subagents")
        .into_iter()
        .flatten()
        .filter_map(|(id, child)| {
            let phase = match text(child, "display_state") {
                "" => text(child, "state"),
                display => display,
            };
            if !matches!(phase, "approval" | "input" | "attention") {
                return None;
            }
            let request = match text(child, "attention_id") {
                "" => text(child, "question_id"),
                request => request,
            };
            let label = child
                .get("label")
                .and_then(Value::as_str)
                .or_else(|| child.get("name").and_then(Value::as_str))
                .unwrap_or(id);
            Some(ChildFacts {
                id: id.clone(),
                label: label.to_owned(),
                phase: phase.to_owned(),
                request: request.to_owned(),
                started: positive_time(number(child, "attention_started"))
                    .or_else(|| positive_time(number(child, "last_event_at"))),
            })
        })
        .collect();
    children.sort_by(|a, b| a.id.cmp(&b.id));
    NotifyFacts {
        attached: value.get("attached").and_then(Value::as_i64).unwrap_or(0) > 0,
        needs_action: session.needs_action(),
        acknowledged: session.attention_acknowledged(),
        cmd: text(value, "cmd").to_owned(),
        created: number(value, "created").map_or(0, |created| created as i64),
        attention_id: text(value, "attention_id").to_owned(),
        attention_started: number(value, "attention_started").unwrap_or(0.0),
        recovery_reason: object(value, "recovery").map_or("", |recovery| {
            recovery.get("reason").and_then(Value::as_str).unwrap_or("")
        }).to_owned(),
        activity_detail: text(value, "activity_detail").to_owned(),
        unread_reply: session.unread_reply(),
        reply_scheduled: value.get("reply_scheduled").and_then(Value::as_bool).unwrap_or(false),
        recurring_schedules: count(value, "schedules", "recurring"),
        background: session.background,
        subagents_active: session.subagents_active,
        coordination_wait: session.coordination_wait().is_some(),
        conversation_state: session.conversation_state.to_owned(),
        subagent_source: text(value, "subagent_source").to_owned(),
        children,
        preferences,
        optional_reported: value.get("optional_questions").is_some(),
        optional_count,
        optional_ids,
    }
}
```

The `object()` helper returns `Option<&Map>`; `object(value, "subagents").into_iter().flatten()` iterates its `(id, child)` pairs, and `text(child, ..)` and `number(child, ..)` take `&Value`.

An empty row now carries Qt's "unknown" counts (-1), so the existing exact comparison in `missing_fields_default_and_wrong_types_are_ignored` needs them. Change its expected `ViewRow` to:

```rust
            ViewRow {
                status: "ready".into(),
                caption: "Not tracked".into(),
                // Qt's default for a row without a state (`HgsClient.cpp`).
                state: "running".into(),
                terminal_supported: true,
                // Unknown counts are -1, never 0 (`HgsClient.cpp:169-174`, `:194-201`).
                notify: NotifyFacts { recurring_schedules: -1, background: -1, optional_count: -1, ..NotifyFacts::default() },
                ..ViewRow::default()
            }
```

`NotifyFacts` stays part of `ViewRow`'s equality: a change of the facts alone must still reach the desk thread.

- [ ] **Step 3: Write the failing attention tests**

Create `client/core/src/attention.rs` with only this test module (and `pub mod attention;` in `lib.rs`). Each test names the Qt test in `tray/tests/test_attention.cpp` it ports:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{ChildFacts, Preferences};
    use crate::{Change, ComputerStatus};

    fn session(name: &str) -> ViewRow {
        let mut row = ViewRow {
            name: name.to_owned(),
            state: "running".to_owned(),
            activity: "idle".to_owned(),
            phase: Some("idle".to_owned()),
            run_id: Some(format!("run-{name}")),
            conversation_id: Some(format!("conv-{name}")),
            ..ViewRow::default()
        };
        row.notify.cmd = "claude".to_owned();
        row.notify.created = 100;
        row.notify.background = -1;
        row.notify.recurring_schedules = -1;
        row.notify.optional_count = -1;
        row
    }

    fn asking(name: &str, phase: &str, request: &str) -> ViewRow {
        let mut row = session(name);
        row.phase = Some(phase.to_owned());
        row.activity = "waiting".to_owned();
        row.notify.needs_action = true;
        row.notify.attention_id = request.to_owned();
        row
    }

    fn replied(name: &str, reply: &str) -> ViewRow {
        let mut row = session(name);
        row.reply_id = Some(reply.to_owned());
        row.notify.unread_reply = true;
        row
    }

    fn with_preferences(mut row: ViewRow, muted: bool) -> ViewRow {
        row.notify.preferences = Some(Preferences {
            muted,
            suppressed: json!({"reply_id": null, "attention_signature": null, "optional_question_ids": [],
                               "main_attention": null, "child_attention": {}}),
        });
        row
    }

    struct Fixture {
        tracker: Tracker,
        notices: u32,
    }

    impl Fixture {
        fn new() -> Self {
            Self { tracker: Tracker::new(Map::new()), notices: 0 }
        }

        fn observe_at(&mut self, list: Vec<ViewRow>, now: i64, focused: bool, ok: bool) -> Changes {
            let rows: BTreeMap<String, ViewRow> = list.into_iter().map(|row| (row.name.clone(), row)).collect();
            let mut count = self.notices;
            let changes = self.tracker.observe(&Host { alias: "", label: "arch", ok, rows: &rows }, now, focused, &mut || {
                count += 1;
                format!("notice-{count}")
            });
            self.notices = count;
            changes
        }

        fn observe(&mut self, list: Vec<ViewRow>) -> Changes {
            self.observe_at(list, 1000, false, true)
        }
    }

    #[test]
    fn an_approval_notifies_once_and_is_withdrawn_when_answered() {
        // edgesOfflineRestartAndIdentity
        let mut fixture = Fixture::new();
        let changes = fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].title, NEEDS_ATTENTION);
        assert_eq!(changes.raised[0].body, "claude/p/a / arch\nApproval needed");
        let token = changes.raised[0].token.clone();
        assert_eq!(fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]), Changes::default());
        assert_eq!(fixture.observe(vec![session("claude/p/a")]), Changes { raised: vec![], cleared: vec![token] });
    }

    #[test]
    fn a_focused_window_consumes_the_edge_without_a_banner() {
        // foregroundRequestsStayActionableWithoutReplay
        let mut fixture = Fixture::new();
        assert!(fixture.observe_at(vec![asking("claude/p/a", "input", "req-1")], 1000, true, true).raised.is_empty());
        assert!(fixture.observe(vec![asking("claude/p/a", "input", "req-1")]).raised.is_empty());
        assert_eq!(fixture.observe(vec![asking("claude/p/a", "input", "req-2")]).raised.len(), 1);
    }

    #[test]
    fn a_reply_waits_for_idle_and_notifies_once() {
        // replyWaitsForLiveBackgroundShellToFinish, deliveredRepliesDoNotRepeatAfterWorking
        let mut fixture = Fixture::new();
        let mut busy = replied("claude/p/a", "rep-1");
        busy.activity = "busy".to_owned();
        busy.phase = Some("working".to_owned());
        assert_eq!(fixture.observe(vec![busy.clone()]), Changes::default());
        let mut background = replied("claude/p/a", "rep-1");
        background.notify.background = 1;
        assert_eq!(fixture.observe(vec![background]), Changes::default());
        let idle = fixture.observe(vec![replied("claude/p/a", "rep-1")]);
        assert_eq!(idle.raised.len(), 1);
        assert_eq!(idle.raised[0].title, NEW_REPLY);
        assert_eq!(idle.raised[0].body, "claude/p/a / arch\nAgent has a new reply");
        assert_eq!(fixture.observe(vec![busy]), Changes::default(), "working again keeps the delivered reply");
        assert_eq!(fixture.observe(vec![replied("claude/p/a", "rep-1")]), Changes::default());
    }

    #[test]
    fn routine_ticks_of_a_recurring_schedule_are_consumed() {
        let mut fixture = Fixture::new();
        let mut tick = replied("claude/p/a", "rep-1");
        tick.notify.reply_scheduled = true;
        tick.notify.recurring_schedules = 2;
        assert!(fixture.observe(vec![tick]).raised.is_empty());
        let mut last = replied("claude/p/a", "rep-2");
        last.notify.reply_scheduled = true;
        last.notify.recurring_schedules = 0;
        assert_eq!(fixture.observe(vec![last]).raised.len(), 1);
    }

    #[test]
    fn muted_sessions_withdraw_their_banners_and_unmuting_never_replays() {
        // muteConsumesAllNotificationEdgesWithoutAcknowledging
        let mut fixture = Fixture::new();
        let token = fixture.observe(vec![with_preferences(asking("claude/p/a", "approval", "r"), false)]).raised[0].token.clone();
        let muted = fixture.observe(vec![with_preferences(asking("claude/p/a", "approval", "r"), true)]);
        assert_eq!(muted, Changes { raised: vec![], cleared: vec![token] });
        assert!(fixture.observe(vec![with_preferences(asking("claude/p/a", "approval", "r"), false)]).raised.is_empty());
    }

    #[test]
    fn an_offline_host_withdraws_only_muted_banners() {
        // muteWithdrawsOnlyMatchingOfflineBannersWithoutReplay
        let mut fixture = Fixture::new();
        let raised = fixture.observe(vec![asking("claude/p/a", "approval", "r"), asking("claude/p/b", "approval", "r")]).raised;
        let token = raised.iter().find(|notice| notice.body.starts_with("claude/p/a")).unwrap().token.clone();
        let offline = fixture.observe_at(
            vec![with_preferences(asking("claude/p/a", "approval", "r"), true), asking("claude/p/b", "approval", "r")],
            1000, false, false,
        );
        assert_eq!(offline, Changes { raised: vec![], cleared: vec![token] });
        assert!(fixture.observe(vec![asking("claude/p/b", "approval", "r")]).raised.is_empty());
    }

    #[test]
    fn an_acknowledged_request_is_withdrawn() {
        // acknowledgedAttentionWithdrawsItsNotification
        let mut fixture = Fixture::new();
        let token = fixture.observe(vec![asking("claude/p/a", "approval", "r")]).raised[0].token.clone();
        let mut read = asking("claude/p/a", "approval", "r");
        read.notify.acknowledged = true;
        assert_eq!(fixture.observe(vec![read]), Changes { raised: vec![], cleared: vec![token] });
    }

    #[test]
    fn child_requests_notify_with_their_label() {
        // onlyExplicitChildRequestsNotify
        let mut fixture = Fixture::new();
        let mut parent = session("claude/p/a");
        parent.activity = "busy".to_owned();
        parent.notify.children = vec![ChildFacts {
            id: "w1".into(), label: "Reviewer".into(), phase: "approval".into(), request: "q-1".into(), started: Some(5.0),
        }];
        let changes = fixture.observe(vec![parent.clone()]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].body, "claude/p/a / arch\nSubagent Reviewer: Approval needed");
        parent.notify.subagent_source = "hook_profiles".into();
        assert_eq!(fixture.observe(vec![parent]).cleared.len(), 1);
    }

    #[test]
    fn optional_questions_are_recorded_and_notify_only_when_enabled() {
        // optionalQuestionsNotifyOncePerQuestionWhenEnabled
        let mut fixture = Fixture::new();
        let mut asked = session("codex/p/a");
        asked.notify.cmd = "codex".into();
        asked.notify.optional_reported = true;
        asked.notify.optional_count = 1;
        asked.notify.optional_ids = vec!["q1".into()];
        assert!(fixture.observe(vec![asked.clone()]).raised.is_empty(), "off by default; q1 is recorded");
        fixture.tracker.set_optional_questions(true);
        assert!(fixture.observe(vec![asked.clone()]).raised.is_empty(), "turning banners on announces only later ones");
        asked.notify.optional_count = 2;
        asked.notify.optional_ids = vec!["q1".into(), "q2".into()];
        let changes = fixture.observe(vec![asked]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].title, OPTIONAL_QUESTION);
        assert_eq!(changes.raised[0].body, "codex/p/a / arch\n2 optional questions waiting");
        assert_eq!(target(&changes.raised[0].token).unwrap()["question"], "q2");
    }

    #[test]
    fn a_failed_delivery_retries_after_a_minute() {
        // failedDeliveryRetriesAfterBackoff
        let mut fixture = Fixture::new();
        let token = fixture.observe(vec![asking("claude/p/a", "approval", "r")]).raised[0].token.clone();
        fixture.tracker.delivery_failed(&token, 1000);
        assert!(fixture.observe_at(vec![asking("claude/p/a", "approval", "r")], 1059, false, true).raised.is_empty());
        let retried = fixture.observe_at(vec![asking("claude/p/a", "approval", "r")], 1060, false, true);
        assert_eq!(retried.raised.len(), 1);
        assert_ne!(retried.raised[0].token, token);
        assert_eq!(retried.cleared, vec![token]);
    }

    #[test]
    fn targets_match_by_conversation_run_or_name_and_creation() {
        let mut fixture = Fixture::new();
        let reply = target(&fixture.observe(vec![replied("claude/p/a", "rep-1")]).raised[0].token).unwrap();
        let mut renamed = replied("claude/p/renamed", "rep-1");
        renamed.conversation_id = Some("conv-claude/p/a".into());
        assert!(matches(&reply, &renamed), "a reply follows its conversation across a rename");
        let mut other_agent = renamed.clone();
        other_agent.notify.cmd = "codex".into();
        assert!(!matches(&reply, &other_agent));
        let mut archived = renamed.clone();
        archived.state = "archived".into();
        assert!(!matches(&reply, &archived), "an archived row never opens");
        let request = target(&fixture.observe(vec![asking("claude/p/b", "approval", "r")]).raised[0].token).unwrap();
        let mut resumed = asking("claude/p/b", "approval", "r");
        resumed.run_id = Some("another-run".into());
        assert!(!matches(&request, &resumed), "a request belongs to its run");
        let mut runless = asking("claude/p/c", "approval", "r");
        runless.run_id = None;
        let by_name = target(&fixture.observe(vec![runless.clone()]).raised[0].token).unwrap();
        assert!(matches(&by_name, &runless));
        runless.notify.created = 101;
        assert!(!matches(&by_name, &runless), "a run-less session is its name and creation time");
        assert_eq!(target(&"x".repeat(8193)), None);
        assert_eq!(target("not base64 !"), None);
    }

    #[test]
    fn a_click_waits_then_reports_a_gone_session() {
        let token = Fixture::new().observe(vec![asking("claude/p/a", "approval", "r")]).raised[0].token.clone();
        let wanted = target(&token).unwrap();
        let mut desk = Desk::default();
        desk.observe(&Change::Computer { computer: ComputerId::local(), status: ComputerStatus::Probing });
        assert_eq!(resolve(&wanted, &desk, false), Resolution::Waiting, "the computer has not answered yet");
        assert_eq!(resolve(&wanted, &desk, true), Resolution::Gone("claude/p/a".into()), "16 s later it is gone");
        desk.observe(&Change::Replace { computer: ComputerId::local(), rows: vec![("k-a".into(), asking("claude/p/a", "approval", "r"))] });
        desk.observe(&Change::Computer { computer: ComputerId::local(), status: ComputerStatus::Live });
        assert_eq!(resolve(&wanted, &desk, false), Resolution::Open { computer: ComputerId::local(), key: "k-a".into() });
        desk.observe(&Change::Replace { computer: ComputerId::local(), rows: vec![] });
        assert_eq!(resolve(&wanted, &desk, false), Resolution::Gone("claude/p/a".into()), "a current listing without it settles at once");
        assert_eq!(gone_text("claude/p/a"), "This session is no longer available: claude/p/a");
    }

    #[test]
    fn a_qt_seeded_state_withdraws_banners_qt_delivered() {
        let mut qt = Fixture::new();
        let token = qt.observe(vec![asking("claude/p/a", "approval", "r")]).raised[0].token.clone();
        let saved = serde_json::to_vec(qt.tracker.state()).unwrap();
        let mut client = Fixture { tracker: Tracker::new(seed(None, Some(&saved))), notices: 0 };
        assert!(client.observe(vec![asking("claude/p/a", "approval", "r")]).raised.is_empty(), "nothing repeats after the switch");
        assert_eq!(client.observe(vec![session("claude/p/a")]).cleared, vec![token]);
    }

    #[test]
    fn the_seed_prefers_the_clients_own_file() {
        let own = br#"{"": {"run:x": {"phase": "approval", "request": "r"}}}"#;
        let qt = br#"{"mac": {}}"#;
        assert!(seed(Some(own), Some(qt)).contains_key(""));
        assert!(seed(None, Some(qt)).contains_key("mac"));
        assert!(seed(None, Some(b"[1]")).is_empty());
        assert!(seed(Some(b"garbage"), Some(qt)).is_empty(), "the client never seeds twice");
    }

    #[test]
    fn preferences_validate_like_qt() {
        let valid = json!({"conversation_id": "c", "muted": false, "revision": 2, "suppressed": {
            "reply_id": "rep", "attention_signature": "0".repeat(64), "optional_question_ids": ["a", "b"],
            "main_attention": {"run_id": "r", "phase": "approval", "request_id": "", "started_at": 5.0},
            "child_attention": {"w": {"phase": "input", "request_id": "q", "started_at": null}}}});
        assert!(valid_preferences(&valid, "c"));
        assert!(!valid_preferences(&valid, "other"));
        let mut unsorted = valid.clone();
        unsorted["suppressed"]["optional_question_ids"] = json!(["b", "a"]);
        assert!(!valid_preferences(&unsorted, "c"), "question IDs strictly increase");
        let mut extra = valid.clone();
        extra["extra"] = json!(1);
        assert!(!valid_preferences(&extra, "c"), "exact keys");
        let mut timeless = valid.clone();
        timeless["suppressed"]["main_attention"]["started_at"] = Value::Null;
        assert!(!valid_preferences(&timeless, "c"), "an identity-less request needs its start time");
        let mut working = valid.clone();
        working["suppressed"]["child_attention"]["w"]["phase"] = json!("working");
        assert!(!valid_preferences(&working, "c"));
    }

    #[test]
    fn request_identity_is_qt_compact_json() {
        assert_eq!(request_json("req-1", Some(5.0)), r#"["req-1",null]"#);
        assert_eq!(request_json("", Some(1761600000.0)), r#"["",1761600000]"#);
        assert_eq!(request_json("", Some(1761600000.25)), r#"["",1761600000.25]"#);
        assert_eq!(request_json("", Some(0.0)), r#"["",null]"#);
        assert_eq!(request_json("", None), r#"["",null]"#);
        assert_eq!(concise("  a\tb\n c ", 120), "a b c");
        assert_eq!(concise(&"x".repeat(50), 40).chars().count(), 40);
    }

    #[test]
    fn tokens_are_base64url_without_padding() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"{\"host\":\"\"}\xff\xfe"] {
            let text = base64url(bytes);
            assert!(!text.contains(['=', '+', '/']));
            assert_eq!(from_base64url(&text).as_deref(), Some(bytes));
        }
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn delivery_follows_the_mac_notifier() {
        // test_macattention.mm: waitsForPermissionAndHonorsWithdrawal
        let mut delivery = Delivery::default();
        assert_eq!(delivery.post("t1", "Title", "Body"), vec![DeliveryStep::CheckAuthorization]);
        assert!(delivery.post("t1", "Title", "Body").is_empty(), "posted once");
        assert!(delivery.post("t2", "Title", "Two").is_empty(), "waits for the running check");
        assert_eq!(delivery.withdraw("t2"), vec![DeliveryStep::Remove { token: "t2".into() }]);
        assert_eq!(
            delivery.authorized(true, None),
            vec![DeliveryStep::Add { token: "t1".into(), title: "Title".into(), body: "Body".into() }]
        );
        assert!(delivery.added("t1", None).is_empty());
        assert_eq!(delivery.post("t3", "T", "B"), vec![DeliveryStep::CheckAuthorization]);
        assert_eq!(delivery.authorized(false, None), vec![DeliveryStep::Fail { token: "t3".into(), detail: DENIED.into() }]);
        assert_eq!(delivery.post("t4", "T", "B"), vec![DeliveryStep::CheckAuthorization]);
        assert_eq!(delivery.authorized(true, None).len(), 1);
        assert_eq!(
            delivery.added("t4", Some("disk full".into())),
            vec![DeliveryStep::Fail { token: "t4".into(), detail: "disk full".into() }]
        );
        let _ = delivery.withdraw("t1");
        assert_eq!(delivery.added("t1", None), vec![DeliveryStep::Remove { token: "t1".into() }]);
        assert_eq!(request_id("abc"), "zerus.attention.abc");
    }
}
```

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml attention::`
Expected: FAIL to compile (every item of the module is missing).

- [ ] **Step 4: Write `attention.rs` above its tests**

```rust
//! Attention notifications as the Qt client decides them: a port of
//! `AttentionTracker` (`tray/src/AttentionTracker.cpp`), of the shared
//! preferences check (`tray/src/SessionNotifications.h`), of the macOS
//! notifier's delivery state (`tray/src/MacAttentionNotifier.mm`) and of the
//! click target (`TrayAgent.cpp:166-193`, `:593-608`). The state keeps Qt's
//! JSON shape, so Qt's saved `attention/observed` can seed it.
use crate::menubar::{Computer, Desk};
use crate::{ComputerId, ViewRow};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const NEEDS_ATTENTION: &str = "Zerus \u{2014} Needs attention";
pub const NEW_REPLY: &str = "Zerus \u{2014} New reply";
pub const OPTIONAL_QUESTION: &str = "Zerus \u{2014} Optional question";
/// Question IDs remembered per conversation (`AttentionTracker.cpp:63`).
pub const SEEN_QUESTIONS: usize = 64;
/// A failed delivery is retried this many seconds later (`AttentionTracker.cpp:297`).
pub const RETRY_SECONDS: i64 = 60;
/// How long a click waits for its session (`TrayAgent.cpp:192`).
pub const CLICK_WAIT: Duration = Duration::from_secs(16);
/// Without permission (`MacAttentionNotifier.mm:90`).
pub const DENIED: &str = "Allow Zerus notifications in System Settings \u{2192} Notifications.";

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or("")
}

fn integer(object: &Map<String, Value>, key: &str) -> i64 {
    object
        .get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|number| number as i64)))
        .unwrap_or(0)
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

fn positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// `concise`: whitespace runs become one space, control characters a space,
/// and text over `limit` characters ends with an ellipsis (`AttentionTracker.cpp:53-59`).
pub fn concise(value: &str, limit: usize) -> String {
    let simplified = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = simplified.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if cleaned.chars().count() > limit {
        let mut cut: String = cleaned.chars().take(limit - 1).collect();
        cut.push('\u{2026}');
        cut
    } else {
        cleaned
    }
}

/// A double as Qt's compact JSON writes the timestamps hosts send.
fn qt_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `notificationRequest`: `[request, started]`, the start time only for an
/// identity-less request (`AttentionTracker.cpp:74-77`).
pub fn request_json(request: &str, started: Option<f64>) -> String {
    let started = match started.and_then(positive) {
        Some(at) if request.is_empty() => qt_number(at),
        _ => "null".to_owned(),
    };
    format!("[{},{started}]", Value::from(request))
}

/// `QByteArray::toBase64(Base64UrlEncoding | OmitTrailingEquals)`.
pub fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `QByteArray::fromBase64(Base64UrlEncoding)`: the URL alphabet, with or
/// without trailing `=`.
pub fn from_base64url(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

pub fn encode_token(target: &Map<String, Value>) -> String {
    base64url(Value::Object(target.clone()).to_string().as_bytes())
}

/// `AttentionTracker::target` (`AttentionTracker.cpp:305-312`).
pub fn target(token: &str) -> Option<Map<String, Value>> {
    if token.is_empty() || token.len() > 8192 {
        return None;
    }
    let Value::Object(object) = serde_json::from_slice(&from_base64url(token)?).ok()? else {
        return None;
    };
    let valid = object.get("host").is_some_and(Value::is_string)
        && !text(&object, "name").is_empty()
        && !text(&object, "notice").is_empty();
    valid.then_some(object)
}

/// `AttentionTracker::matches` (`AttentionTracker.cpp:314-322`).
pub fn matches(target: &Map<String, Value>, row: &ViewRow) -> bool {
    if target.is_empty() || row.state == "archived" {
        return false;
    }
    if !text(target, "reply").is_empty() || !text(target, "question").is_empty() {
        return text(target, "conversation") == row.conversation_id.as_deref().unwrap_or("")
            && text(target, "agent") == row.notify.cmd;
    }
    match text(target, "run") {
        "" => text(target, "name") == row.name && integer(target, "created") == row.notify.created,
        run => Some(run) == row.run_id.as_deref(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Open { computer: ComputerId, key: String },
    Gone(String),
    Waiting,
}

/// Where a clicked notification leads (`TrayAgent::resolveAttentionClick`):
/// its session, or gone once a current listing lacks it or 16 s passed.
pub fn resolve(target: &Map<String, Value>, desk: &Desk, expired: bool) -> Resolution {
    let computer = ComputerId(text(target, "host").to_owned());
    let state = desk.computers().get(&computer);
    if let Some(found) = state.and_then(|state| state.rows.iter().find(|(_, row)| matches(target, row))) {
        return Resolution::Open { computer, key: found.0.clone() };
    }
    if expired || state.is_some_and(Computer::fresh) {
        Resolution::Gone(text(target, "name").to_owned())
    } else {
        Resolution::Waiting
    }
}

pub fn gone_text(name: &str) -> String {
    format!("This session is no longer available: {name}")
}

/// The tracker's saved state: the client's own file; on its first start,
/// Qt's `attention/observed` value (Decision 11).
pub fn seed(own: Option<&[u8]>, qt: Option<&[u8]>) -> Map<String, Value> {
    let parse = |bytes: &[u8]| match serde_json::from_slice(bytes) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    };
    match own {
        Some(bytes) => parse(bytes).unwrap_or_default(),
        None => qt.and_then(parse).unwrap_or_default(),
    }
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn text_ok(value: &Value, maximum: usize, nonempty: bool, line_feed: bool) -> bool {
    let Some(text) = value.as_str() else { return false };
    if (nonempty && text.is_empty()) || text.len() > maximum {
        return false;
    }
    text.chars().all(|c| !((c < ' ' && !(line_feed && c == '\n')) || c == '\u{7f}'))
}

fn identity_ok(value: &Value, maximum: usize) -> bool {
    text_ok(value, maximum, true, false)
}

fn revision_ok(value: &Value) -> bool {
    value.as_f64().is_some_and(|number| number >= 0.0 && number.fract() == 0.0)
}

fn started_ok(value: &Value) -> bool {
    value.is_null() || value.as_f64().is_some_and(|at| at.is_finite() && at > 0.0)
}

/// `SessionNotifications::attention` (`SessionNotifications.h:35-42`).
fn attention_ok(value: &Map<String, Value>, main: bool) -> bool {
    let keys: &[&str] = if main { &["run_id", "phase", "request_id", "started_at"] } else { &["phase", "request_id", "started_at"] };
    if !exact_keys(value, keys) || (main && !identity_ok(&value["run_id"], 128)) {
        return false;
    }
    if !text_ok(&value["phase"], 256, false, false) || !text_ok(&value["request_id"], 65536, false, true) || !started_ok(&value["started_at"]) {
        return false;
    }
    if text(value, "request_id").is_empty() && value["started_at"].is_null() {
        return false;
    }
    main || matches!(text(value, "phase"), "approval" | "input" | "attention")
}

/// `SessionNotifications::valid` (`SessionNotifications.h:43-80`).
pub fn valid_preferences(value: &Value, conversation: &str) -> bool {
    let Some(object) = value.as_object() else { return false };
    // The host's ASCII-escaped budget before persisting unmute.
    let budget: usize = value.to_string().chars().map(|c| if c.is_ascii() { 1 } else { 6 }).sum();
    if budget > 384 * 1024
        || !exact_keys(object, &["conversation_id", "muted", "revision", "suppressed"])
        || !identity_ok(&object["conversation_id"], 256)
        || text(object, "conversation_id") != conversation
        || !object["muted"].is_boolean()
        || !revision_ok(&object["revision"])
    {
        return false;
    }
    let Some(baseline) = object["suppressed"].as_object() else { return false };
    if !exact_keys(baseline, &["reply_id", "attention_signature", "optional_question_ids", "main_attention", "child_attention"]) {
        return false;
    }
    let reply = &baseline["reply_id"];
    let signature = &baseline["attention_signature"];
    if !reply.is_null() && !identity_ok(reply, 256) {
        return false;
    }
    if !signature.is_null()
        && !signature.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    {
        return false;
    }
    let (Some(optional), Some(children)) = (baseline["optional_question_ids"].as_array(), baseline["child_attention"].as_object()) else {
        return false;
    };
    if optional.len() > 64 || !optional.iter().all(|id| identity_ok(id, 256)) {
        return false;
    }
    let ids: Vec<&[u8]> = optional.iter().filter_map(Value::as_str).map(str::as_bytes).collect();
    if ids.windows(2).any(|pair| pair[1] <= pair[0]) {
        return false;
    }
    let main = &baseline["main_attention"];
    if !main.is_null() && !main.as_object().is_some_and(|main| attention_ok(main, true)) {
        return false;
    }
    children.len() <= 256
        && children.iter().all(|(id, child)| {
            identity_ok(&Value::from(id.as_str()), 256) && child.as_object().is_some_and(|child| attention_ok(child, false))
        })
}

fn identity(row: &ViewRow) -> String {
    match nonempty(&row.run_id) {
        Some(run) => format!("run:{run}"),
        None => format!("name:{}:{}", row.name, row.notify.created),
    }
}

fn muted(row: &ViewRow) -> bool {
    row.notify.preferences.as_ref().is_some_and(|preferences| preferences.muted)
}

fn baseline(row: &ViewRow) -> Option<&Map<String, Value>> {
    row.notify.preferences.as_ref().and_then(|preferences| preferences.suppressed.as_object())
}

/// `SessionNotifications::matches` (`SessionNotifications.h:81-84`).
fn matches_saved(saved: &Map<String, Value>, phase: &str, request: &str, started: Option<f64>) -> bool {
    text(saved, "phase") == phase
        && text(saved, "request_id") == request
        && (!request.is_empty()
            || started.and_then(positive).is_some_and(|at| saved.get("started_at").and_then(Value::as_f64) == Some(at)))
}

fn suppressed_main(row: &ViewRow, baseline: &Map<String, Value>) -> bool {
    baseline.get("main_attention").and_then(Value::as_object).is_some_and(|main| {
        !main.is_empty()
            && text(main, "run_id") == row.run_id.as_deref().unwrap_or("")
            && matches_saved(main, row.phase.as_deref().unwrap_or(""), &row.notify.attention_id, Some(row.notify.attention_started))
    })
}

fn suppressed_child(baseline: &Map<String, Value>, child: &crate::view::ChildFacts) -> bool {
    baseline
        .get("child_attention")
        .and_then(Value::as_object)
        .and_then(|children| children.get(&child.id))
        .and_then(Value::as_object)
        .is_some_and(|saved| !saved.is_empty() && matches_saved(saved, &child.phase, &child.request, child.started))
}

fn optional_count(row: &ViewRow) -> i64 {
    if row.state != "running" || row.process_state == "exited" || row.notify.optional_count < 0 {
        0
    } else {
        row.notify.optional_count
    }
}

/// One computer as the tracker reads it.
pub struct Host<'a> {
    /// "" for this computer, else the peer's alias: the state's key and the token's `host`.
    pub alias: &'a str,
    /// The name notices show: the alias, or this computer's host name.
    pub label: &'a str,
    /// A current listing (`box.ok`); otherwise only muted banners are withdrawn.
    pub ok: bool,
    pub rows: &'a BTreeMap<String, ViewRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub token: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub raised: Vec<Notice>,
    pub cleared: Vec<String>,
}

struct Edge {
    key: String,
    phase: String,
    request: String,
    reason: String,
    reply: bool,
    silent: bool,
    started: Option<f64>,
}

struct Pass<'a, 'n> {
    host: &'a Host<'a>,
    previous: &'a Map<String, Value>,
    current: Map<String, Value>,
    raised: Vec<Notice>,
    now: i64,
    focused: bool,
    notice_id: &'n mut dyn FnMut() -> String,
}

impl Pass<'_, '_> {
    fn body(&self, row: &ViewRow, reason: &str) -> String {
        format!("{} / {}\n{reason}", concise(&row.name, 120), concise(self.host.label, 40))
    }

    fn target(&mut self, row: &ViewRow) -> Map<String, Value> {
        let mut target = Map::new();
        target.insert("host".into(), Value::from(self.host.alias));
        target.insert("name".into(), Value::from(row.name.as_str()));
        target.insert("run".into(), Value::from(row.run_id.as_deref().unwrap_or("")));
        target.insert("created".into(), Value::from(row.notify.created));
        target.insert("notice".into(), Value::from((self.notice_id)()));
        target
    }

    /// `notice` (`AttentionTracker.cpp:124-161`).
    fn notice(&mut self, row: &ViewRow, edge: Edge) {
        let previous = self.previous;
        let old = previous.get(&edge.key).and_then(Value::as_object);
        let mut entry = Map::new();
        entry.insert("phase".into(), Value::from(edge.phase.as_str()));
        entry.insert("request".into(), Value::from(edge.request.as_str()));
        if let Some(started) = edge.started {
            entry.insert("started".into(), json!(started));
        }
        let same = old.is_some_and(|old| {
            text(old, "phase") == edge.phase
                && text(old, "request") == edge.request
                && (edge.started.is_none() || !old.contains_key("started") || old.get("started").and_then(Value::as_f64) == edge.started)
        });
        if edge.silent || muted(row) {
            self.current.insert(edge.key, Value::Object(entry));
            return;
        }
        let retry = old.map_or(0, |old| integer(old, "retry_at"));
        if same && (retry == 0 || retry > self.now) {
            let mut kept = old.cloned().unwrap_or_default();
            if let Some(started) = edge.started {
                kept.insert("started".into(), json!(started));
            }
            self.current.insert(edge.key, Value::Object(kept));
            return;
        }
        let mut target = self.target(row);
        if edge.reply {
            target.insert("reply".into(), Value::from(row.reply_id.as_deref().unwrap_or("")));
            target.insert("conversation".into(), Value::from(row.conversation_id.as_deref().unwrap_or("")));
            target.insert("agent".into(), Value::from(row.notify.cmd.as_str()));
        }
        let token = encode_token(&target);
        entry.insert("token".into(), Value::from(token.as_str()));
        self.current.insert(edge.key, Value::Object(entry));
        if !self.focused {
            let title = if edge.reply { NEW_REPLY } else { NEEDS_ATTENTION };
            let body = self.body(row, &edge.reason);
            self.raised.push(Notice { token, title: title.to_owned(), body });
        }
    }

    /// One session's request, reply and children (`AttentionTracker.cpp:162-206`).
    fn session(&mut self, row: &ViewRow) {
        let facts = &row.notify;
        let baseline = baseline(row);
        if facts.needs_action {
            let reason = match row.phase.as_deref() {
                Some("approval") => "Approval needed".to_owned(),
                Some("input") => "Input needed".to_owned(),
                _ if !facts.recovery_reason.is_empty() => concise(&facts.recovery_reason, 240),
                _ if !facts.activity_detail.is_empty() => concise(&facts.activity_detail, 240),
                _ => "Agent needs attention".to_owned(),
            };
            let request = match baseline {
                Some(_) => request_json(&facts.attention_id, Some(facts.attention_started)),
                None => facts.attention_id.clone(),
            };
            self.notice(row, Edge {
                key: identity(row),
                phase: row.phase.clone().unwrap_or_default(),
                request,
                reason,
                reply: false,
                silent: facts.acknowledged || baseline.is_some_and(|b| suppressed_main(row, b)),
                started: facts.attention_id.is_empty().then_some(facts.attention_started),
            });
        }
        if let (true, Some(reply), Some(conversation)) =
            (row.state != "archived" && facts.unread_reply, nonempty(&row.reply_id), nonempty(&row.conversation_id))
        {
            let key = format!("reply:{}:{conversation}", facts.cmd);
            let idle = row.activity == "idle"
                && row.phase.as_deref() == Some("idle")
                && facts.background <= 0
                && facts.subagents_active == 0
                && !facts.coordination_wait
                && !facts.needs_action;
            let suppressed = baseline.and_then(|b| b.get("reply_id")).and_then(Value::as_str) == Some(reply);
            let edge = |reason: &str, silent: bool| Edge {
                key: key.clone(),
                phase: "reply".to_owned(),
                request: reply.to_owned(),
                reason: reason.to_owned(),
                reply: true,
                silent,
                started: None,
            };
            let previous = self.previous;
            if muted(row) || suppressed || (idle && facts.reply_scheduled && facts.recurring_schedules > 0) {
                self.notice(row, edge("", true));
            } else if idle && (!facts.reply_scheduled || facts.recurring_schedules == 0) {
                self.notice(row, edge("Agent has a new reply", false));
            } else if let Some(kept) = previous.get(&key).filter(|old| old.get("request").and_then(Value::as_str) == Some(reply)) {
                self.current.insert(key.clone(), kept.clone());
            }
        }
        if row.state != "running"
            || row.process_state == "exited"
            || row.activity == "unknown"
            || (facts.conversation_state == "ended" && row.activity != "idle")
            || facts.subagent_source == "hook_profiles"
        {
            return;
        }
        for child in &facts.children {
            let request = match baseline {
                Some(_) => request_json(&child.request, child.started),
                None => child.request.clone(),
            };
            let kind = if child.phase == "approval" { "Approval needed" } else { "Input needed" };
            self.notice(row, Edge {
                key: format!("child:{}:{}", identity(row), child.id),
                phase: child.phase.clone(),
                request,
                reason: format!("Subagent {}: {kind}", concise(&child.label, 100)),
                reply: false,
                silent: facts.acknowledged || baseline.is_some_and(|b| suppressed_child(b, child)),
                started: None,
            });
        }
    }

    /// Optional questions per conversation (`AttentionTracker.cpp:207-277`).
    fn questions(&mut self, enabled: bool) -> Vec<Notice> {
        struct Optional<'r> {
            row: &'r ViewRow,
            ids: Vec<String>,
            unknown: bool,
        }
        let live = |row: &ViewRow| row.state == "running" && row.process_state != "exited";
        let rows = self.host.rows;
        let mut optional: BTreeMap<String, Optional> = BTreeMap::new();
        for row in rows.values() {
            let Some(conversation) = nonempty(&row.conversation_id) else { continue };
            if !row.notify.optional_reported {
                continue;
            }
            let entry = optional
                .entry(format!("question:{}:{conversation}", row.notify.cmd))
                .or_insert(Optional { row, ids: Vec::new(), unknown: false });
            if live(row) && !live(entry.row) {
                entry.row = row;
            }
            if live(row) && row.notify.optional_count < 0 {
                entry.unknown = true;
            }
            if optional_count(row) > 0 {
                for id in &row.notify.optional_ids {
                    if !entry.ids.contains(id) {
                        entry.ids.push(id.clone());
                    }
                }
            }
        }
        let previous = self.previous;
        let mut questions = Vec::new();
        for (key, item) in &optional {
            let row = item.row;
            let mut old = previous.get(key).and_then(Value::as_object).cloned().unwrap_or_default();
            let seen_baseline: Vec<Value> = baseline(row)
                .and_then(|b| b.get("optional_question_ids"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !seen_baseline.is_empty() {
                if let Some(posted) = target(text(&old, "token")) {
                    if text(&posted, "host") == self.host.alias
                        && text(&posted, "agent") == row.notify.cmd
                        && text(&posted, "conversation") == row.conversation_id.as_deref().unwrap_or("")
                        && seen_baseline.contains(posted.get("question").unwrap_or(&Value::Null))
                    {
                        old.remove("token");
                        old.remove("retry_at");
                    }
                }
            }
            if item.unknown {
                // An unreadable index is not an answer: keep the banner and IDs.
                if !old.is_empty() {
                    if muted(row) {
                        old.remove("token");
                        old.remove("retry_at");
                    }
                    self.current.insert(key.clone(), Value::Object(old));
                }
                continue;
            }
            let mut known = BTreeSet::new();
            let mut seen = Vec::new();
            for id in old.get("seen").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                known.insert(id.to_owned());
                if !item.ids.iter().any(|current| current == id) {
                    seen.push(id.to_owned());
                }
            }
            known.extend(seen_baseline.iter().filter_map(Value::as_str).map(str::to_owned));
            let fresh: Vec<&String> = item.ids.iter().filter(|id| !known.contains(*id)).collect();
            let drop = (seen.len() + item.ids.len()).saturating_sub(SEEN_QUESTIONS);
            let mut seen: Vec<String> = seen.into_iter().skip(drop).collect();
            seen.extend(item.ids.iter().cloned());
            let mut entry = Map::new();
            entry.insert("seen".into(), json!(seen));
            if !item.ids.is_empty() && enabled && !row.notify.acknowledged && !muted(row) {
                let token = text(&old, "token").to_owned();
                let retry = integer(&old, "retry_at");
                if (!fresh.is_empty() || (!token.is_empty() && retry != 0 && retry <= self.now))
                    && !self.focused
                    && !row.notify.needs_action
                {
                    let question = fresh.last().map(|id| (*id).clone()).or_else(|| item.ids.last().cloned()).unwrap_or_default();
                    let mut target = self.target(row);
                    target.insert("question".into(), Value::from(question));
                    target.insert("conversation".into(), Value::from(row.conversation_id.as_deref().unwrap_or("")));
                    target.insert("agent".into(), Value::from(row.notify.cmd.as_str()));
                    let token = encode_token(&target);
                    entry.insert("token".into(), Value::from(token.as_str()));
                    let count = row.notify.optional_count.max(item.ids.len() as i64);
                    let reason = if count == 1 {
                        "Optional question waiting".to_owned()
                    } else {
                        format!("{count} optional questions waiting")
                    };
                    questions.push(Notice { token, title: OPTIONAL_QUESTION.to_owned(), body: self.body(row, &reason) });
                } else if !token.is_empty() {
                    entry.insert("token".into(), Value::from(token));
                    if retry != 0 {
                        entry.insert("retry_at".into(), Value::from(retry));
                    }
                }
            }
            self.current.insert(key.clone(), Value::Object(entry));
        }
        questions
    }
}

/// Edges per computer, kept across restarts (`AttentionTracker`).
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    hosts: Map<String, Value>,
    optional_questions: bool,
}

impl Tracker {
    pub fn new(saved: Map<String, Value>) -> Self {
        Self { hosts: saved, optional_questions: false }
    }

    /// Banners for new optional questions (off by default).
    pub fn set_optional_questions(&mut self, on: bool) {
        self.optional_questions = on;
    }

    pub fn state(&self) -> &Map<String, Value> {
        &self.hosts
    }

    /// `AttentionTracker::observe` (`AttentionTracker.cpp:86-288`).
    pub fn observe(&mut self, host: &Host, now: i64, focused: bool, notice_id: &mut dyn FnMut() -> String) -> Changes {
        let previous = self.hosts.get(host.alias).and_then(Value::as_object).cloned().unwrap_or_default();
        if !host.ok {
            return self.observe_offline(host, previous);
        }
        let mut pass = Pass { host, previous: &previous, current: Map::new(), raised: Vec::new(), now, focused, notice_id };
        for row in host.rows.values() {
            pass.session(row);
        }
        // Posted first, as Qt does (`AttentionTracker.cpp:278-280`).
        let mut raised = pass.questions(self.optional_questions);
        raised.append(&mut pass.raised);
        let current = pass.current;
        let mut changes = Changes { raised, cleared: Vec::new() };
        for (key, value) in &previous {
            let token = value.get("token").and_then(Value::as_str).unwrap_or("");
            if !token.is_empty() && current.get(key).and_then(|entry| entry.get("token")).and_then(Value::as_str) != Some(token) {
                changes.cleared.push(token.to_owned());
            }
        }
        self.hosts.insert(host.alias.to_owned(), Value::Object(current));
        changes
    }

    /// A failed or stale listing only withdraws muted banners and keeps every
    /// identity (`AttentionTracker.cpp:90-116`).
    fn observe_offline(&mut self, host: &Host, previous: Map<String, Value>) -> Changes {
        let mut changes = Changes::default();
        let mut muted_runs = BTreeSet::new();
        let mut muted_conversations = BTreeSet::new();
        for row in host.rows.values().filter(|row| muted(row) && row.state != "archived") {
            muted_runs.insert(identity(row));
            if let Some(conversation) = nonempty(&row.conversation_id) {
                muted_conversations.insert(format!("{}:{conversation}", row.notify.cmd));
            }
        }
        let mut retained = previous.clone();
        for entry in retained.values_mut().filter_map(Value::as_object_mut) {
            let token = text(entry, "token").to_owned();
            let Some(destination) = target(&token) else { continue };
            if text(&destination, "host") != host.alias {
                continue;
            }
            let conversation = !text(&destination, "reply").is_empty() || !text(&destination, "question").is_empty();
            let is_muted = if conversation {
                muted_conversations.contains(&format!("{}:{}", text(&destination, "agent"), text(&destination, "conversation")))
            } else {
                let key = match text(&destination, "run") {
                    "" => format!("name:{}:{}", text(&destination, "name"), integer(&destination, "created")),
                    run => format!("run:{run}"),
                };
                muted_runs.contains(&key)
            };
            if is_muted {
                changes.cleared.push(token);
                entry.remove("token");
                entry.remove("retry_at");
            }
        }
        if retained != previous {
            self.hosts.insert(host.alias.to_owned(), Value::Object(retained));
        }
        changes
    }

    /// `AttentionTracker::deliveryFailed` (`AttentionTracker.cpp:290-303`).
    pub fn delivery_failed(&mut self, token: &str, now: i64) {
        for entries in self.hosts.values_mut().filter_map(Value::as_object_mut) {
            for entry in entries.values_mut().filter_map(Value::as_object_mut) {
                if entry.get("token").and_then(Value::as_str) == Some(token) {
                    entry.insert("retry_at".into(), Value::from(now + RETRY_SECONDS));
                    return;
                }
            }
        }
    }
}

/// The notification request's identifier (`MacAttentionNotifier.mm:79`, `:102`).
pub fn request_id(token: &str) -> String {
    format!("zerus.attention.{token}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryStep {
    CheckAuthorization,
    Add { token: String, title: String, body: String },
    Remove { token: String },
    Fail { token: String, detail: String },
}

/// The macOS notifier's state (`MacAttentionNotifier.mm:51-114`): one
/// authorization check at a time; tokens withdrawn meanwhile are not posted.
#[derive(Debug, Default)]
pub struct Delivery {
    desired: BTreeSet<String>,
    pending: BTreeMap<String, (String, String)>,
    checking: bool,
}

impl Delivery {
    pub fn post(&mut self, token: &str, title: &str, body: &str) -> Vec<DeliveryStep> {
        if !self.desired.insert(token.to_owned()) {
            return Vec::new();
        }
        self.pending.insert(token.to_owned(), (title.to_owned(), body.to_owned()));
        if self.checking {
            return Vec::new();
        }
        self.checking = true;
        vec![DeliveryStep::CheckAuthorization]
    }

    pub fn withdraw(&mut self, token: &str) -> Vec<DeliveryStep> {
        self.desired.remove(token);
        self.pending.remove(token);
        vec![DeliveryStep::Remove { token: token.to_owned() }]
    }

    pub fn authorized(&mut self, granted: bool, error: Option<String>) -> Vec<DeliveryStep> {
        self.checking = false;
        let pending = std::mem::take(&mut self.pending);
        if !granted {
            let detail = error.filter(|error| !error.is_empty()).unwrap_or_else(|| DENIED.to_owned());
            return pending
                .into_keys()
                .map(|token| {
                    self.desired.remove(&token);
                    DeliveryStep::Fail { token, detail: detail.clone() }
                })
                .collect();
        }
        pending
            .into_iter()
            .filter(|(token, _)| self.desired.contains(token))
            .map(|(token, (title, body))| DeliveryStep::Add { token, title, body })
            .collect()
    }

    /// An add finished: a token withdrawn meanwhile goes, an error fails it.
    pub fn added(&mut self, token: &str, error: Option<String>) -> Vec<DeliveryStep> {
        if !self.desired.contains(token) {
            return self.withdraw(token);
        }
        match error.filter(|error| !error.is_empty()) {
            Some(detail) => {
                self.desired.remove(token);
                vec![DeliveryStep::Fail { token: token.to_owned(), detail }]
            }
            None => Vec::new(),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path core/Cargo.toml -- attention:: notify_facts`
Expected: PASS (18 attention tests and both view tests).

- [ ] **Step 6: Run the task's checks and commit**

Run core fmt, clippy, the core tests, and both src-tauri clippy variants. `NotifyFacts` grew; `ViewRow` literals use defaults.

```bash
git add client/core/src/view.rs client/core/src/attention.rs client/core/src/lib.rs
git commit -m "feat(client): port Qt's attention tracker, preferences check and notifier state"
```

---

### Task 6: Attention notifications on macOS (zerus-5wmh.43)

The client posts Qt's attention notifications under the app's identity. That is a small objc2 module around `UNUserNotificationCenter`, as spec row 290 asks, and not `tauri-plugin-notification`.
- **Desk thread.** It observes every changed computer, and every computer each 10 s so retries fire. It persists the state and withdraws, then posts, on the main thread (`TrayAgent.cpp:551-564`).
- **Clicks.** A click opens the session or reports it gone (`TrayAgent.cpp:166-193`, `:593-608`). A failed delivery is retried and noted in the window (`TrayAgent.cpp:195-201`).
- **Removed peers.** A peer that leaves the fleet has its banners withdrawn (`TrayAgent.cpp:458-462`).
- **Focus.** Window focus is the foreground test (`AttentionTracker.cpp:80-84`).
- **Who posts.** Only the switch identity posts, unless `ZERUS_CLIENT_NOTIFICATIONS=1` is set (Decision 7).
- **Seeding.** The tracker is seeded from Qt's `attention.observed` the first time (Decision 11).
- **Device marks.** The desk reads `AttentionMarks` (`client/core/src/marks.rs`) and applies Task 5's overlay to its attention count, Dock badge and tooltip. It gets an input whenever the marks or the pending read marks change: `update_attention_marks`, `queue_read_marks`, and the read-mark worker when a mark settles. It then offers the status item again, so the counts follow a click in the window and are right while the window is closed.

**Files:**
- Modify: `client/src-tauri/src/identity.rs` (`notifications`)
- Modify: `client/src-tauri/src/platform/macos.rs` (`qt_default_data`, `qt_default_bool`)
- Create: `client/src-tauri/src/platform/macos_notify.rs`; modify `platform/mod.rs`
- Modify: `client/src-tauri/src/desk.rs` (inputs, attention, run loop, `observe_round`, `set_focused`, `sender`)
- Modify: `client/src-tauri/src/lifecycle.rs` (focus), `client/src-tauri/src/lib.rs` (setup), `client/src-tauri/Cargo.toml`
- Test: unit tests in `identity.rs` and `desk.rs`

**Interfaces:**
- Consumes: `attention::*` (Task 5); `desk::{start, open_session, window_notice, DeskState, Pending}` (Task 4); `lifecycle::show_main` (Task 3).
- Produces:
  - `identity::Identity::notifications(requested: bool, replay: bool) -> bool`.
  - `macos::{qt_default_data(&str) -> Option<Vec<u8>>, qt_default_bool(&str) -> Option<bool>}`.
  - `macos_notify::{install(&AppHandle, Sender<DeskInput>), deliver(&AppHandle, Vec<String>, Vec<Notice>), NotifierState}`.
  - `desk::{DeskInput::{Clicked(String), Failed { token, detail }}, Attention { tracker, path }, ATTENTION_FILE, SWEEP, observe_round(..) -> Changes, set_focused(&AppHandle, bool), sender(&AppHandle) -> Option<Sender<DeskInput>>}`; `desk::start` gains `attention: Option<Attention>`.

- [ ] **Step 1: Write the failing tests**

Add to the tests of `client/src-tauri/src/identity.rs`:

```rust
    #[test]
    fn only_the_switch_bundle_posts_unless_asked() {
        assert!(Identity::new("com.hgdev.hgs-tray").notifications(false, false));
        assert!(!Identity::new("com.hgdev.hgs-tray").notifications(false, true), "a replay never posts");
        assert!(!Identity::new("dev.zerus.next").notifications(false, false), "no double banners beside Qt");
        assert!(Identity::new("dev.zerus.next").notifications(true, false));
    }
```

Add to the tests of `client/src-tauri/src/desk.rs`:

```rust
    use std::collections::BTreeSet;
    use zerus_client_core::attention::Tracker;
    use zerus_client_core::{ComputerStatus, ViewRow};

    fn asking(name: &str) -> ViewRow {
        let mut row = ViewRow {
            name: name.into(), state: "running".into(), activity: "waiting".into(), phase: Some("approval".into()),
            run_id: Some(format!("run-{name}")), ..ViewRow::default()
        };
        row.notify.needs_action = true;
        row.notify.attention_id = "r".into();
        row
    }

    fn listed(desk: &mut Desk, computer: &str, rows: Vec<ViewRow>) {
        let id = ComputerId::from(computer);
        desk.observe(&Change::Replace { computer: id.clone(), rows: rows.into_iter().map(|row| (row.name.clone(), row)).collect() });
        desk.observe(&Change::Computer { computer: id, status: ComputerStatus::Live });
    }

    #[test]
    fn a_round_observes_listed_computers_and_withdraws_removed_peers() {
        let mut desk = Desk::default();
        listed(&mut desk, "", vec![asking("claude/p/here")]);
        listed(&mut desk, "mac", vec![asking("claude/p/there")]);
        desk.observe(&Change::Computer { computer: ComputerId::from("studio"), status: ComputerStatus::Probing });
        let mut tracker = Tracker::new(serde_json::Map::new());
        let mut next = 0;
        let mut id = || { next += 1; format!("notice-{next}") };
        let first = observe_round(&mut tracker, &desk, None, 1000, false, &mut id);
        assert_eq!(first.raised.len(), 2, "a computer that has not listed is skipped");
        let mac_token = first.raised.iter().find(|notice| notice.body.contains(" / mac\n")).unwrap().token.clone();
        desk.observe(&Change::Computer { computer: ComputerId::from("mac"), status: ComputerStatus::Ended });
        let only: BTreeSet<ComputerId> = [ComputerId::from("mac")].into();
        let removed = observe_round(&mut tracker, &desk, Some(&only), 1001, false, &mut id);
        assert_eq!(removed.cleared, vec![mac_token], "a peer that left withdraws its banners");
        let local_only: BTreeSet<ComputerId> = [ComputerId::local()].into();
        assert!(observe_round(&mut tracker, &desk, Some(&local_only), 1002, false, &mut id).raised.is_empty());
    }
```

The local label comes from the machine card's host; with no card, notices name it with an empty host, as Qt's `box.host` would be before the first listing.

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- only_the_switch a_round_observes`
Expected: FAIL to compile (`notifications`, `observe_round` missing).

- [ ] **Step 2: Add `Identity::notifications`**

In `impl Identity` of `client/src-tauri/src/identity.rs`:

```rust
    /// Whether attention notifications post: the switch bundle, or another
    /// build on request (`ZERUS_CLIENT_NOTIFICATIONS=1`); never a replay.
    pub fn notifications(&self, requested: bool, replay: bool) -> bool {
        !replay && (self.is_switch() || requested)
    }
```

- [ ] **Step 3: Extend the desk thread**

In `client/src-tauri/src/desk.rs`, remove the `#[allow(dead_code)]` Task 4 put on `window_notice`; the run loop below calls it. Then:
- add the imports `std::collections::{BTreeMap, BTreeSet}`, `std::sync::atomic::{AtomicBool, Ordering}`, `std::sync::Arc`, `std::sync::mpsc::RecvTimeoutError`, `std::time::{Instant, SystemTime, UNIX_EPOCH}`, `serde_json::{Map, Value}` and `zerus_client_core::attention::{self, Changes, Host, Resolution, Tracker, CLICK_WAIT}`;
- extend `DeskInput`, `DeskState` and `start`;
- replace `run`;
- add the rest:

```rust
pub enum DeskInput {
    Change(Box<Change>),
    Mode(OpenMode),
    /// A notification's default action (`MacAttentionNotifier.mm:20-28`).
    Clicked(String),
    /// A delivery failed (`AttentionNotifier::failed`).
    Failed { token: String, detail: String },
}

/// Every listed computer is observed again at least this often, so retries fire.
pub const SWEEP: Duration = Duration::from_secs(10);
/// The tracker's state, in the data directory.
pub const ATTENTION_FILE: &str = "attention-observed.json";

/// Attention notifications, when this build posts them.
pub struct Attention {
    pub tracker: Tracker,
    pub path: PathBuf,
}

impl Attention {
    fn save(&self) {
        let written = serde_json::to_vec(self.tracker.state())
            .map_err(std::io::Error::other)
            .and_then(|bytes| write_private(&self.path, &bytes));
        if let Err(error) = written {
            eprintln!("zerus-client: could not save {}: {error}", self.path.display());
        }
    }
}
```

Add `focused: Arc<AtomicBool>,` to `DeskState`. Change `start` to:

```rust
pub fn start(app: &AppHandle, pump: &crate::pump::Pump, dir: PathBuf, mode: OpenMode, attention: Option<Attention>) {
    let (sender, inputs) = mpsc::channel();
    let tap = sender.clone();
    pump.tap(Box::new(move |change: &Change| {
        if menu_bar_change(change) {
            let _ = tap.send(DeskInput::Change(Box::new(change.clone())));
        }
    }));
    let focused = Arc::new(AtomicBool::new(false));
    app.manage(DeskState {
        sender: Mutex::new(sender),
        mode: Mutex::new(mode),
        dir,
        inbox: Mutex::new(Inbox::default()),
        focused: Arc::clone(&focused),
    });
    let handle = app.clone();
    std::thread::Builder::new()
        .name("zerus-client-desk".into())
        .spawn(move || run(handle, inputs, mode, attention, focused))
        .expect("the desk thread starts");
}

/// The desk thread's input, for the notifier's clicks and failures.
pub fn sender(app: &AppHandle) -> Option<Sender<DeskInput>> {
    app.try_state::<DeskState>().map(|state| lock(&state.sender).clone())
}

/// The window's focus: a focused Zerus consumes edges without banners.
pub fn set_focused(app: &AppHandle, focused: bool) {
    if let Some(state) = app.try_state::<DeskState>() {
        state.focused.store(focused, Ordering::Relaxed);
    }
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// One round of the tracker. Each listed computer is observed: all of them on
/// a sweep, else the changed ones. Each changed peer that left the fleet is
/// observed with no rows, so its banners go (`TrayAgent.cpp:458-462`).
pub fn observe_round(
    tracker: &mut Tracker,
    desk: &Desk,
    only: Option<&BTreeSet<ComputerId>>,
    now: i64,
    focused: bool,
    notice_id: &mut dyn FnMut() -> String,
) -> Changes {
    let mut all = Changes::default();
    let mut add = |changes: Changes| {
        all.raised.extend(changes.raised);
        all.cleared.extend(changes.cleared);
    };
    for (computer, state) in desk.computers() {
        if !state.listed || only.is_some_and(|only| !only.contains(computer)) {
            continue;
        }
        let label = computer.alias().map_or_else(|| state.host.clone().unwrap_or_default(), str::to_owned);
        add(tracker.observe(
            &Host { alias: &computer.0, label: &label, ok: state.fresh(), rows: &state.rows },
            now,
            focused,
            &mut *notice_id,
        ));
    }
    let empty = BTreeMap::new();
    for computer in only.into_iter().flatten() {
        if computer.alias().is_some() && !desk.computers().contains_key(computer) {
            add(tracker.observe(&Host { alias: &computer.0, label: &computer.0, ok: true, rows: &empty }, now, focused, &mut *notice_id));
        }
    }
    all
}

fn run(app: AppHandle, inputs: Receiver<DeskInput>, mut mode: OpenMode, mut attention: Option<Attention>, focused: Arc<AtomicBool>) {
    let mut desk = Desk::default();
    let mut dirty: BTreeSet<ComputerId> = BTreeSet::new();
    let mut click: Option<(Map<String, Value>, Instant)> = None;
    let mut swept = Instant::now();
    // The menu with Open Zerus and Quit before any change arrives, as Qt builds
    // its menu at once (`TrayAgent.cpp:252-254`): tray-icon 0.25.1 shows no menu
    // while it is empty, so a fleet that never starts would leave no Quit.
    publish(&app, &desk, mode);
    loop {
        let wait = if click.is_some() { Duration::from_secs(1) } else { SWEEP };
        let first = match inputs.recv_timeout(wait) {
            Ok(input) => Some(input),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut menu_changed = false;
        for input in first.into_iter().chain(std::iter::from_fn(|| inputs.try_recv().ok())) {
            match input {
                DeskInput::Change(change) => {
                    if let Some(computer) = desk.observe(&change) {
                        dirty.insert(computer);
                        menu_changed = true;
                    }
                }
                DeskInput::Mode(chosen) => {
                    mode = chosen;
                    // Task 4: the desk thread saves the choice, off the main thread.
                    store_mode(&app, chosen);
                    menu_changed = true;
                }
                DeskInput::Clicked(token) => {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        let _ = crate::lifecycle::show_main(&handle);
                    });
                    click = attention::target(&token).map(|target| (target, Instant::now() + CLICK_WAIT));
                }
                DeskInput::Failed { token, detail } => {
                    eprintln!("zerus-client: notification: {detail}");
                    if let Some(attention) = attention.as_mut() {
                        attention.tracker.delivery_failed(&token, unix_now());
                        attention.save();
                    }
                    window_notice(&app, "Zerus", &format!("System notification was not delivered: {detail}"));
                }
            }
        }
        if menu_changed {
            publish(&app, &desk, mode);
        }
        if let Some(attention) = attention.as_mut() {
            let sweep = swept.elapsed() >= SWEEP;
            if sweep {
                swept = Instant::now();
            }
            let before = attention.tracker.state().clone();
            let changes = observe_round(
                &mut attention.tracker,
                &desk,
                (!sweep).then_some(&dirty),
                unix_now(),
                focused.load(Ordering::Relaxed),
                &mut || uuid::Uuid::new_v4().to_string(),
            );
            if attention.tracker.state() != &before {
                attention.save();
            }
            deliver(&app, changes);
        }
        dirty.clear();
        let outcome = click.as_ref().map(|(target, deadline)| attention::resolve(target, &desk, Instant::now() >= *deadline));
        match outcome {
            Some(Resolution::Open { computer, key }) => {
                open_session(&app, &computer, &key);
                click = None;
            }
            Some(Resolution::Gone(name)) => {
                window_notice(&app, "Zerus", &attention::gone_text(&name));
                click = None;
            }
            Some(Resolution::Waiting) | None => {}
        }
        std::thread::sleep(PACE);
    }
}

/// Withdraws, then posts, on the main thread (`TrayAgent.cpp:558-559`).
fn deliver(app: &AppHandle, changes: Changes) {
    if changes.raised.is_empty() && changes.cleared.is_empty() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            crate::platform::macos_notify::deliver(&handle, changes.cleared, changes.raised)
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, changes);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- only_the_switch a_round_observes desk::`
Expected: PASS.

- [ ] **Step 5: Write the macOS notifier**

Create `client/src-tauri/src/platform/macos_notify.rs` and add `#[cfg(all(target_os = "macos", feature = "live"))] pub mod macos_notify;` to `platform/mod.rs` (the live gate, as for `macos_menubar`):

```rust
//! Attention notifications under the app's identity, as Qt posts them
//! (`tray/src/MacAttentionNotifier.mm`). `UNUserNotificationCenter` gets one
//! request per token (`zerus.attention.<token>`) with the default sound,
//! withdrawn from pending and delivered when the attention clears; the
//! default action opens the token's session. The decisions live in
//! `attention::Delivery`; this module only talks to the system.
use std::ptr::NonNull;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNNotificationSettings, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use tauri::{AppHandle, Manager};
use zerus_client_core::attention::{self, Delivery, DeliveryStep, Notice};

use crate::desk::DeskInput;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Ivars {
    desk: Mutex<Sender<DeskInput>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; Delegate has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "ZerusAttentionDelegate"]
    #[ivars = Ivars]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Banners also while Zerus is in front (`MacAttentionNotifier.mm:14-19`).
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        /// The default action opens the token's session (`MacAttentionNotifier.mm:20-28`).
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            if let Some(token) = clicked_token(response) {
                let _ = lock(&self.ivars().desk).send(DeskInput::Clicked(token));
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new(desk: Sender<DeskInput>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { desk: Mutex::new(desk) });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

fn clicked_token(response: &UNNotificationResponse) -> Option<String> {
    // SAFETY: the framework's constant action identifier.
    let default = unsafe { UNNotificationDefaultActionIdentifier };
    if !response.actionIdentifier().isEqualToString(default) {
        return None;
    }
    let info = response.notification().request().content().userInfo();
    let key: &AnyObject = ns_string!("hgsAttentionToken");
    let value = info.objectForKey(key)?;
    value.downcast::<NSString>().ok().map(|token| token.to_string())
}

/// The delivery state, on the main thread.
pub struct NotifierState {
    delivery: Mutex<Delivery>,
    desk: Mutex<Sender<DeskInput>>,
}

/// Sets the delegate before the first notification posts; a click that
/// launched the app reaches it once launching finishes.
pub fn install(app: &AppHandle, desk: Sender<DeskInput>) {
    let delegate = Delegate::new(desk.clone());
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center keeps a weak reference: the delegate lives as long as the app.
    std::mem::forget(delegate);
    app.manage(NotifierState { delivery: Mutex::new(Delivery::default()), desk: Mutex::new(desk) });
}

/// Withdraws, then posts (`TrayAgent::observeAttention`).
pub fn deliver(app: &AppHandle, cleared: Vec<String>, raised: Vec<Notice>) {
    let Some(state) = app.try_state::<NotifierState>() else { return };
    let mut steps = Vec::new();
    {
        let mut delivery = lock(&state.delivery);
        for token in &cleared {
            steps.extend(delivery.withdraw(token));
        }
        for notice in &raised {
            steps.extend(delivery.post(&notice.token, &notice.title, &notice.body));
        }
    }
    execute(app, &state, steps);
}

fn execute(app: &AppHandle, state: &NotifierState, steps: Vec<DeliveryStep>) {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    for step in steps {
        match step {
            DeliveryStep::CheckAuthorization => check_authorization(app, &center),
            DeliveryStep::Add { token, title, body } => add(app, &center, token, &title, &body),
            DeliveryStep::Remove { token } => {
                let ids = NSArray::from_retained_slice(&[NSString::from_str(&attention::request_id(&token))]);
                center.removePendingNotificationRequestsWithIdentifiers(&ids);
                center.removeDeliveredNotificationsWithIdentifiers(&ids);
            }
            DeliveryStep::Fail { token, detail } => {
                let _ = lock(&state.desk).send(DeskInput::Failed { token, detail });
            }
        }
    }
}

/// Completion handlers run on the system's queues; the state lives on the main thread.
fn on_main(app: &AppHandle, work: impl FnOnce(&AppHandle, &NotifierState) + Send + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(state) = handle.try_state::<NotifierState>() {
            work(&handle, &state);
        }
    });
}

fn describe(error: *mut NSError) -> Option<String> {
    // SAFETY: the system passes null or a valid error.
    unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string())
}

/// Settings first; a request only while undetermined, for alerts and sound
/// (`MacAttentionNotifier.mm:58-75`).
fn check_authorization(app: &AppHandle, center: &UNUserNotificationCenter) {
    let app = app.clone();
    let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: the system passes a valid settings object.
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        if status == UNAuthorizationStatus::NotDetermined {
            let app = app.clone();
            let reply = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let detail = describe(error);
                on_main(&app, move |app, state| {
                    let steps = lock(&state.delivery).authorized(granted.as_bool(), detail);
                    execute(app, state, steps);
                });
            });
            UNUserNotificationCenter::currentNotificationCenter()
                .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &reply);
        } else {
            let granted = status == UNAuthorizationStatus::Authorized || status == UNAuthorizationStatus::Provisional;
            on_main(&app, move |app, state| {
                let steps = lock(&state.delivery).authorized(granted, None);
                execute(app, state, steps);
            });
        }
    });
    center.getNotificationSettingsWithCompletionHandler(&block);
}

/// One request per token, the default sound and the token in `userInfo`
/// (`MacAttentionNotifier.mm:94-113`).
fn add(app: &AppHandle, center: &UNUserNotificationCenter, token: String, title: &str, body: &str) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let value = NSString::from_str(&token);
    let info = NSDictionary::<NSString, NSString>::from_slices(&[ns_string!("hgsAttentionToken")], &[&*value]);
    // SAFETY: a dictionary of strings is a valid userInfo; the cast only drops its element types.
    unsafe {
        let info: Retained<NSDictionary> = Retained::cast_unchecked(info);
        content.setUserInfo(&info);
    }
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&attention::request_id(&token)),
        &content,
        None,
    );
    let app = app.clone();
    let done = RcBlock::new(move |error: *mut NSError| {
        let detail = describe(error);
        let token = token.clone();
        on_main(&app, move |app, state| {
            let steps = lock(&state.delivery).added(&token, detail);
            execute(app, state, steps);
        });
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
}
```

Add to `client/src-tauri/src/platform/macos.rs`:

```rust
/// A value Qt's QSettings saved in this bundle's defaults domain
/// (`com.hgdev.hgs-tray`): `group/key` is stored as `group.key`, a
/// QByteArray as data. Read only (Decision 11). Only the live build posts
/// notifications, so only it reads Qt's values.
#[cfg(feature = "live")]
pub fn qt_default_data(key: &str) -> Option<Vec<u8>> {
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    let key = objc2_foundation::NSString::from_str(key);
    if let Some(data) = defaults.dataForKey(&key) {
        return Some(data.to_vec());
    }
    defaults.stringForKey(&key).map(|text| text.to_string().into_bytes())
}

/// A boolean QSettings value of this bundle's defaults domain, read only.
#[cfg(feature = "live")]
pub fn qt_default_bool(key: &str) -> Option<bool> {
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    let key = objc2_foundation::NSString::from_str(key);
    defaults.objectForKey(&key).map(|_| defaults.boolForKey(&key))
}
```

In `client/src-tauri/Cargo.toml`, macOS table:
- add `objc2-user-notifications = { version = "0.3.2", default-features = false, features = ["std", "block2", "UNNotification", "UNNotificationContent", "UNNotificationRequest", "UNNotificationResponse", "UNNotificationSettings", "UNNotificationSound", "UNNotificationTrigger", "UNUserNotificationCenter"] }`;
- extend `objc2-foundation` with `"NSData", "NSError", "NSUserDefaults"`.

Record `objc2-user-notifications` in the lock (Global Constraints, "Cargo.lock"); it must gain no `[[package]]`. As before, add only features the compiler names.

- [ ] **Step 6: Wire focus and the notifier into the app**

In `client/src-tauri/src/lifecycle.rs`, add this arm to `on_event` before `RunEvent::Exit`:

```rust
        RunEvent::WindowEvent { label, event: WindowEvent::Focused(focused), .. } if label == MAIN_WINDOW => {
            #[cfg(feature = "live")]
            crate::desk::set_focused(app, *focused);
            #[cfg(not(feature = "live"))]
            let _ = focused;
        }
```

In the macOS block of `launch`'s setup (Task 4, Step 10), build the attention state and install the notifier around `desk::start`:

```rust
                    // `replay` is the live block's binding (Task 3, Step 7).
                    let attention = identity
                        .notifications(identity::flag("ZERUS_CLIENT_NOTIFICATIONS"), replay)
                        .then(|| {
                            let path = dir.join(desk::ATTENTION_FILE);
                            let own = std::fs::read(&path).ok();
                            // Qt's edges seed the switch bundle's first start (Decision 11).
                            let qt = identity.is_switch().then(|| platform::macos::qt_default_data("attention.observed")).flatten();
                            let mut tracker = zerus_client_core::attention::Tracker::new(
                                zerus_client_core::attention::seed(own.as_deref(), qt.as_deref()),
                            );
                            tracker.set_optional_questions(
                                identity.is_switch()
                                    && platform::macos::qt_default_bool("notifications.optionalQuestions").unwrap_or(false),
                            );
                            desk::Attention { tracker, path }
                        });
                    let notifies = attention.is_some();
                    desk::start(app.handle(), &pump, dir.clone(), desk::load_mode(&dir, conf.as_deref()), attention);
                    if notifies {
                        if let Some(sender) = desk::sender(app.handle()) {
                            platform::macos_notify::install(app.handle(), sender);
                        }
                    }
```

This replaces the single `desk::start(...)` line of Task 4.

- [ ] **Step 7: Check on the Mac, display-free**

Run **mac-lock**, **mac-sync**, **mac-clippy**, **mac-test** and **mac-done**.
Expected: clean. The notifier cannot be exercised without a bundle and a display; the runbook's display checks cover posting, withdrawing, clicking and the carried-over grant.

- [ ] **Step 8: Run the task's checks and commit**

Run fmt, both Linux clippy variants, the src-tauri tests, `check-ui-separators.py` and `check-source-privacy.py`.

```bash
git add client/src-tauri/src/identity.rs client/src-tauri/src/desk.rs client/src-tauri/src/lifecycle.rs \
  client/src-tauri/src/lib.rs client/src-tauri/src/platform client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock
git commit -m "feat(client): attention notifications on macOS with Qt's rules, seed and click routing"
```

---

### Task 7: The updater's drafts roots, rollback slot, Dev guard, settings check and rollback command (zerus-5wmh.12.1, then the updater part of zerus-5wmh.12.2)

`tray/update-installed.py` keeps its contract (quit, snapshot, replace, start, wait for the socket, verify, provenance) and learns:

**zerus-5wmh.12.1, client drafts root.** When the installed or the new bundle is the Tauri client (Info.plist `ZerusGUI = tauri`), its drafts root is snapshotted and verified next to Qt's. The root is `<app data>/drafts` under the bundle's identifier (Decision 5). Each root gets a private folder in the snapshot: 0700 folders, 0600 files, symbolic links refused. A nonempty draft that goes missing or changes keeps the snapshot and is reported with its root.

**zerus-5wmh.12.2, updater part.**
- **Rollback slot.** Keep Qt as the one rollback copy across client updates (Decision 12), and unregister a moved copy from LaunchServices.
- **Dev guard.** Refuse to replace a numbered Dev install with a build that has neither a Dev number nor a local preview label (Decision 13). `--check BUILT INSTALLED` applies the same guard for `HGS_TRAY_RESTART=0` installs, which replace the GUI on disk without the live update, and on macOS refuses a change between Qt and the client there, so a switch always takes the full flow. `docs/reference.md` states the rule in the sentence the refusal quotes (`DEV_GUARD`).
- **Settings snapshot.** At a Qt-to-client switch, snapshot Qt's settings domain beside the rollback copy, through `defaults export` (cfprefsd), never the plist file.
- **Settings check.** Report every Qt key that changed, every unexpected non-Cocoa key, and, as information, the Cocoa keys written. Client updates compare after the new client starts; a rollback compares before Qt starts, since Qt then writes its own keys.
- **`--rollback INSTALLED`.** Reinstalls exactly the rollback copy through the same flow.
- **`--installed-gui INSTALLED`.** Prints `qt`, `tauri` or `none` for `tray/install.sh`.

**Files:**
- Modify: `tray/update-installed.py`
- Modify: `tray/install.sh` (the `HGS_TRAY_RESTART=0` branches of macOS and Linux ask `--check` first)
- Modify: `docs/reference.md` (the guard and the local preview label, in "Installation, configuration and diagnostics")
- Create: `tests/test_tray_update_switch.py` (Linux and macOS, in-process with patched paths)
- Modify: `tests/test_tray_update.py` (snapshot layout, the guard through the updater and `HGS_TRAY_RESTART=0`, rollback end to end on Linux)
- Create: `tests/test_macos_switch_updater.py` (macOS only: codesign with ad-hoc fake bundles)

**Interfaces:**
- Consumes: the bundle keys of Task 9 (`ZerusGUI`, `CFBundleIdentifier`), `--build-info`'s `devBuildNumber` and `previewLabel` (Task 2, and Qt's `UpdateController::buildInfo`) and the client's drafts format (Qt's v1, `draft.json` and `<sha256>.bin`).
- Produces:
  - Constants: `LSREGISTER`, `DEFAULTS`, `PREFERENCES`, `SETTINGS_SNAPSHOT`, `COCOA_PREFIXES`.
  - Bundles and roots: `bundle_info(path) -> dict`, `gui_kind(info) -> 'qt' | 'tauri'`, `client_drafts_root(identifier) -> Path`, `draft_roots(*infos) -> list[(label, Path)]`, `snapshot(destination, roots) -> int`, `changed_drafts(saved, roots) -> (int, list[str])`.
  - Slot and LaunchServices: `keeps_slot(installed_kind, slot_kind, new_kind) -> bool`, `register(bundle)`, `unregister(bundle)`, `replace(stage, installed, aside=None) -> Path`, `roll_back(installed, previous) -> bool`.
  - Dev guard: `DEV_GUARD` (the sentence `docs/reference.md` repeats), `installed_build_info(path) -> dict | None`, `numbered_refusal(installed_info, new_info) -> str | None`, `check_replace(built, installed) -> int`.
  - Settings: `export_settings() -> (bytes, dict) | None`, `cocoa_key(key) -> bool`, `settings_changes(before, after) -> (changed, added, cocoa)`, `save_settings_snapshot()`, `settings_report(rollback=False) -> (problems, notes)`.
  - Entry points: `update(built, installed, rollback=False)`, `roll_back_installed(installed)`, `check_replace(built, installed)`, and the `--rollback`, `--installed-gui` and `--check` options.

- [ ] **Step 1: Write the failing unit tests**

Create `tests/test_tray_update_switch.py`:

```python
"""The updater's switch rules (tray/update-installed.py): GUI kinds, drafts
roots, the rollback slot, the numbered Dev guard and the Qt settings check.
Fake HOME, fake bundles (com.example.zerus-switch-test) and a fake `defaults`;
nothing reaches an installed GUI, its LaunchAgent or a real settings domain."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('update_installed', ROOT / 'tray/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)

TEST_ID = 'com.example.zerus-switch-test'
DEFAULTS = '''#!{python}
import pathlib, sys
domain = pathlib.Path({root!r}) / 'domain.plist'
assert sys.argv[1:] == ['export', 'com.hgdev.hgs-tray', '-'], sys.argv
if not domain.exists():
    sys.stderr.write('Domain com.hgdev.hgs-tray does not exist\\n')
    sys.exit(1)
sys.stdout.buffer.write(domain.read_bytes())
'''


class Fixture(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='zts-')).resolve()
        self.addCleanup(shutil.rmtree, self.root, True)
        self.home = self.root / 'home'
        self.home.mkdir()
        rollback = self.home / '.local/share/hgs/desktop-previous'
        self.defaults = self.root / 'defaults'
        self.defaults.write_text(DEFAULTS.format(python=sys.executable, root=str(self.root)))
        self.defaults.chmod(0o755)
        self.lsregister = self.root / 'lsregister'
        self.lsregister.write_text(f'#!/bin/sh\necho "$@" >> {self.root}/lsregister.log\n')
        self.lsregister.chmod(0o755)
        for name, value in {'HOME': self.home, 'ROLLBACK': rollback,
                            'SETTINGS_SNAPSHOT': rollback / 'com.hgdev.hgs-tray.plist',
                            'SNAPSHOTS': self.home / '.local/state/hgs/desktop-update',
                            'LSREGISTER': self.lsregister, 'DEFAULTS': str(self.defaults)}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        environment = mock.patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.home / '.config'),
                                                   'XDG_DATA_HOME': str(self.home / '.local/share')})
        environment.start()
        self.addCleanup(environment.stop)

    def bundle(self, name, kind, identifier=TEST_ID):
        app = self.root / 'bundles' / name / 'hgs-tray.app'
        info = {'CFBundleIdentifier': identifier, 'CFBundleExecutable': 'hgs-tray'}
        if kind == 'tauri':
            info['ZerusGUI'] = 'tauri'
        (app / 'Contents/MacOS').mkdir(parents=True)
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        (app / 'Contents/MacOS/hgs-tray').write_text(name)
        return app

    def draft(self, root, name, text, blob=None):
        folder = root / name
        folder.mkdir(parents=True)
        attachments = []
        if blob is not None:
            digest = hashlib.sha256(blob).hexdigest()
            (folder / f'{digest}.bin').write_bytes(blob)
            attachments.append({'name': 'a.txt', 'mime': 'text/plain', 'reference': '[1]', 'sha256': digest, 'bytes': len(blob)})
        (folder / 'draft.json').write_text(json.dumps({'version': 1, 'key': name, 'text': text, 'attachments': attachments}))


class Switch(Fixture):
    def test_the_gui_kind_comes_from_the_info_plist(self):
        self.assertEqual(updater.gui_kind(updater.bundle_info(self.bundle('qt', 'qt'))), 'qt')
        self.assertEqual(updater.gui_kind(updater.bundle_info(self.bundle('client', 'tauri'))), 'tauri')
        self.assertEqual(updater.bundle_info(self.root / 'missing.app'), {})
        script = [sys.executable, str(ROOT / 'tray/update-installed.py'), '--installed-gui']
        for path, kind in ((self.bundle('cli', 'tauri'), 'tauri'), (self.root / 'none.app', 'none')):
            result = subprocess.run(script + [str(path)], capture_output=True, text=True, timeout=30,
                                    env=dict(os.environ, HOME=str(self.home)))
            self.assertEqual((result.returncode, result.stdout.strip()), (0, kind), result.stderr)

    def test_the_client_drafts_root_follows_the_bundle_identifier(self):
        with mock.patch.object(updater, 'MAC', True):
            self.assertEqual(updater.client_drafts_root('com.hgdev.hgs-tray'),
                             self.home / 'Library/Application Support/com.hgdev.hgs-tray/drafts')
        with mock.patch.object(updater, 'MAC', False):
            self.assertEqual(updater.client_drafts_root('dev.zerus.next'),
                             self.home / '.local/share/dev.zerus.next/drafts')

    def test_each_client_root_joins_qt_once(self):
        qt = {'CFBundleIdentifier': 'com.hgdev.hgs-tray'}
        client = {'CFBundleIdentifier': 'com.hgdev.hgs-tray', 'ZerusGUI': 'tauri'}
        self.assertEqual([label for label, _ in updater.draft_roots(qt, qt)], ['qt'])
        self.assertEqual([label for label, _ in updater.draft_roots(qt, client, client)],
                         ['qt', 'client-com.hgdev.hgs-tray'])

    def test_snapshots_and_checks_cover_every_root(self):
        client = {'CFBundleIdentifier': TEST_ID, 'ZerusGUI': 'tauri'}
        roots = updater.draft_roots({}, client)
        qt_root, client_root = roots[0][1], roots[1][1]
        self.draft(qt_root, 'qt-draft', 'from Qt')
        self.draft(client_root, 'client-draft', 'from the client', blob=b'attachment')
        self.draft(client_root, 'empty', '')
        saved = self.root / 'saved'
        self.assertEqual(updater.snapshot(saved, roots), 3)
        self.assertEqual(saved.stat().st_mode & 0o777, 0o700)
        self.assertEqual((saved / f'client-{TEST_ID}/client-draft/draft.json').stat().st_mode & 0o777, 0o600)
        self.assertEqual(updater.changed_drafts(saved, roots), (2, []))
        blob = next((client_root / 'client-draft').glob('*.bin'))
        blob.write_bytes(b'changed')
        shutil.rmtree(qt_root / 'qt-draft')
        preserved, problems = updater.changed_drafts(saved, roots)
        self.assertEqual(problems, ['qt-draft: missing', f'client-{TEST_ID} client-draft: attachment changed'])
        link = self.root / 'linked'
        (client_root / 'link').symlink_to(link)
        with self.assertRaisesRegex(updater.UpdateError, 'symbolic links'):
            updater.snapshot(self.root / 'again', roots)

    def test_the_slot_keeps_qt_only_after_the_switch(self):
        self.assertTrue(updater.keeps_slot('tauri', 'qt', 'tauri'))
        for kinds in (('qt', 'qt', 'tauri'), ('tauri', 'tauri', 'tauri'), ('tauri', None, 'tauri'),
                      ('tauri', 'qt', 'qt'), (None, 'qt', 'tauri'), ('qt', None, 'qt')):
            self.assertFalse(updater.keeps_slot(*kinds), kinds)

    def test_a_numbered_dev_install_is_never_replaced_by_an_unnumbered_build(self):
        numbered = {'version': '0.47.0', 'devBuildNumber': 1413}
        refusal = updater.numbered_refusal(numbered, {'version': '0.47.0', 'devBuildNumber': 0})
        self.assertIn('Dev build 1413', refusal)
        self.assertIn(updater.DEV_GUARD, refusal)
        self.assertIsNone(updater.numbered_refusal(numbered, {'devBuildNumber': 1414}))
        self.assertIsNone(updater.numbered_refusal(numbered, {'devBuildNumber': 0, 'previewLabel': 'task-board'}))
        self.assertIsNone(updater.numbered_refusal(None, {'devBuildNumber': 0}))
        self.assertIsNone(updater.numbered_refusal({'devBuildNumber': 0}, {'devBuildNumber': 0}))

    def test_the_refusal_says_what_the_reference_says(self):
        # The coordinator's ruling: the same wording in docs/reference.md and the refusal.
        words = lambda text: ' '.join(text.replace('`', '').split())
        self.assertIn(words(updater.DEV_GUARD), words((ROOT / 'docs/reference.md').read_text()))

    def test_an_on_disk_install_asks_the_updater_first(self):
        installed, built = self.bundle('installed', 'qt'), self.bundle('built', 'qt')
        infos = {installed: {'version': '0.47.0', 'devBuildNumber': 1413}, built: {'version': '0.47.0', 'devBuildNumber': 0}}
        with mock.patch.object(updater, 'build_info', lambda path: infos[Path(path).resolve()]), \
                mock.patch.object(updater, 'installed_build_info', lambda path: infos[Path(path).resolve()]):
            with self.assertRaisesRegex(updater.UpdateError, 'Dev build 1413.*Nothing was replaced'):
                updater.check_replace(built, installed)
            infos[built] = {'version': '0.47.0', 'devBuildNumber': 0, 'previewLabel': 'task-board'}
            self.assertEqual(updater.check_replace(built, installed), 0)
            client = self.bundle('client', 'tauri')
            infos[client] = {'version': '0.47.0', 'devBuildNumber': 1414}
            with mock.patch.object(updater, 'MAC', True):
                with self.assertRaisesRegex(updater.UpdateError, 'needs the live update'):
                    updater.check_replace(client, installed)
            self.assertEqual(updater.check_replace(built, self.root / 'missing.app'), 0, 'nothing installed yet')

    def test_settings_changes_separate_qt_keys_from_cocoa_keys(self):
        before = {'attention.observed': b'{}', 'workspace.reduceMotion': False, 'NSStatusItem Preferred Position Item-0': 900.0}
        after = {'attention.observed': b'{"x":1}', 'NSStatusItem Preferred Position Item-0': 880.0,
                 'NSStatusItem Visible Item-0': True, 'surprise.key': 1}
        self.assertEqual(updater.settings_changes(before, after),
                         (['attention.observed', 'workspace.reduceMotion'], ['surprise.key'],
                          ['NSStatusItem Preferred Position Item-0', 'NSStatusItem Visible Item-0']))
        self.assertEqual(updater.settings_changes(before, dict(before)), ([], [], []))

    def test_the_switch_keeps_and_checks_qt_settings_through_defaults(self):
        domain = self.root / 'domain.plist'
        with self.assertRaisesRegex(updater.UpdateError, 'cannot read the com.hgdev.hgs-tray settings'):
            updater.save_settings_snapshot()
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{}', 'NSWindow Frame main': 'x'}))
        updater.save_settings_snapshot()
        self.assertEqual(updater.SETTINGS_SNAPSHOT.stat().st_mode & 0o777, 0o600)
        self.assertEqual(updater.settings_report(), ([], []))
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{}', 'NSWindow Frame main': 'y',
                                           'NSStatusItem Visible Item-0': True}))
        self.assertEqual(updater.settings_report(),
                         ([], ['Cocoa settings written since the switch: NSStatusItem Visible Item-0, NSWindow Frame main']))
        domain.write_bytes(plistlib.dumps({'attention.observed': b'{"changed":1}'}))
        problems, _ = updater.settings_report(rollback=True)
        self.assertIn('Qt settings changed or missing: attention.observed', problems[0])
        self.assertIn(f'defaults import com.hgdev.hgs-tray {updater.SETTINGS_SNAPSHOT}', problems[1])


class SlotAndRestore(Fixture):
    def stage_of(self, bundle, installed):
        stage = installed.parent / '.stage' / installed.name
        shutil.copytree(bundle, stage)
        return stage

    def test_a_client_update_keeps_qt_as_the_rollback_copy_and_can_restore_the_old_client(self):
        installed = self.root / 'Applications/hgs-tray.app'
        shutil.copytree(self.bundle('client-1', 'tauri'), installed)
        slot = updater.ROLLBACK / 'hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), slot)
        aside = self.root / 'update/previous'
        previous = updater.replace(self.stage_of(self.bundle('client-2', 'tauri'), installed), installed, aside)
        self.assertEqual((installed / 'Contents/MacOS/hgs-tray').read_text(), 'client-2')
        self.assertEqual((slot / 'Contents/MacOS/hgs-tray').read_text(), 'qt', 'Qt stays the rollback copy')
        self.assertEqual(previous, aside / 'hgs-tray.app')
        with mock.patch.object(updater, 'stop_started', lambda installed: None):
            self.assertTrue(updater.roll_back(installed, previous))
        self.assertEqual((installed / 'Contents/MacOS/hgs-tray').read_text(), 'client-1',
                         'a failing new client brings back the client it replaced, not Qt')

    def test_the_switch_moves_qt_into_the_slot_and_unregisters_it(self):
        installed = self.root / 'Applications/hgs-tray.app'
        shutil.copytree(self.bundle('qt', 'qt'), installed)
        with mock.patch.object(updater, 'MAC', True), mock.patch.object(updater, 'run', lambda *a, **k: ''):
            previous = updater.replace(self.stage_of(self.bundle('client', 'tauri'), installed), installed)
        self.assertEqual(previous, updater.ROLLBACK / 'hgs-tray.app')
        self.assertEqual((previous / 'Contents/MacOS/hgs-tray').read_text(), 'qt')
        log = (self.root / 'lsregister.log').read_text().splitlines()
        self.assertEqual(log, [f'-u {previous}', f'-f {installed}'])


if __name__ == '__main__':
    unittest.main()
```

Run: `python3 -m unittest discover -s tests -p test_tray_update_switch.py -v`
Expected: FAIL with `AttributeError` (`gui_kind`, `bundle_info`, `draft_roots`, `keeps_slot`, `numbered_refusal`, `DEV_GUARD`, `check_replace`, `settings_changes`, `save_settings_snapshot`, the patched constants).

- [ ] **Step 2: Add the bundle, roots and slot rules**

In `tray/update-installed.py`:
- import `argparse` and `plistlib`;
- after `QUIT_SECONDS`/`START_SECONDS`, add the constants;
- replace `snapshot`, `changed_drafts`, `replace` and `roll_back` with the versions below;
- in `sweep`, replace the inline `lsregister -u` loop with `for bundle in copies: unregister(bundle)`;
- in `replace`'s old body, drop the inline `lsregister -f`.

```python
LSREGISTER = Path('/System/Library/Frameworks/CoreServices.framework/Frameworks/'
                  'LaunchServices.framework/Support/lsregister')
DEFAULTS = 'defaults'
# Qt's QSettings domain, which is also the NSUserDefaults domain of any bundle
# with this identifier (the Tauri switch bundle included).
PREFERENCES = 'com.hgdev.hgs-tray'
# Kept beside the Qt rollback copy at the switch (docs/macos-distribution.md).
SETTINGS_SNAPSHOT = ROLLBACK / f'{PREFERENCES}.plist'
# Keys AppKit and WebKit write into a bundle's defaults domain; every other key is Qt's.
COCOA_PREFIXES = ('NS', 'Apple', 'WebKit', 'WebView', 'com.apple.', '_')


def bundle_info(bundle):
    """A bundle's Info.plist; {} without one (a Linux binary, a missing bundle)."""
    try:
        return plistlib.loads((Path(bundle) / 'Contents/Info.plist').read_bytes())
    except (OSError, ValueError, plistlib.InvalidFileException):
        return {}


def gui_kind(info):
    """'tauri' for the Tauri client's switch bundle (Info.plist ZerusGUI), else 'qt'."""
    return 'tauri' if info.get('ZerusGUI') == 'tauri' else 'qt'


def keeps_slot(installed_kind, slot_kind, new_kind):
    """Whether the one rollback copy stays. Once it holds Qt and the client
    replaces itself, Qt stays the switch's rollback target; the replaced
    client waits only for this update's own restore."""
    return installed_kind == 'tauri' and slot_kind == 'qt' and new_kind == 'tauri'


def client_drafts_root(identifier):
    """The Tauri client's drafts root, `<app data>/drafts`: Tauri's
    app_data_dir formula with the running bundle's identifier."""
    if MAC:
        return HOME / 'Library/Application Support' / identifier / 'drafts'
    return Path(os.environ.get('XDG_DATA_HOME') or str(HOME / '.local/share')) / identifier / 'drafts'


def draft_roots(*infos):
    """Qt's drafts root, then the drafts root of each Tauri client among the bundles."""
    roots = [('qt', drafts_dir())]
    for info in infos:
        identifier = info.get('CFBundleIdentifier')
        if gui_kind(info) == 'tauri' and identifier:
            root = (f'client-{identifier}', client_drafts_root(identifier))
            if root not in roots:
                roots.append(root)
    return roots


def register(bundle):
    """Refresh LaunchServices' record of the installed bundle; never a global reset."""
    if MAC and LSREGISTER.exists():
        subprocess.run([str(LSREGISTER), '-f', str(bundle)], capture_output=True)


def unregister(bundle):
    """Forget a bundle copy that left the installed path, so a notification
    click or `open -b com.hgdev.hgs-tray` cannot launch it."""
    if MAC and LSREGISTER.exists() and Path(bundle).suffix == '.app':
        subprocess.run([str(LSREGISTER), '-u', str(bundle)], capture_output=True)


def snapshot(destination, roots):
    """Private copies of every drafts root, one folder per root."""
    destination.mkdir(mode=0o700)
    count = 0
    for label, source in roots:
        target = destination / label
        if not source.exists():
            target.mkdir(mode=0o700)
            continue
        if source.is_symlink() or any(p.is_symlink() for p in source.rglob('*')):
            raise UpdateError(f'draft storage {source} contains symbolic links; nothing was replaced')
        shutil.copytree(source, target)
        for p in target.rglob('*'):
            os.chmod(p, 0o700 if p.is_dir() else 0o600)
        os.chmod(target, 0o700)
        count += sum(1 for _ in target.rglob('draft.json'))
    return count


def changed_drafts(saved, roots):
    """Nonempty drafts saved when the old GUI quit that are missing or different now."""
    problems, preserved = [], 0
    for label, current in roots:
        prefix = '' if label == 'qt' else f'{label} '
        kept = saved / label
        for before in sorted(kept.rglob('draft.json')):
            relative = before.relative_to(kept).parent
            original = json.loads(before.read_text())
            if not (original.get('text') or original.get('attachments') or original.get('form_state')):
                continue
            after = current / relative / 'draft.json'
            if not after.exists():
                problems.append(f'{prefix}{relative}: missing')
                continue
            restored = json.loads(after.read_text())
            if any(original.get(key) != restored.get(key) for key in ('key', 'text', 'attachments', 'form_state')):
                problems.append(f'{prefix}{relative}: changed')
                continue
            for attachment in original.get('attachments') or []:
                blob = after.parent / (attachment['sha256'] + '.bin')
                if not blob.exists() or digest(blob) != attachment['sha256']:
                    problems.append(f'{prefix}{relative}: attachment changed')
            preserved += 1
    return preserved, problems


def replace(stage, installed, aside=None):
    """Move the stage into place. The installed GUI becomes the one rollback
    copy, or, with `aside`, waits there only for this update's own restore."""
    if aside is None:
        previous = ROLLBACK / installed.name
        ROLLBACK.mkdir(parents=True, exist_ok=True)
        if previous.exists() or previous.is_symlink():
            remove(previous)
    else:
        aside.mkdir(parents=True, exist_ok=True)
        previous = aside / installed.name
    if installed.exists():
        shutil.move(str(installed), str(previous))
        unregister(previous)
    try:
        os.rename(stage, installed)
    except OSError:
        if previous.exists() and not installed.exists():
            shutil.move(str(previous), str(installed))
        raise
    if MAC:
        run(['codesign', '--verify', '--deep', '--strict', installed])
        register(installed)
    return previous


def roll_back(installed, previous):
    if not previous.exists():
        return False
    stop_started(installed)
    remove(installed)
    shutil.move(str(previous), str(installed))
    register(installed)
    return True
```

- [ ] **Step 3: Add the Dev guard and the settings check**

```python
def installed_build_info(installed):
    """The installed GUI's --build-info, None when it cannot tell (an old build)."""
    try:
        return build_info(installed)
    except UpdateError:
        return None


# The rule as docs/reference.md states it ("Installation, configuration and
# diagnostics"); a test keeps the two equal.
DEV_GUARD = ('A numbered Dev installation is replaced only by a numbered Dev build or by a local preview build: '
             'set ZERUS_DEV_BUILD_NUMBER to the CI sequence of the main push the build comes from, '
             'or build an owner-requested branch preview with ZERUS_LOCAL_PREVIEW_LABEL.')


def numbered_refusal(installed_info, new_info):
    """Why a build may not replace the installed GUI: a numbered Dev install
    is replaced only by a numbered build or an explicit local preview (AGENTS.md)."""
    installed = int((installed_info or {}).get('devBuildNumber') or 0)
    if installed > 0 and not int(new_info.get('devBuildNumber') or 0) and not new_info.get('previewLabel'):
        return (f'the installed GUI is Dev build {installed} and this build has neither a Dev number '
                f'nor a local preview label. {DEV_GUARD}')
    return None


def check_replace(built, installed):
    """For an install that replaces the GUI on disk without the live update
    (HGS_TRAY_RESTART=0): the numbered Dev guard, and on macOS no change
    between Qt and the client, which only the live update may make."""
    built, installed = Path(built).resolve(), Path(installed).absolute()
    if not installed.exists():
        return 0
    refusal = numbered_refusal(installed_build_info(installed), build_info(built))
    if refusal:
        raise UpdateError(refusal + ' Nothing was replaced.')
    if MAC and gui_kind(bundle_info(built)) != gui_kind(bundle_info(installed)):
        raise UpdateError('switching between the Qt GUI and the Tauri client needs the live update; '
                          'run tray/install.sh without HGS_TRAY_RESTART=0. Nothing was replaced.')
    return 0


def export_settings():
    """Qt's settings domain as cfprefsd serves it (never the plist file), or None."""
    result = subprocess.run([DEFAULTS, 'export', PREFERENCES, '-'], capture_output=True)
    if result.returncode:
        return None
    try:
        return result.stdout, plistlib.loads(result.stdout)
    except (ValueError, plistlib.InvalidFileException):
        return None


def cocoa_key(key):
    return key.startswith(COCOA_PREFIXES)


def value_bytes(value):
    return plistlib.dumps({'value': value}, fmt=plistlib.FMT_XML, sort_keys=True)


def settings_changes(before, after):
    """Qt keys that changed or vanished, other non-Cocoa keys that appeared,
    and the Cocoa keys written, each sorted."""
    changed = sorted(k for k, v in before.items()
                     if not cocoa_key(k) and (k not in after or value_bytes(after[k]) != value_bytes(v)))
    added = sorted(k for k in after if k not in before and not cocoa_key(k))
    cocoa = sorted(k for k, v in after.items()
                   if cocoa_key(k) and (k not in before or value_bytes(before[k]) != value_bytes(v)))
    return changed, added, cocoa


def save_settings_snapshot():
    """At the switch, keep Qt's settings beside the Qt rollback copy (0600)."""
    exported = export_settings()
    if exported is None:
        raise UpdateError(f'cannot read the {PREFERENCES} settings to keep a copy; nothing was replaced')
    SETTINGS_SNAPSHOT.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=SETTINGS_SNAPSHOT.parent, prefix=f'.{SETTINGS_SNAPSHOT.name}.')
    with os.fdopen(fd, 'wb') as f:
        f.write(exported[0])
    os.replace(temporary, SETTINGS_SNAPSHOT)
    say(f'kept a copy of the Qt settings in {SETTINGS_SNAPSHOT}')


def settings_report(rollback=False):
    """Qt's keys against the copy kept at the switch: problems and notes."""
    if not SETTINGS_SNAPSHOT.exists():
        return [], []
    exported = export_settings()
    if exported is None:
        return [f'cannot read the {PREFERENCES} settings to compare them with {SETTINGS_SNAPSHOT}'], []
    changed, added, cocoa = settings_changes(plistlib.loads(SETTINGS_SNAPSHOT.read_bytes()), exported[1])
    problems = []
    if changed:
        problems.append(f'Qt settings changed or missing: {", ".join(changed)} (the copy is {SETTINGS_SNAPSHOT})')
        if rollback:
            problems.append(f'with Zerus quit, restore them with: defaults import {PREFERENCES} {SETTINGS_SNAPSHOT}')
    if added:
        problems.append(f"settings written that are neither Qt's nor Cocoa's: {', '.join(added)}")
    notes = [f'Cocoa settings written since the switch: {", ".join(cocoa)}'] if cocoa else []
    return problems, notes
```

In `docs/reference.md`, section "Installation, configuration and diagnostics", add this paragraph after the one that begins "The GUI installer updates only Zerus" (its second sentence is `DEV_GUARD` word for word; the test compares them without backticks and line breaks):

```markdown
The live update and `HGS_TRAY_RESTART=0` both refuse to put an unnumbered build
over a numbered Dev installation (AGENTS.md). A numbered Dev installation is
replaced only by a numbered Dev build or by a local preview build: set
`ZERUS_DEV_BUILD_NUMBER` to the CI sequence of the main push the build comes
from, or build an owner-requested branch preview with
`ZERUS_LOCAL_PREVIEW_LABEL`. The label is at most 32 lowercase letters and
digits in hyphen-separated words; it needs a clean Git checkout and no Dev
number, and the build shows as `DEV_VERSION-preview.LABEL.COMMIT`. The Qt GUI
reads it as a CMake cache variable (configure `tray/build` with
`-DZERUS_DEV_BUILD_NUMBER= -DZERUS_LOCAL_PREVIEW_LABEL=task-board` before
running `tray/install.sh`); the Tauri client reads the environment variable
when it builds. Clear the label before building main again. On macOS, a switch
between the Qt GUI and the Tauri client always takes the live update, never
`HGS_TRAY_RESTART=0`.
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `python3 -m unittest discover -s tests -p test_tray_update_switch.py -v`
Expected: 11 of the 12 tests pass. `test_the_gui_kind_comes_from_the_info_plist` needs Step 6's `--installed-gui`.

- [ ] **Step 5: Write the failing end-to-end tests on Linux**

In `tests/test_tray_update.py`:
- In `test_lost_drafts_are_reported_with_their_snapshot`, change `snapshot / 'kept/draft.json'` to `snapshot / 'qt/kept/draft.json'`.
- In `FAKE_GUI`, a variant named `dev-N` reports Dev build N: replace `'devBuildNumber': 0` with `'devBuildNumber': int(VARIANT[4:]) if VARIANT.startswith('dev-') else 0`.
- Add to `TrayUpdate`:

```python
    def rollback(self):
        return subprocess.run([sys.executable, str(UPDATER), '--rollback', str(self.installed)], env=self.env,
                              capture_output=True, text=True, timeout=60)

    def test_rollback_reinstalls_the_kept_copy(self):
        self.launch('first')
        self.draft('kept', 'unsent text')
        result = self.update(self.build('second'))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = self.rollback()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('reinstalling the rollback copy', result.stdout)
        self.assertEqual([variant for variant, _ in self.running()], ['first'])
        self.assertIn("'first'", self.installed.read_text())
        self.assertIn("'second'", (self.home / '.local/share/hgs/desktop-previous/hgs-tray').read_text())
        self.assertEqual(json.loads((self.drafts / 'kept/draft.json').read_text())['text'], 'unsent text')
        self.assertEqual((self.root / 'activations').read_text(), 'sessions\nsessions\n')

    def test_rollback_without_a_copy_changes_nothing(self):
        self.build('first', self.installed)
        result = self.rollback()
        self.assertEqual(result.returncode, 1)
        self.assertIn('there is no rollback copy', result.stderr)
        self.assertIn("'first'", self.installed.read_text())

    def test_an_unnumbered_build_never_replaces_a_numbered_dev_install(self):
        # AGENTS.md and docs/reference.md: the live update and HGS_TRAY_RESTART=0 refuse alike.
        self.build('dev-1413', self.installed)
        result = self.update(self.build('second'))
        self.assertEqual(result.returncode, 1)
        self.assertIn('Dev build 1413', result.stderr)
        self.assertIn('ZERUS_LOCAL_PREVIEW_LABEL', result.stderr)
        install = self.installer(self.build('third'))
        result = subprocess.run(['bash', str(install)], env=dict(self.env, HGS_TRAY_RESTART='0'),
                                capture_output=True, text=True, timeout=90)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Dev build 1413', result.stderr)
        self.assertIn("'dev-1413'", self.installed.read_text())
        self.assertFalse((self.root / 'systemctl').exists(), 'nothing was started or enabled')
        result = self.update(self.build('dev-1414'))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("'dev-1414'", self.installed.read_text())
```

Run: `python3 -m unittest discover -s tests -p test_tray_update.py -v` (the module imports `tray_update_fixture` from `tests/`, so it runs through discovery)
Expected: the three new tests FAIL (`--rollback` is not an option yet, and `tray/install.sh` does not ask `--check`); `test_lost_drafts_are_reported_with_their_snapshot` FAILS until Step 6 wires the roots into `update`.

- [ ] **Step 6: Wire the rules into `update`, add the rollback command and the options**

Update the module docstring's usage paragraph (the second paragraph, which `main` prints) to:

```
    update-installed.py <built hgs-tray | hgs-tray.app> <installed path>
    update-installed.py --rollback <installed path>
    update-installed.py --installed-gui <installed path>
    update-installed.py --check <built hgs-tray | hgs-tray.app> <installed path>
```

Add these sentences to the numbered list of the docstring:
- Step 3 keeps Qt as the one rollback copy once the Tauri client replaced it.
- `--rollback` reinstalls that copy through the same steps.
- Drafts of Qt and of the Tauri client (`<app data>/drafts`) are both snapshotted and verified.
- At a Qt-to-client switch on macOS, Qt's settings are exported beside the rollback copy and checked afterwards.
- A numbered Dev installation is replaced only by a numbered build or a local preview build (`DEV_GUARD`); `--check` applies that guard for `HGS_TRAY_RESTART=0`, which replaces the GUI on disk only, and refuses a switch between Qt and the client there.

Replace `update` and `main`, and add `roll_back_installed`:

```python
def update(built, installed, rollback=False):
    built, installed = built.resolve(), installed.absolute()
    info = build_info(built)
    slot = ROLLBACK / installed.name
    new_info = bundle_info(built) if MAC else {}
    old_info = bundle_info(installed) if MAC else {}
    old_kind = gui_kind(old_info) if installed.exists() else None
    new_kind = gui_kind(new_info)
    # A rollback consumes the slot it installs from.
    slot_kind = gui_kind(bundle_info(slot)) if MAC and slot.exists() and not rollback else None
    if not rollback and installed.exists():
        refusal = numbered_refusal(installed_build_info(installed), info)
        if refusal:
            raise UpdateError(refusal + ' Nothing was replaced.')
    switching = MAC and old_kind == 'qt' and new_kind == 'tauri'
    keep = keeps_slot(old_kind, slot_kind, new_kind)
    roots = draft_roots(old_info, new_info)
    if MAC:
        run(['codesign', '--verify', '--deep', '--strict', built])
    installed.parent.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='.hgs-tray-update-', dir=installed.parent))
    aside = folder / 'previous' if keep else None
    saved = None
    kept = (f'the Qt GUI stays the rollback copy in {ROLLBACK}' if keep else f'previous GUI kept in {ROLLBACK}')
    try:
        stage = folder / installed.name
        if MAC:
            shutil.copytree(built, stage, symlinks=True)
            run(['codesign', '--verify', '--deep', '--strict', stage])
        else:
            shutil.copy2(built, stage)
            os.chmod(stage, 0o755)
        expected = digest(executable(stage))
        pids = gui_pids(installed)
        if len(pids) > 1:
            raise UpdateError(f'several copies of Zerus are running ({pids}); quit them, then run the installer again')
        if not pids:
            # Before Qt can run again: the client must not have changed its settings.
            early = settings_report(rollback=True) if MAC and rollback else ([], [])
            if switching:
                save_settings_snapshot()
            replace(stage, installed, aside)
            say(f'replaced the stopped GUI with {info.get("versionName", info["version"])}; {kept}')
            sweep(installed.parent, folder)
            if early[0]:
                raise UpdateError('the previous GUI is installed, but ' + '; '.join(early[0]))
            return 0
        pid = pids[0]
        path, before = socket_path(pid), native()
        say(f'quitting Zerus (pid {pid}) so it saves its drafts')
        quit_gui(pid, installed)
        early = settings_report(rollback=True) if MAC and rollback else ([], [])
        try:
            SNAPSHOTS.mkdir(mode=0o700, parents=True, exist_ok=True)
            saved = Path(tempfile.mkdtemp(prefix='drafts-', dir=SNAPSHOTS))
            os.rmdir(saved)
            snapshot(saved, roots)
            if switching:
                save_settings_snapshot()
            previous = replace(stage, installed, aside)
        except (OSError, subprocess.CalledProcessError, UpdateError) as error:
            # The previous GUI is still installed: bring it back before reporting.
            start(installed)
            raise UpdateError(f'{error}; the previous GUI was started again') from None
        installed_at = now()
        start(installed)
        new = wait_started(installed, path, pid)
        if new is None:
            restored = roll_back(installed, previous)
            if restored:
                start(installed)
                wait_started(installed, path, pid)
            raise UpdateError('the new GUI did not start' + ('; the previous GUI was restored and started'
                                                             if restored else ''))
        problems = list(early[0])
        if digest(executable(installed)) != expected or not running_image_matches(new, installed):
            problems.append('the running image is not the new build')
        preserved, draft_problems = changed_drafts(saved, roots)
        problems += draft_problems
        if MAC and not rollback and 'tauri' in (old_kind, new_kind):
            late, notes = settings_report()
            problems += late
            for note in notes:
                say(note)
        after = native()
        replaced = [p for p, value in before.items() if p in after and after[p] != value]
        if replaced:
            problems.append(f'native process identities changed: {replaced}')
        if problems:
            raise UpdateError('the new GUI runs, but ' + '; '.join(problems))
        retained = sum(after.get(p) == value for p, value in before.items())
        provenance(installed, info, installed_at, preserved, retained)
        shutil.rmtree(saved)
        saved = None
        sweep(installed.parent, folder)
        say(f'Zerus {info.get("versionName", info["version"])} runs as pid {new}; {preserved} nonempty drafts kept, '
            f'{retained} native processes untouched; {kept}')
        return 0
    except UpdateError:
        if saved is not None and saved.exists():
            say(f'drafts saved when Zerus quit remain in {saved}')
        raise
    finally:
        if folder.exists():
            shutil.rmtree(folder)


def roll_back_installed(installed):
    """Reinstall the one rollback copy (after the switch: the last Qt GUI)
    through the same quit, snapshot, start and verification as an update."""
    installed = installed.absolute()
    previous = ROLLBACK / installed.name
    if not previous.exists():
        raise UpdateError(f'there is no rollback copy in {ROLLBACK}; nothing was replaced')
    say(f'reinstalling the rollback copy {previous}')
    return update(previous, installed, rollback=True)


def main():
    usage = __doc__.split('\n\n')[1]
    parser = argparse.ArgumentParser(usage=usage, add_help=False)
    parser.add_argument('--rollback', metavar='INSTALLED', type=Path)
    parser.add_argument('--installed-gui', metavar='INSTALLED', type=Path)
    parser.add_argument('--check', nargs=2, metavar=('BUILT', 'INSTALLED'), type=Path)
    parser.add_argument('paths', nargs='*', type=Path)
    args = parser.parse_args()
    chosen = [option for option in (args.rollback, args.installed_gui, args.check) if option]
    try:
        if args.installed_gui and len(chosen) == 1 and not args.paths:
            print(gui_kind(bundle_info(args.installed_gui)) if args.installed_gui.exists() else 'none')
            return 0
        if args.rollback and len(chosen) == 1 and not args.paths:
            return roll_back_installed(args.rollback)
        if args.check and len(chosen) == 1 and not args.paths:
            return check_replace(*args.check)
        if len(args.paths) == 2 and not chosen:
            return update(*args.paths)
    except UpdateError as error:
        print(f'hgs-tray update failed: {error}', file=sys.stderr)
        return 1
    print(usage, file=sys.stderr)
    return 2
```

In `tray/install.sh`, both `HGS_TRAY_RESTART=0` branches ask the updater before they replace anything:
- macOS, step 2, in the `else` branch, before `rm -rf "${APP_DEST}"`:
  ```bash
        # The numbered Dev guard, and no switch between Qt and the client without the live update.
        python3 "${SCRIPT_DIR}/update-installed.py" --check "${BUILD_APP}" "${APP_DEST}"
  ```
- Linux, step 2, in the `else` branch, before `install -Dm755 "${SCRIPT_DIR}/build/hgs-tray" "${BIN_DEST}.new"`:
  ```bash
        # The numbered Dev guard (docs/reference.md).
        python3 "${SCRIPT_DIR}/update-installed.py" --check "${SCRIPT_DIR}/build/hgs-tray" "${BIN_DEST}"
  ```
`set -e` stops the installer on a refusal, before the LaunchAgent or the unit changes.

- [ ] **Step 7: Run all updater tests on Linux**

Run: `python3 -m unittest discover -s tests -p 'test_tray_update*.py' -v`
Expected: PASS, including `test_rollback_reinstalls_the_kept_copy`, `test_rollback_without_a_copy_changes_nothing`, `test_an_unnumbered_build_never_replaces_a_numbered_dev_install` and the Linux updates that were green before. Also run `python3 -m unittest discover -s tests -p 'test_mac_install.py' -v` and the same for `test_macos_local_signing.py`: they copy `tray/install.sh` and must stay green.

- [ ] **Step 8: Write the macOS updater test with codesign**

Create `tests/test_macos_switch_updater.py`:

```python
"""On macOS: switch, client update and rollback with ad-hoc signed fake
bundles (com.example.zerus-switch-test) in a fake HOME. lsregister is replaced
by /usr/bin/true and `defaults` by a fixture, so neither LaunchServices nor a
real settings domain changes. No GUI runs: every update takes the stopped path."""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('update_installed_mac', ROOT / 'tray/update-installed.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)


@unittest.skipUnless(sys.platform == 'darwin', 'codesign and bundles are macOS-only')
class MacSwitch(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='zms-', dir='/tmp')).resolve()
        self.addCleanup(shutil.rmtree, self.root, True)
        self.home = self.root / 'home'
        self.home.mkdir()
        (self.root / 'domain.plist').write_bytes(plistlib.dumps({'attention.observed': b'{}'}))
        defaults = self.root / 'defaults'
        defaults.write_text(f'#!/bin/sh\ncat {self.root}/domain.plist\n')
        defaults.chmod(0o755)
        rollback = self.home / '.local/share/hgs/desktop-previous'
        for name, value in {'HOME': self.home, 'ROLLBACK': rollback, 'SETTINGS_SNAPSHOT': rollback / 'com.hgdev.hgs-tray.plist',
                            'SNAPSHOTS': self.home / '.local/state/hgs/desktop-update',
                            'PROVENANCE': self.home / '.local/share/hgs/build-provenance.json',
                            'CLI': self.home / '.local/bin/hgs', 'HELPER': self.home / '.local/bin/hgs_state.py',
                            'PLIST': self.home / 'Library/LaunchAgents/com.hgdev.hgs-tray.plist',
                            'LSREGISTER': Path('/usr/bin/true'), 'DEFAULTS': str(defaults)}.items():
            patcher = mock.patch.object(updater, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.installed = self.root / 'Applications/hgs-tray.app'

    def bundle(self, name, kind, number=1413):
        app = self.root / 'bundles' / name / 'Zerus.app'
        (app / 'Contents/MacOS').mkdir(parents=True)
        info = {'CFBundleIdentifier': 'com.example.zerus-switch-test', 'CFBundleExecutable': 'hgs-tray',
                'CFBundlePackageType': 'APPL'}
        if kind == 'tauri':
            info['ZerusGUI'] = 'tauri'
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        build_info = json.dumps({'version': '0.47.0', 'versionName': name, 'devBuildNumber': number})
        source = self.root / f'{name}.c'
        source.write_text('#include <stdio.h>\nint main(void) { puts(%s); return 0; }\n' % json.dumps(build_info))
        subprocess.run(['cc', '-o', str(app / 'Contents/MacOS/hgs-tray'), str(source)], check=True)
        subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True, capture_output=True)
        return app

    def variant(self, path):
        return json.loads(subprocess.check_output([str(updater.executable(path)), '--build-info']))['versionName']

    def test_switch_client_update_and_rollback(self):
        self.assertEqual(updater.update(self.bundle('qt', 'qt'), self.installed), 0)
        self.assertEqual(updater.update(self.bundle('client-1', 'tauri'), self.installed), 0)
        slot = updater.ROLLBACK / 'hgs-tray.app'
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('client-1', 'qt'))
        self.assertTrue(updater.SETTINGS_SNAPSHOT.exists(), 'the switch keeps Qt settings beside the Qt copy')
        self.assertEqual(updater.update(self.bundle('client-2', 'tauri'), self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('client-2', 'qt'))
        with self.assertRaisesRegex(updater.UpdateError, 'Dev build 1413'):
            updater.update(self.bundle('unnumbered', 'tauri', number=0), self.installed)
        self.assertEqual(updater.roll_back_installed(self.installed), 0)
        self.assertEqual((self.variant(self.installed), self.variant(slot)), ('qt', 'client-2'))
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(self.installed)], check=True)


if __name__ == '__main__':
    unittest.main()
```

- [ ] **Step 9: Run it on the Mac, display-free**

Run **mac-lock** and **mac-sync**, then:
```bash
ssh -o BatchMode=yes mac 'cd ~/zerus-dev/webui && python3 -m unittest discover -s tests -p "test_macos_switch_updater.py" -v && python3 -m unittest discover -s tests -p "test_tray_update_switch.py" -v'
```
then **mac-done**. Expected: PASS. Nothing outside the temporary folders changes; LaunchServices is untouched because `lsregister` is `/usr/bin/true`, and `defaults` is a fixture.

- [ ] **Step 10: Run the task's checks and commit**

Run `python3 -m unittest discover -s tests -p 'test_macos_*.py' -v`, the updater tests and `python3 scripts/check-source-privacy.py`.

```bash
git add tray/update-installed.py tray/install.sh docs/reference.md tests/test_tray_update_switch.py \
  tests/test_tray_update.py tests/test_macos_switch_updater.py
git commit -m "feat(tray): updater keeps client drafts, the Qt rollback copy and Qt settings, guards Dev numbers and can roll back"
```

---

### Task 8: Main-thread watchdog, as Qt's (zerus-5wmh.12.3)

Spec row 297 and the coordinator's ruling of 2026-10-28: a port of `tray/src/MainThreadWatchdog.{h,cpp}`, `tray/src/MacSystemPower.{h,cpp}` and their wiring in `tray/src/main.cpp:163-166` and `:385-423`. Qt added the watchdog after its menu-bar item hung inside AppKit on macOS 26 for six days while the process still "ran" (`QuietTrayIcon.h`). After the switch the client is the only GUI, so it needs the same safety net.
- **Thresholds.** A main-thread heartbeat every 7.5 s, a check every 15 s on a thread named `hgs-watchdog`, a 30 s threshold (`kMainThreadHangMs`) and a 10-minute wake grace, all Qt's.
- **Clock.** Uptime that stops while the machine sleeps: `CLOCK_UPTIME_RAW` on macOS, `CLOCK_MONOTONIC` on Linux (`MainThreadWatchdog::nowMs`).
- **Sleep and dark wake.** IOKit's root power domain notifies a dispatch queue of its own, never the main thread. The check holds from the sleep notice until the main thread beats after a wake, for the grace at most. Sleep is never vetoed.
- **Exit path.** One line on stderr, then `abort()`, at most once. launchd's `KeepAlive Crashed` restarts the client, and the crash report shows where the main thread stood. A plain `exit` would not be restarted.
- **Not a hang.** An open menu or a modal panel: the macOS heartbeat is an `NSTimer` in the common run-loop modes, as Qt's `QTimer` is. A Tauri `run_on_main_thread` beat would not do: it waits for tao's event callback, which a modal loop inside a callback blocks.
- **Who runs it.** The switch bundle, which launchd supervises, and any other build with `ZERUS_CLIENT_WATCHDOG=1` (Decision 4). The decision logic (`Pulse`, `Watchdog`) is pure and tested on Linux; the platform glue is thin.

**Files:**
- Create: `client/src-tauri/src/watchdog.rs`
- Create: `client/src-tauri/src/platform/macos_power.rs`; modify `client/src-tauri/src/platform/mod.rs`
- Modify: `client/src-tauri/src/platform/macos.rs` (`heartbeat`), `client/src-tauri/src/platform/linux.rs` (`heartbeat`)
- Modify: `client/src-tauri/src/identity.rs` (`watchdog`), `client/src-tauri/src/lib.rs` (module, `start_watchdog`, setup), `client/src-tauri/src/lifecycle.rs` (stop on exit)
- Modify: `client/src-tauri/Cargo.toml` (objc2-foundation features `NSDate`, `NSRunLoop`, `NSTimer`)
- Test: unit tests in `watchdog.rs` and `identity.rs`

**Interfaces:**
- Consumes: `identity::{Identity, flag}` (Task 2); `launch`'s setup closure (Tasks 3, 4, 6).
- Produces:
  - `watchdog::{HANG_MS, WAKE_GRACE_MS, THREAD, Limits { hang_ms, wake_grace_ms }, Limits::QT, Limits::beat_every(), Limits::check_every(), Pulse::{new, beat, will_sleep, powered_on, silent_ms, hung}, Watchdog::{start(Limits, fn() -> i64, impl FnOnce(i64) + Send), pulse() -> Arc<Pulse>, stop()}, now_ms() -> i64, abort_on_hang(i64) -> !}` (Linux and macOS).
  - `lifecycle::on_event` stops the watchdog first on `RunEvent::Exit`.
  - `identity::Identity::watchdog(requested: bool) -> bool`.
  - `platform::macos::heartbeat(Duration, impl Fn() + 'static)`, `platform::linux::heartbeat(Duration, impl Fn() + 'static)`, `platform::macos_power::observe(Arc<Pulse>)`.
  - `lib::start_watchdog() -> std::io::Result<watchdog::Watchdog>`; the running `Watchdog` lives in the app state.

- [ ] **Step 1: Write the failing tests**

Create `client/src-tauri/src/watchdog.rs` with only this test module, and add `#[cfg(any(target_os = "linux", target_os = "macos"))] mod watchdog;` to `lib.rs`. Each test names the Qt test in `tray/tests/test_watchdog.cpp` it ports; Qt's real-time scenarios become fake-clock checks of the same rule, so they cannot flake on a loaded machine, and three short real-time tests cover the thread:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits { hang_ms: 300, wake_grace_ms: 600 };

    #[test]
    fn qt_values_and_periods() {
        assert_eq!(Limits::QT, Limits { hang_ms: 30_000, wake_grace_ms: 600_000 });
        assert_eq!(Limits::QT.beat_every(), Duration::from_millis(7_500));
        assert_eq!(Limits::QT.check_every(), Duration::from_millis(15_000));
    }

    #[test]
    fn a_silent_main_thread_is_hung_past_the_threshold() {
        // firesOnceWhenTheMainThreadStopsPumpingEvents
        let pulse = Pulse::new(1_000);
        assert!(!pulse.hung(1_300, LIMITS), "exactly the threshold is not a hang");
        assert!(pulse.hung(1_301, LIMITS));
    }

    #[test]
    fn beats_keep_it_quiet() {
        // staysQuietWhileEventsFlow
        let pulse = Pulse::new(0);
        for now in (75..=3_000).step_by(75) {
            pulse.beat(now);
            assert!(!pulse.hung(now + 75, LIMITS));
        }
    }

    #[test]
    fn going_to_sleep_holds_until_power_on() {
        // holdsWhileTheSystemGoesToSleep: beats in between do not end the hold.
        let pulse = Pulse::new(0);
        pulse.will_sleep(0);
        pulse.beat(200);
        assert!(!pulse.hung(600, LIMITS));
        assert!(pulse.hung(601, LIMITS), "the hold lasts the wake grace at most");
    }

    #[test]
    fn a_wake_holds_until_the_main_thread_beats() {
        // holdsAfterAWakeUntilTheMainThreadRuns
        let pulse = Pulse::new(0);
        pulse.will_sleep(0);
        pulse.powered_on(10);
        assert!(!pulse.hung(500, LIMITS), "silent after the wake, within the grace");
        pulse.beat(500);
        assert!(!pulse.hung(800, LIMITS));
        assert!(pulse.hung(801, LIMITS), "watched as usual once it ran");
    }

    #[test]
    fn a_main_thread_silent_past_the_wake_grace_is_hung() {
        // firesWhenTheMainThreadStaysSilentPastTheWakeGrace
        let pulse = Pulse::new(0);
        pulse.powered_on(0);
        assert!(!pulse.hung(600, LIMITS));
        assert!(pulse.hung(601, LIMITS));
    }

    #[test]
    fn the_thread_fires_once_on_its_own_name() {
        let (sender, fired) = std::sync::mpsc::channel();
        let dog = Watchdog::start(Limits { hang_ms: 100, wake_grace_ms: 100 }, now_ms, move |silent| {
            let _ = sender.send((std::thread::current().name().map(str::to_owned), silent));
        })
        .unwrap();
        let (name, silent) = fired.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(name.as_deref(), Some(THREAD));
        assert!(silent > 100, "{silent}");
        assert!(fired.recv_timeout(Duration::from_millis(400)).is_err(), "at most once");
        drop(dog);
    }

    #[test]
    fn a_quit_is_not_a_hang() {
        let (sender, fired) = std::sync::mpsc::channel::<i64>();
        let dog = Watchdog::start(Limits { hang_ms: 100, wake_grace_ms: 0 }, now_ms, move |silent| {
            let _ = sender.send(silent);
        })
        .unwrap();
        dog.stop();
        assert!(fired.recv_timeout(Duration::from_millis(500)).is_err(), "stopped before the main thread went quiet");
    }

    #[test]
    fn a_dropped_watchdog_stops_at_once() {
        let (sender, fired) = std::sync::mpsc::channel::<i64>();
        let started = std::time::Instant::now();
        drop(Watchdog::start(Limits { hang_ms: 10_000, wake_grace_ms: 0 }, now_ms, move |silent| {
            let _ = sender.send(silent);
        })
        .unwrap());
        assert!(started.elapsed() < Duration::from_secs(2), "not at the next check, 5 s later");
        assert!(fired.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn the_clock_moves_forward() {
        let first = now_ms();
        std::thread::sleep(Duration::from_millis(20));
        assert!(now_ms() >= first + 15);
    }
}
```

Add to the tests of `client/src-tauri/src/identity.rs`:

```rust
    #[test]
    fn only_the_switch_bundle_runs_the_watchdog_unless_asked() {
        assert!(Identity::new("com.hgdev.hgs-tray").watchdog(false), "launchd restarts it");
        assert!(!Identity::new("dev.zerus.next").watchdog(false), "a dev run is never aborted");
        assert!(Identity::new("dev.zerus.next").watchdog(true));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- watchdog:: only_the_switch_bundle_runs`
Expected: FAIL to compile: `Limits`, `Pulse`, `Watchdog`, `now_ms`, `THREAD` and `Identity::watchdog` are missing.

- [ ] **Step 3: Write `watchdog.rs` above its tests**

```rust
//! The main-thread watchdog, as the Qt GUI has it
//! (`tray/src/MainThreadWatchdog.{h,cpp}`, `tray/src/main.cpp:163-166` and
//! `:385-423`). Qt added it after its menu-bar item hung inside AppKit for six
//! days on macOS 26 while the process kept "running" (`tray/src/QuietTrayIcon.h`).
//!
//! A timer on the main thread records a beat every quarter of [`HANG_MS`]; a
//! thread named `hgs-watchdog` checks every half. Once the main thread has not
//! beaten for longer than [`HANG_MS`], it prints why and aborts, at most once:
//! launchd (`KeepAlive Crashed`, `tray/packaging/com.hgdev.hgs-tray.plist`) or
//! systemd (`Restart=on-failure`) starts the client again, and the crash report
//! shows where the main thread stood. A plain exit would not be restarted.
//!
//! The clock stops while the machine sleeps ([`now_ms`]). Around sleep macOS
//! leaves the main thread unscheduled for longer than the threshold while the
//! rest of the process runs: during the transition into sleep and in a dark
//! wake. [`Pulse::will_sleep`] and [`Pulse::powered_on`] (IOKit,
//! `platform/macos_power.rs`) hold the check from the sleep notice until the
//! main thread has beaten after a wake, for [`WAKE_GRACE_MS`] of uptime at
//! most, so a main thread that never runs again after a wake still counts.
//!
//! Not a hang: an open menu or a modal panel, since the macOS timer runs in
//! the common run-loop modes as Qt's does. A main thread starved of CPU trips
//! it too, whatever its stack shows.
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// `kMainThreadHangMs`: half a minute, far above any honest pause of the
/// event loop and far below the week a hung menu-bar item once stayed.
pub const HANG_MS: i64 = 30_000;
/// `MainThreadWatchdog`'s default `wakeGraceMs`: ten minutes of uptime.
pub const WAKE_GRACE_MS: i64 = 10 * 60 * 1000;
/// The checking thread's name, which crash reports show.
pub const THREAD: &str = "hgs-watchdog";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub hang_ms: i64,
    pub wake_grace_ms: i64,
}

impl Limits {
    /// Qt's values.
    pub const QT: Limits = Limits { hang_ms: HANG_MS, wake_grace_ms: WAKE_GRACE_MS };

    /// How often the main thread beats: a quarter of the threshold.
    pub fn beat_every(self) -> Duration {
        Duration::from_millis((self.hang_ms / 4).max(1).unsigned_abs())
    }

    /// How often the watchdog checks: half of the threshold.
    pub fn check_every(self) -> Duration {
        Duration::from_millis((self.hang_ms / 2).max(1).unsigned_abs())
    }
}

/// What the watchdog knows, written from any thread.
#[derive(Debug)]
pub struct Pulse {
    last_beat: AtomicI64,
    power_event: AtomicI64,
    sleeping: AtomicBool,
    awaiting_beat: AtomicBool,
}

impl Pulse {
    pub fn new(now: i64) -> Self {
        Self {
            last_beat: AtomicI64::new(now),
            power_event: AtomicI64::new(0),
            sleeping: AtomicBool::new(false),
            awaiting_beat: AtomicBool::new(false),
        }
    }

    /// The main thread ran: the heartbeat timer fired.
    pub fn beat(&self, now: i64) {
        self.last_beat.store(now, Ordering::SeqCst);
        self.awaiting_beat.store(false, Ordering::SeqCst);
    }

    /// The system is about to sleep: hold the check until it powers on.
    /// Only macOS reports it (`platform/macos_power.rs`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn will_sleep(&self, now: i64) {
        self.power_event.store(now, Ordering::SeqCst);
        self.sleeping.store(true, Ordering::SeqCst);
    }

    /// The system powered on, also for a dark wake: hold the check until the
    /// main thread beats.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn powered_on(&self, now: i64) {
        self.power_event.store(now, Ordering::SeqCst);
        // Before `sleeping` clears, so that the check never sees neither hold.
        self.awaiting_beat.store(true, Ordering::SeqCst);
        self.sleeping.store(false, Ordering::SeqCst);
    }

    /// How long the main thread has been silent.
    pub fn silent_ms(&self, now: i64) -> i64 {
        now - self.last_beat.load(Ordering::SeqCst)
    }

    /// The check (`MainThreadWatchdog.cpp:62-71`): silent past the threshold
    /// and not held by a sleep or a wake within the grace.
    pub fn hung(&self, now: i64, limits: Limits) -> bool {
        if self.silent_ms(now) <= limits.hang_ms {
            return false;
        }
        let held = self.sleeping.load(Ordering::SeqCst) || self.awaiting_beat.load(Ordering::SeqCst);
        !(held && now - self.power_event.load(Ordering::SeqCst) <= limits.wake_grace_ms)
    }
}

/// The checking thread. Stopping or dropping it ends the thread at once, not
/// at its next check.
pub struct Watchdog {
    pulse: Arc<Pulse>,
    stopping: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Starts checking; `on_hang` runs at most once, on the watchdog thread,
    /// with how long the main thread has been silent.
    pub fn start(limits: Limits, clock: fn() -> i64, on_hang: impl FnOnce(i64) + Send + 'static) -> io::Result<Self> {
        let pulse = Arc::new(Pulse::new(clock()));
        let stopping = Arc::new((Mutex::new(false), Condvar::new()));
        let (watched, signal) = (Arc::clone(&pulse), Arc::clone(&stopping));
        let thread = std::thread::Builder::new().name(THREAD.into()).spawn(move || {
            let (lock, wake) = &*signal;
            let mut stopped = lock.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                // Returns early only when stopped; a timeout is the next check.
                stopped = wake
                    .wait_timeout_while(stopped, limits.check_every(), |stopped| !*stopped)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                if *stopped {
                    return;
                }
                let now = clock();
                if watched.hung(now, limits) {
                    drop(stopped);
                    on_hang(watched.silent_ms(now));
                    return;
                }
            }
        })?;
        Ok(Self { pulse, stopping, thread: Some(thread) })
    }

    /// What the heartbeat and the power notices write to.
    pub fn pulse(&self) -> Arc<Pulse> {
        Arc::clone(&self.pulse)
    }

    /// Stops checking. Quitting is not a hang: Qt destroys its watchdog before
    /// the tray is torn down (`main.cpp:385-389`), and the client stops it
    /// first thing on `RunEvent::Exit`, so a slow flush never ends in an abort
    /// that launchd would answer with a restart.
    pub fn stop(&self) {
        *self.stopping.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.stopping.1.notify_all();
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Milliseconds that stop while the machine sleeps (`MainThreadWatchdog::nowMs`):
/// uptime (`CLOCK_UPTIME_RAW`) on macOS, `CLOCK_MONOTONIC` on Linux.
pub fn now_ms() -> i64 {
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_UPTIME_RAW;
    #[cfg(not(target_os = "macos"))]
    let clock = libc::CLOCK_MONOTONIC;
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes only the struct it is given.
    unsafe { libc::clock_gettime(clock, &mut time) };
    // Both fields are 64-bit on the client's targets.
    time.tv_sec * 1000 + time.tv_nsec / 1_000_000
}

/// Qt's exit path (`main.cpp:410-417`): say why, then abort. The line is
/// written with one `write(2)`: the main thread may hang holding Rust's
/// stderr lock.
pub fn abort_on_hang(silent_ms: i64) -> ! {
    let line = format!(
        "zerus-client: main thread unresponsive for {} s (AppKit deadlock? see tray/src/QuietTrayIcon.h); \
         aborting so the service manager restarts it\n",
        silent_ms / 1000
    );
    // SAFETY: writes the bytes of a live string to stderr.
    unsafe { libc::write(libc::STDERR_FILENO, line.as_ptr().cast(), line.len()) };
    std::process::abort()
}
```

In `impl Identity` of `client/src-tauri/src/identity.rs`, after `notifications`:

```rust
    /// Whether the main-thread watchdog runs: the switch bundle, which launchd
    /// restarts after the watchdog's abort, or another build on request
    /// (`ZERUS_CLIENT_WATCHDOG=1`).
    pub fn watchdog(&self, requested: bool) -> bool {
        self.is_switch() || requested
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd client && nice env CARGO_BUILD_JOBS=4 cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- watchdog:: only_the_switch_bundle_runs`
Expected: PASS (10 watchdog tests and the identity test).

- [ ] **Step 5: Write the heartbeat and the sleep notices**

Add to `client/src-tauri/src/platform/macos.rs`:

```rust
/// Runs `beat` on the main thread every `every`, in the common run-loop modes,
/// so an open menu or a modal panel does not stop it, as Qt's `QTimer` does
/// not stop (the watchdog's heartbeat). Call on the main thread; the run loop
/// keeps the timer for the life of the app.
pub fn heartbeat(every: Duration, beat: impl Fn() + 'static) {
    let block = block2::RcBlock::new(move |_timer: std::ptr::NonNull<objc2_foundation::NSTimer>| beat());
    // SAFETY: a repeating timer whose block owns what it captures.
    let timer = unsafe { objc2_foundation::NSTimer::timerWithTimeInterval_repeats_block(every.as_secs_f64(), true, &block) };
    // SAFETY: Foundation's constant mode name; the main run loop, from the main thread.
    unsafe { objc2_foundation::NSRunLoop::mainRunLoop().addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes) };
}
```

Add to `client/src-tauri/src/platform/linux.rs`:

```rust
/// Runs `beat` on the main thread every `every` (the watchdog's heartbeat): a
/// GLib timeout on the default main context, which nested dialog loops keep
/// dispatching. Call on the main thread.
pub fn heartbeat(every: std::time::Duration, beat: impl Fn() + 'static) {
    gtk::glib::timeout_add_local(every, move || {
        beat();
        gtk::glib::ControlFlow::Continue
    });
}
```

Create `client/src-tauri/src/platform/macos_power.rs` and add `#[cfg(target_os = "macos")] pub mod macos_power;` to `platform/mod.rs`:

```rust
//! System sleep and wake for the main-thread watchdog, as Qt's
//! `tray/src/MacSystemPower.{h,cpp}`. IOKit's root power domain notifies a
//! serial dispatch queue of its own, never the main thread, which macOS may
//! not schedule while it goes to sleep or in a dark wake. Sleep is never
//! vetoed: queries and sleep notices are acknowledged at once. IOKit has no
//! binding among the client's crates, so the few calls are declared here.
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::watchdog::{now_ms, Pulse};

/// `io_object_t`, `io_connect_t` and `io_service_t`: Mach port names.
type IoObject = u32;
type NotificationPort = *mut c_void;
type InterestCallback = extern "C" fn(refcon: *mut c_void, service: IoObject, message: u32, argument: *mut c_void);

// `IOKit/IOMessage.h`: iokit_common_msg(n) is 0xe0000000 | n.
const CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
const SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
const SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;
const SYSTEM_WILL_POWER_ON: u32 = 0xe000_0320;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut NotificationPort,
        callback: InterestCallback,
        notifier: *mut IoObject,
    ) -> IoObject;
    fn IOAllowPowerChange(kernel_port: IoObject, notification_id: isize) -> i32;
    fn IONotificationPortSetDispatchQueue(port: NotificationPort, queue: *mut c_void);
}

extern "C" {
    // libdispatch, part of libSystem; a null attribute makes a serial queue.
    fn dispatch_queue_create(label: *const c_char, attributes: *mut c_void) -> *mut c_void;
}

struct Context {
    pulse: Arc<Pulse>,
    /// The root power domain connection, set before the first notice arrives.
    root: AtomicU32,
}

extern "C" fn received(refcon: *mut c_void, _service: IoObject, message: u32, argument: *mut c_void) {
    // SAFETY: `observe` passes a Context it never frees.
    let context = unsafe { &*refcon.cast::<Context>() };
    match message {
        // An unanswered query delays idle sleep by 30 s.
        CAN_SYSTEM_SLEEP => allow(context, argument),
        SYSTEM_WILL_SLEEP => {
            context.pulse.will_sleep(now_ms());
            allow(context, argument);
        }
        SYSTEM_WILL_POWER_ON | SYSTEM_HAS_POWERED_ON => context.pulse.powered_on(now_ms()),
        _ => {}
    }
}

fn allow(context: &Context, argument: *mut c_void) {
    // SAFETY: the connection IORegisterForSystemPower returned; the argument is the notice's ID.
    unsafe { IOAllowPowerChange(context.root.load(Ordering::Acquire), argument as isize) };
}

/// Holds the watchdog's check around sleep and dark wakes for the life of the
/// process. Without the notices a wake may abort the client, as Qt warns.
pub fn observe(pulse: Arc<Pulse>) {
    let context = Box::into_raw(Box::new(Context { pulse, root: AtomicU32::new(0) }));
    let mut port: NotificationPort = std::ptr::null_mut();
    let mut notifier: IoObject = 0;
    // SAFETY: a plain registration; the context outlives every callback, since it is never freed.
    let root = unsafe { IORegisterForSystemPower(context.cast(), &mut port, received, &mut notifier) };
    if root == 0 {
        eprintln!("zerus-client: cannot observe system sleep; the main-thread watchdog may abort after a wake");
        // SAFETY: IOKit kept no reference to the context.
        drop(unsafe { Box::from_raw(context) });
        return;
    }
    // SAFETY: the context is live; the queue and the port last as long as the process.
    unsafe {
        (*context).root.store(root, Ordering::Release);
        let queue = dispatch_queue_create(c"hgs-system-power".as_ptr(), std::ptr::null_mut());
        IONotificationPortSetDispatchQueue(port, queue);
    }
}
```

In `client/src-tauri/Cargo.toml`, add `"NSDate"`, `"NSRunLoop"` and `"NSTimer"` to the `objc2-foundation` features of the macOS table. Features never change `Cargo.lock`.

- [ ] **Step 6: Start the watchdog in `launch`**

In `lib.rs`, add the module from Step 1 and this function after `forward_activations`:

```rust
/// The main-thread watchdog (Task 8): Qt's thresholds, a heartbeat on the main
/// thread and, on macOS, the hold around sleep. The returned value keeps it running.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn start_watchdog() -> std::io::Result<watchdog::Watchdog> {
    let limits = watchdog::Limits::QT;
    let dog = watchdog::Watchdog::start(limits, watchdog::now_ms, |silent| watchdog::abort_on_hang(silent))?;
    let pulse = dog.pulse();
    #[cfg(target_os = "macos")]
    {
        platform::macos::heartbeat(limits.beat_every(), move || pulse.beat(watchdog::now_ms()));
        platform::macos_power::observe(dog.pulse());
    }
    #[cfg(target_os = "linux")]
    platform::linux::heartbeat(limits.beat_every(), move || pulse.beat(watchdog::now_ms()));
    eprintln!("zerus-client: main-thread watchdog on ({} s)", limits.hang_ms / 1000);
    Ok(dog)
}
```

In the setup closure, add as its last statement before `Ok(())`, so the watchdog starts once the window and the menu bar exist, as Qt starts it after its tray agent:

```rust
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if identity.watchdog(identity::flag("ZERUS_CLIENT_WATCHDOG")) {
                app.manage(start_watchdog()?);
            }
```

In `client/src-tauri/src/lifecycle.rs`, make this the first statement of `exit`, so the watchdog stops before the socket goes and before any other `RunEvent::Exit` work (drafts, media) runs. Keep `lifecycle::on_event` the first call in `lib.rs`'s run closure (Decision 15):

```rust
    // Quitting is not a hang (`Watchdog::stop`).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(dog) = app.try_state::<crate::watchdog::Watchdog>() {
        dog.stop();
    }
```

- [ ] **Step 7: Check the heartbeat headless on arch**

```bash
cd client && RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=4 nice npx tauri build --no-bundle
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
  ZERUS_CLIENT_WATCHDOG=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson \
  xvfb-run -a -s "-screen 0 1440x900x24" sh -c '
    ./src-tauri/target/release/zerus-client 2> watchdog.log & app=$!
    sleep 50; kill -0 $app && echo "alive after 50 s"
    kill -TERM $app; wait $app; echo "exit $?"
    grep -c "main-thread watchdog on (30 s)" watchdog.log; grep -c "unresponsive" watchdog.log'
rm -f watchdog.log
```
Expected: `alive after 50 s`, `exit 0`, `1`, `0`. Without beats the watchdog would have aborted by its third check, at 45 s. A run without `ZERUS_CLIENT_WATCHDOG` prints no watchdog line.

- [ ] **Step 8: Check on the Mac, display-free**

Run **mac-lock**, **mac-sync**, **mac-clippy**, **mac-test** and **mac-done**. Expected: clean, with the `watchdog::` tests passing on macOS (`CLOCK_UPTIME_RAW`). The heartbeat in an open menu and the sleep hold need a display; runbook steps 4 and 8 cover them.

- [ ] **Step 9: Run the task's checks and commit**

Run fmt, both Linux clippy variants, the src-tauri tests, `check-ui-separators.py` and `check-source-privacy.py`.

```bash
git add client/src-tauri/src/watchdog.rs client/src-tauri/src/identity.rs client/src-tauri/src/lib.rs \
  client/src-tauri/src/lifecycle.rs client/src-tauri/src/platform client/src-tauri/Cargo.toml
git commit -m "feat(client): Qt's main-thread watchdog with the sleep hold"
```

---

### Task 9: The switch bundle, its install path, bundle checks, signing and docs (zerus-5wmh.12.2)

The switch bundle is the Tauri build with Qt's Info.plist identity:
- Identifier `com.hgdev.hgs-tray.local`, promoted to `com.hgdev.hgs-tray` at signing.
- `CFBundleName` Zerus, executable `hgs-tray`, `LSUIElement`.
- Qt's localized-name arrangement for the `hgs-tray.app` folder.
- Qt's icons and the usage texts.
- `ZerusGUI = tauri`.
- The LaunchAgent template in `Contents/Resources/setup`.

`tray/install.sh` builds it when `HGS_MAC_GUI=client` is set or the installed bundle is already the client. It signs it through `tray/sign-local-macos.py` (Developer ID continuity, no hardened runtime, like Qt's local builds) and installs it through the updater of Task 7. `mac-install.sh` ships the client sources and forwards the choice. `scripts/check-macos-bundle.py --gui tauri` checks the bundle, and `scripts/sign-macos.py` signs it for release with the hardened runtime and `packaging/macos/Zerus.entitlements` (no JIT entitlement: WKWebView runs JavaScript out of process). The docs and the coordinator's runbook land here.

As built (webui's rulings of 2026-10-28):
- `mac-install.sh` also ships `crates/watch-protocol` (`Cargo.toml`, `src`) beside `client/`, because `client/core` and `client/src-tauri` take it by path (`../../crates`) and on the Mac the CLI's copy sits under `.rust-build`. Its tarball excludes every `client/*/target`.
- `TrayInstallGui`'s installed-client fixture carries a `CFBundleIdentifier`: after Task 7's review, `--installed-gui` names a kind only for a complete bundle.
- Task 9's review: the client branch of `tray/install.sh` exports `CARGO_TARGET_DIR` (its own `target`) and `CARGO_BUILD_JOBS` (default `JOBS`); `mac-install.sh` also replaces `client/src-tauri/capabilities`; `TrayInstallGui` drops the caller's `HGS_MAC_*`, `HGS_TRAY_RESTART` and Cargo variables. The docs state the settings seed (phase 3b's ruling) instead of defaults, and step 8 checks it.
- `Identity::cache_dir_now()` (Tauri's `app_cache_dir` formula with the running identifier) replaces `app_cache_dir` for the attachment cache, and a `build_info` Tauri command returns what `--build-info` prints, registered with and without `live`.
- The docs and the runbook follow Task 7 as built: Qt waits aside until the client runs, the success line ends "the replaced GUI is the rollback copy in …", and kept copies live in `~/Applications/.hgs-tray-kept-*`. Runbook step 7 also checks the draft import marker.

**Files:**
- Create: `client/src-tauri/tauri.switch.conf.json`, `client/src-tauri/Info.switch.plist`, `client/scripts/switch-config.py`
- Modify: `tray/install.sh` (macOS branch), `mac-install.sh`
- Modify: `scripts/check-macos-bundle.py` (`--gui tauri`)
- Create: `tests/test_macos_switch_bundle.py`; modify `tests/test_mac_install.py`, `tests/test_macos_signing.py`
- Modify: `docs/macos-distribution.md`, `client/README.md`, `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`

**Interfaces:**
- Consumes: `--build-info` (Task 2), `--installed-gui` and the updater (Task 7).
- Produces:
  - `switch-config.py OUTPUT` (writes the merged Tauri configuration).
  - `check-macos-bundle.py --gui tauri`, with `tauri_plist_findings(plist, version)`, `tauri_bundle_findings(bundle, version)` and `tauri_selftest(bundle, version, release, commit)`.
  - `HGS_MAC_GUI=qt|client` for `tray/install.sh` and `mac-install.sh`.

- [ ] **Step 1: Write the failing bundle tests**

Create `tests/test_macos_switch_bundle.py`:

```python
"""The Tauri switch bundle takes Qt's identity (docs/macos-distribution.md#tauri-client-switch)."""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("check_macos_bundle", ROOT / "scripts/check-macos-bundle.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

QT_INFO = plistlib.loads((ROOT / "tray/packaging/Info.plist").read_bytes())
SWITCH_INFO = plistlib.loads((ROOT / "client/src-tauri/Info.switch.plist").read_bytes())
VERSION = (ROOT / "VERSION").read_text().strip()
OTOOL = """hgs-tray:
Load command 0
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform 1
    minos 14.0
Load command 1
          cmd LC_LOAD_DYLIB
      cmdsize 88
         name /System/Library/Frameworks/WebKit.framework/Versions/A/WebKit (offset 24)
Load command 2
          cmd LC_LOAD_DYLIB
      cmdsize 56
         name /usr/lib/libSystem.B.dylib (offset 24)
"""
MACH_O = b"\xcf\xfa\xed\xfe" + b"\0" * 28


def installed_folder():
    install = (ROOT / "tray/install.sh").read_text()
    return re.search(r'^APP_DEST="\$\{HOME\}/Applications/(.+)\.app"$', install, re.M).group(1)


class SwitchBundle(unittest.TestCase):
    def test_the_configuration_takes_qt_identity_with_a_local_build_identifier(self):
        with tempfile.TemporaryDirectory() as scratch:
            output = Path(scratch) / "switch.conf.json"
            subprocess.run([sys.executable, str(ROOT / "client/scripts/switch-config.py"), str(output)], check=True)
            config = json.loads(output.read_text())
        self.assertEqual(config["identifier"], "com.hgdev.hgs-tray.local")
        self.assertEqual((config["productName"], config["mainBinaryName"], config["version"]), ("Zerus", "hgs-tray", VERSION))
        bundle = config["bundle"]
        self.assertEqual(bundle["targets"], ["app"])
        self.assertEqual(list(bundle["resources"].values()), ["setup/com.hgdev.hgs-tray.plist"])
        self.assertEqual(bundle["macOS"]["minimumSystemVersion"], "14.0")
        paths = [*bundle["resources"], bundle["macOS"]["infoPlist"], *bundle["macOS"]["files"].values()]
        for path in paths:
            self.assertTrue(Path(path).is_absolute() and Path(path).is_file(), path)
        self.assertEqual(sorted(bundle["macOS"]["files"]), ["Resources/Assets.car", "Resources/en.lproj/InfoPlist.strings",
                                                            "Resources/hgs-zerus-swarm.icns"])

    def test_the_info_plist_keeps_qt_names_and_texts(self):
        # Launch Services shows the localized Zerus only while the unlocalized name equals the folder.
        self.assertEqual(SWITCH_INFO["CFBundleDisplayName"], installed_folder())
        self.assertIs(SWITCH_INFO["LSHasLocalizedDisplayName"], True)
        self.assertEqual(SWITCH_INFO["CFBundleName"], "Zerus")
        self.assertIs(SWITCH_INFO["LSUIElement"], True)
        self.assertEqual(SWITCH_INFO["ZerusGUI"], "tauri")
        for key in ("NSMicrophoneUsageDescription", "NSCameraUsageDescription", "CFBundleIconName", "CFBundleDevelopmentRegion"):
            self.assertEqual(SWITCH_INFO[key], QT_INFO[key], key)
        package = (ROOT / "scripts/package-macos.py").read_text()
        self.assertIn(SWITCH_INFO["NSAppleEventsUsageDescription"], package)
        self.assertEqual(json.loads((ROOT / "client/src-tauri/tauri.conf.json").read_text())["identifier"], "dev.zerus.next",
                         "the dev app keeps its own identity")

    def bundle(self, root, **changes):
        app = Path(root) / "Zerus.app"
        info = dict(SWITCH_INFO, CFBundleIdentifier="com.hgdev.hgs-tray.local", CFBundleExecutable="hgs-tray",
                    CFBundleShortVersionString=VERSION, LSMinimumSystemVersion="14.0")
        info.update(changes)
        for name in check.TAURI_REQUIRED:
            (app / name).parent.mkdir(parents=True, exist_ok=True)
            (app / name).write_bytes(b"")
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        (app / check.TAURI_EXECUTABLE).write_bytes(MACH_O)
        (app / check.TAURI_EXECUTABLE).chmod(0o755)
        return app

    def tools(self, otool=OTOOL):
        def fake(command, **_):
            return "arm64\n" if command[0] == "lipo" else otool
        return mock.patch.object(check.subprocess, "check_output", side_effect=fake)

    def test_the_tauri_profile_accepts_the_switch_layout(self):
        with tempfile.TemporaryDirectory() as root, self.tools():
            self.assertEqual(check.tauri_bundle_findings(self.bundle(root), VERSION), [])

    def test_the_tauri_profile_refuses_what_local_signing_would_refuse(self):
        with tempfile.TemporaryDirectory() as root, self.tools():
            app = self.bundle(root, LSUIElement=False, ZerusGUI="qt")
            (app / "Contents/Frameworks/Helper.dylib").parent.mkdir(parents=True)
            (app / "Contents/Frameworks/Helper.dylib").write_bytes(MACH_O)
            (app / "Contents/Resources/link").symlink_to("/usr")
            (app / "Contents/Resources/setup/com.hgdev.hgs-tray.plist").unlink()
            findings = check.tauri_bundle_findings(app, VERSION)
        self.assertIn("Info.plist LSUIElement is False, expected True", findings)
        self.assertIn("Info.plist ZerusGUI is 'qt', expected 'tauri'", findings)
        self.assertIn("Contents/Frameworks/Helper.dylib: nested code; the switch bundle has one executable", findings)
        self.assertIn("Contents/Resources/link: symbolic link", findings)
        self.assertIn("missing Contents/Resources/setup/com.hgdev.hgs-tray.plist", findings)

    def test_homebrew_links_and_a_newer_minimum_are_refused(self):
        brewed = OTOOL.replace("/usr/lib/libSystem.B.dylib", "/opt/homebrew/lib/libexample.dylib").replace("minos 14.0", "minos 15.0")
        with tempfile.TemporaryDirectory() as root, self.tools(brewed):
            findings = check.tauri_bundle_findings(self.bundle(root), VERSION)
        self.assertIn("Contents/MacOS/hgs-tray: links /opt/homebrew/lib/libexample.dylib", findings)
        self.assertIn("Contents/MacOS/hgs-tray: requires macOS 15.0, newer than 14.0", findings)

    def test_both_identifiers_pass_and_others_do_not(self):
        good = dict(SWITCH_INFO, CFBundleExecutable="hgs-tray", CFBundleShortVersionString=VERSION, LSMinimumSystemVersion="14.0")
        for identifier in ("com.hgdev.hgs-tray", "com.hgdev.hgs-tray.local"):
            self.assertEqual(check.tauri_plist_findings(dict(good, CFBundleIdentifier=identifier), VERSION), [])
        self.assertTrue(check.tauri_plist_findings(dict(good, CFBundleIdentifier="dev.zerus.next"), VERSION))
        missing = dict(good, CFBundleIdentifier="com.hgdev.hgs-tray")
        del missing["NSMicrophoneUsageDescription"]
        self.assertIn("Info.plist lacks NSMicrophoneUsageDescription", check.tauri_plist_findings(missing, VERSION))


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest discover -s tests -p test_macos_switch_bundle.py -v`
Expected: FAIL (`Info.switch.plist`, `switch-config.py`, `TAURI_REQUIRED`, `TAURI_EXECUTABLE`, `tauri_bundle_findings` and `tauri_plist_findings` do not exist).

- [ ] **Step 2: Write the switch configuration**

Create `client/src-tauri/tauri.switch.conf.json` (paths relative to `client/src-tauri`):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Zerus",
  "mainBinaryName": "hgs-tray",
  "identifier": "com.hgdev.hgs-tray.local",
  "bundle": {
    "active": true,
    "targets": ["app"],
    "resources": {
      "../../tray/packaging/com.hgdev.hgs-tray.plist": "setup/com.hgdev.hgs-tray.plist"
    },
    "macOS": {
      "minimumSystemVersion": "14.0",
      "infoPlist": "Info.switch.plist",
      "files": {
        "Resources/en.lproj/InfoPlist.strings": "../../tray/packaging/en.lproj/InfoPlist.strings",
        "Resources/hgs-zerus-swarm.icns": "../../tray/resources/icons/hgs-zerus-swarm.icns",
        "Resources/Assets.car": "../../tray/resources/icons/macos/Assets.car"
      }
    }
  }
}
```

Create `client/src-tauri/Info.switch.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- Merged into the switch bundle's Info.plist (tauri.switch.conf.json).
         Qt's arrangement: Launch Services shows en.lproj's Zerus only while this
         unlocalized name equals the bundle folder, hgs-tray.app. -->
    <key>CFBundleName</key>              <string>Zerus</string>
    <key>CFBundleDisplayName</key>       <string>hgs-tray</string>
    <key>LSHasLocalizedDisplayName</key> <true/>
    <key>CFBundleDevelopmentRegion</key> <string>en</string>
    <key>CFBundleIconFile</key>          <string>hgs-zerus-swarm.icns</string>
    <key>CFBundleIconName</key>          <string>hgs-zerus-swarm</string>
    <!-- A menu-bar app; the client shows a Dock icon while its window is open. -->
    <key>LSUIElement</key>               <true/>
    <key>NSMicrophoneUsageDescription</key><string>Zerus records your voice when you start dictation and sends it to your selected speech computer for recognition.</string>
    <key>NSCameraUsageDescription</key><string>Zerus records video messages when you hold the video recording button.</string>
    <key>NSAppleEventsUsageDescription</key><string>Zerus opens your agent sessions in Terminal.</string>
    <!-- tray/update-installed.py tells the clients apart by this key. -->
    <key>ZerusGUI</key>                  <string>tauri</string>
</dict>
</plist>
```

Create `client/scripts/switch-config.py`:

```python
#!/usr/bin/env python3
"""Write the Tauri configuration of the Mac switch bundle.

tauri.switch.conf.json with Zerus' version from VERSION and every path made
absolute, so `tauri build --config OUTPUT` reads the same files wherever
OUTPUT lives (docs/macos-distribution.md#tauri-client-switch).

    switch-config.py OUTPUT
"""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
TAURI = ROOT / "client/src-tauri"


def absolute(path):
    return str((TAURI / path).resolve())


def config(version):
    data = json.loads((TAURI / "tauri.switch.conf.json").read_text())
    data["version"] = version
    bundle = data["bundle"]
    bundle["resources"] = {absolute(source): target for source, target in bundle["resources"].items()}
    mac = bundle["macOS"]
    mac["infoPlist"] = absolute(mac["infoPlist"])
    mac["files"] = {target: absolute(source) for target, source in mac["files"].items()}
    return data


def main(argv):
    if len(argv) != 2:
        print(__doc__.split("\n\n")[-1], file=sys.stderr)
        return 2
    output = Path(argv[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(config((ROOT / "VERSION").read_text().strip()), indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

- [ ] **Step 3: Add the Tauri profile to `check-macos-bundle.py`**

Add after `bundle_findings`:

```python
# The Tauri switch bundle: one executable, Qt's names and keys, the LaunchAgent template.
TAURI_EXECUTABLE = "Contents/MacOS/hgs-tray"
TAURI_REQUIRED = [
    "Contents/Info.plist", TAURI_EXECUTABLE,
    "Contents/Resources/setup/com.hgdev.hgs-tray.plist",
    "Contents/Resources/en.lproj/InfoPlist.strings",
    "Contents/Resources/hgs-zerus-swarm.icns",
    "Contents/Resources/Assets.car",
]


def tauri_plist_findings(plist, version):
    expected = {"CFBundleExecutable": "hgs-tray", "CFBundleName": "Zerus", "CFBundleDisplayName": "hgs-tray",
                "LSHasLocalizedDisplayName": True, "LSUIElement": True, "CFBundleShortVersionString": version,
                "LSMinimumSystemVersion": MINIMUM_SYSTEM, "CFBundleIconName": "hgs-zerus-swarm", "ZerusGUI": "tauri"}
    findings = [f"Info.plist {key} is {plist.get(key)!r}, expected {value!r}"
                for key, value in expected.items() if plist.get(key) != value]
    # The raw build keeps the local identifier; tray/sign-local-macos.py promotes its signed copy.
    if plist.get("CFBundleIdentifier") not in (BUNDLE_ID, BUNDLE_ID + ".local"):
        findings.append(f"Info.plist CFBundleIdentifier is {plist.get('CFBundleIdentifier')!r}, "
                        f"expected {BUNDLE_ID!r} or {BUNDLE_ID + '.local'!r}")
    for key in ("NSMicrophoneUsageDescription", "NSAppleEventsUsageDescription"):
        if not str(plist.get(key, "")).strip():
            findings.append(f"Info.plist lacks {key}")
    return findings


def tauri_bundle_findings(bundle, version):
    findings = tauri_plist_findings(plistlib.loads((bundle / "Contents/Info.plist").read_bytes()), version)
    findings += [f"missing {name}" for name in TAURI_REQUIRED if not (bundle / name).is_file()]
    main = bundle / TAURI_EXECUTABLE
    if main.is_file() and not os.access(main, os.X_OK):
        findings.append(f"not executable: {TAURI_EXECUTABLE}")
    for directory, names, files in os.walk(bundle):
        for name in sorted(names + files):
            path = Path(directory) / name
            label = str(path.relative_to(bundle))
            # tray/sign-local-macos.py refuses links and nested code.
            if path.is_symlink():
                findings.append(f"{label}: symbolic link")
                continue
            if name not in files or not is_mach_o(path):
                continue
            if path != main:
                findings.append(f"{label}: nested code; the switch bundle has one executable")
                continue
            architectures = subprocess.check_output(["lipo", "-archs", str(path)], text=True).split()
            if architectures != ["arm64"]:
                findings.append(f"{label}: architectures {' '.join(architectures)}, expected arm64 only")
            findings += reference_findings(label, load_commands(subprocess.check_output(["otool", "-l", str(path)], text=True)))
    return findings


def tauri_selftest(bundle, version, release=None, commit=None):
    """`--version` and `--build-info` in an empty home: the bundle starts nothing else."""
    with tempfile.TemporaryDirectory(prefix="zerus-bundle-home-") as scratch:
        environment = {"HOME": scratch, "PATH": "/usr/bin:/bin", "TMPDIR": scratch}
        executable = str(bundle / TAURI_EXECUTABLE)
        result = subprocess.run([executable, "--version"], env=environment, capture_output=True, text=True, timeout=60)
        if result.returncode:
            return [f"--version failed: {result.stderr.strip() or result.stdout.strip()}"]
        result = subprocess.run([executable, "--build-info"], env=environment, capture_output=True, text=True, timeout=60)
        try:
            info = json.loads(result.stdout) if result.returncode == 0 else None
        except ValueError:
            info = None
        return build_identity_findings(info, version, release, commit)
```

In `main`, add `parser.add_argument("--gui", choices=("qt", "tauri"), default="qt", help="the Qt release bundle or the Tauri switch bundle")` and select the profile:

```python
    if args.gui == "tauri":
        findings = tauri_bundle_findings(args.bundle, args.version)
        if not findings and args.selftest:
            findings = tauri_selftest(args.bundle, args.version, args.release, args.source_commit)
    else:
        findings = bundle_findings(args.bundle, args.version, args.release)
        if not findings and args.selftest:
            findings = run_selftest(args.bundle, args.version, args.release, args.source_commit)
```

Keep the `--team-id` signature check after either profile.

- [ ] **Step 4: Run the bundle tests**

Run: `python3 -m unittest discover -s tests -p 'test_macos_*.py' -v` (it includes the switch bundle, bundle and package tests)
Expected: PASS.

- [ ] **Step 5: Pin release signing of the switch layout**

Add to `Signing` in `tests/test_macos_signing.py`:

```python
    def test_a_tauri_switch_bundle_signs_with_the_hardened_runtime_and_app_entitlements(self):
        app = Path(self.temporary.name) / "Tauri/Zerus.app"
        (app / "Contents/MacOS").mkdir(parents=True)
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleExecutable": "hgs-tray", "ZerusGUI": "tauri"}))
        (app / "Contents/MacOS/hgs-tray").write_bytes(MACH_O)
        (app / "Contents/Resources/setup").mkdir(parents=True)
        (app / "Contents/Resources/setup/com.hgdev.hgs-tray.plist").write_text("<plist/>")
        self.assertEqual(sign.signing_order(app), [app], "one executable: the app is the only code object")
        command = sign.codesign_command(app, "-", sign.entitlements_for(app, app))
        self.assertEqual(command[command.index("--options") + 1], "runtime")
        entitlements = plistlib.loads(sign.APP_ENTITLEMENTS.read_bytes())
        self.assertTrue(entitlements["com.apple.security.device.audio-input"])
        self.assertTrue(entitlements["com.apple.security.automation.apple-events"])
        self.assertNotIn("com.apple.security.cs.allow-jit", entitlements, "WKWebView runs JavaScript out of process")
```

Run: `python3 -m unittest discover -s tests -p test_macos_signing.py -v`
Expected: PASS without changing `scripts/sign-macos.py`. Its release archive metadata (`qt_version`) stays Qt's; a Tauri release archive is phase 7 (Decision 17).

Commit the bundle and its checks, the task's first reviewable unit:

```bash
git add client/src-tauri/tauri.switch.conf.json client/src-tauri/Info.switch.plist client/scripts/switch-config.py \
  scripts/check-macos-bundle.py tests/test_macos_switch_bundle.py tests/test_macos_signing.py
git commit -m "feat(client): the Mac switch bundle with Qt's identity and its bundle checks"
```

- [ ] **Step 6: Write the failing install tests**

In `tests/test_mac_install.py`, change the expectation of `test_local_signing_selector_travels_as_one_literal_value` to:

```python
        self.assertEqual(shlex.split(commands[0]), ['HGS_MACOS_SIGN_IDENTITY=' + selector, 'HGS_MAC_GUI=',
                                                  'bash', '~/.local/src/hgs/tray/install.sh'])
```

Add to `MacInstall`:

```python
    def test_the_client_sources_travel_and_the_gui_choice_is_forwarded(self):
        result = self.run_install('fixture-mac', HGS_MAC_GUI='client')
        self.assertEqual(result.returncode, 0, result.stderr)
        tray = [line.split() for line in (self.root / 'transport.log.tar').read_text().splitlines()
                if 'tray' in line.split()]
        self.assertEqual(len(tray), 1)
        for expected in ('client', 'VERSION', 'DEV_VERSION', '--exclude=client/node_modules',
                         '--exclude=client/src-tauri/target', '--exclude=client/dist'):
            self.assertIn(expected, tray[0])
        commands = [line for line in (self.root / 'transport.log').read_text().splitlines() if 'tray/install.sh' in line]
        self.assertIn("HGS_MAC_GUI='client'", commands[0])
```

Add a class beside `TrayInstallCache`:

```python
class TrayInstallGui(unittest.TestCase):
    """The macOS desktop build chooses the Tauri client, stopped before signing."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zerus-tray-gui-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ('tray/install.sh', 'tray/update-installed.py', 'client/scripts/switch-config.py',
                     'client/src-tauri/tauri.switch.conf.json', 'VERSION'):
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_bytes((REPO / name).read_bytes())
        tools = self.root / 'bin'
        tools.mkdir()
        self.log = self.root / 'tools.log'
        for name, text in {
            'uname': '#!/bin/sh\necho Darwin\n',
            'brew': '#!/bin/sh\necho /opt/fixture/qt\n',
            'cmake': f'#!/bin/sh\necho "cmake $*" >> "{self.log}"\nexit 3\n',
            # npm ci succeeds; the build stops the installer before signing.
            'npm': f'#!/bin/sh\necho "npm $*" >> "{self.log}"\n[ "$1" = ci ] && exit 0\nexit 3\n',
        }.items():
            (tools / name).write_text(text)
            (tools / name).chmod(0o755)
        self.home = self.root / 'home'
        self.env = dict(os.environ, HOME=str(self.home), PATH=str(tools) + os.pathsep + os.environ['PATH'])

    def install(self, **settings):
        return subprocess.run(['bash', str(self.root / 'tray/install.sh')], env=dict(self.env, **settings),
                              text=True, capture_output=True, timeout=30)

    def test_the_client_is_built_with_the_switch_configuration(self):
        result = self.install(HGS_MAC_GUI='client')
        self.assertEqual(result.returncode, 3, result.stderr)
        log = self.log.read_text().splitlines()
        config = self.root / 'client/src-tauri/target/switch.conf.json'
        self.assertEqual(log, ['npm ci --no-audit --no-fund',
                               f'npm exec -- tauri build --config {config} --bundles app'])
        self.assertEqual(json.loads(config.read_text())['identifier'], 'com.hgdev.hgs-tray.local')

    def test_an_installed_client_stays_the_client_and_qt_stays_qt(self):
        app = self.home / 'Applications/hgs-tray.app/Contents'
        app.mkdir(parents=True)
        (app / 'Info.plist').write_bytes(plistlib.dumps({'ZerusGUI': 'tauri'}))
        self.install()
        self.assertTrue(self.log.read_text().startswith('npm ci'))
        self.log.unlink()
        (app / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier': 'com.hgdev.hgs-tray'}))
        self.install()
        self.assertTrue(self.log.read_text().startswith('cmake'))
        self.log.unlink()
        self.install(HGS_MAC_GUI='qt')
        self.assertTrue(self.log.read_text().startswith('cmake'))
        self.assertEqual(self.install(HGS_MAC_GUI='other').returncode, 1)
```

Add `import json` and `import plistlib` to the test module's imports.

Run: `python3 -m unittest discover -s tests -p test_mac_install.py -v`
Expected: the new tests FAIL (no `HGS_MAC_GUI` handling, no client build).

- [ ] **Step 7: Build the client from `tray/install.sh`**

In the macOS branch of `tray/install.sh`, replace the `# --- 1. build ---` block with:

```bash
# --- 0. which GUI -------------------------------------------------------------
# The kind already installed stays (update-installed.py --installed-gui) unless
# HGS_MAC_GUI chooses: qt for the Qt GUI, client for the Tauri client
# (docs/macos-distribution.md#tauri-client-switch). A Mac without Zerus gets Qt.
GUI="${HGS_MAC_GUI:-}"
if [ -z "${GUI}" ]; then
    GUI=qt
    if [ -f "${SCRIPT_DIR}/update-installed.py" ] \
        && [ "$(python3 "${SCRIPT_DIR}/update-installed.py" --installed-gui "${APP_DEST}" 2>/dev/null)" = "tauri" ]; then
        GUI=client
    fi
fi
case "${GUI}" in qt|client) ;; *) echo "HGS_MAC_GUI must be qt or client" >&2; exit 1 ;; esac

# --- 1. build -----------------------------------------------------------------
echo "[1/4] Building..."
if [ "${GUI}" = "client" ]; then
    CLIENT_DIR="$(cd "${SCRIPT_DIR}/../client" && pwd)"
    NPM="$(find_tool npm)" || { echo "hgs-tray: npm not found (brew install node)" >&2; exit 1; }
    export PATH="${HOME}/.cargo/bin:$(dirname "${NPM}"):${PATH}"
    export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.99.1}"
    (cd "${CLIENT_DIR}" && "${NPM}" ci --no-audit --no-fund)
    SWITCH_CONFIG="${CLIENT_DIR}/src-tauri/target/switch.conf.json"
    python3 "${CLIENT_DIR}/scripts/switch-config.py" "${SWITCH_CONFIG}"
    # Voice capture (events M1c) ships whenever the client has its feature.
    FEATURES=()
    if grep -q '^media = ' "${CLIENT_DIR}/src-tauri/Cargo.toml" 2>/dev/null; then FEATURES=(--features media); fi
    # ZERUS_DEV_BUILD_NUMBER reaches the client's build.rs through the environment.
    (cd "${CLIENT_DIR}" && "${NPM}" exec -- tauri build --config "${SWITCH_CONFIG}" --bundles app ${FEATURES[@]+"${FEATURES[@]}"})
    BUILD_APP="${CLIENT_DIR}/src-tauri/target/release/bundle/macos/Zerus.app"
else
    CMAKE="$(find_tool cmake)" || { echo "hgs-tray: cmake not found (brew install cmake)" >&2; exit 1; }
    BREW="$(find_tool brew)"   || { echo "hgs-tray: brew not found -- Qt6 comes from it" >&2; exit 1; }
```

Then keep the existing `stale_sdk` function, cache check and the two `"${CMAKE}"` lines inside the `else`, and close it with `fi`. Step 2 stays as is: it copies `${BUILD_APP}` to the stage as `hgs-tray.app`, signs it with `sign-local-macos.py` (which promotes `.local`), compares cdhashes and hands it to the updater. Change the self-test hint to:

```bash
if [ "${GUI}" = "client" ]; then
    HINT_SELFTEST="Check without the menu bar:  ${APP_DEST}/Contents/MacOS/hgs-tray --build-info"
else
    HINT_SELFTEST="Check without the menu bar:  ${APP_DEST}/Contents/MacOS/hgs-tray --selftest --hgs ${HGS_BIN}"
fi
```

The LaunchAgent step does not change: the template's `ProgramArguments` (`__APP__/Contents/MacOS/hgs-tray --hgs __HGS__`) start the client exactly as they start Qt.

- [ ] **Step 8: Ship the client from `mac-install.sh`**

Replace the tray tarball and install lines at the end of `mac-install.sh` with:

```bash
# The Tauri client's sources travel too, so tray/install.sh can build it
# (HGS_MAC_GUI=client, or a Mac whose installed Zerus is already the client).
# Source folders are replaced; their build caches stay. A numbered Dev build
# needs a clean Git checkout on the Mac instead (docs/macos-distribution.md).
tar czf - -C "$here" --exclude='tray/build' --exclude='tray/.cache' \
  --exclude='client/node_modules' --exclude='client/dist' --exclude='client/src-tauri/target' \
  --exclude='client/src-tauri/gen' --exclude='client/core/target' \
  tray client DEV_VERSION VERSION \
  | ssh -o BatchMode=yes "$host" 'mkdir -p ~/.local/src/hgs && cd ~/.local/src/hgs && rm -rf client/src client/core/src client/src-tauri/src && tar xzf -'
ssh -o BatchMode=yes "$host" "HGS_MACOS_SIGN_IDENTITY=$(shq "${HGS_MACOS_SIGN_IDENTITY:-}") HGS_MAC_GUI=$(shq "${HGS_MAC_GUI:-}") bash ~/.local/src/hgs/tray/install.sh"
```

Add `HGS_MAC_GUI` to the help comment at the top of `mac-install.sh`: `# HGS_MAC_GUI=qt|client chooses the desktop GUI; by default the installed kind stays.`

- [ ] **Step 9: Run the install tests**

Run: `python3 -m unittest discover -s tests -p test_mac_install.py -v && python3 -m unittest discover -s tests -p 'test_tray_update*.py' -v`
Expected: PASS.

Commit the install path, the second unit:

```bash
git add tray/install.sh mac-install.sh tests/test_mac_install.py
git commit -m "feat(tray): build and install the Tauri client on the Mac when chosen or installed"
```

- [ ] **Step 10: Write the docs**

Add this section to `docs/macos-distribution.md`, before `## Mac App Store`:

````markdown
## Tauri client switch

On the owner's decision of 2026-10-28, the Tauri client (`client/`) replaces the Qt GUI on the Mac as Zerus. It takes over Qt's identity, `com.hgdev.hgs-tray`, at the installed path `~/Applications/hgs-tray.app`. Qt is removed at the switch and kept as the one rollback copy; all of Qt's data and drafts stay where they are. The dev app keeps `dev.zerus.next`.

### The switch bundle

`client/scripts/switch-config.py` writes `client/src-tauri/target/switch.conf.json` from `client/src-tauri/tauri.switch.conf.json` and `VERSION`; `tauri build --config` with it produces `Zerus.app`:

- **Identity.** It has the executable `hgs-tray` and the build identifier `com.hgdev.hgs-tray.local`; `tray/sign-local-macos.py` promotes its signed copy to `com.hgdev.hgs-tray`, as for Qt's raw bundles.
- **Info.plist.** `Info.switch.plist` adds Qt's keys:
  - `CFBundleName` Zerus, with `CFBundleDisplayName` hgs-tray and `en.lproj` naming it Zerus;
  - `LSUIElement`;
  - Qt's icons and usage texts;
  - `ZerusGUI = tauri`, which `tray/update-installed.py` uses to tell the clients apart.
- **LaunchAgent template.** It ships in `Contents/Resources/setup/com.hgdev.hgs-tray.plist`. The installed agent is unchanged: `KeepAlive Crashed`, `ProcessType Interactive`, `--hgs` and the same label.

The client behaves as Qt does at the points other tools rely on:

- **Command line.** `--hgs`, `--sessions`, `--background`, `--version` and `--build-info` (Qt's JSON). Qt's other options are accepted and ignored.
- **Single instance.** It uses Qt's protocol: the lock `~/Library/Application Support/hgs-tray-start.lock`, the socket `hgs-tray` in the canonical per-user temporary folder, and the lines `show` and `sessions`. A start that launchd did not run waits for the service.
- **Quitting.** The Apple Event quit and SIGTERM, SIGINT and SIGHUP all end in the same exit path, which flushes what must reach the disk and removes the socket.
- **Menu-bar item.** It shows Qt's tile, tooltip and menu. Refresh and the update entries are left out. A primary click opens Zerus; closing the window keeps the app in the menu bar.
- **Notifications.** It posts Qt's attention notifications under the same identifiers (`zerus.attention.<token>`). Qt's saved `attention.observed` seeds the client's state once, so banners Qt delivered are withdrawn and stay clickable. Qt's update notices ("Zerus update available") are not ported yet (`zerus-5wmh.12.4`).
- **Main-thread watchdog.** Qt's: a main thread silent for 30 s aborts the client, so launchd's `KeepAlive Crashed` restarts it, with a hold around sleep and dark wakes. The menu and modal panels do not stop its heartbeat.

The client keeps its own data in `~/Library/Application Support/com.hgdev.hgs-tray/`: read marks, `attention-observed.json`, `menubar.json` and its drafts root, `drafts/`. It reads Qt's settings domain, `~/.config/hgs/tray.conf` and Qt's drafts, and never writes them. Its settings are seeded once, at its first start, from Qt's values (coordinator ruling: "Seed it once from Qt's values with Qt's types and defaults, and never write Qt's store"). The seed reads Qt's domain read-only through `defaults export`, records itself in `settings.json` (source, time, keys) and never runs again, so a later Qt edit is not imported. Three exceptions: the client's own `client/…` settings start at their defaults; the microphone (`speech/microphone`) must be chosen again, because Qt keeps a device id the client cannot use; and automatic approvals stay off, since Qt's choice is kept only as `automaticApprovals/qtEnabledAtSwitch` so the page can say Qt had them on (`zerus-5wmh.54`). Read marks start fresh, since the hosts' read state is authoritative; the one-time draft import (`zerus-5wmh.13.1`) copies Qt's drafts and the dev app's drafts into its drafts root, reading both read-only. AppKit writes its own keys into the shared defaults domain: the status item's position and visibility (`NSStatusItem …`) and, where used, window and panel state (`NSWindow Frame …`, `NSNav…`). The updater reports them after each client update.

### Building and installing

Numbered Dev installs need a clean checkout of the exact main commit, as for Qt:

```sh
export ZERUS_DEV_BUILD_NUMBER=NUMBER_FROM_THIS_MAIN_PUSH
HGS_MAC_GUI=client bash tray/install.sh
```

- **Choosing the GUI.** `HGS_MAC_GUI=client` chooses the client; later runs keep whichever GUI is installed (`HGS_MAC_GUI=qt` chooses Qt again). `mac-install.sh` ships the client sources and forwards `HGS_MAC_GUI`, but its tarball has no Git metadata and cannot make a numbered build.
- **The build.** It needs npm and Rust 1.99.1 on the Mac. With the `media` feature present, the build enables it. It always builds into `client/src-tauri/target`, whatever `CARGO_TARGET_DIR` says, since the installer takes the bundle from there, and the installer's job count bounds Cargo as it bounds the Qt build (`CARGO_BUILD_JOBS` overrides it).
- **The updater at the switch.** `tray/update-installed.py`:
  1. quits Qt as its Quit action does, so Qt saves its drafts: SIGTERM, which current builds handle, or the quit Apple Event for an older one;
  2. snapshots Qt's drafts and the client's drafts root;
  3. exports Qt's settings with `defaults export com.hgdev.hgs-tray` to `~/.local/share/hgs/desktop-previous/com.hgdev.hgs-tray.plist`;
  4. sets Qt aside, removed from LaunchServices, and installs the client; the rollback folder does not change yet;
  5. bootstraps the same LaunchAgent, waits on Qt's socket path and sends `sessions`;
  6. once the client runs, makes Qt the one rollback copy, `~/.local/share/hgs/desktop-previous/hgs-tray.app`;
  7. verifies the drafts, the native processes and Qt's settings keys.

  A failure or interrupt before step 6 puts Qt back, or keeps it in `~/Applications/.hgs-tray-kept-*/previous/hgs-tray.app` and prints where. Later updates list such folders and never remove them.
- **Later client updates.** Qt stays the rollback copy until the owner retires Qt; there is no second slot. The replaced client is kept only while that update runs, so a new client that does not start is replaced by it automatically.
- **Dev numbers.** The updater refuses to replace a numbered Dev install with a build that has neither a Dev number nor a local preview label, also for `HGS_TRAY_RESTART=0` (`docs/reference.md`). `HGS_TRAY_RESTART=0` never switches between Qt and the client.

### Rollback

```sh
python3 tray/update-installed.py --rollback ~/Applications/hgs-tray.app
```

The rollback reinstalls exactly the rollback copy through the same quit, snapshot, start and verification steps. After the switch, that copy is the last Qt bundle. It compares Qt's settings with the copy taken at the switch before Qt starts. If they differ, quit Zerus, run `defaults import com.hgdev.hgs-tray ~/.local/share/hgs/desktop-previous/com.hgdev.hgs-tray.plist`, then `launchctl kickstart gui/$(id -u)/com.hgdev.hgs-tray`, since KeepAlive Crashed does not restart a clean quit. The client then becomes the rollback copy. Drafts typed in the client after the switch stay in `~/Library/Application Support/com.hgdev.hgs-tray/drafts/`, where Qt does not see them. Use `HGS_MAC_GUI=qt` on later installs to stay on Qt.

### Privacy identity

TCC grants and the Notification Center entry belong to the bundle identifier and its designated requirement. The installed Qt is signed with a Developer ID Application certificate, without the hardened runtime. A switch bundle signed by `tray/sign-local-macos.py` with a Developer ID Application certificate of the same team satisfies Qt's requirement, so the grants carry over. An ad hoc or Apple Development signature would not, and the grants would be asked for again. Before the switch, compare the two requirements (the runbook below); `tray/sign-local-macos.py` already refuses an incompatible signature.

### Release signing

`scripts/check-macos-bundle.py --gui tauri` checks the switch bundle; `--selftest` runs only `--version` and `--build-info`. `scripts/sign-macos.py` signs it inside-out with the hardened runtime and `packaging/macos/Zerus.entitlements`. No JIT entitlement is needed, because WKWebView runs JavaScript out of process. Notarization runs only in the `macos-signing` environment. A public Tauri `Zerus.app` with the CLI, connector, setup scripts and notices, and its release archive, is phase 7 (`zerus-5wmh.12`).

### Switch runbook
````

Then copy the plan's **Switch runbook (coordinator)** section, every step, under `### Switch runbook`.

In `client/README.md`, replace the paragraph that starts "The app is a separate application" with:

```markdown
The dev app is a separate application (`dev.zerus.next`, "Zerus Next (dev)"): it
never claims Qt's single-instance socket, never posts notifications and runs no
main-thread watchdog unless asked (`ZERUS_CLIENT_SINGLE_INSTANCE=1`,
`ZERUS_CLIENT_NOTIFICATIONS=1`, `ZERUS_CLIENT_WATCHDOG=1`), and it never starts,
stops or replaces the installed Zerus GUI, CLI, agents or services.
On the Mac the client also builds as Zerus itself, the switch bundle that takes
over `com.hgdev.hgs-tray`; see `docs/macos-distribution.md#tauri-client-switch`.
```

Add a section before `## Build and run`:

```markdown
## Menu bar, single instance and notifications (macOS)

| Piece | Where | Qt source it follows |
| --- | --- | --- |
| Command line (`--hgs`, `--sessions`, `--background`, `--version`, `--build-info`) | `src-tauri/src/cli.rs`, `build.rs` | `AppConfig.cpp`, `UpdateController::buildInfo` |
| Identity and data directory | `src-tauri/src/identity.rs` | owner decision of 2026-10-28 |
| Single instance (lock, socket `hgs-tray`, `show`/`sessions`) | `src-tauri/src/instance.rs` | `SingleInstance.cpp`, `main.cpp` |
| Hidden start, close hides, Dock icon only with a window, signals | `src-tauri/src/lifecycle.rs`, `signals.rs` | `main.cpp`, `MacAppVisibility.mm`, `TerminationSignals.h` |
| Tile, tooltip, menu, "Open in" | `core/src/menubar.rs`, `core/src/trayicon.rs`, `src-tauri/src/desk.rs`, `src-tauri/src/platform/macos_menubar.rs` | `IconFactory.cpp`, `FleetState.cpp`, `TrayMenu.cpp`, `QuietTrayIcon.h`, `MacBackend.cpp` |
| Attention notifications | `core/src/attention.rs`, `src-tauri/src/platform/macos_notify.rs` | `AttentionTracker.cpp`, `MacAttentionNotifier.mm` |
| Main-thread watchdog and its sleep hold | `src-tauri/src/watchdog.rs`, `src-tauri/src/platform/macos_power.rs` | `MainThreadWatchdog.cpp`, `MacSystemPower.cpp` |

The data directory is Tauri's `app_data_dir` formula with the running bundle's
identifier: `~/Library/Application Support/dev.zerus.next/` for the dev app,
`~/Library/Application Support/com.hgdev.hgs-tray/` for the switch bundle. The
switch bundle does not see what the dev app saved there.
```

In the spec's `## Decisions` section, append:

```markdown
Mac switch, 2026-10-28 (plan `docs/superpowers/plans/2026-10-28-tauri-client-mac-switch.md`):

- The client replaces Qt on the Mac as Zerus with Qt's identity
  (`com.hgdev.hgs-tray`, executable `hgs-tray`, `~/Applications/hgs-tray.app`).
  This supersedes, for the Mac, "A distinct bundle identifier from the Qt app"
  and the rule that binary names and paths must not collide with `hgs-tray`.
- Single instance follows Qt's protocol (lock, socket `hgs-tray`, `show` and
  `sessions` lines) instead of `tauri-plugin-single-instance` (row 298): the
  maintained updater waits on Qt's socket path and would roll back otherwise.
- The menu keeps Qt's entries except Refresh (every computer streams) and the
  update entries (no update controller yet, row 300); update notices follow in
  `zerus-5wmh.12.4`. The main-thread watchdog (row 297) is Qt's, and the status
  item is never updated while its menu is open.
- The client's data directory follows the running bundle's identifier. At the
  switch its settings are seeded once, read-only, from Qt's (except the client's
  own `client/` settings, the microphone and automatic approvals, which stay
  off), and read marks start fresh.
```

- [ ] **Step 11: Check the docs and the whole branch**

Run: `python3 scripts/check-ui-separators.py && python3 scripts/check-source-privacy.py && python3 -m unittest discover -s tests -p 'test_macos_*.py' -v && python3 -m unittest discover -s tests -p test_mac_install.py -v && python3 -m unittest discover -s tests -p 'test_tray_update*.py' -v`, then the full Rust and frontend checks from Global Constraints.
Expected: all PASS.

Run: `grep -rn "app_data_dir()" client/src-tauri/src`
Expected: no output. Every client path comes from `Identity::data_dir_now()` (Decision 5); if phase 4 landed first with `app.path().app_data_dir()` for its drafts root, route it through `app.state::<identity::Identity>()` here.

- [ ] **Step 12: Build the switch bundle on the Mac without installing it**

Run **mac-lock** and **mac-sync**, then build and check the switch bundle in the persistent checkout, and end with **mac-done**:

```bash
ssh -o BatchMode=yes mac 'cd ~/zerus-dev/webui/client && export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH" RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=6 \
  && npm ci --no-audit --no-fund && python3 scripts/switch-config.py src-tauri/target/switch.conf.json \
  && npx tauri build --config src-tauri/target/switch.conf.json --bundles app \
  && python3 ../scripts/check-macos-bundle.py --gui tauri --version "$(cat ../VERSION)" --selftest src-tauri/target/release/bundle/macos/Zerus.app'
```

Expected: `Zerus.app <VERSION>: bundle checks passed.` The self-test runs only `--version` and `--build-info`, which answer before anything starts. Do not open, sign or install this bundle: its identifier is `com.hgdev.hgs-tray.local`, and launching it is a display run.

- [ ] **Step 13: Commit**

```bash
git add docs/macos-distribution.md client/README.md docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md
git commit -m "docs(client): the Mac switch, its rollback and the coordinator's runbook"
```

---

## Switch runbook (coordinator)

The coordinator runs this on the owner's Mac after Tasks 1 to 9 are merged to main and their main push has a CI sequence `N`. Steps marked **display** run only in a window the owner gives: the implementer tells the coordinator when the work reaches step 4, and step 8 is the coordinator's own, done with the owner. Nothing here touches the hgs CLI, `com.hgdev.hgs-recovery`, `com.hgdev.hgs-voice`, native agents, tmux or the mobile connector. Signing needs the login keychain, so signing steps run as a `.command` in Terminal, like other Dev rollouts.

**Before the merge (Task 7).** The numbered Dev guard also refuses unnumbered `tray/install.sh` and `mac-install.sh` runs over a numbered Dev install on arch and on the Mac, including `HGS_TRAY_RESTART=0`. Tell the events and menu-parity sessions before Task 7 reaches main: their ad hoc installs then need `ZERUS_DEV_BUILD_NUMBER` or a local preview label (`ZERUS_LOCAL_PREVIEW_LABEL`, `docs/reference.md`), both from a clean Git checkout. `mac-install.sh` ships a tarball without `.git`, so it can make neither; on a Mac with a numbered Dev install it then updates the CLI and stops at the GUI with the refusal.

**What the switch carries over (coordinator rulings).** Settings are seeded once from Qt's, read-only, except the client's own `client/…` settings (defaults), the microphone (chosen again) and automatic approvals (off; Qt's choice is only recorded). Read marks start fresh; the hosts' read state stays authoritative. Qt's drafts and the dev app's drafts reach the client through its one-time import (`zerus-5wmh.13.1`) at its first start, which reads both stores read-only; step 7 checks its marker. In-app update notices are missing after the switch, accepted until `zerus-5wmh.12.4`; the coordinator rolls Dev out by hand meanwhile.

1. **Prepare the clean checkout** (no display). In the clean clone used for Dev rollouts, record its path and the commit: later steps cd to `$CLONE` and compare against `$SHA` (in another Terminal window, export all three again; the `${CLONE:?}` below stops a step whose `CLONE` is unset):
   ```sh
   cd PATH_OF_THE_CLEAN_CLONE
   git fetch origin && git switch --detach SHA_OF_THAT_MAIN_PUSH
   export ZERUS_DEV_BUILD_NUMBER=N CLONE="$PWD" SHA="$(git rev-parse HEAD)"
   ```
2. **Compare the privacy identity** (no display, Terminal, in the clean clone of step 1). The installed app is read only through `codesign`, and the switch bundle is checked and signed in a scratch folder. The `$(…)` that adds the `media` feature splits into words in zsh and bash alike, where a variable would not in zsh, and `CARGO_TARGET_DIR` set to the clone's own target, as `tray/install.sh` sets it, wins over any `build.target-dir` in the Cargo configuration, so the bundle lands where the next lines take it from:
   ```sh
   cd "${CLONE:?}/client" && npm ci --no-audit --no-fund
   python3 scripts/switch-config.py src-tauri/target/switch.conf.json
   CARGO_TARGET_DIR="$PWD/src-tauri/target" RUSTUP_TOOLCHAIN=1.99.1 npx tauri build --config src-tauri/target/switch.conf.json --bundles app \
     $(grep -q '^media = ' src-tauri/Cargo.toml && echo --features media)
   S="$(mktemp -d)"; ditto src-tauri/target/release/bundle/macos/Zerus.app "$S/hgs-tray.app"
   python3 ../scripts/check-macos-bundle.py --gui tauri --version "$(cat ../VERSION)" \
     --release "$(cat ../DEV_VERSION)-dev.$ZERUS_DEV_BUILD_NUMBER" --source-commit "$SHA" --selftest "$S/hgs-tray.app"
   python3 ../tray/sign-local-macos.py "$S/hgs-tray.app" ~/Applications/hgs-tray.app
   codesign -d -r- ~/Applications/hgs-tray.app 2>&1 | grep designated
   codesign -d -r- "$S/hgs-tray.app" 2>&1 | grep designated
   rm -rf "$S"; cd "${CLONE:?}"
   ```
   `sign-local-macos.py` must print "privacy identity matches the installed application". Report both `designated =>` lines to the owner privately; they contain the team. Equal requirements carry the TCC and Notification Center grants over. Different ones need the owner's decision, because the grants would be asked for again.
3. **Check the socket path** (no display, read-only):
   ```sh
   realpath "$(lsof -n -a -c hgs-tray -U -F n | grep '/hgs-tray$' | sed 's/^n//')"
   echo "$(realpath "$(getconf DARWIN_USER_TEMP_DIR)")/hgs-tray"
   ```
   The two paths must be equal: the client binds `hgs-tray` in the canonical per-user temporary folder (`TMPDIR`, else `_CS_DARWIN_USER_TEMP_DIR`, as Qt's `QDir::tempPath`). Both sides go through `realpath`, so `/var` and `/private/var` spellings of one folder compare equal; the updater reaches the socket through either. If they still differ, stop and report: the updater would time out on the new client and roll it back.
4. **Try the dev app** (**display**, in an owner-given window; the implementer tells the coordinator when the work reaches this step). Build the dev bundle under the heavy-build lock (**mac-lock**): in `~/zerus-dev/webui`, `git fetch origin && git checkout --detach origin/main`, then `CARGO_TARGET_DIR="$PWD/src-tauri/target" npx tauri build --bundles app` in `client/`, so the bundle lands in its own `src-tauri/target` whatever the Cargo configuration says; remove `~/zerus-dev/.heavy.lock` once the build is done. Start its executable from Terminal, in `~/zerus-dev/webui/client`:
   ```sh
   ZERUS_CLIENT_SINGLE_INSTANCE=1 ZERUS_CLIENT_NOTIFICATIONS=1 ZERUS_CLIENT_WATCHDOG=1 \
     "src-tauri/target/release/bundle/macos/Zerus Next (dev).app/Contents/MacOS/zerus-client"
   ```
   Then check:
   - **The tile:** counts, amber attention and the stale dot, on a light and a dark menu bar; the tooltip lines.
   - **The menu:** computers; sessions with the marker, "(paused)" and "(stopped)"; "Open in" with Terminal.app and Clipboard; Quit.
   - **The window:** a primary click opens and raises it. Closing hides it and the Dock icon goes. A Dock click restores a minimised window.
   - **`--sessions` while minimised:** minimise the window, then start the executable again with `--sessions`. The window comes back in front and active, as Qt's `activateWindow` brings it (`TrayAgent.cpp:781-783`).
   - **Closing in native fullscreen:** put the window in fullscreen with its green button, then close it. It hides, and no empty Space is left behind in Mission Control.
   - **Session entries:** a running session opens in Terminal.app. Clipboard copies its command and shows "Zerus: copied to the clipboard".
   - **Second start:** the same executable started again with `--sessions` raises the first window and exits with "already running".
   - **Notifications:**
     - allow the one-time prompt for the dev app;
     - a session waiting for approval posts "Zerus — Needs attention";
     - answering withdraws it;
     - clicking a banner opens that session (a click while Zerus is not running is step 8's: started by a click, the dev app runs without the variables above);
     - nothing posts while the window is focused.
   - **Defaults:** `defaults read dev.zerus.next` lists only Cocoa and WebKit keys (`NS…`, `Apple…`, `WebKit…`). Report the list; it shows what Cocoa or WebKit writes into a client's domain.
   - **Watchdog:** Terminal shows `main-thread watchdog on (30 s)`. Open the menu-bar menu and leave it open for 60 s, then close it: the app keeps running and no `main thread unresponsive` line appears (the heartbeat runs in the common run-loop modes).
   - **Ignored tile test:** if `the_tile_draws_amber_attention_and_the_stale_dot` was ignored over SSH, run it here with `--ignored`.
   - **Menu-bar checks from the Task 4 review:**
     - **A click after a rebuild.** Hold the menu open while a session appears above the target entry, then click the target: exactly that session opens, never a neighbour or Quit.
     - **The Dock badge after a closed window.** Let attention arrive while the window is closed. Opening the window shows the Dock badge with the count, and the badge survives closing and reopening the window.
     - **A fleet that cannot start.** Start with `ZERUS_CLIENT_HGS=/nonexistent/hgs`: a right-click still shows Open Zerus and Quit, and Quit works.
     - **`&` in a name.** A session or computer name containing `&` shows a single `&` in the menu.
     - **Other menus.** After using the app menu, a webview context menu and the Open in submenu, tile updates resume once each has closed.
     - **A theme switch with the menu open.** Switch light and dark while the status item's menu is open: the tile redraws in the new colour once the menu closes.
   Quit the dev app from its menu.

   **Scroll measurement (zerus-5wmh.36), in the same window.** Two instances must never share the client's data folder, so start this only after the dev app has quit.

   Conditions:
   - AC power;
   - Low Power Mode off;
   - a nominal thermal state;
   - the window fully visible and left untouched.

   Each run ends by itself after about 5 minutes:
   ```sh
   B="src-tauri/target/release/bundle/macos/Zerus Next (dev).app/Contents/MacOS/zerus-client"
   mkdir -p ~/zerus-dev/perf-36
   ZERUS_CLIENT_PERF=auto ZERUS_CLIENT_PERF_LABEL=mac-switch-live \
     ZERUS_CLIENT_PERF_OUT=$HOME/zerus-dev/perf-36/live.json "$B"
   ZERUS_CLIENT_PERF=auto ZERUS_CLIENT_PERF_LABEL=mac-switch-replay \
     ZERUS_CLIENT_REPLAY="$PWD/fixtures/replay.ndjson" \
     ZERUS_CLIENT_PERF_OUT=$HOME/zerus-dev/perf-36/replay.json "$B"
   ```
   Over SSH, the same runs use `open -n -W --env …` as in "Activity pass in replay mode" in `client/bench/README.md`.
   - **The live run** measures `list-scroll` on the real catalog.
   - **The replay run** measures `activity-scroll` on the fixture's long session. It always scrolls at least 2000 px; see "Activity pass in replay mode" in `client/bench/README.md`.
   - Each report records whether its Activity was live or replay.
   - Put both reports on one page with `client/scripts/measure-report.py`.

   If `list-scroll` or `activity-scroll` fails the harness criteria, zerus-5wmh.36 joins the switch bar. Otherwise it stays after the switch (coordinator, 2026-10-28).
5. **Agree the moment with the owner.** Qt flushes its drafts when it quits, but the owner should not be typing.
6. **Switch** (Terminal). From the root of the clean clone, not the dev app's clone of step 4, with `ZERUS_DEV_BUILD_NUMBER`, `CLONE` and `SHA` still set, run only the GUI installer, not the CLI `install.sh`:
   ```sh
   cd "${CLONE:?}" && HGS_MAC_GUI=client bash tray/install.sh 2>&1 | tee ~/zerus-switch.log
   ```
   Expected lines:
   - quitting Zerus;
   - kept a copy of the Qt settings;
   - `Zerus <DEV_VERSION>-dev.N runs as pid …; … nonempty drafts kept, … native processes untouched; the replaced GUI is the rollback copy in …`;
   - possibly `Cocoa settings written since the switch: …`.
   Any `hgs-tray update failed` line stops the runbook. The updater has already restored or kept the previous GUI and printed where it and the draft snapshot are.
7. **Verify** (no display):
   ```sh
   ~/Applications/hgs-tray.app/Contents/MacOS/hgs-tray --build-info | python3 -c 'import json, os, sys; i = json.load(sys.stdin); print(i["devBuildNumber"], i["sourceCommit"] == os.environ["SHA"], i["sourceDirty"])'
   cat ~/.local/share/hgs/build-provenance.json
   launchctl print gui/$(id -u)/com.hgdev.hgs-tray | grep -E 'state =|program ='
   ls ~/.local/share/hgs/desktop-previous/
   find ~/Applications -maxdepth 1 -name '.hgs-tray-kept-*'
   lsof -n -a -c hgs-tray -U -F n | grep '/hgs-tray$'
   python3 -c 'import json, pathlib; m = json.loads((pathlib.Path.home() / "Library/Application Support/com.hgdev.hgs-tray/drafts/.imported-v1.json").read_text()); print(m["version"], m["report"].get("unfinished"))'
   python3 -c 'import json, pathlib; s = json.loads((pathlib.Path.home() / "Library/Application Support/com.hgdev.hgs-tray/settings.json").read_text()); print((s["seed"]["source"], sorted(s["seed"]["keys"])) if "seed" in s else "no seed record")'
   ```
   Expected:
   - `--build-info` prints `N True False`: Dev number N, the commit of step 1 (`$SHA`), a clean tree;
   - the agent is running `~/Applications/hgs-tray.app/Contents/MacOS/hgs-tray`;
   - the rollback folder holds `hgs-tray.app` (Qt) and `com.hgdev.hgs-tray.plist`, and no `.hgs-tray-kept-*` folder exists;
   - the socket is at the path from step 3;
   - the draft import marker, `<data dir>/drafts/.imported-v1.json` with the data directory `~/Library/Application Support/com.hgdev.hgs-tray`, prints `1 None`: version 1, with no `unfinished`. The marker is written only once the import finished, so a missing one (the command fails) means it did not: it runs again at the next start; report its `drafts import: …` line from `~/Library/Logs/hgs-tray.log`;
   - the settings seed record names its source and the keys it carried over: those Qt has stored (for example `workspace/theme` once the owner chose a theme), never `speech/microphone`, and Qt's automatic approvals only as `automaticApprovals/qtEnabledAtSwitch`. Report the list; step 8 checks one of these keys in Settings. An empty list means Qt had stored none of them, and step 8 then checks only automatic approvals and the microphone. `no seed record` means the seed failed and runs again at the next start: report its `settings seed:` line from `~/Library/Logs/hgs-tray.log`.
8. **Confirm** (**display**; the coordinator's job, done with the owner):
   - one menu-bar item;
   - notifications post without a new permission prompt (when step 2 matched);
   - a click on a banner Qt delivered before the switch opens its session;
   - a click while Zerus is not running: quit Zerus from its menu (launchd leaves a clean exit alone), then click one of its banners in Notification Center. Zerus starts and opens that session. If it starts but the session stays closed, the click that launched it was lost: the notification delegate is set during setup, and the fix is to set it in `applicationWillFinishLaunching` (Task 6); file a bead and tell webui. The instance the click started runs outside launchd, so quit it and run `launchctl kickstart gui/$(id -u)/com.hgdev.hgs-tray` to bring back the supervised one;
   - the owner's sessions, and the drafts in Qt's store, are where they were; Qt drafts and the dev app's drafts appear in the client (the import of step 7);
   - the settings seed: a key from step 7's list shows Qt's value in Settings (for example the theme in Settings > Appearance), automatic approvals are off, and Voice input asks for the microphone again;
   - after the Mac's next sleep and wake, `launchctl print gui/$(id -u)/com.hgdev.hgs-tray | grep 'pid ='` shows the same pid as before, and `~/Library/Logs/hgs-tray.log` has no `main thread unresponsive` line.
9. **Later Dev rollouts.** Run the same `bash tray/install.sh` with the new `N` and without `HGS_MAC_GUI`. The installed client is kept, and Qt stays the rollback copy: there is no second slot. A new client that does not start is replaced by the client it replaced, automatically. The numbered Dev guard refuses a run without `ZERUS_DEV_BUILD_NUMBER` or a local preview label.
10. **Rollback.** From the clean checkout, in Terminal:
    ```sh
    cd "${CLONE:?}" && python3 tray/update-installed.py --rollback ~/Applications/hgs-tray.app
    ```
    Qt comes back with its drafts, and the client moves into the rollback folder, so a second `--rollback` returns to the client. If the updater reports changed Qt settings, quit Zerus, run `defaults import com.hgdev.hgs-tray ~/.local/share/hgs/desktop-previous/com.hgdev.hgs-tray.plist`, then `launchctl kickstart gui/$(id -u)/com.hgdev.hgs-tray` (KeepAlive Crashed does not restart a clean quit). Drafts typed in the client after the switch stay in `~/Library/Application Support/com.hgdev.hgs-tray/drafts/`, where Qt does not see them. Use `HGS_MAC_GUI=qt` on later installs to stay on Qt, and file a bead for the cause.

## Coverage

| Requirement (bead) | Task |
| --- | --- |
| Status item with Qt's icon states and menu entries; primary click; no Dock icon while hidden (.44) | 1, 3, 4 |
| Same per-user lock and socket path as `SingleInstance`, `show`/`sessions`, second start forwards; the updater finds the socket and sends `sessions` (.44) | 2, 3, 7 (end-to-end on Linux) |
| Window activation (.44) | 3, 4 |
| Dock badge with the attention count (spec row 291, not in a bead) | 1, 4 |
| Login only through the tray LaunchAgent template with KeepAlive Crashed and ProcessType Interactive; only the new client starts at login (.44) | 9 (template and agent unchanged; the bundle replaces Qt at the same path) |
| Notifications under the app identity, default sound, one request per token, pending and delivered withdrawn, default action opens the session (.43) | 5, 6 |
| Unread reply identity kept, reply banners wait for background work, explicit attention first, Mute (.43) | 5 |
| Authorization as Qt requests it; the grant carries over with a matching requirement (.43) | 5, 6, runbook step 2 |
| Quit, flush, snapshot (private, no links) and verify the client's drafts root (.12.1) | 3, 7 |
| Info.plist identity, LaunchAgent template in `Resources/setup`, dev app keeps its identity, `check-macos-bundle` accepts the bundle (.12.2) | 9 |
| Signing continuity (Developer ID, same team, no cdhash), release signing with the hardened runtime and entitlements, requirement comparison (.12.2) | 9, runbook step 2 |
| `--build-info` shape with the Dev number and the local preview label; quit and SIGTERM flush (.12.2) | 2, 3 |
| Qt's stores read only; plist snapshot beside the rollback copy; Qt keys byte-for-byte; list of Cocoa keys (.12.2) | 6, 7, runbook step 4 |
| Client data directory decided; dev drafts reach the client through `zerus-5wmh.13.1`; settings at defaults (superseded: seeded once from Qt's, phase 3b ruling), read marks fresh (.12.2, coordinator ruling) | Decision 5, Tasks 2 and 9, runbook |
| Switch only through the installers, numbered builds only, one rollback copy (last Qt) and a rollback command, docs (.12.2, coordinator ruling) | 2, 7, 9, runbook |
| Numbered Dev guard on every platform with the local preview label as the escape hatch; `docs/reference.md` and the refusal say the same; events and menu-parity told before main (coordinator ruling) | 2, 7, runbook "Before the merge" |
| Main-thread watchdog with Qt's thresholds, sleep and dark-wake hold and abort for launchd or systemd (.12.3) | 8, runbook steps 4 and 8 |
| Update notices missing after the switch, follow-up `zerus-5wmh.12.4` (coordinator ruling) | Decisions, Task 9 docs, runbook |
