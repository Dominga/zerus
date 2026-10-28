# Tauri Client Phase 4: Composer, Attachments, Media, Terminal, Processes and Native UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client gets the session Terminal, the Processes tab, the full composer with Qt's guarded Send (block reasons, the cold-cache question, resume-then-send, compact-to-send, local failed messages, durable drafts), attachments, voice and round video, and DeepSeek's Native UI, with the Qt GUI's exact behaviour (Beads `zerus-5wmh.9`).

**Two parts (owner decision, 2026-10-28).** The new client replaces the Qt app on the owner's Mac as soon as it is workable for daily use. **Part A is that bar** for this phase: the Terminal, the composer and Send (block reasons, the cold cache, resume-then-send and compact-to-send, local messages, durable drafts), attachments (stage, send, fetch, preview) and Processes. New session (phase 5, `zerus-5wmh.10`) completes the bar. **Part B** follows the bar and is fully planned here: voice and round video, Native UI, and the external terminal, shell and folder entries. Within Part A the tasks follow the core: first the merged Terminal core, then in the order the core PRs land (#403, #404, #407, #408, #409, #410, #411).

**Architecture:**
- **Ownership (binding).** The pure Rust core modules belong to the events session and are listed with their public surfaces in `docs/superpowers/plans/2026-10-28-tauri-client-phase4-core-section.md` (branch `work-ba0ed9fd-phase4core`). This plan never re-plans them. It covers:
  - the `client/src-tauri` commands, lanes and drivers that expose them;
  - the SolidJS stores and views;
  - the replay and fake-`hgs` fixtures;
  - the smoke runs and the docs.
- **Three hubs in `client/src-tauri`**, each a managed state with injected spawn, clock, notify and `hgs` runners, so its tests need no Tauri and no real `hgs` (the pattern of `screens.rs`):
  - `terminal.rs`: one attach client per view, drained on a std thread into a `tauri::ipc::Channel`;
  - `processes.rs`: the followed session's processes topic, projected through `process_view`, with output reads and the sequential stop queue;
  - `composer/`: the open draft (A1 `Draft`, held only in Rust), the app's one S5 draft store (`Drafts`, shared with voice), the S1/S2 rules, the S3 continuations, the S4 local rows, the A3 attachment cache and its URI scheme.
- **One push path.** `next_batch` keeps returning the core's `Batch` as raw JSON, flattened into an envelope that adds the projected `composer` view, the `processes_view` and the `draft_keys`. Rust recomputes them when the followed Activity's details or a fact change, and once a minute for the cache chip. The webview never calls a rule per render.
- **Native UI** is a child webview at the tunnel URL from `hgs native-ui NAME --json`, origin-locked and incognito; without one, the system browser through `hgs native-ui NAME`.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2.12 with the `unstable` feature for child webviews), `tauri-plugin-dialog` 2, `tauri-plugin-opener` 2, `signal-hook` 0.3, `chrono` 0.4, `sha2` 0.10; SolidJS 1.9 + TypeScript + Vite, `@xterm/xterm` 6.0.0, `@xterm/addon-fit` 0.11.0, `@xterm/addon-webgl` 0.19.0, vitest; Python 3 stdlib for fixtures and fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`:
- process model (PTY, child webview), "Parsing never happens in the webview", "Batched delivery", "Actions";
- frontend lines 212-214 (media messages) and 220-224 (xterm.js on demand, renderers, PTY);
- the round 5 and 6 attachment and hint rules;
- platform rows 290-303 (Terminal PTY, voice and video recording, clipboard and file pickers, opening files, Native UI).

Phase table: `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`, row 4. Core surfaces: the core section above. The behavioural reference is the Qt GUI; every task names its lines (verified on main `43e09401`).

## Requests to the core owner

These surfaces are missing from the core section. This plan does not build them; the tasks that need them say so in their "Depends on" line and keep the dependent control disabled with the reason given there until the surface merges.

- **R1. The context usage chip.** Port `SessionUsage::ContextButton::render` (`tray/src/SessionUsage.h:16-60`) as `client/core/src/session_usage.rs`:
  - `pub fn context(usage: &Value, recorded: bool) -> ContextChip`, where `usage` is the inspection's `session_usage` object;
  - `ContextChip { visible: bool, text: String, tooltip: String, warning: bool, tone: cache::Tone }`, serde snake_case, with Qt's compact numbers ("72k", "1.2M"), the "≈" estimate mark and the tooltip lines.

  The composer's Context chip and S3's `start_question(session, context)` need its `text`. Until R1 merges, the chip stays hidden and `start_question` gets `""`, as Qt does when nothing is reported.
- **R2. Opening session files.** Port `tray/src/ExternalFile.cpp` as `client/core/src/external_file.rs`:
  - `runs_programs(path) -> bool`;
  - `mark_downloaded(path) -> bool`, the macOS quarantine attribute and the Windows Zone.Identifier stream;
  - `download_marker(path) -> Vec<u8>`;
  - `revealed_notice(path)` and `reveal_failed_notice(path)`;
  - `enum Origin { ThisComputer, OtherMachine }`.

  The open and reveal calls themselves run in src-tauri through `tauri-plugin-opener` (Task 9).
- **R3 and R4: delivered.** The voice composer (M1b) and its commands (M1c) are on the events session's branches: M1c `work-ba0ed9fd-media` (head `1b14e68b`) and M3 `work-ba0ed9fd-video` (head `100e86bc`), in `client/src-tauri/src/voice.rs` and `client/core/src/voice_driver.rs`. Task 13 binds to that surface:
  - `voice_open(key, events: Channel<Out>, frames: Channel<InvokeResponseBody>)`, at most 32 open drivers; a replay refuses with "Voice in replay mode needs ZERUS_CLIENT_HGS.";
  - `voice_input(key, input)`, `input` tagged `kind`: `update {target, input_visible, preferences}`, `press`, `stop {forget_send}`, `send`, `discard`, `refresh`, `choose_provider {index}`, `confirm {ticket, confirmed}`, `inserted {token, inserted: refused | saved | not_saved}`, `playing {playing}`;
  - `voice_close(key)`, `voice_microphones()`, `voice_video() -> {available, reason}`;
  - `Out`, tagged `kind`: `shown`, `level`, `ask`, `message`, `insert`, `permission_recovery`, `remember_profile`, `pause_messages`, `stop_playback`, and `preview {mirrored}`, sent once per recording before its first frame. Preview frames are raw JPEG bodies on the `frames` channel.

  The voice driver shares the app's one draft store (`Drafts`, Task 3a), and its `close_all` runs first in the one exit hook.
- **R5. The terminal launcher.** Port `tray/src/TerminalBackend.h`, `WindowsTerminalBackend.cpp`, the macOS and Linux backends and `TrayAgent::openSessionTerminal` (`tray/src/TrayAgent.cpp:725-760`, spec row 294) as `client/core/src/launch.rs`: `open_program(program, args) -> Result<Option<String /*advisory*/>, String>` and `open_shell(directory)`. Task 15's Open terminal, Open shell in session folder and Open externally entries need it (Part B).

## Global Constraints

- **Qt wins.** Where this plan and the Qt GUI disagree, Qt is right; fix the plan in the same PR. Add nothing Qt does not have.
- **Ownership:** core modules are consumed exactly as their surfaces stand on main (or on the named PR). If a name differs at execution time, follow main and fix the task's code, never the core module.
- "Actions keep today's guarded CLI calls and their rules: request identities, expected run and conversation, destructive confirmations with the safe choice focused, and **unknown outcomes are never replayed automatically**." (spec, Actions)
- **Never replay an unknown delivery** (AGENTS.md). In the send driver (Task 4b) and the continuations (Task 5):
  - the durable "sending" (or "waiting") save comes before any dispatch, and only its callback dispatches;
  - an `Uncertain` outcome, or an `Err` after the call may have started, never resends by itself: the draft stays, marked uncertain, and asks for Allow retry;
  - every path that could send re-checks the composer first: a question's answer, a Retry (S4's `retryable` excludes uncertain rows), a continuation's step, a reload and a restart;
  - one request id per delivery, made right before its one dispatch and reused by nothing else; a Retry of a definite failure is a new submission with a new id.
- **One draft store** (agreed with the events session): the app opens one `Arc<DraftStore>` in `setup` and manages it as `Drafts`; the composers and voice take it, so each store has one writer. **One exit hook**: `fn exit(app)` on `RunEvent::Exit` closes voice, flushes the organization, closes the composers and then flushes the store; whoever lands second adds lines to it, never a second hook. Termination signals run the same `exit`.
- **Qt's stores are read, never written** (owner decision, 2026-10-28). Qt's drafts root is read only through `drafts::read_index`, `drafts::read_draft` and `DraftStore::recover` (which writes the client's own store), and left out entirely if it overlaps the client's root. The client's attachment cache is Tauri's `app_cache_dir`, not Qt's `hgdev/hgs-tray` cache. Task 7 proves Qt's folder byte-for-byte unchanged.
- **Identity:** nothing in Part A assumes the dev identifier `dev.zerus.next` or the dev app's name; the switch build uses `com.hgdev.hgs-tray`. Paths come from Tauri's `app_data_dir` and `app_cache_dir`, which follow the identifier, and UI text names Qt's app as "the Qt app", never by a name the client may take over.
- **No bytes as JSON over the IPC.** Attachments, previews, Terminal output and media reach the webview only as raw Channel messages, raw command bodies or `zerus-file` responses; pasted images and Terminal input go to Rust as raw bodies. No serde payload this plan adds carries base64 or a byte array. One exception is not this plan's: hgs's `voice.video.poster_base64` rides inside the core's Activity events, so Task 13 draws posters through the scheme and a request to the core owner (review report) would drop it from the batch.
- Message bytes go only to the standard input of `hgs [@host] send NAME --json`; never argv, a shell command or the clipboard. `Action::Send` is never deserialized from webview JSON: `send_message` builds it in Rust from the held draft and calls `actions::dispatch_as`.
- `attachments::Draft` is not serde; only `AttachmentInfo`-derived views cross the IPC. Pasted images reach `attachments::pasted_image` as a raw IPC body, never JSON.
- **Bounds:**
  - PTY reads at most 256 KiB per batch; one terminal message at most 1 MiB; at most 2 unacknowledged messages per view;
  - queued terminal input at most 4 MiB;
  - message text at most 64 KiB (UTF-8);
  - at most 8 attachments, each nonempty and at most 10 MiB, 20 MiB per message;
  - a pasted image below 32 megapixels;
  - an attachment read at most `10 MiB * 4/3 + 8192` bytes;
  - the attachment cache 512 MiB, least recently used first (`attachments::CACHE_LIMIT`);
  - local message rows 50 per session and 512 sessions;
  - Saved drafts pages at most 200;
  - the composer hub keeps at most 200 events of the followed session, and at most 64 composers and 128 MiB of their attachments in memory;
  - process output as `hgs` bounds it.
- All `hgs` calls, file reads and draft writes run off the UI thread (Tauri's blocking pool or the hubs' own threads).
- The composer's block reason is computed in Rust whenever the followed Activity's details or a fact change and is pushed with the Activity update; the cache chip is recomputed once a minute; `local_time` is formatted in src-tauri.
- Terminal output goes as `InvokeResponseBody::Raw`, the finish as a small JSON message on the same Channel; the `AttachEvent` receiver is drained on a std thread; an attach stops on close or when `terminal::target_changed`.
- **Lanes** (`client/src-tauri/src/actions.rs`):
  - `Lane::Send` is keyed by (computer, name, agent_id);
  - `Lane::Process` is for `StopProcess`;
  - `ClearContext`, `CompactContext` and `StartFresh` run on `Lane::Write`.
- The attachment URI scheme serves only canonical regular files under `cache_root` (`attachments::cached_at`) or bytes of an open draft, and only to the main webview.
- Every app command refuses a webview other than `main` (the Native UI child webview gets no IPC).
- The new UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); `scripts/check-ui-separators.py` enforces it, `.rs` files under `client/` included. Qt's " · " joins become layout, commas or " / ".
- No obvious keyboard hints in the UI (owner rule, round 6): no "(Delete)", "(Space / Esc)", "Enter to send" or shortcut text in tooltips, placeholders or buttons.
- Histories, journals and lists use bounded pages (AGENTS.md); no per-session cache grows without a bound. The draft marks come from the 200 newest drafts plus the 64 composers held; Saved drafts reads pages of at most 200; the attachment cache is pruned to 512 MiB with pins only for what shows now; the Terminal keeps 1 MiB per view; the Processes view holds the followed session only.
- **Each task adds only what it uses.** Clippy runs with `-D warnings`, and an item unused until a later task fails it (`dead_code`). When a later task needs a field, method or variant, that task adds it; the Interfaces blocks say which task brings what.
- A task that adds a Rust dependency updates `client/src-tauri/Cargo.lock` with one `cargo +1.99.1 check --manifest-path src-tauri/Cargo.toml` without `--locked` and commits the lock; every other command keeps `--locked`.
- Rust toolchain 1.99.1 (`cargo +1.99.1`, `rust-version = "1.99"`); `client/core` stays pure Rust without Tauri.
- Never run the real `hgs` against live sessions in tests; tests use fake `hgs` scripts. Fixtures are synthetic: reserved example domains, `/home/example/...`, `claude/sample-project/review`.
- Headless GUI runs only, with `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...` (`client/README.md`, "Headless runs").
- Repository text in English.
- **Shared files** (other plans touch them): changes to `client/src/types.ts`, `bridge.ts`, `store/actions.ts`, `store/catalog.ts`, `ui/sessions/SessionList.tsx`, `client/src-tauri/src/actions.rs`, `commands.rs` and `lib.rs` are additive. Rebase onto whatever lands first.
- Each task is one PR, merged after the "CI gate" check is SUCCESS. The owner authorized commits, pushes, PRs and merging green, reviewed work to main (2026-10-13).

## Review Focus

1. **An uncertain send.** A send that times out, ends abnormally or returns a receipt that does not echo the request:
   - the draft stays, marked uncertain;
   - the local row reads "Delivery not confirmed" with Check Terminal and Delete, but no Retry;
   - Send stays disabled until Allow retry;
   - nothing is sent again by itself, not after a webview reload, an app restart, a reconnect or an older question's answer.

   Tests: Task 4b (`an_uncertain_send_is_never_retried`, `a_stale_question_never_resends_an_uncertain_draft`, `a_send_whose_save_is_queued_at_exit_never_starts`), Task 7 (the row's buttons).
2. **A draft surviving a crash.**
   - Typed text and added attachments reach disk within the coalescing window.
   - A draft saved while sending loads as uncertain with "Delivery was interrupted. Check Activity or Terminal before sending again.".
   - Quit and SIGTERM run the one exit hook: the hub closes (nothing new starts), then the shared store writes everything queued.
   - A rename never overwrites a draft already at the new name; both stay.

   Tests: Task 3a (`a_sending_draft_reloads_uncertain`, `quit_flushes_drafts`, `a_rename_onto_an_existing_draft_keeps_both`).
3. **A Terminal resize storm.** 200 resizes within 100 ms, as in a window drag:
   - at most one resize per 100 ms leaves the webview, plus the trailing one;
   - an unchanged size never reaches the PTY;
   - the attach client sees the final size, and no output is lost or doubled.

   Tests: Task 1 (`identical_sizes_never_reach_the_pty`, `a_view_shown_again_redraws_without_doubling`), Task 2 ("resizes are throttled to one per 100 ms plus the trailing one").
4. **Huge pastes.**
   - A Terminal paste over 4 MiB queues nothing and shows "Terminal input queue is full; wait before pasting more text.".
   - A composer text over 64 KiB shows "Message exceeds 64 KiB." and disables Send.
   - A pasted image of 32 megapixels or more shows "Image is too large. Use an image below 32 megapixels.".
   - Only one paste or attach runs at a time per composer, and the editor is read-only while it does.

   Tests: Task 1 (`a_paste_over_the_queue_bound_queues_nothing`), Task 3a (`a_huge_pasted_image_is_refused`, `over_64_kib_disables_send`) and Task 6 ("over 64 KiB disables Send at once", "one attach at a time").
5. **Attachment cache eviction while a preview is open.** `prune` never removes a pinned copy: an open preview, a playing message, or the cached files of a failed local message that Retry needs. A preview whose file disappeared fetches once more and otherwise shows Qt's error. Tests: Task 9 (`prune_keeps_open_previews_and_retry_files`, and "a vanished preview fetches again once, then shows the error").
6. **Qt's drafts.** Saved drafts, its preview, Recover and Restore read Qt's store and never write, rename or delete there, and a Qt root that overlaps the client's own is left out. Tests: Task 7 (`the_qt_store_is_never_written`, `the_qt_store_is_never_the_clients_own`).

## Decisions

- **Rulings on the plan review (2026-10-28).**
  - Voice-event posters: `hgs` puts `voice.video.poster_base64` into the voice events it publishes, so the Activity batch carries one small JPEG per video message as JSON. Accepted until the core strips it from batches and offers a poster lookup (request R6, a Part B follow-up). Task 13 never decodes it in the webview; posters are served through `zerus-file`.
  - On macOS under `com.hgdev.hgs-tray`, Cocoa writes its own keys (for example the file picker's last folder) into `com.hgdev.hgs-tray.plist`, which is Qt's settings file. Qt's own dialogs write the same keys, and no Qt key is changed. Accepted. The switch slice (zerus-5wmh.12.2) checks that Qt's keys stay byte-for-byte unchanged.
  - The spec's line "a distinct bundle identifier from the Qt app" predates the identity decision of 2026-10-28. It is corrected in the switch slice's PR, not here.
  - The client's own data (read marks, cache, local settings) moving with the identifier is decided by the Mac switch plan, not by this plan.
  - Part A lands as five PRs: the Terminal (Tasks 1-2), the composer and send (3a-7), attachments (8-9), Processes (10-11), then the smoke run and docs (12).

Rulings on the open questions; the tasks follow them.

- **One draft store, one exit hook (agreed with the events session, 2026-10-28).** M1c opens the app's one `Arc<DraftStore>` in `setup` and manages it as `pub struct Drafts(pub Arc<DraftStore>)` in `lib.rs`, at `zerus_client_core::drafts::dir(replay)` (`drafts`, or `drafts-replay` in a replay). `Composers::new` takes that store and never opens its own, so each store has one writer. M1c turns `run()` into `.build(...).expect(...).run(...)` with one `fn exit(app)` on `RunEvent::Exit`: `voices.close_all()`, then the organization's `flush()` (the org branch), then `composers.flush()`, then `Drafts.0.flush()`, each through `try_state`. Task 3a works whichever lands first: with M1c on main it uses M1c's `Drafts` and `exit`; without, it adds exactly the same struct, folder names and hook, and M1c rebases onto them. Whoever lands second adds lines, never a second store or hook.
- **Qt's stores are read, never written (owner decision, 2026-10-28).** The client takes over Qt's identity on the Mac (`com.hgdev.hgs-tray`), and Qt's drafts are kept as they are. Qt's drafts root is read through `read_index`, `read_draft` and `DraftStore::recover` only; recovery copies into the client's store; a Qt root that is, holds or sits inside the client's root is left out. The one-time import of all Qt drafts at the switch (`zerus-5wmh.13`, core section S5) is a copy too.
- **Saved drafts lists two sources.** The client's own store comes first, under "On this computer"; Qt's read-only store follows under "From the previous Zerus app" (it stays right after the client takes over the Zerus name; the owner knows the Qt GUI as Zerus). Each is paged by its own cursor ("Show more"), since the stores cannot share one cursor.
  - A Qt draft is copied into the client's store with `DraftStore::recover` before Restore or Go to session. An existing nonempty draft there is never overwritten (S5's `ALREADY_EXISTS` text).
  - Qt's dialog texts, buttons and the "Replace current draft?" confirmation carry over.
- **The exit closes the hub before the store flushes.** `Composers::flush` stops new edits, attaches and sends and waits up to 2 s for attach reads under way; `Drafts.0.flush()` then writes everything queued. A send whose "sending" save is still queued never starts and reloads as uncertain.
- **Draft attachment URLs carry no digest.** `AttachmentView.path` is `draft/{handle}/{list}/{index}`, where `list` changes with every change of the attachment list; the scheme answers it from memory without hashing, and an old URL answers 404. S3's draft marks hash an attachment list once (Task 5), not per check.
- **Renames lock until the store answers** (Qt's `renameDraft`). A refused rename (`ALREADY_EXISTS`) keeps the moved draft under its old key and loads the draft already at the new key, so neither is overwritten.
- **Tasks 3 and 4 are split** for review: 3a (the hub, drafts and attachments), 3b (the `zerus-file` scheme and the cache), 4a (the session's block reasons, cache chip and inspect fallback), 4b (the delivery and local rows).
- **Processes are off by default, as in Qt.** This phase adds the Settings > Processes section the client's Settings screen lacks: Qt's five controls and hint (`tray/src/SettingsPage.h:109-120`), stored under the QSettings keys `processes/enabled`, `processes/activeSeconds`, `processes/backgroundSeconds`, `processes/unknownSeconds` and `processes/loadingSeconds`. This follows the screen's pattern ("Settings, as far as the client has them"); phase 3b keeps the section when it builds the full screen.
- **Composer parts outside this phase.** Task 12 files a follow-up child of `zerus-5wmh` for each, and each keeps its Qt place, disabled with "Arrives with …":
  - the model settings popup and the OpenCode model picker ("model settings");
  - file references ("file references");
  - Send and push now and the native queue ("the message queue");
  - suggestions;
  - the subagent composer (the subagent view stays read only);
  - question answers ("question answers");
  - the Usage limit and Recovery chips ("account usage" and "automatic recovery").
- **The push envelope.** `next_batch` serializes `{...batch, composer?, processes_view?, draft_keys?}`. Only the open composer and the followed processes are projected; `draft_keys` lists the sessions with a nonempty draft among the 200 newest drafts plus the composers in memory, so the cards' draft marks stay bounded.
- **Editing protocol.**
  - The webview owns the text while the user types and flushes `composer_edit(handle, rev, text, position, anchor)` 250 ms after the last keystroke, on blur and before any attach, paste, remove or send. Rust applies only a higher `rev`.
  - Rust-originated text changes bump `text_epoch`; the webview takes `text` when the epoch grows: a load, an attach or remove, a delivery's end, a restore.
  - An attach or paste locks the editor read-only until it returns.
- **Pasted images.** PNG clipboard data goes as is. Another image type is drawn to a canvas and encoded as PNG in the webview only after `createImageBitmap` reports fewer than 32 megapixels; otherwise Qt's text appears and nothing is sent.
- **Terminal lifecycle (Qt `TerminalView`, `SessionsWindow.cpp:1501-1505, 1637, 3456-3459, 4011-4026`).**
  - Showing the Terminal tab connects.
  - Another selection retargets: a changed target disconnects and clears the screen, then connects if the tab shows.
  - Leaving the Sessions page, losing availability or closing the window disconnects.
  - Hiding the tab keeps the attach running: Rust keeps the last 1 MiB it delivered, and a view mounted again redraws from it exactly (no output twice).
  - `restored_pending` is `false` until the menu-parity plan's Piece E adds the restore follow-up; then Task 2's input reads it.
- **Terminal file drops (Qt `SessionsWindow.cpp:2618-2659, 1602-1620`).** A local session gets quoted paths pasted (`terminal::paste_paths`). A peer gets the files staged with `Action::StageFiles` and then the staged paths pasted, with Qt's checks and texts.
- **Media playback.** Voice and video play from A3's cached file through the `zerus-file` URI scheme, which answers HTTP Range requests (206), since WebKit and WebView2 seek media only with them. The video poster is `voice.video.poster_base64` shown as a `data:` image, as `VoicePlayerImage::videoPoster` does; the webview decodes no media to draw it.
- **Native UI.**
  - Tauri's `unstable` feature enables `Window::add_child`. The child webview is incognito, its main frame stays on the tunnel origin, and http(s) links open in the system browser.
  - App overlays (menus, dialogs) hide it while open, since a native child webview draws above the page.
  - Every app command refuses a webview other than `main`.
  - Without a child webview (add_child failed), the system browser opens through `hgs native-ui NAME`, Qt's fallback when its plugin is missing.
- **Restore follow-up.** Saved drafts restores Qt's second sentence of the menu-parity Piece E notice: "The restored conversation could not be verified. Your archived draft remains in Saved drafts." (owner ruling, `zerus-5wmh.9` comment of 2026-10-28).
- **Composer Stop and Mark as read.** The composer action row's Stop reuses phase 2's `actionEntries(...).stop`. The Mark as read chip uses the menu-parity Piece B's `markReadChip(session)` and runs its `runMarkRead(row)` on click (`zerus-5wmh.16`, PR #406, `ui/sessions/mark-entries.ts`).
- **Keyboard hints.** The mockup's "Remove (Delete)" and "Stop recording (Space / Esc)" lose their parenthesized keys (round 6 rule); the keys still work.

---

## File Structure

```
client/src-tauri/Cargo.toml              + tauri "unstable", tauri-plugin-dialog, tauri-plugin-opener, signal-hook, chrono, sha2
client/src-tauri/src/lib.rs              modules, Drafts (the one store), managed hubs, URI scheme, window events, the one exit hook and its signals, IPC guard
client/src-tauri/src/actions.rs          dispatch_as (caller-given request id), write_kind (facts for the send rules)
client/src-tauri/src/commands.rs         next_batch envelope; Live::follow_processes and unfollow_processes
client/src-tauri/src/pump.rs             Pump::poke, Pump::row, Pump::row_named, Pump::online
client/src-tauri/src/replay.rs           processes states per session, served on follow
client/src-tauri/src/terminal.rs         Terminals: connect, write, resize, ack, disconnect, retarget, status
client/src-tauri/src/processes.rs        ProcessHub: follow, view projection, output reads, stop queue
client/src-tauri/src/composer/mod.rs     Composers: open, edit, attach, paste, remove, undo, rename, saved drafts, recover, view, draft keys, the exit's close
client/src-tauri/src/composer/send.rs    the session's view (block reasons, cache chip), send_message, answers, delivery, local rows, retry, delete, inspect, continuations, clear
client/src-tauri/src/composer/files.rs   zerus-file scheme (Range), the cache, pins, prune lane, attachment_fetch, open and reveal
client/src-tauri/src/native_ui.rs        NativeUi: child webview per launch key, bounds, overlays, browser fallback
client/src-tauri/tests/support/fake_hgs_phase4.py   fake hgs for the hubs' tests and the phase 4 smoke
client/scripts/make-fixture.py           processes topic, cache hints, attachments and voice messages in the replay
client/fixtures/replay*.ndjson, replay-batches.json  regenerated
client/package.json                      + @xterm/xterm, @xterm/addon-fit, @xterm/addon-webgl
client/src/types.ts                      Terminal*, Processes*, Composer*, LocalMessage, SavedDraft*, Batch envelope fields
client/src/bridge.ts                     wrappers for every new command; Batch normalization keeps the envelope
client/src/store/terminal.ts             the Terminal view's state machine (+ .test.ts)
client/src/store/processes.ts            the Processes view state and selection (+ .test.ts)
client/src/store/composer.ts             the open composer: text sync, attach, paste, send replies, dialogs (+ .test.ts)
client/src/store/media.ts                one player at a time, fetch and pin, progress (+ .test.ts)
client/src/store/voice.ts                the voice driver's channels (voice_open), inputs and transcript inserts (+ .test.ts)
client/src/store/settings.ts             processes/* keys
client/src/store/session.ts              routes the envelope to the stores
client/src/store/ui.ts                   detailTab, overlay count
client/src/ui/sessions/Terminal.tsx      xterm.js tab (lazy), statuses, buttons, drops
client/src/ui/sessions/Processes.tsx     the Processes tab
client/src/ui/sessions/Composer.tsx      editor, toolbar chips, action row, height grip
client/src/ui/sessions/Attachments.tsx   the composer's attachment list (mockup's AttachmentsChip)
client/src/ui/sessions/ComposerDialogs.tsx   cold cache, start, clear, replace draft
client/src/ui/sessions/SavedDrafts.tsx   the Saved drafts dialog
client/src/ui/sessions/Recorder.tsx      voice and video recorder controls and preview
client/src/ui/sessions/NativeUi.tsx      the Native UI tab and its placeholder rectangle
client/src/ui/sessions/Activity.tsx      local rows, attachment chips, real media playback
client/src/ui/sessions/transcript.ts     local rows in the transcript
client/src/ui/sessions/SessionDetail.tsx tabs (Activity, Terminal, Native UI, Processes), composer column
client/src/ui/components/SettingsScreen.tsx  Processes section
client/src-tauri/tauri.conf.json         CSP for the zerus-file scheme (img-src, media-src, connect-src)
client/README.md                         Terminal, Processes, Composer and Send, Attachments, Media, Native UI, Phase 4 smoke run
```

## Order and dependencies

**Part A: the bar for replacing the Qt app**

| Task | Piece | Depends on (Beads, PR) |
|---|---|---|
| 1 | Terminal commands | T1 `zerus-5wmh.9.1` (#405) and T2 `zerus-5wmh.9.2` (#401), merged |
| 2 | Terminal tab | Task 1 |
| 3a | Composer hub: drafts and attachments, the one draft store and exit hook | A1 `zerus-5wmh.9.3` (#403), S5 `zerus-5wmh.9.11` (#409); M1c's `Drafts` and `exit` when on main |
| 3b | The `zerus-file` scheme and the attachment cache | Task 3a, the cache bound `zerus-5wmh.9.17` (branch `work-ba0ed9fd-cachelru`) |
| 4a | The session's block reasons, cache chip and inspect fallback | S1 `zerus-5wmh.9.7` (#407) and S2 `zerus-5wmh.9.8` (#408), both on main; A2 `zerus-5wmh.9.4` (#404); Tasks 3a-3b |
| 4b | The delivery: guarded send, cold cache question, local rows | A2 #404, S4 `zerus-5wmh.9.10` (branch `work-ba0ed9fd-localmsgs`), `ViewRow.created` from `zerus-5wmh.16` (#406), Task 4a |
| 5 | Continuations and Clear context | S3 `zerus-5wmh.9.9` (branch `work-ba0ed9fd-continuation`), C1 `zerus-5wmh.9.18` (branch `work-ba0ed9fd-context`), Task 4b |
| 6 | Composer view | Tasks 3a-5; the Mark as read chip also #406; the Context chip R1 |
| 7 | Local rows in Activity and Saved drafts | Tasks 4b and 6 |
| 8 | Dropped files: composer attach, Terminal paste and stage | A2 #404 (StageFiles), #406 (`Session.created`), Tasks 2, 3a and 6 |
| 9 | Activity attachments: fetch, preview, open | A3 `zerus-5wmh.9.5` (#410), `zerus-5wmh.9.17`, Tasks 3b and 4b; Open also R2 |
| 10 | Processes hub | P1 `zerus-5wmh.9.6` (#411, stacked on A2 #404) |
| 11 | Processes tab and Settings > Processes | Task 10 |
| 12 | Fixtures, the Part A smoke run, docs and follow-ups | Tasks 1-11 (3a, 3b, 4a and 4b included) |

**Part B: after the bar**

| Task | Piece | Depends on (Beads, PR) |
|---|---|---|
| 13 | Voice and round video: playback and recording | Task 9's scheme and fetch; M1 `zerus-5wmh.9.12` (#412 is M1a; M1b and M1c on `work-ba0ed9fd-media`, R3 and R4 delivered); round video M2-M4 (`zerus-5wmh.9.13`, `.9.14`, `.9.15`; M3 on `work-ba0ed9fd-video`), shown only while `voice_video().available` |
| 14 | Native UI | none in the core |
| 15 | Open terminal, shell and folder | R5 (Open terminal, shell, externally); Open folder none |

Until Part B lands, the Part A composer keeps the voice slot empty, a DeepSeek session's Native UI tab is absent (Inspect routes to a refresh), and the external entries keep phase 1's disabled state.

Windows: `terminal::Attach::start` reports "Cannot open terminal: Terminal is not available on Windows in this build yet." until T1b (`zerus-5wmh.9.16`); Task 2 shows that text as the status.

---

# Part A: the bar for replacing the Qt app

### Task 1: Terminal attach commands (`client/src-tauri/src/terminal.rs`)

**Depends on:** T1 `zerus-5wmh.9.1` (#405) and T2 `zerus-5wmh.9.2` (#401), both on main.

**Qt reference:**
- `tray/src/TerminalView.cpp:73-160`: `setSession` (the reconnect rule, a kept run), `connectSession`, `disconnectSession` and the statuses.
- `tray/src/PtyProcess.cpp`: write errors become the status.
- `tray/src/SessionsWindow.cpp:5869`: `hideEvent` disconnects.

**Files:**
- Create: `client/src-tauri/src/terminal.rs`
- Modify: `client/src-tauri/src/actions.rs:138` (`REPLAY_NEEDS_HGS` becomes `pub(crate)`)
- Modify: `client/src-tauri/src/lib.rs` (module, managed state, commands, window event)
- Test: unit tests in `terminal.rs`, running `client/core/tests/support/fake_attach.py`

**Interfaces:**
- Consumes (`zerus_client_core::terminal`, on main):
  - `Attach::start(&Target, cols, rows) -> Result<(Attach, Receiver<AttachEvent>), String>`, `Attach::write(&[u8]) -> Result<(), String>`, `resize(cols, rows)`, `stop(&mut self)`, `process_id()`;
  - `AttachEvent::{Output{data}, Finished{code}}`;
  - `Target {hgs, host, name, run_id}`, `TerminalFacts`;
  - `target_changed(&Target, &Target) -> bool`, `kept_run(&Target, &Target) -> String`;
  - `available(&TerminalFacts, &Value, state, online, restored_pending) -> Result<(), String>`, `supported(&TerminalFacts, &Value) -> bool`;
  - `status::{IDLE, connecting, connected, disconnected, finished}`.
- Produces (Task 2 and Task 8 bind to these):

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalTarget { pub computer: String, pub name: String, pub run_id: String, pub machine: String }
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Connected { pub generation: u64, pub status: String, pub reused: bool }
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TerminalStatus { pub supported: bool, pub available: bool, pub reason: String, pub idle: String, pub connecting: String, pub disconnected: String }
pub trait Sink: Send + Sync { fn output(&self, bytes: Vec<u8>); fn message(&self, json: String); }
#[derive(Clone)] pub struct Terminals(/* Arc */);
impl Terminals {
    pub fn new(hgs: PathBuf, allowed: bool) -> Self;
    pub fn connect(&self, view: &str, target: TerminalTarget, cols: u16, rows: u16, sink: Arc<dyn Sink>) -> Result<Connected, String>;
    pub fn write(&self, view: &str, generation: u64, bytes: &[u8]) -> Result<(), String>;
    pub fn resize(&self, view: &str, generation: u64, cols: u16, rows: u16);
    pub fn ack(&self, view: &str, generation: u64);
    pub fn disconnect(&self, view: &str) -> Option<String>;
    pub fn retarget(&self, view: &str, target: &TerminalTarget) -> bool;
    pub fn stop_all(&self);
    // Task 8 adds `paste` and `generation` with their first use (clippy denies unused code).
}
// Commands:
// terminal_connect(view, target, cols, rows, on_event: Channel) -> Result<Connected, String>
// terminal_write(raw body; headers x-zerus-view, x-zerus-generation) -> Result<(), String>
// terminal_resize(view, generation, cols, rows), terminal_ack(view, generation)
// terminal_disconnect(view) -> Option<String>, terminal_retarget(view, target) -> bool
// terminal_status(input: StatusInput) -> TerminalStatus
// Channel messages: raw bytes (output), or JSON {"kind":"finished","code":i32,"status":String}
```

- [ ] **Step 1: Write the failing tests** at the end of `client/src-tauri/src/terminal.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;

    /// What a view received; `ack` mirrors the webview acknowledging each output.
    #[derive(Default)]
    struct Collect {
        seen: Mutex<(Vec<Vec<u8>>, Vec<Value>)>,
    }

    impl Sink for Collect {
        fn output(&self, bytes: Vec<u8>) {
            lock(&self.seen).0.push(bytes);
        }
        fn message(&self, json: String) {
            lock(&self.seen)
                .1
                .push(serde_json::from_str(&json).unwrap());
        }
    }

    impl Collect {
        fn text(&self) -> String {
            String::from_utf8_lossy(&lock(&self.seen).0.concat()).into_owned()
        }
        fn outputs(&self) -> usize {
            lock(&self.seen).0.len()
        }
        fn messages(&self) -> Vec<Value> {
            lock(&self.seen).1.clone()
        }
    }

    fn terminals() -> Terminals {
        Terminals::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/support/fake_attach.py"),
            true,
        )
    }

    fn target(run: &str) -> TerminalTarget {
        TerminalTarget {
            computer: "mac".into(),
            name: "claude/sample-project/review".into(),
            run_id: run.into(),
            machine: "studio".into(),
        }
    }

    /// Waits up to ten seconds for `done`, acknowledging every output as it arrives.
    fn until(t: &Terminals, generation: u64, sink: &Collect, done: impl Fn(&Collect) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut acked = 0;
        while !done(sink) {
            assert!(Instant::now() < deadline, "timed out: {:?}", sink.text());
            for _ in acked..sink.outputs() {
                t.ack("main", generation);
            }
            acked = sink.outputs();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Waits for the fake's hello line: input written before it is flushed
    /// when the fake switches the terminal to raw mode.
    fn hello(t: &Terminals, generation: u64, sink: &Collect) {
        until(t, generation, sink, |s| s.text().contains("\"COLORTERM\""));
        until(t, generation, sink, |s| s.text().ends_with("\r\n"));
    }

    #[test]
    fn connect_runs_qts_attach_call() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 90, 30, sink.clone())
            .unwrap();
        assert_eq!(c.status, "Connected: claude/sample-project/review");
        assert!(!c.reused);
        until(&t, c.generation, &sink, |s| s.text().contains("\r\n"));
        let hello: Value = serde_json::from_str(sink.text().split("\r\n").next().unwrap()).unwrap();
        assert_eq!(
            hello["argv"],
            json!([
                "@mac",
                "a",
                "claude/sample-project/review",
                "--existing",
                "--run-id",
                "run-1"
            ])
        );
        assert_eq!(
            (hello["cols"].as_u64(), hello["rows"].as_u64()),
            (Some(90), Some(30))
        );
        assert_eq!(hello["env"]["HGS_TAB"], "0");
        assert_eq!(
            t.disconnect("main").as_deref(),
            Some("Disconnected — the session continues on studio")
        );
    }

    #[test]
    fn a_view_shown_again_redraws_without_doubling() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let first = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, first.clone())
            .unwrap();
        hello(&t, c.generation, &first);
        t.write("main", c.generation, b"echo one\r").unwrap();
        until(&t, c.generation, &first, |s| s.text().contains("echo one"));
        let second = Arc::new(Collect::default());
        let again = t
            .connect("main", target("run-1"), 80, 24, second.clone())
            .unwrap();
        assert!(again.reused);
        assert_eq!(again.generation, c.generation);
        assert_eq!(
            second.text(),
            first.text(),
            "the redraw is exactly what was delivered"
        );
        let before = first.outputs();
        t.write("main", c.generation, b"echo two\r").unwrap();
        until(&t, c.generation, &second, |s| s.text().contains("echo two"));
        assert_eq!(second.text().matches("echo two").count(), 1);
        assert_eq!(first.outputs(), before, "the hidden view gets nothing more");
        t.disconnect("main");
    }

    #[test]
    fn a_changed_target_disconnects_and_a_rename_does_not() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        assert!(
            t.retarget("main", &target("run-1")),
            "a first target is new"
        );
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        until(&t, c.generation, &sink, |s| s.text().contains("\r\n"));
        let mut renamed = target("run-1");
        renamed.name = "claude/sample-project/review-2".into();
        assert!(
            !t.retarget("main", &renamed),
            "a rename of the same run keeps the attach"
        );
        let mut omitted = renamed.clone();
        omitted.run_id.clear();
        assert!(
            !t.retarget("main", &omitted),
            "an inspection without the run keeps it"
        );
        assert!(t.retarget("main", &target("run-2")));
        assert_eq!(
            t.disconnect("main"),
            None,
            "the changed target already stopped it"
        );
    }

    #[test]
    fn identical_sizes_never_reach_the_pty() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        for _ in 0..50 {
            t.resize("main", c.generation, 80, 24);
        }
        assert_eq!(t.resizes("main"), 0);
        t.resize("main", c.generation, 100, 30);
        t.resize("main", c.generation, 100, 30);
        assert_eq!(t.resizes("main"), 1);
        t.resize("main", c.generation + 1, 120, 40);
        assert_eq!(t.resizes("main"), 1, "a stale generation changes nothing");
        hello(&t, c.generation, &sink);
        t.write("main", c.generation, b"size\r").unwrap();
        until(&t, c.generation, &sink, |s| {
            s.text().contains("size 100 30")
        });
        t.disconnect("main");
    }

    #[test]
    fn a_paste_over_the_queue_bound_queues_nothing() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        assert_eq!(
            t.write("main", c.generation, &vec![b'x'; 5 << 20]),
            Err("Terminal input queue is full; wait before pasting more text.".into())
        );
        hello(&t, c.generation, &sink);
        t.write("main", c.generation, b"echo ok\r").unwrap();
        until(&t, c.generation, &sink, |s| s.text().contains("echo ok"));
        t.disconnect("main");
    }

    #[test]
    fn a_flood_waits_for_acknowledgements_and_loses_nothing() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        hello(&t, c.generation, &sink);
        std::thread::sleep(Duration::from_millis(50));
        ack_all(&t, c.generation, &sink);
        let start = sink.outputs();
        t.write("main", c.generation, b"flood 3000000\r").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            sink.outputs() - start,
            2,
            "exactly two unacknowledged messages, then the drain waits"
        );
        until(&t, c.generation, &sink, |s| s.text().contains("done"));
        let text = sink.text();
        let flood = text.split_once("}}\r\n").unwrap().1;
        assert_eq!(
            flood.matches('x').count(),
            3_000_000,
            "the hello line holds two x of its own"
        );
        t.disconnect("main");
    }

    #[test]
    fn the_finish_reports_qts_status_and_forgets_the_view() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        hello(&t, c.generation, &sink);
        t.write("main", c.generation, b"exit 2\r").unwrap();
        until(&t, c.generation, &sink, |s| !s.messages().is_empty());
        assert_eq!(
            sink.messages()[0],
            json!({"kind": "finished", "code": 2,
                   "status": "Attach client exited (2) — session output is preserved above"})
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while lock(&t.0.views).contains_key("main") {
            assert!(
                Instant::now() < deadline,
                "the finished attach is forgotten"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(t.disconnect("main"), None);
    }

    fn ack_all(t: &Terminals, generation: u64, sink: &Collect) {
        for _ in 0..sink.outputs() {
            t.ack("main", generation);
        }
    }

    /// Connects, acknowledges the hello and starts a 3 MB flood, holding every
    /// acknowledgement once two messages arrived.
    fn held_flood(t: &Terminals, sink: &Arc<Collect>) -> (u64, usize) {
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        hello(t, c.generation, sink);
        std::thread::sleep(Duration::from_millis(50));
        ack_all(t, c.generation, sink);
        let start = sink.outputs();
        t.write("main", c.generation, b"flood 3000000\r").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while sink.outputs() < start + 2 {
            assert!(Instant::now() < deadline, "the flood starts");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Let the core's bounded channel fill behind the held messages.
        std::thread::sleep(Duration::from_millis(300));
        (c.generation, start)
    }

    #[test]
    fn a_remount_during_a_held_flood_loses_and_doubles_nothing() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let first = Arc::new(Collect::default());
        let (g, _) = held_flood(&t, &first);
        let delivered: Vec<u8> = lock(&first.seen).0.concat();
        let second = Arc::new(Collect::default());
        let again = t
            .connect("main", target("run-1"), 80, 24, second.clone())
            .unwrap();
        assert!(again.reused);
        let replay = lock(&second.seen).0[0].clone();
        assert!(
            delivered.ends_with(&replay),
            "the redraw is a suffix of what the first view received"
        );
        until(&t, g, &second, |s| s.text().contains("done"));
        let mut all = delivered[..delivered.len() - replay.len()].to_vec();
        all.extend(lock(&second.seen).0.concat());
        let text = String::from_utf8_lossy(&all).into_owned();
        let flood = text.split_once("}}\r\n").unwrap().1;
        assert_eq!(
            flood.matches('x').count(),
            3_000_000,
            "none twice, none missing"
        );
        t.disconnect("main");
    }

    #[test]
    fn nothing_reaches_a_view_after_its_disconnect() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let _ = held_flood(&t, &sink);
        let at = sink.outputs();
        let started = Instant::now();
        assert!(t.disconnect("main").is_some());
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "no wait for acknowledgements"
        );
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(sink.outputs(), at, "no output after the disconnect");
        assert!(sink.messages().is_empty(), "and no finish");
    }

    #[test]
    fn a_reconnect_on_the_same_sink_never_hears_the_old_attach() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let _ = held_flood(&t, &sink);
        t.disconnect("main");
        let at = sink.outputs();
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        assert!(!c.reused);
        std::thread::sleep(Duration::from_millis(2500));
        let after = lock(&sink.seen).0[at..].to_vec();
        assert!(
            after.iter().all(|m| !m.iter().all(|&b| b == b'x')),
            "no message of the old attach follows"
        );
        t.disconnect("main");
    }

    #[test]
    fn a_reused_connect_applies_its_size() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let first = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, first.clone())
            .unwrap();
        hello(&t, c.generation, &first);
        let second = Arc::new(Collect::default());
        let again = t
            .connect("main", target("run-1"), 100, 30, second.clone())
            .unwrap();
        assert!(again.reused);
        assert_eq!(t.resizes("main"), 1);
        t.connect("main", target("run-1"), 100, 30, second.clone())
            .unwrap();
        assert_eq!(t.resizes("main"), 1, "the same size is not applied twice");
        t.write("main", c.generation, b"size\r").unwrap();
        until(&t, c.generation, &second, |s| {
            s.text().contains("size 100 30")
        });
        t.disconnect("main");
    }

    #[test]
    fn an_unacknowledged_view_gets_the_next_message_after_about_a_second() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t
            .connect("main", target("run-1"), 80, 24, sink.clone())
            .unwrap();
        hello(&t, c.generation, &sink);
        std::thread::sleep(Duration::from_millis(50));
        ack_all(&t, c.generation, &sink);
        let start = sink.outputs();
        t.write("main", c.generation, b"flood 3000000\r").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while sink.outputs() < start + 2 {
            assert!(Instant::now() < deadline, "the flood starts");
            std::thread::sleep(Duration::from_millis(2));
        }
        let held = Instant::now();
        while sink.outputs() < start + 3 {
            assert!(
                held.elapsed() < Duration::from_secs(4),
                "the fallback delivers"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let waited = held.elapsed();
        assert!(waited >= Duration::from_millis(900), "waited {waited:?}");
        t.disconnect("main");
    }

    #[test]
    fn a_message_is_at_most_one_mib_and_no_byte_is_lost() {
        let pipe = Arc::new(Pipe {
            delivery: Mutex::new(Delivery {
                sink: Arc::new(Collect::default()),
                delivered: VecDeque::new(),
                unacked: 0,
                closed: false,
            }),
            acked: Condvar::new(),
        });
        let sink = Arc::new(Collect::default());
        lock(&pipe.delivery).sink = sink.clone();
        let (sender, events) = std::sync::mpsc::sync_channel(64);
        for i in 0..5u8 {
            sender
                .send(AttachEvent::Output {
                    data: vec![i; 250_000],
                })
                .unwrap();
        }
        sender.send(AttachEvent::Finished { code: 3 }).unwrap();
        let acker = {
            let pipe = Arc::clone(&pipe);
            std::thread::spawn(move || {
                let stop = Instant::now() + Duration::from_secs(5);
                while Instant::now() < stop {
                    lock(&pipe.delivery).unacked = 0;
                    pipe.acked.notify_all();
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        assert_eq!(pump(&events, &pipe), Some(3));
        let seen = lock(&sink.seen).0.clone();
        assert!(
            seen.iter().all(|m| m.len() <= 1 << 20),
            "sizes {:?}",
            seen.iter().map(Vec::len).collect::<Vec<_>>()
        );
        let all = seen.concat();
        assert_eq!(all.len(), 1_250_000);
        assert!(all
            .chunks(250_000)
            .enumerate()
            .all(|(i, c)| c.iter().all(|&b| b == i as u8)));
        drop(acker);
    }

    #[test]
    fn a_replay_without_an_explicit_hgs_refuses() {
        let t = Terminals::new(PathBuf::from("hgs"), false);
        let sink = Arc::new(Collect::default());
        assert_eq!(
            t.connect("main", target("run-1"), 80, 24, sink),
            Err("Actions in replay mode need ZERUS_CLIENT_HGS.".into())
        );
    }

    #[test]
    fn status_follows_terminal_available() {
        let input = |cmd: &str, state: &str, online: bool| StatusInput {
            facts: TerminalFacts {
                cmd: cmd.into(),
                terminal_supported: cmd != "dsh",
                ..TerminalFacts::default()
            },
            details: json!({}),
            state: state.into(),
            online,
            restored_pending: false,
            name: "claude/sample-project/review".into(),
            machine: String::new(),
        };
        let ready = status(input("claude", "running", true));
        assert!(ready.supported && ready.available && ready.reason.is_empty());
        assert_eq!(ready.idle, "Connect to use the session's live terminal");
        assert_eq!(
            ready.connecting,
            "Connecting to claude/sample-project/review…"
        );
        assert_eq!(
            ready.disconnected,
            "Disconnected — the session keeps running"
        );
        let paused = status(input("claude", "paused", true));
        assert_eq!(
            paused.reason,
            "Resume this session before opening its terminal."
        );
        let deepseek = status(input("dsh", "running", true));
        assert!(!deepseek.supported);
        assert_eq!(
            deepseek.reason,
            "This native adapter does not provide Terminal."
        );
        assert_eq!(
            status(input("claude", "running", false)).reason,
            "Machine unavailable. Reconnect when it is online."
        );
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml terminal::`
Expected: compile errors, `Terminals` and `status` are not defined.

- [ ] **Step 3: Write the module** above the tests:

```rust
//! The embedded Terminal's attach clients, ported from `tray/src/TerminalView.cpp`:
//! one `hgs [@host] a NAME --existing [--run-id RUN]` per view, in a
//! pseudoterminal (`zerus_client_core::terminal::Attach`). Output goes to the
//! view's `tauri::ipc::Channel` as raw bytes, the finish as one small JSON
//! message on the same Channel.
//!
//! A drain thread per attach reads the core's bounded receiver, joins the
//! output already waiting into one message of about 1 MiB at most and waits
//! while the webview holds two messages it has not acknowledged
//! (`terminal_ack`). A flood therefore back-pressures the PTY, not the
//! webview's main thread. The thread keeps the last 1 MiB it delivered: a
//! view shown again gets exactly that before the live output, so no byte
//! arrives twice or goes missing.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::State;
use zerus_client_core::terminal::{
    self, status as texts, Attach, AttachEvent, Target, TerminalFacts,
};

/// Delivered output a view shown again redraws from (as the core's replay).
const REPLAY_BYTES: usize = 1 << 20;
/// Waiting output joins one message up to this size.
const MESSAGE_BYTES: usize = 1 << 20;
/// The most one core read holds (`AttachEvent::Output`).
const READ_BYTES: usize = 256 << 10;
/// Messages the webview may hold unacknowledged before delivery waits.
const IN_FLIGHT: usize = 2;
/// A webview that stops acknowledging (it reloads, say) holds delivery this long.
const ACK_WAIT: Duration = Duration::from_secs(1);

/// What a view attaches to: `computer` is "" for this computer, else the
/// alias; `machine` names the computer in the disconnected status.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalTarget {
    pub computer: String,
    pub name: String,
    pub run_id: String,
    pub machine: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Connected {
    pub generation: u64,
    pub status: String,
    /// The view's attach kept running; its delivered output came first.
    pub reused: bool,
}

/// Whether the selected session can connect, and the texts the view shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TerminalStatus {
    pub supported: bool,
    pub available: bool,
    pub reason: String,
    pub idle: String,
    pub connecting: String,
    pub disconnected: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StatusInput {
    pub facts: TerminalFacts,
    /// The followed session's inspection (or phase 2's details), `{}` if none.
    #[serde(default)]
    pub details: Value,
    pub state: String,
    pub online: bool,
    #[serde(default)]
    pub restored_pending: bool,
    pub name: String,
    pub machine: String,
}

/// `terminal::available` and the status texts (`SessionsWindow.cpp:4021-4024`).
pub fn status(input: StatusInput) -> TerminalStatus {
    let available = terminal::available(
        &input.facts,
        &input.details,
        &input.state,
        input.online,
        input.restored_pending,
    );
    TerminalStatus {
        supported: terminal::supported(&input.facts, &input.details),
        available: available.is_ok(),
        reason: available.err().unwrap_or_default(),
        idle: texts::IDLE.into(),
        connecting: texts::connecting(&input.name),
        disconnected: texts::disconnected(&input.machine),
    }
}

/// Where a view's output goes: its Channel, or a test's collector.
pub trait Sink: Send + Sync {
    fn output(&self, bytes: Vec<u8>);
    fn message(&self, json: String);
}

struct ChannelSink(Channel);

impl Sink for ChannelSink {
    fn output(&self, bytes: Vec<u8>) {
        let _ = self.0.send(InvokeResponseBody::Raw(bytes));
    }
    fn message(&self, json: String) {
        let _ = self.0.send(InvokeResponseBody::Json(json));
    }
}

struct Delivery {
    sink: Arc<dyn Sink>,
    delivered: VecDeque<u8>,
    unacked: usize,
    /// The attach was stopped: nothing more is delivered, nothing is waited for.
    closed: bool,
}

struct Pipe {
    delivery: Mutex<Delivery>,
    acked: Condvar,
}

struct Open {
    generation: u64,
    target: TerminalTarget,
    attach: Attach,
    pipe: Arc<Pipe>,
    size: (u16, u16),
    resizes: usize,
}

struct Inner {
    hgs: PathBuf,
    allowed: bool,
    views: Mutex<HashMap<String, Open>>,
    /// Each view's last target, connected or not (`TerminalView`'s fields).
    targets: Mutex<HashMap<String, TerminalTarget>>,
    /// One connect at a time, so two never start an attach for one view.
    connecting: Mutex<()>,
    next: AtomicU64,
}

/// The managed state of the terminal commands; clones share it.
#[derive(Clone)]
pub struct Terminals(Arc<Inner>);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Terminals {
    /// `allowed` is false in a replay without an explicit `ZERUS_CLIENT_HGS`.
    pub fn new(hgs: PathBuf, allowed: bool) -> Self {
        Self(Arc::new(Inner {
            hgs,
            allowed,
            views: Mutex::new(HashMap::new()),
            targets: Mutex::new(HashMap::new()),
            connecting: Mutex::new(()),
            next: AtomicU64::new(0),
        }))
    }

    fn core(&self, target: &TerminalTarget) -> Target {
        Target {
            hgs: self.0.hgs.to_string_lossy().into_owned(),
            host: target.computer.clone(),
            name: target.name.clone(),
            run_id: target.run_id.clone(),
        }
    }

    /// The view's target after `next`: a kept run when the target did not change.
    fn remember(&self, view: &str, next: &TerminalTarget) -> (bool, TerminalTarget) {
        let mut targets = lock(&self.0.targets);
        let previous = targets.get(view).cloned();
        let changed = previous
            .as_ref()
            .is_none_or(|old| terminal::target_changed(&self.core(old), &self.core(next)));
        let mut kept = next.clone();
        if let Some(old) = previous.filter(|_| !changed) {
            kept.run_id = terminal::kept_run(&self.core(&old), &self.core(next));
        }
        targets.insert(view.to_owned(), kept.clone());
        (changed, kept)
    }

    /// Qt's `setSession`: true when the target changed, which stops the
    /// view's attach (the view clears its screen).
    pub fn retarget(&self, view: &str, target: &TerminalTarget) -> bool {
        // Not while a connect sets the view up.
        let _one = lock(&self.0.connecting);
        let (changed, kept) = self.remember(view, target);
        if changed {
            self.disconnect(view);
        } else if let Some(open) = lock(&self.0.views).get_mut(view) {
            open.target = kept;
        }
        changed
    }

    /// Connects the view, or rebinds its running attach to `sink` when the
    /// target did not change (a remounted or reloaded view).
    pub fn connect(
        &self,
        view: &str,
        target: TerminalTarget,
        cols: u16,
        rows: u16,
        sink: Arc<dyn Sink>,
    ) -> Result<Connected, String> {
        if !self.0.allowed {
            return Err(crate::actions::REPLAY_NEEDS_HGS.into());
        }
        let _one = lock(&self.0.connecting);
        let (changed, target) = self.remember(view, &target);
        if !changed {
            if let Some(connected) = self.rebind(view, &target, &sink, (cols, rows)) {
                return Ok(connected);
            }
        }
        // Stopping joins the attach's I/O thread: never under the views lock.
        let old = lock(&self.0.views).remove(view);
        if let Some(mut old) = old {
            old.close();
        }
        let (attach, events) = Attach::start(&self.core(&target), cols, rows)?;
        let generation = self.0.next.fetch_add(1, Ordering::Relaxed) + 1;
        let pipe = Arc::new(Pipe {
            delivery: Mutex::new(Delivery {
                sink,
                delivered: VecDeque::new(),
                unacked: 0,
                closed: false,
            }),
            acked: Condvar::new(),
        });
        self.drain(
            view.to_owned(),
            generation,
            events,
            Arc::clone(&pipe),
            target.machine.clone(),
        );
        let status = texts::connected(&target.name);
        lock(&self.0.views).insert(
            view.to_owned(),
            Open {
                generation,
                target,
                attach,
                pipe,
                size: (cols, rows),
                resizes: 0,
            },
        );
        Ok(Connected {
            generation,
            status,
            reused: false,
        })
    }

    fn rebind(
        &self,
        view: &str,
        target: &TerminalTarget,
        sink: &Arc<dyn Sink>,
        size: (u16, u16),
    ) -> Option<Connected> {
        let mut views = lock(&self.0.views);
        let open = views.get_mut(view)?;
        open.target = target.clone();
        open.set_size(size.0, size.1);
        let mut delivery = lock(&open.pipe.delivery);
        let replay: Vec<u8> = delivery.delivered.iter().copied().collect();
        delivery.unacked = usize::from(!replay.is_empty());
        if !replay.is_empty() {
            sink.output(replay);
        }
        delivery.sink = Arc::clone(sink);
        open.pipe.acked.notify_all();
        Some(Connected {
            generation: open.generation,
            status: texts::connected(&open.target.name),
            reused: true,
        })
    }

    fn drain(
        &self,
        view: String,
        generation: u64,
        events: Receiver<AttachEvent>,
        pipe: Arc<Pipe>,
        machine: String,
    ) {
        let inner = Arc::clone(&self.0);
        std::thread::Builder::new()
            .name("zerus-terminal".into())
            .spawn(move || {
                let finished = pump(&events, &pipe);
                // A stop reports nothing; only a finish does.
                let Some(code) = finished else { return };
                let message = json!({"kind": "finished", "code": code, "status": texts::finished(code, &machine)});
                {
                    let delivery = lock(&pipe.delivery);
                    if delivery.closed {
                        return;
                    }
                    delivery.sink.message(message.to_string());
                }
                let ended = {
                    let mut views = lock(&inner.views);
                    match views.get(&view) {
                        Some(open) if open.generation == generation => views.remove(&view),
                        _ => None,
                    }
                };
                drop(ended);
            })
            .expect("the terminal drain thread starts");
    }

    /// Queues input for the view's attach; a stale generation is ignored.
    pub fn write(&self, view: &str, generation: u64, bytes: &[u8]) -> Result<(), String> {
        match lock(&self.0.views).get(view) {
            Some(open) if open.generation == generation => open.attach.write(bytes),
            _ => Ok(()),
        }
    }

    /// Applies a new size; an unchanged size never reaches the PTY.
    pub fn resize(&self, view: &str, generation: u64, cols: u16, rows: u16) {
        let mut views = lock(&self.0.views);
        if let Some(open) = views
            .get_mut(view)
            .filter(|open| open.generation == generation)
        {
            open.set_size(cols, rows);
        }
    }

    /// Resizes applied to the view's attach so far.
    #[cfg(test)]
    fn resizes(&self, view: &str) -> usize {
        lock(&self.0.views).get(view).map_or(0, |open| open.resizes)
    }

    /// The webview wrote one output message to its screen.
    pub fn ack(&self, view: &str, generation: u64) {
        let views = lock(&self.0.views);
        if let Some(open) = views.get(view).filter(|open| open.generation == generation) {
            let mut delivery = lock(&open.pipe.delivery);
            delivery.unacked = delivery.unacked.saturating_sub(1);
            open.pipe.acked.notify_all();
        }
    }

    /// Hangs up the view's attach; the session keeps running. The status
    /// text, or None when nothing was connected.
    pub fn disconnect(&self, view: &str) -> Option<String> {
        let old = lock(&self.0.views).remove(view);
        old.map(|mut open| {
            open.close();
            texts::disconnected(&open.target.machine)
        })
    }

    /// The window closed: every attach hangs up.
    pub fn stop_all(&self) {
        let all: Vec<Open> = lock(&self.0.views).drain().map(|(_, open)| open).collect();
        for mut open in all {
            open.close();
        }
    }
}

impl Open {
    /// Applies a new size; an unchanged size never reaches the PTY.
    fn set_size(&mut self, cols: u16, rows: u16) {
        if self.size != (cols, rows) {
            self.size = (cols, rows);
            self.resizes += 1;
            self.attach.resize(cols, rows);
        }
    }

    /// Hangs up: delivery ends first, so buffered output never reaches the
    /// view's sink and nothing waits for its acknowledgements.
    fn close(&mut self) {
        lock(&self.pipe.delivery).closed = true;
        self.pipe.acked.notify_all();
        self.attach.stop();
    }
}

/// Joins the core's events into messages of at most `MESSAGE_BYTES` and
/// delivers them. Returns the exit code of a finish, None after a stop.
fn pump(events: &Receiver<AttachEvent>, pipe: &Pipe) -> Option<i32> {
    let mut finished = None;
    while finished.is_none() {
        let Ok(first) = events.recv() else { break };
        let mut chunk = Vec::new();
        let mut next = Some(first);
        while let Some(event) = next.take() {
            match event {
                AttachEvent::Output { data } => chunk.extend_from_slice(&data),
                AttachEvent::Finished { code } => finished = Some(code),
            }
            if finished.is_none() && chunk.len() + READ_BYTES <= MESSAGE_BYTES {
                next = events.try_recv().ok();
            }
        }
        if !chunk.is_empty() && !deliver(pipe, chunk) {
            return None;
        }
    }
    finished
}

/// False when the attach was stopped: the chunk is dropped.
fn deliver(pipe: &Pipe, chunk: Vec<u8>) -> bool {
    let deadline = Instant::now() + ACK_WAIT;
    let mut delivery = lock(&pipe.delivery);
    while delivery.unacked >= IN_FLIGHT && !delivery.closed {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        delivery = pipe
            .acked
            .wait_timeout(delivery, left)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    if delivery.closed {
        return false;
    }
    delivery.delivered.extend(chunk.iter().copied());
    let excess = delivery.delivered.len().saturating_sub(REPLAY_BYTES);
    delivery.delivered.drain(..excess);
    delivery.unacked += 1;
    delivery.sink.output(chunk);
    true
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    let task = tauri::async_runtime::spawn_blocking(work);
    async move { task.await.map_err(|error| error.to_string()) }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn terminal_connect(
    state: State<'_, Terminals>,
    view: String,
    target: TerminalTarget,
    cols: u16,
    rows: u16,
    on_event: Channel,
) -> Result<Connected, String> {
    let terminals = state.inner().clone();
    blocking(move || terminals.connect(&view, target, cols, rows, Arc::new(ChannelSink(on_event))))
        .await?
}

/// Input as a raw body: `x-zerus-view` and `x-zerus-generation` name the attach.
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, Terminals>,
    request: Request<'_>,
) -> Result<(), String> {
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    };
    let view = header("x-zerus-view").ok_or("missing x-zerus-view")?;
    let generation = header("x-zerus-generation")
        .and_then(|value| value.parse().ok())
        .ok_or("missing x-zerus-generation")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("terminal input must be a raw body".into());
    };
    state.write(view, generation, bytes)
}

#[tauri::command(rename_all = "snake_case")]
pub fn terminal_resize(
    state: State<'_, Terminals>,
    view: String,
    generation: u64,
    cols: u16,
    rows: u16,
) {
    state.resize(&view, generation, cols, rows);
}

#[tauri::command(rename_all = "snake_case")]
pub fn terminal_ack(state: State<'_, Terminals>, view: String, generation: u64) {
    state.ack(&view, generation);
}

#[tauri::command]
pub async fn terminal_disconnect(
    state: State<'_, Terminals>,
    view: String,
) -> Result<Option<String>, String> {
    let terminals = state.inner().clone();
    blocking(move || terminals.disconnect(&view)).await
}

#[tauri::command]
pub async fn terminal_retarget(
    state: State<'_, Terminals>,
    view: String,
    target: TerminalTarget,
) -> Result<bool, String> {
    let terminals = state.inner().clone();
    blocking(move || terminals.retarget(&view, &target)).await
}

#[tauri::command]
pub fn terminal_status(input: StatusInput) -> TerminalStatus {
    status(input)
}
```

In `client/src-tauri/src/actions.rs`, change `const REPLAY_NEEDS_HGS` to `pub(crate) const REPLAY_NEEDS_HGS`.

- [ ] **Step 4: Register the module** in `client/src-tauri/src/lib.rs`:
  - add `#[cfg(feature = "live")] mod terminal;`;
  - in `setup`, after the actions state: `app.manage(terminal::Terminals::new(zerus_client_core::default_hgs(), actions::actions_allowed(std::env::var("ZERUS_CLIENT_REPLAY").ok().as_deref(), std::env::var("ZERUS_CLIENT_HGS").ok().as_deref())));`;
  - add the seven commands to the live `generate_handler!`;
  - stop every attach when the main window goes, before `.run(...)`:

```rust
        .on_window_event(|window, event| {
            #[cfg(feature = "live")]
            if window.label() == MAIN_WINDOW && matches!(event, tauri::WindowEvent::Destroyed) {
                use tauri::Manager;
                window.state::<terminal::Terminals>().stop_all();
            }
            #[cfg(not(feature = "live"))]
            let _ = (window, event);
        })
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml terminal::`
Expected: PASS, 15 tests. Then `cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and `cargo +1.99.1 fmt --check --manifest-path src-tauri/Cargo.toml`: clean.

- [ ] **Step 6: Commit**

```bash
git add client/src-tauri/src/terminal.rs client/src-tauri/src/lib.rs client/src-tauri/src/actions.rs
git commit -m "feat(client): Terminal attach commands with acknowledged output and exact redraw"
```

---

### Task 2: The Terminal tab (xterm.js)

**Depends on:** Task 1.

**Qt reference:**
- `tray/src/TerminalView.cpp:15-72` (toolbar, buttons), `:182-208` (`updateButtons`: icons, accessible names, tooltips);
- `tray/src/SessionsWindow.cpp:1501-1505` (showing the tab connects), `:1637` (leaving the page disconnects), `:3456-3459` (another selection), `:4011-4026` (availability; the tab shows only when the session has a terminal and no subagent is open);
- `tray/src/TerminalScreen.cpp` (`MaxHistoryLines = 5000`).

Spec: "xterm.js when Terminal first opens"; "DOM renderer by default on Linux, WebGL addon on macOS and Windows with context-loss handling".

**Files:**
- Modify: `client/package.json`, `client/package-lock.json` (`npm install --save-exact @xterm/xterm@6.0.0 @xterm/addon-fit@0.11.0 @xterm/addon-webgl@0.19.0`)
- Modify: `client/src/types.ts`, `client/src/bridge.ts`
- Create: `client/src/store/terminal.ts`, `client/src/store/terminal.test.ts`
- Create: `client/src/ui/sessions/Terminal.tsx`
- Modify: `client/src/store/ui.ts` (`detailTab`), `client/src/ui/sessions/SessionDetail.tsx` (tabs)
- Modify: `client/src/ui/lib/icons.generated.ts` (`unplug`, which the generated set lacks)

**Interfaces:**
- Consumes: Task 1's commands and Channel messages; `detailsFor`, `followedFor` (`store/actions.ts`); `selected`, `page` (`store/ui.ts`); `computerOffline` (`store/catalog.ts`).
- Produces:

```ts
// types.ts
export interface TerminalTarget { computer: ComputerId; name: string; run_id: string; machine: string }
export interface TerminalConnected { generation: number; status: string; reused: boolean }
export interface TerminalStatus { supported: boolean; available: boolean; reason: string; idle: string; connecting: string; disconnected: string }
export interface TerminalFinished { kind: "finished"; code: number; status: string }
// store/ui.ts
export type DetailTab = "activity" | "terminal" | "native" | "processes";
export const [detailTab, setDetailTab]: Signal<DetailTab>;
// store/terminal.ts
export interface Screen { write(bytes: Uint8Array, done: () => void): void; reset(): void; size(): { cols: number; rows: number } }
export function createTerminal(port: TerminalPort, timers?: Timers): TerminalStore;
export const terminal: TerminalStore; // the app's, on the bridge
// TerminalStore: phase(), status(), connected(), ready(), setTarget(target, status), show(screen), hide(), connect(), disconnect(), reconnect(), toggle(), input(bytes), resized(cols, rows), paste(text)
```

- [ ] **Step 1: Write the failing store tests** in `client/src/store/terminal.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createTerminal, RESIZE_MS, type Screen, type TerminalPort, type Timers } from "./terminal";
import type { TerminalStatus, TerminalTarget } from "../types";

const target = (run = "run-1"): TerminalTarget => ({ computer: "mac", name: "claude/sample-project/review", run_id: run, machine: "studio" });
const ok: TerminalStatus = {
  supported: true, available: true, reason: "", idle: "Connect to use the session's live terminal",
  connecting: "Connecting to claude/sample-project/review…", disconnected: "Disconnected — the session continues on studio",
};

function fakes() {
  const calls: string[] = [];
  let onEvent: ((m: ArrayBuffer | { kind: "finished"; code: number; status: string }) => void) | undefined;
  let changed = true;
  let failWrite: string | undefined;
  const port: TerminalPort = {
    connect: async (_view, t, cols, rows, handler) => { calls.push(`connect ${t.run_id} ${cols}x${rows}`); onEvent = handler; return { generation: 7, status: `Connected: ${t.name}`, reused: false }; },
    write: async (_view, generation, bytes) => { calls.push(`write ${generation} ${new TextDecoder().decode(bytes)}`); if (failWrite) throw failWrite; },
    resize: async (_view, generation, cols, rows) => { calls.push(`resize ${generation} ${cols}x${rows}`); },
    ack: (_view, generation) => { calls.push(`ack ${generation}`); },
    disconnect: async () => { calls.push("disconnect"); return "Disconnected — the session continues on studio"; },
    retarget: async (_view, t) => { calls.push(`retarget ${t.run_id}`); return changed; },
  };
  let clock = 0;
  const pending: { at: number; fn: () => void }[] = [];
  const timers: Timers = { now: () => clock, later: (fn, ms) => { const job = { at: clock + ms, fn }; pending.push(job); return () => pending.splice(pending.indexOf(job), 1); } };
  const advance = (ms: number) => {
    clock += ms;
    for (const job of pending.filter((j) => j.at <= clock)) { pending.splice(pending.indexOf(job), 1); job.fn(); }
  };
  const written: string[] = [];
  let resets = 0;
  const screen: Screen = { write: (bytes, done) => { written.push(new TextDecoder().decode(bytes)); done(); }, reset: () => { resets++; }, size: () => ({ cols: 100, rows: 30 }) };
  return { calls, port, timers, advance, screen, written, resets: () => resets, emit: (m: Parameters<NonNullable<typeof onEvent>>[0]) => onEvent?.(m), setChanged: (v: boolean) => { changed = v; }, setFailWrite: (v?: string) => { failWrite = v; } };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("terminal store", () => {
  it("connects when the tab shows and acknowledges each write to the screen", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      expect(t.status()).toBe(ok.idle);
      t.show(f.screen);
      expect(t.status()).toBe(ok.connecting);
      await tick();
      expect(f.calls).toEqual(["retarget run-1", "connect run-1 100x30"]);
      expect(t.status()).toBe("Connected: claude/sample-project/review");
      f.emit(new TextEncoder().encode("hello").buffer);
      expect(f.written).toEqual(["hello"]);
      expect(f.calls.at(-1)).toBe("ack 7");
    });
  });

  it("a hidden view still acknowledges, so the attach never stalls", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.hide();
      f.emit(new TextEncoder().encode("while hidden").buffer);
      expect(f.written).toEqual([]);
      expect(f.calls.at(-1)).toBe("ack 7");
    });
  });

  it("resizes are throttled to one per 100 ms plus the trailing one", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      for (let i = 0; i < 200; i++) { t.resized(80 + (i % 7), 24); if (i % 2) f.advance(1); }
      t.resized(120, 40);
      f.advance(RESIZE_MS);
      f.advance(RESIZE_MS);
      const resizes = f.calls.filter((c) => c.startsWith("resize"));
      expect(resizes.length).toBeLessThanOrEqual(3);
      expect(resizes.at(-1)).toBe("resize 7 120x40");
    });
  });

  it("writes keep their order and a refused one shows its text", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.input(new TextEncoder().encode("a"));
      t.input(new TextEncoder().encode("b"));
      await tick(); await tick();
      expect(f.calls.filter((c) => c.startsWith("write"))).toEqual(["write 7 a", "write 7 b"]);
      f.setFailWrite("Terminal input queue is full; wait before pasting more text.");
      t.input(new TextEncoder().encode("x".repeat(10)));
      await tick(); await tick();
      expect(t.status()).toBe("Terminal input queue is full; wait before pasting more text.");
    });
  });

  it("a changed target clears the screen and reconnects only while shown", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      const before = f.resets();
      await t.setTarget(target("run-2"), ok);
      await tick();
      expect(f.resets()).toBeGreaterThan(before);
      expect(f.calls.at(-1)).toBe("connect run-2 100x30");
      f.setChanged(false);
      t.hide();
      await t.setTarget(target("run-2"), ok);
      expect(f.calls.filter((c) => c.startsWith("connect")).length).toBe(2);
    });
  });

  it("losing availability disconnects with the reason; a finish shows Qt's status", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      f.emit({ kind: "finished", code: 2, status: "Attach client exited (2) — session output is preserved above" });
      expect(t.status()).toBe("Attach client exited (2) — session output is preserved above");
      expect(t.connected()).toBe(false);
      t.show(f.screen);
      await tick();
      f.setChanged(false);
      await t.setTarget(target(), { ...ok, available: false, reason: "Machine unavailable. Reconnect when it is online." });
      expect(f.calls.at(-1)).toBe("disconnect");
      expect(t.status()).toBe("Machine unavailable. Reconnect when it is online.");
    });
  });
});
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && npm test -- src/store/terminal.test.ts`
Expected: FAIL, `./terminal` does not exist.

- [ ] **Step 3: Write `client/src/store/terminal.ts`**

```ts
import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import type { TerminalConnected, TerminalFinished, TerminalStatus, TerminalTarget } from "../types";

// The Terminal tab's state (Qt TerminalView): one attach per view, connected
// while the tab shows. Output is written to the screen and acknowledged once
// xterm parsed it, so Rust delivers at most two messages ahead. A hidden view
// acknowledges at once and drops the bytes: Rust keeps what it delivered and a
// view shown again redraws from it. Writes run one at a time, in order.

export const VIEW = "main";
/** At most one resize per this interval reaches Rust, plus the trailing one. */
export const RESIZE_MS = 100;

export interface TerminalPort {
  connect(view: string, target: TerminalTarget, cols: number, rows: number, onEvent: (message: ArrayBuffer | TerminalFinished) => void): Promise<TerminalConnected>;
  write(view: string, generation: number, bytes: Uint8Array): Promise<void>;
  resize(view: string, generation: number, cols: number, rows: number): Promise<void>;
  ack(view: string, generation: number): void;
  disconnect(view: string): Promise<string | null>;
  retarget(view: string, target: TerminalTarget): Promise<boolean>;
}

export interface Screen {
  write(bytes: Uint8Array, done: () => void): void;
  reset(): void;
  size(): { cols: number; rows: number };
}

export interface Timers { now(): number; later(fn: () => void, ms: number): () => void }
const realTimers: Timers = { now: () => performance.now(), later: (fn, ms) => { const id = setTimeout(fn, ms); return () => clearTimeout(id); } };

const message = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

export function createTerminal(port: TerminalPort, timers: Timers = realTimers) {
  const [phase, setPhase] = createSignal<"idle" | "connecting" | "connected">("idle");
  const [status, setStatus] = createSignal("");
  const [texts, setTexts] = createSignal<TerminalStatus | undefined>();
  let target: TerminalTarget | undefined;
  let screen: Screen | undefined;
  let generation = 0;
  /** Bumps on every connect, disconnect and target change: older Channels are ignored. */
  let serial = 0;
  let writes: Promise<void> = Promise.resolve();
  let pendingSize: { cols: number; rows: number } | undefined;
  let lastResize = Number.NEGATIVE_INFINITY;
  let resizeTimer: (() => void) | undefined;

  const ready = () => !!target && !!texts()?.available;

  const handler = (mine: number) => (m: ArrayBuffer | TerminalFinished) => {
    if (mine !== serial) return;
    if (m instanceof ArrayBuffer || ArrayBuffer.isView(m)) {
      const bytes = m instanceof ArrayBuffer ? new Uint8Array(m) : new Uint8Array((m as Uint8Array).buffer);
      const gen = generation;
      if (screen) screen.write(bytes, () => port.ack(VIEW, gen));
      else port.ack(VIEW, gen);
      return;
    }
    if (m.kind === "finished") {
      serial++;
      generation = 0;
      setPhase("idle");
      setStatus(m.status);
    }
  };

  /** Connects, or rebinds the running attach after the view was mounted again. */
  async function connect() {
    if (!ready() || !screen || phase() === "connecting") return;
    const mine = ++serial;
    const rebinding = phase() === "connected";
    setPhase("connecting");
    if (!rebinding) setStatus(texts()!.connecting);
    screen.reset();
    const { cols, rows } = screen.size();
    try {
      const c = await port.connect(VIEW, target!, cols, rows, handler(mine));
      if (mine !== serial) return;
      generation = c.generation;
      setPhase("connected");
      setStatus(c.status);
    } catch (error) {
      if (mine !== serial) return;
      setPhase("idle");
      setStatus(message(error));
    }
  }

  async function disconnect() {
    if (phase() === "idle") return;
    serial++;
    generation = 0;
    setPhase("idle");
    const text = await port.disconnect(VIEW).catch(() => null);
    setStatus(text ?? texts()?.disconnected ?? "");
  }

  return {
    phase, status, ready,
    connected: () => phase() === "connected",
    /** The selected session's target and texts (on every change of either). */
    async setTarget(next: TerminalTarget | undefined, nextTexts: TerminalStatus | undefined) {
      setTexts(nextTexts);
      if (!next || !nextTexts) { await disconnect(); target = undefined; setStatus(""); return; }
      const changed = await port.retarget(VIEW, next);
      target = next;
      if (changed) {
        serial++;
        generation = 0;
        setPhase("idle");
        screen?.reset();
        setStatus(nextTexts.available ? nextTexts.idle : nextTexts.reason);
      }
      if (!nextTexts.available) {
        if (phase() !== "idle") { serial++; generation = 0; setPhase("idle"); await port.disconnect(VIEW).catch(() => null); }
        setStatus(nextTexts.reason);
        return;
      }
      if (screen && phase() === "idle" && changed) void connect();
    },
    /** The tab shows `s`: connect (Qt's currentChanged), or redraw a running attach. */
    show(s: Screen) { screen = s; void connect(); },
    /** The tab hides; the attach keeps running. */
    hide() { screen = undefined; },
    connect, disconnect,
    async reconnect() { await disconnect(); await connect(); },
    async toggle() { if (phase() === "idle") await connect(); else await disconnect(); },
    input(bytes: Uint8Array) {
      if (phase() !== "connected" || !bytes.length) return;
      const gen = generation;
      writes = writes.then(() => port.write(VIEW, gen, bytes)).catch((error) => { setStatus(message(error)); });
    },
    resized(cols: number, rows: number) {
      pendingSize = { cols, rows };
      if (resizeTimer) return;
      const wait = Math.max(0, lastResize + RESIZE_MS - timers.now());
      resizeTimer = timers.later(() => {
        resizeTimer = undefined;
        if (!pendingSize || phase() !== "connected") return;
        const size = pendingSize;
        pendingSize = undefined;
        lastResize = timers.now();
        void port.resize(VIEW, generation, size.cols, size.rows).catch(() => {});
      }, wait);
    },
  };
}

export type TerminalStore = ReturnType<typeof createTerminal>;

/** The app's Terminal, on the bridge. */
export const terminal = createTerminal({
  connect: bridge.terminalConnect, write: bridge.terminalWrite, resize: bridge.terminalResize,
  ack: bridge.terminalAck, disconnect: bridge.terminalDisconnect, retarget: bridge.terminalRetarget,
});
```

- [ ] **Step 4: Add the types and bridge wrappers.** Append Step "Produces" types to `client/src/types.ts`. In `client/src/bridge.ts`:

```ts
import { Channel } from "@tauri-apps/api/core";
import type { TerminalConnected, TerminalFacts, TerminalFinished, TerminalStatus, TerminalTarget } from "./types";

const DESKTOP_TERMINAL = "Terminal needs the desktop app.";

export async function terminalConnect(view: string, target: TerminalTarget, cols: number, rows: number, onEvent: (m: ArrayBuffer | TerminalFinished) => void): Promise<TerminalConnected> {
  if (!inTauri()) throw DESKTOP_TERMINAL;
  const channel = new Channel<ArrayBuffer | TerminalFinished>();
  channel.onmessage = onEvent;
  return invoke<TerminalConnected>("terminal_connect", { view, target, cols, rows, on_event: channel });
}
export async function terminalWrite(view: string, generation: number, bytes: Uint8Array): Promise<void> {
  if (!inTauri()) return;
  await invoke("terminal_write", bytes, { headers: { "x-zerus-view": view, "x-zerus-generation": String(generation) } });
}
export async function terminalResize(view: string, generation: number, cols: number, rows: number): Promise<void> {
  if (inTauri()) await invoke("terminal_resize", { view, generation, cols, rows });
}
export function terminalAck(view: string, generation: number): void {
  if (inTauri()) void invoke("terminal_ack", { view, generation }).catch(() => {});
}
export async function terminalDisconnect(view: string): Promise<string | null> {
  return inTauri() ? invoke<string | null>("terminal_disconnect", { view }) : null;
}
export async function terminalRetarget(view: string, target: TerminalTarget): Promise<boolean> {
  return inTauri() ? invoke<boolean>("terminal_retarget", { view, target }) : true;
}
export async function terminalStatus(input: { facts: TerminalFacts; details: unknown; state: string; online: boolean; restored_pending: boolean; name: string; machine: string }): Promise<TerminalStatus> {
  if (!inTauri()) return { supported: false, available: false, reason: DESKTOP_TERMINAL, idle: "", connecting: "", disconnected: "" };
  return invoke<TerminalStatus>("terminal_status", { input });
}
```

`TerminalFacts` in `types.ts` mirrors the core's `{cmd, terminal_supported, terminal_reported, terminal_attach_supported, terminal_attach_reported, terminal_reason, terminal_attach_reason}`.

- [ ] **Step 5: Run the store tests to see them pass**

Run: `cd client && npm test -- src/store/terminal.test.ts`
Expected: PASS, 6 tests.

- [ ] **Step 6: Add the tab state** to `client/src/store/ui.ts`:

```ts
export type DetailTab = "activity" | "terminal" | "native" | "processes";
export const [detailTab, setDetailTab] = createSignal<DetailTab>("activity");
```

- [ ] **Step 7: Write `client/src/ui/sessions/Terminal.tsx`** (the mockup's `TerminalTab` with live data):

```tsx
import { createEffect, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { contentScale } from "../../store/ui";
import { skin } from "../../skin/runtime";
import { terminal } from "../../store/terminal";

// xterm.js 6 loads when Terminal first opens. Linux keeps the DOM renderer;
// macOS and Windows add the WebGL addon and fall back to DOM when its
// context is lost. Scrollback is Qt's MaxHistoryLines (5000).

const encoder = new TextEncoder();
const webgl = /Mac|Windows/.test(navigator.userAgent);

function theme() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return { background: v("--z-color-terminal-bg"), foreground: v("--z-color-terminal-fg"), cursor: v("--z-color-accent") };
}

export function TerminalTab() {
  let host!: HTMLDivElement;
  onMount(async () => {
    const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]);
    const css = getComputedStyle(document.documentElement);
    const term = new Terminal({ fontFamily: css.getPropertyValue("--z-font-mono"), fontSize: Math.round(13 * contentScale()), cursorBlink: true, scrollback: 5000, theme: theme() });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    if (webgl) {
      try {
        const { WebglAddon } = await import("@xterm/addon-webgl");
        const gl = new WebglAddon();
        gl.onContextLoss(() => gl.dispose());
        term.loadAddon(gl);
      } catch { /* the DOM renderer stays */ }
    }
    const measure = () => { if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit(); };
    measure();
    term.onData((data) => terminal.input(encoder.encode(data)));
    term.onBinary((data) => terminal.input(Uint8Array.from(data, (c) => c.charCodeAt(0) & 255)));
    term.onResize(({ cols, rows }) => terminal.resized(cols, rows));
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    createEffect(() => { skin(); queueMicrotask(() => { term.options.theme = theme(); }); });
    createEffect(() => { term.options.fontSize = Math.round(13 * contentScale()); measure(); });
    terminal.show({ write: (bytes, done) => term.write(bytes, done), reset: () => term.reset(), size: () => ({ cols: term.cols, rows: term.rows }) });
    term.focus();
    onCleanup(() => { terminal.hide(); observer.disconnect(); term.dispose(); });
  });
  const connected = () => terminal.connected();
  const toggleTip = () => (connected() ? "Disconnect terminal. The session keeps running" : terminal.ready() ? "Connect terminal" : terminal.status());
  return (
    <div class="term-tab">
      <div class="term-bar">
        <span class="state-dot" classList={{ working: connected() }} />
        <span class="ell" title={terminal.status()}>{terminal.status()}</span>
        <span class="grow" />
        <button class="icon-btn" data-tip="Reconnect terminal" aria-label="Reconnect terminal" disabled={!terminal.ready()} onClick={() => { void terminal.reconnect(); }}>
          <Icon name="refresh-cw" size={15} />
        </button>
        <button class="icon-btn" data-tip={toggleTip()} aria-label={connected() ? "Disconnect terminal" : "Connect terminal"} disabled={!connected() && !terminal.ready()}
          onClick={() => { void terminal.toggle(); }}>
          <Icon name={connected() ? "unplug" : "plug"} size={15} />
        </button>
      </div>
      <div class="term-host" ref={host} />
    </div>
  );
}
```

The icon names `refresh-cw`, `plug` and `unplug` come from `lucide-solid`; add any missing one to `client/src/ui/lib/icons.generated.ts` with the generator the file names in its header.

- [ ] **Step 8: Wire the tabs** in `client/src/ui/sessions/SessionDetail.tsx`. Replace `TABS` and the tab body:

```tsx
import { Match, Switch, createEffect, on } from "solid-js";
import { detailTab, page, setDetailTab, subagentOpen } from "../../store/ui";
import { terminal } from "../../store/terminal";
import { TerminalTab } from "./Terminal";
import * as bridge from "../../bridge";

/** The selected session's terminal status (terminal_status), refreshed when its inputs change. */
const [terminalTexts, setTerminalTexts] = createSignal<TerminalStatus | undefined>();
createRoot(() => {
  createEffect(on(() => {
    const s = selected();
    if (!s) return undefined;
    const followed = followedFor(s.key);
    const details = detailsFor(s, !computerOffline(s.computer), followed);
    return JSON.stringify([s.key, s.name, s.runId, s.cmd, s.state, s.terminalSupported, s.terminalReported, s.terminalAttachSupported,
      s.terminalAttachReported, !computerOffline(s.computer), followed.value ?? details ?? null]);
  }, (input) => {
    const s = selected();
    if (!input || !s) { setTerminalTexts(undefined); void terminal.setTarget(undefined, undefined); return; }
    const followed = followedFor(s.key);
    void bridge.terminalStatus({
      facts: { cmd: s.cmd, terminal_supported: s.terminalSupported, terminal_reported: s.terminalReported, terminal_attach_supported: s.terminalAttachSupported,
        terminal_attach_reported: s.terminalAttachReported, terminal_reason: "", terminal_attach_reason: "" },
      details: followed.value ?? detailsFor(s, !computerOffline(s.computer), followed) ?? {},
      state: s.state, online: !computerOffline(s.computer), restored_pending: false, name: s.name, machine: s.machine,
    }).then((status) => {
      setTerminalTexts(status);
      void terminal.setTarget({ computer: s.computer, name: s.name, run_id: s.runId ?? "", machine: s.machine }, status);
    });
  }));
  // Leaving the Sessions page disconnects (SessionsWindow.cpp:1637).
  createEffect(on(page, (p) => { if (p !== "sessions") void terminal.disconnect(); }, { defer: true }));
});

const tabs = () => [
  { id: "activity" as const, label: "Activity", show: true },
  { id: "terminal" as const, label: "Terminal", show: !!terminalTexts()?.supported && !subagentOpen() },
];

// A tab that disappears returns to Activity (SessionsWindow.cpp:4019-4020).
createRoot(() => createEffect(() => { if (!tabs().find((t) => t.id === detailTab())?.show) setDetailTab("activity"); }));
```

and render:

```tsx
<div class="detail-tabs tabs" role="tablist">
  <For each={tabs().filter((t) => t.show)}>{(t) => (
    <button class="tab" role="tab" classList={{ on: detailTab() === t.id }} aria-selected={detailTab() === t.id} onClick={() => setDetailTab(t.id)}>{t.label}</button>
  )}</For>
  <span class="grow" />
</div>
<div class="tab-body">
  <Switch>
    <Match when={detailTab() === "activity"}><div class="activity-wrap" style={columnStyle()}><ActivityView /></div></Match>
    <Match when={detailTab() === "terminal"}><TerminalTab /></Match>
  </Switch>
</div>
```

The effects live in a module-level `createRoot`, so they run once for the app, not per mount.

- [ ] **Step 9: Check the build and the rules**

Run: `cd client && npm run check && npm test && python3 ../scripts/check-ui-separators.py`
Expected: no type errors, all tests pass, no separators.

Then a headless look, which never opens a desktop window:

```sh
cd client && npm run build && RUSTUP_TOOLCHAIN=1.99.1 npx tauri build --no-bundle
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
  ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson ZERUS_CLIENT_QUERY=select=claude/sample-project/api-refactor \
  xvfb-run -a -s "-screen 0 1440x900x24" timeout 20 src-tauri/target/release/zerus-client
```

Expected: the app runs 20 s without a panic on stderr. The replay has no `hgs`, so the tab shows "Actions in replay mode need ZERUS_CLIENT_HGS."; Task 12's smoke connects to the fake.

- [ ] **Step 10: Commit**

```bash
git add client/package.json client/package-lock.json client/src/types.ts client/src/bridge.ts client/src/store/terminal.ts \
  client/src/store/terminal.test.ts client/src/store/ui.ts client/src/ui/sessions/Terminal.tsx client/src/ui/sessions/SessionDetail.tsx client/src/ui/lib/icons.generated.ts
git commit -m "feat(client): the Terminal tab on xterm.js"
```

---

### Task 3a: The composer hub: drafts and attachments

**Depends on:**
- A1 `zerus-5wmh.9.3` (#403): `attachments::{Draft, Attachment, Selection, FileRead, read_file, pasted_image, MAX_ATTACHMENTS}`.
- S5 `zerus-5wmh.9.11` (#409): `drafts::{DraftStore, StoredDraft, Loaded, IndexPage, read_draft, ALREADY_EXISTS}`.
- M1c (`zerus-5wmh.9.12`, branch `work-ba0ed9fd-media`) when it is on main: the app's one `Drafts` store, `drafts::dir` and the `exit` hook. Step 6 says what this task adds when M1c is not on main yet.

**Qt reference:** `tray/src/MessageComposer.cpp`:
- `setSessionKey` and `ensureDraftLoaded` (480-538), `restoreDraft` (539-548);
- `saveDraft` (676-693), `trimDraftCache` (610-619), `renameDraft` (778-809);
- the editor's change handler (370-379);
- `attachFile` and `addAttachment` (969-1000), the pasted image (393-398), `removeAttachment` (1045-1065);
- `updateControls` (1141-1229): the status line's priority, Send's enablement and label;
- the Allow retry button (412-417).

**Files:**
- Create: `client/src-tauri/src/composer/mod.rs`
- Modify: `client/src-tauri/Cargo.toml` (+ `tauri-plugin-dialog = "2"`, `[target.'cfg(unix)'.dependencies] signal-hook = "0.3"`), `client/src-tauri/Cargo.lock`
- Modify: `client/src-tauri/src/pump.rs` (`poke`)
- Modify: `client/src-tauri/src/commands.rs` (the `next_batch` envelope; `start` calls `Composers::resend`)
- Modify: `client/src-tauri/src/lib.rs` (`Drafts` and `exit` unless M1c brought them, the managed `Composers`, the dialog plugin, the signal thread, the commands)
- Test: unit tests in `composer/mod.rs`; one `pump.rs` test

**Interfaces:**
- Produces (Tasks 3b-9 and 13 extend these; the frontend binds to the serde shapes):

```rust
// lib.rs (M1c's, or this task's when M1c is not on main; identical either way)
#[cfg(feature = "live")] pub struct Drafts(pub std::sync::Arc<zerus_client_core::drafts::DraftStore>);
#[cfg(feature = "live")] fn exit(app: &AppHandle); // the one RunEvent::Exit hook
// composer/mod.rs
pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn() + Send + Sync>;
pub struct Deps { pub spawn: Spawn, pub notify: Notify }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef { pub computer: String, pub key: String, pub name: String, #[serde(default)] pub archive_id: Option<String> }
pub fn draft_key(computer: &str, name: &str, archive: Option<&str>) -> String;
#[derive(Clone)] pub struct Composers(/* Arc<Shared> */);
impl Composers {
    pub fn new(drafts: Arc<DraftStore>, deps: Deps) -> Self; // the app's one store, never its own
    pub fn open(&self, session: Option<SessionRef>) -> Option<ComposerView>;
    pub fn edit(&self, handle: u64, rev: u64, text: String, position: i64, anchor: i64);
    pub fn attach_paths(&self, handle: u64, paths: Vec<PathBuf>, selection: Selection);
    pub fn paste_image(&self, handle: u64, png: Vec<u8>, selection: Selection);
    pub fn remove(&self, handle: u64, index: usize);
    pub fn undo_remove(&self, handle: u64);
    pub fn allow_retry(&self, handle: u64);
    pub fn take_view(&self) -> Option<ComposerView>;
    pub fn take_draft_keys(&self) -> Option<Vec<String>>;
    pub fn resend(&self);
    pub fn flush(&self); // the exit hook: closes the hub; the store's own flush follows it
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AttachmentView { pub name: String, pub mime: String, pub reference: String, pub bytes: u64, pub path: String, pub excerpt: Option<String> }
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ComposerView {
    pub handle: u64, pub key: String, pub loaded: bool, pub rev: u64, pub text_epoch: u64, pub text: String,
    pub position: i64, pub anchor: i64, pub attachments: Vec<AttachmentView>, pub removed: Option<String>,
    pub attaching: bool, pub sending: bool, pub waiting: Option<String>, pub uncertain: bool,
    pub status: String, pub status_error: bool, pub can_send: bool, pub send_label: String,
}
// pump.rs
impl Pump { pub fn poke(&self); }
// commands: composer_open(session: Option<SessionRef>) -> Option<ComposerView>; composer_edit(handle, rev, text, position, anchor);
// composer_pick(handle, start, end); composer_attach(handle, paths, start, end); composer_paste_image(raw PNG; x-zerus-composer, x-zerus-start, x-zerus-end);
// composer_remove(handle, index); composer_undo_remove(handle); composer_allow_retry(handle)
// next_batch: {...batch, composer?: ComposerView, draft_keys?: string[]}
```

`AttachmentView.path` is `draft/{handle}/{list}/{index}`: the composer, the revision of its attachment list, and the file's place. Task 3b serves it through the `zerus-file` scheme. No digest is computed per view: a new list (an attach, remove, undo, load or send) gets new paths, so an old URL never shows another file.

- [ ] **Step 1: Write the failing hub tests** at the end of `client/src-tauri/src/composer/mod.rs`:

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use zerus_client_core::drafts::{read_draft, StoredDraft};

    pub(crate) const KEY: &str = "studio\nclaude/sample-project/review";

    pub(crate) fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-composer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The test's draft store under `root/drafts`. A test that drops its hub
    /// and opens another keeps this, as the app's `Drafts` does: a store must
    /// never drop on its own writer thread.
    pub(crate) fn drafts(root: &Path) -> Arc<DraftStore> {
        Arc::new(DraftStore::open(root.join("drafts")))
    }

    pub(crate) fn hub_on(store: &Arc<DraftStore>) -> (Composers, Arc<AtomicUsize>) {
        let notified = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&notified);
        let composers = Composers::new(
            Arc::clone(store),
            Deps {
                spawn: Arc::new(|job| { std::thread::spawn(job); }),
                notify: Arc::new(move || { count.fetch_add(1, Ordering::SeqCst); }),
            },
        );
        (composers, notified)
    }

    pub(crate) fn hub(root: &Path) -> (Composers, Arc<AtomicUsize>) {
        hub_on(&drafts(root))
    }

    pub(crate) fn session() -> SessionRef {
        SessionRef { computer: "studio".into(), key: "run:run-1".into(), name: "claude/sample-project/review".into(), archive_id: None }
    }

    /// The open composer's view once `done` holds, within five seconds.
    pub(crate) fn until(c: &Composers, done: impl Fn(&ComposerView) -> bool) -> ComposerView {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let view = c.current().expect("a composer is open");
            if done(&view) { return view; }
            assert!(Instant::now() < deadline, "timed out: {view:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn draft_keys_are_qts() {
        assert_eq!(draft_key("", "claude/p/a", None), "\nclaude/p/a");
        assert_eq!(draft_key("studio", "claude/p/a", Some("a-1")), "studio\nclaude/p/a\na-1");
    }

    #[test]
    fn a_draft_survives_reopening() {
        let root = dir("reopen");
        let store = drafts(&root);
        let (c, _) = hub_on(&store);
        let view = c.open(Some(session())).unwrap();
        let view = until(&c, |v| v.loaded && v.handle == view.handle);
        c.edit(view.handle, 1, "hello".into(), 5, 5);
        c.flush();
        store.flush();
        drop(c);
        // A new hub reads the draft back from the disk.
        let (again, _) = hub_on(&store);
        again.open(Some(session()));
        let view = until(&again, |v| v.loaded);
        assert_eq!((view.text.as_str(), view.position), ("hello", 5));
    }

    #[test]
    fn quit_flushes_drafts() {
        let root = dir("flush");
        let store = drafts(&root);
        let (c, _) = hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "typed before quitting".into(), 0, 0);
        // The exit hook's order: the hub closes, then the store writes everything queued.
        c.flush();
        store.flush();
        assert_eq!(read_draft(&root.join("drafts"), KEY).draft.draft.text, "typed before quitting");
        c.edit(view.handle, 2, "after the exit".into(), 0, 0);
        assert_eq!(c.current().unwrap().text, "typed before quitting", "a closed hub takes no edits");
    }

    #[test]
    fn a_sending_draft_reloads_uncertain() {
        let root = dir("sending");
        let store = DraftStore::open(root.join("drafts"));
        let mut stored = StoredDraft { sending: true, ..StoredDraft::default() };
        stored.draft.text = "maybe delivered".into();
        store.save_durable(KEY, stored, |_| {});
        store.flush();
        drop(store);
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert!(view.uncertain);
        assert_eq!(view.status, "Delivery was interrupted. Check Activity or Terminal before sending again.");
        assert!(view.status_error && !view.can_send);
        c.allow_retry(view.handle);
        let view = until(&c, |v| !v.uncertain);
        // Send's other conditions (the session's state) arrive with Task 4a.
        assert!(!view.status_error);
    }

    #[test]
    fn stale_edits_are_ignored() {
        let root = dir("stale");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 2, "newer".into(), 0, 0);
        c.edit(view.handle, 1, "older".into(), 0, 0);
        assert_eq!(c.current().unwrap().text, "newer");
    }

    #[test]
    fn over_64_kib_disables_send() {
        let root = dir("long");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "x".repeat(64 * 1024 + 1), 0, 0);
        let view = c.current().unwrap();
        assert_eq!(view.status, "Message exceeds 64 KiB.");
        assert!(view.status_error && !view.can_send);
    }

    #[test]
    fn attaching_inserts_the_reference_and_saves_it() {
        let root = dir("attach");
        let file = root.join("notes.txt");
        std::fs::write(&file, "first line\nsecond line\n").unwrap();
        let store = drafts(&root);
        let (c, _) = hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "see".into(), 3, 3);
        c.attach_paths(view.handle, vec![file], Selection { start: 3, end: 3 });
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        assert_eq!(view.text, "see [File #1] ");
        assert!(view.text_epoch > 0);
        let attachment = &view.attachments[0];
        assert_eq!((attachment.name.as_str(), attachment.mime.as_str()), ("notes.txt", "text/plain"));
        assert_eq!(attachment.excerpt.as_deref(), Some("first line\nsecond line"));
        assert!(attachment.path.starts_with(&format!("draft/{}/", view.handle)));
        store.flush();
        assert_eq!(read_draft(&root.join("drafts"), KEY).draft.draft.attachments.len(), 1);
    }

    #[test]
    fn a_huge_pasted_image_is_refused() {
        let root = dir("paste");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        // A PNG header of 8000 x 5000 pixels: 40 megapixels.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(8000u32.to_be_bytes());
        png.extend(5000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        c.paste_image(view.handle, png, Selection::default());
        let view = until(&c, |v| !v.attaching);
        assert!(view.attachments.is_empty());
        assert_eq!(view.status, "Image is too large. Use an image below 32 megapixels.");
    }

    #[test]
    fn remove_drops_the_reference_and_undo_restores_the_file() {
        let root = dir("remove");
        let file = root.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.attach_paths(view.handle, vec![file], Selection::default());
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        let first_path = view.attachments[0].path.clone();
        c.remove(view.handle, 0);
        let view = c.current().unwrap();
        assert!(view.attachments.is_empty() && !view.text.contains("[File #1]"));
        assert_eq!(view.removed.as_deref(), Some("a.txt"));
        c.undo_remove(view.handle);
        let view = c.current().unwrap();
        assert_eq!(view.attachments.len(), 1);
        assert!(view.text.contains("[File #1]") && view.removed.is_none());
        assert_ne!(view.attachments[0].path, first_path, "a new list gets new paths");
    }

    #[test]
    fn a_rename_moves_the_draft() {
        let root = dir("rename");
        let store = drafts(&root);
        let (c, _) = hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "kept across a rename".into(), 0, 0);
        let mut renamed = session();
        renamed.name = "claude/sample-project/review-2".into();
        c.open(Some(renamed));
        let view = until(&c, |v| v.key == "studio\nclaude/sample-project/review-2" && v.loaded);
        assert_eq!(view.text, "kept across a rename");
        store.flush();
        assert_eq!(read_draft(&root.join("drafts"), KEY).draft.draft.text, "");
    }

    #[test]
    fn a_rename_onto_an_existing_draft_keeps_both() {
        let root = dir("rename-collision");
        let store = drafts(&root);
        let mut there = StoredDraft::default();
        there.draft.text = "already here".into();
        store.save_durable("studio\nclaude/sample-project/review-2", there, |_| {});
        let (c, _) = hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "moving".into(), 0, 0);
        let mut renamed = session();
        renamed.name = "claude/sample-project/review-2".into();
        c.open(Some(renamed));
        let view = until(&c, |v| v.loaded && v.text == "already here");
        assert_eq!(view.status, zerus_client_core::drafts::ALREADY_EXISTS);
        store.flush();
        assert_eq!(read_draft(&root.join("drafts"), KEY).draft.draft.text, "moving", "the moved draft stays for Saved drafts");
        assert_eq!(read_draft(&root.join("drafts"), "studio\nclaude/sample-project/review-2").draft.draft.text, "already here");
    }

    #[test]
    fn draft_keys_follow_nonempty_drafts() {
        let root = dir("keys");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        let _ = c.take_draft_keys();
        c.edit(view.handle, 1, "x".into(), 1, 1);
        assert_eq!(c.take_draft_keys(), Some(vec![KEY.to_owned()]));
        assert_eq!(c.take_draft_keys(), None, "unchanged keys are not sent again");
        c.edit(view.handle, 2, String::new(), 0, 0);
        assert_eq!(c.take_draft_keys(), Some(vec![]));
    }

    #[test]
    fn a_view_is_taken_once_per_change() {
        let root = dir("take");
        let (c, notified) = hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        assert!(c.take_view().is_some());
        assert!(c.take_view().is_none());
        let before = notified.load(Ordering::SeqCst);
        let handle = c.current().unwrap().handle;
        c.edit(handle, 1, "y".into(), 1, 1);
        assert!(notified.load(Ordering::SeqCst) > before, "a change wakes the pump");
        assert_eq!(c.take_view().unwrap().text, "y");
        c.resend();
        assert!(c.take_view().is_some(), "a reloaded webview gets the view again");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::`
Expected: compile errors, the module does not exist.

- [ ] **Step 3: Write `client/src-tauri/src/composer/mod.rs`** above its tests:

```rust
//! The composer hub, ported from `tray/src/MessageComposer.cpp`: the open
//! session's draft (A1's `Draft`, held only here, so attachment bytes never
//! cross the IPC as JSON), its durable copy in the app's one draft store (S5,
//! shared with voice), the attachment list, the status line and Send's
//! enablement. The webview gets a `ComposerView` through `next_batch`
//! whenever it changed.
//!
//! Text protocol: the webview owns the text while the user types and sends
//! `composer_edit` with a growing `rev`; Rust applies only a higher one. When
//! Rust changes the text itself (a load, an attach or remove, a delivery's
//! end, a restore), it bumps `text_epoch` and the webview takes `text`.
//!
//! Bounded: at most 64 composers and 128 MiB of their attachments in memory
//! (the open one and a busy one are never dropped; a dropped one reloads
//! from disk); the draft keys the cards mark come from the 200 newest drafts
//! plus the composers held.
//!
//! The hub never holds the last `Arc<DraftStore>`: the app's `Drafts` keeps
//! one for the life of the process, and tests keep theirs, since a store
//! dropped on its own writer thread would wait for itself.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zerus_client_core::attachments::{self, Attachment, Draft, FileRead, Selection};
use zerus_client_core::drafts::{DraftStore, IndexPage, Loaded, StoredDraft, ALREADY_EXISTS};

pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn() + Send + Sync>;

const COMPOSERS: usize = 64;
/// Attachment bytes the idle composers in memory may hold; their drafts stay on disk.
const HELD_BYTES: usize = 128 * 1024 * 1024;
const DRAFT_KEYS: usize = 200;
/// Message text, in UTF-8 bytes (`MaxTextBytes`).
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
const EXCERPT_LINES: usize = 12;
const EXCERPT_CHARS: usize = 1200;
/// How long the exit waits for attach reads under way.
const EXIT_WAIT: Duration = Duration::from_secs(2);

pub struct Deps {
    pub spawn: Spawn,
    pub notify: Notify,
}

/// The session a composer belongs to, as the webview opened it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    pub computer: String,
    /// The catalog key (`run:...` or `name:...`).
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub archive_id: Option<String>,
}

/// Qt's draft key: `machine\nsession[\narchive]` (`SessionsWindow.cpp:438`).
pub fn draft_key(computer: &str, name: &str, archive: Option<&str>) -> String {
    match archive.filter(|id| !id.is_empty()) {
        Some(archive) => format!("{computer}\n{name}\n{archive}"),
        None => format!("{computer}\n{name}"),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AttachmentView {
    pub name: String,
    pub mime: String,
    pub reference: String,
    pub bytes: u64,
    /// The `zerus-file` path of its bytes (`draft/{handle}/{list}/{index}`,
    /// Task 3b); the webview turns it into a URL with `convertFileSrc`.
    pub path: String,
    /// The first lines of a text file, for the preview.
    pub excerpt: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ComposerView {
    pub handle: u64,
    pub key: String,
    pub loaded: bool,
    pub rev: u64,
    pub text_epoch: u64,
    pub text: String,
    pub position: i64,
    pub anchor: i64,
    pub attachments: Vec<AttachmentView>,
    /// The file the last removal took out, while Undo can restore it.
    pub removed: Option<String>,
    pub attaching: bool,
    pub sending: bool,
    pub waiting: Option<String>,
    pub uncertain: bool,
    pub status: String,
    pub status_error: bool,
    pub can_send: bool,
    pub send_label: String,
}

/// One composer's draft and state (`MessageComposer::Draft` plus its sets).
pub(crate) struct Composer {
    pub(crate) handle: u64,
    pub(crate) key: String,
    pub(crate) loaded: bool,
    pub(crate) draft: Draft,
    pub(crate) position: i64,
    pub(crate) anchor: i64,
    pub(crate) label: String,
    pub(crate) form_state: Map<String, Value>,
    pub(crate) rev: u64,
    pub(crate) text_epoch: u64,
    pub(crate) sending: bool,
    pub(crate) waiting: Option<String>,
    pub(crate) uncertain: bool,
    pub(crate) error: bool,
    pub(crate) notice: String,
    pub(crate) storage_error: Option<String>,
    pub(crate) removed: Option<(usize, Attachment)>,
    pub(crate) attaching: bool,
    /// A rename's store call runs: the composer is locked, as Qt's loading keys.
    pub(crate) renaming: bool,
    /// Bumps whenever the attachment list changes; the `zerus-file` paths name it.
    pub(crate) attachments_rev: u64,
}

impl Composer {
    fn new(handle: u64, key: String) -> Self {
        Composer {
            handle, key, loaded: false, draft: Draft::default(), position: 0, anchor: 0, label: String::new(),
            form_state: Map::new(), rev: 0, text_epoch: 0, sending: false, waiting: None, uncertain: false,
            error: false, notice: String::new(), storage_error: None, removed: None, attaching: false,
            renaming: false, attachments_rev: 0,
        }
    }

    pub(crate) fn has_content(&self) -> bool {
        !self.draft.text.trim().is_empty() || !self.draft.attachments.is_empty()
    }

    pub(crate) fn busy(&self) -> bool {
        self.sending || self.waiting.is_some() || self.attaching || self.renaming
    }

    pub(crate) fn stored(&self) -> StoredDraft {
        StoredDraft {
            draft: self.draft.clone(),
            position: self.position,
            anchor: self.anchor,
            label: self.label.clone(),
            form_state: self.form_state.clone(),
            sending: self.sending,
            uncertain: self.uncertain,
        }
    }

    /// Qt's `showError`: the status line shows it in the error colour.
    pub(crate) fn show_error(&mut self, text: impl Into<String>) {
        self.notice = text.into();
        self.error = true;
    }

    /// The attachment list changed: its files get new `zerus-file` paths.
    pub(crate) fn attachments_changed(&mut self) {
        self.attachments_rev += 1;
    }
}

#[derive(Default)]
pub(crate) struct Hub {
    pub(crate) composers: HashMap<String, Composer>,
    /// Least recently opened first.
    order: VecDeque<String>,
    pub(crate) open: Option<SessionRef>,
    pub(crate) open_key: Option<String>,
    next_handle: u64,
    last_view: Option<ComposerView>,
    /// Keys of the newest stored drafts, newest first, at most 200.
    stored_keys: VecDeque<String>,
    sent_keys: Option<Vec<String>>,
    /// The app quits: no edit, attach or send starts any more.
    pub(crate) closed: bool,
}

pub(crate) struct Shared {
    pub(crate) hub: Mutex<Hub>,
    pub(crate) store: Arc<DraftStore>,
    pub(crate) deps: Deps,
}

/// The managed state of the composer commands; clones share it.
#[derive(Clone)]
pub struct Composers(pub(crate) Arc<Shared>);

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn excerpt(attachment: &Attachment) -> Option<String> {
    let text = attachment.mime.starts_with("text/") || attachment.mime == "application/json";
    text.then(|| {
        let head = String::from_utf8_lossy(&attachment.bytes[..attachment.bytes.len().min(EXCERPT_CHARS * 4)]).into_owned();
        let lines: Vec<&str> = head.lines().take(EXCERPT_LINES).collect();
        lines.join("\n").chars().take(EXCERPT_CHARS).collect()
    })
}

impl Composers {
    /// `drafts` is the app's one draft store (`Drafts` in lib.rs), shared
    /// with voice, so every draft key has one writer.
    pub fn new(drafts: Arc<DraftStore>, deps: Deps) -> Self {
        let composers = Composers(Arc::new(Shared { hub: Mutex::new(Hub::default()), store: drafts, deps }));
        composers.load_stored_keys();
        composers
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Hub> {
        lock(&self.0.hub)
    }

    /// Wakes the pump: the view or the draft keys changed.
    pub(crate) fn changed(&self) {
        (self.0.deps.notify)();
    }

    /// Keys of the 200 newest nonempty drafts, for the cards' draft mark.
    fn load_stored_keys(&self) {
        let me = self.clone();
        self.0.store.index(IndexPage { limit: DRAFT_KEYS, before: None }, move |page| {
            me.lock().stored_keys = page.items.into_iter().map(|(key, _)| key).collect();
            me.changed();
        });
    }

    /// Opens the selected session's composer (None closes it). The same
    /// catalog row under a new name is a rename: its draft moves (`renameDraft`).
    pub fn open(&self, session: Option<SessionRef>) -> Option<ComposerView> {
        let mut hub = self.lock();
        let Some(session) = session else {
            hub.open = None;
            hub.open_key = None;
            drop(hub);
            self.changed();
            return None;
        };
        let key = draft_key(&session.computer, &session.name, session.archive_id.as_deref());
        let renamed = hub.open.as_ref().filter(|old| {
            old.computer == session.computer && old.key == session.key && old.name != session.name
        }).and_then(|_| hub.open_key.clone());
        hub.open = Some(session);
        hub.open_key = Some(key.clone());
        if let Some(old) = renamed {
            self.rename(&mut hub, &old, &key);
        }
        self.ensure(&mut hub, &key);
        hub.order.retain(|k| k != &key);
        hub.order.push_back(key.clone());
        self.trim(&mut hub);
        let view = self.view_of(&hub, &key);
        drop(hub);
        self.changed();
        view
    }

    /// Loads a composer that is not in memory (`ensureDraftLoaded`).
    fn ensure(&self, hub: &mut Hub, key: &str) {
        if hub.composers.contains_key(key) {
            return;
        }
        hub.next_handle += 1;
        let composer = Composer::new(hub.next_handle, key.to_owned());
        let handle = composer.handle;
        hub.composers.insert(key.to_owned(), composer);
        let me = self.clone();
        let owned = key.to_owned();
        self.0.store.load(key, move |loaded| me.loaded(&owned, handle, loaded));
    }

    fn loaded(&self, key: &str, handle: u64, loaded: Loaded) {
        let mut hub = self.lock();
        let Some(composer) = hub.composers.get_mut(key).filter(|c| c.handle == handle && !c.loaded) else { return };
        let stored = loaded.draft;
        composer.loaded = true;
        composer.draft = stored.draft;
        composer.position = stored.position;
        composer.anchor = stored.anchor;
        composer.label = stored.label;
        composer.form_state = stored.form_state;
        composer.uncertain = stored.uncertain || stored.sending || loaded.notice.is_some();
        if let Some(notice) = loaded.notice {
            composer.show_error(notice);
        }
        composer.storage_error = loaded.storage_error;
        composer.text_epoch += 1;
        composer.attachments_changed();
        drop(hub);
        self.changed();
    }

    /// `renameDraft`: the draft moves once the store has it at the new key,
    /// and the composer stays locked until then. A draft already at the new
    /// key is never overwritten (S5's `ALREADY_EXISTS`): it loads instead,
    /// and the moved draft stays under its old key for Saved drafts.
    fn rename(&self, hub: &mut Hub, old: &str, new: &str) {
        let Some(composer) = hub.composers.get(old) else { return };
        if composer.busy() || !composer.loaded || composer.storage_error.is_some() {
            return;
        }
        let stored = composer.stored();
        if let Some(here) = hub.composers.get_mut(new) {
            if here.busy() || !here.loaded {
                return;
            }
            if here.has_content() || here.storage_error.is_some() {
                here.show_error(ALREADY_EXISTS);
                return;
            }
            hub.composers.remove(new);
        }
        let mut moved = hub.composers.remove(old).expect("checked above");
        moved.key = new.to_owned();
        moved.renaming = true;
        hub.composers.insert(new.to_owned(), moved);
        let me = self.clone();
        let (from, to) = (old.to_owned(), new.to_owned());
        self.0.store.rename(old, new, stored, move |result| me.renamed(&from, &to, result));
    }

    fn renamed(&self, old: &str, new: &str, result: Result<(), String>) {
        let mut hub = self.lock();
        match result {
            Ok(()) => {
                if let Some(composer) = hub.composers.get_mut(new) {
                    composer.renaming = false;
                }
            }
            Err(error) => {
                if let Some(mut moved) = hub.composers.remove(new) {
                    moved.key = old.to_owned();
                    moved.renaming = false;
                    hub.composers.insert(old.to_owned(), moved);
                }
                if hub.open_key.as_deref() == Some(new) {
                    self.ensure(&mut hub, new);
                    if let Some(here) = hub.composers.get_mut(new) {
                        here.show_error(error);
                    }
                }
            }
        }
        drop(hub);
        self.changed();
    }

    /// Drops least recently opened composers past the bounds (`trimDraftCache`,
    /// plus a byte bound Qt's unbounded cache lacks); a dropped composer's key
    /// joins or leaves the stored keys, so its card's draft mark stays right.
    fn trim(&self, hub: &mut Hub) {
        let held = |hub: &Hub| -> usize {
            hub.composers.values().map(|c| {
                c.draft.attachments.iter().map(|a| a.bytes.len()).sum::<usize>() + c.removed.as_ref().map_or(0, |(_, a)| a.bytes.len())
            }).sum()
        };
        while hub.composers.len() > COMPOSERS || held(hub) > HELD_BYTES {
            let open = hub.open_key.clone();
            let Some(index) = hub.order.iter().position(|key| {
                Some(key) != open.as_ref() && hub.composers.get(key).is_none_or(|c| c.loaded && !c.busy())
            }) else { break };
            let key = hub.order.remove(index).expect("found above");
            if let Some(gone) = hub.composers.remove(&key) {
                hub.stored_keys.retain(|k| k != &key);
                if gone.has_content() {
                    hub.stored_keys.push_front(key);
                    hub.stored_keys.truncate(DRAFT_KEYS);
                }
            }
        }
    }

    pub(crate) fn by_handle(hub: &mut Hub, handle: u64) -> Option<&mut Composer> {
        hub.composers.values_mut().find(|c| c.handle == handle)
    }

    /// The composer of `handle` when it takes a change: loaded, not busy, and
    /// the app not quitting.
    pub(crate) fn editable(hub: &mut Hub, handle: u64) -> Option<&mut Composer> {
        if hub.closed {
            return None;
        }
        Self::by_handle(hub, handle).filter(|c| c.loaded && !c.busy())
    }

    /// Saves as the user types; a later save of the same key replaces it.
    pub(crate) fn save(&self, composer: &Composer) {
        let me = self.clone();
        let (key, handle) = (composer.key.clone(), composer.handle);
        self.0.store.save(&composer.key, composer.stored(), move |result| me.saved(&key, handle, result));
    }

    fn saved(&self, key: &str, handle: u64, result: Result<(), String>) {
        let mut hub = self.lock();
        if let Some(composer) = hub.composers.get_mut(key).filter(|c| c.handle == handle) {
            composer.storage_error = result.err();
        }
        drop(hub);
        self.changed();
    }

    /// The editor changed (`MessageComposer.cpp:370-379`).
    pub fn edit(&self, handle: u64, rev: u64, text: String, position: i64, anchor: i64) {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return };
        if rev <= composer.rev {
            return;
        }
        composer.rev = rev;
        composer.draft.text = text;
        composer.position = position;
        composer.anchor = anchor;
        if !composer.error && !composer.uncertain {
            composer.notice.clear();
        }
        self.save(composer);
        drop(hub);
        self.changed();
    }

    /// Attaches chosen or dropped files, one after another, off the UI thread.
    pub fn attach_paths(&self, handle: u64, paths: Vec<PathBuf>, selection: Selection) {
        if !self.begin_attach(handle) {
            return;
        }
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let mut at = selection;
            for path in paths {
                let read = attachments::read_file(&path);
                if !me.add(handle, read, &mut at) {
                    break;
                }
            }
            me.end_attach(handle);
        }));
    }

    /// A pasted image (`attachImage`): PNG bytes from a raw IPC body, checked in the core.
    pub fn paste_image(&self, handle: u64, png: Vec<u8>, selection: Selection) {
        if !self.begin_attach(handle) {
            return;
        }
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let mut at = selection;
            me.add(handle, attachments::pasted_image(png), &mut at);
            me.end_attach(handle);
        }));
    }

    fn begin_attach(&self, handle: u64) -> bool {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return false };
        composer.attaching = true;
        drop(hub);
        self.changed();
        true
    }

    /// `addAttachment`: false when it failed, which ends the batch as in Qt.
    fn add(&self, handle: u64, read: Result<FileRead, String>, at: &mut Selection) -> bool {
        let mut hub = self.lock();
        let Some(composer) = Self::by_handle(&mut hub, handle) else { return false };
        let added = read.and_then(|file| composer.draft.add(&file.name, &file.mime, file.bytes, *at));
        let ok = match added {
            Ok(added) => {
                *at = Selection { start: added.cursor, end: added.cursor };
                composer.position = added.cursor as i64;
                composer.anchor = added.cursor as i64;
                composer.text_epoch += 1;
                composer.attachments_changed();
                if !composer.uncertain {
                    composer.notice.clear();
                    composer.error = false;
                }
                self.save(composer);
                true
            }
            Err(error) => {
                composer.show_error(error);
                false
            }
        };
        drop(hub);
        self.changed();
        ok
    }

    fn end_attach(&self, handle: u64) {
        let mut hub = self.lock();
        if let Some(composer) = Self::by_handle(&mut hub, handle) {
            composer.attaching = false;
        }
        drop(hub);
        self.changed();
    }

    /// `removeAttachment`: the file and every mention of its reference.
    pub fn remove(&self, handle: u64, index: usize) {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return };
        if let Some(removed) = composer.draft.remove(index) {
            composer.removed = Some((index, removed));
            composer.text_epoch += 1;
            composer.attachments_changed();
            self.save(composer);
        }
        drop(hub);
        self.changed();
    }

    /// Undo in the attachment list (the approved mockup): the file returns
    /// to its place and its reference to the end of the text.
    pub fn undo_remove(&self, handle: u64) {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return };
        if composer.draft.attachments.len() >= attachments::MAX_ATTACHMENTS {
            return;
        }
        let Some((index, attachment)) = composer.removed.take() else { return };
        if !attachment.reference.is_empty() {
            let text = &mut composer.draft.text;
            if text.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
                text.push(' ');
            }
            text.push_str(&attachment.reference);
            text.push(' ');
        }
        let at = index.min(composer.draft.attachments.len());
        composer.draft.attachments.insert(at, attachment);
        composer.text_epoch += 1;
        composer.attachments_changed();
        self.save(composer);
        drop(hub);
        self.changed();
    }

    /// Allow retry (`MessageComposer.cpp:412-417`): clears an uncertain mark.
    pub fn allow_retry(&self, handle: u64) {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return };
        composer.uncertain = false;
        composer.error = false;
        composer.notice.clear();
        self.save(composer);
        drop(hub);
        self.changed();
    }

    /// The open composer's view (tests).
    #[cfg(test)]
    pub(crate) fn current(&self) -> Option<ComposerView> {
        let hub = self.lock();
        let key = hub.open_key.clone()?;
        self.view_of(&hub, &key)
    }

    /// The open composer's view when it changed since the last take.
    pub fn take_view(&self) -> Option<ComposerView> {
        let mut hub = self.lock();
        let key = hub.open_key.clone()?;
        let view = self.view_of(&hub, &key)?;
        if hub.last_view.as_ref() == Some(&view) {
            return None;
        }
        hub.last_view = Some(view.clone());
        Some(view)
    }

    /// The keys of nonempty drafts, when they changed since the last take:
    /// the 200 newest stored ones and the composers held (at most 264).
    pub fn take_draft_keys(&self) -> Option<Vec<String>> {
        let mut hub = self.lock();
        let mut keys: BTreeSet<String> = hub.stored_keys.iter().cloned().collect();
        for (key, composer) in &hub.composers {
            if composer.loaded && composer.has_content() && !composer.sending {
                keys.insert(key.clone());
            } else if composer.loaded {
                keys.remove(key);
            }
        }
        let keys: Vec<String> = keys.into_iter().collect();
        if hub.sent_keys.as_ref() == Some(&keys) {
            return None;
        }
        hub.sent_keys = Some(keys.clone());
        Some(keys)
    }

    /// A reloaded webview gets the view and the draft keys again.
    pub fn resend(&self) {
        let mut hub = self.lock();
        hub.last_view = None;
        hub.sent_keys = None;
    }

    /// The app quits (`exit` in lib.rs, before the store's own flush): no
    /// edit, attach or send starts any more, and attach reads under way end
    /// first (at most 2 s), so everything the hub accepted is queued in the
    /// store when the store flushes. A send whose "sending" save is still
    /// queued never starts; it reloads as uncertain.
    pub fn flush(&self) {
        self.lock().closed = true;
        let deadline = Instant::now() + EXIT_WAIT;
        while self.lock().composers.values().any(|c| c.attaching) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The status line and Send, by Qt's `updateControls` priority. Task 4a
    /// adds the session's block reason and the cache chip.
    pub(crate) fn view_of(&self, hub: &Hub, key: &str) -> Option<ComposerView> {
        let composer = hub.composers.get(key)?;
        let too_long = composer.draft.text.len() > MAX_TEXT_BYTES;
        let (status, status_error) = if let Some(error) = &composer.storage_error {
            (error.clone(), true)
        } else if let Some(waiting) = &composer.waiting {
            (waiting.clone(), false)
        } else if composer.error || composer.uncertain {
            (composer.notice.clone(), true)
        } else if too_long {
            ("Message exceeds 64 KiB.".to_owned(), true)
        } else {
            (String::new(), false)
        };
        let locked = composer.busy() || !composer.loaded;
        Some(ComposerView {
            handle: composer.handle,
            key: composer.key.clone(),
            loaded: composer.loaded && !composer.renaming,
            rev: composer.rev,
            text_epoch: composer.text_epoch,
            text: if composer.sending { String::new() } else { composer.draft.text.clone() },
            position: composer.position,
            anchor: composer.anchor,
            attachments: if composer.sending { Vec::new() } else {
                composer.draft.attachments.iter().enumerate().map(|(index, a)| AttachmentView {
                    name: a.name.clone(), mime: a.mime.clone(), reference: a.reference.clone(), bytes: a.bytes.len() as u64,
                    path: format!("draft/{}/{}/{index}", composer.handle, composer.attachments_rev),
                    excerpt: excerpt(a),
                }).collect()
            },
            removed: composer.removed.as_ref().map(|(_, a)| a.name.clone()),
            attaching: composer.attaching,
            sending: composer.sending,
            waiting: composer.waiting.clone(),
            uncertain: composer.uncertain,
            status,
            status_error,
            can_send: composer.loaded && composer.has_content() && !too_long && !locked && !composer.uncertain,
            send_label: if composer.sending { "Sending…" } else if composer.waiting.is_some() { "Waiting to send…" } else { "Send" }.into(),
        })
    }
}
```

The commands, at the end of `mod.rs` (before the tests):

```rust
use tauri::ipc::{InvokeBody, Request};
use tauri::State;

#[tauri::command]
pub fn composer_open(state: State<'_, Composers>, session: Option<SessionRef>) -> Option<ComposerView> {
    state.open(session)
}

#[tauri::command(rename_all = "snake_case")]
pub fn composer_edit(state: State<'_, Composers>, handle: u64, rev: u64, text: String, position: i64, anchor: i64) {
    state.edit(handle, rev, text, position, anchor);
}

/// Attach files: the native picker (`attachFiles`), then the reads.
#[tauri::command(rename_all = "snake_case")]
pub async fn composer_pick(app: tauri::AppHandle, state: State<'_, Composers>, handle: u64, start: usize, end: usize) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let composers = state.inner().clone();
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().set_title("Attach files").blocking_pick_files())
        .await
        .map_err(|error| error.to_string())?;
    let paths: Vec<PathBuf> = picked.unwrap_or_default().into_iter().filter_map(|file| file.into_path().ok()).collect();
    if !paths.is_empty() {
        composers.attach_paths(handle, paths, Selection { start, end });
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub fn composer_attach(state: State<'_, Composers>, handle: u64, paths: Vec<PathBuf>, start: usize, end: usize) {
    state.attach_paths(handle, paths, Selection { start, end });
}

/// A pasted image as a raw PNG body: `x-zerus-composer` is the handle,
/// `x-zerus-start` and `x-zerus-end` the selection in UTF-16 units.
#[tauri::command]
pub async fn composer_paste_image(state: State<'_, Composers>, request: Request<'_>) -> Result<(), String> {
    let number = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let handle = number("x-zerus-composer").ok_or("missing x-zerus-composer")?;
    let start = number("x-zerus-start").unwrap_or(0) as usize;
    let end = number("x-zerus-end").unwrap_or(start as u64) as usize;
    let InvokeBody::Raw(png) = request.body() else { return Err("a pasted image must be a raw body".into()) };
    state.paste_image(handle, png.clone(), Selection { start, end });
    Ok(())
}

#[tauri::command]
pub fn composer_remove(state: State<'_, Composers>, handle: u64, index: usize) {
    state.remove(handle, index);
}

#[tauri::command]
pub fn composer_undo_remove(state: State<'_, Composers>, handle: u64) {
    state.undo_remove(handle);
}

#[tauri::command]
pub fn composer_allow_retry(state: State<'_, Composers>, handle: u64) {
    state.allow_retry(handle);
}
```

- [ ] **Step 4: Extend the pump** in `client/src-tauri/src/pump.rs`:

```rust
    /// Wakes a parked `take` without a change: a hub's view changed.
    pub fn poke(&self) {
        self.lock().dirty = true;
        self.0 .1.notify_all();
    }
```

with the test:

```rust
    #[test]
    fn a_poke_wakes_a_parked_take() {
        let pump = Pump::default();
        let _ = pump.take(Duration::ZERO);
        let waker = pump.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(30)); waker.poke(); });
        let started = Instant::now();
        let batch = pump.take(Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(2) && batch.is_empty());
    }
```

- [ ] **Step 5: The envelope** in `client/src-tauri/src/commands.rs`. `next_batch` takes `composers: State<'_, crate::composer::Composers>` and serializes:

```rust
/// What `next_batch` returns: the core's batch, plus the hubs' projected views
/// when they changed (latest value wins).
#[derive(serde::Serialize)]
struct Envelope<'a> {
    #[serde(flatten)]
    batch: &'a zerus_client_core::Batch,
    #[serde(skip_serializing_if = "Option::is_none")]
    composer: Option<crate::composer::ComposerView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_keys: Option<Vec<String>>,
}
```

```rust
    let composers = composers.inner().clone();
    let bytes = blocking(move || {
        let batch = live.pump.take(crate::pump::PARK);
        actions.observe(&batch);
        let envelope = Envelope { batch: &batch, composer: composers.take_view(), draft_keys: composers.take_draft_keys() };
        serde_json::to_vec(&envelope).map_err(|error| error.to_string())
    })
    .await?;
```

The `start` command takes the composers too and calls `composers.resend()` beside `pump.resend()`, so a reloaded webview gets the view and the keys again.

- [ ] **Step 6: Register it** in `client/src-tauri/src/lib.rs`.

**The one draft store.** M1c opens the app's one `Arc<DraftStore>` in `setup` and manages it as `Drafts`, so the composers and voice share one writer per store. If M1c is on main, use its `Drafts` as it is. If it is not, this task adds exactly M1c's struct and lines, and M1c rebases onto them:

```rust
/// The app's one draft store, shared by the composers (and voice), so every
/// draft key has one writer.
#[cfg(feature = "live")]
pub struct Drafts(pub std::sync::Arc<zerus_client_core::drafts::DraftStore>);
```

```rust
                // in setup, after the actions: a replay never touches the real drafts either.
                let drafts = std::sync::Arc::new(zerus_client_core::drafts::DraftStore::open(
                    dir.join(zerus_client_core::drafts::dir(replay)),
                ));
                app.manage(Drafts(drafts));
```

Without M1c's core change, `drafts::dir(replay)` is not there yet: use `dir.join(if replay { "drafts-replay" } else { "drafts" })`, the values of M1c's `drafts::REPLAY_DIR` and `drafts::DIR`, and M1c's rebase replaces the literal with the call.

**The composers**, after the `Drafts` lines (M1c's or these), taking the store from the managed state so they work with either:

```rust
                let drafts = std::sync::Arc::clone(&app.state::<Drafts>().0);
                let notify_pump = app.state::<commands::AppState>().pump();
                app.manage(composer::Composers::new(
                    drafts,
                    composer::Deps {
                        spawn: std::sync::Arc::new(|job| { tauri::async_runtime::spawn_blocking(job); }),
                        notify: std::sync::Arc::new(move || notify_pump.poke()),
                    },
                ));
                #[cfg(unix)]
                exit_on_signals(app.handle().clone());
```

Add `#[cfg(feature = "live")] mod composer;`, the builder's `.plugin(tauri_plugin_dialog::init())`, and the eight composer commands in the live handler.

**The one exit hook.** M1c turns `run()`'s end into `.build(tauri::generate_context!()).expect(...).run(|_app, event| { if let tauri::RunEvent::Exit = event { #[cfg(feature = "live")] exit(_app); } })`, keeping main's `expect` text. If M1c is not on main, this task makes that change. Either way there is one `exit`, and whoever lands second adds its lines to it, never a second hook. The organization branch adds `Organization::flush()` to it the same way. This task adds the composers' flush before the store's:

```rust
/// Before the app quits: recordings under way are cancelled, the hubs take
/// no more changes, and every draft save already queued reaches the disk.
#[cfg(feature = "live")]
fn exit(app: &AppHandle) {
    use tauri::Manager;
    #[cfg(feature = "media")]
    if let Some(voices) = app.try_state::<voice::Voices>() {
        voices.close_all();
    }
    // The organization branch's line: if let Some(o) = app.try_state::<organization::Organization>() { o.flush(); }
    if let Some(composers) = app.try_state::<composer::Composers>() {
        composers.flush();
    }
    if let Some(drafts) = app.try_state::<Drafts>() {
        drafts.0.flush();
    }
}
```

Leave the `media` lines out if M1c is not on main; M1c's rebase brings them.

**Termination signals** run the same hook rather than a second one:

```rust
/// SIGTERM, SIGINT or SIGHUP: the exit hook runs here first (drafts reach the
/// disk even if the event loop is stuck), then the app exits, which runs the
/// hook again harmlessly. Should the event loop not end the process, it ends
/// five seconds later.
#[cfg(all(unix, feature = "live"))]
fn exit_on_signals(app: tauri::AppHandle) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) else { return };
    std::thread::Builder::new()
        .name("zerus-signals".into())
        .spawn(move || {
            if signals.forever().next().is_some() {
                exit(&app);
                app.exit(0);
                std::thread::sleep(std::time::Duration::from_secs(5));
                std::process::exit(0);
            }
        })
        .expect("the signal thread starts");
}
```

The Cargo additions change `client/src-tauri/Cargo.lock`: run `cargo +1.99.1 check --manifest-path src-tauri/Cargo.toml` once without `--locked` and commit the lock.

- [ ] **Step 7: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: pump::`
Expected: PASS (13 composer tests, the pump tests). Then clippy and fmt as in Task 1.

- [ ] **Step 8: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the composer hub with durable drafts in the app's one store"
```

---

### Task 3b: The `zerus-file` scheme and the attachment cache

**Depends on:**
- Task 3a;
- A1 `zerus-5wmh.9.3` (#403): `attachments::{cached_at, sniff, store_at}`;
- the cache bound `zerus-5wmh.9.17` (branch `work-ba0ed9fd-cachelru`): `attachments::touch`.

**Qt reference:** `tray/src/AttachmentViewer.h:28` (the cache root under the app's cache folder); `tray/src/MessageComposer.cpp:1067-1085` (a composer attachment shown from its bytes).

**Files:**
- Create: `client/src-tauri/src/composer/files.rs`
- Modify: `client/src-tauri/src/composer/mod.rs` (`pub mod files`, `Config`, `Shared.cache`, `Composers::cache`, `Composers::new` gains `config`)
- Modify: `client/src-tauri/src/lib.rs` (the cache folder, the URI scheme)
- Modify: `client/src-tauri/tauri.conf.json` (CSP `img-src`, `media-src` and `connect-src` for `zerus-file`)
- Test: unit tests in `composer/files.rs`

**Interfaces:**
- Consumes: Task 3a's `Composers`, `Hub`, `Composer::{handle, attachments_rev, draft}` and the test helpers `dir`, `drafts`, `hub`, `session`, `until`.
- Produces:

```rust
// composer/mod.rs
pub struct Config { pub cache: PathBuf }
impl Composers {
    pub fn new(drafts: Arc<DraftStore>, config: Config, deps: Deps) -> Self; // Task 3a's, with the config
    pub fn cache(&self) -> Arc<files::Cache>;
}
// composer/files.rs
pub struct Cache; // the private cache's canonical root
impl Cache { pub fn new(root: PathBuf) -> Arc<Self>; pub fn root(&self) -> &Path; }
pub fn respond(composers: &Composers, webview: &str, request: &http::Request<Vec<u8>>) -> http::Response<Vec<u8>>;
// the scheme: zerus-file://localhost/draft/{handle}/{list}/{index} and zerus-file://localhost/cache/<path under the root>
// (http://zerus-file.localhost/... on Windows), answered only to the main webview, with HTTP Range (206)
```

Task 4b adds `Cache::prune_later`, and Task 9 the pins, `with_limit` and `set_extra`, each with its first use (clippy denies unused code).

- [ ] **Step 1: Write the failing tests** at the end of `client/src-tauri/src/composer/files.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::tests::{dir, hub, session, until};
    use zerus_client_core::attachments::Selection;

    fn get(path: &str, range: Option<&str>) -> Request<Vec<u8>> {
        let mut request = Request::builder().uri(format!("zerus-file://localhost/{path}"));
        if let Some(range) = range { request = request.header("range", range); }
        request.body(Vec::new()).unwrap()
    }

    #[test]
    fn the_scheme_serves_draft_bytes_with_ranges_and_only_to_main() {
        let root = dir("scheme");
        let file = root.join("clip.bin");
        std::fs::write(&file, b"0123456789").unwrap();
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.attach_paths(view.handle, vec![file], Selection::default());
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        let path = view.attachments[0].path.clone();
        let whole = respond(&c, "main", &get(&path, None));
        assert_eq!(whole.status(), 200);
        assert_eq!(whole.body(), b"0123456789");
        assert_eq!(whole.headers()["accept-ranges"], "bytes");
        assert_eq!(whole.headers()["cross-origin-resource-policy"], "cross-origin");
        let part = respond(&c, "main", &get(&path, Some("bytes=2-5")));
        assert_eq!(part.status(), 206);
        assert_eq!(part.body(), b"2345");
        assert_eq!(part.headers()["content-range"], "bytes 2-5/10");
        assert_eq!(respond(&c, "main", &get(&path, Some("bytes=20-"))).status(), 416);
        assert_eq!(respond(&c, "native-ui-1", &get(&path, None)).status(), 403);
        assert_eq!(respond(&c, "main", &get("draft/0/0/0", None)).status(), 404);
        assert_eq!(respond(&c, "main", &get("cache/../../etc/passwd", None)).status(), 404);
        c.remove(view.handle, 0);
        assert_eq!(respond(&c, "main", &get(&path, None)).status(), 404, "a removed file's path answers nothing");
    }

    #[test]
    fn the_scheme_serves_only_cached_files_under_the_root() {
        let root = dir("cached");
        let (c, _) = hub(&root);
        let cached = zerus_client_core::attachments::store_at(c.cache().root(), "shot.png", b"\x89PNG\r\n\x1a\nrest").unwrap();
        let relative = cached.strip_prefix(c.cache().root()).unwrap().to_string_lossy().replace('\\', "/");
        let response = respond(&c, "main", &get(&format!("cache/{}", urlencode(&relative)), None));
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        std::fs::write(root.join("outside.txt"), b"secret").unwrap();
        assert_eq!(respond(&c, "main", &get("cache/..%2Foutside.txt", None)).status(), 404);
    }

    fn urlencode(text: &str) -> String {
        text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::files::`
Expected: compile errors, `files` does not exist.

- [ ] **Step 3: The cache in the hub.** In `composer/mod.rs`:
  - `pub mod files;` at the top;
  - `pub struct Config { pub cache: PathBuf }`;
  - `Shared` gains `pub(crate) cache: Arc<files::Cache>`;
  - `Composers::new(drafts, config, deps)` builds it with `files::Cache::new(config.cache)`;
  - `pub fn cache(&self) -> Arc<files::Cache> { Arc::clone(&self.0.cache) }`;
  - the test helper `hub_on` passes `Config { cache: store.root().with_file_name("cache") }`, the `cache` folder beside the test's `drafts`.

- [ ] **Step 4: Write `client/src-tauri/src/composer/files.rs`** above its tests:

```rust
//! The `zerus-file` URI scheme over the private attachment cache.
//!
//! `zerus-file://localhost/draft/{handle}/{list}/{index}` serves an
//! attachment of a composer in memory while its attachment list is the one
//! the view named; `.../cache/<path>` serves a copy in the private cache, only
//! when `attachments::cached_at` accepts it (a regular file inside the
//! canonical root, nonempty, at most 10 MiB). Only the main webview gets
//! answers. Range requests answer 206, as media elements seek with them.
//! Bytes reach the webview only this way or as raw IPC bodies, never as JSON.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::http::{Request, Response, StatusCode};
use zerus_client_core::attachments;

use super::Composers;

pub struct Cache {
    root: PathBuf,
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl Cache {
    pub fn new(root: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&root);
        Arc::new(Cache { root: canonical(&root) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).expect("a bare status response")
}

fn decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` within `len`.
fn range(header: Option<&str>, len: usize) -> Option<(usize, usize)> {
    let spec = header?.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.parse::<usize>().ok(), end.parse::<usize>().ok()) {
        (Some(s), Some(e)) => (s, e.min(len.checked_sub(1)?)),
        (Some(s), None) => (s, len.checked_sub(1)?),
        (None, Some(n)) => (len.saturating_sub(n), len.checked_sub(1)?),
        (None, None) => return None,
    };
    (start <= end && end < len).then_some((start, end))
}

/// `draft/{handle}/{list}/{index}`: the attachment of a composer in memory,
/// while its list is still the one the view named. No hashing: an attachment
/// list that changed answers 404 for its old paths.
fn draft_file(composers: &Composers, rest: &str) -> Option<(Arc<[u8]>, String)> {
    let numbers: Vec<u64> = rest.split('/').map(|part| part.parse().ok()).collect::<Option<_>>()?;
    let &[handle, list, index] = numbers.as_slice() else { return None };
    let hub = composers.lock();
    let composer = hub.composers.values().find(|c| c.handle == handle && c.attachments_rev == list)?;
    let attachment = composer.draft.attachments.get(usize::try_from(index).ok()?)?;
    Some((Arc::clone(&attachment.bytes), attachment.mime.clone()))
}

/// Answers one `zerus-file` request for webview `webview`.
pub fn respond(composers: &Composers, webview: &str, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if webview != crate::MAIN_WINDOW {
        return status(StatusCode::FORBIDDEN);
    }
    let Some(path) = decode(request.uri().path().trim_start_matches('/')) else { return status(StatusCode::NOT_FOUND) };
    let found = if let Some(rest) = path.strip_prefix("draft/") {
        draft_file(composers, rest)
    } else if let Some(relative) = path.strip_prefix("cache/") {
        let cache = composers.cache();
        attachments::cached_at(cache.root(), &cache.root().join(relative)).and_then(|file| {
            attachments::touch(&file);
            // `cached_at` refuses files over 10 MiB, so this read is bounded.
            let bytes = std::fs::read(&file).ok()?;
            let mime = attachments::sniff(&bytes);
            Some((Arc::<[u8]>::from(bytes), mime))
        })
    } else {
        None
    };
    let Some((bytes, mime)) = found else { return status(StatusCode::NOT_FOUND) };
    let len = bytes.len();
    let builder = Response::builder()
        .header("content-type", mime)
        .header("accept-ranges", "bytes")
        .header("cache-control", "no-store")
        .header("cross-origin-resource-policy", "cross-origin");
    let requested = request.headers().get("range").and_then(|v| v.to_str().ok());
    match range(requested, len) {
        Some((start, end)) => builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", format!("bytes {start}-{end}/{len}"))
            .body(bytes[start..=end].to_vec()),
        None if requested.is_some() => return status(StatusCode::RANGE_NOT_SATISFIABLE),
        None => builder.status(StatusCode::OK).body(bytes.to_vec()),
    }
    .expect("a file response")
}
```

`attachments::cached_at` canonicalizes and rejects anything outside the root, so `cache/../...` paths answer 404.

- [ ] **Step 5: Register it** in `client/src-tauri/src/lib.rs`. `Composers::new` gets the config, from the app's cache folder (Tauri's `app_cache_dir`, so the identifier decides it; Qt's cache is `hgdev/hgs-tray/attachments`, a different folder this client never touches):

```rust
let cache = app.path().app_cache_dir()?.join(if replay { "attachments-replay" } else { "attachments" });
app.manage(composer::Composers::new(drafts, composer::Config { cache }, composer::Deps { /* Task 3a's */ }));
```

and the builder gains the scheme:

```rust
        .register_asynchronous_uri_scheme_protocol("zerus-file", |ctx, request, responder| {
            #[cfg(feature = "live")]
            {
                use tauri::Manager;
                let webview = ctx.webview_label().to_owned();
                let app = ctx.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let composers = app.state::<composer::Composers>();
                    responder.respond(composer::files::respond(&composers, &webview, &request));
                });
            }
            #[cfg(not(feature = "live"))]
            {
                let _ = (ctx, request);
                responder.respond(tauri::http::Response::builder().status(404).body(Vec::new()).expect("a 404"));
            }
        })
```

In `tauri.conf.json`, in `csp` and `devCsp` (keeping the dev server origin in `devCsp`): `img-src` gains `zerus-file: http://zerus-file.localhost`, `media-src` is `'self' zerus-file: http://zerus-file.localhost`, and `connect-src` gains `zerus-file: http://zerus-file.localhost` (Task 9 reads a text preview's first bytes with `fetch`). The responses carry `Cross-Origin-Resource-Policy: cross-origin`, which the app's `Cross-Origin-Embedder-Policy: require-corp` needs.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::`
Expected: PASS (Task 3a's 13 and these 2). Clippy and fmt clean.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the zerus-file scheme over draft attachments and the private cache"
```

---

### Task 4a: The session's part of the composer: block reasons, the cache chip and the inspect fallback

**Depends on:**
- S1 `zerus-5wmh.9.7` (#407, on main): `send::{BlockFacts, block_reason}`;
- S2 `zerus-5wmh.9.8` (#408, on main): `cache::{expired, warning, chip, Chip, details}`;
- A2 `zerus-5wmh.9.4` (#404): the `dispatch` function in `actions.rs`, which this task marks writes in;
- Tasks 3a and 3b.

**Qt reference:** `tray/src/SessionsWindow.cpp`:
- `messageBlockReason` (4386-4432) and the availability it gives the composer (3982-3989);
- `m_pending`, `isTerminating` and the update lanes the block reason reads;
- the cache chip's minute and its popover text (987-1002);
- the inspection a selected session gets (`inspect()`), and `m_inspectError`.

**Files:**
- Create: `client/src-tauri/src/composer/send.rs`
- Modify: `client/src-tauri/src/composer/mod.rs` (`pub mod send`, `Deps`, `Followed`, `Hub`, `open`, `observe`, the view; Task 3a's test helper)
- Modify: `client/src-tauri/src/actions.rs` (`WriteKind`, `write_kind`, the mark in `dispatch`)
- Modify: `client/src-tauri/src/pump.rs` (`row`, `row_named`, `online`)
- Modify: `client/src-tauri/src/commands.rs` (`next_batch` calls `composers.observe`)
- Modify: `client/src-tauri/Cargo.toml` (+ `chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }`), `client/src-tauri/Cargo.lock`
- Modify: `client/src-tauri/src/lib.rs` (the new deps, the ticker)
- Test: unit tests in `composer/send.rs`, `actions.rs` and `pump.rs`

**Interfaces:**
- Consumes: Task 3a's `Composers`, `Hub`, `Composer`, `SessionRef`, `lock`; Task 3b's `Config`; `zerus_client_core::details::details_invocation`, `run_limited`, `Exit`.
- Produces (Tasks 4b and 5 extend these):

```rust
// actions.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum WriteKind { Update, Terminate } // Task 5 adds Clear
impl WriteKind { pub fn of(action: &Action) -> Self; }
impl Actions { pub fn write_kind(&self, computer: &str, name: &str) -> Option<WriteKind>; }
// composer/mod.rs: Deps gains
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;              // seconds since the epoch
pub type Rows = Arc<dyn Fn(&str, &str, &str) -> Option<(ViewRow, bool)> + Send + Sync>; // (computer, catalog key, name) -> (row, online); a new run is found by name
pub type Facts = Arc<dyn Fn(&str, &str) -> Option<WriteKind> + Send + Sync>;
pub type Inspect = Arc<dyn Fn(&Target, Option<&str>) -> Option<Value> + Send + Sync>; // blocking
pub struct Deps { pub spawn: Spawn, pub notify: Notify, pub clock: Clock, pub rows: Rows, pub facts: Facts, pub inspect: Inspect }
impl Composers { pub fn observe(&self, batch: &Batch); pub fn tick(&self); pub fn start_ticker(&self); }
// ComposerView gains
pub available: bool, pub unavailable_reason: String, pub block_reason: Option<String>,
pub attachments_available: bool, pub cache: Chip, pub cache_warning: String, pub cache_details: String,
// composer/send.rs (crate-internal; Task 4b builds on them)
pub(crate) struct Context { session: SessionRef, key: String, row: ViewRow, online: bool, details: Option<Arc<Value>>, facts: BlockFacts }
impl Composers { pub(crate) fn now(&self) -> f64; pub(crate) fn context(&self, hub: &Hub) -> Option<Context>; pub(crate) fn refresh_details(&self); }
pub(crate) fn session_view(composers: &Composers, hub: &Hub, composer: &Composer) -> SessionView;
// pump.rs
impl Pump { pub fn row(&self, computer: &str, key: &str) -> Option<ViewRow>; pub fn row_named(&self, computer: &str, name: &str) -> Option<ViewRow>; pub fn online(&self, computer: &str) -> bool; }
```

- [ ] **Step 1: The write kinds.** In `client/src-tauri/src/actions.rs`, write the failing test:

```rust
    #[test]
    fn a_running_write_reports_its_kind() {
        let actions = Actions::new(std::env::temp_dir().join(format!("zerus-kinds-{}.json", std::process::id())));
        assert_eq!(actions.write_kind("", "claude/sample-project/review"), None);
        let guard = actions.mark_write("", "claude/sample-project/review", &Action::Terminate);
        assert_eq!(actions.write_kind("", "claude/sample-project/review"), Some(WriteKind::Terminate));
        drop(guard);
        assert_eq!(actions.write_kind("", "claude/sample-project/review"), None);
        assert_eq!(WriteKind::of(&Action::Pause), WriteKind::Update);
    }
```

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml actions::` and see it fail. Then implement:

```rust
/// What a session's running write is, for the send rules (`BlockFacts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    Update,
    Terminate,
}

impl WriteKind {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::Terminate | Action::Kill => Self::Terminate,
            _ => Self::Update,
        }
    }
}

/// Clears a session's write kind when its call ends.
pub struct WriteMark {
    writes: Arc<Mutex<HashMap<(String, String), WriteKind>>>,
    key: (String, String),
}

impl Drop for WriteMark {
    fn drop(&mut self) {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.key);
    }
}
```

`Actions` gains `writes: Arc<Mutex<HashMap<(String, String), WriteKind>>>` (initialized empty in `new`), and:

```rust
    pub fn write_kind(&self, computer: &str, name: &str) -> Option<WriteKind> {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).get(&(computer.to_owned(), name.to_owned())).copied()
    }

    fn mark_write(&self, computer: &str, name: &str, action: &Action) -> WriteMark {
        let key = (computer.to_owned(), name.to_owned());
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone(), WriteKind::of(action));
        WriteMark { writes: Arc::clone(&self.writes), key }
    }
```

In `dispatch` (A2), right after the claim:

```rust
    let _write = (Lane::of(&action) == Lane::Write).then(|| state.mark_write(&target.computer, &target.name, &action));
```

Run the actions tests again: PASS.

- [ ] **Step 2: Write the failing tests** at the end of `client/src-tauri/src/composer/send.rs`:

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::composer::tests::{dir, drafts, session, until};
    use crate::composer::{lock, ComposerView, Composers, Config, Deps};
    use serde_json::json;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use zerus_client_core::drafts::DraftStore;
    use zerus_client_core::{ActivityOut, Batch, ComputerId};

    /// The session, clock and inspect a test controls.
    pub(crate) struct Fake {
        pub(crate) row: Mutex<ViewRow>,
        pub(crate) now: Mutex<f64>,
        pub(crate) inspected: Mutex<Option<Value>>,
        pub(crate) inspects: AtomicUsize,
    }

    pub(crate) fn row(name: &str) -> ViewRow {
        ViewRow { name: name.into(), agent: "claude".into(), state: "running".into(), tracked: true, run_id: Some("run-1".into()),
                  conversation_id: Some("conv-1".into()), ..ViewRow::default() }
    }

    pub(crate) fn send_hub_on(store: &Arc<DraftStore>) -> (Composers, Arc<Fake>) {
        let fake = Arc::new(Fake {
            row: Mutex::new(row("claude/sample-project/review")),
            now: Mutex::new(1_793_000_100.0),
            inspected: Mutex::new(None),
            inspects: AtomicUsize::new(0),
        });
        let (f1, f2, f3) = (fake.clone(), fake.clone(), fake.clone());
        let composers = Composers::new(
            Arc::clone(store),
            Config { cache: store.root().with_file_name("cache") },
            Deps {
                spawn: Arc::new(|job| { std::thread::spawn(job); }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&f1.now)),
                rows: Arc::new(move |_, _, _| Some((lock(&f2.row).clone(), true))),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(move |_, _| { f3.inspects.fetch_add(1, Ordering::SeqCst); lock(&f3.inspected).clone() }),
            },
        );
        (composers, fake)
    }

    pub(crate) fn send_hub(root: &Path) -> (Composers, Arc<Fake>) {
        send_hub_on(&drafts(root))
    }

    pub(crate) fn ready() -> Value {
        json!({"tracked": true, "run_id": "run-1", "conversation_id": "conv-1", "runtime_state": "live", "process_state": "running",
               "phase": "idle", "activity": "idle", "compact_context_supported": true, "clear_context_supported": true})
    }

    pub(crate) fn state(details: Value) -> Batch {
        Batch {
            activity: vec![ActivityOut { computer: ComputerId::from("studio"), key: "run:run-1".into(), kind: "state".into(), payload: json!({"value": details, "rev": 1}) }],
            ..Batch::default()
        }
    }

    pub(crate) fn opened(c: &Composers, details: Value) -> ComposerView {
        c.open(Some(session()));
        c.observe(&state(details));
        until(c, |v| v.loaded)
    }

    #[test]
    fn a_ready_session_takes_messages() {
        let root = dir("ready");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        assert!(view.available && view.block_reason.is_none() && view.unavailable_reason.is_empty());
        let handle = view.handle;
        c.edit(handle, 1, "hello".into(), 5, 5);
        assert!(c.current().unwrap().can_send);
    }

    #[test]
    fn a_block_reason_is_shown() {
        let root = dir("blocked");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["phase"] = json!("approval");
        let view = opened(&c, details);
        assert_eq!(view.block_reason.as_deref(), Some("Respond to the agent in the Terminal tab."));
        assert_eq!(view.unavailable_reason, "Respond to the agent in the Terminal tab.");
        assert!(!view.available);
        c.edit(view.handle, 1, "hi".into(), 2, 2);
        let view = c.current().unwrap();
        assert!(!view.can_send);
        assert_eq!(view.status, "Respond to the agent in the Terminal tab.");
    }

    #[test]
    fn the_cache_chip_counts_down_in_minutes() {
        let root = dir("chip");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["session_usage"] = json!({"prompt_cache": {"status": "warm", "expires_at": 1_793_000_100.0 + 540.0}});
        let view = opened(&c, details);
        assert_eq!((view.cache.label.as_str(), view.cache.short_label.as_str()), ("Cache ~9m", "~9m"));
        assert!(view.cache_details.starts_with("Estimated expiry from the TTL reported in native usage: "));
        *lock(&fake.now) += 240.0;
        c.tick();
        assert_eq!(c.current().unwrap().cache.label, "Cache ~5m");
    }

    #[test]
    fn without_a_state_the_session_is_inspected_once() {
        let root = dir("inspect");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(view.block_reason.as_deref(), Some("Checking the session…"));
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        c.tick();
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_state_after_a_failed_inspect_clears_its_reason() {
        let root = dir("inspect-failed");
        let (c, fake) = send_hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.as_deref() == Some("Refresh the session before sending."));
        c.observe(&state(ready()));
        assert!(c.current().unwrap().block_reason.is_none());
    }
}
```

In `pump.rs`:

```rust
    #[test]
    fn rows_are_looked_up_by_key_and_name() {
        let pump = Pump::default();
        pump.push(upsert("run:a", "claude/p/a"));
        pump.push(Change::Computer { computer: ComputerId(String::new()), status: ComputerStatus::Live });
        pump.push(Change::Computer { computer: ComputerId("studio".into()), status: ComputerStatus::Reconnecting { attempt: 1 } });
        let _ = pump.take(PARK);
        assert_eq!(pump.row("", "run:a").unwrap().name, "claude/p/a");
        assert_eq!(pump.row_named("", "claude/p/a").unwrap().name, "claude/p/a");
        assert!(pump.online(""));
        assert!(!pump.online("studio"));
        assert!(pump.online("unknown"), "as the frontend reads it: only reconnecting or ended is offline");
    }
```

- [ ] **Step 3: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::send:: pump::`
Expected: compile errors.

- [ ] **Step 4: Extend `composer/mod.rs`.**
  - `pub mod send;` at the top.
  - `Deps` gains `clock`, `rows`, `facts` and `inspect` (the aliases in "Produces"). Task 3a's test helper `hub_on` passes `clock: Arc::new(|| 1_793_000_100.0)`, `rows: Arc::new(|_, _, _| None)`, `facts: Arc::new(|_, _| None)` and `inspect: Arc::new(|_, _| None)`: those tests have no session, so their views read "Select a session.".
  - The followed session's Activity state, which the batches bring:

```rust
/// The followed session's Activity state, as the batches bring it.
#[derive(Default)]
pub(crate) struct Followed {
    pub(crate) computer: String,
    pub(crate) key: String,
    pub(crate) details: Option<Arc<Value>>,
    /// The last inspect failed (`m_inspectError`); a state clears it.
    pub(crate) inspect_error: bool,
    pub(crate) opened_at: f64,
    pub(crate) inspecting: bool,
}
```

  - `Hub` gains `pub(crate) followed: Followed` and `pub(crate) minute: i64`.
  - `open` resets the followed state when the session changes, before `hub.open = Some(session)`:

```rust
        if hub.followed.computer != session.computer || hub.followed.key != session.key {
            hub.followed = Followed { computer: session.computer.clone(), key: session.key.clone(), opened_at: self.now(), ..Followed::default() };
        }
```

  - `observe` records the state:

```rust
    /// Records the followed session's Activity state from a batch.
    pub fn observe(&self, batch: &Batch) {
        let mut hub = self.lock();
        let (computer, key) = (hub.followed.computer.clone(), hub.followed.key.clone());
        let mut changed = false;
        for out in batch.activity.iter().filter(|out| out.computer.0 == computer && out.key == key) {
            match out.kind.as_str() {
                "state" => {
                    hub.followed.details = Some(Arc::new(out.payload["value"].clone()));
                    hub.followed.inspect_error = false;
                    changed = true;
                }
                "ended" => {
                    hub.followed.details = None;
                    changed = true;
                }
                _ => {}
            }
        }
        drop(hub);
        if changed {
            self.after_observe();
        }
    }

    /// What a change of the followed session's Activity sets off (Task 4b
    /// adds the reconcile and Task 5 the continuations).
    fn after_observe(&self) {
        self.changed();
    }
```

  - `ComposerView` gains the fields in "Produces". `view_of` fills them through `send::session_view(self, hub, composer)` and uses its `block_reason`, `available` and attachments rule in the status line and `can_send`:

```rust
        let session = send::session_view(self, hub, composer);
        let attachments_blocked = !composer.draft.attachments.is_empty() && !session.attachments_available;
        // after the storage, waiting and own error cases:
        } else if attachments_blocked {
            (session.attachments_reason.clone(), false)
        } else if too_long {
            ("Message exceeds 64 KiB.".to_owned(), true)
        } else {
            (session.unavailable_reason.clone(), false)
        };
        // can_send: ... && session.available && !attachments_blocked
```

- [ ] **Step 5: Write `client/src-tauri/src/composer/send.rs`** above its tests:

```rust
//! The open session's part of the composer, ported from `SessionsWindow`:
//! the block reason (`messageBlockReason`, S1), the cache chip and its
//! details (S2), and one inspect when no Activity state arrived. Task 4b adds
//! the delivery to this module.

use std::sync::Arc;

use serde_json::Value;
use zerus_client_core::actions::Target;
use zerus_client_core::cache::{self, Chip};
use zerus_client_core::send::{block_reason, BlockFacts};
use zerus_client_core::ViewRow;

use super::{Composer, Composers, Hub, SessionRef};
use crate::actions::WriteKind;

/// No Activity state this long after opening: inspect once.
const DETAILS_WAIT: f64 = 3.0;

/// The session's state for the composer view.
#[derive(Clone, Debug, Default)]
pub(crate) struct SessionView {
    pub(crate) available: bool,
    pub(crate) unavailable_reason: String,
    pub(crate) block_reason: Option<String>,
    pub(crate) attachments_available: bool,
    pub(crate) attachments_reason: String,
    pub(crate) cache: Chip,
    pub(crate) cache_warning: String,
    pub(crate) cache_details: String,
}

/// The agent command the send rules read: the row says `deepseek` for `dsh`.
pub(crate) fn cmd(row: &ViewRow) -> String {
    if row.agent == "deepseek" { "dsh".into() } else { row.agent.clone() }
}

/// Local time as the cache details show it; formatted in Rust, never in the webview.
fn local_time(seconds: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.format("%-d %b %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// The open session's row, online state, details and facts.
pub(crate) struct Context {
    pub(crate) session: SessionRef,
    pub(crate) key: String,
    pub(crate) row: ViewRow,
    pub(crate) online: bool,
    pub(crate) details: Option<Arc<Value>>,
    pub(crate) facts: BlockFacts,
}

impl Context {
    pub(crate) fn reason(&self) -> Option<String> {
        block_reason(&self.facts, self.details.as_deref())
    }
    /// Qt's `recorded`: session data that is not live (`SessionsWindow.cpp:3783`).
    pub(crate) fn recorded(&self) -> bool {
        !self.online || matches!(self.row.state.as_str(), "archived" | "paused") || self.row.process_state == "exited" || self.facts.inspect_error
    }
}

impl Composers {
    pub(crate) fn now(&self) -> f64 {
        (self.0.deps.clock)()
    }

    /// The open session as the catalog and the followed Activity show it now.
    pub(crate) fn context(&self, hub: &Hub) -> Option<Context> {
        let session = hub.open.clone()?;
        let key = hub.open_key.clone()?;
        let (row, online) = (self.0.deps.rows)(&session.computer, &session.key, &session.name)?;
        let followed = hub.followed.computer == session.computer && hub.followed.key == session.key;
        let details = if followed { hub.followed.details.clone() } else { None };
        let write = (self.0.deps.facts)(&session.computer, &session.name);
        let facts = BlockFacts {
            start_send_waiting: false,
            compacting: false,
            clearing: false,
            terminating: write == Some(WriteKind::Terminate),
            settings_changing: false,
            online,
            state: row.state.clone(),
            updating: write == Some(WriteKind::Update),
            cmd: cmd(&row),
            inspect_error: followed && hub.followed.inspect_error,
        };
        Some(Context { session, key, row, online, details, facts })
    }

    /// One `inspect` of the open session, off the UI thread; its value
    /// stands in for the stream's state until the next state frame.
    pub(crate) fn refresh_details(&self) {
        let mut hub = self.lock();
        if hub.followed.inspecting {
            return;
        }
        let Some(ctx) = self.context(&hub) else { return };
        hub.followed.inspecting = true;
        drop(hub);
        let target = Target {
            computer: ctx.session.computer.clone(), name: ctx.session.name.clone(),
            run_id: ctx.row.run_id.clone().unwrap_or_default(), conversation_id: ctx.row.conversation_id.clone().unwrap_or_default(),
        };
        let session = ctx.session;
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let value = (me.0.deps.inspect)(&target, session.archive_id.as_deref());
            let mut hub = me.lock();
            hub.followed.inspecting = false;
            if hub.followed.computer == session.computer && hub.followed.key == session.key {
                match value {
                    Some(value) => {
                        hub.followed.inspect_error = false;
                        hub.followed.details = Some(Arc::new(value));
                    }
                    None => hub.followed.inspect_error = true,
                }
            }
            drop(hub);
            me.after_observe();
        }));
    }

    /// Once a second: the cache chip's minute and the inspect fallback when
    /// no Activity state arrived within 3 s of opening.
    pub fn tick(&self) {
        let now = self.now();
        let (minute_changed, wants_details) = {
            let mut hub = self.lock();
            let minute = (now / 60.0).floor() as i64;
            let changed = std::mem::replace(&mut hub.minute, minute) != minute;
            let wants = hub.open.is_some() && hub.followed.details.is_none() && !hub.followed.inspecting
                && !hub.followed.inspect_error && now - hub.followed.opened_at >= DETAILS_WAIT;
            (changed, wants)
        };
        if wants_details {
            self.refresh_details();
        }
        if minute_changed {
            self.changed();
        }
    }

    pub fn start_ticker(&self) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("zerus-composer".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                me.tick();
            })
            .expect("the composer ticker starts");
    }
}

/// The open session's part of the composer view (`renderDetails` 3982-3989).
pub(crate) fn session_view(composers: &Composers, hub: &Hub, composer: &Composer) -> SessionView {
    let Some(ctx) = composers.context(hub).filter(|ctx| ctx.key == composer.key) else {
        return SessionView { unavailable_reason: "Select a session.".into(), attachments_available: true, ..SessionView::default() };
    };
    let details = ctx.details.as_deref().cloned().unwrap_or_default();
    let now = composers.now();
    let reason = ctx.reason();
    let attachments_available = ctx.facts.cmd != "opencode" && details.get("attachments_supported").and_then(Value::as_bool).unwrap_or(true);
    let attachments_reason = details.get("attachments_reason").and_then(Value::as_str).unwrap_or(
        "This native adapter does not support file attachments. Your existing files remain saved; remove them to send text.",
    ).to_owned();
    let recorded = ctx.recorded();
    SessionView {
        available: reason.is_none(),
        unavailable_reason: reason.clone().unwrap_or_default(),
        block_reason: reason,
        attachments_available,
        attachments_reason,
        cache: cache::chip(&details, now),
        cache_warning: cache::warning(&details, now),
        cache_details: cache::details(&details, recorded, local_time),
    }
}
```

`send` is a child module of `composer`, so it reaches the hub's private items (`after_observe`, `Hub`'s fields) without widening them.

- [ ] **Step 6: The pump lookups** in `client/src-tauri/src/pump.rs`:

```rust
    /// The last-known row of `(computer, catalog key)`.
    pub fn row(&self, computer: &str, key: &str) -> Option<ViewRow> {
        self.lock().mirror.computers.get(computer)?.rows.get(key).cloned()
    }

    /// The computer's live row of that name (a session that started a new
    /// run has a new catalog key). A scan of one computer's rows, only when
    /// the key is gone.
    pub fn row_named(&self, computer: &str, name: &str) -> Option<ViewRow> {
        self.lock().mirror.computers.get(computer)?.rows.values().find(|row| row.name == name && row.state != "archived").cloned()
    }

    /// Whether the computer counts as online, as the frontend's
    /// `computerOffline` reads it: neither reconnecting nor ended.
    pub fn online(&self, computer: &str) -> bool {
        self.lock().mirror.computers.get(computer)
            .and_then(|rows| rows.status)
            .is_none_or(|status| !matches!(status, ComputerStatus::Reconnecting { .. } | ComputerStatus::Ended))
    }
```

`next_batch` calls `composers.observe(&batch)` after `actions.observe(&batch)`.

- [ ] **Step 7: Register it** in `client/src-tauri/src/lib.rs`. `composer::Deps` gets, besides Task 3a's:

```rust
clock: std::sync::Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())),
rows: {
    let pump = app.state::<commands::AppState>().pump();
    std::sync::Arc::new(move |computer, key, name| {
        pump.row(computer, key).or_else(|| pump.row_named(computer, name)).map(|row| (row, pump.online(computer)))
    })
},
facts: {
    let actions = app.state::<actions::Actions>().inner().clone();
    std::sync::Arc::new(move |computer, name| actions.write_kind(computer, name))
},
inspect: {
    // A replay without an explicit ZERUS_CLIENT_HGS never reaches the real CLI.
    let allowed = actions::actions_allowed(std::env::var("ZERUS_CLIENT_REPLAY").ok().as_deref(), std::env::var("ZERUS_CLIENT_HGS").ok().as_deref());
    let hgs = zerus_client_core::default_hgs();
    std::sync::Arc::new(move |target, archive| {
        if !allowed {
            return None;
        }
        let inv = zerus_client_core::details::details_invocation(target, archive);
        match zerus_client_core::run_limited(&hgs, &inv, &[], 8 << 20) {
            Ok(zerus_client_core::Exit::Code { code: 0, stdout, .. }) => serde_json::from_slice(&stdout).ok(),
            _ => None,
        }
    })
},
```

After managing it: `app.state::<composer::Composers>().start_ticker();`. The chrono addition changes `Cargo.lock`: one `cargo +1.99.1 check` without `--locked`, as in Task 3a.

- [ ] **Step 8: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: actions:: pump::`
Expected: PASS (Tasks 3a and 3b's tests still pass with the new deps). Clippy and fmt clean.

- [ ] **Step 9: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the composer's block reasons, cache chip and inspect fallback"
```

---

### Task 4b: The delivery: the guarded send, the cold cache question and local rows

**Depends on:**
- A2 `zerus-5wmh.9.4` (#404): `Action::Send`, `Outcome::Uncertain`, `actions::dispatch` in src-tauri, `Lane::Send`;
- S2 `zerus-5wmh.9.8` (#408, on main): `cache::{cold_prompt, ColdPrompt, ColdAction}`;
- S4 `zerus-5wmh.9.10` (branch `work-ba0ed9fd-localmsgs`): `local_messages::{LocalMessages, LocalMessage, inspect, Inspect}`;
- A1's `attachments::{prepare, read_stored, StoredFile}`;
- `zerus-5wmh.16` (#406): `ViewRow.created`;
- Tasks 3a, 3b and 4a.

**Qt reference:** `tray/src/SessionsWindow.cpp`:
- `sendMessage` with the cold cache dialog (4806-4853);
- `deliverMessage` and its `stillValid` (4855-4935);
- `finishMessage` (4937-4968), `reconcileMessages` (4970-5000);
- `messageAction` (4470-4527).

Also `MessageComposer::prepareSendingAsync` and `deliveryFinished` (729-776).

**Files:**
- Modify: `client/src-tauri/src/composer/send.rs` (the delivery, its commands, its tests)
- Modify: `client/src-tauri/src/composer/mod.rs` (`Deps.dispatch`, `Hub`, `Followed.events`, the view's `local`, the rename and observe hooks)
- Modify: `client/src-tauri/src/composer/files.rs` (`Cache::prune_later`)
- Modify: `client/src-tauri/src/actions.rs` (`dispatch_as`)
- Modify: `client/src-tauri/src/lib.rs` (the dispatch dep, the commands)
- Test: unit tests in `composer/send.rs`

**Interfaces:**
- Consumes: Task 4a's `Context`, `context`, `now`, `refresh_details`, `session_view`, `Fake`, `send_hub_on`, `ready`, `state`, `opened`, `row`; Task 3b's `Cache`, `canonical`; Task 3a's `editable`, `save`, `KEY`, `drafts`.
- Produces (Tasks 5-7 bind to these):

```rust
// actions.rs
pub async fn dispatch_as(state: &Actions, target: Target, action: Action, request_id: String) -> Result<Outcome, String>;
// composer/mod.rs: Deps gains
pub type Dispatch = Arc<dyn Fn(Target, Action, String) -> Result<Outcome, String> + Send + Sync>; // blocking; the String is the request id
// ComposerView gains
pub local: Vec<LocalMessage>,
// composer/files.rs
impl Cache { pub fn prune_later(self: &Arc<Self>, spawn: &Spawn, extra: Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync>); }
// composer/send.rs
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendReply { Sending, Busy, Blocked { reason: String }, Notice { text: String },
                     Cold { token: u64, prompt: ColdPrompt }, ClearContext { token: u64 } }
#[serde(tag = "kind", rename_all = "snake_case")] pub enum Answer { Cold { action: ColdAction }, Cancel }
impl Composers {
    pub fn send_message(&self, handle: u64, rev: u64, text: String, position: i64, anchor: i64) -> SendReply;
    pub fn answer(&self, token: u64, answer: Answer) -> SendReply;
    pub fn retry(&self, handle: u64, id: &str) -> SendReply;   // blocking: reads the cached files
    pub fn delete_local(&self, handle: u64, id: &str);
    pub fn inspect_local(&self, handle: u64, id: &str) -> Option<Inspect>;
}
// crate-internal, for Task 5: Retry, Prompt, Delivery, Context::{run, conversation}, ask, delivery, deliver, delivery_finished, reconcile
// commands: send_message(handle, rev, text, position, anchor) -> SendReply; send_answer(token, answer) -> SendReply;
// local_retry(handle, id) -> SendReply; local_delete(handle, id); local_inspect(handle, id) -> Option<Inspect>
```

**The rules this task keeps (AGENTS.md: unknown delivery never authorizes replay):**
- The durable "sending" save comes before any call: `deliver` saves, and only its callback (`prepared`) can dispatch.
- One request id per delivery, made right before its one dispatch. The local row, the CLI request and the receipt check carry it, and no other delivery reuses it.
- An `Uncertain` outcome (or an `Err`, which means the call's thread died) never resends. The draft stays, marked uncertain, and asks for Allow retry. Its row offers Check Terminal and Delete, never Retry.
- A question's answer re-checks the composer. A busy or uncertain draft is never sent by an older question, and a delivery drops its key's open questions.
- A closed hub (the app quits) starts nothing. A draft saved as "sending" reloads as uncertain.

- [ ] **Step 1: `dispatch_as`.** In `client/src-tauri/src/actions.rs`, `dispatch` becomes `dispatch_as(state, target, action, uuid::Uuid::new_v4().to_string())`. `dispatch_as` is the old body with the caller's `request_id` in place of the one it made. The id reaches `invocation` and the receipt check unchanged, and nothing in it retries.

- [ ] **Step 2: Write the failing driver tests** in `composer/send.rs`'s `tests` module. Task 4a's `Fake` gains `calls` and `outcome`, and `send_hub_on` passes `dispatch`:

```rust
    // Fake gains (with `type Answers = Box<dyn Fn(&Action, &str) -> Outcome + Send>;` in the module):
    pub(crate) calls: Mutex<Vec<(Target, Action, String)>>,
    pub(crate) outcome: Mutex<Answers>,
    // send_hub_on initializes them with Vec::new() and Box::new(receipt), and passes:
    dispatch: Arc::new(move |target, action, id| {
        let out = (lock(&f4.outcome))(&action, &id);
        lock(&f4.calls).push((target, action, id));
        Ok(out)
    }),
```

(`f4` is one more clone of `fake`.) Task 3a's `hub_on` passes `dispatch: Arc::new(|_, _, _| Err("no call in these tests".into()))`.

```rust
    pub(crate) fn receipt(action: &Action, id: &str) -> Outcome {
        Outcome::Done { receipt: json!({"status": "submitted", "request_id": id, "kind": format!("{action:?}").split_whitespace().next()}), cleanup: None }
    }

    pub(crate) fn sent(fake: &Fake) -> Vec<(Target, Action, String)> {
        lock(&fake.calls).clone()
    }

    #[test]
    fn send_runs_the_held_draft_once_and_clears_it() {
        let root = dir("send-once");
        let file = root.join("a.txt");
        std::fs::write(&file, "attached").unwrap();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.attach_paths(view.handle, vec![file], zerus_client_core::attachments::Selection { start: 0, end: 0 });
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        c.edit(view.handle, 4, "hello [File #1] ".into(), 0, 0);
        assert!(c.current().unwrap().can_send);
        assert_eq!(c.send_message(view.handle, 5, "hello [File #1] ".into(), 0, 0), SendReply::Sending);
        let view = until(&c, |v| !v.sending && v.text.is_empty());
        let calls = sent(&fake);
        assert_eq!(calls.len(), 1);
        let (target, action, id) = &calls[0];
        assert_eq!((target.computer.as_str(), target.run_id.as_str(), target.conversation_id.as_str()), ("studio", "run-1", "conv-1"));
        let Action::Send { text, attachments, agent_id, expected_compaction_id } = action else { panic!("not a send") };
        assert_eq!(text, "hello [File #1] ");
        assert_eq!(&*attachments[0].bytes, b"attached");
        assert!(agent_id.is_none() && expected_compaction_id.is_none());
        assert!(view.attachments.is_empty());
        assert_eq!(view.local.len(), 1);
        assert_eq!(&view.local[0].id, id, "the row carries the request's own id");
        assert_eq!(view.local[0].status, zerus_client_core::local_messages::Status::Sent);
    }

    #[test]
    fn a_blocked_send_runs_nothing() {
        let root = dir("blocked-send");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["phase"] = json!("approval");
        let view = opened(&c, details);
        assert_eq!(
            c.send_message(view.handle, 1, "hi".into(), 2, 2),
            SendReply::Blocked { reason: "Respond to the agent in the Terminal tab.".into() }
        );
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_cold_cache_asks_first_and_sends_only_after_consent() {
        let root = dir("cold");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        assert_eq!(view.cache.label, "Cold cache");
        let SendReply::Cold { token, prompt } = c.send_message(view.handle, 1, "hi".into(), 2, 2) else { panic!("no question") };
        assert_eq!(prompt.title, "Continue with a cold cache?");
        assert!(sent(&fake).is_empty());
        assert_eq!(c.answer(token, Answer::Cancel), SendReply::Busy);
        assert_eq!(c.answer(token, Answer::Cold { action: ColdAction::Send }), SendReply::Busy, "a cancelled question is gone");
        let SendReply::Cold { token, .. } = c.send_message(view.handle, 2, "hi".into(), 2, 2) else { panic!() };
        assert_eq!(c.answer(token, Answer::Cold { action: ColdAction::Send }), SendReply::Sending);
        until(&c, |v| !v.sending);
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn an_uncertain_send_is_never_retried() {
        let root = dir("uncertain");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain { detail: "Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.".into() });
        let view = opened(&c, ready());
        assert_eq!(c.send_message(view.handle, 1, "maybe".into(), 5, 5), SendReply::Sending);
        let view = until(&c, |v| !v.sending && v.uncertain);
        assert_eq!(view.text, "maybe", "the draft comes back");
        assert!(!view.can_send);
        let row = &view.local[0];
        assert!(row.uncertain);
        assert_eq!(row.error, "Delivery is not confirmed. Check Terminal before retrying. Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.");
        assert_eq!(c.retry(view.handle, &row.id), SendReply::Busy);
        assert_eq!(c.send_message(view.handle, 2, "maybe".into(), 5, 5), SendReply::Busy, "Send waits for Allow retry");
        c.flush();
        store.flush();
        drop(c);
        let (again, _) = send_hub_on(&store);
        let view = opened(&again, ready());
        assert!(view.uncertain && !view.can_send, "a restart keeps it uncertain");
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_stale_question_never_resends_an_uncertain_draft() {
        let root = dir("stale-question");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        let SendReply::Cold { token: first, .. } = c.send_message(view.handle, 1, "maybe".into(), 5, 5) else { panic!("no question") };
        let SendReply::Cold { token: second, .. } = c.send_message(view.handle, 1, "maybe".into(), 5, 5) else { panic!("no question") };
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain { detail: "Delivery timed out.".into() });
        assert_eq!(c.answer(second, Answer::Cold { action: ColdAction::Send }), SendReply::Sending);
        until(&c, |v| !v.sending && v.uncertain);
        assert_eq!(c.answer(first, Answer::Cold { action: ColdAction::Send }), SendReply::Busy, "the older question went with the delivery");
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_send_whose_save_is_queued_at_exit_never_starts() {
        let root = dir("exit-send");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        let view = opened(&c, ready());
        // Hold the writer, so the "sending" save waits in the queue.
        let (release, held) = std::sync::mpsc::channel::<()>();
        store.load("hold", move |_| { let _ = held.recv(); });
        assert_eq!(c.send_message(view.handle, 1, "at exit".into(), 7, 7), SendReply::Sending);
        c.flush();
        release.send(()).unwrap();
        store.flush();
        assert!(sent(&fake).is_empty(), "nothing starts once the app quits");
        let saved = zerus_client_core::drafts::read_draft(&root.join("drafts"), crate::composer::tests::KEY);
        assert!(saved.draft.uncertain, "the draft saved as sending reloads as uncertain");
    }

    #[test]
    fn a_definite_failure_retries_its_own_text_and_keeps_a_newer_draft() {
        let root = dir("retry");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed { detail: "hgs: refused".into() });
        let view = opened(&c, ready());
        c.send_message(view.handle, 1, "first".into(), 5, 5);
        let view = until(&c, |v| !v.sending && v.local.first().is_some_and(|r| !r.error.is_empty()));
        let id = view.local[0].id.clone();
        c.edit(view.handle, 9, "a newer draft".into(), 13, 13);
        *lock(&fake.outcome) = Box::new(receipt);
        assert_eq!(c.retry(view.handle, &id), SendReply::Sending);
        let view = until(&c, |v| !v.sending && v.local.iter().all(|r| r.error.is_empty()));
        let calls = sent(&fake);
        assert_eq!(calls.len(), 2);
        let Action::Send { text, .. } = &calls[1].1 else { panic!() };
        assert_eq!(text, "first");
        assert_ne!(calls[0].2, calls[1].2, "a retry is a new submission with its own request id");
        assert_eq!(view.text, "a newer draft");
        assert_eq!(view.local.len(), 1, "the retry replaced the failed row");
    }

    #[test]
    fn the_history_reconciles_sent_rows() {
        let root = dir("reconcile");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        c.send_message(view.handle, 1, "shown in history".into(), 0, 0);
        until(&c, |v| !v.sending && v.local.len() == 1);
        let mut batch = state(ready());
        batch.activity.push(ActivityOut {
            computer: ComputerId::from("studio"), key: "run:run-1".into(), kind: "events".into(),
            payload: json!({"events": [{"seq": 9, "type": "UserPromptSubmit", "detail": "shown in history", "at": 1_793_000_100.0}], "cursor": 9}),
        });
        c.observe(&batch);
        until(&c, |v| v.local.is_empty());
    }

    #[test]
    fn a_session_that_changed_is_not_sent_to() {
        let root = dir("changed");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        let delivery = c.delivery_for_test(view.handle, "hi");
        assert!(c.still_valid(&delivery));
        lock(&fake.row).created += 1;
        assert!(!c.still_valid(&delivery), "another session under the same name");
        lock(&fake.row).created -= 1;
        lock(&fake.row).run_id = Some("run-2".into());
        assert!(!c.still_valid(&delivery), "another run");
    }
```

The tests module also imports `zerus_client_core::actions::{Action, Outcome}`, `zerus_client_core::cache::ColdAction` and `crate::composer::tests::drafts` (already there from Task 4a).

- [ ] **Step 3: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::send::`
Expected: compile errors.

- [ ] **Step 4: Extend `composer/mod.rs`.**
  - `Deps` gains `dispatch: Dispatch`.
  - `Followed` gains `events: VecDeque<Value>`, the last 200 events of the followed session.
  - `Hub` gains `local: LocalMessages`, `retry_reads: HashSet<String>`, `prompts: HashMap<u64, send::Prompt>`, `next_token: u64`, and `opened: HashMap<(String, String), String>` with `opened_order: VecDeque<(String, String)>` (computer and catalog key to draft key; `open` records it and drops the oldest entry past 512, matching S4's 512 sessions).
  - `renamed` also calls `hub.local.rename(old, new)` when the store moved the draft.
  - `observe` also keeps the events (`"events"` appends, `"replace"` clears first; at most 200, oldest dropped). It forgets the local rows of a session that left the catalog: for `batch.removes` with a reason other than `replaced`, `hub.local.forget(key)` through `opened`.
  - `after_observe` becomes `self.reconcile()` followed by `self.changed()`.
  - `ComposerView` gains `local`, which `view_of` fills with `hub.local.rows(&composer.key).to_vec()` (at most 50).
  - `impl Hub { pub(crate) fn local_keys(&self) -> Vec<String> { self.opened.values().cloned().collect() } }`, since S4 exposes rows per key only.

- [ ] **Step 5: `Cache::prune_later`** in `composer/files.rs`. `Cache` gains `pruning: AtomicBool` and `again: AtomicBool`, false in `new`:

```rust
    /// One prune after a store, on the blocking pool; a store during a prune
    /// runs exactly one more. `extra` names the copies Retry still needs.
    pub fn prune_later(self: &Arc<Self>, spawn: &Spawn, extra: Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync>) {
        if self.pruning.swap(true, Ordering::SeqCst) {
            self.again.store(true, Ordering::SeqCst);
            return;
        }
        let cache = Arc::clone(self);
        spawn(Box::new(move || loop {
            cache.again.store(false, Ordering::SeqCst);
            let kept: HashSet<PathBuf> = extra().iter().map(|path| canonical(path)).collect();
            attachments::prune(&cache.root, attachments::CACHE_LIMIT, |path| kept.contains(&canonical(path)));
            if !cache.again.load(Ordering::SeqCst) {
                cache.pruning.store(false, Ordering::SeqCst);
                break;
            }
        }));
    }
```

- [ ] **Step 6: Write the delivery** in `client/src-tauri/src/composer/send.rs`, below Task 4a's code; its `use` lines join the module's imports at the top:

```rust
use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zerus_client_core::actions::{Action, Outcome};
use zerus_client_core::attachments::{self, Attachment};
use zerus_client_core::cache::{ColdAction, ColdPrompt};
use zerus_client_core::local_messages::{self, Inspect, LocalMessages};
use zerus_client_core::terminal::{self, TerminalFacts};

const CHANGED: &str = "The session changed. Your draft was kept without sending.";

/// What `send_message`, `send_answer` and `local_retry` answer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendReply {
    /// The delivery started; its end arrives with the composer view.
    Sending,
    /// Nothing happened: another send or question runs, or the answer is stale.
    Busy,
    /// The composer shows `reason` (`deliveryFinished(false, reason)`).
    Blocked { reason: String },
    /// A sticky notice (`showNotice(..., true)`).
    Notice { text: String },
    /// Ask "Continue with a cold cache?"; answer with `token`.
    Cold { token: u64, prompt: ColdPrompt },
    /// The cold question's Clear context: open the Clear session dialog (Task 5).
    ClearContext { token: u64 },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Cold { action: ColdAction },
    Cancel,
}

/// A failed row's message, sent again by Retry.
#[derive(Clone, Debug)]
pub(crate) struct Retry {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
}

/// A question waiting for its answer, with the identity it was asked about.
#[derive(Clone, Debug)]
pub(crate) struct Prompt {
    pub(crate) key: String,
    pub(crate) session: SessionRef,
    pub(crate) run: String,
    pub(crate) conversation: String,
    pub(crate) retry: Option<Retry>,
}

/// One delivery: what is sent, to which run, with which consent.
#[derive(Clone, Debug)]
pub(crate) struct Delivery {
    pub(crate) key: String,
    pub(crate) session: SessionRef,
    pub(crate) created: i64,
    pub(crate) run: String,
    pub(crate) conversation: String,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) retry_id: Option<String>,
    pub(crate) compaction: Option<String>,
    pub(crate) consent: bool,
    /// The composer's draft is not the message (a Retry): keep the draft.
    pub(crate) preserve: bool,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

impl Context {
    pub(crate) fn run(&self) -> String {
        self.details.as_deref().map(|d| text(d, "run_id").to_owned()).unwrap_or_default()
    }
    pub(crate) fn conversation(&self) -> String {
        self.details.as_deref().map(|d| text(d, "conversation_id").to_owned()).unwrap_or_default()
    }
}

impl Composers {
    /// Asks a question about the open session; its answer brings this back.
    pub(crate) fn ask(&self, hub: &mut Hub, ctx: &Context, retry: Option<Retry>) -> u64 {
        hub.next_token += 1;
        let token = hub.next_token;
        // Only the latest few questions can be answered.
        hub.prompts.retain(|old, _| *old + 8 > token);
        hub.prompts.insert(token, Prompt {
            key: ctx.key.clone(), session: ctx.session.clone(), run: ctx.run(), conversation: ctx.conversation(), retry,
        });
        token
    }

    /// Send pressed (`MessageComposer::send` then `SessionsWindow::sendMessage`).
    pub fn send_message(&self, handle: u64, rev: u64, text: String, position: i64, anchor: i64) -> SendReply {
        self.edit(handle, rev, text, position, anchor);
        let mut hub = self.lock();
        let Some(ctx) = self.context(&hub) else { return SendReply::Busy };
        let Some(composer) = hub.composers.get(&ctx.key).filter(|c| c.handle == handle) else { return SendReply::Busy };
        if hub.closed || !composer.loaded || composer.busy() || composer.uncertain {
            return SendReply::Busy;
        }
        let (message, attachments) = (composer.draft.text.clone(), composer.draft.attachments.clone());
        if let Some(reason) = ctx.reason() {
            if let Some(composer) = hub.composers.get_mut(&ctx.key) {
                composer.show_error(reason.clone());
            }
            drop(hub);
            self.changed();
            return SendReply::Blocked { reason };
        }
        self.consent_or_deliver(&mut hub, &ctx, message, attachments, None)
    }

    /// The cold cache check, then the delivery (`sendMessage` 4815-4852).
    fn consent_or_deliver(&self, hub: &mut Hub, ctx: &Context, text: String, attachments: Vec<Attachment>, retry: Option<Retry>) -> SendReply {
        let details = ctx.details.clone().unwrap_or_default();
        let now = self.now();
        if cache::expired(&details, now) {
            let token = self.ask(hub, ctx, retry);
            let prompt = cache::cold_prompt(&details, self.compacting(hub, &ctx.key), ctx.facts.clearing, now);
            return SendReply::Cold { token, prompt };
        }
        let delivery = self.delivery(hub, ctx, text, attachments, retry, None, false);
        self.deliver(hub, delivery);
        SendReply::Sending
    }

    /// Whether a compaction waits to send for `key` (Task 5 fills it in).
    pub(crate) fn compacting(&self, _hub: &Hub, _key: &str) -> bool {
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn delivery(&self, hub: &Hub, ctx: &Context, text: String, attachments: Vec<Attachment>, retry: Option<Retry>, compaction: Option<String>, consent: bool) -> Delivery {
        let preserve = retry.is_some() && hub.composers.get(&ctx.key).is_some_and(|c| {
            c.draft.text != text || c.draft.attachments.len() != attachments.len()
                || c.draft.attachments.iter().zip(&attachments).any(|(a, b)| a != b)
        });
        Delivery {
            key: ctx.key.clone(), session: ctx.session.clone(), created: ctx.row.created,
            run: ctx.run(), conversation: ctx.conversation(), text, attachments,
            retry_id: retry.map(|r| r.id), compaction, consent, preserve,
        }
    }

    /// The answer to a question asked by this hub. It re-checks everything:
    /// the session's identity, and that the composer is neither busy nor (for
    /// its own draft) uncertain, so an older question never resends a draft
    /// whose delivery is unknown.
    pub fn answer(&self, token: u64, answer: Answer) -> SendReply {
        let mut hub = self.lock();
        let Some(prompt) = hub.prompts.remove(&token) else { return SendReply::Busy };
        let Answer::Cold { action } = answer else { return SendReply::Busy };
        let Some(ctx) = self.context(&hub) else { return SendReply::Busy };
        // Qt returns quietly when the question's session changed meanwhile.
        if ctx.key != prompt.key || ctx.session.computer != prompt.session.computer || ctx.session.name != prompt.session.name
            || ctx.run() != prompt.run || ctx.conversation() != prompt.conversation {
            return SendReply::Busy;
        }
        let Some(composer) = hub.composers.get(&prompt.key) else { return SendReply::Busy };
        if hub.closed || !composer.loaded || composer.busy() || (prompt.retry.is_none() && composer.uncertain) {
            return SendReply::Busy;
        }
        match action {
            ColdAction::Send => {
                if let Some(reason) = ctx.reason() {
                    return SendReply::Notice { text: reason };
                }
                let (text, attachments) = match &prompt.retry {
                    Some(retry) => (retry.text.clone(), retry.attachments.clone()),
                    None => (composer.draft.text.clone(), composer.draft.attachments.clone()),
                };
                let delivery = self.delivery(&hub, &ctx, text, attachments, prompt.retry.clone(), None, true);
                self.deliver(&mut hub, delivery);
                SendReply::Sending
            }
            // Task 5 runs these.
            ColdAction::Compact | ColdAction::Clear => SendReply::Busy,
        }
    }

    /// `prepareSendingAsync`: the draft is saved as "sending" before anything
    /// runs, so a crash afterwards reloads it as uncertain. A delivery drops
    /// the open questions of its key.
    pub(crate) fn deliver(&self, hub: &mut Hub, delivery: Delivery) {
        if hub.closed {
            return;
        }
        let Some(composer) = hub.composers.get_mut(&delivery.key) else { return };
        if composer.sending {
            return;
        }
        composer.sending = true;
        composer.waiting = None;
        composer.error = false;
        composer.notice = "Saving before sending…".into();
        self.0.store.save_durable(&composer.key, composer.stored(), {
            let me = self.clone();
            move |saved| me.prepared(delivery, saved)
        });
        let key = composer.key.clone();
        hub.prompts.retain(|_, prompt| prompt.key != key);
        self.changed();
    }

    fn prepared(&self, delivery: Delivery, saved: Result<(), String>) {
        if let Err(error) = saved {
            let mut hub = self.lock();
            if let Some(composer) = hub.composers.get_mut(&delivery.key) {
                composer.sending = false;
                composer.notice.clear();
                composer.storage_error = Some(error);
            }
            drop(hub);
            self.changed();
            return;
        }
        // The app quits: the draft is on disk as "sending" and reloads as
        // uncertain; nothing starts now.
        if self.lock().closed {
            return;
        }
        if !self.still_valid(&delivery) {
            self.delivery_finished(&delivery.key, false, CHANGED, false, delivery.preserve);
            return;
        }
        if let Some(composer) = self.lock().composers.get_mut(&delivery.key) {
            composer.notice = "Sending…".into();
        }
        self.changed();
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || me.send_now(delivery)));
    }

    /// Caches the files for Retry, records the local row, runs the one call.
    fn send_now(&self, delivery: Delivery) {
        let cache = self.cache();
        let files = match attachments::prepare(cache.root(), &delivery.attachments) {
            Ok(files) => files,
            Err(error) => return self.delivery_finished(&delivery.key, false, &error, false, delivery.preserve),
        };
        let me = self.clone();
        cache.prune_later(&self.0.deps.spawn, Arc::new(move || me.retry_files()));
        if !self.still_valid(&delivery) {
            return self.delivery_finished(&delivery.key, false, CHANGED, false, delivery.preserve);
        }
        // One request id per delivery, made here for its one dispatch: the
        // local row, the CLI request and the receipt check carry it, and no
        // other delivery reuses it.
        let request_id = uuid::Uuid::new_v4().to_string();
        let now = self.now();
        {
            let mut hub = self.lock();
            if hub.closed {
                return;
            }
            hub.local.sending(&delivery.key, &request_id, delivery.retry_id.as_deref(), &delivery.text, &files, &delivery.run, &delivery.conversation, now);
        }
        self.changed();
        let target = Target {
            computer: delivery.session.computer.clone(),
            name: delivery.session.name.clone(),
            run_id: delivery.run.clone(),
            conversation_id: delivery.conversation.clone(),
        };
        let action = Action::Send {
            text: delivery.text.clone(),
            attachments: delivery.attachments.clone(),
            agent_id: None,
            expected_compaction_id: delivery.compaction.clone(),
        };
        let outcome = match (self.0.deps.dispatch)(target, action, request_id.clone()) {
            Ok(outcome) => outcome,
            // A2 answers a busy send lane and every refusal before the call as
            // `Failed`; an `Err` means the call's thread died, so it may have run.
            Err(error) => Outcome::Uncertain { detail: error },
        };
        let detail = self.lock().local.finished(&delivery.key, &request_id, &outcome);
        let ok = matches!(outcome, Outcome::Done { .. });
        let uncertain = matches!(outcome, Outcome::Uncertain { .. });
        self.delivery_finished(&delivery.key, ok, &detail, uncertain, delivery.preserve);
    }

    /// Qt's `stillValid` (`SessionsWindow.cpp:4880-4904`).
    pub(crate) fn still_valid(&self, delivery: &Delivery) -> bool {
        let hub = self.lock();
        let Some((row, online)) = (self.0.deps.rows)(&delivery.session.computer, &delivery.session.key, &delivery.session.name) else { return false };
        let differs = |known: &Option<String>, expected: &str| known.as_deref().is_some_and(|value| !value.is_empty() && value != expected);
        if row.name != delivery.session.name || row.created != delivery.created || !online
            || differs(&row.run_id, &delivery.run) || differs(&row.conversation_id, &delivery.conversation) {
            return false;
        }
        if (self.0.deps.facts)(&delivery.session.computer, &delivery.session.name).is_some() {
            return false;
        }
        if let Some(id) = &delivery.retry_id {
            let retained = hub.local.rows(&delivery.key).iter().any(|r| &r.id == id && !r.error.is_empty() && !r.uncertain);
            if !retained {
                return false;
            }
        }
        let open = hub.open_key.as_deref() == Some(delivery.key.as_str());
        if open {
            let Some(ctx) = self.context(&hub) else { return false };
            let Some(details) = ctx.details.as_deref() else { return false };
            if text(details, "run_id") != delivery.run || text(details, "conversation_id") != delivery.conversation
                || text(details, "runtime_state") != "live" || text(details, "process_state") != "running"
                || ctx.reason().is_some() || (cache::expired(details, self.now()) && !delivery.consent) {
                return false;
            }
        }
        true
    }

    /// `MessageComposer::deliveryFinished`.
    pub(crate) fn delivery_finished(&self, key: &str, ok: bool, detail: &str, uncertain: bool, preserve: bool) {
        let mut hub = self.lock();
        if let Some(composer) = hub.composers.get_mut(key) {
            composer.sending = false;
            composer.waiting = None;
            if !preserve {
                composer.error = !ok;
                composer.uncertain = uncertain;
                composer.notice = if ok { "Submitted to terminal".into() } else { detail.to_owned() };
                if ok {
                    composer.draft = attachments::Draft::default();
                    composer.position = 0;
                    composer.anchor = 0;
                    composer.attachments_changed();
                }
            }
            composer.text_epoch += 1;
            self.save(composer);
        }
        drop(hub);
        self.reconcile();
        self.changed();
    }

    /// The cached files every failed, definite local row still needs.
    pub(crate) fn retry_files(&self) -> HashSet<PathBuf> {
        let hub = self.lock();
        hub.local_keys().into_iter()
            .flat_map(|key| hub.local.rows(&key).to_vec())
            .filter(|row| !row.error.is_empty() && !row.uncertain)
            .filter_map(|row| LocalMessages::stored_files(&row))
            .flatten()
            .map(|file| file.local_path)
            .collect()
    }

    /// Drops local rows the history now shows (`reconcileMessages`); a
    /// failed row delivered after all clears a draft that still holds it.
    pub(crate) fn reconcile(&self) {
        let mut hub = self.lock();
        let Some(key) = hub.open_key.clone() else { return };
        let Some(details) = hub.followed.details.clone() else { return };
        let events: Vec<Value> = hub.followed.events.iter().cloned().collect();
        let reconciled = hub.local.reconcile(&key, &details, &events);
        let clear = hub.composers.get(&key).is_some_and(|c| {
            !c.sending && c.draft.attachments.is_empty() && reconciled.delivered.contains(&c.draft.text)
        });
        drop(hub);
        if clear {
            self.delivery_finished(&key, true, "", false, false);
        }
    }

    /// Retry on a failed row (`messageAction` "retry"). Blocking: it reads
    /// the cached files back. Only a definite failure (S4's `retryable`) is
    /// ever sent again, as a new submission with its own request id.
    pub fn retry(&self, handle: u64, id: &str) -> SendReply {
        let (ctx, files, text) = {
            let mut hub = self.lock();
            let Some(ctx) = self.context(&hub) else { return SendReply::Busy };
            if hub.closed || !hub.composers.get(&ctx.key).is_some_and(|c| c.handle == handle && !c.busy()) {
                return SendReply::Busy;
            }
            let Some(row) = hub.local.retryable(&ctx.key, id, &ctx.run(), &ctx.conversation()).cloned() else { return SendReply::Busy };
            if let Some(reason) = ctx.reason() {
                return SendReply::Notice { text: reason };
            }
            if !hub.retry_reads.insert(ctx.key.clone()) {
                return SendReply::Busy;
            }
            let Some(files) = LocalMessages::stored_files(&row) else {
                hub.retry_reads.remove(&ctx.key);
                return SendReply::Busy;
            };
            (ctx, files, row.text)
        };
        let read = attachments::read_stored(self.cache().root(), &files);
        let mut hub = self.lock();
        hub.retry_reads.remove(&ctx.key);
        let attachments = match read {
            Ok(attachments) => attachments,
            Err(error) => return SendReply::Notice { text: error },
        };
        let Some(now) = self.context(&hub) else { return SendReply::Busy };
        if now.key != ctx.key || now.row.created != ctx.row.created || now.run() != ctx.run()
            || now.conversation() != ctx.conversation() || now.reason().is_some()
            || hub.local.retryable(&ctx.key, id, &ctx.run(), &ctx.conversation()).is_none() {
            return SendReply::Busy;
        }
        let retry = Retry { id: id.to_owned(), text: text.clone(), attachments: attachments.clone() };
        self.consent_or_deliver(&mut hub, &now, text, attachments, Some(retry))
    }

    pub fn delete_local(&self, handle: u64, id: &str) {
        let mut hub = self.lock();
        let Some(key) = hub.open_key.clone() else { return };
        if hub.composers.get(&key).is_some_and(|c| c.handle == handle) && hub.local.delete(&key, id) {
            drop(hub);
            self.changed();
        }
    }

    /// Inspect on a failed row: Native UI, Terminal, or a refresh with Qt's notice.
    pub fn inspect_local(&self, handle: u64, _id: &str) -> Option<Inspect> {
        let hub = self.lock();
        let ctx = self.context(&hub)?;
        hub.composers.get(&ctx.key).filter(|c| c.handle == handle)?;
        let facts = TerminalFacts {
            cmd: ctx.facts.cmd.clone(),
            terminal_supported: ctx.row.terminal_supported,
            terminal_reported: ctx.row.terminal_reported,
            terminal_attach_supported: ctx.row.terminal_attach_supported,
            terminal_attach_reported: ctx.row.terminal_attach_reported,
            ..TerminalFacts::default()
        };
        let details = ctx.details.as_deref().cloned().unwrap_or_default();
        let routed = local_messages::inspect(&ctx.facts.cmd, terminal::supported(&facts, &details), &ctx.row.machine);
        if matches!(routed, Inspect::Refresh { .. }) {
            drop(hub);
            self.refresh_details();
        }
        Some(routed)
    }

    #[cfg(test)]
    pub(crate) fn delivery_for_test(&self, handle: u64, message: &str) -> Delivery {
        let hub = self.lock();
        let ctx = self.context(&hub).unwrap();
        assert!(hub.composers.get(&ctx.key).is_some_and(|c| c.handle == handle));
        self.delivery(&hub, &ctx, message.to_owned(), Vec::new(), None, None, false)
    }
}
```

The commands, in `send.rs` (with `use tauri::State;`):

```rust
#[tauri::command(rename_all = "snake_case")]
pub async fn send_message(state: State<'_, Composers>, handle: u64, rev: u64, text: String, position: i64, anchor: i64) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.send_message(handle, rev, text, position, anchor)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn send_answer(state: State<'_, Composers>, token: u64, answer: Answer) -> SendReply {
    state.answer(token, answer)
}

#[tauri::command]
pub async fn local_retry(state: State<'_, Composers>, handle: u64, id: String) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.retry(handle, &id)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_delete(state: State<'_, Composers>, handle: u64, id: String) {
    state.delete_local(handle, &id);
}

#[tauri::command]
pub fn local_inspect(state: State<'_, Composers>, handle: u64, id: String) -> Option<Inspect> {
    state.inspect_local(handle, &id)
}
```

`Action::Send` is never deserialized: it is built here, from the held draft or the cached retry files.

- [ ] **Step 7: Register it** in `client/src-tauri/src/lib.rs`. `composer::Deps` gets:

```rust
dispatch: {
    let actions = app.state::<actions::Actions>().inner().clone();
    std::sync::Arc::new(move |target, action, id| tauri::async_runtime::block_on(actions::dispatch_as(&actions, target, action, id)))
},
```

It runs on the hub's spawned blocking threads, never on an async worker. The five commands join the live handler.

- [ ] **Step 8: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: actions::`
Expected: PASS (Tasks 3a-4a's tests still pass with the new dep). Clippy and fmt clean.

- [ ] **Step 9: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the guarded send with the cold cache question and local rows"
```

---

### Task 5: Continuations: resume-then-send, compact-to-send and Clear context

**Depends on:**
- S3 `zerus-5wmh.9.9` (branch `work-ba0ed9fd-continuation`): `send::{Session, can_start_stopped, start_question, StartChoice, StartQuestion, StartSend, CompactSend, Step, DraftMark, CANCELLED_BY_USER, INTERRUPTED}`, `Action::StartFresh`;
- C1 `zerus-5wmh.9.18` (branch `work-ba0ed9fd-context`): `Action::{ClearContext, CompactContext}`, `actions::clearable`;
- Tasks 4a and 4b;
- R1 for the start question's context text (`""` until then).

**Qt reference:** `tray/src/SessionsWindow.cpp`:
- `canStartStopped` (4672-4680), `startToSend` (4706-4770), `cancelStartContinuation` (4772-4778), `continueAfterStart` (4780-4802);
- `compactToSend` (4536-4565), `pollCompactContinuations` (4567-4592), `continueAfterCompact` (4594-4625), `cancelCompactContinuation` (4528-4534);
- the Cancel send button (958-968), the interrupt hook (1187-1191);
- `clearContext` (4637-4670);
- the composer's availability for a stopped session (3988).

**Files:**
- Modify: `client/src-tauri/src/composer/send.rs`, `client/src-tauri/src/composer/mod.rs`
- Modify: `client/src-tauri/src/actions.rs` (`WriteKind::Clear`)
- Modify: `client/src-tauri/Cargo.toml` (+ `sha2 = "0.10"`), `client/src-tauri/Cargo.lock`
- Modify: `client/src-tauri/src/lib.rs` (the three commands)
- Test: unit tests in `composer/send.rs` (module `continuations`) and `actions.rs`

**Interfaces:**
- Consumes: the S3 and C1 surfaces in "Depends on"; Task 4a's `Composers::{context, now, refresh_details}` and `Context`; Task 4b's `Composers::{ask, deliver, delivery, delivery_finished}`, `Prompt`, `Retry`, `Delivery`, `CHANGED` and the test helpers `send_hub`, `opened`, `ready`, `state`, `Fake`, `receipt`.
- Produces:

```rust
// SendReply gains
Start { token: u64, question: StartQuestion },
// Answer gains
Start { choice: StartChoice },
// ComposerView gains
pub cancel_send: bool,   // "Cancel send" while a start or compaction waits
pub compacting: bool,    // the compaction indicator
pub clear: ClearControl, // the cache popover's Clear context and the header's Clear session
#[derive(Serialize)] pub struct ClearControl { pub enabled: bool, pub dismiss: bool, pub title: String, pub text: String, pub confirm: String, pub run: String, pub conversation: String }
impl Composers {
    pub fn cancel_send(&self, handle: u64);
    pub fn interrupted(&self, handle: u64);           // before the composer's Stop interrupts
    /// After the Clear session confirmation, with the identity the dialog showed.
    pub fn clear_context(&self, handle: u64, run: &str, conversation: &str, dismiss: bool) -> SendReply;
}
// commands: send_cancel(handle), send_interrupted(handle), clear_context(handle, run, conversation, dismiss) -> SendReply
```

- [ ] **Step 1: Write the failing tests** in `client/src-tauri/src/composer/send.rs`:

```rust
#[cfg(test)]
mod continuations {
    use super::tests::{opened, ready, send_hub, state};
    use super::*;
    use crate::composer::lock;
    use crate::composer::tests::{dir, until};
    use serde_json::json;
    use zerus_client_core::send::StartChoice;

    fn stopped(fake: &super::tests::Fake) {
        let mut row = lock(&fake.row);
        row.state = "paused".into();
        row.resumable = true;
    }

    #[test]
    fn a_stopped_session_asks_how_to_start_and_sends_once_ready() {
        let root = dir("start");
        let (c, fake) = send_hub(&root);
        stopped(&fake);
        let mut details = ready();
        details["runtime_state"] = json!("stopped");
        details["process_state"] = json!("exited");
        let view = opened(&c, details);
        assert!(view.available, "Send stays enabled to ask");
        assert_eq!(view.unavailable_reason, "Session stopped. Send asks whether to resume it or start fresh.");
        let SendReply::Start { token, question } = c.send_message(view.handle, 1, "after start".into(), 11, 11) else { panic!("no question") };
        assert_eq!(question.title, "Start the session to send?");
        assert_eq!(c.answer(token, Answer::Start { choice: StartChoice::Resume }), SendReply::Sending);
        let view = until(&c, |v| v.waiting.is_some());
        assert!(view.cancel_send && !view.can_send);
        until(&c, |_| lock(&fake.calls).iter().any(|(_, a, _)| *a == Action::Resume));
        // The new run reports ready: the exact draft goes out, once.
        {
            let mut row = lock(&fake.row);
            row.state = "running".into();
            row.run_id = Some("run-2".into());
        }
        let mut after = ready();
        after["run_id"] = json!("run-2");
        c.observe(&state(after.clone()));
        c.observe(&state(after));
        until(&c, |v| v.waiting.is_none() && !v.sending);
        let sends: Vec<_> = lock(&fake.calls).iter().filter(|(_, a, _)| matches!(a, Action::Send { .. })).cloned().collect();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0.run_id, "run-2");
    }

    #[test]
    fn a_compaction_must_complete_before_the_send() {
        let root = dir("compact");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|action, id| match action {
            Action::CompactContext => Outcome::Done { receipt: json!({"status": "submitted", "request_id": "cmp-1"}), cleanup: None },
            _ => super::tests::receipt(action, id),
        });
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold"});
        let view = opened(&c, details.clone());
        let SendReply::Cold { token, prompt } = c.send_message(view.handle, 1, "compact me".into(), 0, 0) else { panic!() };
        assert!(prompt.choices.iter().any(|ch| ch.action == ColdAction::Compact && ch.enabled));
        assert_eq!(c.answer(token, Answer::Cold { action: ColdAction::Compact }), SendReply::Sending);
        until(&c, |v| v.compacting && v.cancel_send);
        until(&c, |_| lock(&fake.calls).iter().any(|(_, a, _)| *a == Action::CompactContext));
        details["compact_context_request"] = json!({"request_id": "cmp-1", "status": "running"});
        c.observe(&state(details.clone()));
        assert!(!lock(&fake.calls).iter().any(|(_, a, _)| matches!(a, Action::Send { .. })));
        details["compact_context_request"] = json!({"request_id": "cmp-1", "status": "completed"});
        c.observe(&state(details.clone()));
        c.observe(&state(details));
        until(&c, |v| !v.sending && !v.compacting);
        let sends: Vec<_> = lock(&fake.calls).iter().filter_map(|(_, a, _)| match a {
            Action::Send { expected_compaction_id, .. } => Some(expected_compaction_id.clone()),
            _ => None,
        }).collect();
        assert_eq!(sends, [Some("cmp-1".to_owned())]);
    }

    #[test]
    fn cancel_send_keeps_the_draft_and_an_interrupt_names_itself() {
        let root = dir("cancel");
        let (c, fake) = send_hub(&root);
        stopped(&fake);
        let view = opened(&c, ready());
        let SendReply::Start { token, .. } = c.send_message(view.handle, 1, "kept".into(), 0, 0) else { panic!() };
        c.answer(token, Answer::Start { choice: StartChoice::Fresh });
        until(&c, |v| v.waiting.is_some());
        c.cancel_send(view.handle);
        let view = until(&c, |v| v.waiting.is_none());
        assert_eq!(view.status, "Pending send cancelled. Your draft is unchanged.");
        assert_eq!(view.text, "kept");
        let SendReply::Start { token, .. } = c.send_message(view.handle, 2, "kept".into(), 0, 0) else { panic!() };
        c.answer(token, Answer::Start { choice: StartChoice::Resume });
        until(&c, |v| v.waiting.is_some());
        c.interrupted(view.handle);
        assert_eq!(until(&c, |v| v.waiting.is_none()).status, "Interrupted. Your draft was kept without sending.");
    }

    #[test]
    fn a_start_that_never_becomes_ready_gives_up_after_180_s() {
        let root = dir("timeout");
        let (c, fake) = send_hub(&root);
        stopped(&fake);
        let view = opened(&c, ready());
        let SendReply::Start { token, .. } = c.send_message(view.handle, 1, "late".into(), 0, 0) else { panic!() };
        c.answer(token, Answer::Start { choice: StartChoice::Resume });
        until(&c, |v| v.waiting.is_some());
        *lock(&fake.now) += 181.0;
        c.tick();
        let view = until(&c, |v| v.waiting.is_none());
        assert_eq!(view.status, "The agent did not become ready. Check Terminal; your draft was kept without sending.");
        assert!(!lock(&fake.calls).iter().any(|(_, a, _)| matches!(a, Action::Send { .. })));
    }

    #[test]
    fn clear_context_runs_on_the_write_lane_with_its_identity() {
        let root = dir("clear");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        assert!(view.clear.enabled && !view.clear.dismiss);
        assert_eq!(view.clear.title, "Clear session?");
        assert_eq!((view.clear.run.as_str(), view.clear.conversation.as_str()), ("run-1", "conv-1"));
        assert_eq!(c.clear_context(view.handle, "run-1", "conv-1", false), SendReply::Sending);
        until(&c, |_| lock(&fake.calls).iter().any(|(_, a, _)| *a == Action::ClearContext { dismiss_prompt: false }));
        let (target, _, _) = lock(&fake.calls).last().cloned().unwrap();
        assert_eq!((target.run_id.as_str(), target.conversation_id.as_str()), ("run-1", "conv-1"));
    }

    #[test]
    fn a_clear_confirmed_for_another_conversation_does_nothing() {
        let root = dir("clear-moved");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        let shown = view.clear.clone();
        let mut moved = ready();
        moved["conversation_id"] = json!("conv-2");
        c.observe(&state(moved));
        assert_eq!(c.clear_context(view.handle, &shown.run, &shown.conversation, shown.dismiss), SendReply::Busy);
        assert!(lock(&fake.calls).is_empty(), "Qt returns quietly when the session changed under its dialog");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::send::continuations`
Expected: compile errors (`Start`, `cancel_send`, `clear_context`).

- [ ] **Step 3: Implement the continuations.**
  - In `actions.rs`, `WriteKind` gains `Clear`, `WriteKind::of` maps `Action::ClearContext { .. }` to it, and `a_running_write_reports_its_kind` gains `assert_eq!(WriteKind::of(&Action::ClearContext { dismiss_prompt: false }), WriteKind::Clear);`. `context()` sets `clearing: write == Some(WriteKind::Clear)`.
  - In `mod.rs`, `Hub` gains `start: Option<(send::Pending, StartSend)>`, `compacts: HashMap<String, (send::Pending, CompactSend)>` and `last_poll: f64`. `Composer` gains `pub(crate) digests: Option<(u64, Vec<String>)>` (None in `new`), the attachment digests of one attachment list. `ComposerView` gains the fields in "Produces".
  - `Prompt` gains `rules: Option<Session>` and `mark: DraftMark`, which `ask` fills from the context and the composer.

In `send.rs`:

```rust
use zerus_client_core::send::{self as rules, CompactSend, DraftMark, Session, StartChoice, StartQuestion, StartSend, Step};

/// What a continuation sends when it is done: the composer's exact draft.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub(crate) key: String,
    pub(crate) session: SessionRef,
    pub(crate) retry: Option<Retry>,
}

/// The text and attachment digests S3 compares at every step. The SHA-256
/// digests are computed once per attachment list, not per check: while a
/// continuation waits, every Activity state and poll compares the mark.
pub(crate) fn mark(composer: &mut Composer) -> DraftMark {
    let digests = match &composer.digests {
        Some((list, digests)) if *list == composer.attachments_rev => digests.clone(),
        _ => {
            let digests: Vec<String> = composer.draft.attachments.iter().map(|a| digest(&a.bytes)).collect();
            composer.digests = Some((composer.attachments_rev, digests.clone()));
            digests
        }
    };
    DraftMark { text: composer.draft.text.clone(), attachments: digests }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

impl Composers {
    /// The catalog's view of the open session for S3 (`Session`).
    pub(crate) fn rules_session(ctx: &Context) -> Session {
        Session {
            key: ctx.key.clone(), host: ctx.session.computer.clone(), name: ctx.session.name.clone(), created: ctx.row.created,
            run_id: ctx.row.run_id.clone().unwrap_or_default(), conversation_id: ctx.row.conversation_id.clone().unwrap_or_default(),
            state: ctx.row.state.clone(), online: ctx.online, terminating: ctx.facts.terminating, tracked: ctx.row.tracked,
            cmd: ctx.facts.cmd.clone(), resumable: ctx.row.resumable,
        }
    }

    /// `canStartStopped`'s busy part: an update runs, another start waits, or
    /// this session compacts or sends.
    fn start_busy(&self, hub: &Hub, ctx: &Context) -> bool {
        ctx.facts.updating || hub.start.is_some() || hub.compacts.contains_key(&ctx.key)
            || hub.composers.get(&ctx.key).is_some_and(|c| c.sending)
    }

    fn draft_mark(hub: &mut Hub, key: &str) -> DraftMark {
        hub.composers.get_mut(key).map(mark).unwrap_or_default()
    }
}
```

Then wire them in:
- `context()` sets `start_send_waiting: hub.start.as_ref().is_some_and(|(p, _)| p.key == key)` and `compacting: hub.compacts.contains_key(&key)`.
- `compacting()` returns `hub.compacts.contains_key(key)`.
- `session_view` uses the stopped-session availability (`SessionsWindow.cpp:3988`):

```rust
    let can_start = rules::can_start_stopped(&Composers::rules_session(&ctx), composers.start_busy(hub, &ctx));
    let (available, unavailable_reason) = match &reason {
        Some(_) if can_start => (true, "Session stopped. Send asks whether to resume it or start fresh.".to_owned()),
        Some(reason) => (false, reason.clone()),
        None => (true, String::new()),
    };
```

- `send_message` asks the start question after Task 4b's composer checks and draft copy and before the block reason, for a send that is not a retry (`sendMessage` 4809):

```rust
        let session = Composers::rules_session(&ctx);
        if rules::can_start_stopped(&session, self.start_busy(&hub, &ctx)) {
            let token = self.ask(&mut hub, &ctx, None);
            return SendReply::Start { token, question: rules::start_question(&session, "") };
        }
```

  `""` is the R1 context text; when R1 merges, pass `session_usage::context(&details["session_usage"], ctx.recorded()).text`. `ask` stores the question's `Session` (`rules`) and the composer's `DraftMark` (`mark`) in the `Prompt`.

`answer` keeps Task 4b's identity and composer checks for every answer (a busy or uncertain draft is never started or sent by an older question), then handles the start choices and the compaction:

```rust
            // Answer::Start { choice }
            let Some(original) = prompt.rules.clone() else { return SendReply::Busy };
            let current = Composers::rules_session(&ctx);
            let now_mark = Composers::draft_mark(&mut hub, &prompt.key);
            match StartSend::begin(&original, Some(&current), self.start_busy(&hub, &ctx), choice, prompt.mark.clone(), &now_mark, self.now()) {
                Err(notice) => SendReply::Notice { text: notice },
                Ok((start, waiting)) => {
                    let pending = Pending { key: prompt.key.clone(), session: ctx.session.clone(), retry: None };
                    hub.start = Some((pending, start));
                    self.wait(&mut hub, &prompt.key, waiting, true);
                    SendReply::Sending
                }
            }
            // ColdAction::Compact (sendMessage 4843-4846)
            if Composers::draft_mark(&mut hub, &prompt.key) != prompt.mark {
                return SendReply::Notice { text: "Draft changed. Send it when you are ready.".into() };
            }
            let (compact, waiting) = CompactSend::begin(&Composers::rules_session(&ctx), &prompt.run, &prompt.conversation, prompt.mark.clone(), prompt.retry.as_ref().map(|r| r.id.clone()), self.now());
            let pending = Pending { key: prompt.key.clone(), session: ctx.session.clone(), retry: prompt.retry.clone() };
            hub.compacts.insert(prompt.key.clone(), (pending, compact));
            self.wait(&mut hub, &prompt.key, waiting, false);
            SendReply::Sending
            // ColdAction::Clear (4841)
            SendReply::ClearContext { token }
```

`wait` is `prepareWaitingToSendAsync`: it sets `composer.waiting = Some(text)`, saves durably and then calls `prepared`:

```rust
    fn wait(&self, hub: &mut Hub, key: &str, waiting: String, start: bool) {
        let Some(composer) = hub.composers.get_mut(key) else { return };
        composer.waiting = Some(waiting);
        let me = self.clone();
        let owned = key.to_owned();
        self.0.store.save_durable(key, composer.stored(), move |saved| me.waiting_saved(&owned, saved.is_ok(), start));
        self.changed();
    }

    /// The waiting draft is durable: S3's `prepared` decides whether to run.
    /// A closed hub (the app quits) starts nothing; the draft reloads as it
    /// was saved, not sent.
    fn waiting_saved(&self, key: &str, ok: bool, start: bool) {
        let mut hub = self.lock();
        if hub.closed {
            return;
        }
        let ctx = self.context(&hub);
        let current = ctx.as_ref().filter(|c| c.key == key).map(Composers::rules_session);
        let evidence = ctx.as_ref().filter(|c| c.key == key).and_then(|c| c.details.as_deref().cloned());
        let busy = ctx.as_ref().is_some_and(|c| c.facts.updating);
        let step = if start {
            match hub.start.as_mut() {
                Some((pending, start)) if pending.key == key => start.prepared(ok, current.as_ref(), evidence.as_ref(), busy),
                _ => return,
            }
        } else {
            match hub.compacts.get_mut(key) {
                Some((_, compact)) => compact.prepared(ok, current.as_ref(), evidence.as_ref(), evidence.as_ref(), busy),
                None => return,
            }
        };
        drop(hub);
        self.step(key, start, step);
    }
```

`step` carries out S3's answer; a `Run` goes through `dispatch` on the write lane and feeds the outcome back:

```rust
    pub(crate) fn step(&self, key: &str, start: bool, step: Step) {
        match step {
            Step::Idle => {}
            Step::Cancelled(text) => self.cancel(key, start, &text),
            // A start or compaction whose outcome is uncertain ends the
            // continuation (S3 answers `Cancelled`): the message is never sent after it.
            Step::Run(action) => {
                let Some(target) = self.target_of(key) else { return self.cancel(key, start, CHANGED) };
                let me = self.clone();
                let owned = key.to_owned();
                (self.0.deps.spawn)(Box::new(move || {
                    let id = uuid::Uuid::new_v4().to_string();
                    let outcome = (me.0.deps.dispatch)(target, action, id)
                        .unwrap_or_else(|error| Outcome::Uncertain { detail: error });
                    let mut hub = me.lock();
                    let next = if start {
                        match hub.start.as_mut() { Some((p, s)) if p.key == owned => s.start_finished(&outcome), _ => Step::Idle }
                    } else {
                        match hub.compacts.get_mut(&owned) { Some((_, c)) => c.compact_finished(&outcome), None => Step::Idle }
                    };
                    drop(hub);
                    me.step(&owned, start, next);
                    me.changed();
                }));
            }
            Step::Compact(compact, waiting) => {
                let mut hub = self.lock();
                if let Some((pending, _)) = hub.start.take() {
                    hub.compacts.insert(key.to_owned(), (pending, compact));
                    self.wait(&mut hub, key, waiting, false);
                }
            }
            Step::Send { run_id, conversation_id, compaction_id } => {
                let mut hub = self.lock();
                let pending = if start { hub.start.take().map(|(p, _)| p) } else { hub.compacts.remove(key).map(|(p, _)| p) };
                let Some(pending) = pending else { return };
                let Some(composer) = hub.composers.get(key) else { return };
                let (text, attachments) = match &pending.retry {
                    Some(retry) => (retry.text.clone(), retry.attachments.clone()),
                    None => (composer.draft.text.clone(), composer.draft.attachments.clone()),
                };
                // A Retry's message is not the draft: a newer draft stays (Qt's preserveDraft).
                let preserve = pending.retry.is_some() && (composer.draft.text != text || composer.draft.attachments != attachments);
                // The open session, when this is it: the webview reopened it under the new run's catalog key.
                let session = match (&hub.open, hub.open_key.as_deref()) {
                    (Some(open), Some(open_key)) if open_key == key => open.clone(),
                    _ => pending.session.clone(),
                };
                let created = (self.0.deps.rows)(&session.computer, &session.key, &session.name).map(|(row, _)| row.created).unwrap_or_default();
                let delivery = Delivery {
                    key: key.to_owned(), session, created, run: run_id, conversation: conversation_id,
                    text, attachments, retry_id: pending.retry.as_ref().map(|r| r.id.clone()), compaction: compaction_id,
                    // The send counts as consent to the full context (Qt's contextChosen).
                    consent: true, preserve,
                };
                if let Some(composer) = hub.composers.get_mut(key) {
                    composer.waiting = None;
                }
                self.deliver(&mut hub, delivery);
            }
        }
    }

    /// Ends a continuation without sending: the draft stays (`cancel*Continuation`).
    pub(crate) fn cancel(&self, key: &str, start: bool, text: &str) {
        let mut hub = self.lock();
        if start { hub.start = None; } else { hub.compacts.remove(key); }
        drop(hub);
        self.delivery_finished(key, false, text, false, false);
    }
```

`target_of(key)` builds the `Target` from the continuation's session (or the open session) and the current row's run and conversation:

```rust
    fn target_of(&self, key: &str) -> Option<Target> {
        let hub = self.lock();
        let session = hub.start.as_ref().filter(|(p, _)| p.key == key).map(|(p, _)| p.session.clone())
            .or_else(|| hub.compacts.get(key).map(|(p, _)| p.session.clone()))
            .or_else(|| hub.open.clone().filter(|_| hub.open_key.as_deref() == Some(key)))?;
        let (row, _) = (self.0.deps.rows)(&session.computer, &session.key, &session.name)?;
        Some(Target { computer: session.computer, name: session.name, run_id: row.run_id.unwrap_or_default(), conversation_id: row.conversation_id.unwrap_or_default() })
    }
```
 A resume that leaves the stopped row in place keeps working, since the row of the same catalog key is looked up by name when the key changed: it scans `rows` only for the open session, so `Pending.session.key` is refreshed in `open` when the webview reopens the same draft key under a new catalog key (`replaced` hands the selection to the new run, `session.ts` `onReplaced`).

Every Activity state of the session and every poll feeds `inspected`. `after_observe` becomes `self.reconcile()`, then `self.continue_with(key, details)` for the open key, then `self.changed()`:

```rust
    pub(crate) fn continue_with(&self, key: &str, details: &Value) {
        let now = self.now();
        let mut hub = self.lock();
        let draft_now = Composers::draft_mark(&mut hub, key);
        let terminating = self.context(&hub).is_some_and(|c| c.key == key && c.facts.terminating);
        let pending = self.context(&hub).is_some_and(|c| c.key == key && c.facts.updating);
        let start_step = match hub.start.as_mut() {
            Some((p, s)) if p.key == key => Some(s.inspected(details, &draft_now, pending, terminating, now)),
            _ => None,
        };
        let compact_step = hub.compacts.get_mut(key).map(|(_, c)| c.inspected(details, &draft_now, terminating, now));
        drop(hub);
        if let Some(step) = start_step { self.step(key, true, step); }
        if let Some(step) = compact_step { self.step(key, false, step); }
    }
```

The duplicate states in the tests are safe: S3's `CompactSend` marks itself sent before it returns `Send`, `step` takes a start's continuation out of the hub before it delivers (a second state finds none), and `deliver` refuses a composer that is already sending.

`tick` gains the 2.5 s poll (`pollCompactContinuations`): each continuation calls `tick(now)` (180 s and 600 s limits, giving `Cancelled`), outside the hub's lock:

```rust
        let now = self.now();
        let steps: Vec<(String, bool, Step)> = {
            let hub = self.lock();
            hub.start.iter().map(|(p, s)| (p.key.clone(), true, s.tick(now)))
                .chain(hub.compacts.iter().map(|(k, (_, c))| (k.clone(), false, c.tick(now))))
                .collect()
        };
        for (key, start, step) in steps {
            self.step(&key, start, step);
        }
```

Every 2.5 s (`last_poll`), a continuation whose session is not the open one (or is open without a state) and that `polling()` (compactions) or is a start runs one `inspect` through `deps.inspect`. The result goes to `continue_with`. One inspect per continuation is in flight at a time, and every poll runs off the UI thread.

Cancel send, the interrupt hook and Clear context:

```rust
    pub fn cancel_send(&self, handle: u64) {
        let key = { let hub = self.lock(); hub.composers.values().find(|c| c.handle == handle).map(|c| c.key.clone()) };
        let Some(key) = key else { return };
        let (compact, start) = { let hub = self.lock(); (hub.compacts.contains_key(&key), hub.start.as_ref().is_some_and(|(p, _)| p.key == key)) };
        if compact { self.cancel(&key, false, rules::CANCELLED_BY_USER); }
        if start { self.cancel(&key, true, rules::CANCELLED_BY_USER); }
    }

    /// The composer's Stop interrupts the turn: a waiting send ends first (1187-1191).
    pub fn interrupted(&self, handle: u64) {
        let key = { let hub = self.lock(); hub.composers.values().find(|c| c.handle == handle).map(|c| c.key.clone()) };
        let Some(key) = key else { return };
        let (compact, start) = { let hub = self.lock(); (hub.compacts.contains_key(&key), hub.start.as_ref().is_some_and(|(p, _)| p.key == key)) };
        if compact { self.cancel(&key, false, rules::INTERRUPTED); }
        if start { self.cancel(&key, true, rules::INTERRUPTED); }
    }

    /// Clear session confirmed (`clearContext` 4637-4670): the identity the
    /// dialog showed (run, conversation, the dismiss variant) must still hold,
    /// as Qt checks after its dialog, then C1's call runs on the write lane.
    pub fn clear_context(&self, handle: u64, run: &str, conversation: &str, dismiss: bool) -> SendReply {
        let hub = self.lock();
        let Some(ctx) = self.context(&hub) else { return SendReply::Busy };
        if hub.closed || !hub.composers.get(&ctx.key).is_some_and(|c| c.handle == handle && !c.sending) {
            return SendReply::Busy;
        }
        let details = ctx.details.as_deref().cloned().unwrap_or_default();
        let control = clear_control(&ctx, &details);
        if !control.enabled || control.run != run || control.conversation != conversation || control.dismiss != dismiss {
            return SendReply::Busy;
        }
        drop(hub);
        let Some(target) = self.target_of(&ctx.key) else { return SendReply::Busy };
        let me = self.clone();
        let dismiss = control.dismiss;
        (self.0.deps.spawn)(Box::new(move || {
            let outcome = (me.0.deps.dispatch)(target, Action::ClearContext { dismiss_prompt: dismiss }, uuid::Uuid::new_v4().to_string());
            if let Ok(Outcome::Failed { detail } | Outcome::Uncertain { detail }) | Err(detail) = outcome {
                let mut hub = me.lock();
                if let Some(composer) = hub.open_key.clone().and_then(|key| hub.composers.get_mut(&key)) {
                    composer.show_error(detail);
                }
            }
            me.changed();
        }));
        SendReply::Sending
    }
```

`clear_control` gives Qt's texts. The dialog text is "Clear the conversation context in {name} on {machine}?\n\nThis runs the agent’s native clear command. Future messages will start without the current conversation context. Your unsent draft will be kept." with the button "Clear session". With the dismiss variant (`clear_context_supported` false, `clear_context_dismiss` true) the text is "Claude in {name} on {machine} is waiting for a choice in Terminal. Dismiss it and clear the conversation context?\n\nThis presses Esc once, as in Terminal: a pending approval is denied and a question stays unanswered. Then the agent’s native clear command runs. Your unsent draft will be kept." with the button "Dismiss and clear". It is enabled when the session is online, `actions::clearable(details)` holds, there is no inspect error and no clear or send runs:

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ClearControl { pub enabled: bool, pub dismiss: bool, pub title: String, pub text: String, pub confirm: String, pub run: String, pub conversation: String }

fn clear_control(ctx: &Context, details: &Value) -> ClearControl {
    let dismiss = !details["clear_context_supported"].as_bool().unwrap_or(false);
    let (name, machine) = (&ctx.session.name, &ctx.row.machine);
    ClearControl {
        enabled: ctx.online && zerus_client_core::actions::clearable(details) && !ctx.facts.inspect_error && !ctx.facts.clearing,
        dismiss,
        title: "Clear session?".into(),
        text: if dismiss {
            format!("Claude in {name} on {machine} is waiting for a choice in Terminal. Dismiss it and clear the conversation context?\n\nThis presses Esc once, as in Terminal: a pending approval is denied and a question stays unanswered. Then the agent’s native clear command runs. Your unsent draft will be kept.")
        } else {
            format!("Clear the conversation context in {name} on {machine}?\n\nThis runs the agent’s native clear command. Future messages will start without the current conversation context. Your unsent draft will be kept.")
        },
        confirm: if dismiss { "Dismiss and clear" } else { "Clear session" }.into(),
        run: ctx.run(),
        conversation: ctx.conversation(),
    }
}
```

`view_of` fills `cancel_send` (a compaction or a start waits for this key), `compacting` (a compaction for this key) and `clear` (`clear_control` of the open session). Commands: `send_cancel(handle)`, `send_interrupted(handle)` and `clear_context(handle, run, conversation, dismiss) -> SendReply`, registered with the others.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: actions::`
Expected: PASS (Tasks 3a-5). Clippy and fmt clean. The sha2 addition updates `Cargo.lock` as in Task 3a.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): resume-then-send, compact-to-send and Clear context"
```

---

### Task 6: The composer view

**Depends on:**
- Tasks 3a-5;
- for the Mark as read chip, `markReadChip(session)` and `runMarkRead(row)` from the menu-parity Piece B (`zerus-5wmh.16`, PR #406, `ui/sessions/mark-entries.ts`): until it merges, the chip stays hidden;
- for the Context chip, R1: until it merges, the chip stays hidden.

**Qt reference:**
- `tray/src/MessageComposer.cpp`: the layout (300-366), the editor's Enter, Escape and paste hooks (380-400), `updateControls` (1141-1229), `rebuildAttachments` and `removeAttachment` (1008-1065).
- `tray/src/SessionsWindow.cpp`:
  - the toolbar chips: Mark as read (981-983, 3777-3780), the compaction indicator and Cancel send (958-968, 3763), the cache chip and popover (987-1002);
  - the cold cache dialog (4815-4836), the start dialog (4706-4730), the Clear session dialog (4646-4656).
- `tray/src/ActivityView.cpp`: the compaction indicator (1269-1279, 1899-1901) and the jump button (1625-1636).
- Mockup: `design/webui/src/screens/sessions/Composer.tsx` and `Attachments.tsx`, whose CSS is already in `client/src/ui/styles/detail.css`.

**Files:**
- Modify: `client/src/types.ts`, `client/src/bridge.ts`
- Create: `client/src/store/composer.ts`, `client/src/store/composer.test.ts`
- Create: `client/src/ui/sessions/Composer.tsx`, `client/src/ui/sessions/Attachments.tsx`, `client/src/ui/sessions/ComposerDialogs.tsx`
- Modify: `client/src/store/session.ts` (route `composer`, open the selected session's composer)
- Modify: `client/src/store/catalog.ts` (`setDraftKeys`, additive)
- Modify: `client/src/store/actions.ts` (export `notify`, additive, unless #406 already exports it)
- Modify: `client/src/ui/sessions/Activity.tsx` (export the jump state: `following`, `unseen`, `jumpToLatest`)
- Modify: `client/src/ui/sessions/SessionDetail.tsx` (the composer column under Activity; the header's Clear session entry and button)
- Modify: `client/src/ui/styles/detail.css` (the rules Step 7 names, where the port lacks them), `client/src/ui/lib/icons.generated.ts` (any icon the generator must add)

**Interfaces:**
- Consumes: Tasks 3a-5's commands and the serde shapes of `ComposerView`, `SendReply`, `Answer`, `ColdPrompt`, `StartQuestion`, `ClearControl`, `LocalMessage` and `Chip`.
- Produces:

```ts
// types.ts (mirrors of the Rust shapes; snake_case fields)
export interface AttachmentView { name: string; mime: string; reference: string; bytes: number; path: string; excerpt: string | null }
export interface Chip { visible: boolean; tone: "quiet" | "danger" | "warning" | "success"; label: string; short_label: string }
export interface ClearControl { enabled: boolean; dismiss: boolean; title: string; text: string; confirm: string; run: string; conversation: string }
export interface LocalMessage { id: string; text: string; status: "sending" | "sent" | "error"; attachments: Record<string, unknown>[]; run_id: string; conversation_id: string;
  submitted_at: number; error: string; uncertain: boolean; message_id: string | null; submitted_text: string | null; source: string | null; voice: Record<string, unknown> | null }
export interface ComposerView { handle: number; key: string; loaded: boolean; rev: number; text_epoch: number; text: string; position: number; anchor: number;
  attachments: AttachmentView[]; removed: string | null; attaching: boolean; sending: boolean; waiting: string | null; uncertain: boolean;
  status: string; status_error: boolean; can_send: boolean; send_label: string; available: boolean; unavailable_reason: string; block_reason: string | null;
  attachments_available: boolean; cache: Chip; cache_warning: string; cache_details: string; local: LocalMessage[];
  cancel_send: boolean; compacting: boolean; clear: ClearControl }
export interface ColdChoice { action: "send" | "compact" | "clear"; label: string; enabled: boolean; tooltip: string }
export interface ColdPrompt { title: string; text: string; informative: string; choices: ColdChoice[] }
export interface StartOption { choice: "resume" | "resume_compact" | "fresh"; label: string; enabled: boolean; tooltip: string }
export interface StartQuestion { title: string; text: string; informative: string; options: StartOption[] }
export type SendReply = { kind: "sending" } | { kind: "busy" } | { kind: "blocked"; reason: string } | { kind: "notice"; text: string }
  | { kind: "cold"; token: number; prompt: ColdPrompt } | { kind: "clear_context"; token: number } | { kind: "start"; token: number; question: StartQuestion };
export type Answer = { kind: "cold"; action: ColdChoice["action"] } | { kind: "start"; choice: StartOption["choice"] } | { kind: "cancel" };
export type Inspect = { kind: "native_ui" } | { kind: "terminal" } | { kind: "refresh"; notice: string };
export interface SessionRef { computer: ComputerId; key: string; name: string; archive_id: string | null }
// Batch gains: composer?: ComposerView; draft_keys?: string[]
// store/composer.ts
export const EDIT_MS = 250;
export const MAX_TEXT_BYTES = 64 * 1024;
export function createComposer(port: ComposerPort, timers?: Timers, images?: Images): ComposerStore;
export const composer: ComposerStore;
// ComposerStore: view(), text(), selection(), locked(), sendEnabled(), status(), dialog(), apply(view), open(session|undefined),
//   input(text, start, end), select(start, end), flush(), attachFiles(), pasteImage(blob), remove(i), undo(), send(),
//   answer(answer), closeDialog(), cancelSend(), allowRetry(), interrupted(), confirmClear()
```

- [ ] **Step 1: Write the failing store tests** in `client/src/store/composer.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createComposer, EDIT_MS, type ComposerPort, type Images } from "./composer";
import type { ComposerView, SendReply } from "../types";
import type { Timers } from "./terminal";

const view = (extra: Partial<ComposerView> = {}): ComposerView => ({
  handle: 1, key: "studio\nclaude/sample-project/review", loaded: true, rev: 0, text_epoch: 1, text: "", position: 0, anchor: 0,
  attachments: [], removed: null, attaching: false, sending: false, waiting: null, uncertain: false, status: "", status_error: false,
  can_send: true, send_label: "Send", available: true, unavailable_reason: "", block_reason: null, attachments_available: true,
  cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "", local: [],
  cancel_send: false, compacting: false, clear: { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-1", conversation: "conv-1" }, ...extra,
});

function fakes(reply: SendReply = { kind: "sending" }) {
  const calls: string[] = [];
  let attachDone: (() => void) | undefined;
  const port: ComposerPort = {
    open: async () => view(),
    edit: async (_h, rev, text) => { calls.push(`edit ${rev} ${text}`); },
    pick: () => new Promise<void>((resolve) => { calls.push("pick"); attachDone = resolve; }),
    pasteImage: async (_h, bytes) => { calls.push(`paste ${bytes.length}`); },
    send: async (_h, rev, text) => { calls.push(`send ${rev} ${text}`); return reply; },
    answer: async (token, answer) => { calls.push(`answer ${token} ${JSON.stringify(answer)}`); return { kind: "sending" }; },
    remove: async () => {}, undo: async () => {}, allowRetry: async () => {}, cancelSend: async () => {}, interrupted: async () => {},
    clear: async (_h, run, conversation) => { calls.push(`clear ${run} ${conversation}`); return { kind: "sending" }; },
  };
  let clock = 0;
  const jobs: { at: number; fn: () => void }[] = [];
  const timers: Timers = { now: () => clock, later: (fn, ms) => { const j = { at: clock + ms, fn }; jobs.push(j); return () => jobs.splice(jobs.indexOf(j), 1); } };
  const advance = (ms: number) => { clock += ms; for (const j of jobs.filter((x) => x.at <= clock)) { jobs.splice(jobs.indexOf(j), 1); j.fn(); } };
  return { calls, port, timers, advance, finishAttach: () => attachDone?.() };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const images = (width: number, height: number): Images => ({ size: async () => ({ width, height }), png: async () => new Uint8Array([1, 2, 3]) });

describe("composer store", () => {
  it("edits are debounced and flushed before a send", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view());
      c.input("h", 1, 1); c.input("he", 2, 2); c.input("hel", 3, 3);
      f.advance(EDIT_MS);
      await tick();
      expect(f.calls).toEqual(["edit 3 hel"]);
      c.input("hello", 5, 5);
      await c.send();
      expect(f.calls.slice(1)).toEqual(["edit 4 hello", "send 4 hello"]);
    });
  });

  it("a higher text epoch replaces the text; an older echo does not", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view({ text: "loaded", text_epoch: 2, rev: 0 }));
      expect(c.text()).toBe("loaded");
      c.input("typing", 6, 6);
      c.apply(view({ text: "loaded", text_epoch: 2, rev: 0 }));
      expect(c.text()).toBe("typing");
      c.apply(view({ text: "loaded [File #1] ", text_epoch: 3, rev: 1 }));
      expect(c.text()).toBe("loaded [File #1] ");
    });
  });

  it("over 64 KiB disables Send at once and shows Qt's text", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view());
      c.input("ж".repeat(32 * 1024 + 1), 0, 0);
      expect(c.sendEnabled()).toBe(false);
      expect(c.status()).toEqual({ text: "Message exceeds 64 KiB.", error: true });
    });
  });

  it("one attach at a time, and the editor is read-only meanwhile", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view());
      const first = c.attachFiles();
      await tick();
      expect(c.locked()).toBe(true);
      await c.attachFiles();
      expect(f.calls.filter((x) => x === "pick").length).toBe(1);
      f.finishAttach();
      await first;
      expect(c.locked()).toBe(false);
    });
  });

  it("a non-PNG paste of 32 megapixels or more is refused before any call", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers, images(8000, 4000));
      c.apply(view());
      await c.pasteImage(new Blob([new Uint8Array([0xff, 0xd8])], { type: "image/jpeg" }));
      expect(f.calls.some((x) => x.startsWith("paste"))).toBe(false);
      expect(c.status()).toEqual({ text: "Image is too large. Use an image below 32 megapixels.", error: true });
      const small = createComposer(f.port, f.timers, images(800, 600));
      small.apply(view());
      await small.pasteImage(new Blob([new Uint8Array([0xff, 0xd8])], { type: "image/jpeg" }));
      expect(f.calls.at(-1)).toBe("paste 3");
    });
  });

  it("a cold reply opens the question and its answer goes back with the token", async () => {
    const f = fakes({ kind: "cold", token: 9, prompt: { title: "Continue with a cold cache?", text: "t", informative: "", choices: [] } });
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view());
      c.input("hi", 2, 2);
      await c.send();
      expect(c.dialog()?.kind).toBe("cold");
      await c.answer({ kind: "cold", action: "send" });
      expect(f.calls.at(-1)).toBe(`answer 9 {"kind":"cold","action":"send"}`);
      expect(c.dialog()).toBeUndefined();
    });
  });
});
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && npm test -- src/store/composer.test.ts`
Expected: FAIL, `./composer` does not exist.

- [ ] **Step 3: Write `client/src/store/composer.ts`**

```ts
import { batch, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import type { Timers } from "./terminal";
import type { Answer, ClearControl, ColdPrompt, ComposerView, SendReply, SessionRef, StartQuestion } from "../types";

// The open composer (Qt MessageComposer) over the Rust hub. The webview owns
// the text while the user types: edits go to Rust 250 ms after the last key,
// on blur and before any attach, paste or send, each with a growing `rev`.
// Rust owns the attachments and the delivery state, and a growing
// `text_epoch` hands the text back (a load, an attach or remove, a
// delivery's end). One attach or paste runs at a time; the editor is
// read-only while it does, as Qt's synchronous attach is.

export const EDIT_MS = 250;
export const MAX_TEXT_BYTES = 64 * 1024;
const MAX_PIXELS = 32_000_000;
const TOO_LONG = "Message exceeds 64 KiB.";
const TOO_LARGE = "Image is too large. Use an image below 32 megapixels.";
const UNREADABLE = "Could not read the pasted image.";

export interface ComposerPort {
  open(session: SessionRef | undefined): Promise<ComposerView | null>;
  edit(handle: number, rev: number, text: string, position: number, anchor: number): Promise<void>;
  pick(handle: number, start: number, end: number): Promise<void>;
  pasteImage(handle: number, png: Uint8Array, start: number, end: number): Promise<void>;
  send(handle: number, rev: number, text: string, position: number, anchor: number): Promise<SendReply>;
  answer(token: number, answer: Answer): Promise<SendReply>;
  remove(handle: number, index: number): Promise<void>;
  undo(handle: number): Promise<void>;
  allowRetry(handle: number): Promise<void>;
  cancelSend(handle: number): Promise<void>;
  interrupted(handle: number): Promise<void>;
  clear(handle: number, run: string, conversation: string, dismiss: boolean): Promise<SendReply>;
}

/** Image decoding for pastes that are not PNG; tests pass their own. */
export interface Images {
  size(blob: Blob): Promise<{ width: number; height: number }>;
  png(blob: Blob): Promise<Uint8Array>;
}

const browserImages: Images = {
  async size(blob) { const bitmap = await createImageBitmap(blob); const size = { width: bitmap.width, height: bitmap.height }; bitmap.close(); return size; },
  async png(blob) {
    const bitmap = await createImageBitmap(blob);
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
    bitmap.close();
    return new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer());
  },
};

const realTimers: Timers = { now: () => performance.now(), later: (fn, ms) => { const id = setTimeout(fn, ms); return () => clearTimeout(id); } };
const utf8 = new TextEncoder();

export type Dialog =
  | { kind: "cold"; token: number; prompt: ColdPrompt }
  | { kind: "start"; token: number; question: StartQuestion }
  | { kind: "clear"; control: ClearControl; token?: number };

export function createComposer(port: ComposerPort, timers: Timers = realTimers, images: Images = browserImages) {
  const [view, setView] = createSignal<ComposerView | undefined>();
  const [text, setText] = createSignal("");
  const [selection, setSelection] = createSignal({ start: 0, end: 0 });
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string | undefined>();
  const [dialog, setDialog] = createSignal<Dialog | undefined>();
  let rev = 0;
  let epoch = 0;
  let dirty = false;
  let timer: (() => void) | undefined;

  const handle = () => view()?.handle;
  const tooLong = () => utf8.encode(text()).length > MAX_TEXT_BYTES;
  const locked = () => pending() || !!view()?.attaching || !!view()?.sending || !!view()?.waiting || !view()?.loaded;

  function apply(next: ComposerView | undefined) {
    batch(() => {
      if (!next) { setView(undefined); return; }
      if (next.handle !== view()?.handle) { epoch = 0; rev = next.rev; dirty = false; }
      setView(next);
      if (next.text_epoch > epoch) {
        epoch = next.text_epoch;
        rev = Math.max(rev, next.rev);
        dirty = false;
        setText(next.text);
        setSelection({ start: next.anchor, end: next.position });
        setError(undefined);
      }
    });
  }

  async function flush() {
    timer?.();
    timer = undefined;
    const h = handle();
    if (!dirty || h === undefined) return;
    dirty = false;
    await port.edit(h, rev, text(), selection().end, selection().start);
  }

  function reply(r: SendReply) {
    switch (r.kind) {
      case "cold": setDialog({ kind: "cold", token: r.token, prompt: r.prompt }); break;
      case "start": setDialog({ kind: "start", token: r.token, question: r.question }); break;
      case "clear_context": { const control = view()?.clear; if (control) setDialog({ kind: "clear", control, token: r.token }); break; }
      case "notice": notify("error", r.text, true); break;
      default: break;
    }
  }

  /** Runs one attach or paste; a second while one runs does nothing. */
  async function exclusive(work: (h: number) => Promise<void>) {
    const h = handle();
    if (h === undefined || locked()) return;
    setPending(true);
    try { await flush(); await work(h); } finally { setPending(false); }
  }

  return {
    view, text, selection, dialog, locked,
    sendEnabled: () => !!view()?.can_send && !tooLong() && !locked(),
    /** The status line: the composer's own error first, then Qt's order from Rust. */
    status: (): { text: string; error: boolean } => {
      const v = view();
      if (error()) return { text: error()!, error: true };
      if (tooLong() && !v?.status_error) return { text: TOO_LONG, error: true };
      return { text: v?.status ?? "", error: !!v?.status_error };
    },
    apply,
    async open(session: SessionRef | undefined) { await flush(); apply((await port.open(session)) ?? undefined); },
    input(next: string, start: number, end: number) {
      if (locked()) return;
      rev++;
      dirty = true;
      batch(() => { setText(next); setSelection({ start, end }); setError(undefined); });
      timer?.();
      timer = timers.later(() => { void flush(); }, EDIT_MS);
    },
    select(start: number, end: number) { setSelection({ start, end }); },
    flush,
    attachFiles: () => exclusive((h) => port.pick(h, selection().start, selection().end)),
    async pasteImage(blob: Blob) {
      await exclusive(async (h) => {
        let png: Uint8Array;
        try {
          if (blob.type === "image/png") png = new Uint8Array(await blob.arrayBuffer());
          else {
            const { width, height } = await images.size(blob);
            if (width * height >= MAX_PIXELS) { setError(TOO_LARGE); return; }
            png = await images.png(blob);
          }
        } catch { setError(UNREADABLE); return; }
        await port.pasteImage(h, png, selection().start, selection().end);
      });
    },
    async remove(index: number) { const h = handle(); if (h !== undefined && !locked()) { await flush(); await port.remove(h, index); } },
    async undo() { const h = handle(); if (h !== undefined && !locked()) await port.undo(h); },
    async send() {
      const h = handle();
      if (h === undefined || locked()) return;
      await flush();
      reply(await port.send(h, rev, text(), selection().end, selection().start));
    },
    async answer(answer: Answer) {
      const d = dialog();
      setDialog(undefined);
      if (!d || d.token === undefined) return;
      reply(await port.answer(d.token, answer));
    },
    closeDialog() { const d = dialog(); setDialog(undefined); if (d?.token !== undefined) void port.answer(d.token, { kind: "cancel" }); },
    /** The Clear session dialog keeps the identity it shows; the confirm sends it back for Rust to check. */
    openClear() { const control = view()?.clear; if (control) setDialog({ kind: "clear", control }); },
    async confirmClear() {
      const d = dialog();
      setDialog(undefined);
      const h = handle();
      if (h !== undefined && d?.kind === "clear") reply(await port.clear(h, d.control.run, d.control.conversation, d.control.dismiss));
    },
    async cancelSend() { const h = handle(); if (h !== undefined) await port.cancelSend(h); },
    async allowRetry() { const h = handle(); if (h !== undefined) await port.allowRetry(h); },
    async interrupted() { const h = handle(); if (h !== undefined) await port.interrupted(h); },
  };
}

export type ComposerStore = ReturnType<typeof createComposer>;

export const composer = createComposer({
  open: bridge.composerOpen, edit: bridge.composerEdit, pick: bridge.composerPick, pasteImage: bridge.composerPasteImage,
  send: bridge.sendMessage, answer: bridge.sendAnswer, remove: bridge.composerRemove, undo: bridge.composerUndoRemove,
  allowRetry: bridge.composerAllowRetry, cancelSend: bridge.sendCancel, interrupted: bridge.sendInterrupted, clear: bridge.clearContext,
});
```

In `client/src/store/actions.ts`, add `notify` to the object `createActions` returns and to the app's exported names (additive), unless #406 already exports it.

- [ ] **Step 4: The bridge.** In `client/src/bridge.ts`, one wrapper per command. Outside Tauri each rejects with "Actions need the desktop app.", and `composerOpen` answers `null`:

```ts
const DESKTOP_ACTIONS = "Actions need the desktop app.";
const desktop = <T>(command: string, args: Record<string, unknown>): Promise<T> =>
  inTauri() ? invoke<T>(command, args) : Promise.reject(DESKTOP_ACTIONS);

/** A pasted image goes as a raw IPC body, never as JSON. */
export async function composerPasteImage(handle: number, png: Uint8Array, start: number, end: number): Promise<void> {
  if (!inTauri()) throw DESKTOP_ACTIONS;
  await invoke("composer_paste_image", png, { headers: { "x-zerus-composer": String(handle), "x-zerus-start": String(start), "x-zerus-end": String(end) } });
}
export const composerOpen = (session: SessionRef | undefined) => (inTauri() ? invoke<ComposerView | null>("composer_open", { session: session ?? null }) : Promise.resolve(null));
export const composerEdit = (handle: number, rev: number, text: string, position: number, anchor: number) => desktop<void>("composer_edit", { handle, rev, text, position, anchor });
export const composerPick = (handle: number, start: number, end: number) => desktop<void>("composer_pick", { handle, start, end });
export const composerAttach = (handle: number, paths: string[], start: number, end: number) => desktop<void>("composer_attach", { handle, paths, start, end });
export const composerRemove = (handle: number, index: number) => desktop<void>("composer_remove", { handle, index });
export const composerUndoRemove = (handle: number) => desktop<void>("composer_undo_remove", { handle });
export const composerAllowRetry = (handle: number) => desktop<void>("composer_allow_retry", { handle });
export const sendMessage = (handle: number, rev: number, text: string, position: number, anchor: number) => desktop<SendReply>("send_message", { handle, rev, text, position, anchor });
export const sendAnswer = (token: number, answer: Answer) => desktop<SendReply>("send_answer", { token, answer });
export const sendCancel = (handle: number) => desktop<void>("send_cancel", { handle });
export const sendInterrupted = (handle: number) => desktop<void>("send_interrupted", { handle });
export const clearContext = (handle: number, run: string, conversation: string, dismiss: boolean) => desktop<SendReply>("clear_context", { handle, run, conversation, dismiss });
export const localRetry = (handle: number, id: string) => desktop<SendReply>("local_retry", { handle, id });
export const localDelete = (handle: number, id: string) => desktop<void>("local_delete", { handle, id });
export const localInspect = (handle: number, id: string) => desktop<Inspect | null>("local_inspect", { handle, id });
```

`normalize` keeps `composer` and `draft_keys`, and `isEmptyBatch` treats a batch with either as not empty.

- [ ] **Step 5: Route the envelope and open the composer** in `client/src/store/session.ts`:

```ts
import { composer } from "./composer";
import { setDraftKeys } from "./catalog";

export function receive(b: Batch) {
  applyBatch(b);
  observeBatch(b);
  if (b.activity.length) activity.applyActivity(b.activity);
  if (b.composer) composer.apply(b.composer);
  if (b.draft_keys) setDraftKeys(b.draft_keys);
}

createRoot(() => {
  // The selected session's composer; a rename keeps the catalog key, so Rust moves its draft.
  createEffect(on(() => { const s = selected(); return s ? `${s.computer}\n${s.catalogKey}\n${s.name}\n${s.archiveId ?? ""}` : undefined; }, () => {
    const s = selected();
    void composer.open(s ? { computer: s.computer, key: s.catalogKey, name: s.name, archive_id: s.state === "archived" ? s.archiveId ?? null : null } : undefined);
  }));
});
```

`setDraftKeys(keys)` in `store/catalog.ts` (additive) sets each row's `draft` to whether `computer + "\n" + name` is in the set. The cards already show `draft`.

- [ ] **Step 6: Write `client/src/ui/sessions/Attachments.tsx`**, the mockup's `AttachmentsChip` with the store:
  - the rows read `composer.view().attachments`;
  - a thumbnail or preview of an image uses `convertFileSrc(a.path, "zerus-file")`, and a text file's preview shows `a.excerpt`;
  - a row does not open its file yet: Task 9 adds Open (`attachment_open_draft`), disabled until R2;
  - remove calls `composer.remove(index)`, and Undo calls `composer.undo()` while `view().removed` names the file;
  - the head reads "{n} of 8, up to 10 MiB each";
  - the remove button's tooltip is "Remove" (the mockup's "(Delete)" goes), while Delete and Backspace still remove;
  - the chip reads "{n} attached", its title lists "{reference} {name}" per line, and it is hidden without attachments.

```tsx
import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Icon } from "../lib/icons";
import { composer } from "../../store/composer";
import type { AttachmentView } from "../../types";

const kib = (bytes: number) => `${Math.max(1, Math.ceil(bytes / 1024))} KiB`;
const src = (a: AttachmentView) => convertFileSrc(a.path, "zerus-file");
const image = (a: AttachmentView) => a.mime.startsWith("image/");

export function AttachmentsChip() {
  const items = () => composer.view()?.attachments ?? [];
  const [open, setOpen] = createSignal(false);
  const [preview, setPreview] = createSignal<number>();
  const [pos, setPos] = createSignal<{ left: number; bottom: number; side: "right" | "left" | "above" }>({ left: 0, bottom: 0, side: "right" });
  let chip!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;
  let arrowed = false;
  const place = () => {
    const r = chip.getBoundingClientRect();
    const left = Math.max(8, Math.min(r.left, innerWidth - 368));
    setPos({ left, bottom: innerHeight - r.top + 6, side: innerWidth - (left + 360) >= 344 ? "right" : left >= 344 ? "left" : "above" });
  };
  const close = (refocus = false) => { setOpen(false); setPreview(undefined); if (refocus) chip.focus(); };
  const rows = () => [...(list?.querySelectorAll<HTMLElement>(".att-row") ?? [])];
  const remove = (index: number) => {
    setPreview(undefined);
    void composer.remove(index);
    queueMicrotask(() => { const r = rows(); (r[Math.min(index, r.length - 1)] ?? list?.querySelector<HTMLElement>(".att-undo"))?.focus(); });
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(true); return; }
    const r = rows(), i = r.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); arrowed = true; r[(i + (e.key === "ArrowDown" ? 1 : -1) + r.length) % r.length]?.focus(); }
  };
  const outside = (e: PointerEvent) => { if (open() && !list?.contains(e.target as Node) && !chip.contains(e.target as Node)) close(); };
  onMount(() => addEventListener("pointerdown", outside));
  onCleanup(() => removeEventListener("pointerdown", outside));
  const current = () => { const i = preview(); return i === undefined ? undefined : items()[i]; };
  return (
    <span class="att-anchor">
      <button ref={chip} class="tchip" classList={{ on: open() }} hidden={!items().length && !open()} aria-haspopup="dialog" aria-expanded={open()}
        title={items().map((a) => `${a.reference} ${a.name}`).join("\n")}
        onClick={(e) => { setPreview(undefined); arrowed = false; place(); setOpen(!open()); if (open()) queueMicrotask(() => (e.detail === 0 ? rows()[0] : list)?.focus({ preventScroll: true })); }}>
        <Icon name="paperclip" size={13} />{items().length} attached
      </button>
      <Show when={open()}>
        <div class={`att-pop pv-${pos().side}`} ref={list} tabindex="-1" role="dialog" aria-label="Attachments" onKeyDown={onKey} style={{ left: `${pos().left}px`, bottom: `${pos().bottom}px` }}>
          <div class="att-head"><b>Attachments</b><span class="dim">{items().length} of 8, up to 10 MiB each</span></div>
          <For each={items()}>{(a, i) => (
            <div class="att-row" tabindex="0" aria-label={a.name}
              onMouseEnter={() => setPreview(i())} onMouseLeave={() => setPreview(undefined)} onFocus={() => { if (arrowed) setPreview(i()); }}
              onKeyDown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(i()); }
              }}>
              <span class="att-thumb" classList={{ file: !image(a) }}>
                <Show when={image(a)} fallback={<Icon name="file-text" size={16} />}><img src={src(a)} alt="" /></Show>
              </span>
              <span class="att-text"><span class="ell"><span class="att-ref">{a.reference}</span> {a.name}</span><span class="dim tabular">{kib(a.bytes)}</span></span>
              <button class="icon-btn small att-remove" aria-label={`Remove attachment ${a.name}`} data-tip="Remove" disabled={composer.locked()}
                onClick={(e) => { e.stopPropagation(); remove(i()); }}><Icon name="x" size={13} /></button>
            </div>
          )}</For>
          <Show when={composer.view()?.removed}>{(name) => (
            <div class="att-removed"><span class="ell">Removed {name()}</span><span class="grow" />
              <button class="btn small ghost att-undo" onClick={() => { void composer.undo(); }}>Undo</button></div>
          )}</Show>
          <Show when={!items().length && !composer.view()?.removed}><div class="dim att-empty">No attachments.</div></Show>
          <Show when={current()}>{(a) => (
            <div class="att-preview" aria-hidden="true">
              <Show when={image(a())} fallback={<pre class="mono">{a().excerpt ?? ""}</pre>}><img src={src(a())} alt="" /></Show>
              <div class="att-preview-cap"><span class="ell">{a().name}</span><span class="dim tabular">{kib(a().bytes)}</span></div>
            </div>
          )}</Show>
        </div>
      </Show>
    </span>
  );
}
```

- [ ] **Step 7: Write `client/src/ui/sessions/Composer.tsx`.** It follows the mockup's structure: the edge grip, the 34 px toolbar, the editor (76-720 px, kept under the local storage key `zerus-client-editor-height`) and the action row. The toolbar holds, in Qt's slot order:
  1. the `AttachmentsChip`;
  2. an empty voice slot, which Task 13 fills;
  3. Mark as read: when `markReadChip(session).active`, an accent chip "Mark as read" with `markReadChip(session).detail` as its tooltip, running #406's `runMarkRead(snapshotOf(session))` (the menu entry's own action, with its icon `check-check`);
  4. Latest: the icon chip `chevrons-down` with the tooltip "Jump to latest activity" or "Jump to latest activity ({n} new)", shown while Activity is not following the latest item;
  5. the grip;
  6. the compaction indicator: "Compacting context…" with the tooltip "The agent is compressing conversation context.", shown when the followed state's `phase` is `compacting`, `runtime_state` is `live`, `process_state` is `running` and `tracked` is true;
  7. Cancel send, with the tooltip "Cancel the pending send and unlock the message for editing. Compaction or startup continues; use Stop to interrupt the agent.", shown while `view.cancel_send`;
  8. the cache chip (`view.cache`: `label`, or `short_label` when narrow, toned by `tone`), whose popover shows `cache_warning`, `cache_details`, "Usage details" (disabled with "Arrives with the side panel.") and "Clear context" (`view.clear.enabled`, tooltip "Clear this agent’s conversation, like /clear in Terminal. Your unsent message stays here.", opening the Clear session dialog);
  9. the Context chip (R1, hidden until it merges).

  The action row holds:
  - Attach (`paperclip`): `composer.attachFiles()`, enabled when not locked. Its tooltip is "Attach files, paste an image, or drop files onto this session", or the attachments reason when the session takes none.
  - Reference file (`file-text`), disabled with "Arrives with file references.".
  - The status line (`composer.status()`, in the error colour when `error`).
  - Allow retry, shown while `view.uncertain` and not locked, with the tooltip "Check the terminal before sending again to avoid a duplicate message".
  - The model button, disabled with "Arrives with model settings.".
  - Stop: phase 2's `actionEntries(s, details).stop`, shown while `stop.shown`; its click calls `composer.interrupted()` before the interrupt (`SessionsWindow.cpp:1187-1191`).
  - Send: label `view.send_label`, enabled by `composer.sendEnabled()`.

  The editor:
  - Enter sends, Shift+Enter inserts a newline, and IME composition (`e.isComposing`) never sends.
  - Escape runs Stop when it is enabled (`edit->escape`).
  - It is `readOnly` while `composer.locked()`; its placeholder is the unavailable reason, else "Message this agent…".
  - Paste checks the clipboard's files for images first (`composer.pasteImage(file)`) and lets text paste normally.
  - Blur flushes the edit.

```tsx
import { For, Show, createMemo, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { composer } from "../../store/composer";
import { activity } from "../../store/session";
import { selected, contentScale } from "../../store/ui";
import { computerOffline } from "../../store/catalog";
import { detailsFor, followedFor } from "../../store/actions";
import { actionEntries } from "./ActionDialogs";
import { snapshotOf } from "./actions-ui";
import { markReadChip, runMarkRead } from "./mark-entries";
import { AttachmentsChip } from "./Attachments";
import { ColdDialog, StartDialog, ClearDialog } from "./ComposerDialogs";
import { following, unseen, jumpToLatest } from "./Activity";

const EDITOR = { initial: 76, min: 76, max: 720 };
const KEY = "zerus-client-editor-height";
const saved = (() => { try { return Number(localStorage.getItem(KEY)) || 0; } catch { return 0; } })();
const [editorHeight, setEditorHeightRaw] = createSignal(saved >= EDITOR.min ? Math.min(EDITOR.max, saved) : EDITOR.initial);
const setEditorHeight = (h: number) => {
  const v = Math.round(Math.max(EDITOR.min, Math.min(EDITOR.max, h)));
  setEditorHeightRaw(v);
  try { localStorage.setItem(KEY, String(v)); } catch { /* storage may be unavailable */ }
};

export function Composer() {
  const s = () => selected()!;
  const v = () => composer.view();
  const details = createMemo(() => detailsFor(s(), !computerOffline(s().computer), followedFor(s().key)));
  const act = createMemo(() => actionEntries(s(), details()));
  const summary = () => activity.summary() ?? {};
  const compacting = () => summary().phase === "compacting" && summary().runtime_state === "live" && summary().process_state === "running" && summary().tracked === true;
  const mark = createMemo(() => markReadChip(s()));
  const [cacheOpen, setCacheOpen] = createSignal(false);
  let editor!: HTMLTextAreaElement;
  const caret = () => ({ start: editor.selectionStart, end: editor.selectionEnd });
  const onPaste = (e: ClipboardEvent) => {
    const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith("image/"));
    if (!file) return;
    e.preventDefault();
    void composer.pasteImage(file);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing) return;
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void composer.send(); }
    else if (e.key === "Escape" && act().stop.shown && !act().stop.disabled) { e.preventDefault(); void composer.interrupted().then(() => act().stop.action()); }
  };
  return (
    <div class="composer" classList={{ disabled: !v()?.available }}>
      <ResizeHandle edge height={editorHeight} setHeight={setEditorHeight} />
      <div class="cmp-toolbar">
        <AttachmentsChip />
        <span class="rec-slot-host" />
        <Show when={mark().active}>
          <button class="tchip accent" title={mark().detail} onClick={() => { runMarkRead(snapshotOf(s())); }}>
            <Icon name="check-check" size={13} />Mark as read
          </button>
        </Show>
        <Show when={!following()}>
          <button class="tchip icon" data-tip={unseen() > 0 ? `Jump to latest activity (${unseen()} new)` : "Jump to latest activity"}
            aria-label={unseen() > 0 ? `Jump to latest activity (${unseen()} new)` : "Jump to latest activity"} onClick={jumpToLatest}><Icon name="chevrons-down" size={13} /></button>
        </Show>
        <ResizeHandle height={editorHeight} setHeight={setEditorHeight} />
        <Show when={compacting()}><span class="tchip" title="The agent is compressing conversation context."><span class="spinner small" />Compacting context…</span></Show>
        <Show when={v()?.cancel_send}>
          <button class="tchip" title="Cancel the pending send and unlock the message for editing. Compaction or startup continues; use Stop to interrupt the agent."
            onClick={() => { void composer.cancelSend(); editor.focus(); }}>Cancel send</button>
        </Show>
        <Show when={v()?.cache.visible}>
          <span class="att-anchor">
            <button class={`tchip ${v()!.cache.tone === "danger" ? "danger" : v()!.cache.tone === "warning" ? "warn" : v()!.cache.tone === "success" ? "ok" : ""}`}
              title={[v()!.cache_warning, v()!.cache_details].filter(Boolean).join("\n\n")} onClick={() => setCacheOpen(!cacheOpen())}>
              <Icon name="timer" size={13} />{v()!.cache.label}
            </button>
            <Show when={cacheOpen()}>
              <div class="att-pop cache-pop" role="dialog" aria-label="Prompt cache">
                <Show when={v()!.cache_warning}><p class="cache-warning">{v()!.cache_warning}</p></Show>
                <p class="cache-detail">{v()!.cache_details}</p>
                <div class="row"><span class="grow" />
                  <button class="btn small" disabled title="Arrives with the side panel.">Usage details</button>
                  <button class="btn small" disabled={!v()!.clear.enabled} title="Clear this agent’s conversation, like /clear in Terminal. Your unsent message stays here."
                    onClick={() => { setCacheOpen(false); composer.openClear(); }}>Clear context</button>
                </div>
              </div>
            </Show>
          </span>
        </Show>
      </div>
      <textarea ref={editor} class="cmp-editor" style={{ height: `${editorHeight() * contentScale()}px` }}
        placeholder={v()?.available === false ? v()!.unavailable_reason : "Message this agent…"} readOnly={composer.locked()} value={composer.text()}
        onInput={(e) => composer.input(e.currentTarget.value, e.currentTarget.selectionStart, e.currentTarget.selectionEnd)}
        onSelect={() => { const c = caret(); composer.select(c.start, c.end); }}
        onBlur={() => { void composer.flush(); }} onKeyDown={onKey} onPaste={onPaste} />
      <div class="cmp-actions">
        <span class="cmp-files">
          <button class="icon-btn cmp-attach" disabled={composer.locked()} aria-label="Attach files to message"
            title={v()?.attachments_available === false ? "This native adapter does not support file attachments." : "Attach files, paste an image, or drop files onto this session"}
            onClick={() => { void composer.attachFiles(); }}><Icon name="paperclip" size={17} /></button>
          <button class="icon-btn cmp-attach" disabled title="Arrives with file references."><Icon name="file-text" size={16} /></button>
        </span>
        <Show when={composer.status().text}><span class="cmp-status ell" classList={{ error: composer.status().error }} title={composer.status().text}>{composer.status().text}</span></Show>
        <span class="grow" />
        <Show when={v()?.uncertain && !composer.locked()}>
          <button class="btn ghost" title="Check the terminal before sending again to avoid a duplicate message" onClick={() => { void composer.allowRetry(); editor.focus(); }}>Allow retry</button>
        </Show>
        <button class="btn ghost cmp-model" disabled title="Arrives with model settings.">{s().model} <span class="dim">{s().effort}</span><Icon name="chevron-down" size={12} /></button>
        <Show when={act().stop.shown}>
          <button class="btn" title={act().stop.hint ?? "Interrupt the current turn, like Escape in Terminal. The session stays open; queued messages follow the agent's native behavior."}
            disabled={act().stop.disabled} onClick={() => { void composer.interrupted().then(() => act().stop.action()); }}><Icon name="square" size={12} />Stop</button>
        </Show>
        <button class="btn primary" disabled={!composer.sendEnabled()} onClick={() => { void composer.send(); }}>{v()?.send_label ?? "Send"}<Icon name="arrow-up" size={13} /></button>
      </div>
      <ColdDialog /><StartDialog /><ClearDialog />
    </div>
  );
}
```

`ResizeHandle` is the mockup's (`design/webui/src/screens/sessions/Composer.tsx:31-68`), taking `height` and `setHeight` props instead of module state. In `Activity.tsx`, the scroll handler sets the module signals:
- `following`: true within `STICK_EDGE` of the bottom;
- `unseen`: the items added since following stopped, reset when following resumes;
- `jumpToLatest()`: scrolls the scroller to the bottom.

Add `.cmp-status.error { color: var(--z-color-danger); }`, `.tchip.danger`, `.tchip.warn` and the `.cache-pop` padding to `detail.css` if the port lacks them.

- [ ] **Step 8: Write `client/src/ui/sessions/ComposerDialogs.tsx`**: three modal dialogs over the existing dialog styles (`ActionDialogs.tsx`). Each focuses Cancel first, makes it the default, and Escape closes as Cancel (`setDefaultButton(cancel)`, `setEscapeButton(cancel)`):
  - **ColdDialog** (`dialog().kind === "cold"`): the title, `prompt.text` (plain text, `white-space: pre-wrap`), `prompt.informative` as plain text, one button per `prompt.choices` (disabled when `!enabled`, with `tooltip` as its title), then Cancel. A choice calls `composer.answer({kind: "cold", action})`. Clear turns into the Clear dialog through the `clear_context` reply.
  - **StartDialog** (`kind === "start"`): `question.title`, `question.text`, `question.informative`, one button per `question.options` in order ("Resume and send", "Resume, compact and send", "Start fresh and send"), then Cancel. A choice calls `composer.answer({kind: "start", choice})`.
  - **ClearDialog** (`kind === "clear"`): the dialog's own `control` (the snapshot taken when it opened): its `title`, `text` and a danger button `confirm`, which calls `composer.confirmClear()`; then Cancel. Rust refuses the confirm quietly when the session's run, conversation or dismiss variant changed meanwhile, as Qt does.

- [ ] **Step 9: Place it** in `client/src/ui/sessions/SessionDetail.tsx`. The Activity tab body becomes:

```tsx
<div class="activity-wrap" style={columnStyle()}>
  <ActivityView />
  <Show when={!subagentOpen()}><div class="cmp-col"><Composer /></div></Show>
</div>
```

The header's "Clear session…" menu entry and its button use `composer.view()?.clear`: they are enabled by `clear.enabled` (which Qt also gates on online) and run `composer.openClear()`.

- [ ] **Step 10: Run the checks**

Run: `cd client && npm test -- src/store/composer.test.ts && npm run check && npm test && python3 ../scripts/check-ui-separators.py`
Expected: PASS, 6 composer tests; no type errors; no separators.

- [ ] **Step 11: Commit**

```bash
git add client/src
git commit -m "feat(client): the composer with its toolbar, attachments list and send questions"
```

---

### Task 7: Local message rows in Activity, and Saved drafts

**Depends on:** Tasks 4b and 6; S5's `drafts::{IndexPage, Page, Cursor, Summary, owner, qt_root, read_draft, read_index, ALREADY_EXISTS}` and `DraftStore::{index, load, recover}`.

**Qt's store is read, never written** (owner decision, 2026-10-28: the client takes over Qt's identity on the Mac, and Qt's drafts are kept as they are). Every path that touches Qt's drafts root goes through `Config.qt_drafts` and only these reads: `drafts::read_index` (Saved drafts' Qt section), `drafts::read_draft` (its preview) and `DraftStore::recover`, which reads Qt's copy and writes only the client's own store. Nothing writes, renames, locks or deletes in Qt's folder, and `qt_store` leaves Qt's folder out entirely if it is, holds or sits inside the client's own root. A test proves the folder byte-for-byte unchanged after every Qt path runs.

**Qt reference:**
- `tray/src/ActivityView.cpp:665-720` (local rows merged into the timeline) and `:1053-1068` (the stamp, the error line, Retry, Check Terminal, Check native UI and Delete).
- `tray/src/SessionsWindow.cpp`: `messageAction` (4470-4527) and the Saved drafts button (485-489).
- `tray/src/MessageComposer.cpp:1230-1387`: `showSavedDrafts`, with its titles, details, buttons, the "Replace current draft?" confirmation and the loading text.
- The restore notice (`SessionsWindow.cpp:3331`).

**Files:**
- Modify: `client/src-tauri/src/composer/mod.rs` (`Config.qt_drafts`, `qt_store`, saved drafts, recover, restore, their commands and tests)
- Modify: `client/src-tauri/src/lib.rs` (`qt_drafts` in the config, the four commands)
- Modify: `client/src/types.ts`, `client/src/bridge.ts`
- Modify: `client/src/ui/sessions/transcript.ts`, `client/src/ui/sessions/transcript.test.ts` (local rows)
- Modify: `client/src/ui/sessions/Activity.tsx` (the local card and its buttons)
- Create: `client/src/ui/sessions/SavedDrafts.tsx`
- Modify: `client/src/ui/sessions/SessionList.tsx:442` (enable the Saved drafts button)
- Modify: the menu-parity restore follow-up's notice, adding Qt's second sentence (in `store/actions.ts` or wherever Piece E put it; search for "could not be verified")

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum DraftSource { Client, Qt }
#[derive(Serialize)] pub struct SavedDraftItem { pub key: String, pub kind: OwnerKind, pub computer: String, pub session: String, pub title: String, pub caption: String, pub machine: String, pub meta: String, pub detail: String, pub modified: f64, pub attachments: usize }
#[derive(Serialize)] pub struct SavedDraftsPage { pub items: Vec<SavedDraftItem>, pub next: Option<zerus_client_core::drafts::Cursor> }
#[derive(Serialize)] pub struct SavedDraftDetail { pub text: String, pub attachments: usize, pub detail: String, pub restorable: bool }
// composer/mod.rs
pub struct Config { pub cache: PathBuf, pub qt_drafts: Option<PathBuf> } // Task 3b's, with Qt's read-only root
pub fn qt_store(own: &Path, qt: Option<PathBuf>) -> Option<PathBuf>;
impl Composers {
    pub fn saved_drafts(&self, source: DraftSource, limit: usize, before: Option<Cursor>) -> SavedDraftsPage; // blocking
    pub fn saved_draft(&self, source: DraftSource, key: &str) -> SavedDraftDetail;                           // blocking
    pub fn recover(&self, key: &str) -> Result<(), String>;                                                   // blocking
    pub fn restore(&self, handle: u64, source: DraftSource, key: &str) -> Result<(), String>;                 // blocking
}
// commands: saved_drafts(source, limit, before) -> SavedDraftsPage; saved_draft(source, key) -> SavedDraftDetail;
// draft_recover(key) -> Result<(), String>; draft_restore(handle, source, key) -> Result<(), String>
```

```ts
// transcript.ts: buildTranscript(events, summary, local: LocalMessage[] = []) adds local rows as user items with
// local: { id: string; stamp: string; error: string; uncertain: boolean; actions: ("retry" | "inspect" | "delete")[]; inspectLabel: string }
```

- [ ] **Step 1: Failing tests.** Rust, in `composer/mod.rs`:

```rust
    #[test]
    fn saved_drafts_page_and_name_their_owner() {
        let root = dir("saved");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "first line\nsecond".into(), 0, 0);
        // The index runs on the store's writer after the queued save.
        let page = c.saved_drafts(DraftSource::Client, 10, None);
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.title, "claude/sample-project/review / studio");
        assert_eq!(item.caption, "review");
        assert_eq!(item.detail, "first line");
        let detail = c.saved_draft(DraftSource::Client, KEY);
        assert_eq!((detail.text.as_str(), detail.detail.as_str()), ("first line\nsecond", "0 attachments"));
    }

    /// A Qt store with one draft, written by a store of its own that is gone
    /// before the client's hub opens.
    fn qt_store_with_a_draft(root: &Path) -> PathBuf {
        let qt = root.join("qt");
        let writer = DraftStore::open(qt.clone());
        let mut stored = StoredDraft { sending: true, ..StoredDraft::default() };
        stored.draft.text = "from the Qt app".into();
        writer.save_durable(KEY, stored, |_| {});
        writer.flush();
        drop(writer);
        qt
    }

    #[test]
    fn a_qt_draft_is_recovered_never_over_a_draft_here() {
        let root = dir("recover");
        let qt = qt_store_with_a_draft(&root);
        let (c, _) = hub_with_qt(&drafts(&root), Some(qt));
        assert_eq!(c.saved_drafts(DraftSource::Qt, 10, None).items.len(), 1);
        assert_eq!(c.recover(KEY), Ok(()));
        assert_eq!(c.saved_draft(DraftSource::Client, KEY).text, "from the Qt app");
        assert_eq!(c.recover(KEY), Err("Another draft already exists here. Recover the previous one from Saved drafts.".into()));
    }

    /// Every file under `root`, with its bytes and modification time.
    fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files = Vec::new();
        let mut folders = vec![root.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
                files.push((path.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(&path).unwrap(), modified));
            }
        }
        files.sort();
        files
    }

    #[test]
    fn the_qt_store_is_never_written() {
        let root = dir("qt-untouched");
        let qt = qt_store_with_a_draft(&root);
        let before = snapshot(&qt);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt.clone()));
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(c.saved_drafts(DraftSource::Qt, 10, None).items.len(), 1);
        assert!(c.saved_draft(DraftSource::Qt, KEY).text.contains("from the Qt app"));
        assert_eq!(c.recover(KEY), Ok(()));
        assert!(c.recover(KEY).is_err());
        assert_eq!(c.restore(view.handle, DraftSource::Qt, KEY), Ok(()));
        c.flush();
        store.flush();
        assert_eq!(snapshot(&qt), before, "Qt's folder is byte-for-byte unchanged, with no file added or removed");
    }

    #[test]
    fn the_qt_store_is_never_the_clients_own() {
        let root = dir("qt-same");
        let own = root.join("drafts");
        std::fs::create_dir_all(&own).unwrap();
        assert_eq!(qt_store(&own, Some(own.clone())), None);
        assert_eq!(qt_store(&own, Some(root.clone())), None, "nor a folder that holds it");
        assert_eq!(qt_store(&own, Some(own.join("inner"))), None, "nor a folder inside it");
        assert_eq!(qt_store(&own, Some(root.join("qt"))), Some(root.join("qt")));
        assert_eq!(qt_store(&own, None), None);
    }
```

Task 3a's test helper becomes `hub_with_qt(store, qt: Option<PathBuf>)`, which passes `Config { cache: store.root().with_file_name("cache"), qt_drafts: qt }`; `hub_on(store)` calls it with `None`, and Task 4a's `send_hub_on` passes `qt_drafts: None`.

`qt_store` in `mod.rs`:

```rust
/// Qt's draft store, read-only, unless it is, holds or sits inside the
/// client's own root: then it is left out, so nothing the client writes can
/// land in Qt's folder.
pub fn qt_store(own: &Path, qt: Option<PathBuf>) -> Option<PathBuf> {
    let qt = qt?;
    let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (own_real, qt_real) = (real(own), real(&qt));
    (!own_real.starts_with(&qt_real) && !qt_real.starts_with(&own_real)).then_some(qt)
}
```

In `lib.rs`, the config gains `qt_drafts: composer::qt_store(drafts.root(), (!replay).then(zerus_client_core::drafts::qt_root).flatten())`: a replay never lists Qt's drafts.

TypeScript, in `transcript.test.ts`:

```ts
it("local rows follow Qt's stamps and buttons", () => {
  const local = (status: "sending" | "error" | "sent", uncertain = false): LocalMessage => ({ id: `l-${status}-${uncertain}`, text: "hello", status, attachments: [], run_id: "r", conversation_id: "c",
    submitted_at: 1_793_000_000, error: status === "error" ? "hgs: refused" : "", uncertain, message_id: null, submitted_text: null, source: null, voice: null });
  const items = buildTranscript([], {}, [local("sending"), local("error"), local("error", true), local("sent")]) as ActivityItem[];
  expect(items.map((i) => i.local?.stamp)).toEqual(["Sending…", "Not sent", "Delivery not confirmed", "Submitted to terminal"]);
  expect(items[1].local?.actions).toEqual(["retry", "delete"]);
  expect(items[2].local?.actions).toEqual(["inspect", "delete"]);
  expect(items[2].local?.inspectLabel).toBe("Check Terminal");
  expect(buildTranscript([], { native_ui_available: true }, [local("error", true)])[0]).toMatchObject({ local: { inspectLabel: "Check native UI" } });
});
```

Run both; they fail.

- [ ] **Step 2: Implement the Rust side** in `composer/mod.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftSource { Client, Qt }

/// One Saved drafts row, as `showSavedDrafts` titles it (1257-1300; " / " joins, never dots).
fn item(key: String, summary: &Summary) -> SavedDraftItem {
    let owner = drafts::owner(&key);
    let machine = if owner.machine.is_empty() { "this computer".to_owned() } else { owner.machine.clone() };
    let mut title = Vec::new();
    let mut context = Vec::new();
    match owner.kind {
        OwnerKind::TaskDescription | OwnerKind::TaskComment => {
            title.push(if summary.label.is_empty() { owner.task.clone() } else { summary.label.clone() });
            title.push(owner.project.clone());
        }
        OwnerKind::Unknown => title.push(if summary.label.is_empty() { key.replace('\n', " / ") } else { summary.label.clone() }),
        kind => {
            title.push(owner.session.clone());
            title.push(machine.clone());
            let (word, tag) = match kind {
                OwnerKind::Subagent => ("subagent", "Subagent"),
                OwnerKind::Question => ("question answer", "Question answer"),
                OwnerKind::Voice => ("voice message", "Voice message"),
                OwnerKind::Task => ("task prompt", "Task prompt"),
                _ => ("", ""),
            };
            if !word.is_empty() { title.push(word.into()); context.push(tag.to_owned()); }
            if !owner.archive.is_empty() { title.push("archived".into()); context.push("Archived".into()); }
        }
    }
    let attachments = summary.attachments;
    let count = if attachments == 1 { "1 attachment".to_owned() } else { format!("{attachments} attachments") };
    if attachments > 0 && !summary.preview.is_empty() { context.push(count.clone()); }
    let caption = owner.session.split('/').nth(2).or_else(|| owner.session.split('/').nth(1)).filter(|c| !c.is_empty())
        .map(str::to_owned).unwrap_or_else(|| title[0].clone());
    let mut meta = vec![owner.session.clone()];
    meta.extend(context);
    SavedDraftItem {
        key, kind: owner.kind, computer: owner.machine.clone(), session: owner.session.clone(), title: title.join(" / "), caption, machine,
        meta: meta.into_iter().filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" / "),
        detail: if summary.preview.is_empty() && attachments > 0 { count } else { summary.preview.clone() },
        modified: summary.modified, attachments,
    }
}
```

Go to session is the webview's: an item is reachable when its `kind` is a message, subagent, question or voice draft and the catalog lists a row with its `computer` and `session` (Qt's `canOpenDraftSession`).

`Composers::saved_drafts` reads the client store through `self.0.store.index(...)` (waiting on a channel, off the UI thread through the command's `spawn_blocking`), or Qt's through `drafts::read_index(qt_root, &page)`. `limit` is clamped to 1-200. `saved_draft` uses `DraftStore::load` or `drafts::read_draft` and Qt's detail texts:
- "{n} attachment(s)";
- a draft saved as `sending` shows "This message may already have been sent. Check Activity or Terminal before sending again.";
- a voice draft shows "Open the original conversation ({session}) to review or check this voice message." and is not restorable.

`recover(key)` is `DraftStore::recover(qt_root, key, ...)`, waiting on a channel for its answer; without a Qt store it answers "No saved drafts from the Qt app.". `restore(handle, source, key)` replaces the open composer's draft:
- with the selected draft, its label and form state cleared;
- only while `editable` gives the composer (loaded, not busy, the app not quitting);
- a Qt source is read with `drafts::read_draft` and also recovered into the client's store; an existing draft there stays, and its `ALREADY_EXISTS` does not stop the restore, since the "Replace current draft?" confirmation already covered the open draft;
- it saves, bumps `text_epoch` and calls `attachments_changed`.

Register the four commands.

- [ ] **Step 3: Local rows in the transcript.** `buildTranscript(events, summary, local)` appends one item per local row, ordered by `submitted_at`, after the events of the same second. A sent row whose text an event already shows is not added (S4's reconcile removes it on the next state). Each item is:
  - `{kind: "user", id: "local-" + id, at: timeText(submitted_at), body: text, attachments}`;
  - with `local.stamp` "Sending…", "Not sent", "Delivery not confirmed" (uncertain) or "Submitted to terminal";
  - `local.error` set for a failed row;
  - `local.actions` `["retry", "delete"]` for a definite failure and `["inspect", "delete"]` for an uncertain one;
  - `local.inspectLabel` "Check native UI" when `summary.native_ui_available`, else "Check Terminal".

  `Activity.tsx` reads `composer.view()?.local` into `buildTranscript`, and renders, under a local item's body:
  - the stamp in the header (instead of the receipt);
  - the error line (`act-error`);
  - the buttons, which call `bridge.localRetry(handle, id)` (its reply goes through `composer`'s reply handler, so a cold retry asks first), `bridge.localDelete(handle, id)`, and `bridge.localInspect(handle, id)`.

  Inspect routes:
  - `terminal`: `setDetailTab("terminal")`;
  - `native_ui`: `setDetailTab("native")` (Task 14; until then a notice "Arrives with Native UI.");
  - `refresh`: `notify("info", notice)`.

- [ ] **Step 4: Saved drafts.** Write `client/src/ui/sessions/SavedDrafts.tsx`, a dialog titled "Saved drafts".
  - The hint reads "Drafts stay on this desktop, including those from ended or unavailable sessions. Restoring never sends a message.", and "No saved drafts on this desktop." when both sections are empty.
  - Two sections, "On this computer" and "From the previous Zerus app" (the second only when Qt's store has drafts), each with its rows (title, meta, detail, modified time as "d MMM HH:mm") and "Show more" while `next` is set. Each page is one bounded read of at most 200 drafts; Show more appends the next page on request only, and the dialog drops its rows when it closes, so nothing loads the whole store or grows by itself.
  - A preview pane reads "Loading draft…" while `saved_draft` runs.
  - The buttons are Copy text, Restore in current session, Go to session (disabled with "This session is not in the session list now." when not reachable), Recover (Qt section only) and Close.
  - Restore over a nonempty current draft asks "Replace current draft?" with "Replace the unsent draft in {key with ' / '} with the selected saved draft?", the button "Replace draft", and Cancel as the default.
  - Go to session selects the row and closes.
  - Enable the session list's Saved drafts button (`SessionList.tsx:442`) to open it.

  Where the menu-parity Piece E shows "The restored conversation could not be verified.", append " Your archived draft remains in Saved drafts." (the owner ruling in `zerus-5wmh.9`).

- [ ] **Step 5: Run the checks**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/src-tauri/src/composer client/src-tauri/src/lib.rs client/src
git commit -m "feat(client): local message rows with Retry, Delete and Inspect, and Saved drafts"
```

---

### Task 8: Dropped files: composer attach, Terminal paste and stage

**Depends on:** A2 `zerus-5wmh.9.4` (#404): `Action::StageFiles {paths, expected_created}` and its `staged` receipt; `Session.created` from #406; Tasks 2, 3a and 6.

**Qt reference:**
- `tray/src/SessionsWindow.cpp:1595-1601`: `SessionFileDrop`'s targets. The composer takes drops on the Activity tab; the Terminal takes them while connected.
- `tray/src/SessionsWindow.cpp:1602-1620`: the stage's finish, its checks and texts.
- `tray/src/SessionsWindow.cpp:2618-2659`: `terminalDropTarget`, `pasteTerminalPaths`, `dropTerminalFiles`.
- `tray/src/MessageComposer.cpp:961-967`: `attachDroppedFiles` collapses the selection.

**Files:**
- Modify: `client/src-tauri/src/terminal.rs` (`terminal_drop`, `staged_paths`, `paste`, `generation`)
- Modify: `client/src-tauri/src/lib.rs` (register `terminal_drop`)
- Create: `client/src/store/drops.ts`, `client/src/store/drops.test.ts`
- Modify: `client/src/store/terminal.ts` (`generation()`), `client/src/bridge.ts`, `client/src/main.tsx` (start the drop listener)

**Interfaces:**
- Consumes: `terminal::paste_paths(paths, windows)`; `crate::actions::{Actions, dispatch}`; `Action::StageFiles`; Task 3a's `composer_attach`.
- Produces:

```rust
/// The staged paths of a `staged` receipt: absolute, without control characters, exactly `count`.
pub fn staged_paths(receipt: &Value, count: usize) -> Result<Vec<String>, String>;
impl Terminals { pub fn generation(&self, view: &str) -> Option<u64>; pub fn paste(&self, view: &str, text: &str) -> Result<(), String>; }
#[derive(Deserialize)] pub struct DropTarget { pub computer: String, pub name: String, pub machine: String, pub run_id: String,
    pub conversation_id: String, pub tracked: bool, pub created: i64 }
// command: terminal_drop(view, generation, target, paths) -> Result<Option<String /*info notice*/>, String /*sticky notice*/>
```

```ts
// store/drops.ts
export function routeDrop(paths: string[], tab: DetailTab, terminalConnected: boolean): "composer" | "terminal" | "none";
export function startDrops(): () => void;
```

- [ ] **Step 1: Failing tests.** In `terminal.rs` tests:

```rust
    #[test]
    fn staged_receipts_are_checked_as_qt_does() {
        let receipt = json!({"files": [{"path": "/home/example/.hgs/drop/a.txt"}, {"path": "/home/example/.hgs/drop/b c.txt"}]});
        assert_eq!(staged_paths(&receipt, 2).unwrap(), ["/home/example/.hgs/drop/a.txt", "/home/example/.hgs/drop/b c.txt"]);
        assert_eq!(staged_paths(&receipt, 3), Err("Incomplete file transfer".into()));
        assert_eq!(staged_paths(&json!({"files": [{"path": "relative.txt"}]}), 1), Err("Invalid transferred file path".into()));
        assert_eq!(staged_paths(&json!({"files": [{"path": "/home/example/a\nb"}]}), 1), Err("Invalid transferred file path".into()));
    }

    #[test]
    fn a_paste_goes_to_the_current_attach() {
        let _tree = crate::PROCESS_TREE.read().unwrap();
        let t = terminals();
        let sink = Arc::new(Collect::default());
        let c = t.connect("main", target("run-1"), 80, 24, sink.clone()).unwrap();
        assert_eq!(t.generation("main"), Some(c.generation));
        t.write("main", c.generation, b"echo ").unwrap();
        let text = zerus_client_core::terminal::paste_paths(&["/home/example/it's.txt".to_owned()], false).unwrap();
        t.paste("main", &text).unwrap();
        t.write("main", c.generation, b"\r").unwrap();
        until(&t, c.generation, &sink, |s| s.text().contains("echo '/home/example/it'\\''s.txt'"));
        t.disconnect("main");
        assert_eq!(t.paste("main", "x"), Err("Terminal changed during file transfer. Drop the files again in the intended session.".into()));
    }
```

In `client/src/store/drops.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { routeDrop } from "./drops";

describe("file drops", () => {
  it("follow Qt's SessionFileDrop targets", () => {
    expect(routeDrop(["/home/example/a"], "activity", false)).toBe("composer");
    expect(routeDrop(["/home/example/a"], "terminal", true)).toBe("terminal");
    expect(routeDrop(["/home/example/a"], "terminal", false)).toBe("none");
    expect(routeDrop(["/home/example/a"], "processes", true)).toBe("none");
    expect(routeDrop([], "activity", false)).toBe("none");
  });
});
```

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml terminal:: && npm test -- src/store/drops.test.ts`
Expected: FAIL to compile or resolve.

- [ ] **Step 2: Implement the Rust side** in `terminal.rs`:

```rust
pub fn staged_paths(receipt: &Value, count: usize) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for file in receipt["files"].as_array().into_iter().flatten() {
        let path = file["path"].as_str().unwrap_or("");
        let absolute = path.starts_with('/') || (path.len() > 2 && path.as_bytes()[1] == b':');
        if !absolute || path.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
            return Err("Invalid transferred file path".into());
        }
        paths.push(path.to_owned());
    }
    if paths.len() != count {
        return Err("Incomplete file transfer".into());
    }
    Ok(paths)
}

#[derive(Clone, Debug, Deserialize)]
pub struct DropTarget {
    pub computer: String,
    pub name: String,
    pub machine: String,
    pub run_id: String,
    pub conversation_id: String,
    pub tracked: bool,
    pub created: i64,
}

impl Terminals {
    pub fn generation(&self, view: &str) -> Option<u64> {
        lock(&self.0.views).get(view).map(|open| open.generation)
    }

    /// Pastes text into the view's current attach (dropped paths).
    pub fn paste(&self, view: &str, text: &str) -> Result<(), String> {
        match lock(&self.0.views).get(view) {
            Some(open) => open.attach.write(text.as_bytes()),
            None => Err("Terminal changed during file transfer. Drop the files again in the intended session.".into()),
        }
    }
}

/// `dropTerminalFiles`: this computer's paths are pasted quoted; a peer's
/// files are staged there first and their staged paths pasted. `Ok` carries
/// an info notice, `Err` a sticky one.
#[tauri::command(rename_all = "snake_case")]
pub async fn terminal_drop(
    terminals: State<'_, Terminals>,
    actions: State<'_, crate::actions::Actions>,
    view: String,
    generation: u64,
    target: DropTarget,
    paths: Vec<String>,
) -> Result<Option<String>, String> {
    if terminals.generation(&view) != Some(generation) {
        return Ok(None);
    }
    if target.computer.is_empty() {
        let text = terminal::paste_paths(&paths, cfg!(windows))?;
        terminals.paste(&view, &text)?;
        return Ok(None);
    }
    let untracked = target.run_id.is_empty() && target.conversation_id.is_empty() && !target.tracked;
    if target.run_id.is_empty() && !untracked {
        return Err("Wait for the session identity to load, then drop the files again.".into());
    }
    if terminals.0.dropping.swap(true, Ordering::SeqCst) {
        return Ok(Some("Wait for the current file transfer to finish.".into()));
    }
    let staged = crate::actions::dispatch(
        actions.inner(),
        zerus_client_core::Target { computer: target.computer.clone(), name: target.name.clone(), run_id: target.run_id.clone(), conversation_id: target.conversation_id.clone() },
        zerus_client_core::Action::StageFiles { paths: paths.iter().map(PathBuf::from).collect(), expected_created: untracked.then_some(target.created) },
    )
    .await;
    terminals.0.dropping.store(false, Ordering::SeqCst);
    let receipt = match staged {
        Ok(zerus_client_core::Outcome::Done { receipt, .. }) => receipt,
        Ok(zerus_client_core::Outcome::Failed { detail } | zerus_client_core::Outcome::Uncertain { detail }) | Err(detail) => {
            if untracked && detail.contains("no saved binding") {
                return Err(format!("Update hgs on {} to drop files into an untracked session.", target.machine));
            }
            return Err(detail);
        }
    };
    if terminals.generation(&view) != Some(generation) {
        return Err("Terminal changed during file transfer. Drop the files again in the intended session.".into());
    }
    let staged = staged_paths(&receipt, paths.len())?;
    terminals.paste(&view, &terminal::paste_paths(&staged, false)?)?;
    Ok(None)
}
```

`Inner` gains `dropping: AtomicBool`. Peers are POSIX (`pasteTerminalPaths`: "every peer keeps its own single-quote escaping"). The webview shows "Transferring files to {machine}…" as an info notice before awaiting a peer drop. Register `terminal_drop`.

- [ ] **Step 3: Write `client/src/store/drops.ts`**

```ts
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as bridge from "../bridge";
import { composer } from "./composer";
import { terminal } from "./terminal";
import { notify } from "./actions";
import { detailTab, selected, type DetailTab } from "./ui";
import { inTauri } from "../bridge";

// Files dropped on the window (Qt SessionFileDrop): the Activity tab's
// composer attaches them; a connected Terminal gets their paths pasted, after
// staging them on a peer. Nothing else takes drops.

export function routeDrop(paths: string[], tab: DetailTab, terminalConnected: boolean): "composer" | "terminal" | "none" {
  if (!paths.length) return "none";
  if (tab === "activity") return "composer";
  if (tab === "terminal" && terminalConnected) return "terminal";
  return "none";
}

export function startDrops(): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  void getCurrentWebview().onDragDropEvent(async (event) => {
    if (event.payload.type !== "drop") return;
    const paths = event.payload.paths;
    const s = selected();
    if (!s) return;
    switch (routeDrop(paths, detailTab(), terminal.connected())) {
      case "composer": {
        const v = composer.view();
        if (!v || composer.locked()) return;
        await composer.flush();
        const at = composer.selection().end;
        await bridge.composerAttach(v.handle, paths, at, at);
        break;
      }
      case "terminal": {
        if (s.computer) notify("info", `Transferring files to ${s.machine}…`);
        try {
          const info = await bridge.terminalDrop(terminal.generation(), { computer: s.computer, name: s.name, machine: s.machine, run_id: s.runId ?? "",
            conversation_id: s.conversationId ?? "", tracked: s.tracked, created: s.created ?? -1 }, paths);
          if (info) notify("info", info);
        } catch (error) { notify("error", String(error), true); }
        break;
      }
      default: break;
    }
  }).then((unlisten) => { stop = unlisten; });
  return () => stop?.();
}
```

`terminal.generation()` is a new accessor on the Terminal store returning the current generation. `Session.created` is the row's `created`, which #406 adds. `bridge.terminalDrop(generation, target, paths)` invokes `terminal_drop` with `view: "main"`. `main.tsx` calls `startDrops()` once.

- [ ] **Step 4: Run the tests**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml terminal:: && npm test && npm run check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/terminal.rs client/src-tauri/src/lib.rs client/src
git commit -m "feat(client): dropped files attach to the composer or paste into Terminal"
```

---

### Task 9: Activity attachments: fetch, preview and open

**Depends on:**
- A3 `zerus-5wmh.9.5` (#410): `reads::{AttachmentRef, fetch_attachment, CachedAttachment, attachment_message}`;
- the cache bound `zerus-5wmh.9.17`: `attachments::{prune, touch, CACHE_LIMIT}`;
- Task 3b (`files::Cache`, the `zerus-file` scheme and its CSP) and Task 4b (`Cache::prune_later`, `Composers::retry_files`).

**Open** depends also on R2. Until R2 merges, Open on an Activity attachment and on a composer attachment stays disabled with "Arrives with the downloaded-file marker.", because Qt never opens a program-running type directly and marks files from other machines.

**Qt reference:**
- `tray/src/AttachmentViewer.h:140-215`:
  - `readAsync`;
  - `fetch`, with one job per (host, session, conversation, archive, agent, file) and priority;
  - `open`, with `ExternalFile::Origin::OtherMachine`.
- `tray/src/SessionsWindow.cpp:1104-1117`: playback and preview fetches.
- `tray/src/MessageComposer.cpp:1067-1085`: `openAttachment`, with Qt's notices for revealed, reveal-failed and open-failed.
- `tray/src/ActivityView.cpp`: `attachmentCards`, the preview on hover.

**Files:**
- Modify: `client/src-tauri/src/composer/files.rs` (pins, the limit, `set_extra`, `Fetcher`, the open commands)
- Modify: `client/src-tauri/Cargo.toml` (+ `tauri-plugin-opener = "2"`), `client/src-tauri/Cargo.lock`, `client/src-tauri/src/lib.rs`
- Modify: `client/src-tauri/src/commands.rs` (a later `start` clears the pins)
- Create: `client/src/store/attachments.ts`, `client/src/store/attachments.test.ts`
- Modify: `client/src/ui/sessions/Activity.tsx` (attachment chips with preview), `client/src/ui/sessions/transcript.ts` (fetch refs on chips)
- Modify: `client/src/ui/sessions/Attachments.tsx` (Open on a composer attachment, disabled until R2)
- Modify: `client/src/types.ts`, `client/src/bridge.ts`

**Interfaces:**
- Produces:

```rust
pub type FetchFn = Arc<dyn Fn(&ComputerId, &AttachmentRef, &Path) -> Result<CachedAttachment, String> + Send + Sync>;
pub struct Fetcher; impl Fetcher {
    pub fn new(cache: Arc<Cache>, fetch: FetchFn, spawn: Spawn) -> Arc<Self>;
    pub fn fetch(&self, computer: &str, file: AttachmentRef) -> Result<Fetched, String>; // blocking, shared per file
}
#[derive(Clone, Debug, PartialEq, Serialize)] pub struct Fetched { pub path: String /* "cache/<digest>/<name>" */, pub name: String, pub mime: String, pub bytes: u64 }
// commands: attachment_fetch(computer, file: AttachmentRef) -> Result<Fetched, String>; attachment_pin(path); attachment_unpin(path);
// attachment_open(path) -> Result<Option<String /*notice*/>, String>; attachment_open_draft(handle, index) -> Result<Option<String>, String>
impl Cache {
    pub fn with_limit(root: PathBuf, limit: u64) -> Arc<Self>;
    pub fn pin(&self, path: &Path); pub fn unpin(&self, path: &Path); pub fn clear_pins(&self);
    pub fn set_extra(&self, extra: Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync>);
}
```

```ts
export interface AttachmentRef { name: string; request_id: string; index: number; conversation_id: string; archive_id: string | null; agent_id: string | null }
export interface Fetched { path: string; name: string; mime: string; bytes: number }
// store/attachments.ts
export function createPreviews(port: PreviewPort): { show(computer, ref): void; hide(ref): void; failed(ref): void; state(ref): PreviewState };
export type PreviewState = { kind: "loading" } | { kind: "ready"; url: string; fetched: Fetched } | { kind: "error"; text: string };
```

- [ ] **Step 1: Failing Rust tests** in `composer/files.rs`:

```rust
    #[test]
    fn prune_keeps_open_previews_and_retry_files() {
        let root = dir("prune");
        let cache = Cache::with_limit(root.join("cache"), 10);
        let keep = attachments::store_at(cache.root(), "open.png", &[1u8; 8]).unwrap();
        let retry = attachments::store_at(cache.root(), "retry.txt", &[2u8; 8]).unwrap();
        let old = attachments::store_at(cache.root(), "old.txt", &[3u8; 8]).unwrap();
        cache.pin(&keep);
        let extra: Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync> = { let retry = retry.clone(); Arc::new(move || HashSet::from([retry.clone()])) };
        let spawn: Spawn = Arc::new(|job| job());
        cache.prune_later(&spawn, extra);
        assert!(keep.exists() && retry.exists(), "pinned copies survive");
        assert!(!old.exists(), "the unpinned copy went to stay under the bound");
        cache.unpin(&keep);
        cache.prune_later(&spawn, Arc::new(HashSet::new));
        assert!(!keep.exists());
    }

    #[test]
    fn the_same_attachment_is_fetched_once_for_two_previews() {
        let root = dir("fetch");
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let fetch: FetchFn = Arc::new(move |_, file, root| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(100));
            let local_path = attachments::store_at(root, "shot.png", b"\x89PNG\r\n\x1a\nrest").unwrap();
            Ok(CachedAttachment { metadata: serde_json::json!({"name": "shot.png", "request_id": file.request_id}), local_path })
        });
        let fetcher = Fetcher::new(Cache::new(root.join("cache")), fetch, Arc::new(|job| { std::thread::spawn(job); }));
        let file = AttachmentRef { name: "claude/sample-project/review".into(), request_id: "req-1".into(), index: 0,
            conversation_id: "conv-1".into(), archive_id: None, agent_id: None };
        let (a, b) = (fetcher.clone(), fetcher.clone());
        let (fa, fb) = (file.clone(), file.clone());
        let one = std::thread::spawn(move || a.fetch("studio", fa));
        let two = std::thread::spawn(move || b.fetch("studio", fb));
        let (one, two) = (one.join().unwrap().unwrap(), two.join().unwrap().unwrap());
        assert_eq!(one, two);
        assert!(one.path.starts_with("cache/") && one.path.ends_with("/shot.png"));
        assert_eq!(one.mime, "image/png");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
```

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::files::` and see it fail.

- [ ] **Step 2: Implement** in `composer/files.rs`. `Cache` gains:
  - `limit: u64` (`new` uses `attachments::CACHE_LIMIT`; `with_limit` takes one), which `prune_later` uses instead of the constant;
  - `pins: Mutex<HashMap<PathBuf, usize>>`, counted by canonical path: `pin` adds one, `unpin` removes one and forgets the path at zero, `clear_pins` empties them (a reloaded webview starts without previews). The map holds only what the webview shows now, so it stays small;
  - `extra: Mutex<Option<Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync>>>`, set once by `set_extra`.

  `prune_later` keeps a copy when it is pinned, in its own `extra`, or in the set extra:

```rust
            let mut kept: HashSet<PathBuf> = extra().iter().map(|path| canonical(path)).collect();
            if let Some(more) = lock(&cache.extra).clone() {
                kept.extend(more().iter().map(|path| canonical(path)));
            }
            let pins: HashSet<PathBuf> = lock(&cache.pins).keys().cloned().collect();
            attachments::prune(&cache.root, cache.limit, |path| {
                let path = canonical(path);
                pins.contains(&path) || kept.contains(&path)
            });
```

`lib.rs` calls `composers.cache().set_extra(...)` with the composers' `retry_files`, so the Fetcher's prunes keep Retry's files too, and `start` (a reloaded webview) calls `composers.cache().clear_pins()`. Then:

```rust
use std::sync::mpsc;
use zerus_client_core::reads::{AttachmentRef, CachedAttachment};
use zerus_client_core::ComputerId;

pub type FetchFn = Arc<dyn Fn(&ComputerId, &AttachmentRef, &Path) -> Result<CachedAttachment, String> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fetched { pub path: String, pub name: String, pub mime: String, pub bytes: u64 }

type Waiters = Vec<mpsc::Sender<Result<Fetched, String>>>;

/// Reads Activity attachments into the cache, one read per file however many
/// views ask (`AttachmentLoader::fetch`).
pub struct Fetcher {
    cache: Arc<Cache>,
    fetch: FetchFn,
    spawn: Spawn,
    jobs: Mutex<HashMap<String, Waiters>>,
}

impl Fetcher {
    pub fn new(cache: Arc<Cache>, fetch: FetchFn, spawn: Spawn) -> Arc<Self> {
        Arc::new(Fetcher { cache, fetch, spawn, jobs: Mutex::new(HashMap::new()) })
    }

    pub fn cache(&self) -> &Arc<Cache> { &self.cache }

    /// Blocks until the file is cached; a second caller for the same file waits for the first read.
    pub fn fetch(self: &Arc<Self>, computer: &str, file: AttachmentRef) -> Result<Fetched, String> {
        let key = serde_json::json!([computer, file.name, file.conversation_id, file.archive_id, file.agent_id, file.request_id, file.index]).to_string();
        let (sender, receiver) = mpsc::channel();
        let first = {
            let mut jobs = lock(&self.jobs);
            let waiters = jobs.entry(key.clone()).or_default();
            waiters.push(sender);
            waiters.len() == 1
        };
        if first {
            let me = Arc::clone(self);
            let computer = ComputerId(computer.to_owned());
            let job_key = key.clone();
            (self.spawn)(Box::new(move || {
                let result = (me.fetch)(&computer, &file, me.cache.root()).map(|cached| {
                    let relative = cached.local_path.strip_prefix(me.cache.root()).unwrap_or(&cached.local_path).to_string_lossy().replace('\\', "/");
                    let bytes = std::fs::metadata(&cached.local_path).map(|m| m.len()).unwrap_or(0);
                    let mut head = Vec::new();
                    if let Ok(file) = std::fs::File::open(&cached.local_path) {
                        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(file, 512), &mut head);
                    }
                    Fetched {
                        path: format!("cache/{relative}"),
                        name: cached.metadata["name"].as_str().unwrap_or("attachment").to_owned(),
                        mime: cached.metadata["mime"].as_str().map(str::to_owned).unwrap_or_else(|| attachments::sniff(&head)),
                        bytes,
                    }
                });
                if result.is_ok() {
                    me.cache.prune_later(&me.spawn, Arc::new(HashSet::new));
                }
                let waiters = lock(&me.jobs).remove(&job_key).unwrap_or_default();
                for waiter in waiters {
                    let _ = waiter.send(result.clone());
                }
            }));
        }
        receiver.recv().map_err(|_| "Could not read the attachment.".to_owned())?
    }
}
```

The app's `FetchFn` is `|computer, file, root| reads::fetch_attachment(&default_hgs(), computer, file, root)`, refusing with "Actions in replay mode need ZERUS_CLIENT_HGS." when `actions_allowed` is false, so a replay never reaches the real CLI. The Fetcher shares the composers' `Cache` (`Fetcher::new(composers.cache(), ...)`), so one root, one set of pins and one prune lane serve both.

The commands, in `files.rs`:

```rust
#[tauri::command]
pub async fn attachment_fetch(state: tauri::State<'_, Arc<Fetcher>>, computer: String, file: AttachmentRef) -> Result<Fetched, String> {
    let fetcher = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || fetcher.fetch(&computer, file)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn attachment_pin(state: tauri::State<'_, Arc<Fetcher>>, path: String) {
    if let Some(relative) = path.strip_prefix("cache/") { state.cache().pin(&state.cache().root().join(relative)); }
}

#[tauri::command]
pub fn attachment_unpin(state: tauri::State<'_, Arc<Fetcher>>, path: String) {
    if let Some(relative) = path.strip_prefix("cache/") { state.cache().unpin(&state.cache().root().join(relative)); }
}
```

`attachment_open(path)` and `attachment_open_draft(handle, index)` (R2):
- They resolve the file through `attachments::cached_at`; the draft variant first `attachments::prepare`s the one attachment into the cache.
- If `external_file::runs_programs(path)`, the file is revealed instead (`app.opener().reveal_item_in_dir(path)`), answering `external_file::revealed_notice(path)`, or `reveal_failed_notice(path)` when the reveal fails.
- Otherwise a fetched file gets `external_file::mark_downloaded(path)` (origin `OtherMachine`), and then `app.opener().open_path(path, None::<&str>)`; a failure answers "Could not open the attachment with its default application.".
- Until R2 merges, both commands answer `Err("Arrives with the downloaded-file marker.")` and the buttons that call them are disabled with that title.

Register the `Fetcher` as managed state, `tauri_plugin_opener::init()`, and the five commands.

- [ ] **Step 3: Failing store tests** in `client/src/store/attachments.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createPreviews, type PreviewPort } from "./attachments";
import type { AttachmentRef } from "../types";

const ref: AttachmentRef = { name: "claude/sample-project/review", request_id: "req-1", index: 0, conversation_id: "conv-1", archive_id: null, agent_id: null };

function fakePort(results: (string | Error)[]) {
  const calls: string[] = [];
  const port: PreviewPort = {
    fetch: async () => { const next = results.shift(); calls.push("fetch"); if (next instanceof Error) throw next.message; return { path: next!, name: "shot.png", mime: "image/png", bytes: 8 }; },
    pin: (path) => { calls.push(`pin ${path}`); },
    unpin: (path) => { calls.push(`unpin ${path}`); },
    url: (path) => `zerus-file://localhost/${path}`,
  };
  return { port, calls };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("attachment previews", () => {
  it("pin while shown and unpin when hidden", async () => {
    const f = fakePort(["cache/abc/shot.png"]);
    await createRoot(async () => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      await tick();
      expect(p.state(ref)).toMatchObject({ kind: "ready", url: "zerus-file://localhost/cache/abc/shot.png" });
      p.hide(ref);
      expect(f.calls).toEqual(["fetch", "pin cache/abc/shot.png", "unpin cache/abc/shot.png"]);
    });
  });

  it("a vanished preview fetches again once, then shows the error", async () => {
    const f = fakePort(["cache/abc/shot.png", "cache/abc/shot.png", new Error("Invalid attachment response. Try again.")]);
    await createRoot(async () => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      await tick();
      p.failed(ref);
      await tick();
      expect(p.state(ref).kind).toBe("ready");
      expect(f.calls.filter((c) => c === "fetch").length).toBe(2);
      p.failed(ref);
      await tick();
      expect(p.state(ref)).toEqual({ kind: "error", text: "Could not show the attachment." });
    });
  });
});
```

Run: `cd client && npm test -- src/store/attachments.test.ts` and see it fail.

- [ ] **Step 4: Write `client/src/store/attachments.ts`**

```ts
import { createStore } from "solid-js/store";
import { convertFileSrc } from "@tauri-apps/api/core";
import * as bridge from "../bridge";
import type { AttachmentRef, Fetched } from "../types";

// Activity attachment previews (Qt AttachmentLoader): a shown preview fetches
// its file into the private cache once and pins it, so the cache bound never
// removes it while it shows; hiding unpins. A file that disappeared anyway
// (removed outside the client) is fetched once more; a second failure shows
// an error instead of retrying.

export interface PreviewPort {
  fetch(computer: string, ref: AttachmentRef): Promise<Fetched>;
  pin(path: string): void;
  unpin(path: string): void;
  url(path: string): string;
}

export type PreviewState = { kind: "loading" } | { kind: "ready"; url: string; fetched: Fetched } | { kind: "error"; text: string };

const keyOf = (r: AttachmentRef) => JSON.stringify([r.name, r.conversation_id, r.archive_id, r.agent_id, r.request_id, r.index]);

export function createPreviews(port: PreviewPort) {
  const [states, setStates] = createStore<Record<string, PreviewState>>({});
  const retried = new Set<string>();
  const shown = new Map<string, { computer: string; path?: string }>();

  async function load(computer: string, ref: AttachmentRef) {
    const key = keyOf(ref);
    setStates(key, { kind: "loading" });
    try {
      const fetched = await port.fetch(computer, ref);
      const entry = shown.get(key);
      if (!entry) return;
      if (entry.path) port.unpin(entry.path);
      entry.path = fetched.path;
      port.pin(fetched.path);
      setStates(key, { kind: "ready", url: port.url(fetched.path), fetched });
    } catch (error) {
      setStates(key, { kind: "error", text: String(error) });
    }
  }

  return {
    state: (ref: AttachmentRef): PreviewState => states[keyOf(ref)] ?? { kind: "loading" },
    show(computer: string, ref: AttachmentRef) {
      const key = keyOf(ref);
      if (shown.has(key)) return;
      shown.set(key, { computer });
      void load(computer, ref);
    },
    hide(ref: AttachmentRef) {
      const key = keyOf(ref);
      const entry = shown.get(key);
      shown.delete(key);
      if (entry?.path) port.unpin(entry.path);
    },
    /** The image or player could not load the cached file. */
    failed(ref: AttachmentRef) {
      const key = keyOf(ref);
      const entry = shown.get(key);
      if (!entry) return;
      if (retried.has(key)) { setStates(key, { kind: "error", text: "Could not show the attachment." }); return; }
      retried.add(key);
      void load(entry.computer, ref);
    },
  };
}

export const previews = createPreviews({
  fetch: bridge.attachmentFetch, pin: bridge.attachmentPin, unpin: bridge.attachmentUnpin,
  url: (path) => convertFileSrc(path, "zerus-file"),
});
```

- [ ] **Step 5: Show them in Activity, and Open in the composer's list.**
  - `transcript.ts` keeps, on each attachment of an item, a fetch reference built from the event (or the local row's file after a send): `request_id` and `index`, the session's name, the followed summary's `conversation_id`, the row's `archive_id` when archived, and the event's `agent_id`.
  - In `Activity.tsx`, an attachment chip with a reference shows, on hover or after an arrow-key move (never on focus alone, round 6), a preview of at most 320 px beside it. It is an `<img>` with `onError={() => previews.failed(ref)}` for an image, or the first 2 KiB of a text file read through `fetch(url)`.
  - Hiding the preview calls `previews.hide(ref)`.
  - Open uses `bridge.attachmentOpen(path)` (disabled until R2).
  - In `Attachments.tsx`, a composer attachment row gains Open: a click, Enter or Space calls `bridge.attachmentOpenDraft(handle, index)`, and the row's title reads "Open {name}". Until R2 the row's Open is disabled with "Arrives with the downloaded-file marker." as its title.

- [ ] **Step 6: Run the checks**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri client/src
git commit -m "feat(client): fetch, preview and pin Activity attachments in the private cache"
```

---

### Task 10: The Processes hub (`client/src-tauri/src/processes.rs`)

**Depends on:** P1 `zerus-5wmh.9.6` (#411, stacked on A2 #404):
- `process_view::{active, stoppable, status_label, status_tooltip, output_revision, normalize, group, group_title, Group, summary, notes, details, duration, tone, Tone, Settings, output_due, output_unavailable, output, Output, StopContext, StopControl, stop_control, Stopped, stop_result, Next, StopQueue}`;
- `Read::ProcessOutput`, `Action::StopProcess`, `Lane::Process`.

The processes topic (`Fleet::follow_processes`) is on main (`zerus-5wmh.21`).

**Qt reference:**
- `tray/src/ProcessView.cpp`:
  - `setSession` (99-150), with its identity key and the default selection;
  - `select` and `details` (151-163), `updateActions` (166-185), `updateBusy` (186-191);
  - `requestOutput` (192-202), `stopSelected` and `stopNext` (203-220);
  - the output reply (81-93), `refreshDurations` (225-233).
- `tray/src/SessionsWindow.cpp:3783-3787`: the session the view shows, and the tab's count.
- `docs/reference.md`, "Processes launched by agents".

**Files:**
- Create: `client/src-tauri/src/processes.rs`
- Modify: `client/src-tauri/src/commands.rs`:
  - `Live::follow_processes` and `Live::unfollow_processes`;
  - the envelope's `processes_view`;
  - `observe` takes the batch's processes entries.
- Modify: `client/src-tauri/src/replay.rs` (processes states per session, served on follow)
- Modify: `client/src-tauri/src/lib.rs`
- Modify: `client/scripts/make-fixture.py`, then regenerate `client/fixtures/replay*.ndjson` and `replay-batches.json`
- Test: unit tests in `processes.rs`; a replay test in `replay.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessTarget { pub computer: String, pub key: String, pub name: String, pub run_id: String, pub conversation_id: String,
    #[serde(default)] pub archive_id: Option<String>, pub machine: String, pub online: bool, pub live: bool }
#[derive(Clone, Debug, Default, PartialEq, Serialize)] pub struct RowView { pub id: String, pub command: String, pub command_full: String, pub owner: String, pub status: String, pub tooltip: String, pub tone: Tone, pub duration: String }
#[derive(Clone, Debug, PartialEq, Serialize)] pub struct GroupView { pub group: Group, pub title: String, pub rows: Vec<RowView> }
#[derive(Clone, Debug, Default, PartialEq, Serialize)] pub struct DetailView { pub id: String, pub command: String, pub details: String, pub output: Option<String>, pub notice: String }
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProcessesView { pub key: String, pub summary: String, pub summary_tooltip: String, pub notes: String, pub groups: Vec<GroupView>,
    pub selected: Vec<String>, pub current: Option<String>, pub stop: StopControl, pub stop_notice: String, pub busy: bool,
    pub failed: Option<String>, pub detail: Option<DetailView>, pub count: usize }
pub struct ProcessHub; impl ProcessHub {
    pub fn new(deps: ProcessDeps, hgs_allowed: bool) -> Self;
    pub fn follow(&self, target: Option<ProcessTarget>);
    pub fn settings(&self, settings: Settings);
    pub fn shown(&self, shown: bool);
    pub fn select(&self, ids: Vec<String>, current: Option<String>);
    pub fn stop(&self);
    pub fn observe(&self, batch: &mut Batch);   // takes the followed key's entries out of batch.processes
    pub fn take_view(&self) -> Option<ProcessesView>;
    pub fn tick(&self);                          // every 250 ms
}
pub struct ProcessDeps {
    pub spawn: Spawn, pub notify: Notify, pub clock_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub follow: Arc<dyn Fn(&ProcessTarget) -> Result<(), String> + Send + Sync>,
    pub unfollow: Arc<dyn Fn(&ProcessTarget) + Send + Sync>,
    pub read: Arc<dyn Fn(&ComputerId, &Read) -> Result<Value, String> + Send + Sync>,
    pub stop: Arc<dyn Fn(Target, Action) -> Result<Outcome, String> + Send + Sync>,
}
// commands: processes_follow(target: Option<ProcessTarget>); processes_settings(settings: Settings); processes_shown(shown);
// processes_select(ids, current); processes_stop()
// next_batch envelope: processes_view?: ProcessesView
```

- [ ] **Step 1: Write the failing tests** at the end of `processes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zerus_client_core::{ComputerId, ProcessesOut};

    struct Fake { now: AtomicU64, reads: Mutex<Vec<String>>, stops: Mutex<Vec<String>>, stop_ok: Mutex<bool> }

    fn hub() -> (ProcessHub, Arc<Fake>) {
        let fake = Arc::new(Fake { now: AtomicU64::new(0), reads: Mutex::new(vec![]), stops: Mutex::new(vec![]), stop_ok: Mutex::new(true) });
        let (a, b, c) = (fake.clone(), fake.clone(), fake.clone());
        let hub = ProcessHub::new(ProcessDeps {
            spawn: Arc::new(|job| job()),
            notify: Arc::new(|| {}),
            clock_ms: Arc::new(move || a.now.load(Ordering::SeqCst)),
            follow: Arc::new(|_| Ok(())),
            unfollow: Arc::new(|_| {}),
            read: Arc::new(move |_, read| {
                let Read::ProcessOutput { process_id, .. } = read else { panic!() };
                lock(&b.reads).push(process_id.clone());
                Ok(json!({"id": process_id, "run_id": "run-1", "conversation_id": "conv-1", "output": "running 42 tests", "truncated": false}))
            }),
            stop: Arc::new(move |_, action| {
                let Action::StopProcess { process_id, .. } = action else { panic!() };
                lock(&c.stops).push(process_id.clone());
                Ok(if *lock(&c.stop_ok) {
                    Outcome::Done { receipt: json!({"id": process_id, "run_id": "run-1", "conversation_id": "conv-1", "status": "requested"}), cleanup: None }
                } else {
                    Outcome::Failed { detail: "Stop was not confirmed".into() }
                })
            }),
        }, true);
        (hub, fake)
    }

    fn target() -> ProcessTarget {
        ProcessTarget { computer: "studio".into(), key: "run:run-1".into(), name: "claude/sample-project/review".into(), run_id: "run-1".into(),
            conversation_id: "conv-1".into(), archive_id: None, machine: "studio".into(), online: true, live: true }
    }

    fn process(id: &str, status: &str, source: &str) -> Value {
        json!({"id": id, "status": status, "source": source, "command": "cargo   test --workspace", "owner": "claude", "cwd": "/home/example/src/sample-project",
               "started_at": 100.0, "capabilities": {"stop": true, "output": true}})
    }

    fn state(items: Vec<Value>) -> Batch {
        Batch { processes: vec![ProcessesOut { computer: ComputerId::from("studio"), key: "run:run-1".into(), kind: "state".into(),
            payload: json!({"value": {"processes": {"items": items, "active_count": 2, "live_count": 1, "complete": true}}, "rev": 1})}], ..Batch::default() }
    }

    fn enabled(hub: &ProcessHub) {
        hub.settings(Settings { enabled: true, ..Settings::default() });
        hub.follow(Some(target()));
    }

    #[test]
    fn the_view_groups_labels_and_takes_its_entries_out_of_the_batch() {
        let (hub, _) = hub();
        enabled(&hub);
        let mut batch = state(vec![process("p1", "running", "process_tree"), process("p2", "unknown", "hook"), process("p3", "finished", "hook")]);
        hub.observe(&mut batch);
        assert!(batch.processes.is_empty());
        let view = hub.take_view().unwrap();
        let titles: Vec<&str> = view.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Live shells / processes", "Recorded commands / unconfirmed", "Finished"]);
        assert_eq!(view.groups[0].rows[0].command, "cargo test --workspace");
        assert_eq!(view.groups[1].rows[0].status, "Unconfirmed");
        assert_eq!(view.summary, "1 live    0 active    1 unconfirmed    2 recorded");
        assert_eq!(view.current.as_deref(), Some("p1"), "the first live process is selected");
        assert_eq!(view.count, 2);
    }

    #[test]
    fn output_is_read_while_shown_at_qts_cadence() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        hub.tick();
        assert!(lock(&fake.reads).is_empty(), "a hidden tab reads no output");
        hub.shown(true);
        hub.tick();
        assert_eq!(*lock(&fake.reads), ["p1"]);
        fake.now.store(1000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1, "within 2.5 s nothing is read again");
        fake.now.store(2600, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 2);
        let detail = hub.take_view().unwrap().detail.unwrap();
        assert_eq!(detail.output.as_deref(), Some("running 42 tests"));
    }

    #[test]
    fn stops_run_one_at_a_time_and_are_never_retried() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook"), process("p2", "running", "hook")]));
        hub.select(vec!["p1".into(), "p2".into()], Some("p1".into()));
        let view = hub.take_view().unwrap();
        assert_eq!(view.stop.label, "Stop selected (2)");
        assert!(view.stop.enabled);
        *lock(&fake.stop_ok) = false;
        hub.stop();
        assert_eq!(*lock(&fake.stops), ["p1", "p2"]);
        let view = hub.take_view().unwrap();
        assert!(view.stop_notice.contains("2 not confirmed. No automatic retries."));
        hub.tick();
        assert_eq!(lock(&fake.stops).len(), 2);
    }

    #[test]
    fn a_new_identity_cancels_unsent_stops() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        hub.select(vec!["p1".into()], Some("p1".into()));
        let mut other = target();
        other.run_id = "run-2".into();
        other.key = "run:run-2".into();
        hub.follow(Some(other));
        hub.stop();
        assert!(lock(&fake.stops).is_empty());
    }
}
```

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml processes::` and see it fail.

- [ ] **Step 2: Write `processes.rs`.** The hub follows one session (the selected one, while Processes is enabled) and projects its view through `process_view`. Every rule is P1's, and the sequencing is Qt's.

```rust
//! The Processes tab's hub, ported from `tray/src/ProcessView.cpp`: the
//! selected session's processes from the watch topic (`Fleet::follow_processes`),
//! projected with P1's `process_view` rules; the selected process's output,
//! read again at Qt's cadence only while the tab shows; and the stop queue,
//! sequential and never retried, cancelled by a change of the session's
//! identity (host, name, archive, run, conversation, host generation).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zerus_client_core::actions::{Action, Outcome, Target};
use zerus_client_core::process_view::{self as rules, Group, Next, Settings, StopContext, StopControl, StopQueue, Stopped, Tone};
use zerus_client_core::reads::Read;
use zerus_client_core::{Batch, ComputerId};

pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn() + Send + Sync>;

// ProcessTarget, RowView, GroupView, DetailView, ProcessesView, ProcessDeps as in "Produces".

#[derive(Default)]
struct OutputState { process: String, text: Option<String>, notice: String, revision: Value, read_at: Option<u64>, in_flight: Option<u64>, dirty: bool, forced: bool }

#[derive(Default)]
struct Hub {
    settings: Settings,
    shown: bool,
    target: Option<ProcessTarget>,
    value: Option<Value>,
    failed: Option<String>,
    selected: Vec<String>,
    current: Option<String>,
    output: OutputState,
    queue: StopQueue,
    stop_notice: String,
    stop_in_flight: bool,
    last: Option<ProcessesView>,
    last_second: u64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> { mutex.lock().unwrap_or_else(PoisonError::into_inner) }

fn text<'a>(value: &'a Value, key: &str) -> &'a str { value.get(key).and_then(Value::as_str).unwrap_or("") }

impl Hub {
    fn live(&self) -> bool { self.target.as_ref().is_some_and(|t| t.live) }

    /// Normalized records in arrival order (`setSession` 112-121).
    fn records(&self) -> Vec<Value> {
        let live = self.live();
        self.value.as_ref().and_then(|v| v["processes"]["items"].as_array()).into_iter().flatten()
            .filter(|r| !text(r, "id").is_empty())
            .map(|r| { let mut r = r.clone(); rules::normalize(&mut r, live); r })
            .collect()
    }

    fn find(&self, id: &str) -> Option<Value> { self.records().into_iter().find(|r| text(r, "id") == id) }

    fn generation(&self) -> String {
        let value = self.value.clone().unwrap_or_default();
        let top = text(&value, "host_generation");
        if top.is_empty() { text(&value["processes"], "host_generation").to_owned() } else { top.to_owned() }
    }

    /// Qt's view key: host, name, archive, run, conversation and generation.
    fn identity(&self) -> String {
        self.target.as_ref().map(|t| [t.computer.as_str(), &t.name, t.archive_id.as_deref().unwrap_or(""), &t.run_id, &t.conversation_id, &self.generation()].join("\n")).unwrap_or_default()
    }

    fn context(&self) -> StopContext {
        StopContext {
            enabled: self.settings.enabled,
            online: self.target.as_ref().is_some_and(|t| t.online),
            live: self.live(),
            sending: self.stop_in_flight || self.queue.sending(),
            pending: self.queue.pending.clone(),
        }
    }
}
```

The `ProcessHub` methods:
- **`follow(target)`:** a new identity (any field of the target, or another catalog key) resets `queue`, `selected`, `current`, the output and `stop_notice` (`setSession` "changed"). It calls `deps.unfollow` for the old target and `deps.follow` for the new one, only while `settings.enabled`; a refused follow (the 16-subscription limit) sets `failed`.
- **`observe(batch)`:** `batch.processes.retain(...)` removes the entries of the followed key.
  - `state` sets `value` (`payload.value`) and clears `failed`; `failed` sets `failed` to `payload.message`; `ended` clears `value`.
  - Then `queue.settle(find)`.
  - The output turns dirty when `output_revision` of the current record changed (`setSession` 149).
  - When `current` is gone, the default selection applies: the first live row, else the first recorded one, else the first finished one (125).
- **`select(ids, current)`:** stores them. A new `current` resets the output and sets `forced` (`select` 153).
- **`stop()`:**
  - `stop_control(selected, context)` must be enabled;
  - then `queue.start(identity, selected)`, `stop_notice = "Sending stop requests…"`, and `step_stops()`.
- **`step_stops()`:**
  - `queue.next(identity, find, online && live && enabled)`;
  - `Next::Stop(id)` dispatches `Action::StopProcess { process_id: id, archive_id, generation: Some(generation).filter(non-empty) }` through `deps.stop` (on `spawn`, `Lane::Process`), then maps the outcome: `Done { receipt }` becomes `rules::stop_result(Ok(&receipt), id, run, conversation)`, and `Failed` or `Uncertain` becomes `Stopped::Failed(detail)`;
  - then `queue.finished(result)` and `step_stops()` again;
  - `Next::Done(message)` sets `stop_notice`.
- **`tick()`:** when shown, enabled, online and a current record exists:
  - `output_unavailable(record)` sets the notice and skips;
  - `output_due(record, settings, forced, dirty, age)` starts `Read::ProcessOutput { name, process_id, run_id, conversation_id, archive_id, generation }` through `deps.read` on `spawn`, one at a time;
  - its reply becomes `rules::output(&reply, id, run, conversation)`: `text` replaces the output when `Some`, `notice` is shown, and `refresh` clears `value` until the next state;
  - a failure's text becomes the notice.

  `busy` is true while a read or stop has run for at least `settings.loading_ms()` (`updateBusy`). While shown, a new second also marks the view changed, for the duration column.
- **`view()`:**
  - groups in the order Live, Recorded, Finished, with `group_title(group, live)`;
  - rows with `command` (whitespace collapsed), `command_full`, `owner`, `status_label`, `status_tooltip`, `tone` and `duration(record, now_seconds)`;
  - `summary(records, live)`, and `notes(&value["processes"])` for the notes and the summary tooltip;
  - `stop_control`, `stop_notice`, `busy` and `failed`;
  - `detail` for the current record: `details(record, machine-or-empty)` and the output;
  - `count` is 0 when recorded (not online or not live), else `max(live_count, active_count)`.

  `take_view` returns it when it differs from the last one taken.

- [ ] **Step 3: The data path.**
  - `commands.rs`: `Live::follow_processes(&self, target: &ProcessTarget) -> Result<(), String>` calls `Fleet::follow_processes(&ComputerId(target.computer), SessionRef { name, run_id: Some(run), conversation_id: Some(conversation) })` and checks the returned key equals `target.key`. `unfollow_processes` calls `Fleet::unfollow_processes`. A replay serves processes too (below).
  - The envelope gains `#[serde(skip_serializing_if = "Option::is_none")] processes_view: Option<crate::processes::ProcessesView>`, and `next_batch` calls `processes.observe(&mut batch)` before serializing.
  - `replay.rs`:
    - `load` keeps `Body::State` frames whose topic is `topic::PROCESSES` per key, as it keeps Activity logs;
    - `Replay::follow_processes(computer, key)` sends `Change::Processes { computer, key, update: ProcessesUpdate::State { value, rev } }` to the sink when a state is held;
    - test: a fixture line `{"v":1,"seq":9,"t":"state","topic":"processes","key":"run:a","value":{"processes":{"items":[]}},"rev":1}` is served once on follow.
  - `make-fixture.py`:
    - add `"processes"` to every hello's `topics` and `"processes_subscriptions": 16` to `limits`;
    - write one `processes` state for `claude/sample-project/api-refactor`, with a live `cargo test --workspace` (`source: process_tree`, `capabilities.stop: true, output: true`), a recorded `npm run build` (`status: unknown`) and a finished `git fetch`;
    - every path `/home/example/...`. Run `python3 client/scripts/make-fixture.py` and commit the regenerated fixtures.
  - `lib.rs`: manage the hub, whose deps are:
    - `read`: `reads::read(&default_hgs(), computer, read).map_err(|e| e.to_string())`;
    - `stop`: `block_on(actions::dispatch(...))`;
    - `follow` and `unfollow`: through `AppState`'s `Live`;
    - `clock_ms`: an `Instant` since start;
    - a 250 ms ticker thread calling `tick`.

    Register the five commands. With `actions_allowed` false (a replay without `ZERUS_CLIENT_HGS`), output reads and stops refuse with "Actions in replay mode need ZERUS_CLIENT_HGS." as their notice.

- [ ] **Step 4: Run the tests**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml processes:: replay::`
Expected: PASS. Clippy and fmt clean.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri client/scripts/make-fixture.py client/fixtures
git commit -m "feat(client): the Processes hub with output reads and the sequential stop queue"
```

---

### Task 11: The Processes tab and Settings > Processes

**Depends on:** Task 10.

**Qt reference:**
- `tray/src/ProcessView.cpp:36-79`: the layout, the columns Command, Agent, State and Duration, the context menu's "Terminate" and "Terminate selected (N)", and Copy command.
- `tray/src/SettingsPage.h:109-120`: the section, its five controls and the hint.
- `tray/src/SessionsWindow.cpp`: the tab and its count (1330, 3787), and the menu entry "Show running commands" (5298-5299, 5463-5465).

**Files:**
- Modify: `client/src/types.ts`, `client/src/bridge.ts`
- Create: `client/src/store/processes.ts`, `client/src/store/processes.test.ts`
- Create: `client/src/ui/sessions/Processes.tsx`
- Modify: `client/src/store/settings.ts` (the five `processes/*` keys), `client/src/store/settings.test.ts`
- Modify: `client/src/ui/components/SettingsScreen.tsx` (the Processes section)
- Modify: `client/src/ui/sessions/SessionDetail.tsx` (the tab), `client/src/ui/sessions/SessionList.tsx` (the menu entry), `client/src/store/session.ts` (route `processes_view`, follow the selection)

**Interfaces:**
- Produces:

```ts
export interface ProcessesView { key: string; summary: string; summary_tooltip: string; notes: string; groups: { group: "live" | "recorded" | "finished"; title: string; rows: ProcessRow[] }[];
  selected: string[]; current: string | null; stop: { label: string; enabled: boolean; tooltip: string }; stop_notice: string; busy: boolean; failed: string | null;
  detail: { id: string; command: string; details: string; output: string | null; notice: string } | null; count: number }
export interface ProcessRow { id: string; command: string; command_full: string; owner: string; status: string; tooltip: string; tone: "danger" | "muted" | "warning" | "success"; duration: string }
// store/processes.ts
export function nextSelection(order: string[], selected: string[], current: string | null, clicked: string, mods: { toggle: boolean; range: boolean }): { selected: string[]; current: string };
export const processes: { view(): ProcessesView | undefined; apply(v): void; click(id, mods): void; stop(): void; setShown(on: boolean): void };
// settings.ts: processesEnabled, processesActiveSeconds (2.5), processesBackgroundSeconds (30), processesUnknownSeconds (10), processesLoadingSeconds (0.5)
```

- [ ] **Step 1: Failing tests.** `client/src/store/processes.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { nextSelection } from "./processes";

const order = ["p1", "p2", "p3", "p4"];

describe("process selection (QTreeWidget ExtendedSelection)", () => {
  it("a click selects one, Ctrl/Cmd toggles, Shift extends from the current row", () => {
    expect(nextSelection(order, ["p1"], "p1", "p3", { toggle: false, range: false })).toEqual({ selected: ["p3"], current: "p3" });
    expect(nextSelection(order, ["p1"], "p1", "p3", { toggle: true, range: false })).toEqual({ selected: ["p1", "p3"], current: "p3" });
    expect(nextSelection(order, ["p1", "p3"], "p3", "p3", { toggle: true, range: false })).toEqual({ selected: ["p1"], current: "p3" });
    expect(nextSelection(order, ["p2"], "p2", "p4", { toggle: false, range: true })).toEqual({ selected: ["p2", "p3", "p4"], current: "p4" });
  });
});
```

In `settings.test.ts`, add a case that a stored `processes/enabled: true` and `processes/activeSeconds: 0.2` read back as `true` and `0.2`; the Rust hub clamps it to 1 s. Run: `cd client && npm test -- src/store/processes.test.ts src/store/settings.test.ts`, which fails.

- [ ] **Step 2: Implement.**
  - **Settings:** `settings.ts` gains the five keys with Qt's defaults (`false`, 2.5, 30, 10, 0.5). Numbers read as finite numbers, else the default.
  - **Store:** `store/processes.ts` holds the view applied from `b.processes_view` and implements `nextSelection`. A click calls `bridge.processesSelect(selected, current)`; `stop()` calls `bridge.processesStop()`.
  - **Effects in `session.ts`:**
    - On every change of `settings.processes*`: `bridge.processesSettings({ enabled, active_seconds, background_seconds, unknown_seconds, loading_seconds })`.
    - On the selected session or `processesEnabled`: `bridge.processesFollow(enabled && s ? { computer, key: catalogKey, name, run_id, conversation_id, archive_id, machine, online: !computerOffline(computer), live } : null)`, with `live` Qt's `!recorded` (online, not archived or paused, process state not `exited`).
    - On `detailTab() === "processes"`: `bridge.processesShown(...)`.
  - **`Processes.tsx`,** the mockup's `ProcessesTab` with live data:
    - the summary (with `summary_tooltip`), a spinner while `busy`, and the danger button `stop.label` (enabled by `stop.enabled`, titled `stop.tooltip`, calling `processes.stop()`);
    - `stop_notice` and `notes` as hint lines, and `failed` as a warning;
    - the table with one group header per group (Finished starts collapsed and toggles) and rows of Command (`command`, titled `command_full`), Agent, State (in the `tone` colour, titled `tooltip`) and Duration;
    - a right click on a row selects it if it is not selected and opens the menu with "Terminate" or "Terminate selected (N)", enabled and titled as the stop button;
    - the detail pane with the command, `details` (pre-wrapped), the notice, a Copy command button (`bridge.copyText(command_full)`), and the output in a `pre`, which stays at the bottom when it was there before an update;
    - "Select a command to view its output." without a current process.
  - **The tab** shows when `settings.processesEnabled`, labelled "Processes ({count})" when `count > 0`, else "Processes".
  - **The Settings section** "Processes":
    - the toggle "Enable Processes tab";
    - four number fields with a " s" suffix, step 0.5, and Qt's ranges: "Open Processes tab" 1-60, "Background process count" 5-600, "Unconfirmed process output" 1-600, "Show loading indicator after" 0-5, disabled while the toggle is off;
    - Qt's hint text.
  - **The session menu** gains "Show running commands", enabled when the row is online and Processes is enabled. Its hint is "Follow this session's background work in Processes", or "Enable Settings > Processes to inspect running commands". It selects the row and opens the tab.

- [ ] **Step 3: Run the checks**

Run: `cd client && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS. The summary's four-space gaps are Qt's and are not dot separators.

- [ ] **Step 4: Commit**

```bash
git add client/src
git commit -m "feat(client): the Processes tab and Settings > Processes"
```

---

### Task 12: Fixtures, the Part A smoke run, docs and follow-ups

**Depends on:** Tasks 1-11 as merged.

**Files:**
- Create: `client/src-tauri/tests/support/fake_hgs_phase4.py`
- Modify: `client/scripts/make-fixture.py`, then regenerate `client/fixtures/*`:
  - a warm prompt cache on `claude/sample-project/api-refactor`'s Activity state (`session_usage.prompt_cache: {status: warm, expires_at: BASE_AT + 1800}`, where S2 reads it);
  - a cold `cache_hint` on `codex/sample-project/pagination`;
  - a sent message with one attachment (`attachments: [{name: "orders.json", request_id: "req-fixture-1", index: 0, mime: "application/json", size: 812}]`) on api-refactor;
  - a paused `claude/sample-project/review` (already in the fixture) with `resumable: true`.
- Modify: `client/src/ui/PerfHost.tsx` (`?phase4-smoke=1`)
- Modify: `client/src-tauri/src/commands.rs` and `client/src-tauri/src/lib.rs` (`smoke_log`)
- Modify: `client/README.md`
- Modify: `.beads/issues.jsonl` (export after filing the follow-ups)

- [ ] **Step 1: Write the fake.** `fake_hgs_phase4.py` reads `config.json` beside itself and logs every call to `log.ndjson` as `{argv, stdin}`, never echoing message bytes elsewhere. It answers by the arguments after an optional `@alias`:
  - `a NAME --existing ...` runs `client/core/tests/support/fake_attach.py` with the same arguments (`os.execv` with the path from `config.attach`);
  - `send NAME --json`: the `echo_send` receipt of `fake_hgs_actions.py` (status `submitted`, the request's `request_id`, `name`, `run_id`, `conversation_id`);
  - `attachment NAME --stage --json`: `staged`, with one `files[].path` per input attachment under `/home/example/.hgs/drop/`;
  - `attachment NAME --request R --index N --conversation C`: `{request_id: R, index: N, name: "orders.json", mime: "application/json", data_base64: base64 of config.attachment}`;
  - `inspect NAME --after 0 --skip-processes`: `config.inspect[NAME]`;
  - `resume NAME [--fresh] ...`: exit 0;
  - `compact-context NAME --json` and `clear-context NAME --json`: `submitted` receipts echoing the request;
  - `processes NAME --output ID ...`: `{id, run_id, conversation_id, output: "running 42 tests\n", truncated: false}`; `--stop ID`: `{id, run_id, conversation_id, status: "requested"}`;
  - `--help`: the watch-v1 marker line only;
  - anything else exits 2.

- [ ] **Step 2: The smoke hook.** In `PerfHost.tsx`, `?phase4-smoke=1` runs only when `config().actions_smoke` (replay, an explicit `ZERUS_CLIENT_HGS`, `ZERUS_CLIENT_PERF`), as the actions smoke does. It logs `phase4-smoke <step>: <outcome>` per step and exits 0, or 1 on the first failure. The steps are, on the replay:
  1. `terminal`: select `claude/sample-project/api-refactor`, open the Terminal tab, wait for "Connected: …", write `echo smoke\r`, and wait for `echo smoke` in the screen buffer.
  2. `drafts`: type "phase 4 smoke" into the composer, wait 300 ms, select another session and back, and check that the text returned.
  3. `attach`: drop `$ZERUS_CLIENT_SMOKE_FILE` (a synthetic file the run script writes) through `bridge.composerAttach`, and wait for "[File #1]".
  4. `send`: Send, wait for the local row "Submitted to terminal", and check that `log.ndjson` has one `send` call whose stdin names the file and whose argv has no message text (the hook reads the log through a `perf_log`-guarded command `smoke_log`, allowed only in a measurement run).
  5. `cold`: select `codex/sample-project/pagination`, Send, expect the "Continue with a cold cache?" dialog, choose "Send with full context", and expect a second `send` call.
  6. `start`: select the paused `claude/sample-project/review`, Send, expect "Start the session to send?", choose "Resume and send", and expect `resume` in the log, then the waiting text.
  7. `processes`: set `processesEnabled` (not persisted), open Processes, expect three groups, and wait for the output "running 42 tests".
  8. `preview`: hover the fixture message's `orders.json` chip and expect a ready preview.

  The command the hook reads the fake's log with, in `commands.rs` (live builds only, registered beside `perf_log`):

```rust
/// The fake `hgs` call log beside `ZERUS_CLIENT_HGS`, for the phase 4 smoke;
/// refused outside a smoke run, so a real `hgs` folder is never read.
#[cfg(feature = "live")]
#[tauri::command]
pub fn smoke_log() -> Result<Vec<serde_json::Value>, String> {
    if !crate::actions::smoke_env_allowed() {
        return Err("not a smoke run".into());
    }
    let hgs = std::path::PathBuf::from(std::env::var("ZERUS_CLIENT_HGS").map_err(|e| e.to_string())?);
    let log = hgs.with_file_name("log.ndjson");
    let text = std::fs::read_to_string(&log).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
}
```

  The README's run script writes the config with the fixture's inspect values and runs the release build under the headless environment, as the other smoke runs do.

- [ ] **Step 3: The docs.** `client/README.md` gains these sections, each with the rules a reader needs and the Qt files they follow:
  - "Terminal": lifecycle, acknowledgements, redraw, drops, statuses, Windows until T1b;
  - "Processes": settings, the topic, cadence, stops;
  - "Composer and Send":
    - the text protocol and the push envelope;
    - block reasons, the cold cache, resume and compact, local rows;
    - drafts: the app's one store at `<app data>/drafts` (`drafts-replay` in a replay), shared with voice; Qt's store read, never written; the one exit hook on quit and on SIGTERM;
  - "Attachments": limits, the `zerus-file` scheme, the cache bound and pins, Open and R2;
  - "Phase 4 smoke run": the commands, as in Step 2.

  Note in "Rules" that Part B adds voice and video, Native UI and the external entries.

- [ ] **Step 4: File the follow-ups.**
  - One Beads child of `zerus-5wmh` per composer part outside this phase (Decisions); each description names its Qt files and states that the control keeps its place disabled with its "Arrives with …" text until then:
    - model settings and the OpenCode picker;
    - file references;
    - Send and push now with the native queue;
    - suggestions;
    - the subagent composer;
    - question answers;
    - the Usage limit and Recovery chips.

    No paths outside the repository go into Beads text: no workspace or home paths.
  - Then:
    - `bd export -o .beads/issues.jsonl`;
    - `python3 scripts/check-source-privacy.py`;
    - `git add .beads/issues.jsonl`.

- [ ] **Step 5: Run everything**

```sh
cd client && npm ci && npm run check && npm test && python3 ../scripts/check-ui-separators.py
cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml
cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run build && RUSTUP_TOOLCHAIN=1.99.1 npx tauri build --no-bundle
```

Then the Part A smoke from the README under the headless environment only:

```sh
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...
```

Expected:
- every step logs `phase4-smoke ...: done` and the run exits 0;
- `log.ndjson` shows exactly one `send` per Send, with the message only in stdin.

Keep the raw log with its sha256 and the executed test counts for the handoff.

- [ ] **Step 6: Commit**

```bash
git add client .beads/issues.jsonl
git commit -m "test(client): the phase 4 Part A smoke run, fixtures and docs"
```

---

# Part B: after the bar

Part B starts once Part A is merged and the owner has the client for daily use. Its tasks are independent of each other.

### Task 13: Voice and round video: playback and recording

**Depends on:**
- **Playback:** Task 9 (`attachment_fetch`, pins, the `zerus-file` scheme with Range).
- **Recording:** M1c on main (`zerus-5wmh.9.12`, branch `work-ba0ed9fd-media`, head `1b14e68b`): `client/src-tauri/src/voice.rs` with `Voices` and the commands `voice_open`, `voice_input`, `voice_close` and `voice_microphones`, registered under the `media` feature; `client/core/src/voice_driver.rs` (`Out`, `Input`) and `voice_composer.rs` (`Shown`, `Ask`, `Target`, `Preferences`). R3 and R4 are delivered by it; this task adds no voice driver of its own.
- **Round video:** M3 for Linux (`zerus-5wmh.9.14`, branch `work-ba0ed9fd-video`, head `100e86bc`: the `frames` channel, `Out::Preview`, `voice_video`), M2 for macOS (`.9.13`) and M4 for Windows (`.9.15`), each per platform as it merges. Video shows only while `voice_video().available`; otherwise the mode switch stays hidden and voice works alone, with `reason` as the tooltip where Qt shows one.
- Task 3a (the composer hub, for transcript inserts) and Task 4b (local rows, for accepted voice messages).

**Qt reference:**
- `tray/src/ActivityView.cpp`:
  - the voice card and its state labels (978-1025);
  - `activateVoice`: one player, seek and pending seek (1818-1849);
  - the video note plays in place.
- `tray/src/SessionsWindow.cpp:1104-1108`: playback fetches the file.
- `tray/src/speech/SpeechComposer.cpp`: the record button, the mode switch locked while recording, preview before send, the provider choice, Send and Discard, the uncertain path, the permission recovery texts, and `speech/*` settings (`video_mode`, `preview_before_send`, `microphone`, `machine_id`, `profile`, `profile_revision`).
- `tray/src/MessageComposer.cpp:513-537`: `insertVoiceText`, which appends a transcript at the caret only to an unchanged draft and records its operation in `form_state.voice_insertions` (at most 32).
- `tray/src/speech/RoundVideo.cpp`: the round preview and the 59 s limit.
- `tray/src/speech/VoicePlayerImage.h`: the waveform, the times and the video poster.
- Spec lines 212-214.

**Files:**
- Create: `client/src/store/media.ts`, `client/src/store/media.test.ts`
- Create: `client/src/store/voice.ts`, `client/src/store/voice.test.ts`
- Create: `client/src/ui/sessions/Recorder.tsx`, mounted in the composer's voice slot
- Modify: `client/src/ui/sessions/Composer.tsx` (mount the recorder; Escape and Space stop a recording)
- Modify: `client/src/ui/sessions/Activity.tsx` (`MediaCard` plays the real file; the poster through the scheme)
- Modify: `client/src-tauri/src/composer/mod.rs` (`insert_voice_text`, `voice_message`, their commands and tests)
- Modify: `client/src-tauri/src/composer/files.rs` (`poster/{message id}` from the followed events)
- Modify: `client/src-tauri/src/lib.rs` (the two composer commands; the voice commands are M1c's)
- Modify: `client/src-tauri/Cargo.toml` (+ `base64 = "0.22"`, the core's version), `client/src-tauri/Cargo.lock`
- Modify: `client/src/store/settings.ts` (the `speech/*` keys), `client/src/types.ts`, `client/src/bridge.ts`

**Interfaces:**
- Consumes: M1c's commands and serde shapes as listed under "R3 and R4: delivered"; `voice_video() -> {available, reason}` (M3); Task 9's `attachment_fetch`, `attachment_pin` and `attachment_unpin`; S4's `LocalMessages::voice_message`.
- Produces:

```rust
// composer/mod.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")] pub enum Inserted { Refused, Saved, NotSaved }
impl Composers {
    /// `insertVoiceText`: `rev` is the composer's rev when Send or Check was
    /// pressed; a changed, busy or closed draft refuses. Blocking: answers
    /// once the durable save ends.
    pub fn insert_voice_text(&self, handle: u64, rev: u64, text: &str, operation: &str) -> Inserted;
    /// An accepted voice message (`Out::Message`), kept as a local row until the history shows it.
    pub fn voice_message(&self, key: &str, message: &Value);
}
// commands: composer_insert_voice(handle, rev, text, operation) -> Inserted; composer_voice_message(key, message)
// scheme: zerus-file://localhost/poster/{message id}: a voice message's poster JPEG from the followed session's held events
```

```ts
// store/media.ts
export interface MediaRef { computer: string; file: AttachmentRef; durationMs: number; video: boolean }
export function createMedia(port: MediaPort): { playing(): string | undefined; position(id: string): number; loading(id: string): boolean; error(id: string): string | undefined;
  toggle(id: string, ref: MediaRef): Promise<void>; seek(id: string, ref: MediaRef, fraction: number): void; ended(id: string): void; element(id: string, el: HTMLMediaElement | undefined): void; pauseAll(): void };
// store/voice.ts
export function createVoice(port: VoicePort): { shown(): Shown | undefined; level(): { rms: number; peak: number }; preview(): { url: string; mirrored: boolean } | undefined;
  ask(): Ask | undefined; video(): { available: boolean; reason: string | null }; open(key: string): Promise<void>; close(): Promise<void>; input(input: VoiceInput): void;
  answer(confirmed: boolean): void };
```

- [ ] **Step 1: Failing playback tests** in `client/src/store/media.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createMedia, type MediaPort, type MediaRef } from "./media";

const ref = (id: string): MediaRef => ({ computer: "", durationMs: 4000, video: false,
  file: { name: "claude/sample-project/review", request_id: id, index: 0, conversation_id: "conv-1", archive_id: null, agent_id: null } });

function fake() {
  const calls: string[] = [];
  const port: MediaPort = {
    fetch: async (_c, file) => { calls.push(`fetch ${file.request_id}`); return { path: `cache/${file.request_id}/voice.wav`, name: "voice.wav", mime: "audio/wav", bytes: 10 }; },
    pin: (p) => calls.push(`pin ${p}`), unpin: (p) => calls.push(`unpin ${p}`), url: (p) => `zerus-file://localhost/${p}`,
  };
  return { port, calls };
}

describe("media playback", () => {
  it("one recording plays at a time and the previous one is unpinned", async () => {
    const f = fake();
    await createRoot(async () => {
      const m = createMedia(f.port);
      await m.toggle("a", ref("a"));
      expect(m.playing()).toBe("a");
      await m.toggle("b", ref("b"));
      expect(m.playing()).toBe("b");
      expect(f.calls).toEqual(["fetch a", "pin cache/a/voice.wav", "unpin cache/a/voice.wav", "fetch b", "pin cache/b/voice.wav"]);
    });
  });

  it("a seek before the file loads is kept and applied once it plays", async () => {
    const f = fake();
    await createRoot(async () => {
      const m = createMedia(f.port);
      m.seek("a", ref("a"), 0.5);
      expect(m.position("a")).toBe(2000);
      await m.toggle("a", ref("a"));
      expect(m.position("a")).toBe(2000);
    });
  });
});
```

Run: `cd client && npm test -- src/store/media.test.ts` and see it fail.

- [ ] **Step 2: Write `client/src/store/media.ts`** (Qt `activateVoice`):
  - One player at a time; `pauseAll()` pauses it (the voice driver's `pause_messages`).
  - A play fetches the file with `attachment_fetch` (the voice event's `voice.audio`, or `voice.video` for a round video: `request_id` and `index`), pins it, and plays it through an `<audio>` or `<video>` element whose `src` is `convertFileSrc(path, "zerus-file")`. The bytes never cross the IPC as JSON; the element reads them through the scheme with Range requests.
  - A seek while loading is kept (`m_voicePendingSeek`); a play at the end starts from 0.
  - A playback error reads "Could not play the voice message." and fetches once more, as previews do.
  - Ending or switching unpins.

  `MediaCard` replaces its timer with the element's `timeupdate`, `ended` and `error` events through `media.element(id, el)`. The video note keeps its ring, its in-place play and the transcript button on the right. Its poster is `convertFileSrc("poster/" + message id, "zerus-file")`, an `<img>` whose `onError` hides it (the frames placeholder stays): the webview never builds a `data:` URL from base64.

- [ ] **Step 3: Posters and transcript inserts in Rust.** Failing tests in `composer/mod.rs`:

```rust
    #[test]
    fn a_transcript_goes_in_at_the_caret_once_and_only_into_an_unchanged_draft() {
        let root = dir("voice-insert");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, "before after".into(), 6, 6);
        assert_eq!(c.insert_voice_text(view.handle, 1, "spoken", "op-1"), Inserted::Saved);
        assert_eq!(c.current().unwrap().text, "before spoken after");
        assert_eq!(c.insert_voice_text(view.handle, 1, "spoken", "op-1"), Inserted::Saved, "the same operation is saved again, not inserted twice");
        assert_eq!(c.current().unwrap().text, "before spoken after");
        c.edit(view.handle, 2, "changed".into(), 7, 7);
        assert_eq!(c.insert_voice_text(view.handle, 1, "late", "op-2"), Inserted::Refused);
    }
```

  and in `composer/files.rs`, `a_poster_is_served_from_the_followed_events`: an `events` batch with `{"seq": 3, "source": "hgs_voice", "voice": {"id": "vm-1", "video": {"poster_base64": <base64 of a small JPEG>}}}` makes `poster/vm-1` answer 200 `image/jpeg` with the decoded bytes, `poster/vm-2` 404, and another webview 403.

  Implement:
  - `insert_voice_text` (`insertVoiceText`): refuses (`Refused`) unless `editable` gives the composer, its `rev` equals the rev given and `text` is nonempty; an operation already in `form_state.voice_insertions` only saves durably again; otherwise it inserts at `position` (UTF-16 units, converted as A1's `Draft::add` converts a `Selection`) without replacing a selection, adding a space where the neighbour is not whitespace, records the operation (at most 32, oldest dropped), bumps `text_epoch` (never `rev`, which counts the webview's edits), saves with the store's `save_durable` and waits on a channel for the result (`Saved` or `NotSaved`).
  - `voice_message(key, message)`: `hub.local.voice_message(key, message)` and `changed()`.
  - The scheme's `poster/{id}`: the held events of the followed session (Task 4b's `Followed.events`, at most 200) give the `voice.video.poster_base64` of the message with that `id`, decoded in Rust with `base64` (at most 256 KiB decoded) and served as `image/jpeg`; anything else answers 404.
  - The commands `composer_insert_voice(handle, rev, text, operation) -> Inserted` (on the blocking pool) and `composer_voice_message(key, message)`, registered in `lib.rs`.

- [ ] **Step 4: The voice store and the recorder.** Failing tests in `client/src/store/voice.test.ts`, with a fake `VoicePort` whose `open` hands the test the two channel callbacks:
  - "a shown update renders the controls": an `{kind: "shown", shown}` message sets `shown()`;
  - "frames become one object URL at a time": after `{kind: "preview", mirrored: true}`, each raw JPEG on the frames callback replaces `preview().url` and revokes the previous one, and `mirrored` is true;
  - "an insert answers with the composer's result": `{kind: "insert", token: 4, text: "spoken", id: "op-1"}` calls `composerInsertVoice(handle, revAtPress, "spoken", "op-1")` and then `input({kind: "inserted", token: 4, inserted: "saved"})`;
  - "video stays hidden while unavailable": with `voice_video` answering `{available: false, reason: "…"}`, `video().available` is false and an `update` input sends `preferences.video_mode: false`.

  Then write `client/src/store/voice.ts`:
  - `open(key)` calls `voice_open(key, events, frames)` with two `Channel`s: `events` gets `Out` as JSON, `frames` gets raw JPEG bodies (`ArrayBuffer`), turned into one `Blob` object URL at a time (the previous one revoked), so a frame never crosses as JSON. A refusal ("Voice in replay mode needs ZERUS_CLIENT_HGS.", or the 32-driver limit's text) becomes the voice button's tooltip and the button stays disabled.
  - `voice_video()` runs once per process; until it answers, the mode switch is hidden.
  - On every change of the selected session, the composer view or the settings: `input({kind: "update", target, input_visible, preferences})`, with `target` built from the open composer (key, host, session name, run, conversation, the cold cache warning, `available` and `reason` from the composer view, `draft_only: false`), and `preferences` from the `speech/*` settings (`video_mode` false while video is unavailable).
  - `Out` handling:
    - `shown`: the controls;
    - `level`: the meter (about 30 Hz);
    - `ask`: the cold cache or discard question, answered with `confirm {ticket, confirmed}`;
    - `message`: `composer_voice_message(key, message)`, so the card shows until the history has it;
    - `insert`: `composer.flush()` first (typing since the press then refuses the insert, as Qt does), then `composer_insert_voice` with the composer view's rev recorded when Send or Check was pressed, then `inserted {token, inserted}`;
    - `permission_recovery`: the microphone recovery dialog with Qt's text and a button opening the system's microphone settings;
    - `remember_profile`: stores `speech/machine_id`, `speech/profile` and `speech/profile_revision`;
    - `pause_messages`: `media.pauseAll()`;
    - `stop_playback`: stops the recording preview's playback.
  - `close()` calls `voice_close(key)` when the composer closes or another opens.

  `Recorder.tsx` fills the composer's voice slot from `shown()`:
  - the record chip (`voice_label`, `voice_icon`, `voice_danger`, enabled by `voice_enabled`) with `press`;
  - the mode switch (`mode_visible && video().available`, `mode_enabled`, `mode_icon`, `mode_label`), which flips `speech/video_mode`;
  - the hint and the elapsed time against the limit (120 s voice, 59 s video);
  - the level meter (a bar of `rms`, a tick at `peak`);
  - for video, the round preview from `preview()`, mirrored when it says so;
  - the preview row: Play (`playing {playing}`), Send, Discard, the provider choice (`choose_provider {index}`) and Refresh, from `shown`;
  - the notice chip (`notice_label`, `notice_detail`).

  Tooltips carry no key hints ("Stop recording", not "(Space / Esc)"); in `Composer.tsx`, Space and Escape still stop a recording (`stop {forget_send: false}`), and the composer hiding or the window losing focus sends `stop {forget_send: true}`.

- [ ] **Step 5: Run the checks**

Run: `cd client && npm test && npm run check && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: && cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets --features media -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/src client/src-tauri
git commit -m "feat(client): voice and round video playback and recording"
```

---

### Task 14: Native UI (DeepSeek)

**Depends on:** no core module. It needs Tauri's `unstable` feature (`Window::add_child`).

**Qt reference:**
- `tray/src/NativeUiView.cpp`: the whole file, for the statuses, the URL check, the browser per launch key and reload.
- `tray/plugins/NativeUiPlugin.cpp`: the main frame stays on origin, links open in the system browser, same-origin new windows open in place, downloads ask where to save, an off-the-record profile, and console output kept out of logs.
- `tray/src/HgsClient.cpp:1242-1260`: `openNativeUi` and `requestNativeUi`, 45 s here and 60 s on a peer.
- `tray/src/SessionsWindow.cpp`:
  - the tab and its visibility (1328-1333, 4012-4020);
  - `showNativeUi` (5506-5511), the sign-in banner (880-891);
  - the menu entry "Open native DeepSeek UI" (5295, 5466-5467);
  - "Respond in Activity" (5043).

**Files:**
- Modify: `client/src-tauri/Cargo.toml` (`tauri = { version = "2", features = ["unstable"] }`), `client/src-tauri/Cargo.lock`
- Create: `client/src-tauri/src/native_ui.rs`
- Modify: `client/src-tauri/src/lib.rs` (managed state, commands, the IPC guard)
- Create: `client/src/ui/sessions/NativeUi.tsx`, `client/src/store/native-ui.ts`
- Modify: `client/src/store/ui.ts` (count open overlays), `client/src/ui/sessions/SessionDetail.tsx` (the tab), `client/src/ui/sessions/SessionList.tsx` (the menu entry), `client/src/ui/sessions/Activity.tsx` (the sign-in banner), `client/src/types.ts`, `client/src/bridge.ts`
- Modify: `client/src/ui/PerfHost.tsx` and `client/src-tauri/tests/support/fake_hgs_phase4.py` (the smoke step `native`)

**Interfaces:**
- Produces:

```rust
pub fn launch_url(address: &str) -> Option<tauri::Url>;   // Qt's ^http://127\.0\.0\.1:[0-9]+/\?token=[A-Za-z0-9_%.-]+$, port 1-65535
pub fn launch_key(host: &str, url: &tauri::Url) -> String; // host \n origin \n sha256(url)
/// Every app command is refused for webviews other than the main one.
pub fn allowed(webview_label: &str) -> bool;
// commands: native_ui_open(host, name, reload) -> Result<NativeStatus, String>; native_ui_bounds(x, y, width, height);
// native_ui_visible(visible); native_ui_browser(host, name) -> Result<(), String>
// event "native-ui": {key, status: "loading" | "ready" | "failed" | "stopped"}
```

- [ ] **Step 1: Failing tests** in `native_ui.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_qts_loopback_launch_addresses_load() {
        assert!(launch_url(concat!("http://127.0.0.1:43210/?tok", "en=abc_DEF-1.2%3D")).is_some());
        for bad in ["http://127.0.0.1:0/?token=a", "https://127.0.0.1:4321/?token=a", "http://localhost:4321/?token=a",
                    "http://127.0.0.1:4321/x?token=a", "http://127.0.0.1:4321/?token=a b", "http://127.0.0.1:70000/?token=a"] {
            assert!(launch_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn launch_keys_never_revive_another_generation() {
        let a = launch_url("http://127.0.0.1:43210/?token=one").unwrap();
        let b = launch_url("http://127.0.0.1:43210/?token=two").unwrap();
        assert_ne!(launch_key("", &a), launch_key("", &b));
        assert_ne!(launch_key("", &a), launch_key("studio", &a));
        assert!(!launch_key("", &a).contains("one"), "only a fingerprint of the credential is kept");
    }

    #[test]
    fn only_the_main_webview_may_call_commands() {
        assert!(allowed("main"));
        assert!(!allowed("native-ui-1"));
    }
}
```

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml native_ui::` and see it fail.

- [ ] **Step 2: Write `native_ui.rs`**

```rust
//! DeepSeek's native workspace in a child webview, ported from
//! `tray/src/NativeUiView.cpp` and `tray/plugins/NativeUiPlugin.cpp`.
//! `hgs [@host] native-ui NAME --json` returns a loopback launch address;
//! a child webview of the main window loads it. It is incognito, its main
//! frame stays on that origin, and http(s) links open in the system
//! browser. One child per launch key (host, origin, a fingerprint of the
//! address), at most four, the least recently shown closed first. Without a
//! child webview the browser opens through `hgs native-ui NAME`.

use sha2::{Digest, Sha256};

pub fn launch_url(address: &str) -> Option<tauri::Url> {
    let rest = address.strip_prefix("http://127.0.0.1:")?;
    let (port, token) = rest.split_once("/?token=")?;
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port: u32 = port.parse().ok()?;
    let token_ok = !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || "_%.-".contains(c));
    if !(1..=65535).contains(&port) || !token_ok {
        return None;
    }
    tauri::Url::parse(address).ok()
}

pub fn launch_key(host: &str, url: &tauri::Url) -> String {
    let origin = url.origin().ascii_serialization();
    let digest: String = Sha256::digest(url.as_str().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("{host}\n{origin}\n{digest}")
}

pub fn allowed(webview_label: &str) -> bool {
    webview_label == crate::MAIN_WINDOW
}
```

The managed `NativeUi`:
- **Fetch:** `native_ui_open(host, name, reload)` runs `hgs [@host] native-ui NAME --json` (45 s here, 60 s on a peer) on the blocking pool and checks the `url` with `launch_url`.
  - An invalid reply gives "Invalid native UI connection response", or "The native UI returned an invalid connection address.".
  - A failure gives "Could not open native DeepSeek UI. {error}".
- **The child:** it is looked up by `launch_key`, or created with:

```rust
let origin = url.origin().ascii_serialization();
let builder = tauri::webview::WebviewBuilder::new(label.clone(), tauri::WebviewUrl::External(url.clone()))
    .incognito(true)
    .on_navigation({ let origin = origin.clone(); let app = app.clone(); move |next| {
        if next.as_str() == "about:blank" || next.origin().ascii_serialization() == origin { return true; }
        if matches!(next.scheme(), "http" | "https") { let _ = tauri_plugin_opener::OpenerExt::opener(&app).open_url(next.as_str(), None::<&str>); }
        false
    }})
    .on_new_window({ let (origin, app, me) = (origin.clone(), app.clone(), Arc::clone(&this_child)); move |next, _| {
        // Same origin loads in place; other web links open in the system browser.
        if next.origin().ascii_serialization() == origin {
            if let Some(child) = lock(&me).as_ref() { let _ = child.navigate(next); }
        } else if matches!(next.scheme(), "http" | "https") {
            let _ = tauri_plugin_opener::OpenerExt::opener(&app).open_url(next.as_str(), None::<&str>);
        }
        tauri::webview::NewWindowResponse::Deny
    }})
    .on_page_load(move |webview, payload| { /* emit "native-ui" {key, status: loading | ready} */ });
let child = main_window.as_ref().window().add_child(builder, bounds.position(), bounds.size())?;
*lock(&this_child) = Some(child.clone());
```

  `this_child` is an `Arc<Mutex<Option<tauri::Webview>>>` made before the builder. A same-origin new window loads in the child itself, and every new window is then denied as a window. Downloads go through `on_download`, which asks where to save with `tauri-plugin-dialog`, as Qt's `downloadRequested` does.
- **Placement:** `native_ui_bounds` moves and sizes the visible child to the tab's rectangle (logical pixels), and `native_ui_visible(false)` hides it (for other tabs and while an overlay is open).
- **Lifetime:** a fifth key closes the least recently shown child, and a session change hides the current one. A reload loads the URL again in the same child.
- **Fallback:** if `add_child` fails, the reply is `{status: "browser"}`; the view says "The embedded native UI is not installed. Open it in your browser." and offers `native_ui_browser`, which runs `hgs [@host] native-ui NAME` (45 s here, 60 s on a peer).

The IPC guard, in `lib.rs`:

```rust
fn guarded(inner: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        if !native_ui::allowed(invoke.message.webview().label()) {
            invoke.resolver.reject("not allowed");
            return true;
        }
        inner(invoke)
    }
}
```

`run()` uses `.invoke_handler(guarded(handler()))`. The `zerus-file` scheme already refuses other webviews (Task 3b).

- [ ] **Step 3: The tab.**
  - `NativeUi.tsx` shows for a `dsh` session when no subagent is open. Its toolbar holds the title "DeepSeek workspace / {machine}", "Respond in Activity" (shown while the session has a question or approval pending, switching to Activity) and Reload ("Reload native UI").
  - Status texts:
    - "Open the native workspace to manage DeepSeek sessions and settings.";
    - "Native UI is unavailable while this machine is offline.";
    - "Connecting to native DeepSeek UI…";
    - "Loading native DeepSeek UI…";
    - "Native UI could not load. Use Reload to reconnect, or open it in your browser.";
    - "The native UI renderer stopped. Use Reload to reopen it.".
  - A placeholder `div` fills the tab. A `ResizeObserver` and the window's resize send its rectangle through `native_ui_bounds`, once per animation frame at most.
  - `store/ui.ts` counts open overlays (the menu, every dialog); while the count is above zero or the tab is hidden, `native_ui_visible(false)`.
  - The session menu's "Open native DeepSeek UI" for `dsh` rows, Inspect's `native_ui` route (Task 7), and the "Sign in to DeepSeek…" banner (shown when `auth_required` on a live `dsh` session, with Qt's text "Sign in to DeepSeek to start using this session. Complete sign-in in the official DeepSeek UI, then return here.") all open this tab.

- [ ] **Step 4: Run the checks and a headless look**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml native_ui:: && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS.

Then the headless smoke step `native` (Task 12's hook gains it): select `deepseek/docs-site/examples`, with the fake's `native-ui NAME --json` answering `http://127.0.0.1:<a local http.server port>/?token=fixture`, served by `python3 -m http.server` on a scratch folder with a synthetic `index.html`. Expect the `ready` status, and expect `window.__TAURI_INTERNALS__.invoke("metrics")` from that page to be refused ("not allowed").

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri client/src
git commit -m "feat(client): DeepSeek's Native UI in an isolated child webview"
```

---

### Task 15: Open terminal, Open shell in session folder, Open folder and Open externally

**Depends on:** R5 for Open terminal, Open shell and Open externally. Open folder needs no core (the opener plugin, Task 9).

**Qt reference:**
- `tray/src/SessionsWindow.cpp`:
  - the menu entries (5295-5301);
  - `runSessionMenuAction`'s OpenFolder, FolderShell and OpenTerminal (5435-5477);
  - `openSession` and `m_openAction`'s caption (5520-5531, 3893-3895);
  - `confirmLocalStart` (5533-5540).
- `tray/src/TrayAgent.cpp:725-760`: the folder shell and the terminal backends.

**Files:**
- Create: `client/src-tauri/src/launch.rs` (commands over R5)
- Modify: `client/src-tauri/src/lib.rs` (the module and its four commands)
- Modify: `client/src/ui/sessions/SessionList.tsx` (the context menu entries), `client/src/ui/sessions/SessionDetail.tsx` (the header's Open folder, Open shell and Open externally), `client/src/bridge.ts`

**Interfaces:**
- Produces: commands `open_terminal(computer, name)`, `open_folder_shell(computer, directory)`, `open_folder(directory)` and `open_externally(computer, name)`, each `-> Result<Option<String /*advisory*/>, String>`.

- [ ] **Step 1: Failing tests** in `launch.rs`, for the argument shapes Qt passes:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_are_qts() {
        assert_eq!(terminal_args("", "claude/sample-project/review"), ["a", "claude/sample-project/review"]);
        assert_eq!(terminal_args("studio", "claude/sample-project/review"), ["@studio", "a", "claude/sample-project/review"]);
        assert_eq!(shell_args("studio", "/home/example/src/sample-project"), ["machine", "ssh", "studio", "--directory", "/home/example/src/sample-project"]);
        assert!(folder_allowed("", "/home/example/src/sample-project"));
        assert!(!folder_allowed("studio", "/home/example/src/sample-project"), "Open folder is for this computer only");
        assert!(!folder_allowed("", "relative/path"));
    }
}
```

Run, see it fail, then implement:

```rust
pub fn terminal_args(computer: &str, name: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !computer.is_empty() { args.push(format!("@{computer}")); }
    args.extend(["a".to_owned(), name.to_owned()]);
    args
}

pub fn shell_args(computer: &str, directory: &str) -> Vec<String> {
    ["machine", "ssh", computer, "--directory", directory].map(str::to_owned).to_vec()
}

pub fn folder_allowed(computer: &str, directory: &str) -> bool {
    computer.is_empty() && std::path::Path::new(directory).is_absolute()
}
```

The commands call R5's `launch::open_program(default_hgs(), args)` (Open terminal and Open externally) and `launch::open_shell(directory)` for this computer, or `open_program(hgs, shell_args)` for a peer. `open_folder` calls `app.opener().open_path(directory, None::<&str>)`.

A failure answers Qt's notice titles: "Could not open folder shell", "Could not open the terminal" and "Could not open the folder", each with the error. A backend advisory (R5's `Option<String>`) shows as an info notice.

- [ ] **Step 2: The entries.**
  - **Open terminal:** the context menu entry is enabled for an online, non-archived session whose terminal is supported (`terminal_status`, Task 2). A `dsh` session's entry reads "Open native DeepSeek UI" and opens Task 14's tab. A stopped session asks Qt's local-start confirmation only on Windows hosts (none in this client yet), so it runs directly.
  - **Open folder:** for this computer's sessions with an absolute folder.
  - **Open shell in session folder:** for online sessions with an absolute folder.
  - **The header's `m_openAction`:** "Open externally", or "Open native DeepSeek UI in browser" for `dsh` (`native_ui_browser`). It is enabled under Qt's condition: online, not updating, not archived, and DeepSeek or a terminal.

  The phase 1 disabled header buttons for Open folder and Open shell become these entries.

- [ ] **Step 3: Run the checks**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml launch:: && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/launch.rs client/src-tauri/src/lib.rs client/src
git commit -m "feat(client): open terminal, folder shell, folder and externally"
```

---

## Self-review notes

- **Spec coverage:**
  - the PTY and xterm.js: Tasks 1-2;
  - attachments, round 5 and round 6: Tasks 3a, 3b, 6, 8 and 9;
  - Send's rules: Tasks 4a, 4b and 5;
  - drafts: Tasks 3a and 7;
  - Processes: Tasks 10-11;
  - media lines 212-214 and row 296: Task 13;
  - Native UI, row 302: Task 14;
  - opening files and pickers, row 301: Tasks 3a, 9 and 15;
  - terminal launch, row 294: Task 15 (R5).

  The epic's menu entries (Open terminal, Open native DeepSeek UI, Open folder, Open shell in session folder, Show running commands) are in Tasks 11, 14 and 15.
- **Review Focus:** each line names its owning test above.
- **Not in this plan:** the composer parts listed in Decisions, filed by Task 12; Windows ConPTY (T1b, `zerus-5wmh.9.16`); macOS microphone and camera entitlements (M1 and M2, with the coordinator); the voice driver itself (M1c, the events session's).
- **Review of 2026-10-28** (the coordinator's amendments and a review pass): one draft store and one exit hook shared with M1c and the organization branch; R3 and R4 delivered and Task 13 rebound to them; Qt's stores read-only with a byte-for-byte test; the unknown-delivery rules made explicit and tested (stale questions, exit while saving); no digests per view; rename collisions keep both drafts; Tasks 3 and 4 split; each task adds only what it uses.
