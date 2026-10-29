# Tauri Client Phase 4: Composer, Attachments, Media, Terminal, Processes and Native UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client gets the session Terminal, the Processes tab, the full composer with Qt's guarded Send (block reasons, the cold-cache question, resume-then-send, compact-to-send, local failed messages, durable drafts), attachments, voice and round video, DeepSeek's Native UI, agent question cards and approvals, and the agent's native queue with Send now and Send and push now, with the Qt GUI's exact behaviour (Beads `zerus-5wmh.9`).

**Two parts (owner decision, 2026-10-28).** The new client replaces the Qt app on the owner's Mac as soon as it is workable for daily use. **Part A is that bar** for this phase: the Terminal, the composer and Send (block reasons, the cold cache, resume-then-send and compact-to-send, local messages, durable drafts), attachments (stage, send, fetch, preview) and Processes. **Part A2** joins the bar by the coordinator's ruling of 2026-10-28: question answers (`zerus-5wmh.9.23`) and sending while the agent works (`zerus-5wmh.9.24`). New session (phase 5, `zerus-5wmh.10`) completes the bar. **Part B** follows the bar and is fully planned here: voice and round video, Native UI, and the external terminal, shell and folder entries. Within Part A the tasks follow the core: first the merged Terminal core, then in the order the core PRs land (#403, #404, #407, #408, #409, #410, #411).

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
- **One push path.** `next_batch` keeps returning the core's `Batch` as raw JSON, flattened into an envelope that adds the projected `composer` view, the `processes_view` and the `draft_keys`. Rust recomputes them when the followed Activity's details or a fact change, and once a second for the cache chip (Qt's chip refresh), pushing only a view that changed. The webview never calls a rule per render.
- **Native UI** is a child webview at the tunnel URL from `hgs native-ui NAME --json`, origin-locked and incognito; without one, the system browser through `hgs native-ui NAME`.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2.12 with the `unstable` feature for child webviews), `tauri-plugin-dialog` 2, `tauri-plugin-opener` 2, `chrono` 0.4, `sha2` 0.10; SolidJS 1.9 + TypeScript + Vite, `@xterm/xterm` 6.0.0, `@xterm/addon-fit` 0.11.0, `@xterm/addon-webgl` 0.19.0, vitest; Python 3 stdlib for fixtures and fake `hgs`.

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
- **R7 and R8: question cards and the native queue.** See Part A2, "Requests to the core owner: R7 and R8". R7 covers the cards, their drafts and the guarded `hgs answer` (`zerus-5wmh.9.23`). R8 covers the native queue, Send now, the push and OpenCode's inbox (`zerus-5wmh.9.24`). Tasks A2-1 to A2-6 start once they merge. The bar needs both, so no control ships disabled waiting for them.

## Global Constraints

- **Client private paths come from one function.** New code never calls `app.path().app_data_dir()`. In the Mac switch build that reports the compile-time identifier `com.hgdev.hgs-tray.local`, so the updater would check an empty drafts root.
  - Every private path takes its root from `crate::data_dir(app)`, a single function in `lib.rs` that returns `app_data_dir()` today. This includes the draft store root (`<data dir>/drafts`, or `drafts-replay` in replay), read marks and any later file.
  - The Mac switch plan (docs/superpowers/plans/2026-10-28-tauri-client-mac-switch.md, Task 2) replaces that function's body with `identity::Identity::data_dir_now()`. Its Task 9 checks that `grep -rn "app_data_dir()" client/src-tauri/src` prints only that one function.
  - The attachment cache may stay on `app_cache_dir`; it is disposable.

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
  - the composer hub keeps at most 500 events of the followed session (Qt's `m_events`, `SessionsWindow.cpp:3651`), and at most 64 composers and 128 MiB of their attachments in memory;
  - process output as `hgs` bounds it.
- All `hgs` calls, file reads and draft writes run off the UI thread (Tauri's blocking pool or the hubs' own threads).
- The composer's block reason is computed in Rust whenever the followed Activity's details or a fact change and is pushed with the Activity update; the composer's session part, the cache chip included, is recomputed once a second and pushed only when it changed; `local_time` is formatted in src-tauri.
- Terminal output goes as `InvokeResponseBody::Raw`, the finish as a small JSON message on the same Channel; the `AttachEvent` receiver is drained on a std thread; an attach stops on close or when `terminal::target_changed`.
- **Lanes** (`client/src-tauri/src/actions.rs`):
  - `Lane::Send` is keyed by (computer, name, agent_id);
  - `Lane::Process` is for `StopProcess`;
  - `ClearContext`, `CompactContext` and `StartFresh` run on `Lane::Write`;
  - `Lane::Answer` (R7) is keyed by (computer, name): one answer per session, as Qt's `m_answersInFlight`;
  - `Lane::Queue` (`SendNow`) and `Lane::Inbox` (`RemoveQueued`) are R8's. The hub also keeps Qt's one send-now and one removal in the whole app.
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
7. **A question card that changes while it is answered.** The hash changes, the card is answered in Terminal or on the phone, or the delivery is unknown.
   - Nothing is sent for a changed card: the card reads "The question changed. Your answers were kept without submitting.".
   - A card answered elsewhere goes away and the composer returns.
   - An uncertain answer reads "Delivery is not confirmed. Check Terminal before retrying. …". It sends nothing again until Allow retry: not after a new state, a reload or a restart. A draft saved while sending reloads as interrupted.
   - A retry is a new request id.

   Tests: A2-1 (`a_sending_answer_reloads_uncertain`, `a_card_answered_elsewhere_goes_away`), A2-2 (`a_card_changed_during_the_save_is_never_sent`, `an_uncertain_answer_waits_for_allow_retry`).
8. **A broader approval option.** The first Submit only arms Confirm, and the second one sends. Holding Enter never confirms. Approve once waits out the 3 s review while the card is visible and the window active; Deny never waits.

   Tests: R7 (`a_broader_option_needs_a_second_confirm`), A2-2 (`a_broader_choice_sends_only_after_confirm`), A2-3 ("holding Enter never submits again", "the review delay counts only while the card is active").
9. **A push never resends text.** Send and push now sends the message once, then only the agent's own send-now of the snapshot that holds it. It ends with Qt's notice when another session opens, after 20 s, or when the send fails or is uncertain.

   Tests: R8 (`a_push_sends_only_the_snapshot_that_holds_the_message`, `the_twenty_second_endings`), A2-4 (`a_push_sends_the_queue_once_the_agent_queues_it`, `selecting_another_session_ends_the_push`, `a_push_ends_after_twenty_seconds`).

## Decisions

- **Rulings on the plan review (2026-10-28).**
  - Voice-event posters: `hgs` puts `voice.video.poster_base64` into the voice events it publishes, so the Activity batch carries one small JPEG per video message as JSON. Accepted until the core strips it from batches and offers a poster lookup (request R6, a Part B follow-up). Task 13 never decodes it in the webview; posters are served through `zerus-file`.
  - On macOS under `com.hgdev.hgs-tray`, Cocoa writes its own keys (for example the file picker's last folder) into `com.hgdev.hgs-tray.plist`, which is Qt's settings file. Qt's own dialogs write the same keys, and no Qt key is changed. Accepted. The switch slice (zerus-5wmh.12.2) checks that Qt's keys stay byte-for-byte unchanged.
  - The spec's line "a distinct bundle identifier from the Qt app" predates the identity decision of 2026-10-28. It is corrected in the switch slice's PR, not here.
  - The client's own data (read marks, cache, local settings) moving with the identifier is decided by the Mac switch plan, not by this plan.
  - Part A lands as five PRs: the Terminal (Tasks 1-2), the composer and send (3a-7), attachments (8-9), Processes (10-11), then the smoke run and docs (12). Part A2 adds three:
    - the question cards (A2-1 to A2-3);
    - the queue and the push (A2-4 and A2-5);
    - OpenCode's inbox (A2-6).

Rulings on the open questions; the tasks follow them.

- **One draft store, one exit hook (agreed with the events session, 2026-10-28).** M1c opens the app's one `Arc<DraftStore>` in `setup` and manages it as `pub struct Drafts(pub Arc<DraftStore>)` in `lib.rs`, at `zerus_client_core::drafts::dir(replay)` (`drafts`, or `drafts-replay` in a replay). `Composers::new` takes that store and never opens its own, so each store has one writer. M1c turns `run()` into `.build(...).expect(...).run(...)` with one `fn exit(app)` on `RunEvent::Exit`: `voices.close_all()`, then the organization's `flush()` (the org branch), then `composers.flush()`, then `Drafts.0.flush()`, each through `try_state`. Task 3a works whichever lands first: with M1c on main it uses M1c's `Drafts` and `exit`; without, it adds exactly the same struct, folder names and hook, and M1c rebases onto them. Whoever lands second adds lines, never a second store or hook.
- **Qt's stores are read, never written (owner decision, 2026-10-28).** The client takes over Qt's identity on the Mac (`com.hgdev.hgs-tray`), and Qt's drafts are kept as they are. Qt's drafts root is read through `read_index`, `read_draft` and `DraftStore::recover` only; recovery copies into the client's store; a Qt root that is, holds or sits inside the client's root is left out. The one-time import of all Qt drafts at the switch (`zerus-5wmh.13`, core section S5) is a copy too.
- **Saved drafts lists two sources.** The client's own store comes first, under "On this computer"; Qt's read-only store follows under "From the previous Zerus app" (it stays right after the client takes over the Zerus name; the owner knows the Qt GUI as Zerus). Each is paged by its own cursor ("Show more"), since the stores cannot share one cursor.
  - A Qt draft is copied into the client's store with `DraftStore::recover` before Restore or Go to session. An existing nonempty draft there is never overwritten (S5's `ALREADY_EXISTS` text).
  - Qt's dialog texts, buttons and the "Replace current draft?" confirmation carry over.
- **The exit closes the hub before the store flushes.** `Composers::flush` stops new edits, attaches and sends and waits up to 2 s for attach reads under way; `Drafts.0.flush()` then writes everything queued. A send whose "sending" save is still queued never starts and reloads as uncertain.
- **Draft attachment URLs carry no digest.** `AttachmentView.path` is `draft/{handle}/{list}/{index}`, where `list` changes with every change of the attachment list; the scheme answers it from memory without hashing, and an old URL answers 404. S3's draft marks hash an attachment list once (Task 5), not per check.
- **Renames lock until the store answers** (Qt's `renameDraft`). A refused rename (`ALREADY_EXISTS`) keeps the moved draft under its old key, marked with the refusal as its storage error, and loads the draft already at the new key with no message (`MessageComposer.cpp:795-798`), so neither is overwritten. A newer composer of the old key (a rename back while the first is pending) is never replaced.
- **Tasks 3 and 4 are split** for review: 3a (the hub, drafts and attachments), 3b (the `zerus-file` scheme and the cache), 4a (the session's block reasons, cache chip and inspect fallback), 4b (the delivery and local rows).
- **Processes are off by default, as in Qt.** This phase adds the Settings > Processes section the client's Settings screen lacks: Qt's five controls and hint (`tray/src/SettingsPage.h:109-120`), stored under the QSettings keys `processes/enabled`, `processes/activeSeconds`, `processes/backgroundSeconds`, `processes/unknownSeconds` and `processes/loadingSeconds`. This follows the screen's pattern ("Settings, as far as the client has them"); phase 3b keeps the section when it builds the full screen.
- **Composer parts outside this phase.** Task 12 files a follow-up child of `zerus-5wmh` for each, and each keeps its Qt place, disabled with "Arrives with …":
  - the model settings popup and the OpenCode model picker ("model settings");
  - file references ("file references");
  - suggestions;
  - the subagent composer (the subagent view stays read only);
  - the Usage limit and Recovery chips ("account usage" and "automatic recovery").
- **Part A2 (coordinator ruling, 2026-10-28).** Question answers and sending while the agent works join the bar: R7 and R8 for the core owner, and Tasks A2-1 to A2-6 here.
  - **Send needs no new rule.** S1's block reasons already let a busy agent take messages, and the agent's TUI queues them. A2 adds Qt's "Queued in agent" panel with Send now, its window chord, and Send and push now.
  - **The chords.** Send and push now is a composer chord only, as in Qt:
    - Control-X Control-S on macOS (Qt's Meta+X, Meta+S);
    - Ctrl+X Ctrl+S elsewhere, where Ctrl+X with a selection stays Cut.

    Send now is the panel's button and Command+Shift+Return (Ctrl+Shift+Return elsewhere). No tooltip names a chord (owner rule).
  - **No edit, reorder or remove of a native queue.** Qt has none, and the CLI marks Codex's ordinary follow-ups read-only. Remove exists only for OpenCode's inbox (A2-6), which lands after the switch unless the owner rules otherwise.
  - **Every A2 action is built in Rust from held state** and is `skip_deserializing`:
    - `Answer` from the shown card's raw entry and its held draft;
    - `SendNow` from the followed details' queue id;
    - `RemoveQueued` after the confirmation's re-check.
  - **The review delay** (3 s) runs in the webview store, which knows the card's visibility and the window's focus. The card's view says which buttons it gates. Rust does not time it: Qt's gate is the button state too.
  - **Question drafts** use the app's one draft store with Qt's keys (`question\n…`) and `form_state`, so Saved drafts and the switch's import see them.
  - **Later notices.** Rust's notices that arrive after the call (push endings, send-now results, answer follow-ups) and the Terminal tab a Codex update opens reach the webview as `composer_events` in the envelope, at most 16 per batch.
  - **The question read error.** Qt builds "Could not load agent questions: … Check Terminal." into the session hint (`SessionsWindow.cpp:3913-3918`), hides the hint banner (`m_hint->hide()`, 3953) and shows the hint only as the composer's session status (3984-3986). The client has that one path: `send::HintFacts.question_error` puts it into `session_status`, in the error tone and locking input while a block reason stands (A2-1 ruling, 2026-10-29).
  - **Naming.** Task 4b's "questions" are the hub's own dialogs (cold cache, start). The agent's requests are "question cards" (`questions.rs`, `QuestionReply`).
- **The push envelope.** `next_batch` serializes `{...batch, composer?, processes_view?, draft_keys?}`. Only the open composer and the followed processes are projected; `draft_keys` lists the sessions with a nonempty draft among the 200 newest drafts plus the composers in memory, so the cards' draft marks stay bounded. Part A2 adds `composer_events?`, and the composer view carries the question card, the queue panel and the inbox state.
- **Editing protocol.**
  - The webview owns the text while the user types and flushes `composer_edit(handle, rev, epoch, text, position, anchor)` 250 ms after the last keystroke, on blur and before any attach, paste, remove or send. `epoch` is the `text_epoch` the text is based on. Rust applies only a higher `rev` of the current epoch.
  - Rust-originated text changes bump `text_epoch`; the webview takes `text` when the epoch grows: a load, an attach or remove, an Undo, a delivery's end, a restore.
  - A change the composer cannot take hands the text back under a new epoch (Qt's `restoreDraft`): an edit while a rename locks it, a refused remove or Undo.
  - An attach or paste locks the editor read-only until it returns. A remove or Undo locks it until a view with a higher epoch arrives; Rust always answers them with one.
- Ruling (Task 3a review): composer_edit carries the text epoch; Rust ignores an edit whose epoch differs, and a busy refusal bumps the epoch to hand the text back — stale edits after remove/Undo/rename otherwise resurrect or drop file mentions — one more argument
- Ruling (Task 3a review): refused-rename error placement follows Qt — Qt wins — none
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
- **Rulings on the Task 4a review (2026-10-28).** The reviewer confirmed each against Qt.
  - **The composer's session status (ruling 1).** Rust computes Qt's `setSessionStatus(contextVisible ? hint : "", contextError, contextError && !messageProblem.isEmpty())` (`SessionsWindow.cpp:3984-3986`) and applies it in the view: the status line falls back to it before the block reason (`MessageComposer.cpp:1204`), it is an error when Qt says so (1225), and its lock joins `can_send`'s lock (1149, 1186) and refuses attach, paste, remove and Undo (`canEditAttachments`, `MessageComposer.h:108`). The hint is the first six links of Qt's chain (3917-3922): terminating, offline, "Last restore failed: %1", `m_inspectError` ("Could not refresh activity: %1"), the question error, archived and occupied. A2 fills the question error and Piece E the restore error and occupied, through `send::HintFacts`. Task 6 renders it: the read-only editor with the status as its tooltip, and Refresh (311, 1194).
  - **One window-wide update block (ruling 2).** Qt's `m_pending` is one bool (`SessionsWindow.h:554`) that every session's or computer's write sets (2876, 2881, 4699, 4758, 5554, 5575, 5693, 5752, 5808, 5815) and `messageBlockReason` reads without a key (4394). So the open session's own Terminate or Kill, running or retained, is a termination, and any other message-blocking write anywhere is "Wait for the session update to finish.". A context reset, a compaction and a Terminal file drop are not among them: Qt keys `m_clearKey` and `m_compacts` per session and sets no `m_pending` for them (4668, 4544, 2657).
  - **Write marks wake the composer at once (ruling 3).** `Actions::on_writes` takes a hook, set in `setup` to `Composers::changed_hook()`; a mark fires it as it appears and as it goes, after the marks' lock is released. The hook is `changed()`: a counter and a pump poke, no hub lock.
  - **A retained termination compares only the run id (ruling 4).** hgs mints a fresh UUID v4 for every start, resume (`src/state/lifecycle.rs:763`), restore (660), fresh start (808) and dsh resume (`src/state/dsh.rs:1271`), so a run id names one run; Qt's `created` comparison adds nothing. A session without a run id (untracked) is never held.
  - **The plan follows the code (ruling 5).** Task 4a's "As built" lists what changed from its first text for Tasks 4b, 5 and 6.

---

## File Structure

```
client/src-tauri/Cargo.toml              + tauri "unstable", tauri-plugin-dialog, tauri-plugin-opener, chrono, sha2
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
client/src-tauri/src/composer/questions.rs  question cards: answer drafts, the shown card, the guarded answer (A2-1, A2-2)
client/src-tauri/src/composer/queue.rs  the native queue, Send now, Send and push now, OpenCode's inbox (A2-4, A2-6)
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
client/src/store/questions.ts            the question card: review delay, replies, cold cache dialog (+ .test.ts)
client/src/store/settings.ts             processes/* keys
client/src/store/session.ts              routes the envelope to the stores
client/src/store/ui.ts                   detailTab, overlay count
client/src/ui/sessions/Terminal.tsx      xterm.js tab (lazy), statuses, buttons, drops
client/src/ui/sessions/Processes.tsx     the Processes tab
client/src/ui/sessions/Composer.tsx      editor, toolbar chips, action row, height grip
client/src/ui/sessions/Attachments.tsx   the composer's attachment list (mockup's AttachmentsChip)
client/src/ui/sessions/ComposerDialogs.tsx   cold cache, start, clear, replace draft
client/src/ui/sessions/SavedDrafts.tsx   the Saved drafts dialog
client/src/ui/sessions/QuestionCard.tsx  the question card and its cold cache dialog
client/src/ui/sessions/QueuePanel.tsx    "Queued in agent" with Send now
client/src/ui/sessions/chords.ts         the Send and push now and Send now chords (+ .test.ts)
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
| A2-1 | Question cards in the hub: drafts, the shown card, its view | R7 (`zerus-5wmh.9.23`); Tasks 3a, 4a and 4b |
| A2-2 | The guarded answer: Submit, Approve once, Deny, Skip, the cold cache | A2-1 |
| A2-3 | The question card view, answers in Activity | A2-2; Tasks 2 and 6 |
| A2-4 | The native queue, Send now and Send and push now in the hub | R8 (`zerus-5wmh.9.24`); Tasks 4b and 5; A2-2's events (or its Step 3) |
| A2-5 | The queue panel, the push chord, "You (queued)" | A2-4; Task 6 |
| A2-6 | OpenCode's inbox: Queued, Not delivered, Remove (after the switch unless ruled onto the bar) | R8; A2-4; Task 7 |
| 8 | Dropped files: composer attach, Terminal paste and stage | A2 #404 (StageFiles), #406 (`Session.created`), Tasks 2, 3a and 6 |
| 9 | Activity attachments: fetch, preview, open | A3 `zerus-5wmh.9.5` (#410), `zerus-5wmh.9.17`, Tasks 3b and 4b; Open also R2 |
| 10 | Processes hub | P1 `zerus-5wmh.9.6` (#411, stacked on A2 #404) |
| 11 | Processes tab and Settings > Processes | Task 10 |
| 12 | Fixtures, the Part A smoke run, docs and follow-ups | Tasks 1-11 (3a, 3b, 4a and 4b included) and A2-1 to A2-5 |

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
- Create: `client/src-tauri/src/signals.rs`, verbatim from the Mac switch plan (`2026-10-28-tauri-client-mac-switch.md`, Task 3), so both branches carry the same file
- Modify: `client/src-tauri/Cargo.toml` (+ `tauri-plugin-dialog = "2"`, `[target.'cfg(unix)'.dependencies] libc = "0.2"`), `client/src-tauri/Cargo.lock`
- Modify: `client/src-tauri/src/pump.rs` (`poke`)
- Modify: `client/src-tauri/src/commands.rs` (the `next_batch` envelope; `start` calls `Composers::resend`)
- Modify: `client/src-tauri/src/lib.rs` (`Drafts` and `exit` unless M1c brought them, the managed `Composers`, the dialog plugin, the signal handlers, the commands)
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
    pub fn edit(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64);
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
// commands: composer_open(session: Option<SessionRef>) -> Option<ComposerView>; composer_edit(handle, rev, epoch, text, position, anchor);
// composer_pick(handle, start, end); composer_attach(handle, paths, start, end); composer_paste_image(raw PNG; x-zerus-composer, x-zerus-start, x-zerus-end);
// composer_remove(handle, index); composer_undo_remove(handle); composer_allow_retry(handle)
// next_batch: {...batch, composer?: ComposerView, draft_keys?: string[]}
```

**Review round 1** (commits `2f4260ce`, `44067e5f`; the implementation is authoritative where the code below differs):
- `edit` takes the text epoch and ignores an edit over older text. A change the composer cannot take hands the text back under a new epoch; remove and Undo always answer with a new epoch.
- Undo is refused past 8 files or 20 MiB, with the core's texts, and stays available.
- A refused rename marks the moved draft under its old key and reloads the new key with no message (Qt). A newer composer of the old key is never replaced, and a successful rename takes the old key out of the eviction order.
- `status_error` follows Qt's `ownError`: an uncertain mark alone is a caution.
- The file picker is modal to the main window; a pasted body is copied up to 10 MiB and one byte only.
- The termination signals use the Mac switch plan's `signals.rs`; `signal-hook` is not used. A signal only quits the app (`app.exit(0)`): the exit hook runs once, on the main thread, and a 5 s fallback writes the draft store and ends the process (fix round 1b).

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
        c.edit(view.handle, 1, view.text_epoch, "hello".into(), 5, 5);
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
        c.edit(view.handle, 1, view.text_epoch, "typed before quitting".into(), 0, 0);
        // The exit hook's order: the hub closes, then the store writes everything queued.
        c.flush();
        store.flush();
        assert_eq!(read_draft(&root.join("drafts"), KEY).draft.draft.text, "typed before quitting");
        c.edit(view.handle, 2, view.text_epoch, "after the exit".into(), 0, 0);
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
        c.edit(view.handle, 2, view.text_epoch, "newer".into(), 0, 0);
        c.edit(view.handle, 1, view.text_epoch, "older".into(), 0, 0);
        assert_eq!(c.current().unwrap().text, "newer");
    }

    #[test]
    fn over_64_kib_disables_send() {
        let root = dir("long");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, view.text_epoch, "x".repeat(64 * 1024 + 1), 0, 0);
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
        c.edit(view.handle, 1, view.text_epoch, "see".into(), 3, 3);
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
        c.edit(view.handle, 1, view.text_epoch, "kept across a rename".into(), 0, 0);
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
        c.edit(view.handle, 1, view.text_epoch, "moving".into(), 0, 0);
        let mut renamed = session();
        renamed.name = "claude/sample-project/review-2".into();
        c.open(Some(renamed));
        let view = until(&c, |v| v.loaded && v.text == "already here");
        // Qt's placement: no message on the new key; the moved draft carries the refusal.
        assert!(view.status.is_empty() && !view.status_error);
        store.flush();
        assert_eq!(c.lock().composers[KEY].storage_error.as_deref(), Some(zerus_client_core::drafts::ALREADY_EXISTS));
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
        c.edit(view.handle, 1, view.text_epoch, "x".into(), 1, 1);
        assert_eq!(c.take_draft_keys(), Some(vec![KEY.to_owned()]));
        assert_eq!(c.take_draft_keys(), None, "unchanged keys are not sent again");
        c.edit(view.handle, 2, view.text_epoch, String::new(), 0, 0);
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
        let view = c.current().unwrap();
        c.edit(view.handle, 1, view.text_epoch, "y".into(), 1, 1);
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
//! `composer_edit` with a growing `rev` and the `text_epoch` its text is
//! based on; Rust applies only a higher `rev` of the current epoch. When
//! Rust changes the text itself (a load, an attach or remove, an Undo, a
//! delivery's end, a restore), it bumps `text_epoch` and the webview takes
//! `text`. A locked composer hands its text back the same way.
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

    /// Qt's `restoreDraft`: a change the composer cannot take now (a rename
    /// under way, a remove or Undo it refuses) hands its text back under a
    /// new epoch, so the webview takes the text again and unlocks its editor.
    fn hand_back(&self, mut hub: MutexGuard<'_, Hub>, handle: u64) {
        let mut handed_back = false;
        if !hub.closed {
            if let Some(composer) = Self::by_handle(&mut hub, handle).filter(|c| c.loaded) {
                composer.text_epoch += 1;
                handed_back = true;
            }
        }
        drop(hub);
        if handed_back {
            self.changed();
        }
    }

    /// The editor changed (`MessageComposer.cpp:367-379`). An edit over older
    /// text is ignored; a locked composer hands its text back (`hand_back`).
    pub fn edit(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) {
        let mut hub = self.lock();
        let Some(composer) = Self::editable(&mut hub, handle) else { return self.hand_back(hub, handle) };
        if epoch != composer.text_epoch || rev <= composer.rev {
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
pub fn composer_edit(state: State<'_, Composers>, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) {
    state.edit(handle, rev, epoch, text, position, anchor);
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
```

and, after the `live` block of `setup`:

```rust
            #[cfg(any(target_os = "linux", target_os = "macos"))]
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

**Termination signals** run the same hook rather than a second one. `client/src-tauri/src/signals.rs` is the Mac switch plan's module (Task 3, Steps 1 and 4), copied verbatim with its test, so both branches carry one file: raw `sigaction` with a self-pipe and `SA_RESETHAND` (a second signal ends the process), and a signal ignored at start stays ignored, as Qt's `TerminationSignals`. Declare it `#[cfg(any(target_os = "linux", target_os = "macos"))] mod signals;` in `lib.rs`, and wire it:

```rust
/// SIGTERM, SIGINT or SIGHUP (`signals.rs`, Qt's `TerminationSignals`):
/// the app quits as Qt's posted quit does. `app.exit` ends in
/// `RunEvent::Exit`, where the one exit hook runs once, on the main thread.
/// Should the process not end within five seconds, the fallback ends it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exit_on_signals(app: AppHandle) {
    let installed = signals::install(move |_signal| {
        let stalled = app.clone();
        std::thread::spawn(move || {
            force_exit_after(std::time::Duration::from_secs(5), || flush_drafts(&stalled), || std::process::exit(0))
        });
        app.exit(0);
    });
    if let Err(error) = installed {
        eprintln!("zerus-client: termination signals keep their default action: {error}");
    }
}

/// The fallback of a quit that stalls (the exit hook never runs): after
/// `grace`, the draft store writes what is queued, then the process ends.
/// Only the store is flushed here (thread-safe, idempotent); voice and
/// anything else bound to the main thread is left alone. A flush that hangs
/// keeps the process up, and a second signal still ends it (SA_RESETHAND).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn force_exit_after(grace: std::time::Duration, flush_drafts: impl FnOnce(), end: impl FnOnce()) {
    std::thread::sleep(grace);
    flush_drafts();
    end();
}
// flush_drafts(app): `app.try_state::<Drafts>()` then `drafts.0.flush()` (live builds).
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
// the scheme: zerus-file://localhost/draft/{handle}/{list}/{index} and zerus-file://localhost/cache/{64 hex digest}/{name}
// (http://zerus-file.localhost/... on Windows), answered only to the main webview and only to GET (405 with
// `Allow: GET` otherwise), with one HTTP byte range (206; 416 with `bytes */len`)
// lib.rs
pub(crate) fn cache_dir(app: &AppHandle) -> PathBuf; // the only call to `app_cache_dir()`, like `data_dir`
```

The scheme's rules (Task 3b review):
- **Draft paths** name a live composer's handle, its current attachment list and an index, each plain decimal digits. A draft that sends answers 404, since Qt lists no attachments then (`tray/src/MessageComposer.cpp:1017`) and opens none (`:1069`).
- **Cache paths** are exactly `{64 lowercase hex digest}/{name}`. The name has no `/`, `\` or `:` and is not a `.partial-` copy that `store_at` is still writing. Neither the digest folder nor the file may be a link (`symlink_metadata`, under the canonical root), and `attachments::cached_at` must accept the file. So no `..`, absolute path, drive, stream, link or second folder leads anywhere else.
- **Decoding:** the path is percent-decoded once, before routing or any check. A `%` without two hex digits, a control character (NUL included) or bytes that are not UTF-8, overlong forms such as `%C0%AE` included, answer 404.
- **Ranges:**
  - One `bytes=a-b`, `bytes=a-` or `bytes=-n` with digits only; the end is clamped to the last byte.
  - A unit other than bytes is ignored and the whole file answers 200 (RFC 9110 §14.2).
  - A field without a unit token answers 416 with `Content-Range: bytes */len`, and so do several ranges, a sign, space or other character, a start past the end, an end before the start, `-0`, two `Range` fields and a value that is not text.
  - An `If-Range` never matches, since these answers carry no validator, so the whole file answers.
- **Reads:** a cached copy is opened once and its size taken from the handle: a regular file of at most 10 MiB. Its type is sniffed from a head of up to 64 KiB. A range seeks and reads only its bytes, and a copy that shrank under it answers 404. A whole read stops one byte past 10 MiB.
- **Types:** the type is `sniff`'s for images, audio and video, any `+xml` (SVG) aside. Other text, HTML and XML included, is `text/plain`; the rest is `application/octet-stream`. Nothing served is a page or a script.
- **Guard headers on every answer:** `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'; sandbox`, `Cache-Control: no-store` and `Cross-Origin-Resource-Policy: cross-origin`. 200 and 206 also carry `Content-Length`.

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

The other tests in `composer/files.rs` each pin a rule:
- `only_the_main_webview_is_answered`: other labels, `""`, `Main` and `mainx` get 403.
- `only_get_is_answered`: other methods get 405 with `Allow: GET`.
- `paths_are_decoded_once_and_strictly`, including overlong UTF-8.
- `cache_paths_never_leave_the_cache`: `..` in every encoding, a `..` that reaches a real copy, backslashes, absolute, drive and UNC paths, NUL, streams, uppercase or short digests, non-digest folders, loose files, and `\` and `:` names on Unix.
- `a_copy_still_being_written_is_not_served`.
- `cached_copies_keep_the_cache_bounds`: empty, 10 MiB + 1, a folder, exactly 10 MiB, the handle's size check, a copy that grew after it was opened, and `read_at_most` reads at most limit + 1 bytes.
- `a_link_never_leads_out_of_or_within_the_cache` (Unix): a file or folder linked out, a link to another copy, a folder linked to another digest folder, and a link reached through `..`.
- `ranges_are_parsed_strictly`, including unknown units.
- `a_bad_or_unsatisfiable_range_answers_416`: clamping, suffix, bad ranges, a value that is not text, two fields, `If-Range`, and unknown units answering 200.
- `a_range_reads_only_its_bytes_of_the_cached_copy`: the bytes equal the file's slice past the 64 KiB head; a copy that shrank answers nothing.
- `draft_paths_name_a_live_composer_and_its_list`: encoded paths, stale lists, signs, extra segments, the type, and sending.
- `nothing_served_is_a_page`.

`lib.rs` has `the_attachment_cache_is_the_clients_own`.

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
//! The `zerus-file` URI scheme over draft attachments and the private
//! attachment cache.
//!
//! `zerus-file://localhost/draft/{handle}/{list}/{index}` serves an
//! attachment of a composer in memory while its attachment list is the one
//! the view named; `.../cache/{digest}/{name}` serves a copy in the private
//! cache, only when `attachments::cached_at` accepts it (a regular file inside
//! the canonical root, nonempty, at most 10 MiB) and no link leads to it.
//! Windows webviews ask `http://zerus-file.localhost/...` instead. Only the
//! main webview gets answers, and only to GET. Range requests answer 206, as
//! media elements seek with them; a cached copy is read only as far as the
//! range needs. Bytes reach the webview only this way or as raw IPC bodies,
//! never as JSON.
//!
//! The path is percent-decoded once, before any check: the webview's
//! `convertFileSrc` encodes it whole. A response names the type `sniff`
//! finds in the bytes, never one a webview would run as a page.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::http::{header, response::Builder, Method, Request, Response, StatusCode};
use zerus_client_core::attachments;

use super::Composers;

/// The private attachment cache: `{root}/{sha256}/{name}`, as
/// `attachments::store_at` writes it.
pub struct Cache {
    root: PathBuf,
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl Cache {
    pub fn new(root: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&root);
        Arc::new(Cache {
            root: canonical(&root),
        })
    }

    /// The canonical root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The copy `relative` names, decoded: exactly `{digest}/{name}`, a
    /// digest folder and a file that are neither links, which
    /// `attachments::cached_at` accepts. So no `..`, absolute path, drive,
    /// link or second folder ever leads anywhere else.
    fn copy(&self, relative: &str) -> Option<PathBuf> {
        let (digest, name) = relative.split_once('/')?;
        let digest_ok = digest.len() == 64
            && digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !digest_ok || !plain_name(name) {
            return None;
        }
        let folder = self.root().join(digest);
        let file = folder.join(name);
        // The root is canonical; `symlink_metadata` never follows a link.
        let real_folder = std::fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir());
        let real_file = std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file());
        if !real_folder || !real_file {
            return None;
        }
        attachments::cached_at(self.root(), &file)
    }
}

/// One file name: no separator of any platform, no drive or stream colon,
/// and not a copy `store_at` is still writing. (An empty name, `.` and `..`
/// name a folder, which `copy` never takes for a file.)
fn plain_name(name: &str) -> bool {
    !name.contains(['/', '\\', ':']) && !name.starts_with(".partial-")
}

/// Percent-decodes a URI path once. A `%` without two hex digits after it,
/// a control character (NUL included) or bytes that are not UTF-8 make the
/// path invalid.
fn decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let digit = |at: usize| char::from(*bytes.get(at)?).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = if bytes[i] == b'%' {
            let value = digit(i + 1)? * 16 + digit(i + 2)?;
            i += 3;
            value as u8
        } else {
            i += 1;
            bytes[i - 1]
        };
        if byte.is_ascii_control() {
            return None;
        }
        out.push(byte);
    }
    String::from_utf8(out).ok()
}

/// What a request's `Range` asks of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    /// No range, an `If-Range`, or a unit other than bytes (RFC 9110 §14.2).
    Whole,
    /// Bytes `start..=end`, the end clamped to the last byte.
    Part(usize, usize),
    /// 416: not one satisfiable byte range.
    Unsatisfiable,
}

/// The `Range` of a request for `len` bytes. An `If-Range` never matches,
/// since these answers carry no validator, so the whole file answers then.
/// Two `Range` fields, or one that is not text, are unsatisfiable.
fn wanted(headers: &tauri::http::HeaderMap, len: usize) -> Wanted {
    let ranges: Vec<_> = headers.get_all(header::RANGE).iter().collect();
    match ranges.as_slice() {
        [] => Wanted::Whole,
        _ if headers.contains_key(header::IF_RANGE) => Wanted::Whole,
        [one] => one
            .to_str()
            .map_or(Wanted::Unsatisfiable, |spec| range(spec, len)),
        _ => Wanted::Unsatisfiable,
    }
}

/// One `Range` field: a unit other than `bytes` is ignored (RFC 9110
/// §14.2); a field without a unit token is unsatisfiable, and so is a
/// bytes range `byte_range` refuses.
fn range(spec: &str, len: usize) -> Wanted {
    let Some((unit, set)) = spec.trim().split_once('=') else {
        return Wanted::Unsatisfiable;
    };
    let token = !unit.is_empty()
        && unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    if !token {
        Wanted::Unsatisfiable
    } else if !unit.eq_ignore_ascii_case("bytes") {
        Wanted::Whole
    } else {
        byte_range(set, len).map_or(Wanted::Unsatisfiable, |(start, end)| {
            Wanted::Part(start, end)
        })
    }
}

/// One `a-b`, `a-` or `-n` byte range of `len` bytes, its end clamped to the
/// last byte. None for anything else: several ranges, a number with a sign,
/// space or other character, a start past the last byte or after the end,
/// an empty suffix, or an empty file.
fn byte_range(set: &str, len: usize) -> Option<(usize, usize)> {
    let (first, last) = set.trim().split_once('-')?;
    // Digits only; a number too large for `usize` is past any file.
    let number = |text: &str| {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse::<usize>().unwrap_or(usize::MAX))
    };
    let last_byte = len.checked_sub(1)?;
    let (start, end) = match (first.is_empty(), last.is_empty()) {
        (false, false) => (number(first)?, number(last)?),
        (false, true) => (number(first)?, last_byte),
        // `-0` starts past the last byte, so it is refused below.
        (true, false) => (len - number(last)?.min(len), last_byte),
        (true, true) => return None,
    };
    (start <= end && start <= last_byte).then_some((start, end.min(last_byte)))
}

/// The type a response names for `bytes`.
fn served_type(bytes: &[u8]) -> String {
    safe_type(attachments::sniff(bytes))
}

/// A sniffed type as served: images, audio and video keep theirs (any XML
/// aside, SVG's `image/svg+xml` included), any other text is plain text, HTML
/// and XML included, and the rest an opaque stream. Nothing served is a page
/// or a script.
fn safe_type(sniffed: String) -> String {
    let media = ["image/", "audio/", "video/"]
        .iter()
        .any(|kind| sniffed.starts_with(kind));
    if media && !sniffed.contains("xml") {
        sniffed
    } else if sniffed.starts_with("text/") {
        "text/plain".into()
    } else {
        "application/octet-stream".into()
    }
}

/// Headers of every answer: never stored, never sniffed into another type,
/// run as nothing if opened as a page, and loadable by the app's pages under
/// their `Cross-Origin-Embedder-Policy: require-corp`.
fn guarded(builder: Builder) -> Builder {
    builder
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        )
        .header("cross-origin-resource-policy", "cross-origin")
}

/// The response, or a bare 500 should the builder refuse a header (it never
/// does: every value here is ASCII), so a request is always answered.
fn finish(builder: Builder, body: Vec<u8>) -> Response<Vec<u8>> {
    builder
        .header(header::CONTENT_LENGTH, body.len())
        .body(body)
        .unwrap_or_else(|_| {
            let mut response = Response::new(Vec::new());
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            response
        })
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    finish(guarded(Response::builder()).status(code), Vec::new())
}

fn unsatisfiable(len: usize) -> Response<Vec<u8>> {
    finish(
        guarded(Response::builder())
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}")),
        Vec::new(),
    )
}

/// `{handle}/{list}/{index}`, each plain decimal digits.
fn draft_numbers(rest: &str) -> Option<[u64; 3]> {
    let mut parts = rest.split('/').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    });
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(numbers)
}

/// `draft/{handle}/{list}/{index}`: the attachment of a composer in memory,
/// while its list is still the one the view named. No hashing: an attachment
/// list that changed answers 404 for its old paths. A draft that sends lists
/// no attachments (Qt's `rebuildAttachments`, `MessageComposer.cpp:1017`),
/// so its paths answer nothing until it is back.
fn draft_file(composers: &Composers, rest: &str) -> Option<Arc<[u8]>> {
    let [handle, list, index] = draft_numbers(rest)?;
    let hub = composers.lock();
    let composer = hub
        .composers
        .values()
        .find(|c| c.handle == handle && c.attachments_rev == list && !c.sending)?;
    let attachment = composer
        .draft
        .attachments
        .get(usize::try_from(index).ok()?)?;
    Some(Arc::clone(&attachment.bytes))
}

/// The bytes a type is sniffed from.
const HEAD_BYTES: usize = 64 * 1024;

/// A cached copy, opened once, and its size, taken from the open handle: a
/// regular file of at most 10 MiB.
fn open_copy(file: &Path) -> Option<(File, usize)> {
    let handle = File::open(file).ok()?;
    let metadata = handle.metadata().ok()?;
    let len = usize::try_from(metadata.len()).ok()?;
    (metadata.is_file() && len <= attachments::MAX_ATTACHMENT_BYTES).then_some((handle, len))
}

/// The bytes `part` names of an open copy, with its served type: the whole
/// file (at most 10 MiB, should it have grown since it was opened) typed
/// from its head, or only the range's bytes, typed from a head read first.
/// A copy that shrank so the range is short answers nothing.
fn read_copy(mut handle: File, part: Option<(usize, usize)>) -> Option<(String, Vec<u8>)> {
    let Some((start, end)) = part else {
        let bytes = read_at_most(&mut handle, attachments::MAX_ATTACHMENT_BYTES)?;
        return Some((served_type(&bytes[..bytes.len().min(HEAD_BYTES)]), bytes));
    };
    let mut head = Vec::new();
    (&mut handle)
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)
        .ok()?;
    let count = end - start + 1;
    handle.seek(SeekFrom::Start(start as u64)).ok()?;
    let mut part = Vec::with_capacity(count);
    handle.take(count as u64).read_to_end(&mut part).ok()?;
    (part.len() == count).then(|| (served_type(&head), part))
}

/// All of `source` when it holds at most `limit` bytes. It reads at most
/// one byte past the limit, never the whole of a larger source.
fn read_at_most(source: impl Read, limit: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    source.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

/// A file of `len` bytes, whole or the one range the request names; `read`
/// gives the served type and the bytes of the whole file (None) or of the
/// range, or nothing when the file is gone.
fn answer(
    request: &Request<Vec<u8>>,
    len: usize,
    read: impl FnOnce(Option<(usize, usize)>) -> Option<(String, Vec<u8>)>,
) -> Response<Vec<u8>> {
    let part = match wanted(request.headers(), len) {
        Wanted::Whole => None,
        Wanted::Part(start, end) => Some((start, end)),
        Wanted::Unsatisfiable => return unsatisfiable(len),
    };
    let Some((mime, body)) = read(part) else {
        return status(StatusCode::NOT_FOUND);
    };
    let builder = guarded(Response::builder())
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes");
    match part {
        Some((start, end)) => finish(
            builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}")),
            body,
        ),
        None => finish(builder.status(StatusCode::OK), body),
    }
}

/// Answers one `zerus-file` request for webview `webview`.
pub fn respond(
    composers: &Composers,
    webview: &str,
    request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if webview != crate::MAIN_WINDOW {
        return status(StatusCode::FORBIDDEN);
    }
    if request.method() != Method::GET {
        return finish(
            guarded(Response::builder())
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header(header::ALLOW, "GET"),
            Vec::new(),
        );
    }
    let Some(path) = request.uri().path().strip_prefix('/').and_then(decode) else {
        return status(StatusCode::NOT_FOUND);
    };
    if let Some(rest) = path.strip_prefix("draft/") {
        let Some(bytes) = draft_file(composers, rest) else {
            return status(StatusCode::NOT_FOUND);
        };
        answer(request, bytes.len(), |part| {
            let mime = served_type(&bytes[..bytes.len().min(HEAD_BYTES)]);
            Some(match part {
                Some((start, end)) => (mime, bytes[start..=end].to_vec()),
                None => (mime, bytes.to_vec()),
            })
        })
    } else if let Some(relative) = path.strip_prefix("cache/") {
        let copy = composers.cache().copy(relative).and_then(|file| {
            attachments::touch(&file);
            open_copy(&file)
        });
        let Some((handle, len)) = copy else {
            return status(StatusCode::NOT_FOUND);
        };
        answer(request, len, |part| read_copy(handle, part))
    } else {
        status(StatusCode::NOT_FOUND)
    }
}
```

Each guard has a test that fails without it. The mutation runs are recorded in the Task 3b report.

- [ ] **Step 5: Register it** in `client/src-tauri/src/lib.rs`. `Composers::new` gets the config, from the app's cache folder. `cache_dir(app)` is the only call to Tauri's `app_cache_dir`, as `data_dir` is for the data folder, so the Mac switch build can replace its body. Qt's cache is `hgdev/hgs-tray/attachments` (`tray/src/AttachmentViewer.h:28`), a different folder this client never touches:

```rust
#[cfg(feature = "live")]
pub(crate) fn cache_dir(app: &AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.path().app_cache_dir().expect("the platform has an app cache directory")
}

#[cfg(feature = "live")]
fn attachments_root(cache_dir: &std::path::Path, replay: bool) -> std::path::PathBuf {
    cache_dir.join(if replay { "attachments-replay" } else { "attachments" })
}

// in setup:
let cache = attachments_root(&cache_dir(app.handle()), replay);
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
                    let response = match app.try_state::<composer::Composers>() {
                        Some(composers) => composer::files::respond(&composers, &webview, &request),
                        None => tauri::http::Response::builder().status(404).body(Vec::new()).expect("a bare 404"),
                    };
                    responder.respond(response);
                });
            }
            #[cfg(not(feature = "live"))]
            {
                let _ = (ctx, request);
                responder.respond(tauri::http::Response::builder().status(404).body(Vec::new()).expect("a 404"));
            }
        })
```

In `tauri.conf.json`, in `csp` and `devCsp`, keeping the dev server origin in `devCsp` (its `media-src` keeps `http://127.0.0.1:1420`):
- `img-src` gains `zerus-file: http://zerus-file.localhost`;
- `media-src` is `'self' zerus-file: http://zerus-file.localhost`;
- `connect-src` gains `zerus-file: http://zerus-file.localhost`.

The `connect-src` entry is for Task 9's text preview only if Task 9 reads it with `fetch`; see Task 9's CORS conditions. Task 9 removes the entry if it takes the IPC route.

The responses carry `Cross-Origin-Resource-Policy: cross-origin`, which the app's `Cross-Origin-Embedder-Policy: require-corp` needs for images and media. They carry no `Access-Control-Allow-Origin`, so a cors-mode `fetch` cannot read them yet.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::`
Expected: PASS, with Task 3a's tests plus the 14 in `composer/files.rs`. `lib.rs` adds `the_attachment_cache_is_the_clients_own`. Clippy with and without default features, the Windows-target clippy (only the known `display.rs` and `metrics.rs` items) and fmt are clean.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the zerus-file scheme over draft attachments and the private cache"
```

---

### Task 4a: The session's part of the composer: block reasons, the cache chip and the inspect fallback

**As built** (`ad286289` and its review fix round; the Decisions' Task 4a rulings). Where the steps below differ, the code and this list win:
- **The tick.** It recomputes the open composer's session part (`send::SessionView`) once a second and wakes the pump only when it differs from the last tick's (`Hub.seen`, in place of `Hub.minute`). The ticker sleeps on `Shared.opened` while no composer is open and ends when the hub closes.
- **`Followed`** is `{computer, key, details, expiry, inspect_error: Option<String>, opened_at, inspected_at, inspecting: Option<u64>, streamed, inspected: Option<send::Basis>}`; `Hub` also has `next_inspect`. `expiry` is the cache expiry formatted before the hub lock is taken (`send::expiry`).
- **`observe`** returns at once on an empty batch, applies the followed session's last `state` or `ended` in a `match` (A2-4 runs its queue hook only for an accepted state), and wakes the view when `touches` finds the open session's row or its computer's status in the batch. `ended` keeps the details, clears `streamed`, `inspected` and `inspecting`, and the next tick inspects (Qt's `activityEnded`).
- **`refresh_details`** reads nothing while the stream keeps the details or the computer is offline, one inspect at a time, and a late answer of another follow is dropped by its number. The tick calls it 3 s after opening without a state, at once after `ended`, every 2.5 s after a failure (Qt's `m_activityPollMs`), and when the row's run, conversation or state changed. Task 6's Refresh calls it.
- **`Inspect`** returns `Result<Value, String>`; `send::inspection` reads the CLI's answer as Qt's `requestInspection` does, and the error becomes "Could not refresh activity: %1".
- **`ComposerView`** also has `attachments_reason`, `session_status`, `session_status_error` and `session_blocks_input` (ruling 1).
- **`actions.rs`**: `WriteKind`, `WriteMark`, `Running` (the mark drops before the claim), `blocks_messages`, `Actions::{write_kind, begin, finished, session_write, on_writes}`, `WritesHook`, and `dispatch_with(…, runner)` behind `dispatch`. `write_kind` is the window-wide rule (ruling 2); `session_write` adds a retained termination (ruling 4).
- **The `facts` dep** in `lib.rs` is `actions.session_write(computer, name, || (pump.row_named(..), pump.online(..)))`.
- **Test helpers**: `send_hub(&Scratch)`; `Fake` also has `online` and `write`, and its `inspected: None` answers `Err("timed out")`.

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
        c.edit(handle, 1, view.text_epoch, "hello".into(), 5, 5);
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
        c.edit(view.handle, 1, view.text_epoch, "hi".into(), 2, 2);
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

  - `Hub` gains `pub(crate) followed: Followed` and `pub(crate) seen: Option<send::SessionView>` (as built: see the list at the top of this task).
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

    /// Once a second while a composer is open: its session part, the
    /// cache chip included (Qt's chip refreshes every second and counts
    /// `ceil((expires_at - now) / 60)` minutes, `CacheStatus.h:31, 51`), and
    /// the inspect fallback (`wants_details`, as built).
    pub fn tick(&self) {
        let now = self.now();
        let mut hub = self.lock();
        let seen = hub
            .open_key
            .as_ref()
            .and_then(|key| hub.composers.get(key))
            .map(|composer| session_view(self, &hub, composer));
        let changed = hub.seen != seen;
        hub.seen = seen;
        let wants = self.wants_details(&hub, now);
        drop(hub);
        if wants {
            self.refresh_details();
        }
        if changed {
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

**As built** (`71a2f63d` and its review fix round 1). Where the steps below differ, the code and this list win (Qt wins over the brief; `SessionsWindow.cpp` and `MessageComposer.cpp` lines):
- **Send's checks.** `send_message` also refuses (`Busy`) when the editor's text is not the draft's after its own edit, and when Qt's Send would be disabled (`view.can_send`: content, 64 KiB, files the agent takes; `MessageComposer.cpp:1185`, `send` 814). Every `Busy` or `Blocked` answer hands the text back under a new epoch. `Cold` does not: the question holds the send, as Qt's modal dialog does.
- **The last gate.** `deliver` returns `bool` and refuses (Qt's `prepareSendingAsync`, 731): a closed hub, a composer that is not loaded or is sending, attaching or renaming, an uncertain draft unless the delivery preserves it, and a Retry whose row is no longer a definite failure (`deliverMessage`, 4862-4869). It does not refuse `waiting` (Task 5 delivers from a waiting continuation; Qt drops `m_waitingSends`, 732). Callers answer `Busy` when it refuses.
- **The save revision.** The durable save takes the next `saves` and sets `saving`. `prepared(delivery, handle, revision, saved)` ignores a stale answer (741). A failed one ends the send, sets `save_failed`, schedules `retry_save` and hands the text back. A successful one clears the storage error and bumps `text_epoch` (Qt clears the editor, 751).
- **Undo.** `deliver` clears `removed` for every delivery, a Retry's included.
- **Preserve.** A Retry that keeps the draft keeps its error mark and notice in `Composer.kept` and gives them back at the end (Qt's `m_preservedDrafts`, `MessageComposer.cpp:758-763`).
- **`Delivery`** gains `row_run`, `row_conversation` (Qt's `observedIdentityMatches`, 147-154) and `evidence` (the details checked at Send, Qt's send evidence). Task 5 builds deliveries with `delivery()` or fills these in its literal.
- **`still_valid`** follows `observedIdentityMatches` (the row's run at Send, or the details' run). Without consent, a cache that expired in the evidence or in the current details is invalid (4881). For a session no longer open, the evidence stands in for its details.
- **`Prompt`** gains `handle`, `text` and `attachments`, the message asked about, filled by `ask(hub, ctx, retry)` (its signature is unchanged). A new question replaces its composer's older one. A non-retry answer sends only while the draft still holds that message; otherwise it answers `Notice` with Qt's "Draft changed. Send it when you are ready." (4845).
- **`SendReply::Notice { text, error }`** carries Qt's `showNotice(text, error)` tone (`SessionsWindow.h:299`), not stickiness: block reasons (4486, 4850) and read errors (4500) are errors, "Draft changed" (4845) is plain. Task 5's notices pass the tone of the Qt call they port; Task 6 shows `error` as the error tone and the rest as information.
- **The delivery under way.** `Composer.delivering` holds the revision of the live delivery's "sending" save and `Delivery.revision` its own. `prepared`, `send_now` and every end of a delivery (`ended`) act only while they match, so a delivery that a cancel ended, or that a newer one replaced, neither calls nor ends anything. `delivery_finished` (no revision) ends whatever runs: Task 5's cancels and the reconcile use it.
- **Retry's seam.** `retry` is `retry_begin` (the checks and `retry_reads`, before the read), the read, then `retry_finish(begun, read)`. Tests drive the two halves around a session switch.
- **Retry's pins** come from S4's rows: the core's `LocalMessages::keys()` (added here), not `opened`, which only serves forgetting a session that left the catalog. `Hub::local_keys` is gone. `retry_files` copies only the definite failures under the hub lock and reads their files after it.
- **`SendReply::ClearContext`** is not in 4b: nothing constructs it before Task 5, and clippy's `dead_code` refuses it. Task 5 adds it with the Clear answer.
- **The call.** `send_now` treats a panicked dispatch like an `Err`: uncertain. After the end it calls `refresh_details` for the open key (`finishMessage`'s `inspect()`, 4956).
- **Retry after the read** (`messageAction`, 4497-4523): Qt's quiet checks (identity, online, no write, live and running evidence, the row unchanged, the composer not busy). For the open session, a block reason becomes the composer's error (`Blocked`), then the cold question. For a session no longer open, a warm retry is delivered there and a cold one is dropped. Unreadable file entries answer `Notice` with `read_stored`'s text.
- **`inspect_local`** answers only for the id of a failed row (4475-4481).
- **Events.** `Followed.events` keeps Qt's 500 (`EVENTS`, 3651). `refresh_details` keeps the inspected `events` and merges them into `Followed.events` by `seq` (3639-3651; `replace_events` replaces). `observe` reconciles on events-only batches too and wakes the view when a row went or when the batch touches the open row (`touches`). `reconcile` returns whether a row went and runs with null details when none are known.
- **Not done, by Qt:** a send is not a write mark in `Actions`. Qt's `m_pending` is set by writes only; a send lives in `m_pendingMessages` and blocks only its own session (`isSending`, 4808, 4858). `still_valid` refuses while any write runs (`facts`, Qt's `m_pending` at 4889).
- **Tests.** `send_hub_with(store, spawn)` takes the test's spawn, and `held_spawn`/`next_job` hold the hub's jobs so a test orders them; the brief's `dir("retry")` became `dir("retry-row")`, since Task 3a's `a_failed_save_is_retried_until_it_succeeds` uses that folder.
- **`Cache::prune_later`** sets `again` before it claims `pruning`, and the job checks `again` once more after its release, so a store that meets a prune about to end is never lost.

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
pub enum SendReply { Sending, Busy, Blocked { reason: String }, Notice { text: String, error: bool },
                     Cold { token: u64, prompt: ColdPrompt }, ClearContext { token: u64 } }
#[serde(tag = "kind", rename_all = "snake_case")] pub enum Answer { Cold { action: ColdAction }, Cancel }
impl Composers {
    pub fn send_message(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> SendReply;
    pub fn answer(&self, token: u64, answer: Answer) -> SendReply;
    pub fn retry(&self, handle: u64, id: &str) -> SendReply;   // blocking: reads the cached files
    pub fn delete_local(&self, handle: u64, id: &str);
    pub fn inspect_local(&self, handle: u64, id: &str) -> Option<Inspect>;
}
// crate-internal, for Task 5: Retry, Prompt, Delivery, Context::{run, conversation}, ask, delivery, deliver, delivery_finished, reconcile
// commands: send_message(handle, rev, epoch, text, position, anchor) -> SendReply; send_answer(token, answer) -> SendReply;
// local_retry(handle, id) -> SendReply; local_delete(handle, id); local_inspect(handle, id) -> Option<Inspect>
```

**The rules this task keeps (AGENTS.md: unknown delivery never authorizes replay):**
- The durable "sending" save comes before any call: `deliver` saves, and only its callback (`prepared`) can dispatch.
- One request id per delivery, made right before its one dispatch. The local row, the CLI request and the receipt check carry it, and no other delivery reuses it.
- An `Uncertain` outcome (or an `Err`, which means the call's thread died) never resends. The draft stays, marked uncertain, and asks for Allow retry. Its row offers Check Terminal and Delete, never Retry.
- A question's answer re-checks the composer. A busy or uncertain draft is never sent by an older question, and a delivery drops its key's open questions.
- A closed hub (the app quits) starts nothing. A draft saved as "sending" reloads as uncertain. The durable save's callback checks `closed` before it dispatches.
- `send_message` takes the text epoch, as `composer_edit` does, and refuses (`Busy`) when it differs from the composer's: Send never delivers text typed over an older draft (Task 3a review).
- Every write to a composer's key, the durable ones included, goes through the hub's `save` revision (`saves`), so an older answer never overwrites a newer state and a failed save is retried.
- A send clears `removed`: Undo never brings a sent file back.

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
        c.edit(view.handle, 4, view.text_epoch, "hello [File #1] ".into(), 0, 0);
        assert!(c.current().unwrap().can_send);
        assert_eq!(c.send_message(view.handle, 5, view.text_epoch, "hello [File #1] ".into(), 0, 0), SendReply::Sending);
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
            c.send_message(view.handle, 1, view.text_epoch, "hi".into(), 2, 2),
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
        let SendReply::Cold { token, prompt } = c.send_message(view.handle, 1, view.text_epoch, "hi".into(), 2, 2) else { panic!("no question") };
        assert_eq!(prompt.title, "Continue with a cold cache?");
        assert!(sent(&fake).is_empty());
        assert_eq!(c.answer(token, Answer::Cancel), SendReply::Busy);
        assert_eq!(c.answer(token, Answer::Cold { action: ColdAction::Send }), SendReply::Busy, "a cancelled question is gone");
        let SendReply::Cold { token, .. } = c.send_message(view.handle, 2, view.text_epoch, "hi".into(), 2, 2) else { panic!() };
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
        assert_eq!(c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5), SendReply::Sending);
        let view = until(&c, |v| !v.sending && v.uncertain);
        assert_eq!(view.text, "maybe", "the draft comes back");
        assert!(!view.can_send);
        let row = &view.local[0];
        assert!(row.uncertain);
        assert_eq!(row.error, "Delivery is not confirmed. Check Terminal before retrying. Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.");
        assert_eq!(c.retry(view.handle, &row.id), SendReply::Busy);
        assert_eq!(c.send_message(view.handle, 2, view.text_epoch, "maybe".into(), 5, 5), SendReply::Busy, "Send waits for Allow retry");
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
        let SendReply::Cold { token: first, .. } = c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5) else { panic!("no question") };
        let SendReply::Cold { token: second, .. } = c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5) else { panic!("no question") };
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
        assert_eq!(c.send_message(view.handle, 1, view.text_epoch, "at exit".into(), 7, 7), SendReply::Sending);
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
        c.send_message(view.handle, 1, view.text_epoch, "first".into(), 5, 5);
        let view = until(&c, |v| !v.sending && v.local.first().is_some_and(|r| !r.error.is_empty()));
        let id = view.local[0].id.clone();
        c.edit(view.handle, 9, view.text_epoch, "a newer draft".into(), 13, 13);
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
        c.send_message(view.handle, 1, view.text_epoch, "shown in history".into(), 0, 0);
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
  - `Followed` gains `events: VecDeque<Value>`, the last 500 events of the followed session (Qt's 3651).
  - `Hub` gains `local: LocalMessages`, `retry_reads: HashSet<String>`, `prompts: HashMap<u64, send::Prompt>`, `next_token: u64`, and `opened: HashMap<(String, String), String>` with `opened_order: VecDeque<(String, String)>` (computer and catalog key to draft key; `open` records it and drops the oldest entry past 512, matching S4's 512 sessions).
  - `renamed` also calls `hub.local.rename(old, new)` when the store moved the draft.
  - `observe` also keeps the events (`"events"` appends, `"replace"` clears first; at most 500, oldest dropped). It forgets the local rows of a session that left the catalog: for `batch.removes` with a reason other than `replaced`, `hub.local.forget(key)` through `opened`.
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
    pub fn send_message(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> SendReply {
        self.edit(handle, rev, epoch, text, position, anchor);
        let mut hub = self.lock();
        let Some(ctx) = self.context(&hub) else { return SendReply::Busy };
        let Some(composer) = hub.composers.get(&ctx.key).filter(|c| c.handle == handle) else { return SendReply::Busy };
        // Text typed over an older draft (a remove, an Undo, a hand-back) is never sent.
        if hub.closed || !composer.loaded || composer.busy() || composer.uncertain || composer.text_epoch != epoch {
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
pub async fn send_message(state: State<'_, Composers>, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.send_message(handle, rev, epoch, text, position, anchor)).await.map_err(|e| e.to_string())
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

**As built** (`9ca0a201` and its review fix round 1). Where the steps below differ, the code and this list win (Qt wins over the brief; `SessionsWindow.cpp` and `MessageComposer.cpp` lines). Tasks 6 and 7 bind to this list:
- **`WriteKind::Clear`.** `begin` marks a running `ClearContext` as `Clear` under its own computer and name, though it blocks no other session. `write_kind` answers the session's own `Terminate` or `Clear` first, then `Update` for any other mark; another session's `Clear` is no update anywhere. `session_write` keeps a running `Clear` ahead of a retained termination, as `messageBlockReason` reads `m_clearKey` first (4390). `context()` sets `clearing` from `Clear` or from the hub's own reset (`Hub.clearing`), and `start_send_waiting` and `compacting` from the hub's continuations.
- **`Hub`** gains `start: Option<(Pending, StartSend)>`, `compacts: HashMap<String, (Pending, CompactSend)>`, `last_poll` and `clearing: Option<String>`. `clearing` is Qt's `m_clearKey` while `m_clearRequest` runs: one reset at a time across all sessions.
- **`Pending`** is `{key, session, run, conversation, retry, revision, evidence, polling}`:
  - `run` and `conversation` are what its call names. A start names the stopped run (`resumeSession`, `startFreshSession`, 4759-4760). A compaction names the conversation it compacts (`requestCompactContext`, 4561), and after a start that is the new run's.
  - `evidence` holds the details its session last showed (Qt's pending send evidence).
  - `polling` keeps its poll to one inspect at a time.
- **The revision rule (Task 4b review M1).** `wait` (`prepareWaitingToSendAsync`) saves the waiting draft durably under the next save revision. That revision is the continuation's `Pending.revision`, and `Composer.delivering` holds it while the draft waits.
  - Every step, call outcome and cancel names that revision. A late one finds nothing and changes nothing.
  - A poll's answer uses the revision only to free its `polling` mark. Its reading then goes to whatever continuation of that key waits, as Qt feeds every inspection to both continuations (3607). This is harmless: S3 compares the run, the conversation and the draft again.
  - The call's thread checks the continuation again before it calls, as Qt asks inside the save's callback (4762, 4561). A Cancel send or Stop that lands between the save's answer and the call leaves no call.
  - A cancel ends the composer only while `delivering` still holds its revision. A cancel that arrives after the continuation's send began never ends that delivery.
- **`Composer.digests`** caches the SHA-256 digests of one attachment list (`attachments_rev`). `mark` builds S3's `DraftMark` from them. Each digest covers the name, type, reference and bytes, as Qt's `draftMatches` compares them (`MessageComposer.cpp:668-676`).
- **`Prompt`** gains `rules: Option<Session>` and `mark: DraftMark`, both set only by `ask_start`, for the start question. `ask(hub, ctx, retry)` is unchanged. A cold answer to a start question, and a start answer to a cold question, answer `Busy`.
- **The start question.** `send_message` asks it after Task 4b's composer checks and before the block reason, and only while Qt's Send is enabled (`view.can_send`). It passes R1's Context chip text (`session_usage::context(&details["session_usage"], ctx.recorded()).text`, 4709).
  - A start answer skips the cold question's quiet identity check. S3's `StartSend::begin` compares the catalog's session instead (4730-4734).
  - Its refusals are notices in Qt's tones: "The session changed…" is an error, "Draft changed…" is plain.
  - The view's availability for a stopped session follows 3987-3989: `available` stays true with "Session stopped. Send asks whether to resume it or start fresh.", and `block_reason` still holds the block reason.
- **Compact and continue** (4844-4847, 4536-4564) answers:
  - `Busy` without `compact_context_supported` or while this session already compacts;
  - "Draft changed" (plain) unless the draft is the question's message, a Retry's message too;
  - "Compaction was cancelled…" (error) for an uncertain draft, whose uncertain mark stays. Qt's `deliveryFinished` would clear the mark there; that is not ported.
- **Clear context** in the cold question answers `SendReply::ClearContext { token }` (4843), which opens the Clear session dialog. Its choice is disabled while any reset runs (4825).
- **The calls.** Each Resume, StartFresh and CompactContext call runs on the hub's blocking threads with its own request id. S3 ends the continuation on any failure, and its message is never sent:
  - A taken write lane (`actions::BUSY`, refused before anything ran) is a definite failure. Its text is Qt's "Starting was cancelled. Your draft was kept without sending." (4756) or "Compaction was cancelled. Your draft was kept without sending." (4559), in the error tone.
  - Any other `Err`, or a panic, is uncertain.
  - The send at the end is an ordinary `deliver`: consent, its durable "sending" save, its own request id and `still_valid`.
  - `Delivery.evidence` is the reading that showed the session ready (Qt passes `data`, 4623 and 4801).
  - A delivery that `deliver` refuses ends the waiting draft with "The session changed…".
  - After each call's outcome, the open session's followed details are read again (`continue_open`), and an accepted compaction is polled at once (975).
- **Readings and polls.** Every Activity state of the open session (`after_observe`), every `refresh_details` answer and every poll feeds S3's `inspected`, the compaction's first (3607). A reading without fields is ignored.
  - The tick ends continuations past 180 s or 600 s. Every 2.5 s it polls the start, and each compaction whose request was accepted, whatever is open (4566-4590).
  - The open session is polled through `refresh_details`, which reads nothing while the stream keeps its details: the stream's state carries `compact_context_request` (`hgs` `state_value`).
  - Another session gets its own `deps.inspect`, one at a time and only while its computer is online. Only the continuation sees that answer.
  - The ticker runs while a continuation waits, even with no composer open.
- **Cancel send and Stop** act only on the open composer's handle (Qt's `m_selectedKey`, 964-965 and 1188-1191). They cancel its compaction, then its start. Qt's other interrupt checks (`interrupt_supported`, one interrupt at a time, 1189) stay with the Stop entry, which calls `send_interrupted` only when it interrupts.
- **`ComposerView`** gains `cancel_send`, `compacting` and `clear: ClearControl`. `SessionView` carries `clear` too, so the tick notices its changes.
- **`ClearControl`** is `{enabled, dismiss, title, text, confirm, run, conversation, cache_enabled, running, reason, cache_reason}`:
  - `enabled` is Qt's `canClear` (3798): online, not archived, no reset running anywhere, no send of this session, no inspect error, and `actions::clearable`.
  - `cache_enabled` is the cache popover's Clear context (3792): online, no reset running, and `clear_context_supported`. A chooser alone does not enable it.
  - `running` drives the "Clearing…" label (646, 3804).
  - `reason` and `cache_reason` are the tooltips' reason sentences (`explainContextAction`, 3795-3803), empty for the usual tooltip. `reason` is empty while a chooser allows Dismiss and clear.
  - Task 6's popover button uses `cache_enabled`, not `enabled`.
- **`clear_context(handle, run, conversation, dismiss)` blocks** until the reset ends. It answers `Busy` when the confirmed identity or variant no longer holds, or when it is not enabled (4660-4664). It reads the session again (1026).
  - Once the call ends it answers Qt's notice (1021-1028): "Context cleared. Your draft is ready to send." for a confirmed receipt, otherwise "Clear command sent. Waiting for the agent to confirm the new conversation…"; a failure is an error notice with its detail.
  - It answers `Sending` when another session is open by then, since Qt shows nothing.
  - The target is the run and conversation the dialog showed (`m_details`, 4668).
  - A taken write lane is a definite refusal with Qt's block reason, in the error tone: "The session is terminating. Your draft stays here." while the session's own termination runs (4391), else "Wait for the session update to finish." (4395). It never shows "busy".
- **Every Clear goes through `clear_context`.** This covers Task 6's header and popover buttons and Task 7's session menu "Clear session…" (Qt's `SessionMenuAction::Clear`, 5339-5340, which calls `clearContext()` at 5490). Nothing dispatches `ClearContext` through `run_action`: that would skip the one-reset-at-a-time rule (`Hub.clearing`) and the dialog's identity check.
- **The start question keeps the epoch, as the cold question does.** Neither a `Start` reply nor its answer or close changes `text_epoch`. Task 6 unlocks the editor when the dialog closes, and flushes the editor before answering.
- **Commands:**
  - `send_cancel(handle)` and `send_interrupted(handle)`;
  - the async `clear_context(handle, run, conversation, dismiss) -> SendReply`;
  - `send_answer`, now async: it runs on `spawn_blocking`, since Compact and continue hashes the draft's files (at most 20 MiB) the first time.

  All four are registered in the live handler.
- **Not ported.** Qt's Windows session-host question before a start (`LocalSessionHost::confirmLaunch`, 4728) has no client counterpart yet, as for Resume elsewhere (`client/src/store/batch.ts`).

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
        let SendReply::Start { token, question } = c.send_message(view.handle, 1, view.text_epoch, "after start".into(), 11, 11) else { panic!("no question") };
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
        let SendReply::Cold { token, prompt } = c.send_message(view.handle, 1, view.text_epoch, "compact me".into(), 0, 0) else { panic!() };
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
        let SendReply::Start { token, .. } = c.send_message(view.handle, 1, view.text_epoch, "kept".into(), 0, 0) else { panic!() };
        c.answer(token, Answer::Start { choice: StartChoice::Fresh });
        until(&c, |v| v.waiting.is_some());
        c.cancel_send(view.handle);
        let view = until(&c, |v| v.waiting.is_none());
        assert_eq!(view.status, "Pending send cancelled. Your draft is unchanged.");
        assert_eq!(view.text, "kept");
        let SendReply::Start { token, .. } = c.send_message(view.handle, 2, view.text_epoch, "kept".into(), 0, 0) else { panic!() };
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
        let SendReply::Start { token, .. } = c.send_message(view.handle, 1, view.text_epoch, "late".into(), 0, 0) else { panic!() };
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
  - `ClearContext` and `CompactContext` stay out of the window-wide update (Decisions, Task 4a ruling 2). Task 4a's `blocks_messages` leaves both unmarked. This task marks a running `ClearContext` as `Clear` for its own session only (Qt's `m_clearKey`), so another session's reset is no update anywhere; `write_kind` reports `Clear` only to the session it resets and never counts it as an update. `CompactContext` stays unmarked: compact-to-send's own `compacting` fact blocks its session (Qt's `m_compacts`).
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

**As built** (`c5e94b2a` and its review fix rounds, `94efe79d` and `f1eef421`). Where the steps below differ, the code and this list win (Qt wins over the brief; `MessageComposer.cpp`, `SessionsWindow.cpp` and `ActivityView.cpp` lines). Task 7 binds to this list:
- **The store** is `createComposer(port, timers?, images?, notices?)`; `notices` defaults to the app's `notify`. `ComposerPort` gains `refresh(handle)`. It exports `EDIT_MS`, `MAX_TEXT_BYTES`, `MAX_PIXELS`, `TOO_LONG`, `draftKey`, `statusLine`, and the `Dialog`, `Images`, `Notices` and `ComposerPort` types.
  - Its API: `view`, `text`, `selection` (`start` is Qt's anchor, `end` its position), `dialog`, `locked`, `operationLocked`, `sendEnabled`, `hint`, `status`, `apply`, `receive`, `open`, `input`, `select`, `flush`, `attachFiles`, `pasteImage`, `remove`, `undo`, `send`, `answer`, `closeDialog`, `openClear`, `confirmClear`, `cancelSend`, `allowRetry`, `interrupted` and `refresh`.
- **The locks** follow Qt's `locked` (1148-1149). `operationLocked` covers an `open` under way, the open composer's own attach or paste, a remove or Undo until a higher epoch, the open composer's own send or answer call (locked before its flush, so one Enter sends once; the attach and call locks are sets of handles, so a call keeps locking its own composer after the user went to another one and back), a started send until the view shows `sending`/`waiting` or a higher epoch, an open cold cache or start question, and Rust's `!loaded`/`attaching`/`sending`/`waiting`. `locked` adds `session_blocks_input`.
  - An `open` flushes the composer being left first and keeps the editor read-only until it answers, so no key is typed into the composer being left.
  - A refused Send (`busy`, `blocked`) stays locked until Rust hands the text back. An answer's refusal changes nothing.
  - A question keeps the epoch, so closing it unlocks the editor. The store flushes before answering.
  - A question still open when the composer changes or goes answers Cancel.
- **Replies keep their composer.** Every send, answer and clear keeps the handle its call started on (`reply(r, h, at, refused)`). A reply that arrives after another composer opened keeps only its notice. A cold or start question it asks is answered Cancel and never shown. Busy, blocked, sending and the `clear_context` reply change nothing there. Task 7's `local_retry` replies go through the same `reply`.
- **`sendEnabled`** is Qt's Send rule (1185), computed from the view's facts and the text being typed, so it does not wait for an edit's round trip. A disabled Send pressed with content shows the status at the editor for 5 s (`hint`, 814-818).
- **Edits.** A cursor move counts as an edit (`rememberCursor`). Every key flushes at once while the view has `status_error`, an uncertain mark or a failed local row. Unflushed typing also goes when the page hides or goes: `flushWhenHidden` listens to `visibilitychange`, `pagehide` and Rust's `window-visibility` event. Cmd+Q's exit handshake is `zerus-5wmh.9.30`.
- **The 64 KiB check** is a memo of the text: a text of at most a third of the limit in UTF-16 units needs no encoding, and one over the limit in units is too long.
- **Envelope views** go through `receive`. It drops a view of another composer (by draft key, or by the open handle). It holds views that arrive while an `open` runs and applies them after the open's answer.
- **The status line** is `statusLine(view, ownError, tooLong)`. Its order: waiting, the store's own error, then Rust's status unless that is the session status, the block reason or the stale length text, then the length.
- **Qt over the brief:**
  - The store's own error (a refused paste) survives typing, as Qt's `showError` notice (textChanged, 366-376).
  - A paste is refused above 32 000 000 pixels, not at it (394; the core's `pasted_image`).
  - Notices are sticky only in the error tone (`showNotice`, `SessionsWindow.cpp:5210`).
  - The placeholder is always "Message this agent…" (`updatePlaceholder`, 569); the unavailable reason is in the status line.
- **`Dialog`** is `cold | start | clear`. `clear` holds only its `control` snapshot: the `clear_context` reply's token is already spent in Rust. `openClear()` opens only while `clear.enabled`, as Qt's `clearContext()` returns early. `ComposerDialogs` is mounted once in `SessionDetail`, so the header's Clear works on any tab.
- **The toolbar** keeps Qt's slot order (`ComposerToolbar.h:105`, `leading` at `ComposerToolbar.cpp:242`): Attachments, the voice slot (Task 13), Latest, Mark as read, the compaction indicator, Cancel send, the grip, the cache chip and the Context chip. Below 600 px (times the content scale) the chips shorten: "{n}", the cache's `short_label`, an icon-only Mark as read.
  - Mark as read uses `markReadChip(s, online)` (this base's signature) with Qt's `read-all` icon (982). A focused chip moves the focus to the transcript before it hides (1038), as the Latest chip does (`ActivityView.cpp:1611`): `.act-scroll` takes the focus from code (`tabindex="-1"`, through Activity's `focusTranscript()`, which answers false when no transcript is shown). Without one, the message field takes it while shown and enabled, the toolbar's focus fallback (`MessageComposer.cpp:280`; `ComposerToolbar.cpp:225-227, 266-267`). Both are `moveFocusOff` in `ui/sessions/composer-ui.ts`.
  - The compaction indicator needs the row `tracked`, online and `running`, no read error (`ComposerView.inspect_error`, Qt's `m_inspectError`), and the followed state's `phase` `compacting`, `runtime_state` `live`, `process_state` `running` and a `state` other than `archived` (1899-1901; `SessionsWindow.cpp:3974-3975`).
  - The cache chip shows the warning glyph except when warm (`CacheStatus.h`). Its popover's Clear context shows only with a warning (3791), is enabled by `clear.cache_enabled`, and its tooltip is `cache_reason` or Qt's.
  - The Context chip is R1's `ComposerView.context` (`session_usage::ContextChip`, 3788). It is not clickable until the side panel exists.
- **The action row** follows Qt's order (364): Attach, Reference file (disabled), the status line, Refresh, Allow retry, the model button (disabled), Stop and Send.
  - Refresh shows while the session status is an error and is enabled while `!operationLocked` (1194-1195). It calls `composer_refresh`.
  - Allow retry needs `uncertain && !operationLocked` (1193).
  - Send's tooltip is only the waiting text, since no tooltip names a chord.
  - The status line takes the warning colour while a send waits (1224-1226).
- **Stop.** Every Stop (the composer's, the header icon, the header menu) calls `composer.interrupted()` before phase 2's `actionEntries(...).stop.action()`, since every Qt Stop is the composer's `interruptRequested` (1187-1191).
- **Clear session** in the header (button and menu entry) uses `composer.view()?.clear`: `enabled`, `reason` as the hint and "Clearing…" while `running`. It calls `composer.openClear()`.
- **Placement (updated by zerus-5wmh.9.29).** The composer is hidden only while a subagent is open. An archived row opens its own draft key and keeps the editor mounted and editable; Send is blocked with "Resume the session to send a message." while online (Qt `SessionsWindow.cpp:3454,4393`). Recorded Activity remains read-only and is never followed live. The hub resolves an archive key without a live catalog row, isolates it from any live session of the same name, and starts no composer inspection for it. Offline block-reason priority is unchanged.
- **Keyboard.** The composer root has `data-shortcut-context="composer"`. `message.send` is registered with `onCommand(id, handler, editor)` while mounted. `message.push` is not registered: push is not enabled before A2 (Qt's `setPushEnabled`, 841-847). The attachments list and the cache popover are `data-shortcut-context="none"`.
- **src-tauri:**
  - `composer_refresh(handle)` runs `Composers::refresh`, which calls `refresh_details` for the open composer's handle only.
  - `composer_tab(tab)` sets `Hub.poll` through `Composers::set_poll(send::poll_seconds(tab, settings))`. Terminal and Native UI take `processes/backgroundSeconds`, Processes takes `processes/activeSeconds`, and any other tab keeps 2.5 s for a failed read's retry (2013-2016).
  - `ComposerView` and `SessionView` gain `context: ContextChip` and `inspect_error: bool` (the open session's last read failed).
- **Envelope and catalog.** `bridge.normalize` is exported and keeps `composer` and `draft_keys`; `isEmptyBatch` counts both. `session.receive` calls `composer.receive` and `setDraftKeys`. `setDraftKeys(keys)` marks rows by `computer + "\n" + name` and keeps the mark through later upserts.
- **Activity** exports `following`, `unseen` (events whose `seq` follows the newest one seen, while not following), `jumpToLatest()` and `focusTranscript(): boolean`.
- **Attachment rows** are keyed by their `reference` (a view's attachments are new objects every time), so a new view keeps the rows and their focus. A row reads its attachment through `keptItem(items, reference)` (`ui/sessions/composer-ui.ts`): once its reference went, it keeps its own last item until Solid disposes it, never another row's.
- **Exit flush handshake (zerus-5wmh.9.30, webui ruling 2026-10-29).** Every quit the client originates uses `exit_flush::quit_with_flush`: tray/menu-bar Quit, Cmd+Q, a non-menu-bar main-window close, signals and measurement exit. A live or hidden page gets one nonce-bearing `exit-flush` event; a background worker waits at most 150 ms, including dispatch/scheduling, for the main page's matching acknowledgement, then authorizes exactly one `ExitRequested` carrying the captured exit code. Repeats coalesce; foreign, stale and duplicate acknowledgements do not release it. No page skips the wait. The menu-bar `code: None` prevention and hide-only closes are unchanged. `exit_work` remains the single final flush and socket-release hook; the signal path's 5 s fallback and SA_RESETHAND are unchanged.
  - The page's `exitFlush` registry first calls the open composer's `flushForExit`, then acknowledges. It freezes typing during quit and awaits edits already handed to IPC, including a composer left meanwhile. The registry is the registration point for the future subagent composer (zerus-5wmh.71). Question drafts have no debounce.
  - Cmd+Q: Tauri's default menu is built, then only its last application-menu Quit item is replaced at the same position, using the exact predefined caption and CmdOrCtrl+Q. Runtime snapshots compare the complete ids/kinds/captions/order tree before and after, allowing only that item. All other items remain the same predefined native instances, including Edit's Undo/Redo/Cut/Copy/Paste/Select All, About, Services, Hide/Hide Others and Window. The locked Tauri default has no Show All; webui confirmed it stays absent here and filed Qt's Show All parity as zerus-5wmh.73. Tauri/muda expose no accelerator getter. Hand source check: locked Tauri 2.12.5 `menu/menu.rs::Menu::default` and muda 0.20 `items/predefined.rs` keep the default menu and accelerator table; only Quit is removed/reinserted. No Mac display check was run under the headless-only rule.
  - Boundary: Dock Quit, logout/shutdown, other Cocoa terminate:, Windows WM_ENDSESSION, SIGKILL and Tauri restart retain the existing Exit/exit_work fallback without this page handshake. No app.restart call exists today. The updater's restart work (.12.4/.12.6) must join this helper before restart; it must not call app.restart directly and claim the page was flushed.
- **Follow-ups ruled in the review:** the archived composer (`zerus-5wmh.9.29`), the exit flush handshake (`zerus-5wmh.9.30`) and Refresh while offline (`zerus-5wmh.9.31`).
- **For Task 7:** the bridge already has `localRetry`, `localDelete` and `localInspect`, and `ComposerView.local` is typed as `LocalMessage[]`.

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
- Flushing (Task 4b review ruling): while the view has an error, an uncertain mark or a failed local row, the editor flushes `composer_edit` on every keystroke, not 250 ms later, since Rust may then change the text itself (a reconciled delivery clears it) and Qt has no such window (its editor is the draft).
- The session status (Decisions, Task 4a ruling 1): the editor is read-only while `session_blocks_input`, with `session_status` as its tooltip (`MessageComposer.cpp:1153, 1161-1164`). Refresh (311) shows while `session_status` is shown as an error (1194) and calls a `composer_refresh` command, which runs `Composers::refresh_details` (Qt's `sessionRefreshRequested`: `refreshRequested` and `inspect()`, `SessionsWindow.cpp:1031-1034`).
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
  attachments_available: boolean; attachments_reason: string; session_status: string; session_status_error: boolean; session_blocks_input: boolean;
  cache: Chip; cache_warning: string; cache_details: string; local: LocalMessage[];
  cancel_send: boolean; compacting: boolean; clear: ClearControl }
export interface ColdChoice { action: "send" | "compact" | "clear"; label: string; enabled: boolean; tooltip: string }
export interface ColdPrompt { title: string; text: string; informative: string; choices: ColdChoice[] }
export interface StartOption { choice: "resume" | "resume_compact" | "fresh"; label: string; enabled: boolean; tooltip: string }
export interface StartQuestion { title: string; text: string; informative: string; options: StartOption[] }
export type SendReply = { kind: "sending" } | { kind: "busy" } | { kind: "blocked"; reason: string } | { kind: "notice"; text: string; error: boolean }
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
    edit: async (_h, rev, _epoch, text) => { calls.push(`edit ${rev} ${text}`); },
    pick: () => new Promise<void>((resolve) => { calls.push("pick"); attachDone = resolve; }),
    pasteImage: async (_h, bytes) => { calls.push(`paste ${bytes.length}`); },
    send: async (_h, rev, _epoch, text) => { calls.push(`send ${rev} ${text}`); return reply; },
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

  it("edits and sends carry the epoch of the text they are based on", async () => {
    const f = fakes();
    const epochs: number[] = [];
    const port: ComposerPort = {
      ...f.port,
      edit: async (_h, _rev, epoch) => { epochs.push(epoch); },
      send: async (_h, _rev, epoch) => { epochs.push(epoch); return { kind: "sending" }; },
    };
    await createRoot(async () => {
      const c = createComposer(port, f.timers);
      c.apply(view({ text_epoch: 4 }));
      c.input("hello", 5, 5);
      await c.send();
      expect(epochs).toEqual([4, 4]);
    });
  });

  it("remove and Undo lock the editor until a view with a higher epoch arrives", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view({ text: "a [File #1] ", text_epoch: 2 }));
      await c.remove(0);
      expect(c.locked()).toBe(true);
      c.input("typed while locked", 18, 18);
      expect(c.text()).toBe("a [File #1] ");
      c.apply(view({ text: "a  ", text_epoch: 2 }));
      expect(c.locked()).toBe(true);
      c.apply(view({ text: "a  ", text_epoch: 3 }));
      expect(c.locked()).toBe(false);
      expect(c.text()).toBe("a  ");
      await c.undo();
      expect(c.locked()).toBe(true);
      c.apply(view({ text: "a  [File #1] ", text_epoch: 4 }));
      expect(c.locked()).toBe(false);
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
// on blur and before any attach, paste or send, each with a growing `rev`
// and the `text_epoch` the text is based on (Rust ignores an edit over older
// text). Rust owns the attachments and the delivery state, and a growing
// `text_epoch` hands the text back (a load, an attach or remove, an Undo, a
// delivery's end, a refused change). One attach or paste runs at a time; the
// editor is read-only while it does, as Qt's synchronous attach is, and
// during a remove or Undo until a view with a higher epoch arrives.

export const EDIT_MS = 250;
export const MAX_TEXT_BYTES = 64 * 1024;
const MAX_PIXELS = 32_000_000;
const TOO_LONG = "Message exceeds 64 KiB.";
const TOO_LARGE = "Image is too large. Use an image below 32 megapixels.";
const UNREADABLE = "Could not read the pasted image.";

export interface ComposerPort {
  open(session: SessionRef | undefined): Promise<ComposerView | null>;
  edit(handle: number, rev: number, epoch: number, text: string, position: number, anchor: number): Promise<void>;
  pick(handle: number, start: number, end: number): Promise<void>;
  pasteImage(handle: number, png: Uint8Array, start: number, end: number): Promise<void>;
  send(handle: number, rev: number, epoch: number, text: string, position: number, anchor: number): Promise<SendReply>;
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
  /** The epoch a remove or Undo waits to see exceeded; the editor is locked until then. */
  const [handBack, setHandBack] = createSignal<number | undefined>();
  let rev = 0;
  let epoch = 0;
  let dirty = false;
  let timer: (() => void) | undefined;

  const handle = () => view()?.handle;
  const tooLong = () => utf8.encode(text()).length > MAX_TEXT_BYTES;
  const locked = () => pending() || handBack() !== undefined || !!view()?.attaching || !!view()?.sending || !!view()?.waiting || !view()?.loaded;

  function apply(next: ComposerView | undefined) {
    batch(() => {
      if (!next) { setView(undefined); return; }
      if (next.handle !== view()?.handle) { epoch = 0; rev = next.rev; dirty = false; setHandBack(undefined); }
      setView(next);
      if (next.text_epoch > epoch) {
        epoch = next.text_epoch;
        setHandBack(undefined);
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
    await port.edit(h, rev, epoch, text(), selection().end, selection().start);
  }

  function reply(r: SendReply) {
    switch (r.kind) {
      case "cold": setDialog({ kind: "cold", token: r.token, prompt: r.prompt }); break;
      case "start": setDialog({ kind: "start", token: r.token, question: r.question }); break;
      case "clear_context": { const control = view()?.clear; if (control) setDialog({ kind: "clear", control, token: r.token }); break; }
      case "notice": notify(r.error ? "error" : "info", r.text, true); break;
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
    /** Remove and Undo change the text in Rust: the editor stays locked until the higher epoch arrives. */
    async remove(index: number) {
      const h = handle();
      if (h === undefined || locked()) return;
      await flush();
      setHandBack(epoch);
      await port.remove(h, index);
    },
    async undo() {
      const h = handle();
      if (h === undefined || locked()) return;
      await flush();
      setHandBack(epoch);
      await port.undo(h);
    },
    async send() {
      const h = handle();
      if (h === undefined || locked()) return;
      await flush();
      reply(await port.send(h, rev, epoch, text(), selection().end, selection().start));
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
export const composerEdit = (handle: number, rev: number, epoch: number, text: string, position: number, anchor: number) => desktop<void>("composer_edit", { handle, rev, epoch, text, position, anchor });
export const composerPick = (handle: number, start: number, end: number) => desktop<void>("composer_pick", { handle, start, end });
export const composerAttach = (handle: number, paths: string[], start: number, end: number) => desktop<void>("composer_attach", { handle, paths, start, end });
export const composerRemove = (handle: number, index: number) => desktop<void>("composer_remove", { handle, index });
export const composerUndoRemove = (handle: number) => desktop<void>("composer_undo_remove", { handle });
export const composerAllowRetry = (handle: number) => desktop<void>("composer_allow_retry", { handle });
export const sendMessage = (handle: number, rev: number, epoch: number, text: string, position: number, anchor: number) => desktop<SendReply>("send_message", { handle, rev, epoch, text, position, anchor });
export const sendAnswer = (token: number, answer: Answer) => desktop<SendReply>("send_answer", { token, answer });
export const sendCancel = (handle: number) => desktop<void>("send_cancel", { handle });
export const sendInterrupted = (handle: number) => desktop<void>("send_interrupted", { handle });
export const clearContext = (handle: number, run: string, conversation: string, dismiss: boolean) => desktop<SendReply>("clear_context", { handle, run, conversation, dismiss });
export const localRetry = (handle: number, id: string) => desktop<SendReply>("local_retry", { handle, id });
export const localDelete = (handle: number, id: string) => desktop<void>("local_delete", { handle, id });
export const localInspect = (handle: number, id: string) => desktop<Inspect | null>("local_inspect", { handle, id });
```

`normalize` keeps `composer` and `draft_keys`, and `isEmptyBatch` treats a batch with either as not empty. Rust sends each view and each key set once (`take_view`, `take_draft_keys`), so an envelope the pump discarded is not resent.

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

**As built** (`1b45b750` and its review fix round 1, `7da83390`). Where the steps below differ, the code and this list win (Qt wins over the brief; `ActivityView.cpp`, `SessionsWindow.cpp` and `MessageComposer.cpp` lines):
- **The Rust side is `composer/saved.rs`**, not `mod.rs`: `DraftSource`, the items, `saved_drafts`, `saved_draft`, `import_conflicts`, `recover`, `restore` and the five commands. `Config`, `qt_store` and `dev_stores` stay in `mod.rs`, and `Composers::new` applies `qt_store` again, so a caller cannot hand the hub an overlapping Qt root.
- **`qt_store`** reuses `drafts::import_sources` (the switch import's rule), which compares a folder not created yet by its longest existing ancestor. The brief's `canonicalize`-or-raw comparison misses `own/inner` when the temp folder is a symlink (macOS `/tmp`).
- **`DraftSource` has a third value, `Dev`.** The switch import's conflicts (`<drafts root>/.imported-v1.json`, zerus-5wmh.13.1) name Qt's store or the dev app's, so `Config` also has `dev_drafts` (`drafts::dev_roots()` without the client's own root; empty in a replay). Dev drafts are only read: their preview and Restore. `saved_drafts(Dev, ..)` lists nothing.
- **The conflicts** are a third section, "Not imported at the switch", with the note "A different draft was already here, so these copies stayed where they were.". The command is `draft_conflicts(limit, after) -> ConflictsPage { items, next: Option<usize> }`. It reads the marker (at most 16 MiB) on each page, drops repeated entries and unknown sources, and pages by offset (1-200). A row's detail is "Kept in the previous Zerus app" or "Kept in the development app".
- **`SavedDraftsPage.next` is a string token** (`"{modified_ns}:{folder}"`, `cursor_token`/`cursor_of`), not a `Cursor`. A cursor's nanoseconds exceed what a JavaScript number holds exactly, so the next page would skip or repeat drafts. An invalid token is an error.
- **`SavedDraftItem`** also has `source` and `archived`. Qt over the brief (`showSavedDrafts`, 1257-1300):
  - a subagent's caption is "{caption} / {subagent}" (1287);
  - the caption is everything after the second slash (`section('/', 2)`), not only the third part;
  - `meta` is the session alone while there is no context (1294).
- **`saved_draft`** reads a draft the hub holds loaded before the store (Qt reads `m_preservedDrafts` and `m_drafts` first, 1331-1333):
  - a draft being sent shows "This message may already have been sent. Check Activity or Terminal before sending again." (1310-1313);
  - a Retry that keeps the draft (`Composer.kept`) shows the draft as it was.
  - A draft read from disk that was saved while sending shows the core's "Delivery was interrupted…" (Qt's `loadDirect`, `ComposerDraftStore.cpp:178-180`), not the brief's "may already have been sent".
  - The voice rule reads the form state's `kind == "voice_message"` and its `session`, as Qt does (1318-1323), not the key's owner kind.
  - The detail is a storage error, else the voice text, else "{n} attachment(s)" and the notice joined by " / ".
- **`recover(key)`** answers a copy Qt could not read completely with its `NOT_RESTORED` text and writes nothing. A composer held here with no draft of its own takes the recovered draft and saves it again, so an empty save queued meanwhile cannot leave the store empty. The cards' draft mark follows (`stored_keys`).
- **`restore(handle, source, key)`** refuses with `RESTORE_LATER` unless `handle` is the open composer and `editable` (loaded, not busy, not quitting), before and after its read. It keeps the selected draft's doubt: an uncertain draft stays uncertain until Allow retry. A Qt source is recovered here first; `ALREADY_EXISTS` or any other failure of that copy does not stop the restore.
- **Local rows** (`transcript.ts` `mergeLocal`, `localItem`) follow `ActivityView.cpp:665-720`:
  - a sent row takes over its own prompt event: the one with exactly its text when it has files and no other sent row competes, else the nearest prompt within 10 s that starts like it. The event then shows the row's text and files.
  - A voice row the journal holds is left out.
  - The others are placed by `submitted_at` against each event's `at` (Qt compares the exact times), after events of the same time.
  - **Qt over the brief:** the stamp replaces the time (`at` is ""), as `stamp=` replaces `timeText` (1053-1055).
  - `LocalItem` also has `available` per file: Qt's `request_id || local_path` rule (765). Activity's journal cards (Task 9's `cardTarget`) read a file by its request only; a local file without either shows "Attachment: {name}" (780).
- **The composer store** has `localRetry`, `localDelete` and `localInspect` (`ComposerPort` too). Retry flushes, calls `local_retry` and answers through Task 6's `reply(r, h, at, false)`, so a reply after another composer opened keeps only its notice and its question answers Cancel. One Retry per row is on its way at a time. Check answers nothing once its composer went.
- **Activity** reads the rows through `localRows(source, selected)`: none for a read-only source, a subagent, or a composer view of another draft key. The buttons are Retry, Check Terminal or Check native UI, and Delete.
  - Check's routes are in `local-actions.ts`: `terminal` opens and connects the Terminal tab (Qt's `connectSession`), `native_ui` shows "Arrives with Native UI." until Task 14, and `refresh` shows its notice.
  - **Code over Qt:** Delete asks "Delete message?" / "Remove this message from Activity? This cannot be undone." with Cancel focused and the default (ruling: destructive actions confirm). Qt deletes at once (4476). The dialog uses Qt's `trash` glyph in the danger colour. Cancel returns the focus to the row's Delete, and Delete moves it to the transcript.
  - Retry stays focusable while off and names why: "Retry is available once the current message is sent." while the composer sends or waits, and "Retry is available once the composer is ready." while it is otherwise locked (`local-actions.ts` `retryReason`). Qt's link returns quietly then (4483).
  - The error line is Qt's composition, unchanged: "Delivery is not confirmed. Check Terminal before retrying. {error}" is one string (`SessionsWindow.cpp:4937`) in one paragraph (`ActivityView.cpp:1056`). It is `--fs-sm`, in the warning colour mixed 78% toward the text colour, so it keeps its contrast on the tinted bubble in every skin.
- **Saved drafts** (`store/savedDrafts.ts`, `ui/sessions/SavedDrafts.tsx`):
  - pages of 100; Show more reads the next page on request, and a page never repeats a row the section shows. Closing drops every row and any late answer.
  - The list is one `listbox` of options grouped by section (`role="group"`). Show more is a button per section below the list ("Show more on this computer", "Show more from the previous Zerus app", "Show more not imported at the switch"; "Loading…" while it reads).
  - Keys: ArrowUp and ArrowDown by one, Home and End, PageUp and PageDown by a list's height (`listStep`). Enter goes to the session (a double click's equivalent). The list takes the focus once its first draft is selected after opening.
  - The detail line is `aria-live="polite"`; Recover's result is `role="status"`, its error `role="alert"`; Recover reads "Recovering…" while it runs.
  - Copy text, Restore in current session, Go to session and Recover stay focusable while off (`aria-disabled`) and name their reason as their tooltip (`REASONS`: "Select a draft.", "Loading draft…", "This draft has no text.", "Select a session to restore a draft into it.", "Another session opened after Saved drafts. Open Saved drafts again to restore into it.", Rust's `RESTORE_LATER`, "Wait for the current action to finish.", and a draft's own detail for a storage error or a voice message).
  - With no draft the dialog is Qt's hint ("No saved drafts on this desktop.") and its buttons only, 480 px wide and as tall as its content. The list takes three parts of the height and the text one.
  - The focus returns to the Saved drafts button when the dialog closes (`openSavedDrafts(button, handle)`), and to Restore when "Replace current draft?" is cancelled.
  - The first draft listed is selected once (Qt's `setCurrentRow(0)`), and a late preview of an earlier selection is dropped (Qt's `selection`).
  - Sections show only with rows. Qt's Close is the default button and has the focus. Rows show the caption, machine, meta, detail and "d MMM HH:mm", and a double click is Go to session.
  - "Replace current draft?" asks when the editor holds text or files and the selection is not that very draft. A Qt draft of the same key is another draft, so it asks. Cancel is focused and the default. **Review over Qt:** the key's empty parts are left out ("claude/sample-project/review", not Qt's " / claude/sample-project/review" for this computer).
  - Go to session is Qt's `draftOwner`: not unknown, not archived, a non-archived row of that name, the key's alias first, else a subagent key's machine name. It applies to every kind with a session, task prompts included. It is found at selection and again on click, then the dialog closes and `openSession` shows the row (Qt's `showSession`, which clears the filters).
  - Recover shows for Qt rows, Qt conflicts included. After a recover, "On this computer" is read again and the dialog says "Recovered to this computer.".
  - Copy text uses the page's clipboard: `copy_text` refuses a line break.
  - The dialog opens from the session list's Saved drafts button, for the open composer, or for none while a subagent shows (Qt picks the subagent composer, 488).
- **The session menu's Clear session** (Task 5's As built) opens `composer.openClear()` for the selected row only, with `clear.enabled` and `clear.reason` (5339-5340). A row whose state changed since the menu opened shows "The session state changed. Open its actions again." (5487-5488).
- **The restore notice** is "The restored conversation could not be verified. Your archived draft remains in Saved drafts." (3331). The client's follow-up moves no draft: its draft stays under its archive key (now editable there after zerus-5wmh.9.29).
- `dialog.dlg` has `max-width: calc(100vw - 48px)`, so an inline width never exceeds the window.
- **Follow-ups:** Go to session's simplifications (filters, subagent, Tasks, Qt's row roles) are `zerus-5wmh.9.34`; Clear session's "Checking whether this session can be cleared…" for an unselected row is `zerus-5wmh.9.35`; restoring an archived draft waits for `zerus-5wmh.9.29`.
- **Tests:** src-tauri 479 (18 in `composer::saved`; the recovered draft's save is tested with the store's writer held, not a wait), vitest 1330. Hand mutations: 14 Rust and 27 TypeScript, all killed.

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
        c.edit(view.handle, 1, view.text_epoch, "first line\nsecond".into(), 0, 0);
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

# Part A2: question answers and sending while the agent works

The coordinator ruled on 2026-10-28 that two features block the switch, because the owner uses them every day in the Qt app:
- **Question answers** (`zerus-5wmh.9.23`): Claude, Codex and Kimi question cards and approvals are answered from the app.
- **Sending while the agent works** (`zerus-5wmh.9.24`): the agent's native queue, Send now and Send and push now.

Part A2 is on the bar. Its tasks follow Task 7 and come before Task 12, which runs the smoke for both parts. Two requests go to the core owner, R7 and R8. The webui tasks bind to them and to Tasks 3a-7.

**The ownership split** (fixed):
- The **events session** owns the pure core in `client/core`: parsing, view models, and the guarded actions with their lanes in `actions::dispatch`. This is R7 and R8.
- **webui** owns the `client/src-tauri` commands and hub state, and the SolidJS UI. These are Tasks A2-1 to A2-6.

**Estimate (2026-10-28), in working days:**
- **Core (events session):** R7 about 2, R8 about 1. They are independent and can run in parallel. Started by 2026-10-30, both land before webui reaches Part A2 (after Task 7), so they stay off the critical path.
- **webui:** 5.5 in all:
  - A2-1: 1; A2-2: 1; A2-3: 1.5;
  - A2-4: 1; A2-5: 0.75;
  - Task 12's additions: 0.25.

  A2-6 adds 0.5 if the owner puts it on the bar.
- **The switch** was previously about 2026-11-05. With Part A2 it is:
  - about 2026-11-13 with one webui implementer;
  - about 2026-11-09 when a second implementer runs the A2 chains beside Tasks 8-12 once Task 7 is merged (see "A2 tasks").

## A2 behaviour (Qt)

Verified on main `99835752`. Qt wins: where this section and Qt disagree, fix this section.

**Where questions come from.**
- They come from the inspection's `pending_questions`, not from Activity events. The client already receives the inspection as the followed session's Activity `state` value (`src/watch/activity.rs:115-123`; only `events`, `processes`, `process_work`, `cursor`, `history_truncated` and `replace_events` are removed), and Task 4a keeps it in `Followed.details`.
- Each card is `{question_id, question_hash, run_id, conversation_id, source, answer_transport, created_at, optional, can_answer, can_skip, approval, approval_choices, trust_request, plan_review, answer_unavailable_reason, answer_delivery, questions: [{id, question, header, body, multi_select, allow_other, required, other_label, other_description, options: [{id, label, description, scope}], default_option_ids, default_text, min_items, max_items, max_length}]}` (`src/state/questions.rs:34-111, 394-462`, `src/state/claude_approval.rs:257-269, 381-383`).
- `pending_questions_error` names a failed read. Qt then shows "Could not load agent questions: {error}. Check Terminal." in the session hint (`SessionsWindow.cpp:3913-3918`).

**Which card shows.** `renderQuestion` (`SessionsWindow.cpp:5003-5068`) walks `pending_questions` in order and skips four kinds of entry:
- one without an id;
- a repeated id;
- one without questions that can be answered (`can_answer` missing counts as true);
- a Codex optional question whose answer was already submitted.

Of the rest:
- `pending` counts each card's questions, at least one per card.
- The first required card shows. A later required card that can be answered replaces one that cannot.
- Without a required card, the optional cards are sorted newest first by `created_at` and one of them shows. The user's choice among them is remembered per (session, run, conversation) and survives polls.
- A required card **hides the composer's editor and action row**. The toolbar stays (`MessageComposer::setInputVisible`, `MessageComposer.cpp:1126-1131`). An optional card keeps the composer.
- The card sits in the Activity column, between the Activity view and the composer (`SessionsWindow.cpp:918, 1030`).

**Availability.** The first rule that holds disables the card with its text (`SessionsWindow.cpp:5054-5065`):
1. "Machine unavailable. Your answers stay here."
2. "This session is not running."
3. "Wait for the session update to finish." (terminating, an update or a rename)
4. "Refresh the session before answering." (the last inspect failed)
5. "Refresh the pending question before answering." (the card's run or conversation is not the details')
6. "The agent is no longer running." (`runtime_state` not `live` or `process_state` not `running`)

**The card** (`QuestionCard.cpp`):
- **Heading.** "Agent needs your approval" for an approval, a trust request or a plan review. Otherwise "Optional question", or "Optional question #i/n" while several optional cards wait, or "Agent needs your answer" (805-807).
- **Progress.** "i of n answered", or "Awaiting agent" after a submission. Hidden for approvals (828-829).
- **Metadata row** (380-385, 396-405):
  - "Asked 28 Oct 14:03:05" (with the year when it is not this year);
  - "n pending", with the tooltip "Questions waiting for an answer, including the questions shown in this card.";
  - for several optional cards, Previous and Next arrows with "Previous queued question (newer)" / "Next queued question (older)" and "Request i of n, newest first. Draft answers are kept when switching.".
- **Tabs.** One per question, only when there are several. The title is "Question i", or "i: {header, at most 32 characters}", with a "✓ " prefix once the question is filled (625-627, 818-827).
- **Text and Markdown.** The prompt and body are Markdown for authored sources (`hook`, `codex_async`, `kimi_wire`, `kimi_plan_review`, `dsh_native`, `opencode_native`, `claude_native_question`). Approvals, trust requests and any other source show plain text, and an approval's body is monospace (515-519, 561-577).
- **Mode line.** "Select one" or "Select one or more". OpenCode bounds give "Select {min} to {max}", "Select at least {min}" or "Select up to {max}". It is hidden for plan reviews (578-580, 623).
- **Option rows.** A radio button or a check box, with the label and the description:
  - A `scope: broader` option adds "Applies beyond this request. Asks for confirmation." to its description (587-591).
  - Approvals without `approval_choices` hide their synthesized rows: Approve once and Deny stand in for them (582-584, 599).
- **Other.** "Other" (or the provider's `other_label`) with "Write your own answer" (or `other_description`) (602-612).
- **The free-text line.** Placeholder "Write your answer…". It shows when the question has no options or Other is chosen. Its bound is 4096 UTF-16 units, or OpenCode's `max_length` in characters (613-621, 785-786).
- **Custom answer checks** (104-113):
  - "Keep your custom answer under 4 KiB.";
  - "Use one line for a custom answer. Multiline input is available in Terminal.";
  - "Remove control characters from your custom answer.";
  - "Use Terminal for answers starting with / or !." (except OpenCode form replies).
- **Footer** (211-236, 793-817):
  - Open Terminal, or "Open native UI" for DeepSeek;
  - Skip, with the tooltip "Dismiss this optional question without sending an answer. The agent can continue.";
  - Allow retry, with the tooltip "Check the agent in Terminal before retrying to avoid submitting twice.";
  - "← Back" and "Next →";
  - Submit, labelled "Submit answer", "Submit answers", "Submitting…", "Submitted", "Confirm" or "Submit answer (3)";
  - Deny and "Approve once (3)".
- **Status line** (830-854). The first of these that applies:
  1. the storage error;
  2. the draft's notice;
  3. the unavailable reason;
  4. the card's own `answer_unavailable_reason`;
  5. "Answer this request in Terminal.";
  6. "Loading your saved answer…";
  7. the first custom answer problem;
  8. "“{label}” changes the agent's permissions beyond this request. Select Confirm to send it.";
  9. "Review the request, then choose one of the agent's options.";
  10. "Review the plan, then choose how the agent continues.";
  11. "Approval applies only to this command. Review it before continuing." for an approval;
  12. "The agent can continue. Answer when ready or skip this question." for an optional card;
  13. "Your choices are ready to send." when every question is answered;
  14. "Answer each question to continue.".

  The error colour applies for an error or a storage error.
- **The review delay** (237-246, 793-817). An approval's Submit (with native choices) and Approve once stay disabled for 3 s of review, counting "(3)", "(2)", "(1)". The count runs only while the card is visible, the window is active and the card is available. Otherwise it starts over. Deny is never delayed.
- **The second Confirm** (916-941, 846-847). Submitting a broader option the first time only arms Confirm. The next Submit sends it. Any edit disarms it.
- **Enter** (898-914). Enter on an option row clicks Submit, never on auto-repeat, so holding Enter cannot confirm or resend. Enter in the free-text line submits too.
- **Read-only cards** (779-780). A card with `can_answer` false from a Terminal-only source keeps its options disabled.
- **Drafts** (282-309, 676-739):
  - The draft key is `question\n` + `["{session key}","{question_id}","{question_hash}"]` (compact JSON) + `\n{run_id}\n{conversation_id}`.
  - The label is "{session key with ' / '} (question answer)".
  - `form_state` is `{answers: [{id, text, other, options}], page, answered, submitted}`.
  - The draft text joins the free-text answers, so Saved drafts lists it.
  - Untouched OpenCode defaults are not saved (523-551, 719-720).

**The answer path** (`answerQuestion` and `finishQuestion`, `SessionsWindow.cpp:5070-5186`):
1. **Quiet refusals.** Nothing happens while a cold-cache dialog of an answer is open, while an answer of this session is in flight, or when the session is not the selected one, offline, not running, updating, failed to inspect, or not `live`/`running`.
2. **A changed card.** When the card is gone, can no longer be answered (or skipped), or its run or conversation differs from the details', the card shows "This question changed. Refresh before answering.".
3. **A cold cache.** It asks "Submit with a cold cache?", with the cache warning, a blank line and "Submitting this answer may resume the agent with the entire conversation, increasing token usage, cost or subscription usage. Compaction or clearing can invalidate the pending question. Cancel keeps your answers.". The buttons are "Submit with full context" and Cancel, with Cancel the default and Escape. Kimi's own cache panel (`kimi_cache_hint` with `kimi_tui`) skips the dialog. After the dialog, any change shows "The question or your answers changed. Your draft was kept; review it before submitting again." as an error notice and sends nothing:
   - in the session's identity;
   - in the card's consent identity (`question_id`, `question_hash`, `run_id`, `conversation_id`, `questions`, `source`, `tool`, `tool_name`, `tool_call_id`, `native_question_id`, `agent_id`, `answer_transport`, `approval`, `approval_choices`, `trust_request`, `optional`);
   - in the draft.
4. **The durable save.** The draft is saved durably as sending ("Saving before submitting…", `prepareSendingAsync` 428-445). Only its callback can continue.
5. **The re-check.** The callback checks everything again, on the details or the evidence captured at the click (`sendEvidence`, 138-145), and on the consent identity. On any change the card shows "The question changed. Your answers were kept without submitting." and nothing is sent.
6. **The call.** `hgs [@host] answer NAME --json` (`HgsClient.cpp:1872-1959`):
   - **Standard input:** `{request_id, question_id, expected_question_hash, expected_run_id, expected_conversation_id, answers}`, with a fresh UUID per attempt.
   - **Answers:** `{question_id, selected_option_ids, text}` per question; Skip sends `{question_id, skip: true}`; Approve once and Deny send `{question_id: first question, selected_option_ids: ["allow" | "deny"], text: ""}`.
   - **Timeouts:** 30 s locally and 45 s on a peer. One answer per session at a time ("An answer to this session is already being sent.").
   - **Refusals before the call:**
     - "This request is not ready to answer. Wait for an updated request or open Terminal." when an identity or the answers are missing, or the card cannot be answered or skipped. A conversation may be empty only for the recognized startup requests: folder trust, Claude's permission mode, Codex's hooks review and Codex's update.
     - "The answers exceed 64 KiB.".
   - **Outcomes:**
     - a failed start is definite: "Cannot start hgs: …";
     - a timeout is uncertain: "Answer delivery timed out. Check Terminal before retrying.";
     - a nonzero exit is uncertain unless it is a normal exit 1 without "delivery uncertain". The detail is stderr (at most 4000 characters) or "Answer transfer failed (exit {code}).";
     - more than 64 KiB of output is uncertain: "The answer acknowledgement is too large. Check Terminal before retrying.";
     - a receipt that does not echo `request_id`, `name`, `question_id`, `question_hash`, `run_id` and a string `conversation_id` is uncertain: "The answer acknowledgement is invalid. Check Terminal before retrying.";
     - success is `status: answered`, or `submitted` for a Codex optional answer that is not a skip.
7. **After the answer** (447-478, 1156-1186):
   - `answered` shows "Answer sent. Waiting for the agent…" ("Response recorded." for an optional card).
   - `submitted` shows "Answer submitted. Waiting for Codex to record it. You can check the queue above or open Terminal.".
   - A definite failure shows its detail.
   - An uncertain one shows "Delivery is not confirmed. Check Terminal before retrying. {detail}" and keeps Submit disabled until Allow retry.
   - The session is inspected again.
   - Codex's hooks review and Update now open the Terminal tab when the receipt says `open_terminal`.
   - Update now also shows "Codex in {name} ({machine}) is installing its update in Terminal. Resume the session after Codex exits." or "… exited after its update. Check the result in Terminal, then resume the session.".
8. **Answered elsewhere.** An answer in Terminal, the phone or another desktop removes the card from `pending_questions`. The next state hides the card and returns the composer. A Codex answer recorded as submitted (`answer_delivery`) is never offered again, even when a stale poll lists it (311-368). If the card changed between the click and the call, the CLI refuses with "question changed; refresh before answering" (exit 1, definite).
9. **Unknown never replays.** The CLI keeps a receipt per request id and answers "delivery uncertain: … do not resend automatically" for an id it saw before (`src/state/questions.rs:695-711`). Qt makes a new id per attempt, and only after Allow retry.

**Sending while the agent works.**
- **Send.** Send already works while the agent is busy. The block reasons let a busy agent in `idle`, `working`, `tool` or `compacting` take messages (`messageBlockReason` 4425-4430, S1 on main). `hgs send` types them into the agent's TUI, which queues them natively.
- **The queue panel.** The inspection's `input_queue` is `{id, text, can_send_now, hint}` (`src/state/input_queue.rs:108-124`):
  - It exists for Claude, Codex and Kimi while they are busy and their panel shows queued input.
  - `can_send_now` is false while compacting, while the Terminal input is not empty ("Return Terminal to an empty input before sending the queue now"), and for Codex's ordinary follow-ups ("Use Terminal to edit or submit these queued follow-ups").
- **"Queued in agent."** Activity shows the panel at the bottom of the transcript column when the session is tracked and a queue id exists (`ActivityView.cpp:1280-1292, 1890-1895`):
  - the caption "Queued in agent";
  - the queue's text, plain and selectable, at most 100 px high, with the whole text as tooltip;
  - a **Send now** button, labelled "Sending…" while a send-now runs, enabled when `can_send_now` holds and none runs. It is disabled when offline or after a failed inspect (`SessionsWindow.cpp:3976-3980`). Its tooltip is the queue's `hint`; Qt appends the `queue.sendNow` chord, which the owner rule drops.
- **Send now** (`sendQueuedNow`, `SessionsWindowShortcuts.cpp:248-260`):
  - One runs at a time in the whole app.
  - It needs the selected session online, the queue id unchanged and `can_send_now`; otherwise "Nothing queued can be sent now".
  - The call is `hgs [@host] send-now NAME --json` with `{request_id, expected_run_id, expected_conversation_id, queue_id}`, 20 s locally and 35 s on a peer (`HgsClient.cpp:2235-2290`). Success is `status: submitted` echoing all five.
  - Texts: "Queue action timed out. Check Terminal before retrying.", "Queue action failed. Check Terminal before retrying.", "Could not confirm sending the queue. Check Terminal before retrying." and "Invalid queue acknowledgement. Check Terminal before retrying.".
  - After it, a still-selected session hides the panel until the next inspection, shows "Send now requested. Waiting for the agent to accept the queue." or the error, and is inspected again (`SessionsWindow.cpp:1859-1873`). A stale queue shows "The native queue is no longer visible; nothing was sent. Activity is being refreshed; check Terminal or the current queue before retrying." as a transient error.
  - The CLI re-reads the screen and sends only the agent's own shortcut, never the text (`src/state/input_queue.rs:125-158`).
- **The keyboard.** `queue.sendNow` is a window chord, Ctrl+Shift+Return (Command+Shift+Return on macOS) (`Shortcuts.cpp:40, 61`; `SessionsWindowShortcuts.cpp:242`).
- **Send and push now** (`message.push`, `MessageComposer.cpp:399-406, 811-847`; `SessionsWindowShortcuts.cpp:265-341`) is a composer chord only: Control-X Control-S on macOS (Qt's Meta+X, Meta+S) and Ctrl+X, Ctrl+S elsewhere (`Shortcuts.cpp:41, 70`). There is no button.
  - With an empty field (or while a message is on its way) it is Send now.
  - Otherwise, when Send is enabled, it arms a push for the selected session with the typed text, then sends as usual.
  - The push records the send's request, run and conversation (`deliverMessage` 4910-4912), and becomes confirmed when that send succeeds.
  - On each later inspection of that session, the first queue snapshot whose items hold the message's start (`queueHolds` 95-161: the leading `[Image #n]`/`[File #n]` references and the first 24 non-space characters) and can be sent now is sent with Send now. A push never sends text.
  - It ends with a notice when:
    - the session changes: "The message was not pushed: the session changed";
    - the send fails: "The message was not pushed: sending did not complete";
    - the send is uncertain: "The message was not pushed: delivery is not confirmed";
    - the message has no text: "Sent without push: a message without text cannot be found in the agent's queue";
    - another push replaces it: "The earlier push was replaced by a new one";
    - another session is selected: "The message to {title} was not pushed: another session was selected", or "… its session is no longer selected";
    - 20 s pass: "The message was sent without push: the agent did not offer to send it now within 20 seconds", "The message will be sent without push: it was not delivered within 20 seconds" or "The message was not pushed: it was not sent within 20 seconds".
- **Edit, reorder and remove.** Qt has no edit or reorder of a native queue; the CLI marks Codex's ordinary follow-ups read-only. Remove exists only for OpenCode's inbox (`ActivityView.cpp:1034-1038, 1069-1087`; `SessionsWindow.cpp:4433-4467`):
  - a user message with `delivery_state` `queued` or `not_delivered` stamps "Queued" or "Not delivered";
  - a not-delivered one adds "Still waiting in OpenCode's queue. It runs first with your next message unless you remove it.";
  - while the session allows `remove_queued`, it offers Remove. That asks "Remove queued message" / "Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it." with Remove and Cancel (the default), and runs `hgs session-action NAME --json` with `action: remove_queued`.
- **Activity labels** (`ActivityView.cpp:1029-1046, 975-981`):
  - "You (queued)" for `UserPromptQueued`;
  - "You (answer)" for `QuestionAnswered` and for Codex question replies (`<send_user_message_question_reply>`, `QuestionReply.h`), shown as "Question" and "Your answer" blocks;
  - "You (recorded answer)" for a recorded prompt that holds replies.

  Session cards show "Your answer: …" in place of such a prompt (`SessionPresentation.h:314-327`).

## Requests to the core owner: R7 and R8

Both requests follow the rules of PR #404 (`Action::Send`):
- Every new action is `#[serde(skip_deserializing)]` and is built in Rust from held state. Webview JSON can never produce it.
- Every action keeps the request identity, the expected run and conversation, and Qt's texts.
- An `Outcome::Uncertain` is never retried.
- serde names are snake_case.
- All new structs derive `Clone, Debug, Default, PartialEq`.
- The view structs that cross the IPC also derive `Serialize`: `CardView` and its parts, `ColdAnswer`, `Reply`, `QueueView` and `Notice`. The composer view carries them and compares them.

### R7. Question cards and the guarded answer (`zerus-5wmh.9.23`)

**Files:**
- `client/core/src/questions.rs` (new, `pub mod questions` in `lib.rs`);
- `client/core/src/actions.rs` (`Action::Answer`);
- `client/core/src/view.rs` (answer previews on the cards);
- `client/src-tauri/src/actions.rs` (`Lane::Answer`, as #404 added `Lane::Send`);
- `client/core/tests/questions.rs`.

**Qt reference:**
- `tray/src/QuestionCard.cpp` (all of it);
- `tray/src/SessionsWindow.cpp`: 138-154, 1131-1186, 3913-3918, 5003-5186;
- `tray/src/HgsClient.cpp:1872-1959`;
- `tray/src/QuestionReply.h`;
- `tray/src/ActivityView.cpp:347-363, 975-981, 1029-1031`;
- `tray/src/SessionPresentation.h:314-327`.

The CLI side:
- `src/state/questions.rs:34-111, 394-485, 659-913`;
- `src/state/claude_approval.rs:257-269, 381-383`.

**Surface:**

```rust
// client/core/src/questions.rs
pub const REVIEW_MS: u64 = 3000;
pub const STORE_PREFIX: &str = "question\n";
pub const CHANGED: &str = "This question changed. Refresh before answering.";
pub const CHANGED_KEPT: &str = "The question changed. Your answers were kept without submitting.";
pub const CONSENT_CHANGED: &str = "The question or your answers changed. Your draft was kept; review it before submitting again.";

pub struct Choice { pub id: String, pub label: String, pub description: String, pub broader: bool }
pub struct Item {
    pub id: String, pub question: String, pub header: String, pub body: String,
    pub multi: bool, pub allow_other: bool, pub required: bool,
    pub other_label: String, pub other_description: String, pub options: Vec<Choice>,
    pub default_option_ids: Vec<String>, pub default_text: String,
    pub min_items: usize, pub max_items: usize, pub max_length: usize,
}
/// One `pending_questions` entry. `raw` is the object as listed: the answer pins it.
pub struct Question {                       // Clone, Debug, Default, PartialEq (not Serialize)
    pub id: String, pub hash: String, pub run_id: String, pub conversation_id: String,
    pub source: String, pub transport: String, pub created_at: f64,
    pub optional: bool, pub can_answer: bool, pub can_skip: bool, pub approval: bool,
    pub approval_choices: bool, pub trust_request: bool, pub plan_review: bool,
    pub unavailable_reason: String, pub items: Vec<Item>, pub raw: Value,
}
pub fn pending(details: &Value) -> Vec<Question>;
pub fn questions_error(details: &Value) -> Option<String>;

pub struct Selection { pub question: Option<Question>, pub pending: usize, pub optional: Vec<String>, pub index: usize, pub chosen: Option<String> }
impl Selection { pub fn input_visible(&self) -> bool; }
pub fn select(questions: &[Question], submitted: &dyn Fn(&Question) -> bool, chosen: Option<&str>, navigation: i32) -> Selection;

pub struct AnswerFacts { pub online: bool, pub state: String, pub updating: bool, pub inspect_error: bool }
pub fn unavailable(facts: &AnswerFacts, question: &Question, details: &Value) -> Option<&'static str>;

pub fn store_key(session_key: &str, question: &Question) -> Option<String>;
pub fn label(session_key: &str) -> String;

pub struct ItemAnswer { pub options: BTreeSet<String>, pub text: String, pub other: bool }
#[derive(Deserialize)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum Edit { Option { item: String, option: String, checked: bool }, Other { item: String, checked: bool }, Text { item: String, text: String } }
pub struct AnswerDraft {                    // Clone, Debug, Default, PartialEq
    pub answers: BTreeMap<String, ItemAnswer>, pub seeded: BTreeSet<String>,
    pub notice: String, pub confirm: String, pub storage_error: String, pub label: String, pub page: usize,
    pub sending: bool, pub submitted: bool, pub answered: bool, pub error: bool, pub uncertain: bool,
}
pub struct Snapshot { /* answers, page, confirm, sending, submitted, answered, uncertain, storage_error */ }  // PartialEq
impl AnswerDraft {
    pub fn from_loaded(loaded: &drafts::Loaded, label: &str) -> Self;
    pub fn stored(&self) -> drafts::StoredDraft;
    pub fn locked(&self) -> bool;                           // sending || submitted || answered
    pub fn seed(&mut self, question: &Question);
    pub fn restore_submitted(&mut self, question: &Question) -> bool;
    pub fn apply(&mut self, question: &Question, edit: Edit) -> bool;
    pub fn confirm_step(&mut self, question: &Question) -> bool;
    pub fn begin_sending(&mut self);
    pub fn sending_failed(&mut self);
    pub fn set_error(&mut self, detail: &str, uncertain: bool);
    pub fn set_answered(&mut self, optional: bool);
    pub fn set_submitted(&mut self);
    pub fn allow_retry(&mut self);
    pub fn snapshot(&self) -> Snapshot;
}

#[derive(Serialize)] #[serde(untagged)]
pub enum AnswerItem { Skip { question_id: String, skip: bool }, Choice { question_id: String, selected_option_ids: Vec<String>, text: String } }
pub fn answers(question: &Question, draft: &AnswerDraft) -> (Vec<AnswerItem>, bool);
pub fn skip_answers(question: &Question) -> Vec<AnswerItem>;
pub fn decision(question: &Question, allow: bool) -> Option<Vec<AnswerItem>>;
pub fn custom_answer_problem(text: &str, form_reply: bool) -> Option<&'static str>;
pub fn broader(question: &Question, draft: &AnswerDraft) -> Option<String>;
pub fn same_form(old: &Question, new: &Question) -> bool;

pub struct CardContext { pub available: bool, pub unavailable_reason: String, pub loading: bool, pub in_flight: bool, pub pending: usize, pub queue_index: usize, pub queue_count: usize }
pub struct ButtonView { pub visible: bool, pub enabled: bool, pub label: String, pub review_label: Option<String> }
pub struct OptionView { pub id: String, pub label: String, pub description: String, pub markdown: bool, pub checked: bool, pub enabled: bool, pub visible: bool }
pub struct TextView { pub visible: bool, pub enabled: bool, pub value: String, pub placeholder: String, pub accessible_name: String, pub max_units: usize, pub max_chars: usize }
pub struct FormView { pub id: String, pub tab: String, pub prompt: String, pub body: String, pub markdown: bool, pub monospace_body: bool,
                      pub multi: bool, pub mode: String, pub mode_visible: bool, pub options: Vec<OptionView>, pub other: Option<OptionView>, pub text: Option<TextView> }
pub struct QueueNav { pub previous: bool, pub next: bool, pub previous_tip: String, pub next_tip: String }
pub struct CardView {
    pub heading: String, pub progress: String, pub pending: String, pub pending_tip: String, pub nav: Option<QueueNav>,
    pub tabs_visible: bool, pub page: usize, pub forms: Vec<FormView>, pub status: String, pub status_error: bool,
    pub available: bool, pub locked: bool, pub review: bool, pub created_at: f64,
    pub submit: ButtonView, pub approve: ButtonView, pub deny: ButtonView, pub skip: ButtonView,
    pub previous: ButtonView, pub next: ButtonView, pub retry: bool, pub skip_tip: String, pub retry_tip: String,
}
pub fn card(question: &Question, draft: &AnswerDraft, ctx: &CardContext) -> CardView;

pub enum Actionable { Ready(Question), Silent, Changed }
pub fn actionable(details: &Value, facts: &AnswerFacts, question_id: &str, skipping: bool) -> Actionable;
pub fn consent_identity(question: &Value) -> Value;
pub struct Recheck<'a> {
    pub pinned: &'a Value, pub question: &'a Question, pub evidence: &'a Value, pub captured: &'a Value,
    pub row_same: bool, pub row_identity: bool, pub facts: &'a AnswerFacts,
    pub skipping: bool, pub consent: bool, pub submitted: bool, pub now: f64,
}
pub fn still_answerable(r: &Recheck<'_>) -> bool;
pub struct ColdAnswer { pub title: String, pub text: String, pub proceed: String }   // Serialize
pub fn cold_answer(details: &Value, question: &Question, now: f64) -> Option<ColdAnswer>;
pub struct FollowUp { pub open_terminal: bool, pub notice: Option<String> }
pub fn answered_follow_up(question: &Question, receipt: &Value, open_run: Option<&str>, target: &str) -> FollowUp;

pub struct Reply { pub question: String, pub answer: String }                       // Serialize
pub fn replies(text: &str) -> Vec<Reply>;
pub fn replies_preview(replies: &[Reply]) -> String;
pub fn annotate_replies(payload: &mut Value);

// client/core/src/actions.rs
#[serde(skip_deserializing)]
Answer { question: Value, answers: Vec<questions::AnswerItem> },
pub const ALREADY_ANSWERING: &str = "An answer to this session is already being sent.";
// client/src-tauri/src/actions.rs: Lane::Answer, keyed by (computer, name); a busy lane answers Ok(failed(ALREADY_ANSWERING)).
```

**Rules:**

- **`pending(details)`** reads `details["pending_questions"]` in order.
  - The question's fields are `question_id`, `question_hash`, `run_id`, `conversation_id`, `source`, `answer_transport`, `created_at`, `optional`, `can_answer`, `can_skip`, `approval`, `approval_choices`, `trust_request` and `plan_review`. Missing flags are false.
  - `unavailable_reason` is the first nonempty trimmed of `answer_unavailable_reason` and `unavailable_reason`.
  - An item has `id`, `question`, `header`, `body`, `multi_select`, `allow_other`, `required` (missing: true), and `other_label` and `other_description` with whitespace simplified.
  - Its options are `{id, label, description, broader: scope == "broader"}`.
  - `default_option_ids` and `default_text` are read as listed.
  - OpenCode only: `min_items` and `max_items` (multi only, at least 0), and `max_length` (clamped to 0..=4096).
  - `raw` is the entry unchanged.
- **`questions_error(details)`** gives "Could not load agent questions: {error}. Check Terminal." for a nonempty `pending_questions_error`.
- **`select`** is `renderQuestion` 5009-5040:
  - An entry is left out without an id, when its id repeats, when it has no items and `raw.can_answer` is not false (a missing value counts as true), or when `submitted(question)` holds.
  - `pending` adds `max(1, items)` per entry left in.
  - Required entries: the first one, replaced by a later one that `can_answer` when the first cannot.
  - Optional entries: ids newest first by `created_at`, a stable sort.
  - `chosen` (the remembered id) gives the index when it is still listed. Otherwise the index is 0 and `Selection.chosen` is None, so the hub forgets it.
  - Without a required entry, `navigation` (-1, 0 or +1) moves the index within bounds, and `Selection.chosen` becomes that id.
  - `input_visible()` is true without a question or for an optional one.
- **`unavailable`** gives the six texts in "Availability" above, in that order:
  - `facts.state != "running"` → "This session is not running.";
  - `facts.updating` → "Wait for the session update to finish.";
  - `facts.inspect_error` → "Refresh the session before answering.";
  - the card's run or conversation not the details' → "Refresh the pending question before answering.";
  - `runtime_state` or `process_state` not `live`/`running` → "The agent is no longer running.".
- **`store_key`** is `STORE_PREFIX` + `serde_json::to_string(&[session_key, id, hash])` + `"\n" + run + "\n" + conversation`. It is None for an empty id.
- **`label(session_key)`** is "{session_key with '\n' replaced by ' / '} (question answer)".
- **`AnswerDraft`** follows Qt's state rules:
  - **`from_loaded`** is `restoredDraft` 691-707:
    - `storage_error` and `notice` come from `Loaded` (S5 already marks a draft saved while sending as uncertain with "Delivery was interrupted. Check Activity or Terminal before sending again.");
    - `error` is `uncertain`;
    - `page`, `answered`, `submitted` and `answers` come from `form_state`;
    - `submitted` sets the notice "Answer submitted. Waiting for the agent to record it. Check Activity or Terminal.";
    - `answered` sets "Response recorded.".
  - **`stored()`** is `saveDraft` 709-739:
    - `form_state` is `{answers: answered ? [] : [{id, text, other, options (sorted)}] by sorted id without the seeded ones, page, answered, submitted}`;
    - the draft text is the nonempty answer texts joined by "\n" (empty once answered or submitted);
    - `label`, `sending` and `uncertain` are kept;
    - no attachments, position 0.
  - **`apply(edit)`** is `capture` 676-689 and returns false while `locked()`:
    - In a single-select item, checking an option clears the others and Other, and checking Other clears the options. Unchecking a radio does nothing.
    - In a multi-select item, an option or Other toggles.
    - `Text` sets the text.
    - Every applied edit clears `confirm` and `seeded`, and leaves an entry for every item, as `capture` reads every form. A later `seed` then never refills an answer the user emptied.
  - **`seed`** is `rebuild` 523-551, for OpenCode only and not while locked:
    - An item without an answer gets the `default_option_ids` it offers (only the first for single select).
    - An item without options that allows Other gets `default_text`, cut to `max_length` characters (or 4096).
    - Seeded items are marked in `seeded`.
  - **`restore_submitted`** is 311-338: a `codex_async` optional card whose `answer_delivery` is `submitted` and echoes id, hash, run and conversation:
    - fills the answers from it the first time;
    - sets `submitted`, clears `sending`, `error` and `uncertain`;
    - sets the notice "Answer submitted. Waiting for Codex to record it. You can check the queue above or open Terminal.";
    - returns true when the answers changed.
  - **`confirm_step`** is `submit` 921-925: for `approval_choices`, while `broader` names an option that `confirm` does not, it sets `confirm` and returns true (only the first click). Otherwise it clears `seeded` and returns false.
  - **`begin_sending`** sets `sending` and the notice "Saving before submitting…", and clears `error`.
  - **`sending_failed`** clears `sending`, and also the notice unless submitted or answered.
  - **`set_error`**:
    - once submitted or answered, it only clears `sending`;
    - otherwise it sets `error` and `uncertain` and the notice: `detail`, or "Delivery is not confirmed. Check Terminal before retrying. {detail}" when uncertain.
  - **`set_answered(optional)`**:
    - sets "Response recorded." for an optional card, else "Answer sent. Waiting for the agent…";
    - sets `answered`;
    - clears `sending`, `error` and `uncertain`.
  - **`set_submitted`**:
    - sets "Answer submitted. Waiting for Codex to record it. You can check the queue above or open Terminal.";
    - sets `submitted`;
    - clears `sending`, `error` and `uncertain`.
  - **`allow_retry`** clears `uncertain`, `error` and the notice.
  - **`snapshot()`** holds what `submissionSnapshot` 340-357 compares, without the notice.
- **`answers`** is 741-765, in source option order:
  - An item is incomplete when:
    - it has neither options nor text;
    - Other is chosen without text;
    - a single-select item has more than one option;
    - `custom_answer_problem` holds;
    - its id is empty;
    - an OpenCode bound fails (options plus an Other text count).
  - `form_reply` is `source == "opencode_native" && !approval`.
- **`skip_answers`** gives one `Skip { question_id, skip: true }` per item.
- **`decision`** gives `[Choice { question_id: items[0].id, selected_option_ids: ["allow" | "deny"], text: "" }]` for an approval card, and None otherwise.
- **`custom_answer_problem`** gives Qt's four texts (104-113). The size is UTF-8 bytes over 4096. Control characters are below U+0020 and U+007F-U+009F. The "/" and "!" check applies only when `!form_reply`.
- **`broader`** is the label of the first chosen option with `broader` (931-941).
- **`same_form(old, new)`** is `refreshNativeBody` 484-510. For the sources `claude_tool_approval`, `claude_terminal_safety` and `claude_terminal_choice` with the same nonempty hash and the same items except `body`, it is true: the form is kept and only the bodies change.
- **`card`** is `rebuild` and `updateControls` 512-632, 767-856:
  - `locked` = `draft.locked() || ctx.in_flight || ctx.loading`.
  - `read_only` = `!can_answer` and the source is one of `claude_tool_approval`, `kimi_tool_approval`, `claude_terminal_choice`, `claude_native_question`, `claude_terminal_safety`, `claude_usage_limit` or `startup_terminal_choice`.
  - `literal` (no Markdown) = approval, trust request, or a source outside the authored list.
  - `listed` = `!approval || approval_choices`.
  - The texts are those in "The card" above.
  - Option rows are `enabled` = `!locked && !read_only` and `visible` = `listed`.
  - A broader option's description gains "Applies beyond this request. Asks for confirmation." (alone when it has none), and its `markdown` is false.
  - `markdown` is `!literal` and only for a nonempty description.
  - Other is present when `allow_other` and the item has options.
  - The text line is present when `allow_other`:
    - `visible` when the item has no options or Other is checked;
    - `max_units` is 4096, or `2 * max_length` for OpenCode;
    - `max_chars` is `max_length` (0 when there is no bound);
    - the placeholder is "Write your answer…", the accessible name "Your answer to {prompt}".
  - **Submit:**
    - `visible` = `!read_only && !forms.empty && (!approval || (approval_choices && can_answer)) && page == last`;
    - `enabled` = `available && can_answer && complete && !locked && !uncertain`;
    - the label is "Submitted", "Submitting…", "Confirm", "Submit answer" or "Submit answers" in Qt's precedence;
    - `review_label` is `Some("Submit answer")` for `approval_choices` unless submitted or sending.
  - **Approve once and Deny:**
    - `visible` = `approval && !approval_choices && can_answer && items[0] has options`;
    - `enabled` = `available && !locked && !uncertain`;
    - Approve's `review_label` is `Some("Approve once")`; Deny has none.
  - **Skip:** `visible` = `optional`; `enabled` = `available && can_skip && !locked && !uncertain`.
  - **Allow retry:** `retry` = `uncertain && !locked`.
  - **Back and Next:** Back is visible with several forms and enabled = `page > 0 && !locked`. Next is visible while `page < last` and enabled = `!locked`.
  - **Pending:** `pending` is "{n} pending" with `n = max(ctx.pending, forms)`, or empty for 0.
  - **Navigation:** `nav` is Some for optional cards with `queue_count > 1`.
  - **Status:** the status line's priority, cut to 600 characters. `status_error` = `error || storage_error` is nonempty.
  - **Review:** `review` = `approval`.
  - The page is clamped to the forms.
- **`actionable`** is `actionableQuestion` 5078-5096:
  - `Silent` while the session is not ready: offline, not running, updating, inspect error, or not `live`/`running`. The hub checks "an answer of this session is in flight" and "already submitted" before.
  - `Changed` when the card is missing, cannot be answered (`can_skip` when skipping), or its run or conversation is not the details'.
  - Otherwise `Ready`.
- **`consent_identity`** keeps exactly the sixteen fields listed in "The answer path".
- **`still_answerable`** is the callback's check (5153-5170). It is false when:
  - `!row_same || !row_identity`;
  - the session is offline or not running;
  - the session is updating;
  - there is an inspect error;
  - the evidence is not `live`/`running`;
  - the evidence's run or conversation is not the question's;
  - the card's consent identity, found by id in the evidence, differs from `pinned`;
  - without consent and outside Kimi's cache panel, `cache::expired` holds on the captured details or the evidence;
  - the card cannot be answered or skipped;
  - `submitted` holds.
- **`native_cache_choice`** is `source == "kimi_cache_hint" && transport == "kimi_tui"`.
- **`cold_answer`** is None for Kimi's cache panel or when `cache::expired(details, now)` is false. Otherwise it gives the title "Submit with a cold cache?", the text `cache::warning(details, now)` + "\n\n" + the dialog text above, and the button "Submit with full context".
- **`answered_follow_up`** is 1156-1179:
  - `open_terminal` holds when the id starts with `codex-hooks-trust:` or `codex-update:`, the receipt's `question_hash` is the card's, `open_run` equals the receipt's `run_id`, and `receipt.open_terminal` is true.
  - For `codex-update:`, `receipt.codex_update` `installing` or `exited` gives the two update notices with `target` ("{name} ({machine})").
- **`Action::Answer`** invocation (`HgsClient.cpp:1872-1932`):
  - The request goes as above. Its identity comes from `question`, and the target's run and conversation are the question's.
  - The two refusals return `Err` with Qt's texts. The startup forms match exactly:
    - `{provider}-trust:{hash}` with `{provider}_tui` for `claude`, `codex` and `kimi`;
    - `claude-permissions:{hash}` with `claude_tui`;
    - `codex-hooks-trust:{hash}` and `codex-update:{hash}` with `codex_tui`;
    - each with a 64-character hash.
  - The answers are allowed when nonempty and the card `can_answer`, or when every answer is a Skip and the card is a `codex_async` optional with `can_skip`.
  - The timeouts are 30 s and 45 s.
- **`classify` for `Answer`** (1934-1957):
  - `StartFailed` is a definite failure: "Cannot start hgs: {error}".
  - `TimedOut` is uncertain: "Answer delivery timed out. Check Terminal before retrying.".
  - `WriteFailed` is uncertain with the same text: Qt's runner keeps waiting until its timeout in that case.
  - `Abnormal` is uncertain, with stderr or "Answer transfer failed (exit -1).".
  - A nonzero exit is uncertain unless it is code 1 without "delivery uncertain" (case-insensitive). The detail is stderr (trimmed, at most 4000 characters) or "Answer transfer failed (exit {code}).".
  - More than 65536 bytes of output is uncertain: "The answer acknowledgement is too large. Check Terminal before retrying.".
  - Done needs:
    - `status: answered`, or `submitted` for a `codex_async` optional card whose answers are not all Skip and whose receipt is not `skipped`;
    - `request_id`, `name`, `question_id`, `question_hash` and `run_id` equal to the request's;
    - a string `conversation_id` equal to the request's.

    Anything else is uncertain: "The answer acknowledgement is invalid. Check Terminal before retrying.".
- **Replies** (`QuestionReply.h`):
  - **`replies(text)`** trims the text.
    - It drops a Codex IDE preamble up to "\n## My request for Codex:\n".
    - It requires the `<send_user_message_question_reply>` … `</send_user_message_question_reply>` envelope around one object or an array.
    - Each item needs nonempty `questionItemId` and `question` and a string `answer`. Otherwise the result is empty.
  - **`replies_preview`** gives "Your answer: {a}" or "Your answers: {a} / {b}", with "(empty answer)" for an empty answer.
  - **`annotate_replies(payload)`** adds `question_replies: [{question, answer}]` to every user event (`UserPromptSubmit`, `UserPromptQueued`, `TurnStarted`, `QuestionAnswered`) of an Activity payload's `events` (and of an `event` frame's `event`) whose `detail` holds replies. A `state` payload's `value.prompt` gets `value.prompt_replies`.
  - **`view.rs`:** a row's preview reads `replies_preview` in place of `prompt`, and in place of `activity_detail` (or a clipped excerpt of the prompt, as `activityDetailPreview` 319-328).

**Tests that pin Qt** (`client/core/tests/questions.rs` unless noted):
- `a_required_card_wins_and_hides_the_editor`
- `optional_cards_are_newest_first_and_keep_the_choice`
- `a_card_without_items_that_cannot_be_answered_still_shows`
- `availability_texts_follow_qts_order`
- `store_keys_and_labels_are_qts` (byte-exact key for `"studio\nclaude/p/a"`)
- `drafts_round_trip_through_form_state` (seeded answers left out)
- `a_sending_draft_loads_uncertain`
- `single_select_edits_are_exclusive_and_multi_toggles`
- `opencode_defaults_seed_but_are_not_saved`
- `a_submitted_codex_answer_is_restored_and_hidden`
- `answers_follow_source_order_and_bounds`
- `custom_answer_problems`
- `a_broader_option_needs_a_second_confirm`
- `an_edit_disarms_confirm`
- `card_labels_and_status_priority` (one assertion per status rule)
- `approvals_without_choices_use_approve_and_deny`
- `read_only_cards_disable_their_options`
- `a_body_refresh_keeps_the_form`
- `actionable_is_silent_for_a_session_that_is_not_ready`
- `actionable_reports_a_changed_card`
- `still_answerable_refuses_every_change`
- `a_cold_cache_asks_except_kimis_own_panel`
- `codex_update_follow_ups`
- `replies_parse_and_preview`
- `annotate_replies_marks_user_events_only`
- `card_previews_read_answers` (in `view.rs`)
- `actions.rs`:
  - `answer_input_is_qts` (stdin fields and argv, never answers in argv);
  - `answer_refusals`;
  - `answer_outcomes` (each classification, including an echo mismatch per field and the `submitted` rule);
  - `an_uncertain_answer_is_never_retried`.
- `client/src-tauri/src/actions.rs`: `one_answer_per_session`.

### R8. The native queue, Send now, the push and OpenCode's inbox (`zerus-5wmh.9.24`)

**Files:**
- `client/core/src/input_queue.rs` (new);
- `client/core/src/actions.rs` (`Action::SendNow`, `Action::RemoveQueued`, `allowed_with`);
- `client/src-tauri/src/actions.rs` (`Lane::Queue`, `Lane::Inbox`);
- `client/core/tests/input_queue.rs`.

**Qt reference:**
- `tray/src/SessionsWindowShortcuts.cpp:95-161, 248-341`;
- `tray/src/SessionsWindow.cpp:1015-1018, 1859-1873, 3976-3980, 4433-4467, 4908-4912, 4939`;
- `tray/src/ActivityView.cpp:1029-1046, 1069-1087, 1280-1292, 1477-1487, 1890-1895`;
- `tray/src/HgsClient.cpp:2156-2290`;
- `tray/src/NativeSessionCapabilities.h:7-15`.

The CLI side: `src/state/input_queue.rs`.

**Surface:**

```rust
// client/core/src/input_queue.rs
pub const NOTHING_QUEUED: &str = "Nothing queued can be sent now";
pub const PUSH_SECONDS: f64 = 20.0;

pub struct QueueView { pub id: String, pub text: String, pub hint: String, pub can_send_now: bool, pub enabled: bool, pub label: String }
pub fn queue_view(details: &Value, tracked: bool, online: bool, inspect_error: bool, sending: bool) -> Option<QueueView>;
pub fn can_send_now(details: &Value, online: bool, queue_id: &str) -> bool;
pub struct Notice { pub text: String, pub error: bool, pub transient: bool }            // Serialize, Eq
pub fn send_now_notice(outcome: &Outcome) -> Notice;

pub struct MessageStart { pub references: Vec<String>, pub text: String, pub whole: bool }
pub fn message_start(message: &str) -> MessageStart;
pub fn queue_holds(queue: &str, message: &str) -> bool;

pub struct Push { pub key: String, pub run: String, pub conversation: String, pub text: String, pub request: Option<String>, pub confirmed: bool, pub deadline: f64 }
pub struct PushFacts { pub dialog: bool, pub sending: bool, pub waiting: bool }       // Copy, Default
pub enum PushStep { Wait, End(Option<Notice>), SendNow(String) }
#[derive(Default)] pub struct Pusher { /* Option<Push> */ }
impl Pusher {
    pub fn active(&self) -> Option<&Push>;
    pub fn arm(&mut self, key: &str, text: &str, now: f64) -> Option<Notice>;
    pub fn dispatched(&mut self, key: &str, text: &str, request: &str, run: &str, conversation: &str);
    pub fn send_finished(&mut self, request: &str, ok: bool, uncertain: bool) -> Option<Notice>;
    pub fn inspected(&mut self, key: &str, details: &Value, now: f64, facts: PushFacts, queue_busy: bool) -> PushStep;
    pub fn selection_changed(&mut self, key: Option<&str>, title: &str) -> Option<Notice>;
    pub fn check(&mut self, now: f64, facts: PushFacts) -> Option<Notice>;
}

pub struct InboxFacts<'a> { pub cmd: &'a str, pub online: bool, pub inspect_error: bool, pub busy: bool, pub allowed: bool, pub details: &'a Value, pub events: &'a [Value] }
pub fn inbox_removable(facts: &InboxFacts<'_>, message_id: &str) -> bool;
pub const REMOVE_TITLE: &str = "Remove queued message";
pub const REMOVE_TEXT: &str = "Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it.";
pub const REMOVE_CHANGED: &str = "The queued message changed. Check Activity before removing it.";
pub const REMOVED: &str = "Queued message removed.";

// client/core/src/actions.rs
#[serde(skip_deserializing)] SendNow { queue_id: String },
#[serde(skip_deserializing)] RemoveQueued { message_id: String },
pub fn allowed_with(row: &ViewRow, details: &Value, operation: &str) -> bool;  // NativeSessionCapabilities::action
// client/src-tauri/src/actions.rs: Lane::Queue (SendNow) and Lane::Inbox (RemoveQueued), keyed by (computer, name); busy → Err("busy").
```

**Rules:**

- **`queue_view`** reads `details["input_queue"]`. It is None when `!tracked` or the id is empty.
  - `can_send_now` = `input_queue.can_send_now && online && !inspect_error`.
  - `enabled` = `can_send_now && !sending`.
  - `label` is "Sending…" while sending, else "Send now".
  - `hint` is the queue's hint, with no chord.
- **`can_send_now(details, online, id)`** is `sendQueuedNow`'s check: online, a nonempty id equal to the queue's, and `can_send_now`.
- **`send_now_notice`**:
  - Done gives "Send now requested. Waiting for the agent to accept the queue." (info).
  - A failure whose trimmed detail is "hgs: The native queue is no longer visible; nothing was sent" or "The native queue is no longer visible; nothing was sent" gives the stale text as a transient error.
  - Any other failure gives its detail as an error.
- **`message_start`, `queue_holds`** are ported line for line (95-161):
  - the references are `\[(?:Image|File) #\d+\]`;
  - the text is the first 24 non-space characters, with whitespace simplified;
  - a pasted file location may follow a reference;
  - an item starts at `❯`, `↳` or `›` and continues on indented rows, but not on `•` rows. Write the bullet as `'\u{2022}'` in Rust source and test strings, since `scripts/check-ui-separators.py` scans `client/`;
  - a whole short text must end a word.

  `client/core` has no `regex` dependency: match by hand, with no new crate.
- **`Pusher`** is `armPush`, `pushSendFinished`, `pushInspected`, `pushSelectionChanged`, `checkPush` and `endPush`, with Qt's texts and a clock in seconds.
  - **`arm`** replaces an active push with "The earlier push was replaced by a new one" (info). The deadline is `now + PUSH_SECONDS`.
  - **`dispatched`** records the request, run and conversation when the key and text match and no request is recorded yet.
  - **`send_finished`** ends the push with a notice when the send was uncertain, failed, or had no text (`message_start(text).text` is empty). Otherwise it marks the push confirmed.
  - **`inspected`** works on the push of `key`, in this order:
    1. with a request recorded, a different run, or a different nonempty conversation, ends it ("The message was not pushed: the session changed", error);
    2. otherwise adopts the conversation;
    3. runs `check`;
    4. once confirmed, returns `SendNow(id)` when the queue `can_send_now`, `queue_holds(text, push.text)` holds and `!queue_busy`, ending the push without a notice.
  - **`check`**:
    - `sending` is `request.is_some() && !confirmed`, or `dialog || sending || waiting` before a request;
    - unconfirmed and not sending ends the push with "… sending did not complete" (error);
    - before the deadline nothing happens;
    - after it, the three 20-second notices follow Qt's order.
  - **`selection_changed`** ends a push of another key with "The message to {title} was not pushed: another session was selected", or "… its session is no longer selected" for None.
- **`inbox_removable`** is `queuedMessageRemovable` 4433-4442:
  - `cmd == "opencode"`, online, no inspect error, not busy;
  - a nonempty `details.run_id`;
  - `allowed` (from `allowed_with(row, details, "remove_queued")`);
  - an event with that `message_id` and `delivery_state == "not_delivered"`.
- **`allowed_with`**: the details' `allowed_actions` array when present, else `allowed(row, operation)`.
- **`Action::SendNow`** is `hgs [@host] send-now NAME --json` with `{request_id, expected_run_id, expected_conversation_id, queue_id}`, 20 s and 35 s.
  - **Classification:**
    - `StartFailed(e)` fails definitely with `e`;
    - `TimedOut` is uncertain: "Queue action timed out. Check Terminal before retrying.";
    - `WriteFailed` is uncertain: "Queue action failed. Check Terminal before retrying.";
    - `Abnormal` or a nonzero exit gives stderr (trimmed, at most 2000 characters) or "Could not confirm sending the queue. Check Terminal before retrying.". It is uncertain when abnormal or when stderr contains "delivery uncertain", definite otherwise;
    - Done needs `status: submitted` echoing `request_id`, `name`, `run_id`, `conversation_id` and `queue_id`. Anything else is uncertain: "Invalid queue acknowledgement. Check Terminal before retrying.".
- **`Action::RemoveQueued`** is the scoped `runSessionAction` (2156-2233).
  - The id must match `msg_[A-Za-z0-9_-]{1,124}`; otherwise `Err("Choose the exact queued OpenCode message.")`.
  - The call is `hgs [@host] session-action NAME --json` with `{request_id, action: "remove_queued", expected_run_id, expected_conversation_id, message_id}`, 15 s and 45 s.
  - **Classification, with the scoped texts:**
    - "Transfer failed. Check the session before retrying.";
    - "Request timed out. Check the session before retrying.";
    - a nonzero exit gives stderr or "Could not complete the request. Check the session before retrying.";
    - a receipt that echoes request, name, run and conversation with `failed` is definite: its `error` or "The queued message was not removed.";
    - with `uncertain` it is uncertain: "The result was not confirmed. Refresh before trying again." or "The result was not confirmed ({error}). Refresh before trying again.";
    - Done needs `completed`;
    - anything else is uncertain: "Invalid acknowledgement. Check the session before retrying.".

**Tests that pin Qt** (`client/core/tests/input_queue.rs` unless noted):
- `the_panel_follows_tracking_online_and_sending`
- `send_now_needs_the_same_queue`
- `send_now_notices` (the stale text, both forms)
- `queue_holds_matches_qts_cases`: Claude's `❯` rows with wrapped continuation, Codex's `↳` items under the steer header, `[File #1]` with a pasted location, "test" not matching "testing", and attachments only
- `a_push_sends_only_the_snapshot_that_holds_the_message`
- `a_push_ends_when_the_session_changes`
- `a_push_ends_on_failure_uncertainty_or_no_text`
- `the_twenty_second_endings` (each of the three)
- `a_new_push_replaces_the_old_one`
- `selecting_another_session_ends_the_push`
- `inbox_removal_needs_a_not_delivered_message`
- `actions.rs`:
  - `send_now_input_and_outcomes`;
  - `remove_queued_input_and_outcomes`;
  - `a_bad_message_id_is_refused`;
  - `allowed_with_prefers_details`.

## A2 tasks

Each task is one PR, merged after the "CI gate" check is SUCCESS. The PRs:
- the question cards: A2-1 to A2-3;
- the queue and the push: A2-4 and A2-5;
- OpenCode's inbox: A2-6.

Two chains can run side by side once Task 7 is merged:
- A2-1 → A2-2 → A2-3 (with R7);
- A2-4 → A2-5 → A2-6 (with R8).

They share `composer/mod.rs`, `Composer.tsx` and `SessionDetail.tsx` only additively. A2-4 reads A2-2's `answers.cold` when A2-2 is on main. Whichever lands second adds that line.

**Naming.** Task 4b's "questions" are the hub's own dialogs (`Prompt`, `SendReply::Cold`, `SendReply::Start`). The agent's requests are **question cards** here: `questions.rs`, `QuestionView`, `QuestionReply` and the `question_*` commands.

### Task A2-1: Question cards in the composer hub: drafts, the shown card and its view

**As built** (this task's commit and its review rounds). Where the steps below differ, the code and this list win (Qt wins over the brief):
- **The question read error** shows only as the composer's session status, as in Qt (`SessionsWindow.cpp:3913-3918`, `m_hint->hide()` 3953, `setSessionStatus` 3984-3986). `send::session_view` fills `HintFacts.question_error` from `pending_questions_error`, so `session_status` reads "Could not load agent questions: {error}. Check Terminal." in the error tone and locks input while a block reason stands. There is no `ComposerView.questions_error`, and `question_part` returns `(Option<QuestionView>, bool)` (ruling, 2026-10-29). Its test is `a_failed_question_read_is_the_session_status`.
- **Revision and epoch** (the composer's text epoch rules, ruling 2026-10-29):
  - `QuestionView.revision` bumps only on Qt's `changed` in `setQuestion` (`QuestionCard.cpp:387`): a new card, a changed form or a restored answer. The webview restarts the review delay and resets its fields.
  - `QuestionView.epoch` (`Answers.epoch`) bumps with every change of the answers that comes from Rust: every revision, a draft load of the shown card (Qt's load rebuilds the form without restarting the review, 289-301), seeded OpenCode defaults, and a refused edit. The webview takes the fields' values again whenever it grows.
  - `question_edit(card, epoch, edit)` applies only an edit of the current epoch. A stale one is dropped and Rust's answers stand. One the card refuses (loading, locked, an unchecked radio, a text over its bound) bumps the epoch and so hands Rust's answers back.
  - `question_page`, `question_navigate` and `question_allow_retry` take no epoch: Qt applies them at once.
  - Tests: `a_rebuild_bumps_the_revision_and_the_epoch`, `a_stale_edit_after_a_draft_load_leaves_rusts_answers`, `a_refused_edit_hands_the_answers_back`.
- **"Asked"** is formatted before the hub lock is taken (`Answers.asked`, refreshed by `sync_questions`), as Task 4a formats the cache expiry: local time may read the time zone files.
- **Commands** use `rename_all = "snake_case"` like the composer's and sit in the live handler only (the composer is `live`-only). `question_allow_retry` does nothing once the hub has closed.
- **Review round 1.** Drafts whose newest save has not answered and drafts of cards the open session lists now are never evicted. A selection change, or details not read yet, clears the shown card, so the next card is built again (Qt clears its details, `SessionsWindow.cpp:451`). The "Asked" cache takes only the details it was formatted from. Nothing loads or saves once the hub has closed. Allow retry acts on the shown card only (Qt's `m_key`). Each ensure numbers its load, and only that load lands (`Answers.loads`): a load whose mark a Codex record cleared, then an eviction and a new ensure, can no longer land an older read. Seven tests pin the retry timer, eviction, the choice cap, Codex's recorded answers, OpenCode's defaults, a new run and the page.
- **Review round 2** (a commit on top of A2-2). Every way out of `sync_questions` trims, the early returns included, so leaving a session with many cards drops back to 64 drafts. A load ticket goes with an evicted draft, and with a load whose mark a Codex record cleared. Tests pin `open(None)` forgetting the shown card and the "Asked" cache's `Arc::ptr_eq` guard (a test-only hook runs between `sync_questions`' two lock sections). The ticket test now evicts through the real trim.
- **A2-3 sends every input event** of the card to `question_edit` at once, without a debounce (ruling, 2026-10-29). The epoch is global across cards, and Qt captures each keystroke.
- **Later tasks bind to this:** A2-2's tests take the card and its epoch from `tests::shown(&c)`. A2-3 calls `question_edit` with `{ card, epoch, edit }`, and its `QuestionView` has `epoch: number`.

**Depends on:**
- R7 (`questions::{pending, questions_error, select, Selection, AnswerFacts, unavailable, store_key, label, AnswerDraft, Edit, CardContext, CardView, card, same_form}`);
- Tasks 3a, 4a and 4b (`Context`, `after_observe`, `tick`, and the test helpers `send_hub`, `send_hub_on`, `opened`, `ready`, `state`, `Fake`).

**Qt reference:**
- `tray/src/QuestionCard.cpp`:
  - `ensureDraft` 282-309, `restoreSubmittedAnswer` 311-338, `hasSubmittedAnswer` 359-368, `setQuestion` 370-389;
  - `setQueueNavigation` 396-406, `refreshNativeBody` 484-510, `rebuild` 512-632;
  - `capture` 676-689, `restoredDraft` 691-707, `saveDraft` 709-739;
  - the Allow retry and save-retry timers 256-265.
- `tray/src/SessionsWindow.cpp`: `renderQuestion` 5003-5068, the hint 3913-3918.

**Files:**
- Create: `client/src-tauri/src/composer/questions.rs`
- Modify: `client/src-tauri/src/composer/mod.rs` (`pub mod questions`, `Hub.answers`, the view's three fields, `open`, `after_observe` and `tick` calls)
- Modify: `client/src-tauri/src/lib.rs` (four commands)
- Test: unit tests in `composer/questions.rs`

**Interfaces:**
- Consumes: R7's surface above; Task 3a's `Composers::{lock, changed, current}`, `self.0.store`, `Hub.closed`, `Hub.open_key`, test helpers `dir`, `drafts`, `until`; Task 4a's `Composers::{context, now}`, `Context { session, key, row, online, details, facts }`.
- Produces (A2-2 and A2-3 bind to these):

```rust
// composer/questions.rs
pub(crate) type Done = Box<dyn FnOnce(bool) + Send>;
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QuestionView { pub card: String, pub revision: u64, pub asked: String, pub asked_tip: String, pub terminal_label: String, pub view: CardView }
#[derive(Default)] pub(crate) struct Answers { pub(crate) drafts: HashMap<String, AnswerDraft>, pub(crate) loading: HashSet<String>, pub(crate) revision: u64, /* private: order, saves, failed, choices, shown, last_retry */ }
pub(crate) fn facts(ctx: &Context) -> AnswerFacts;
pub(crate) fn text<'a>(value: &'a Value, key: &str) -> &'a str;
impl Composers {
    pub fn question_edit(&self, card: &str, edit: Edit);
    pub fn question_page(&self, card: &str, page: usize);
    pub fn question_navigate(&self, direction: i32);
    pub fn question_allow_retry(&self, card: &str);
    pub(crate) fn sync_questions(&self);
    pub(crate) fn question_part(&self, hub: &Hub) -> (Option<QuestionView>, bool, Option<String>);
    pub(crate) fn shown_question(&self, hub: &Hub, card: &str) -> Option<(Context, Question)>;
    pub(crate) fn save_answer(&self, hub: &mut Hub, key: &str, durable: bool, done: Option<Done>) -> bool;
    pub(crate) fn retry_answer_saves(&self);
}
// ComposerView gains
pub question: Option<QuestionView>, pub input_visible: bool, pub questions_error: Option<String>,
// commands: question_edit(card, edit); question_page(card, page); question_navigate(direction); question_allow_retry(card)
```

- [ ] **Step 1: Write the failing tests** at the end of `client/src-tauri/src/composer/questions.rs`:

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::composer::send::tests::{opened, ready, send_hub, send_hub_on, state};
    use crate::composer::tests::{dir, drafts, until};
    use crate::composer::ComposerView;
    use serde_json::json;
    use zerus_client_core::drafts::{read_draft, StoredDraft};

    pub(crate) const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// A Claude AskUserQuestion card, as `hgs inspect` lists it.
    pub(crate) fn card_json(id: &str) -> Value {
        json!({"question_id": id, "question_hash": HASH, "run_id": "run-1", "conversation_id": "conv-1",
               "source": "hook", "answer_transport": "claude_hook", "created_at": 1_793_000_000.0, "can_answer": true,
               "questions": [{"id": "q_0", "question": "Which checks?", "header": "Checks", "body": "", "multi_select": false,
                              "allow_other": true, "options": [{"id": "opt_0_0", "label": "Unit", "description": ""},
                                                                 {"id": "opt_0_1", "label": "UI", "description": ""}]}]})
    }

    pub(crate) fn asking(cards: Vec<Value>) -> Value {
        let mut details = ready();
        details["phase"] = json!("input");
        details["pending_questions"] = json!(cards);
        details
    }

    pub(crate) fn store_key(id: &str) -> String {
        format!("question\n[\"studio\\nclaude/sample-project/review\",\"{id}\",\"{HASH}\"]\nrun-1\nconv-1")
    }

    /// The composer view once a card shows and its draft has loaded.
    pub(crate) fn with_card(c: &Composers) -> ComposerView {
        until(c, |v| v.question.as_ref().is_some_and(|q| q.view.status != "Loading your saved answer…" && !q.view.forms.is_empty()))
    }

    #[test]
    fn a_required_card_replaces_the_editor() {
        let root = dir("q-required");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let view = with_card(&c);
        assert!(!view.input_visible, "a required card hides the editor and the action row");
        let q = view.question.unwrap();
        assert_eq!(q.card, store_key("tool:t1"));
        assert_eq!(q.view.heading, "Agent needs your answer");
        assert_eq!(q.view.forms[0].options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["Unit", "UI"]);
        assert_eq!(q.view.status, "Answer each question to continue.");
        assert_eq!(q.terminal_label, "Open Terminal");
        assert!(q.asked.starts_with("Asked "));
    }

    #[test]
    fn edits_are_saved_under_qts_key() {
        let root = dir("q-saved");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let card = with_card(&c).question.unwrap().card;
        c.question_edit(&card, Edit::Option { item: "q_0".into(), option: "opt_0_1".into(), checked: true });
        let view = until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.forms[0].options[1].checked));
        assert_eq!(view.question.unwrap().view.status, "Your choices are ready to send.");
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("tool:t1"));
        assert_eq!(saved.draft.label, "studio / claude/sample-project/review (question answer)");
        assert_eq!(saved.draft.form_state["answers"], json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_1"]}]));
    }

    #[test]
    fn an_answer_draft_survives_reopening() {
        let root = dir("q-reopen");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let card = with_card(&c).question.unwrap().card;
        c.question_edit(&card, Edit::Other { item: "q_0".into(), checked: true });
        c.question_edit(&card, Edit::Text { item: "q_0".into(), text: "Lint only".into() });
        c.flush();
        store.flush();
        drop(c);
        let (again, _) = send_hub_on(&store);
        opened(&again, asking(vec![card_json("tool:t1")]));
        let q = until(&again, |v| v.question.as_ref().is_some_and(|q| q.view.forms[0].other.as_ref().is_some_and(|o| o.checked))).question.unwrap();
        assert_eq!(q.view.forms[0].text.as_ref().unwrap().value, "Lint only");
        assert!(q.view.forms[0].text.as_ref().unwrap().visible);
    }

    #[test]
    fn optional_cards_are_newest_first_and_keep_the_choice() {
        let root = dir("q-optional");
        let (c, _) = send_hub(&root);
        let optional = |id: &str, at: f64| {
            let mut card = card_json(id);
            card["source"] = json!("codex_async");
            card["optional"] = json!(true);
            card["can_skip"] = json!(true);
            card["created_at"] = json!(at);
            card
        };
        let mut details = ready();
        details["pending_questions"] = json!([optional("codex:old", 10.0), optional("codex:new", 20.0)]);
        opened(&c, details.clone());
        let view = with_card(&c);
        assert!(view.input_visible, "an optional card keeps the composer");
        let q = view.question.unwrap();
        assert_eq!(q.card, store_key("codex:new"));
        assert_eq!(q.view.heading, "Optional question #1/2");
        c.question_navigate(1);
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| q.card == store_key("codex:old"))).question.unwrap();
        assert_eq!(q.view.heading, "Optional question #2/2");
        assert!(q.view.nav.as_ref().is_some_and(|nav| nav.previous && !nav.next));
        c.observe(&state(details));
        assert_eq!(c.current().unwrap().question.unwrap().card, store_key("codex:old"), "a new state keeps the choice");
    }

    #[test]
    fn an_unavailable_session_says_why() {
        let root = dir("q-unavailable");
        let (c, _) = send_hub(&root);
        let mut details = asking(vec![card_json("tool:t1")]);
        details["runtime_state"] = json!("stopped");
        opened(&c, details);
        let q = with_card(&c).question.unwrap();
        assert!(!q.view.available && !q.view.submit.enabled);
        assert_eq!(q.view.status, "The agent is no longer running.");
    }

    #[test]
    fn a_sending_answer_reloads_uncertain() {
        let root = dir("q-sending");
        let store = drafts(&root);
        let mut stored = StoredDraft { sending: true, ..StoredDraft::default() };
        stored.form_state.insert("answers".into(), json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_0"]}]));
        store.save_durable(&store_key("tool:t1"), stored, |_| {});
        store.flush();
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.retry)).question.unwrap();
        assert_eq!(q.view.status, "Delivery was interrupted. Check Activity or Terminal before sending again.");
        assert!(q.view.status_error && !q.view.submit.enabled);
        assert!(q.view.forms[0].options[0].checked);
        c.question_allow_retry(&q.card);
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| !q.view.retry)).question.unwrap();
        assert!(q.view.submit.enabled);
    }

    #[test]
    fn a_card_answered_elsewhere_goes_away() {
        let root = dir("q-elsewhere");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        with_card(&c);
        c.observe(&state(ready()));
        let view = until(&c, |v| v.question.is_none());
        assert!(view.input_visible, "the composer returns");
    }

    #[test]
    fn a_failed_question_read_is_shown() {
        let root = dir("q-error");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["pending_questions_error"] = json!("timed out");
        let view = opened(&c, details);
        assert_eq!(view.questions_error.as_deref(), Some("Could not load agent questions: timed out. Check Terminal."));
        assert!(view.question.is_none() && view.input_visible);
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::questions::`
Expected: compile errors (no `questions` module).

- [ ] **Step 3: Write `client/src-tauri/src/composer/questions.rs`** above its tests:

```rust
//! Agent question cards and approvals of the open session: Qt's
//! `QuestionCard` state and `SessionsWindow::renderQuestion`
//! (`tray/src/QuestionCard.cpp`, `tray/src/SessionsWindow.cpp:5003-5068`).
//! The rules are the core's (`questions`, R7). This module holds the answer
//! drafts, loads and saves them through the app's one store under Qt's keys,
//! and projects the shown card into the composer view. A2-2 adds the delivery.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use serde_json::Value;
use zerus_client_core::drafts::Loaded;
use zerus_client_core::questions::{self as rules, AnswerDraft, AnswerFacts, CardContext, CardView, Edit, Question, Selection};

use super::send::Context;
use super::{Composers, Hub};

/// At most this many answer drafts stay in memory; Qt keeps every one.
const HELD: usize = 64;
/// At most this many remembered choices among optional cards.
const CHOICES: usize = 64;
/// A failed save is tried again after this long (Qt's 2 s timer).
const RETRY_SAVE: f64 = 2.0;

pub(crate) type Done = Box<dyn FnOnce(bool) + Send>;

/// The shown card, as the composer view carries it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QuestionView {
    /// The card's draft key; every card command names it.
    pub card: String,
    /// Bumps when the card is built again (`rebuild`): the webview resets its
    /// fields and the review delay.
    pub revision: u64,
    pub asked: String,
    pub asked_tip: String,
    pub terminal_label: String,
    pub view: CardView,
}

/// The question cards' state (`QuestionCard`'s drafts and key sets).
#[derive(Default)]
pub(crate) struct Answers {
    pub(crate) drafts: HashMap<String, AnswerDraft>,
    /// Least recently shown first.
    order: VecDeque<String>,
    /// Keys whose load has not answered yet (`m_loadingKeys`).
    pub(crate) loading: HashSet<String>,
    /// The newest save per key (`m_saveRevision`): an older save's answer is ignored.
    saves: HashMap<String, u64>,
    /// Keys whose last save failed (`m_failedSaves`).
    failed: HashSet<String>,
    /// The chosen optional card per (session, run, conversation) (`m_questionSelections`).
    choices: VecDeque<(String, String)>,
    /// The shown card's key and question, to tell a rebuild from a body refresh.
    shown: Option<(String, Question)>,
    pub(crate) revision: u64,
    last_retry: f64,
}

impl Answers {
    fn choice(&self, scope: &str) -> Option<&str> {
        self.choices.iter().find(|(s, _)| s == scope).map(|(_, id)| id.as_str())
    }

    fn remember(&mut self, scope: String, chosen: Option<String>) {
        self.choices.retain(|(s, _)| *s != scope);
        if let Some(id) = chosen {
            self.choices.push_back((scope, id));
            while self.choices.len() > CHOICES {
                self.choices.pop_front();
            }
        }
    }
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Where a choice among optional cards is kept (`renderQuestion`'s scope).
fn scope(key: &str, details: &Value) -> String {
    serde_json::to_string(&[key, text(details, "run_id"), text(details, "conversation_id")]).unwrap_or_default()
}

/// The answer rules' view of the open session (`renderQuestion` 5054-5065).
pub(crate) fn facts(ctx: &Context) -> AnswerFacts {
    AnswerFacts {
        online: ctx.online,
        state: ctx.row.state.clone(),
        updating: ctx.facts.terminating || ctx.facts.updating,
        inspect_error: ctx.facts.inspect_error,
    }
}

/// "Asked 28 Oct 14:03:05" (with the year when it is not this year) and the
/// full time with its zone; formatted in Rust, never in the webview.
fn asked(created: f64) -> (String, String) {
    use chrono::{Datelike, Local, TimeZone};
    if !(created > 0.0 && created < 253_402_300_800.0) {
        return (String::new(), String::new());
    }
    let Some(time) = Local.timestamp_opt(created as i64, 0).single() else { return (String::new(), String::new()) };
    let format = if time.year() == Local::now().year() { "%-d %b %H:%M:%S" } else { "%-d %b %Y %H:%M:%S" };
    (format!("Asked {}", time.format(format)), time.format("%-d %b %Y %H:%M:%S %Z").to_string())
}

impl Composers {
    /// The open session's cards as Qt chooses them, without side effects.
    fn selection(&self, hub: &Hub, ctx: &Context, navigation: i32) -> Option<(Selection, String)> {
        let details = ctx.details.as_deref()?;
        let questions = rules::pending(details);
        let answers = &hub.answers;
        // `hasSubmittedAnswer`: the held draft, or the card's own record of a submitted answer.
        let submitted = |q: &Question| {
            if q.source != "codex_async" || !q.optional {
                return false;
            }
            let Some(key) = rules::store_key(&ctx.key, q) else { return false };
            let mut draft = answers.drafts.get(&key).cloned().unwrap_or_default();
            draft.restore_submitted(q);
            draft.submitted
        };
        let scope = scope(&ctx.key, details);
        Some((rules::select(&questions, &submitted, answers.choice(&scope), navigation), scope))
    }

    /// What a change of the open session's cards sets off (`renderQuestion`,
    /// `setQuestion`): drafts load, Codex's submitted answers are restored,
    /// OpenCode's defaults are seeded, the choice is remembered, and a new
    /// card is built.
    pub(crate) fn sync_questions(&self) {
        self.sync_moving(0);
    }

    fn sync_moving(&self, navigation: i32) {
        let mut hub = self.lock();
        let Some(ctx) = self.context(&hub) else {
            hub.answers.shown = None;
            return;
        };
        let Some(details) = ctx.details.clone() else { return };
        let label = rules::label(&ctx.key);
        let mut restored = HashSet::new();
        for q in rules::pending(&details).into_iter().filter(|q| q.source == "codex_async" && q.optional) {
            let Some(key) = rules::store_key(&ctx.key, &q) else { continue };
            self.ensure_answer(&mut hub, &key, &label);
            let Some(draft) = hub.answers.drafts.get_mut(&key) else { continue };
            let changed = draft.restore_submitted(&q);
            if draft.submitted {
                // A verified record wins over a load still on its way (Qt 321).
                hub.answers.loading.remove(&key);
            }
            if changed {
                restored.insert(key.clone());
                self.save_answer(&mut hub, &key, false, None);
            }
        }
        let Some((selection, scope)) = self.selection(&hub, &ctx, navigation) else { return };
        hub.answers.remember(scope, selection.chosen.clone());
        let Some(question) = selection.question else {
            hub.answers.shown = None;
            drop(hub);
            self.changed();
            return;
        };
        let Some(key) = rules::store_key(&ctx.key, &question) else { return };
        self.ensure_answer(&mut hub, &key, &label);
        if !hub.answers.loading.contains(&key) {
            if let Some(draft) = hub.answers.drafts.get_mut(&key) {
                draft.seed(&question);
            }
        }
        let rebuilt = restored.contains(&key)
            || match &hub.answers.shown {
                Some((shown, old)) => *shown != key || (old.items != question.items && !rules::same_form(old, &question)),
                None => true,
            };
        if rebuilt {
            hub.answers.revision += 1;
        }
        hub.answers.shown = Some((key, question));
        drop(hub);
        self.changed();
    }

    /// Loads a card's draft that is not in memory (`ensureDraft`).
    fn ensure_answer(&self, hub: &mut Hub, key: &str, label: &str) {
        hub.answers.order.retain(|k| k != key);
        hub.answers.order.push_back(key.to_owned());
        if hub.answers.drafts.contains_key(key) {
            return;
        }
        hub.answers.drafts.insert(key.to_owned(), AnswerDraft { label: label.to_owned(), ..AnswerDraft::default() });
        hub.answers.loading.insert(key.to_owned());
        let save = hub.answers.saves.get(key).copied().unwrap_or(0);
        self.trim_answers(hub);
        let me = self.clone();
        let (owned, label) = (key.to_owned(), label.to_owned());
        self.0.store.load(key, move |loaded| me.answer_loaded(&owned, save, &label, &loaded));
    }

    fn answer_loaded(&self, key: &str, save: u64, label: &str, loaded: &Loaded) {
        let mut hub = self.lock();
        if !hub.answers.loading.contains(key) || hub.answers.saves.get(key).copied().unwrap_or(0) != save {
            return;
        }
        hub.answers.loading.remove(key);
        hub.answers.drafts.insert(key.to_owned(), AnswerDraft::from_loaded(loaded, label));
        if hub.answers.shown.as_ref().is_some_and(|(shown, _)| shown == key) {
            hub.answers.revision += 1;
        }
        drop(hub);
        self.sync_questions();
    }

    /// Keeps at most `HELD` drafts, never the shown one, one loading, one
    /// sending or one whose save failed.
    fn trim_answers(&self, hub: &mut Hub) {
        let shown = hub.answers.shown.as_ref().map(|(key, _)| key.clone());
        let answers = &mut hub.answers;
        let mut i = 0;
        while answers.drafts.len() > HELD && i < answers.order.len() {
            let key = answers.order[i].clone();
            let keep = shown.as_deref() == Some(key.as_str()) || answers.loading.contains(&key) || answers.failed.contains(&key)
                || answers.drafts.get(&key).is_some_and(|d| d.sending);
            if keep {
                i += 1;
                continue;
            }
            answers.order.remove(i);
            answers.drafts.remove(&key);
            answers.saves.remove(&key);
        }
    }

    /// Saves a card's draft (`saveDraft`); `durable` for the save before a
    /// submission, which nothing replaces. `done` hears whether it was
    /// written, never under the hub's lock.
    pub(crate) fn save_answer(&self, hub: &mut Hub, key: &str, durable: bool, done: Option<Done>) -> bool {
        let refuse = |done: Option<Done>| {
            if let Some(done) = done {
                (self.0.deps.spawn)(Box::new(move || done(false)));
            }
            false
        };
        if hub.answers.loading.contains(key) {
            return refuse(done);
        }
        let Some(draft) = hub.answers.drafts.get(key) else { return refuse(done) };
        if !durable && draft.sending {
            return false;
        }
        let stored = draft.stored();
        let save = {
            let n = hub.answers.saves.entry(key.to_owned()).or_default();
            *n += 1;
            *n
        };
        let me = self.clone();
        let owned = key.to_owned();
        let finished = move |result: Result<(), String>| me.answer_saved(&owned, save, result, done);
        if durable {
            self.0.store.save_durable(key, stored, finished);
        } else {
            self.0.store.save(key, stored, finished);
        }
        true
    }

    fn answer_saved(&self, key: &str, save: u64, result: Result<(), String>, done: Option<Done>) {
        let mut hub = self.lock();
        if hub.answers.saves.get(key) != Some(&save) || !hub.answers.drafts.contains_key(key) {
            drop(hub);
            if let Some(done) = done {
                done(false);
            }
            return;
        }
        let ok = result.is_ok();
        if let Some(draft) = hub.answers.drafts.get_mut(key) {
            draft.storage_error = result.err().unwrap_or_default();
        }
        if ok {
            hub.answers.failed.remove(key);
        } else {
            hub.answers.failed.insert(key.to_owned());
        }
        drop(hub);
        self.changed();
        if let Some(done) = done {
            done(ok);
        }
    }

    /// Saves again, every 2 s, the drafts whose last save failed.
    pub(crate) fn retry_answer_saves(&self) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.answers.failed.is_empty() || now - hub.answers.last_retry < RETRY_SAVE {
            return;
        }
        hub.answers.last_retry = now;
        let keys: Vec<String> = hub.answers.failed.iter().cloned().collect();
        for key in keys {
            self.save_answer(&mut hub, &key, false, None);
        }
    }

    /// The question part of the open composer's view: the shown card, whether
    /// the editor shows, and a failed question read.
    pub(crate) fn question_part(&self, hub: &Hub) -> (Option<QuestionView>, bool, Option<String>) {
        let Some(ctx) = self.context(hub) else { return (None, true, None) };
        let Some(details) = ctx.details.as_deref() else { return (None, true, None) };
        let error = rules::questions_error(details);
        let Some((selection, _)) = self.selection(hub, &ctx, 0) else { return (None, true, error) };
        let input_visible = selection.input_visible();
        let Some(question) = selection.question.as_ref() else { return (None, input_visible, error) };
        let Some(key) = rules::store_key(&ctx.key, question) else { return (None, input_visible, error) };
        let draft = hub.answers.drafts.get(&key).cloned().unwrap_or_default();
        let reason = rules::unavailable(&facts(&ctx), question, details);
        let card = CardContext {
            available: reason.is_none(),
            unavailable_reason: reason.unwrap_or_default().to_owned(),
            loading: hub.answers.loading.contains(&key) || !hub.answers.drafts.contains_key(&key),
            in_flight: false,
            pending: selection.pending,
            queue_index: selection.index,
            queue_count: selection.optional.len(),
        };
        let (asked, asked_tip) = asked(question.created_at);
        let view = QuestionView {
            card: key,
            revision: hub.answers.revision,
            asked,
            asked_tip,
            terminal_label: if ctx.facts.cmd == "dsh" { "Open native UI" } else { "Open Terminal" }.to_owned(),
            view: rules::card(question, &draft, &card),
        };
        (Some(view), input_visible, error)
    }

    /// The shown card when `card` names it; commands for another card do nothing.
    pub(crate) fn shown_question(&self, hub: &Hub, card: &str) -> Option<(Context, Question)> {
        let ctx = self.context(hub)?;
        let (selection, _) = self.selection(hub, &ctx, 0)?;
        let question = selection.question?;
        (rules::store_key(&ctx.key, &question).as_deref() == Some(card)).then_some((ctx, question))
    }

    /// A change on the card (`capture`): saved as the user makes it.
    pub fn question_edit(&self, card: &str, edit: Edit) {
        let mut hub = self.lock();
        if hub.closed || hub.answers.loading.contains(card) {
            return;
        }
        let Some((_, question)) = self.shown_question(&hub, card) else { return };
        let Some(draft) = hub.answers.drafts.get_mut(card) else { return };
        if !draft.apply(&question, edit) {
            return;
        }
        self.save_answer(&mut hub, card, false, None);
        drop(hub);
        self.changed();
    }

    /// A tab, Back or Next (`currentChanged`): the page is kept with the draft.
    pub fn question_page(&self, card: &str, page: usize) {
        let mut hub = self.lock();
        if hub.closed || hub.answers.loading.contains(card) || self.shown_question(&hub, card).is_none() {
            return;
        }
        let Some(draft) = hub.answers.drafts.get_mut(card) else { return };
        if draft.locked() || draft.page == page {
            return;
        }
        draft.page = page;
        self.save_answer(&mut hub, card, false, None);
        drop(hub);
        self.changed();
    }

    /// The optional cards' arrows: -1 for newer, +1 for older.
    pub fn question_navigate(&self, direction: i32) {
        self.sync_moving(direction.signum());
    }

    /// Allow retry on the card, after the user checked Terminal.
    pub fn question_allow_retry(&self, card: &str) {
        let mut hub = self.lock();
        let Some(draft) = hub.answers.drafts.get_mut(card) else { return };
        draft.allow_retry();
        self.save_answer(&mut hub, card, false, None);
        drop(hub);
        self.changed();
    }
}

#[tauri::command]
pub fn question_edit(state: tauri::State<'_, Composers>, card: String, edit: Edit) {
    state.question_edit(&card, edit);
}

#[tauri::command]
pub fn question_page(state: tauri::State<'_, Composers>, card: String, page: usize) {
    state.question_page(&card, page);
}

#[tauri::command]
pub fn question_navigate(state: tauri::State<'_, Composers>, direction: i32) {
    state.question_navigate(direction);
}

#[tauri::command]
pub fn question_allow_retry(state: tauri::State<'_, Composers>, card: String) {
    state.question_allow_retry(&card);
}
```

- [ ] **Step 4: Wire it into `composer/mod.rs`.**
  - Add `pub mod questions;`.
  - `Hub` gains `pub(crate) answers: questions::Answers`.
  - `ComposerView` gains `pub question: Option<questions::QuestionView>`, `pub input_visible: bool` and `pub questions_error: Option<String>`.
  - `view_of` fills them: `let (question, input_visible, questions_error) = self.question_part(hub);`. Without an open session `input_visible` is `true`.
  - `after_observe` calls `self.sync_questions();` before its `self.changed()`: Task 4b's reconcile, then Task 5's continuation, then this line.
  - `open` calls `self.sync_questions();` after it drops the hub and before it returns the view.
  - `tick` calls `self.retry_answer_saves();`.

- [ ] **Step 5: Register the commands** in `client/src-tauri/src/lib.rs`, beside the composer's: `composer::questions::question_edit`, `question_page`, `question_navigate` and `question_allow_retry`. They pass the IPC guard like every app command.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::`
Expected: PASS, the 8 new tests and every earlier composer test.

Run: `cd client && cargo +1.99.1 clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings && cargo +1.99.1 fmt --manifest-path src-tauri/Cargo.toml --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): question cards in the composer hub, with Qt's answer drafts"
```

---

### Task A2-2: The guarded answer: Submit, Approve once, Deny, Skip and the cold cache

**As built** (this task's commit and its review rounds). Where the steps below differ, the code and this list win (Qt wins over the brief):
- **The call's end** (ruling, 2026-10-29). `answer_now` runs the dispatch inside `catch_unwind` and maps it through Task 5's `send::outcome_of` with Qt's `ALREADY_ANSWERING` (`HgsClient.cpp:1905`). A taken lane is a definite failure with that text, never "busy" and never uncertain. Any other `Err`, or a panic, means the call's thread died, so it is uncertain. `in_flight` is always released. Tests: `a_taken_answer_lane_is_a_definite_failure`, `a_call_that_dies_is_uncertain_and_never_retried`.
- **A reload drops the cold question.** `Composers::resend` (the reloaded webview) clears `Answers.cold`: the dialog is gone with the old page, and a held ask would refuse every later answer of the app. Test: `a_reload_drops_the_cold_question`.
- **More pinned guards:** `one_answer_per_session_at_a_time` (Qt's quiet refusal while an answer of the session is in flight, 5072), `nothing_is_answered_once_the_app_quits`, and `an_answer_saved_as_the_app_quits_is_never_sent` (the draft stays on disk as sending and reloads uncertain).
- **Names.** The events cap is `COMPOSER_EVENTS` (16), since `EVENTS` is Task 4b's 500 followed events. The core's `Actionable::Ready` holds a `Box<Question>`. Tests take the card and its epoch from A2-1's `tests::shown`, and the changed-card test holds the store with Task 3a's `hold`.
- **Review round 1.**
  - One answer per session: a second answer whose save answers while another of its session is under way ends with Qt's `ALREADY_ANSWERING`, definitely and without a call. An answer's end removes only its own `in_flight` entry. The cold question yields to an answer already under way.
  - A cold question goes with its card: the hub drops it, as cancelled with nothing sent, once its card is no longer shown (another session, a changed or answered card) or the selection closes. A2-3 also answers `question_cold(token, false)` when its dialog goes unanswered.
  - The captured details stay an `Arc`, never deep-copied under the hub lock.
  - Tests pin each input of the post-save re-check (a cache that expires, a new run, offline, stopped, a terminate, a recreated row, each ending in `CHANGED_KEPT` with nothing sent), Approve once and Deny refused on a card with native choices, a consent change behind the cold question (`CONSENT_CHANGED`), and the dropped cold question.
- **Run the tests** with `cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml -- composer:: actions::`: cargo takes one filter before `--`.
- **A2-4 binds to this:** `ComposerEvent`, `Composers::{event, take_events}`, the envelope's `composer_events`, and `Answers.cold` (an answer's cold question is open).

**Depends on:**
- A2-1;
- R7 (`questions::{answers, skip_answers, decision, actionable, Actionable, consent_identity, Recheck, still_answerable, cold_answer, ColdAnswer, answered_follow_up, Snapshot, AnswerItem, CHANGED, CHANGED_KEPT, CONSENT_CHANGED}`, `Action::Answer`, `Lane::Answer`);
- Task 4b (`Deps.dispatch`, `actions::dispatch_as`, the test fake's `calls` and `outcome`, `sent`).

**Qt reference:**
- `tray/src/QuestionCard.cpp`: `prepareSendingAsync` 428-445, `setError` 447-458, `setAnswered` 460-468, `setSubmitted` 470-478, `submit` 916-929, the Approve once, Deny and Skip handlers 222-236.
- `tray/src/SessionsWindow.cpp`: `answerQuestion` 5070-5175, `finishQuestion` 5177-5186, the receipt's follow-ups 1156-1186, `sendEvidence` 138-145, `observedIdentityMatches` 147-154.

**The rules this task keeps** (AGENTS.md):
- **Unknown delivery never authorizes replay.** The durable "sending" save comes before the call, and only its callback continues.
  - An `Uncertain` outcome or an `Err` marks the draft uncertain. Submit stays disabled until Allow retry.
  - A draft saved as sending reloads uncertain (A2-1's `a_sending_answer_reloads_uncertain`).
- **The exact identity.** The answer carries the card's own `question_id`, `question_hash`, `run_id` and `conversation_id`, pinned at the click.
  - The callback re-checks the consent identity on the newest details.
  - The CLI checks the hash again.
- **One request id per attempt.** It is made right before the one call, and nothing reuses it.
- **Broader options need the second Confirm.** `confirm_step` arms Confirm on the first click. The review delay is the webview's (A2-3).
- **Ordinary questions stay manual.** Nothing here answers without a click: no automatic approvals and no default answers sent.

**Files:**
- Modify: `client/src-tauri/src/composer/questions.rs` (the delivery, its commands, its tests)
- Modify: `client/src-tauri/src/composer/mod.rs` (`ComposerEvent`, `Hub.events`, `take_events`, `event`)
- Modify: `client/src-tauri/src/commands.rs` (`composer_events` in the envelope)
- Modify: `client/src-tauri/src/lib.rs` (four commands)
- Test: unit tests in `composer/questions.rs`

**Interfaces:**
- Consumes: A2-1's `Answers`, `facts`, `text`, `shown_question`, `question_part`, `save_answer`, `Done`; Task 4a's `refresh_details`, `now`; Task 4b's `Deps.dispatch` and test helpers.
- Produces:

```rust
// composer/mod.rs
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposerEvent { Notice { text: String, error: bool, transient: bool }, OpenTerminal { key: String } }
impl Composers { pub fn take_events(&self) -> Vec<ComposerEvent>; pub(crate) fn event(&self, hub: &mut Hub, event: ComposerEvent); }
// composer/questions.rs
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionReply { Sending, Busy, Cold { token: u64, prompt: ColdAnswer }, Notice { text: String } }
// Answers gains: pub(crate) cold: Option<ColdAsk>, pub(crate) in_flight: HashMap<String /*session key*/, String /*card*/>, next_token
impl Composers {
    pub fn question_submit(&self, card: &str) -> QuestionReply;
    pub fn question_decide(&self, card: &str, allow: bool) -> QuestionReply;
    pub fn question_skip(&self, card: &str) -> QuestionReply;
    pub fn question_cold(&self, token: u64, proceed: bool) -> QuestionReply;
}
// commands: question_submit(card) -> QuestionReply; question_decide(card, allow) -> QuestionReply;
//           question_skip(card) -> QuestionReply; question_cold(token, proceed) -> QuestionReply
// next_batch envelope: composer_events?: ComposerEvent[] (drained; at most 16)
```

- [ ] **Step 1: Write the failing tests** in `composer/questions.rs`'s `tests` module:

```rust
    use crate::composer::send::tests::sent;
    use crate::composer::{lock, ComposerEvent};
    use zerus_client_core::{Action, Outcome};

    fn answered(_: &Action, id: &str) -> Outcome {
        Outcome::Done { receipt: json!({"status": "answered", "request_id": id}), cleanup: None }
    }

    /// Chooses Unit on the shown card and returns its key once Submit is enabled.
    fn chosen(c: &Composers) -> String {
        let card = with_card(c).question.unwrap().card;
        c.question_edit(&card, Edit::Option { item: "q_0".into(), option: "opt_0_0".into(), checked: true });
        until(c, |v| v.question.as_ref().is_some_and(|q| q.view.submit.enabled));
        card
    }

    fn status_is(c: &Composers, status: &str) -> QuestionView {
        until(c, |v| v.question.as_ref().is_some_and(|q| q.view.status == status)).question.unwrap()
    }

    #[test]
    fn an_answer_goes_out_once_with_the_cards_identity() {
        let root = dir("q-answer");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(answered);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let card = chosen(&c);
        assert_eq!(c.question_submit(&card), QuestionReply::Sending);
        let q = status_is(&c, "Answer sent. Waiting for the agent…");
        assert!(q.view.locked && !q.view.submit.enabled);
        let calls = sent(&fake);
        assert_eq!(calls.len(), 1);
        let (target, action, _) = &calls[0];
        assert_eq!((target.name.as_str(), target.run_id.as_str(), target.conversation_id.as_str()), ("claude/sample-project/review", "run-1", "conv-1"));
        let Action::Answer { question, answers } = action else { panic!("not an answer") };
        assert_eq!((question["question_id"].as_str(), question["question_hash"].as_str()), (Some("tool:t1"), Some(HASH)));
        assert_eq!(serde_json::to_value(answers).unwrap(), json!([{"question_id": "q_0", "selected_option_ids": ["opt_0_0"], "text": ""}]));
        assert_eq!(c.question_submit(&card), QuestionReply::Busy, "an answered card sends nothing again");
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_card_changed_during_the_save_is_never_sent() {
        let root = dir("q-changed");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let card = chosen(&c);
        // Hold the writer, so the durable save waits while the card changes.
        let (release, held) = std::sync::mpsc::channel::<()>();
        store.load("hold", move |_| { let _ = held.recv(); });
        assert_eq!(c.question_submit(&card), QuestionReply::Sending);
        let mut changed = card_json("tool:t1");
        changed["question_hash"] = json!("b".repeat(64));
        c.observe(&state(asking(vec![changed])));
        release.send(()).unwrap();
        store.flush();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while c.lock().answers.drafts.get(&card).is_some_and(|d| d.sending) {
            assert!(std::time::Instant::now() < deadline, "the callback never ran");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(c.lock().answers.drafts[&card].notice, rules::CHANGED_KEPT);
        assert!(sent(&fake).is_empty(), "a changed card is never answered");
    }

    #[test]
    fn an_uncertain_answer_waits_for_allow_retry() {
        let root = dir("q-uncertain");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain { detail: "Answer delivery timed out. Check Terminal before retrying.".into() });
        opened(&c, asking(vec![card_json("tool:t1")]));
        let card = chosen(&c);
        c.question_submit(&card);
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.retry)).question.unwrap();
        assert_eq!(q.view.status, "Delivery is not confirmed. Check Terminal before retrying. Answer delivery timed out. Check Terminal before retrying.");
        assert!(q.view.status_error && !q.view.submit.enabled);
        assert_eq!(c.question_submit(&card), QuestionReply::Busy);
        c.observe(&state(asking(vec![card_json("tool:t1")])));
        assert_eq!(c.question_submit(&card), QuestionReply::Busy, "a new state never resends");
        assert_eq!(sent(&fake).len(), 1);
        c.question_allow_retry(&card);
        until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.submit.enabled));
        *lock(&fake.outcome) = Box::new(answered);
        assert_eq!(c.question_submit(&card), QuestionReply::Sending);
        status_is(&c, "Answer sent. Waiting for the agent…");
        let calls = sent(&fake);
        assert_ne!(calls[0].2, calls[1].2, "a retry is a new request");
    }

    #[test]
    fn a_broader_choice_sends_only_after_confirm() {
        let root = dir("q-broader");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(answered);
        let mut card = card_json("claude-approval:1");
        card["source"] = json!("claude_tool_approval");
        card["answer_transport"] = json!("claude_tui");
        card["approval"] = json!(true);
        card["approval_choices"] = json!(true);
        card["questions"] = json!([{"id": "approval", "question": "Run cargo test?", "header": "Bash", "body": "cargo test", "allow_other": false,
            "options": [{"id": "1", "label": "Yes", "scope": "once"}, {"id": "2", "label": "Yes, and don't ask again", "scope": "broader"}, {"id": "3", "label": "No", "scope": "once"}]}]);
        let mut details = asking(vec![card]);
        details["phase"] = json!("approval");
        opened(&c, details);
        let key = with_card(&c).question.unwrap().card;
        c.question_edit(&key, Edit::Option { item: "approval".into(), option: "2".into(), checked: true });
        until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.submit.enabled));
        assert_eq!(c.question_submit(&key), QuestionReply::Busy, "the first click only arms Confirm");
        let q = status_is(&c, "“Yes, and don't ask again” changes the agent's permissions beyond this request. Select Confirm to send it.");
        assert_eq!(q.view.submit.label, "Confirm");
        assert!(sent(&fake).is_empty());
        assert_eq!(c.question_submit(&key), QuestionReply::Sending);
        status_is(&c, "Answer sent. Waiting for the agent…");
        let Action::Answer { answers, .. } = &sent(&fake)[0].1 else { panic!("not an answer") };
        assert_eq!(serde_json::to_value(answers).unwrap(), json!([{"question_id": "approval", "selected_option_ids": ["2"], "text": ""}]));
    }

    #[test]
    fn approve_and_deny_send_the_decision() {
        let root = dir("q-decide");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(answered);
        let mut card = card_json("kimi-approval:1");
        card["source"] = json!("kimi_tool_approval");
        card["answer_transport"] = json!("kimi_tui");
        card["approval"] = json!(true);
        card["questions"] = json!([{"id": "approval", "question": "Run rm -rf build?", "body": "rm -rf build", "allow_other": false,
            "options": [{"id": "allow", "label": "Approve"}, {"id": "deny", "label": "Deny"}]}]);
        opened(&c, asking(vec![card]));
        let q = with_card(&c).question.unwrap();
        assert!(q.view.approve.visible && q.view.deny.visible && !q.view.submit.visible);
        assert_eq!(q.view.approve.review_label.as_deref(), Some("Approve once"));
        assert_eq!(c.question_decide(&q.card, false), QuestionReply::Sending);
        status_is(&c, "Answer sent. Waiting for the agent…");
        let Action::Answer { answers, .. } = &sent(&fake)[0].1 else { panic!("not an answer") };
        assert_eq!(serde_json::to_value(answers).unwrap(), json!([{"question_id": "approval", "selected_option_ids": ["deny"], "text": ""}]));
    }

    #[test]
    fn a_cold_cache_asks_first_and_cancel_sends_nothing() {
        let root = dir("q-cold");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(answered);
        let mut details = asking(vec![card_json("tool:t1")]);
        details["session_usage"] = json!({"prompt_cache": {"status": "warm", "expires_at": 1_793_000_000.0}});
        opened(&c, details);
        let card = chosen(&c);
        let QuestionReply::Cold { token, prompt } = c.question_submit(&card) else { panic!("no cold question") };
        assert_eq!((prompt.title.as_str(), prompt.proceed.as_str()), ("Submit with a cold cache?", "Submit with full context"));
        assert_eq!(c.question_submit(&card), QuestionReply::Busy, "one cold question at a time");
        assert_eq!(c.question_cold(token, false), QuestionReply::Busy);
        assert!(sent(&fake).is_empty());
        let QuestionReply::Cold { token, .. } = c.question_submit(&card) else { panic!("no cold question") };
        assert_eq!(c.question_cold(token, true), QuestionReply::Sending);
        status_is(&c, "Answer sent. Waiting for the agent…");
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn answers_changed_behind_the_cold_question_are_kept() {
        let root = dir("q-cold-changed");
        let (c, fake) = send_hub(&root);
        let mut details = asking(vec![card_json("tool:t1")]);
        details["session_usage"] = json!({"prompt_cache": {"status": "warm", "expires_at": 1_793_000_000.0}});
        opened(&c, details);
        let card = chosen(&c);
        let QuestionReply::Cold { token, .. } = c.question_submit(&card) else { panic!("no cold question") };
        c.question_edit(&card, Edit::Option { item: "q_0".into(), option: "opt_0_1".into(), checked: true });
        assert_eq!(c.question_cold(token, true), QuestionReply::Notice { text: rules::CONSENT_CHANGED.into() });
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn skip_sends_skips_and_codex_update_opens_terminal() {
        let root = dir("q-skip");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).machine = "studio".into();
        *lock(&fake.outcome) = Box::new(answered);
        let mut optional = card_json("codex:q1");
        optional["source"] = json!("codex_async");
        optional["optional"] = json!(true);
        optional["can_skip"] = json!(true);
        let mut details = ready();
        details["pending_questions"] = json!([optional]);
        opened(&c, details);
        let card = with_card(&c).question.unwrap().card;
        assert_eq!(c.question_skip(&card), QuestionReply::Sending);
        status_is(&c, "Response recorded.");
        let Action::Answer { answers, .. } = &sent(&fake)[0].1 else { panic!("not an answer") };
        assert_eq!(serde_json::to_value(answers).unwrap(), json!([{"question_id": "q_0", "skip": true}]));

        let id = format!("codex-update:{HASH}");
        let mut update = card_json(&id);
        update["source"] = json!("codex_update");
        update["answer_transport"] = json!("codex_tui");
        update["questions"] = json!([{"id": "update", "question": "Update Codex?", "allow_other": false,
            "options": [{"id": "update_now", "label": "Update now"}, {"id": "skip", "label": "Skip"}]}]);
        *lock(&fake.outcome) = Box::new(|_, id| Outcome::Done { receipt: json!({"status": "answered", "request_id": id, "question_hash": HASH,
            "run_id": "run-1", "open_terminal": true, "codex_update": "installing"}), cleanup: None });
        c.observe(&state(asking(vec![update])));
        let card = until(&c, |v| v.question.as_ref().is_some_and(|q| q.card.contains("codex-update:") && !q.view.forms.is_empty())).question.unwrap().card;
        c.question_edit(&card, Edit::Option { item: "update".into(), option: "update_now".into(), checked: true });
        until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.submit.enabled));
        assert_eq!(c.question_submit(&card), QuestionReply::Sending);
        status_is(&c, "Answer sent. Waiting for the agent…");
        assert_eq!(c.take_events(), vec![
            ComposerEvent::OpenTerminal { key: crate::composer::tests::KEY.into() },
            ComposerEvent::Notice { text: "Codex in claude/sample-project/review (studio) is installing its update in Terminal. Resume the session after Codex exits.".into(), error: false, transient: false },
        ]);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::questions::`
Expected: compile errors (no `question_submit`, `QuestionReply` or `ComposerEvent`).

- [ ] **Step 3: The events** in `composer/mod.rs`:

```rust
/// What Rust tells the webview besides the views: a notice (Qt's
/// `showNotice`), or the Terminal tab to open. Drained with each batch.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposerEvent {
    Notice { text: String, error: bool, transient: bool },
    OpenTerminal { key: String },
}

/// At most this many events wait for the next batch; older ones go first.
const EVENTS: usize = 16;

impl Composers {
    /// Queues an event; the caller wakes the pump (`changed`) after it drops the hub.
    pub(crate) fn event(&self, hub: &mut Hub, event: ComposerEvent) {
        hub.events.push_back(event);
        while hub.events.len() > EVENTS {
            hub.events.pop_front();
        }
    }

    /// The events since the last batch.
    pub fn take_events(&self) -> Vec<ComposerEvent> {
        self.lock().events.drain(..).collect()
    }
}
```

`Hub` gains `pub(crate) events: VecDeque<ComposerEvent>`. In `commands.rs`, the `Envelope` gains `#[serde(skip_serializing_if = "Vec::is_empty")] composer_events: Vec<crate::composer::ComposerEvent>`, filled with `composers.take_events()` beside `take_view()`.

- [ ] **Step 4: The delivery** in `composer/questions.rs`. `Answers` gains `pub(crate) cold: Option<ColdAsk>`, `pub(crate) in_flight: HashMap<String, String>` and `next_token: u64`. `question_part` passes `in_flight: hub.answers.in_flight.values().any(|card| *card == key)`.

```rust
use std::sync::MutexGuard;
use zerus_client_core::questions::{Actionable, AnswerItem, ColdAnswer, Snapshot};
use zerus_client_core::{Action, Outcome, Target};
use super::{ComposerEvent, SessionRef};

/// What Submit, Approve once, Deny, Skip and the cold question answer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionReply {
    /// The answer is on its way; its end arrives with the card.
    Sending,
    /// Nothing happened: the card is not ready, changed quietly, or another answer runs.
    Busy,
    /// Ask "Submit with a cold cache?" and answer with `question_cold(token, …)`.
    Cold { token: u64, prompt: ColdAnswer },
    /// A sticky error notice.
    Notice { text: String },
}

/// One answer from the click to its end, with everything it was asked under.
#[derive(Clone)]
pub(crate) struct Answering {
    card: String,
    key: String,
    session: SessionRef,
    created: i64,
    row_run: String,
    row_conversation: String,
    question: Question,
    answers: Vec<AnswerItem>,
    /// The card's consent identity at the click.
    pinned: Value,
    /// The details at the click (`sendEvidence`).
    captured: Value,
    skipping: bool,
    consent: bool,
}

/// An answer waiting for the cold-cache consent (`answerQuestion` 5111-5142).
pub(crate) struct ColdAsk {
    token: u64,
    answering: Answering,
    snapshot: Snapshot,
}

impl Composers {
    fn button_ready(&self, hub: &Hub, card: &str, pick: fn(&CardView) -> &rules::ButtonView) -> bool {
        self.question_part(hub).0.filter(|q| q.card == card).is_some_and(|q| pick(&q.view).visible && pick(&q.view).enabled)
    }

    /// Submit on the card (`QuestionCard::submit`): the shown values, after a
    /// broader option's second Confirm. The review delay is the webview's.
    pub fn question_submit(&self, card: &str) -> QuestionReply {
        let mut hub = self.lock();
        let Some((ctx, question)) = self.shown_question(&hub, card) else { return QuestionReply::Busy };
        if !self.button_ready(&hub, card, |v| &v.submit) {
            return QuestionReply::Busy;
        }
        let Some(draft) = hub.answers.drafts.get_mut(card) else { return QuestionReply::Busy };
        let (answers, valid) = rules::answers(&question, draft);
        if !valid {
            return QuestionReply::Busy;
        }
        if draft.confirm_step(&question) {
            self.save_answer(&mut hub, card, false, None);
            drop(hub);
            self.changed();
            return QuestionReply::Busy;
        }
        self.request_answer(hub, ctx, question, card, answers, false)
    }

    /// Approve once or Deny on an approval without native choices.
    pub fn question_decide(&self, card: &str, allow: bool) -> QuestionReply {
        let hub = self.lock();
        let Some((ctx, question)) = self.shown_question(&hub, card) else { return QuestionReply::Busy };
        let pick: fn(&CardView) -> &rules::ButtonView = if allow { |v| &v.approve } else { |v| &v.deny };
        if !self.button_ready(&hub, card, pick) {
            return QuestionReply::Busy;
        }
        let Some(answers) = rules::decision(&question, allow) else { return QuestionReply::Busy };
        self.request_answer(hub, ctx, question, card, answers, false)
    }

    /// Skip on an optional Codex card.
    pub fn question_skip(&self, card: &str) -> QuestionReply {
        let hub = self.lock();
        let Some((ctx, question)) = self.shown_question(&hub, card) else { return QuestionReply::Busy };
        if !self.button_ready(&hub, card, |v| &v.skip) {
            return QuestionReply::Busy;
        }
        let answers = rules::skip_answers(&question);
        self.request_answer(hub, ctx, question, card, answers, true)
    }

    /// `answerQuestion`: the quiet checks, the changed card, the cold cache.
    fn request_answer(&self, mut hub: MutexGuard<'_, Hub>, ctx: Context, question: Question, card: &str, answers: Vec<AnswerItem>, skipping: bool) -> QuestionReply {
        if hub.closed || hub.answers.cold.is_some() || hub.answers.in_flight.contains_key(&ctx.key) {
            return QuestionReply::Busy;
        }
        let Some(details) = ctx.details.clone() else { return QuestionReply::Busy };
        if question.source == "codex_async" && question.optional && hub.answers.drafts.get(card).is_some_and(|d| d.submitted) {
            return QuestionReply::Busy;
        }
        let question = match rules::actionable(&details, &facts(&ctx), &question.id, skipping) {
            Actionable::Ready(question) => question,
            Actionable::Silent => return QuestionReply::Busy,
            Actionable::Changed => {
                if let Some(draft) = hub.answers.drafts.get_mut(card) {
                    draft.set_error(rules::CHANGED, false);
                }
                self.save_answer(&mut hub, card, false, None);
                drop(hub);
                self.changed();
                return QuestionReply::Busy;
            }
        };
        let answering = Answering {
            card: card.to_owned(),
            key: ctx.key.clone(),
            session: ctx.session.clone(),
            created: ctx.row.created,
            row_run: ctx.row.run_id.clone().unwrap_or_default(),
            row_conversation: ctx.row.conversation_id.clone().unwrap_or_default(),
            pinned: rules::consent_identity(&question.raw),
            captured: (*details).clone(),
            question,
            answers,
            skipping,
            consent: false,
        };
        if let Some(prompt) = rules::cold_answer(&details, &answering.question, self.now()) {
            let Some(draft) = hub.answers.drafts.get(card) else { return QuestionReply::Busy };
            if draft.sending || draft.submitted || draft.answered || draft.uncertain || !draft.storage_error.is_empty() {
                return QuestionReply::Busy;
            }
            let snapshot = draft.snapshot();
            hub.answers.next_token += 1;
            let token = hub.answers.next_token;
            hub.answers.cold = Some(ColdAsk { token, answering, snapshot });
            return QuestionReply::Cold { token, prompt };
        }
        self.prepare_answer(hub, answering)
    }

    /// The cold question's answer. Anything that changed behind it keeps the
    /// draft and sends nothing (`answerQuestion` 5131-5142).
    pub fn question_cold(&self, token: u64, proceed: bool) -> QuestionReply {
        let mut hub = self.lock();
        let Some(ask) = hub.answers.cold.take_if(|ask| ask.token == token) else { return QuestionReply::Busy };
        if !proceed {
            return QuestionReply::Busy;
        }
        let a = &ask.answering;
        let same = self.context(&hub).is_some_and(|ctx| {
            let details = ctx.details.as_deref().cloned().unwrap_or_default();
            let identity = match rules::actionable(&details, &facts(&ctx), &a.question.id, a.skipping) {
                Actionable::Ready(question) => rules::consent_identity(&question.raw),
                _ => Value::Null,
            };
            ctx.key == a.key && ctx.session.computer == a.session.computer && ctx.session.name == a.session.name
                && ctx.row.created == a.created
                && ctx.row.run_id.clone().unwrap_or_default() == a.row_run
                && ctx.row.conversation_id.clone().unwrap_or_default() == a.row_conversation
                && text(&details, "run_id") == text(&a.captured, "run_id")
                && text(&details, "conversation_id") == text(&a.captured, "conversation_id")
                && identity == a.pinned
        }) && hub.answers.drafts.get(&a.card).is_some_and(|d| d.snapshot() == ask.snapshot);
        if !same {
            return QuestionReply::Notice { text: rules::CONSENT_CHANGED.to_owned() };
        }
        let mut answering = ask.answering;
        answering.consent = true;
        self.prepare_answer(hub, answering)
    }

    /// `prepareSendingAsync`: the draft is saved durably as sending before
    /// anything runs, and only that save's answer can continue.
    fn prepare_answer(&self, mut hub: MutexGuard<'_, Hub>, answering: Answering) -> QuestionReply {
        let card = answering.card.clone();
        if hub.answers.loading.contains(&card) {
            return QuestionReply::Busy;
        }
        let Some(draft) = hub.answers.drafts.get_mut(&card) else { return QuestionReply::Busy };
        if draft.sending || draft.submitted || draft.answered || draft.uncertain {
            return QuestionReply::Busy;
        }
        draft.begin_sending();
        let me = self.clone();
        self.save_answer(&mut hub, &card, true, Some(Box::new(move |saved| me.answer_prepared(answering, saved))));
        drop(hub);
        self.changed();
        QuestionReply::Sending
    }

    /// The durable save answered: check everything again, then call `hgs answer` once.
    fn answer_prepared(&self, answering: Answering, saved: bool) {
        let mut hub = self.lock();
        let card = answering.card.clone();
        if !saved {
            if let Some(draft) = hub.answers.drafts.get_mut(&card) {
                draft.sending_failed();
            }
            drop(hub);
            self.changed();
            return;
        }
        // The app quits: the draft is on disk as sending and reloads uncertain.
        if hub.closed {
            return;
        }
        if !self.still_answerable(&hub, &answering) {
            if let Some(draft) = hub.answers.drafts.get_mut(&card) {
                draft.set_error(rules::CHANGED_KEPT, false);
            }
            self.save_answer(&mut hub, &card, false, None);
            drop(hub);
            self.changed();
            return;
        }
        hub.answers.in_flight.insert(answering.key.clone(), card);
        drop(hub);
        self.changed();
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || me.answer_now(answering)));
    }

    /// The callback's re-check (5153-5170), on the open session's details or,
    /// after a switch, on the details captured at the click.
    fn still_answerable(&self, hub: &Hub, a: &Answering) -> bool {
        let Some((row, online)) = (self.0.deps.rows)(&a.session.computer, &a.session.key, &a.session.name) else { return false };
        let ctx = (hub.open_key.as_deref() == Some(a.key.as_str())).then(|| self.context(hub)).flatten();
        let evidence = ctx.as_ref().and_then(|c| c.details.as_deref().cloned()).unwrap_or_else(|| a.captured.clone());
        let facts = AnswerFacts {
            online,
            state: row.state.clone(),
            updating: (self.0.deps.facts)(&a.session.computer, &a.session.name).is_some(),
            inspect_error: ctx.as_ref().is_some_and(|c| c.facts.inspect_error),
        };
        let (run, conversation) = (row.run_id.clone().unwrap_or_default(), row.conversation_id.clone().unwrap_or_default());
        rules::still_answerable(&rules::Recheck {
            pinned: &a.pinned,
            question: &a.question,
            evidence: &evidence,
            captured: &a.captured,
            row_same: row.name == a.session.name && row.created == a.created,
            row_identity: (run == a.row_run && conversation == a.row_conversation)
                || (run == a.question.run_id && conversation == a.question.conversation_id),
            facts: &facts,
            skipping: a.skipping,
            consent: a.consent,
            submitted: hub.answers.drafts.get(&a.card).is_some_and(|d| d.submitted),
            now: self.now(),
        })
    }

    /// The one `hgs answer` call of this attempt, with its own request id.
    fn answer_now(&self, answering: Answering) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let target = Target {
            computer: answering.session.computer.clone(),
            name: answering.session.name.clone(),
            run_id: answering.question.run_id.clone(),
            conversation_id: answering.question.conversation_id.clone(),
        };
        let action = Action::Answer { question: answering.question.raw.clone(), answers: answering.answers.clone() };
        let outcome = match (self.0.deps.dispatch)(target, action, request_id) {
            Ok(outcome) => outcome,
            // The call's thread died, so it may have run.
            Err(error) => Outcome::Uncertain { detail: error },
        };
        self.answer_finished(&answering, outcome);
    }

    /// `finishQuestion`: the card says how it ended. Nothing is sent again by itself.
    fn answer_finished(&self, a: &Answering, outcome: Outcome) {
        let mut hub = self.lock();
        hub.answers.in_flight.remove(&a.key);
        let open = hub.open_key.as_deref() == Some(a.key.as_str());
        let open_run = open.then(|| hub.followed.details.as_deref().map(|d| text(d, "run_id").to_owned())).flatten();
        let mut follow = None;
        if let Some(draft) = hub.answers.drafts.get_mut(&a.card) {
            match &outcome {
                Outcome::Done { receipt, .. } if receipt["status"] == "submitted" => draft.set_submitted(),
                Outcome::Done { receipt, .. } => {
                    draft.set_answered(a.question.optional);
                    let target = (self.0.deps.rows)(&a.session.computer, &a.session.key, &a.session.name)
                        .map(|(row, _)| format!("{} ({})", a.session.name, row.machine))
                        .unwrap_or_else(|| a.session.name.clone());
                    follow = Some(rules::answered_follow_up(&a.question, receipt, open_run.as_deref(), &target));
                }
                Outcome::Failed { detail } => draft.set_error(detail, false),
                Outcome::Uncertain { detail } => draft.set_error(detail, true),
            }
        }
        self.save_answer(&mut hub, &a.card, false, None);
        if let Some(follow) = follow {
            if follow.open_terminal {
                self.event(&mut hub, ComposerEvent::OpenTerminal { key: a.key.clone() });
            }
            if let Some(text) = follow.notice {
                self.event(&mut hub, ComposerEvent::Notice { text, error: false, transient: false });
            }
        }
        drop(hub);
        self.changed();
        if open {
            self.refresh_details();
        }
    }
}

#[tauri::command]
pub fn question_submit(state: tauri::State<'_, Composers>, card: String) -> QuestionReply {
    state.question_submit(&card)
}

#[tauri::command]
pub fn question_decide(state: tauri::State<'_, Composers>, card: String, allow: bool) -> QuestionReply {
    state.question_decide(&card, allow)
}

#[tauri::command]
pub fn question_skip(state: tauri::State<'_, Composers>, card: String) -> QuestionReply {
    state.question_skip(&card)
}

#[tauri::command]
pub fn question_cold(state: tauri::State<'_, Composers>, token: u64, proceed: bool) -> QuestionReply {
    state.question_cold(token, proceed)
}
```

`Action::Answer` is never deserialized: it is built here from the shown card's raw entry and the held draft's answers.

- [ ] **Step 5: Register** the four commands in `lib.rs` beside A2-1's.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: actions::`
Expected: PASS, A2-1's tests and the 8 new ones. Clippy and fmt clean.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): answer question cards and approvals through the guarded hgs answer"
```

---

### Task A2-3: The question card view, and answers in Activity

**As built** (this task's commit and its review rounds). Where the steps below differ, the code and this list win (Qt wins over the brief):
- **The epoch** (A2-1 As built). `questionEdit(card, epoch, edit)` names the epoch the fields show, and every input event goes at once, without a debounce. The review restarts only when the card or its revision changes. A grown epoch alone keeps it running (Qt's load rebuilds the form without restarting the review, `QuestionCard.cpp:289-301`).
- **Rust's values win on a grown epoch.** Forms are keyed by their id, not by the form object, which is new with every view, so an echo never remounts the text field. When the epoch grows, the answer line and every option input take Rust's values again. Solid does not set an unchanged `checked` again, so without this a dropped stale click would stay on screen.
- **The card follows the composer's accepted view** (`composer.view()?.question`), not the raw envelope. Task 6's `receive` drops or holds a view of another composer, and the card follows the same view.
- **No read-error element.** A failed question read is the composer's `session_status` (A2-1 As built), which Task 6 shows.
- **Open Terminal** and the `open_terminal` event call `terminal.opened()` before showing the Terminal tab, as the tab bar does (`SessionsWindow.cpp:1141,1177`). Showing it connects (`terminal.show`); its availability and reason come from `terminal.texts()`.
- **A required card** hides both resize handles (Qt hides `m_resizeHandle`, `MessageComposer.cpp:1129`), the editor, its hint and the action row. The toolbar keeps a plain spacer where its grip was, so the trailing chips stay on the right.
- **The store's port** reads the bridge at each call, not when the module loads.
- **Tests:** the brief's six, plus "an unavailable card holds the review at its start" (`QuestionCard.cpp:239-240`), "a grown epoch alone keeps the review running" and "every edit goes at once, with the epoch the fields show". The client has no DOM test environment, so the card component itself is not unit-tested.
- **Review round 1:** the editor, action row and resize handles stay mounted while a required card hides them. `sendEnabled`, `send()` and Rust's `send_message` refuse under that card; the Rust refusal hands the text back with Busy. A2-4 applies the same rule to `send_push`.
- **Echoes and focus:** a memo tracks the epoch and a remembered value detects growth, including the first change after mount. Same-epoch echoes leave local input alone. Options and tabs use Solid's `Index` to preserve their elements across fresh view objects; a rebuilt form still follows its id. Headless DOM probes check typing, focus and required-card remounts.
- **Keys and review:** repeated Enter or Space on the card is swallowed; Enter on a button does nothing (`QuestionCard.cpp:907`). Enter in an answer line or option ignores IME composition and key code 229. Locked cards retain browsable tabs without changing the hub's saved page; their status is a live region (`QuestionCard.cpp:249-255`).
- **Cold dialog:** changing its card or unmounting it unanswered sends `question_cold(token, false)` once. A local generation also cancels a cold reply that arrives later, including after the same card remounts, without showing that stale prompt. Cancel keeps default focus and Escape cancels (`SessionsWindow.cpp:5111-5131`).
- **Activity:** archived events receive the same bounded reply annotation as live events. Queued status takes precedence over answer labels (`ActivityView.cpp:1036`); answer and recorded-answer bodies render read-only. Envelope and accepted-composer wiring tests cover the card and composer events. The A2-2 concurrency test also pins a third Skip while the first answer remains in flight: Busy, `ALREADY_ANSWERING` retained, no call.

**Depends on:**
- A2-2;
- Task 2 (`terminal`, `setDetailTab`) and Task 6 (`composer`, `SessionDetail`'s composer column);
- R7's `annotate_replies` for the transcript.

**Qt reference:**
- `tray/src/QuestionCard.cpp`: the layout 179-268, the form 512-632, the controls 767-856, Enter 898-914, the review timer 237-246.
- `tray/src/SessionsWindow.cpp`:
  - the placement 918 and 1030;
  - `setInputVisible` at 5049;
  - Open Terminal 1137-1142;
  - the cold dialog 5111-5131.
- `tray/src/ActivityView.cpp`: the answer labels and bodies 347-363, 975-981 and 1029-1031.

There is no mockup for the card: it follows Qt's layout with the skin's tokens.

**Files:**
- Modify: `client/src/types.ts` (the card's shapes, `ComposerView`'s three fields, `Batch.composer_events`), `client/src/bridge.ts` (eight wrappers; `normalize` keeps `composer_events`)
- Create: `client/src/store/questions.ts`, `client/src/store/questions.test.ts`
- Create: `client/src/ui/sessions/QuestionCard.tsx`
- Modify: `client/src/store/session.ts` (route the card and the events)
- Modify: `client/src/ui/sessions/Composer.tsx` (the input hides under a required card)
- Modify: `client/src/ui/sessions/SessionDetail.tsx` (the card and the read error above the composer)
- Modify: `client/src/ui/sessions/Activity.tsx` (export `Body`; reply blocks), `client/src/ui/sessions/types.ts` (`TranscriptItem.replies`)
- Modify: `client/src/ui/sessions/transcript.ts`, `client/src/ui/sessions/transcript.test.ts`
- Modify: `client/src/ui/styles/detail.css`
- Modify: `client/src-tauri/src/commands.rs` (`annotate` before the envelope, and its test)

**Interfaces:**
- Consumes: A2-1 and A2-2's commands and serde shapes; Task 6's `composer` store and `notify`; Task 2's `terminal.status()`, `terminal.connect()`, `setDetailTab`.
- Produces:

```ts
// types.ts
export interface ButtonView { visible: boolean; enabled: boolean; label: string; review_label: string | null }
export interface OptionView { id: string; label: string; description: string; markdown: boolean; checked: boolean; enabled: boolean; visible: boolean }
export interface TextView { visible: boolean; enabled: boolean; value: string; placeholder: string; accessible_name: string; max_units: number; max_chars: number }
export interface FormView { id: string; tab: string; prompt: string; body: string; markdown: boolean; monospace_body: boolean; multi: boolean;
  mode: string; mode_visible: boolean; options: OptionView[]; other: OptionView | null; text: TextView | null }
export interface QueueNav { previous: boolean; next: boolean; previous_tip: string; next_tip: string }
export interface CardView { heading: string; progress: string; pending: string; pending_tip: string; nav: QueueNav | null; tabs_visible: boolean;
  page: number; forms: FormView[]; status: string; status_error: boolean; available: boolean; locked: boolean; review: boolean; created_at: number;
  submit: ButtonView; approve: ButtonView; deny: ButtonView; skip: ButtonView; previous: ButtonView; next: ButtonView; retry: boolean; skip_tip: string; retry_tip: string }
export interface QuestionView { card: string; revision: number; epoch: number; asked: string; asked_tip: string; terminal_label: string; view: CardView }
export type QuestionEdit = { kind: "option"; item: string; option: string; checked: boolean } | { kind: "other"; item: string; checked: boolean } | { kind: "text"; item: string; text: string };
export interface ColdAnswer { title: string; text: string; proceed: string }
export type QuestionReply = { kind: "sending" } | { kind: "busy" } | { kind: "cold"; token: number; prompt: ColdAnswer } | { kind: "notice"; text: string };
export type ComposerEvent = { kind: "notice"; text: string; error: boolean; transient: boolean } | { kind: "open_terminal"; key: string };
export interface Reply { question: string; answer: string }
// ComposerView gains: question: QuestionView | null; input_visible: boolean
// (a failed question read is the composer's session_status, A2-1 As built)
// Batch gains: composer_events?: ComposerEvent[]
// store/questions.ts
export const REVIEW_MS = 3000;
export const TICK_MS = 100;
export function createQuestions(port: QuestionPort, clock?: Clock): QuestionsStore;
export const questions: QuestionsStore;
// QuestionsStore: view(), left(), dialog(), apply(view), setActive(on), label(button), enabled(button), edit(e), page(i), navigate(d),
//   allowRetry(), submit(), enter(repeat), decide(allow), skip(), answerCold(proceed)
// Activity.tsx: export function Body(props: { text: string })
```

- [ ] **Step 1: Write the failing store tests** in `client/src/store/questions.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createQuestions, type Clock, type QuestionPort } from "./questions";
import type { ButtonView, CardView, QuestionView } from "../types";

const button = (over: Partial<ButtonView> = {}): ButtonView => ({ visible: true, enabled: true, label: "Submit answer", review_label: null, ...over });

function card(over: Partial<CardView> = {}, revision = 1): QuestionView {
  return {
    card: "question\n[\"studio\\nclaude/sample-project/review\",\"tool:t1\",\"h\"]\nrun-1\nconv-1", revision, asked: "", asked_tip: "", terminal_label: "Open Terminal",
    view: {
      heading: "Agent needs your answer", progress: "", pending: "1 pending", pending_tip: "", nav: null, tabs_visible: false, page: 0, forms: [],
      status: "", status_error: false, available: true, locked: false, review: false, created_at: 0,
      submit: button(), approve: button({ visible: false, label: "Approve once" }), deny: button({ visible: false, label: "Deny" }),
      skip: button({ visible: false, label: "Skip" }), previous: button({ visible: false, label: "← Back" }), next: button({ visible: false, label: "Next →" }),
      retry: false, skip_tip: "", retry_tip: "", ...over,
    },
  };
}

const approval = (revision = 1) => card({ heading: "Agent needs your approval", review: true, submit: button({ visible: false }),
  approve: button({ label: "Approve once", review_label: "Approve once" }), deny: button({ label: "Deny" }) }, revision);

function fakes() {
  const calls: string[] = [];
  let now = 0;
  const ticks = new Set<() => void>();
  const clock: Clock = { now: () => now, every: (_ms, f) => { ticks.add(f); return () => ticks.delete(f); } };
  let next: Record<string, unknown> = { kind: "sending" };
  const port: QuestionPort = {
    edit: async (c, e) => { calls.push(`edit ${JSON.stringify(e)}`); void c; },
    page: async (_c, p) => { calls.push(`page ${p}`); },
    navigate: async (d) => { calls.push(`navigate ${d}`); },
    allowRetry: async () => { calls.push("retry"); },
    submit: async () => { calls.push("submit"); return next as never; },
    decide: async (_c, allow) => { calls.push(`decide ${allow}`); return next as never; },
    skip: async () => { calls.push("skip"); return next as never; },
    cold: async (token, proceed) => { calls.push(`cold ${token} ${proceed}`); return { kind: "busy" }; },
    notify: (text) => { calls.push(`notify ${text}`); },
  };
  return { calls, clock, port, ticking: () => ticks.size, advance(ms: number) { now += ms; ticks.forEach((f) => f()); }, reply(r: Record<string, unknown>) { next = r; } };
}

describe("question cards", () => {
  it("the review delay counts only while the card is active", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(approval());
    const approve = () => q.view()!.view.approve;
    expect([q.label(approve()), q.enabled(approve())]).toEqual(["Approve once (3)", false]);
    f.advance(5000);
    expect(q.label(approve())).toBe("Approve once (3)");
    q.setActive(true);
    f.advance(1000);
    expect(q.label(approve())).toBe("Approve once (2)");
    expect(q.enabled(q.view()!.view.deny)).toBe(true);
    f.advance(2000);
    expect([q.label(approve()), q.enabled(approve())]).toEqual(["Approve once", true]);
    q.setActive(false);
    expect(q.label(approve())).toBe("Approve once (3)");
    dispose();
  }));

  it("a rebuilt card starts the review again, and only a review card ticks", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(approval());
    q.setActive(true);
    f.advance(3000);
    expect(q.label(q.view()!.view.approve)).toBe("Approve once");
    q.apply(approval(2));
    f.advance(100);
    expect(q.label(q.view()!.view.approve)).toBe("Approve once (3)");
    q.apply(card());
    expect(f.ticking()).toBe(0);
    dispose();
  }));

  it("buttons that are off send nothing", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(card({ submit: button({ enabled: false }) }));
    await q.submit();
    q.apply(approval());
    await q.decide(true);
    expect(f.calls).toEqual([]);
    await q.decide(false);
    expect(f.calls).toEqual(["decide false"]);
    dispose();
  }));

  it("holding Enter never submits again", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(card());
    await q.enter(true);
    expect(f.calls).toEqual([]);
    await q.enter(false);
    expect(f.calls).toEqual(["submit"]);
    dispose();
  }));

  it("a cold reply opens the dialog, and Cancel answers it", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(card());
    f.reply({ kind: "cold", token: 7, prompt: { title: "Submit with a cold cache?", text: "…", proceed: "Submit with full context" } });
    await q.submit();
    expect(q.dialog()?.prompt.title).toBe("Submit with a cold cache?");
    await q.answerCold(false);
    expect(q.dialog()).toBeUndefined();
    expect(f.calls).toEqual(["submit", "cold 7 false"]);
    dispose();
  }));

  it("a notice reply raises a sticky error", () => createRoot(async (dispose) => {
    const f = fakes();
    const q = createQuestions(f.port, f.clock);
    q.apply(card());
    f.reply({ kind: "notice", text: "The question or your answers changed. Your draft was kept; review it before submitting again." });
    await q.submit();
    expect(f.calls.at(-1)).toBe("notify The question or your answers changed. Your draft was kept; review it before submitting again.");
    dispose();
  }));
});
```

In `client/src/ui/sessions/transcript.test.ts`:

```ts
  it("answers are labelled and keep their questions", () => {
    const items = buildTranscript([
      { seq: 1, type: "QuestionAnswered", at: 10, detail: "Which checks? → Unit" },
      { seq: 2, type: "UserPromptSubmit", at: 11, detail: "<send_user_message_question_reply>…", question_replies: [{ question: "Which checks?", answer: "Unit" }] },
    ] as ActivityEvent[]);
    expect(items.map((i) => i.label)).toEqual(["You (answer)", "You (answer)"]);
    expect(items[1].replies).toEqual([{ question: "Which checks?", answer: "Unit" }]);
  });

  it("a recorded prompt with answers reads as a recorded answer", () => {
    const items = buildTranscript([], { prompt: "<send_user_message_question_reply>…", prompt_replies: [{ question: "Q", answer: "A" }] });
    expect(items[0].label).toBe("You (recorded answer)");
    expect(items[0].replies).toEqual([{ question: "Q", answer: "A" }]);
  });
```

In `client/src-tauri/src/commands.rs` (its `tests` module; add `#[cfg(test)] mod tests { use super::*; … }` if it has none):

```rust
    #[test]
    fn question_replies_are_annotated_before_the_webview() {
        use zerus_client_core::{ActivityOut, Batch, ComputerId};
        let detail = "<send_user_message_question_reply>{\"questionItemId\":\"q1\",\"question\":\"Which checks?\",\"answer\":\"Unit\"}</send_user_message_question_reply>";
        let mut batch = Batch {
            activity: vec![ActivityOut { computer: ComputerId::from("studio"), key: "run:run-1".into(), kind: "events".into(),
                payload: serde_json::json!({"events": [{"seq": 1, "type": "UserPromptSubmit", "detail": detail}], "cursor": 1}) }],
            ..Batch::default()
        };
        annotate(&mut batch);
        assert_eq!(batch.activity[0].payload["events"][0]["question_replies"], serde_json::json!([{"question": "Which checks?", "answer": "Unit"}]));
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && npm test -- src/store/questions.test.ts src/ui/sessions/transcript.test.ts`
Expected: FAIL (no `questions` store; no labels).

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml commands::`
Expected: compile error (no `annotate`).

- [ ] **Step 3: Annotate in Rust.** In `commands.rs`:

```rust
/// Codex's question replies become `question_replies` on their events
/// (R7's `annotate_replies`), so the webview never parses them.
pub(crate) fn annotate(batch: &mut zerus_client_core::Batch) {
    for out in &mut batch.activity {
        zerus_client_core::questions::annotate_replies(&mut out.payload);
    }
}
```

`next_batch` takes its batch as `let mut batch = live.pump.take(crate::pump::PARK);` and calls `annotate(&mut batch);` before `actions.observe(&batch)`.

- [ ] **Step 4: The store.** Write `client/src/store/questions.ts`:

```ts
import { createRoot, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import type { ButtonView, ColdAnswer, QuestionEdit, QuestionReply, QuestionView } from "../types";

/** Qt's review delay before an approval can be granted, and its timer. */
export const REVIEW_MS = 3000;
export const TICK_MS = 100;

export interface QuestionPort {
  edit(card: string, edit: QuestionEdit): Promise<void>;
  page(card: string, page: number): Promise<void>;
  navigate(direction: number): Promise<void>;
  allowRetry(card: string): Promise<void>;
  submit(card: string): Promise<QuestionReply>;
  decide(card: string, allow: boolean): Promise<QuestionReply>;
  skip(card: string): Promise<QuestionReply>;
  cold(token: number, proceed: boolean): Promise<QuestionReply>;
  notify(text: string): void;
}

export interface Clock { now(): number; every(ms: number, f: () => void): () => void }

const browserClock: Clock = {
  now: () => performance.now(),
  every: (ms, f) => { const id = setInterval(f, ms); return () => clearInterval(id); },
};

export function createQuestions(port: QuestionPort, clock: Clock = browserClock) {
  const [view, setView] = createSignal<QuestionView | undefined>();
  const [left, setLeft] = createSignal(REVIEW_MS / 1000);
  const [dialog, setDialog] = createSignal<{ token: number; prompt: ColdAnswer } | undefined>();
  let active = false;
  let started: number | undefined;
  let shown = "";
  let stop: (() => void) | undefined;
  const secondsLeft = (elapsed: number) => Math.ceil(Math.max(0, REVIEW_MS - elapsed) / 1000);

  /** The count runs only while the card is on screen, the window is active and the card is available. */
  function tick() {
    const v = view();
    if (!v?.view.review) return;
    if (!active || !v.view.available) { started = undefined; setLeft(secondsLeft(0)); return; }
    started ??= clock.now();
    setLeft(secondsLeft(clock.now() - started));
  }

  /** A new card or a rebuilt one starts the review again; only a review card ticks. */
  function apply(next: QuestionView | null | undefined) {
    const id = next ? `${next.card}\n${next.revision}` : "";
    if (id !== shown) { shown = id; started = undefined; setLeft(secondsLeft(0)); }
    setView(next ?? undefined);
    const review = !!next?.view.review;
    if (review && !stop) stop = clock.every(TICK_MS, tick);
    if (!review && stop) { stop(); stop = undefined; }
    tick();
  }

  const reviewing = (b: ButtonView) => b.review_label !== null && left() > 0;
  const label = (b: ButtonView) => (reviewing(b) ? `${b.review_label} (${left()})` : b.label);
  const enabled = (b: ButtonView) => b.visible && b.enabled && !reviewing(b);
  const reply = (r: QuestionReply) => {
    if (r.kind === "cold") setDialog({ token: r.token, prompt: r.prompt });
    else if (r.kind === "notice") port.notify(r.text);
  };
  const card = () => view()?.card;

  async function submit() {
    const v = view();
    if (v && enabled(v.view.submit)) reply(await port.submit(v.card));
  }

  return {
    view, left, dialog, apply, label, enabled, submit,
    setActive(on: boolean) { active = on; tick(); },
    edit(e: QuestionEdit) { const c = card(); if (c !== undefined) void port.edit(c, e); },
    async page(i: number) { const c = card(); if (c !== undefined) await port.page(c, i); },
    async navigate(direction: number) { await port.navigate(direction); },
    async allowRetry() { const c = card(); if (c !== undefined) await port.allowRetry(c); },
    /** Enter on an option or the answer line: Submit when it shows, never on auto-repeat. */
    async enter(repeat: boolean) { if (!repeat && view()?.view.submit.visible) await submit(); },
    async decide(allow: boolean) {
      const v = view();
      if (v && enabled(allow ? v.view.approve : v.view.deny)) reply(await port.decide(v.card, allow));
    },
    async skip() { const v = view(); if (v && enabled(v.view.skip)) reply(await port.skip(v.card)); },
    async answerCold(proceed: boolean) {
      const d = dialog();
      if (!d) return;
      setDialog(undefined);
      reply(await port.cold(d.token, proceed));
    },
  };
}

export type QuestionsStore = ReturnType<typeof createQuestions>;

export const questions = createRoot(() => createQuestions({
  edit: bridge.questionEdit, page: bridge.questionPage, navigate: bridge.questionNavigate, allowRetry: bridge.questionAllowRetry,
  submit: bridge.questionSubmit, decide: bridge.questionDecide, skip: bridge.questionSkip, cold: bridge.questionCold,
  notify: (text) => notify("error", text, true),
}));
```

The bridge, in `client/src/bridge.ts`. Outside Tauri each wrapper rejects with "Actions need the desktop app.", as the composer's do:

```ts
export const questionEdit = (card: string, epoch: number, edit: QuestionEdit) => desktop<void>("question_edit", { card, epoch, edit });
export const questionPage = (card: string, page: number) => desktop<void>("question_page", { card, page });
export const questionNavigate = (direction: number) => desktop<void>("question_navigate", { direction });
export const questionAllowRetry = (card: string) => desktop<void>("question_allow_retry", { card });
export const questionSubmit = (card: string) => desktop<QuestionReply>("question_submit", { card });
export const questionDecide = (card: string, allow: boolean) => desktop<QuestionReply>("question_decide", { card, allow });
export const questionSkip = (card: string) => desktop<QuestionReply>("question_skip", { card });
export const questionCold = (token: number, proceed: boolean) => desktop<QuestionReply>("question_cold", { token, proceed });
```

`normalize` keeps `composer_events`, and `isEmptyBatch` treats a batch with events as not empty.

- [ ] **Step 5: Route it** in `client/src/store/session.ts`:

```ts
import { questions } from "./questions";
import { terminal } from "./terminal";
import { notify } from "./actions";
import { setDetailTab } from "./ui";
import type { ComposerEvent } from "../types";

function composerEvent(e: ComposerEvent) {
  // Qt's showNotice: an error stays until dismissed unless it is transient.
  if (e.kind === "notice") notify(e.error ? "error" : "info", e.text, e.error && !e.transient);
  // Codex's hooks review and Update now continue in Terminal (SessionsWindow.cpp:1157-1178).
  else if (composer.view()?.key === e.key) { setDetailTab("terminal"); void terminal.connect(); }
}
```

In `receive`: `if (b.composer) questions.apply(b.composer.question);` after `composer.apply(...)`, then `for (const e of b.composer_events ?? []) composerEvent(e);`. The selection effect calls `questions.apply(undefined)` when no session is selected.

Task 6's `view()` fixture in `store/composer.test.ts` gains `question: null, input_visible: true`, so the type check passes. A failed question read needs nothing here: it is the composer's `session_status`, which Task 6 shows.

- [ ] **Step 6: The transcript.** In `client/src/ui/sessions/types.ts`, `TranscriptItem` gains `replies?: Reply[]`, and `ActivityEvent` gains `question_replies?: Reply[]`. In `transcript.ts`:
  - A user item's label is "You (answer)" when `e.type === "QuestionAnswered"` or `e.question_replies?.length`, and it carries `replies: e.question_replies`.
  - `recordedContext` reads `summary.prompt_replies`. With replies, the prompt item is labelled "You (recorded answer)" and carries them.

In `Activity.tsx`:
- Export the existing `Body` (`export function Body(props: { text: string })`).
- A user item with `replies` renders, for each reply:

```tsx
<div class="act-reply">
  <div class="act-reply-q">Question</div>
  <p class="wrap-pre">{r.question}</p>
  <div class="act-reply-a"><b>Your answer</b><p class="wrap-pre">{r.answer}</p></div>
</div>
```

  It renders this in place of the plain text body.

- [ ] **Step 7: Write `client/src/ui/sessions/QuestionCard.tsx`:**

```tsx
import { For, Show, createEffect, createMemo, onCleanup, onMount, untrack } from "solid-js";
import { Icon } from "../lib/icons";
import { questions } from "../../store/questions";
import { terminal } from "../../store/terminal";
import { setDetailTab } from "../../store/ui";
import { Body } from "./Activity";
import type { FormView, OptionView } from "../../types";

/** The shown question card (Qt's QuestionCard), between Activity and the composer. */
export function QuestionCard() {
  // Rebuilt for a new card or a rebuilt one only: typing never remounts the fields.
  const ident = createMemo(() => { const q = questions.view(); return q ? `${q.card}\n${q.revision}` : undefined; });
  return (
    <>
      <Show when={ident()} keyed>{() => <Card />}</Show>
      <ColdAnswerDialog />
    </>
  );
}

function Card() {
  const q = () => questions.view()!;
  const c = () => q().view;
  const form = () => c().forms[c().page];
  let el!: HTMLDivElement;
  onMount(() => {
    // The review delay counts only while the card is on screen and the window is active.
    let visible = false;
    const update = () => questions.setActive(visible && document.visibilityState === "visible" && document.hasFocus());
    const observer = new IntersectionObserver((entries) => { visible = entries.some((e) => e.isIntersecting); update(); });
    observer.observe(el);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
      questions.setActive(false);
    });
  });
  const native = () => q().terminal_label === "Open native UI";
  const terminalTip = () => (native() ? "Arrives with Native UI." : terminal.status().available ? undefined : terminal.status().reason);
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return;
    e.preventDefault();
    void questions.enter(e.repeat);
  };
  return (
    <div class="qcard" ref={el} role="group" aria-label={c().heading}>
      <div class="qcard-head">
        <b class="qcard-title">{c().heading}</b><span class="grow" />
        <Show when={c().progress}><span class="qcard-dim">{c().progress}</span></Show>
      </div>
      <div class="qcard-meta">
        <Show when={q().asked}><span class="qcard-dim" title={q().asked_tip}>{q().asked}</span></Show>
        <span class="grow" />
        <Show when={c().pending}><span class="qcard-dim" title={c().pending_tip}>{c().pending}</span></Show>
        <Show when={c().nav}>{(nav) => (
          <>
            <button class="icon-btn small" aria-label="Previous queued question" title={nav().previous_tip} disabled={!nav().previous}
              onClick={() => void questions.navigate(-1)}><Icon name="chevron-left" size={12} /></button>
            <button class="icon-btn small" aria-label="Next queued question" title={nav().next_tip} disabled={!nav().next}
              onClick={() => void questions.navigate(1)}><Icon name="chevron-right" size={12} /></button>
          </>
        )}</Show>
      </div>
      <Show when={c().tabs_visible}>
        <div class="qcard-tabs tabs" role="tablist">
          <For each={c().forms}>{(f, i) => (
            <button class="tab" role="tab" classList={{ on: i() === c().page }} aria-selected={i() === c().page} disabled={c().locked}
              onClick={() => void questions.page(i())}>{f.tab}</button>
          )}</For>
        </div>
      </Show>
      <Show when={form()} keyed>{(f) => <Form form={f} onKey={onKey} />}</Show>
      <p class="qcard-status" classList={{ error: c().status_error }} title={c().status}>{c().status}</p>
      <div class="qcard-actions">
        <button class="btn ghost" disabled={native() || !terminal.status().available} title={terminalTip()}
          onClick={() => { setDetailTab("terminal"); void terminal.connect(); }}>{q().terminal_label}</button>
        <span class="grow" />
        <Show when={c().skip.visible}><button class="btn ghost" title={c().skip_tip} disabled={!questions.enabled(c().skip)} onClick={() => void questions.skip()}>{c().skip.label}</button></Show>
        <Show when={c().retry}><button class="btn ghost" title={c().retry_tip} onClick={() => void questions.allowRetry()}>Allow retry</button></Show>
        <Show when={c().previous.visible}><button class="btn ghost" disabled={!c().previous.enabled} onClick={() => void questions.page(c().page - 1)}>{c().previous.label}</button></Show>
        <Show when={c().next.visible}><button class="btn ghost accent" disabled={!c().next.enabled} onClick={() => void questions.page(c().page + 1)}>{c().next.label}</button></Show>
        <Show when={c().submit.visible}><button class="btn primary" disabled={!questions.enabled(c().submit)} onClick={() => void questions.submit()}>{questions.label(c().submit)}</button></Show>
        <Show when={c().deny.visible}><button class="btn ghost" disabled={!questions.enabled(c().deny)} onClick={() => void questions.decide(false)}>{c().deny.label}</button></Show>
        <Show when={c().approve.visible}><button class="btn primary" disabled={!questions.enabled(c().approve)} onClick={() => void questions.decide(true)}>{questions.label(c().approve)}</button></Show>
      </div>
    </div>
  );
}

function Form(props: { form: FormView; onKey: (e: KeyboardEvent) => void }) {
  const f = () => questions.view()?.view.forms.find((form) => form.id === props.form.id) ?? props.form;
  const group = `qcard-${props.form.id}`;
  return (
    <div class="qcard-page">
      <div class="qcard-scroll">
        <Show when={f().markdown} fallback={<p class="qcard-prompt wrap-pre">{f().prompt}</p>}><div class="qcard-prompt"><Body text={f().prompt} /></div></Show>
        <Show when={f().body}>
          <Show when={f().markdown} fallback={<p class="qcard-body wrap-pre" classList={{ mono: f().monospace_body }}>{f().body}</p>}><div class="qcard-body"><Body text={f().body} /></div></Show>
        </Show>
        <Show when={f().mode_visible}><p class="qcard-mode">{f().mode}</p></Show>
        <For each={f().options}>{(o) => <Choice form={f()} option={o} group={group} onKey={props.onKey} />}</For>
        <Show when={f().other}>{(o) => <Choice form={f()} option={o()} group={group} other onKey={props.onKey} />}</Show>
      </div>
      <Show when={f().text?.visible ? f().text : undefined}>{(t) => (
        <input class="qcard-text" type="text" placeholder={t().placeholder} aria-label={t().accessible_name} maxLength={t().max_units}
          disabled={!t().enabled} value={untrack(() => t().value)}
          onInput={(e) => {
            const input = e.currentTarget;
            // OpenCode counts its bound in characters, as Qt's CharacterLimit does.
            if (t().max_chars > 0 && [...input.value].length > t().max_chars) input.value = [...input.value].slice(0, t().max_chars).join("");
            questions.edit({ kind: "text", item: props.form.id, text: input.value });
          }}
          onKeyDown={props.onKey} />
      )}</Show>
    </div>
  );
}

function Choice(props: { form: FormView; option: OptionView; group: string; other?: boolean; onKey: (e: KeyboardEvent) => void }) {
  const o = () => props.option;
  const toggle = (checked: boolean) => questions.edit(props.other
    ? { kind: "other", item: props.form.id, checked }
    : { kind: "option", item: props.form.id, option: o().id, checked });
  return (
    <Show when={o().visible}>
      <label class="qchoice" classList={{ on: o().checked, off: !o().enabled }}>
        <input type={props.form.multi ? "checkbox" : "radio"} name={props.group} checked={o().checked} disabled={!o().enabled}
          onChange={(e) => toggle(e.currentTarget.checked)} onKeyDown={props.onKey} />
        <span class="qchoice-copy">
          <span class="qchoice-title">{o().label}</span>
          <Show when={o().description}>
            <Show when={o().markdown} fallback={<span class="qchoice-desc wrap-pre">{o().description}</span>}><span class="qchoice-desc"><Body text={o().description} /></span></Show>
          </Show>
        </span>
      </label>
    </Show>
  );
}

/** "Submit with a cold cache?": Cancel is the default and Escape (SessionsWindow.cpp:5111-5131). */
function ColdAnswerDialog() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    const d = questions.dialog();
    if (!el) return;
    if (!d) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="qcard-cold-title" aria-describedby="qcard-cold-text" style={{ width: "460px" }}
      onClose={() => { if (questions.dialog()) void questions.answerCold(false); }}>
      <Show when={questions.dialog()}>{(d) => (
        <>
          <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="qcard-cold-title">{d().prompt.title}</h2></div>
          <div class="dlg-body"><div class="confirm-body wrap-pre" id="qcard-cold-text">{d().prompt.text}</div></div>
          <div class="dlg-foot">
            <span class="spacer" />
            <button type="button" class="btn primary" data-initial autofocus onClick={() => void questions.answerCold(false)}>Cancel</button>
            <button type="button" class="btn" onClick={() => void questions.answerCold(true)}>{d().prompt.proceed}</button>
          </div>
        </>
      )}</Show>
    </dialog>
  );
}
```

- [ ] **Step 8: Place it.**
  - In `SessionDetail.tsx`, the composer column (Task 6 Step 9) becomes:

```tsx
<div class="cmp-col">
  <QuestionCard />
  <Composer />
</div>
```

  - In `Composer.tsx`, `const input = () => v()?.input_visible !== false;`. The two `ResizeHandle`s, the `<textarea>` and `.cmp-actions` render inside `<Show when={input()}>`. The toolbar stays, as Qt's `setInputVisible` keeps it.
  - In `detail.css`:

```css
.qcard { display: flex; flex-direction: column; gap: 9px; padding: 12px 14px; margin-bottom: 8px; background: var(--z-color-surface);
  border: 1px solid var(--z-color-border); border-radius: var(--z-radius-lg); max-height: 60vh; }
.qcard-head, .qcard-meta, .qcard-actions { display: flex; align-items: center; gap: 8px; }
.qcard-title { color: var(--z-color-accent); font-weight: 600; }
.qcard-dim, .qcard-mode { color: var(--z-color-muted); font-size: 11px; }
.qcard-page { display: flex; flex-direction: column; gap: 7px; min-height: 0; }
.qcard-scroll { display: flex; flex-direction: column; gap: 7px; overflow-y: auto; min-height: 40px; }
.qcard-prompt { font-weight: 600; }
.qcard-body { color: var(--z-color-muted); font-size: 11px; }
.qchoice { display: flex; gap: 8px; align-items: flex-start; padding: 9px 12px 9px 10px; border: 1px solid var(--z-color-border);
  border-radius: var(--z-radius-md); background: var(--z-color-raised); cursor: pointer; }
.qchoice:hover, .qchoice.on { border-color: var(--z-color-accent); }
.qchoice.on { background: var(--z-color-accent-bg); }
.qchoice.off { cursor: default; opacity: .7; }
.qchoice-copy { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
.qchoice-desc { color: var(--z-color-muted); font-size: 11px; }
.qcard-text { min-height: 35px; padding: 7px; border: 1px solid var(--z-color-border); border-radius: var(--z-radius-md); background: var(--z-color-raised); color: var(--z-color-text); }
.qcard-text:focus { border-color: var(--z-color-accent); outline: none; }
.qcard-status { color: var(--z-color-muted); font-size: 11px; margin: 0; }
.qcard-status.error, .qcard-error { color: var(--z-color-danger); }
.qcard-error { font-size: 11px; margin: 0 0 6px; }
.wrap-pre { white-space: pre-wrap; }
.act-reply-q { color: var(--z-color-muted); font-size: 10px; margin-bottom: 3px; }
.act-reply-a { border: 1px solid var(--z-color-border); background: var(--z-color-code-bg); padding: 10px; border-radius: var(--z-radius-md); margin-bottom: 10px; }
.act-reply-a b { color: var(--z-color-accent); font-size: 10px; }
```

`.mono` (`base.css`) already gives `--z-font-mono`.

- [ ] **Step 9: Run the checks**

Run: `cd client && npm test -- src/store/questions.test.ts src/ui/sessions/transcript.test.ts && npm run check && npm test && python3 ../scripts/check-ui-separators.py`
Expected: PASS, the 6 store tests and the 2 transcript tests; no type errors; no separators.

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml commands::`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add client/src client/src-tauri
git commit -m "feat(client): the question card, its review delay and cold cache question, and answers in Activity"
```

---

### Task A2-4: The native queue, Send now and Send and push now in the hub

**As built (a2hub; webui rulings, 2026-10-29).** The current core and Tasks 4a/4b/5 surfaces win over the snippets below:
- `begin_queue` captures the selected session, its native queue id and its run/conversation under the hub lock. A push's snapshot match and its Send now claim share that lock, so navigation cannot redirect the action. The worker retains the captured target through navigation and starts nothing after the hub closes.
- A completed Send now removes `input_queue` from the actual held details, hides its panel and calls `refresh_details`, retaining Task 4a's live-stream no-poll policy (Qt `inspect`, 3467-3492). Only an accepted state or inspection shows a queue again. The stale-queue notice is the full transient Qt error.
- Lane::Queue busy is a quiet pre-dispatch refusal: release the window-wide request, keep the queue and show no notice. Other errors or dispatch panics are uncertain via A2-2's `outcome_of`; nothing retries them automatically.
- **Approved deviation from Qt for no replay:** an explicit Send now consumes an armed push of the same session when the captured queue holds its message, including while the text acknowledgement is pending. Done, failed and uncertain outcomes all leave that push consumed; definite failure leaves the message queued for an explicit next action. No extra notice appears. Qt leaves `m_push` armed in `sendQueuedNow`; consuming it prevents a later fresh snapshot from repeating an uncertain action through that older push. An unrelated queue or another session's push stays untouched.
- `send_push` checks the current handle, editor epoch and text, arms only the selected session, and calls the existing `send_held` under that same lock (`pub(super)` for this reuse). Its ordinary durable save, request identity, cold/start dialogs and continuations stay on Task 5's path. No text is resent by the push.
- `queue_send_now` and `send_push` commands are async blocking-pool wrappers and refuse a child webview. Their normal IPC results remain a bool and `SendReply`. All notices use A2-2's existing bounded `composer_events`; no second event field is added.
- **Review follow-up (2026-10-29):** the empty-field/in-flight `send_push` chord calls `begin_queue` under its original hub lock before releasing the selection. Every Busy result hands the editor back, including an empty field and a disabled Send, just as `send_message` does. `after_observe(accepted_details)` evaluates a push only after an accepted state or successful, still-current inspection, never for a stream-ended frame, failed inspection or superseded read (Qt `pushInspected`, `SessionsWindow.cpp:3627, 3653`). The redundant `Queue.hidden` field is removed; removing `details.input_queue` provides the required invalidation. The brief's notice-title snippet is corrected to Qt's display title (`project / tag`, `SessionPresentation.h:196-201`), rather than the row's compact title.
- **Required-card refusal (A2-3/A2-4 review ruling).** Before its empty/in-flight native queue branch or arming a push, `send_push` checks the same `question_part(&hub).1` that supplies `ComposerView.input_visible`. A shown required question (`input_visible == false`) returns Busy with a hand-back, no queue dispatch and no armed push. A2-3 adds the matching `send_message` refusal.


**Depends on:**
- R8 (`input_queue::{queue_view, can_send_now, send_now_notice, Notice, NOTHING_QUEUED, Pusher, PushFacts, PushStep, QueueView}`, `Action::SendNow`, `Lane::Queue`);
- Tasks 4a, 4b and 5 (`observe`, `refresh_details`, `after_observe`, `send_now`, `prompts`);
- A2-2's `ComposerEvent` and `event`. If A2-2 is not on main yet, this task adds exactly A2-2's Step 3, and A2-2 rebases onto it.

**Qt reference:**
- `tray/src/SessionsWindowShortcuts.cpp:248-341`;
- `tray/src/SessionsWindow.cpp`: 1218-1221, 1859-1873, 3976-3980, 4908-4912, 4939;
- `tray/src/MessageComposer.cpp:811-847`;
- `tray/src/ActivityView.cpp:1890-1895`.

**Files:**
- Create: `client/src-tauri/src/composer/queue.rs`
- Modify: `client/src-tauri/src/composer/mod.rs` (`pub mod queue`, `Hub.queue`, `ComposerView.queue`, the hooks in `open`, `observe`, `after_observe` and `tick`)
- Modify: `client/src-tauri/src/composer/send.rs` (two lines in `send_now`; one in `refresh_details`)
- Modify: `client/src-tauri/src/lib.rs` (two commands)
- Test: unit tests in `composer/queue.rs`

**Interfaces:**
- Consumes: R8's surface; Task 4b's `send_message`, `SendReply`, `Prompt.key`, `Hub.prompts`, test helpers; A2-2's `ComposerEvent`, `event`, `take_events`.
- Produces:

```rust
// composer/queue.rs
#[derive(Default)] pub(crate) struct Queue { pub(crate) pusher: Pusher, pub(crate) sending: Option<String>, pub(crate) hidden: bool, /* session */ }
impl Composers {
    pub fn send_queue_now(&self, queue_id: Option<String>) -> bool;
    pub fn send_push(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> SendReply;
    pub(crate) fn queue_part(&self, hub: &Hub) -> Option<QueueView>;
    pub(crate) fn push_finished(&self, request: &str, ok: bool, uncertain: bool);
    pub(crate) fn queue_observed(&self);
    pub(crate) fn push_selection(&self, hub: &mut Hub, key: Option<&str>);
    pub(crate) fn push_check(&self);
}
// ComposerView gains: pub queue: Option<QueueView>,
// commands: queue_send_now(queue_id: Option<String>) -> bool; send_push(handle, rev, epoch, text, position, anchor) -> SendReply
```

- [ ] **Step 1: Write the failing tests** at the end of `client/src-tauri/src/composer/queue.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::send::tests::{opened, ready, receipt, send_hub, sent, state};
    use crate::composer::tests::{dir, until};
    use crate::composer::{lock, ComposerEvent};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn working() -> Value {
        let mut details = ready();
        details["activity"] = json!("busy");
        details["phase"] = json!("working");
        details
    }

    fn queued(text: &str, can: bool) -> Value {
        let mut details = working();
        details["input_queue"] = json!({"id": "queue-1", "text": text, "can_send_now": can, "hint": "Send the queued messages now"});
        details
    }

    fn submitted(action: &Action, id: &str) -> Outcome {
        match action {
            Action::SendNow { queue_id } => Outcome::Done { receipt: json!({"status": "submitted", "request_id": id, "queue_id": queue_id}), cleanup: None },
            _ => receipt(action, id),
        }
    }

    fn send_nows(fake: &crate::composer::send::tests::Fake) -> usize {
        sent(fake).iter().filter(|(_, action, _)| matches!(action, Action::SendNow { .. })).count()
    }

    fn wait_for(done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn notice(text: &str) -> ComposerEvent {
        ComposerEvent::Notice { text: text.into(), error: false, transient: false }
    }

    #[test]
    fn send_now_runs_once_and_hides_the_panel_until_the_next_state() {
        let root = dir("send-now");
        let (c, fake) = send_hub(&root);
        let (gate, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        *lock(&fake.outcome) = Box::new(move |action, id| {
            if matches!(action, Action::SendNow { .. }) {
                let _ = lock(&wait).recv();
            }
            submitted(action, id)
        });
        let view = opened(&c, queued("❯ first", true));
        let queue = view.queue.clone().unwrap();
        assert_eq!((queue.text.as_str(), queue.label.as_str(), queue.enabled), ("❯ first", "Send now", true));
        assert!(c.send_queue_now(Some("queue-1".into())));
        until(&c, |v| v.queue.as_ref().is_some_and(|q| q.label == "Sending…" && !q.enabled));
        assert!(!c.send_queue_now(None), "one send-now at a time");
        gate.send(()).unwrap();
        until(&c, |v| v.queue.is_none());
        assert_eq!(c.take_events(), vec![notice("Send now requested. Waiting for the agent to accept the queue.")]);
        c.observe(&state(queued("❯ first", true)));
        assert!(c.current().unwrap().queue.is_some(), "the next state shows the queue again");
        assert_eq!(send_nows(&fake), 1);
    }

    #[test]
    fn nothing_queued_says_so() {
        let root = dir("nothing-queued");
        let (c, fake) = send_hub(&root);
        opened(&c, working());
        assert!(!c.send_queue_now(None));
        assert_eq!(c.take_events(), vec![notice("Nothing queued can be sent now")]);
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_push_sends_the_queue_once_the_agent_queues_it() {
        let root = dir("push");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        assert_eq!(c.send_push(view.handle, 1, view.text_epoch, "please also run lint".into(), 20, 20), SendReply::Sending);
        until(&c, |v| !v.sending && v.text.is_empty());
        assert_eq!(send_nows(&fake), 0, "nothing is queued yet");
        c.observe(&state(queued("❯ please also run lint", true)));
        wait_for(|| send_nows(&fake) == 1);
        let calls = sent(&fake);
        assert_eq!(calls.iter().filter(|(_, action, _)| matches!(action, Action::Send { .. })).count(), 1, "a push never sends text again");
        let Some((_, Action::SendNow { queue_id }, _)) = calls.last() else { panic!("no send-now") };
        assert_eq!(queue_id, "queue-1");
        c.observe(&state(queued("❯ please also run lint", true)));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(send_nows(&fake), 1, "the push ended with its one send-now");
    }

    #[test]
    fn a_push_with_an_empty_field_is_send_now() {
        let root = dir("push-empty");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, queued("❯ earlier", true));
        assert_eq!(c.send_push(view.handle, 1, view.text_epoch, String::new(), 0, 0), SendReply::Busy);
        wait_for(|| send_nows(&fake) == 1);
    }

    #[test]
    fn selecting_another_session_ends_the_push() {
        let root = dir("push-switch");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).title = "review".into();
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5);
        until(&c, |v| !v.sending && v.text.is_empty());
        c.take_events();
        c.open(Some(SessionRef { computer: "studio".into(), key: "run:run-2".into(), name: "codex/sample-project/other".into(), archive_id: None }));
        assert_eq!(c.take_events(), vec![notice("The message to review was not pushed: another session was selected")]);
    }

    #[test]
    fn a_push_ends_after_twenty_seconds() {
        let root = dir("push-late");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(submitted);
        let view = opened(&c, working());
        c.send_push(view.handle, 1, view.text_epoch, "later".into(), 5, 5);
        until(&c, |v| !v.sending && v.text.is_empty());
        c.take_events();
        *lock(&fake.now) += 21.0;
        c.tick();
        assert_eq!(c.take_events(), vec![notice("The message was sent without push: the agent did not offer to send it now within 20 seconds")]);
        c.observe(&state(queued("❯ later", true)));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(send_nows(&fake), 0, "a snapshot after the deadline never sends the queue");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::queue::`
Expected: compile errors (no `queue` module).

- [ ] **Step 3: Write `client/src-tauri/src/composer/queue.rs`** above its tests:

```rust
//! The agent's native queue and Send and push now, as Qt runs them
//! (`SessionsWindowShortcuts.cpp:248-341`, `SessionsWindow.cpp:1859-1873`,
//! `3976-3980`, `4908-4912`, `4939`). The rules are the core's
//! (`input_queue`, R8): a push never sends text, only the agent's own
//! send-now of the snapshot that holds the message.

use serde_json::Value;
use zerus_client_core::input_queue::{self as rules, Notice, PushFacts, PushStep, Pusher, QueueView};
use zerus_client_core::{Action, Outcome, Target};

use super::send::SendReply;
use super::{ComposerEvent, Composers, Hub, SessionRef};

#[derive(Default)]
pub(crate) struct Queue {
    pub(crate) pusher: Pusher,
    /// The session (composer key) whose send-now runs: one in the app (`m_queueSendRequest`).
    pub(crate) sending: Option<String>,
    /// A send-now of the open session ended: its queue hides until new
    /// details arrive (`m_details.remove("input_queue")`).
    pub(crate) hidden: bool,
    /// The pushed session, for its title when the push ends.
    session: Option<SessionRef>,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn event(notice: Notice) -> ComposerEvent {
    ComposerEvent::Notice { text: notice.text, error: notice.error, transient: notice.transient }
}

impl Composers {
    /// The open session's queue panel (`ActivityView::render` 1890-1895).
    pub(crate) fn queue_part(&self, hub: &Hub) -> Option<QueueView> {
        if hub.queue.hidden {
            return None;
        }
        let ctx = self.context(hub)?;
        let details = ctx.details.as_deref()?;
        let sending = hub.queue.sending.as_deref() == Some(ctx.key.as_str());
        rules::queue_view(details, ctx.row.tracked, ctx.online, ctx.facts.inspect_error, sending)
    }

    /// Send now: the panel's button, the window's chord, an empty push, or a
    /// push's snapshot (`sendQueuedNow`). One runs at a time in the app.
    pub fn send_queue_now(&self, queue_id: Option<String>) -> bool {
        let mut hub = self.lock();
        if hub.closed || hub.queue.sending.is_some() {
            return false;
        }
        let ctx = self.context(&hub);
        let details = ctx.as_ref().and_then(|ctx| ctx.details.clone());
        let id = queue_id.unwrap_or_else(|| details.as_deref().map(|d| d["input_queue"]["id"].as_str().unwrap_or("").to_owned()).unwrap_or_default());
        let ready = matches!((&ctx, &details), (Some(ctx), Some(details)) if rules::can_send_now(details, ctx.online, &id));
        let (Some(ctx), Some(details), true) = (ctx, details, ready) else {
            self.event(&mut hub, event(Notice { text: rules::NOTHING_QUEUED.into(), error: false, transient: false }));
            drop(hub);
            self.changed();
            return false;
        };
        hub.queue.sending = Some(ctx.key.clone());
        drop(hub);
        self.changed();
        let target = Target {
            computer: ctx.session.computer.clone(),
            name: ctx.session.name.clone(),
            run_id: text(&details, "run_id").to_owned(),
            conversation_id: text(&details, "conversation_id").to_owned(),
        };
        let (me, key) = (self.clone(), ctx.key.clone());
        (self.0.deps.spawn)(Box::new(move || {
            let outcome = (me.0.deps.dispatch)(target, Action::SendNow { queue_id: id }, uuid::Uuid::new_v4().to_string())
                .unwrap_or_else(|error| Outcome::Uncertain { detail: error });
            me.queue_sent(&key, &outcome);
        }));
        true
    }

    /// `queueSendFinished` (1860-1872): only a still-selected session hears how
    /// it ended; nothing is sent again by itself.
    fn queue_sent(&self, key: &str, outcome: &Outcome) {
        let mut hub = self.lock();
        hub.queue.sending = None;
        let current = hub.open_key.as_deref() == Some(key);
        if current {
            hub.queue.hidden = true;
            self.event(&mut hub, event(rules::send_now_notice(outcome)));
        }
        drop(hub);
        self.changed();
        if current {
            self.refresh_details();
        }
    }

    /// Send and push now (`MessageComposer::push`): with nothing typed, or a
    /// message on its way, it is Send now. Otherwise, when Send would run, it
    /// arms a push for the open session and sends as usual.
    pub fn send_push(&self, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> SendReply {
        self.edit(handle, rev, epoch, text.clone(), position, anchor);
        let mut hub = self.lock();
        let Some(key) = hub.open_key.clone() else { return SendReply::Busy };
        let Some(composer) = hub.composers.get(&key).filter(|c| c.handle == handle) else { return SendReply::Busy };
        if composer.sending || !composer.has_content() {
            drop(hub);
            self.send_queue_now(None);
            return SendReply::Busy;
        }
        let message = composer.draft.text.clone();
        // Qt's send() does nothing while Send is disabled: no push either.
        if !self.view_of(&hub, &key).is_some_and(|view| view.can_send) {
            return SendReply::Busy;
        }
        if let Some(notice) = hub.queue.pusher.arm(&key, &message, self.now()) {
            self.event(&mut hub, event(notice));
        }
        hub.queue.session = hub.open.clone();
        drop(hub);
        self.changed();
        self.send_message(handle, rev, epoch, text, position, anchor)
    }

    /// The send ended (`pushSendFinished`).
    pub(crate) fn push_finished(&self, request: &str, ok: bool, uncertain: bool) {
        let mut hub = self.lock();
        if let Some(notice) = hub.queue.pusher.send_finished(request, ok, uncertain) {
            self.event(&mut hub, event(notice));
        }
    }

    /// What the push waits on: a dialog of that message, its save or its call,
    /// or a start or compaction before it (`checkPush`).
    fn push_facts(&self, hub: &Hub) -> PushFacts {
        let Some(push) = hub.queue.pusher.active() else { return PushFacts::default() };
        let composer = hub.composers.get(&push.key);
        PushFacts {
            dialog: hub.prompts.values().any(|prompt| prompt.key == push.key) || hub.answers.cold.is_some(),
            sending: composer.is_some_and(|c| c.sending),
            waiting: composer.is_some_and(|c| c.waiting.is_some()),
        }
    }

    /// Each new state of the open session (`pushInspected`): the push sends
    /// the snapshot that holds its message, or ends.
    pub(crate) fn queue_observed(&self) {
        let mut hub = self.lock();
        let (Some(key), Some(details)) = (hub.open_key.clone(), hub.followed.details.clone()) else { return };
        let facts = self.push_facts(&hub);
        let busy = hub.queue.sending.is_some();
        let step = hub.queue.pusher.inspected(&key, &details, self.now(), facts, busy);
        match step {
            PushStep::Wait => {}
            PushStep::End(notice) => {
                if let Some(notice) = notice {
                    self.event(&mut hub, event(notice));
                }
            }
            PushStep::SendNow(id) => {
                drop(hub);
                self.send_queue_now(Some(id));
            }
        }
    }

    /// Another session (or none) is about to open (`pushSelectionChanged`).
    pub(crate) fn push_selection(&self, hub: &mut Hub, key: Option<&str>) {
        let Some(push) = hub.queue.pusher.active() else { return };
        if key == Some(push.key.as_str()) {
            return;
        }
        let fallback = push.key.split('\n').nth(1).unwrap_or_default().to_owned();
        let title = hub.queue.session.as_ref()
            .and_then(|s| (self.0.deps.rows)(&s.computer, &s.key, &s.name))
            .map(|(row, _)| row.title)
            .filter(|title| !title.is_empty())
            .unwrap_or(fallback);
        if let Some(notice) = hub.queue.pusher.selection_changed(key, &title) {
            self.event(hub, event(notice));
        }
    }

    /// Once a second while a push waits (`m_pushTimer`).
    pub(crate) fn push_check(&self) {
        let mut hub = self.lock();
        if hub.queue.pusher.active().is_none() {
            return;
        }
        let facts = self.push_facts(&hub);
        if let Some(notice) = hub.queue.pusher.check(self.now(), facts) {
            self.event(&mut hub, event(notice));
            drop(hub);
            self.changed();
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn queue_send_now(state: tauri::State<'_, Composers>, queue_id: Option<String>) -> bool {
    state.send_queue_now(queue_id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn send_push(state: tauri::State<'_, Composers>, handle: u64, rev: u64, epoch: u64, text: String, position: i64, anchor: i64) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.send_push(handle, rev, epoch, text, position, anchor)).await.map_err(|e| e.to_string())
}
```

`push_facts` reads `hub.answers.cold` (A2-2). When A2-2 is not on main, leave that term out; A2-2 then adds it, since a cold question of an answer is a modal dialog in Qt too.

- [ ] **Step 4: The hooks.**
  - `composer/mod.rs`:
    - `pub mod queue;`, `Hub` gains `pub(crate) queue: queue::Queue`, and `ComposerView` gains `pub queue: Option<QueueView>`, filled by `self.queue_part(hub)`.
    - `open` starts with these lines, before the session is replaced:

      ```rust
      let next = session.as_ref().map(|s| draft_key(&s.computer, &s.name, s.archive_id.as_deref()));
      self.push_selection(&mut hub, next.as_deref());
      ```

    - `observe`'s `"state"` arm sets `hub.queue.hidden = false;`.
    - `after_observe` calls `self.queue_observed();` before its `self.changed()`.
    - `tick` calls `self.push_check();`.
  - `composer/send.rs`:
    - In `refresh_details`, the `Some(value)` arm sets `hub.queue.hidden = false;`. A failed inspect keeps the queue hidden, as Qt's removal holds until an inspection is accepted.
    - In `send_now`, the block that records the local row adds `hub.queue.pusher.dispatched(&delivery.key, &delivery.text, &request_id, &delivery.run, &delivery.conversation);` after `hub.local.sending(...)`.
    - In `send_now`, the line before `self.delivery_finished(...)` is `self.push_finished(&request_id, ok, uncertain);`.

- [ ] **Step 5: Register** `composer::queue::queue_send_now` and `composer::queue::send_push` in `lib.rs`.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::`
Expected: PASS, the 6 new tests and every earlier composer test. Clippy and fmt clean.

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri
git commit -m "feat(client): the native queue, Send now and Send and push now in the composer hub"
```

---

### Task A2-5: The queue panel, the push chord and "You (queued)"

**As built** (webui rulings, 2026-10-29; these override the snippets below):
- The existing shortcut router and registry own both commands. There is no second `chords.ts`, chord parser or window key listener. `queue.sendNow` registers at window scope while Sessions shows a selected session, on every detail tab; Terminal uses the router's existing context rule. `message.push` registers on the message field only while its composer is open and its input is visible (`Shortcuts.cpp:39-41,61,70`; `MessageComposer.cpp:841-853`).
- There are exactly three Send now sources: the queue panel's button, the window shortcut, and the push chord on an empty or sending field. There is no separate Activity link (`ActivityView.cpp:1292`; `SessionsWindow.cpp:1221,1859`; `MessageComposer.cpp:837`).
- The panel uses the queue's plain text and hub-projected enabled state and label, in the shared reading column. Its tooltip combines the native hint with the first configured registry chord, in Qt's order (`ActivityView.cpp:1477-1486,1890-1896`); this corrects the brief's hint-only tooltip.
- An open under way hides the old session's queue. An explicit Send now in that gap sends nothing, shows "Nothing queued can be sent now" as info, and schedules no action for later. Push is also disabled during that gap; a required card disables and unregisters it.
- Push always calls `send_push` with the handle, revision, epoch, text and cursor captured at the chord, including an empty or sending field. Rust decides under that handle whether it becomes Send now (`MessageComposer.cpp:837` carries the field's own key). This corrects the brief's direct `queue_send_now(null)` call for a sending field; only the panel and window shortcut call `queue_send_now` directly.
- Push and ordinary Send share the flush, per-handle call lock and reply handling. Busy/Blocked from push waits for Rust's bumped text epoch before unlocking. After the flush, a changed handle or an open under way cancels dispatch. A hand-back crossing a push's flush never substitutes a different epoch or message for the captured payload.
- The bridge adds only `sendPush` and `queueSendNow`; `ComposerView.queue` mirrors A2-4's `QueueView`. The app resolves the new bridge methods at call time. Native queued prompts already received "You (queued)" before answer labels in A2-3; the regression also pins its receipt changing to "Sent" after a matching submit.

**Depends on:** A2-4, Task 6 (the composer store and view).

**Qt reference:**
- `tray/src/ActivityView.cpp`: the panel 1280-1292, its state 1890-1895, the tooltip 1477-1487, "You (queued)" 1030.
- `tray/src/MessageComposer.cpp:399-406, 833-847` (the push chord) and `tray/src/Shortcuts.cpp:39-41, 61, 70` (the chords).
- `tray/src/SessionsWindowShortcuts.cpp:242` (`queue.sendNow`).

**Files:**
- Modify: `client/src/types.ts` (`QueueView`, `ComposerView.queue`), `client/src/bridge.ts` (`queueSendNow`, `sendPush`)
- Modify: `client/src/store/composer.ts`, `client/src/store/composer.test.ts` (`push`, `sendNow`)
- Create: `client/src/ui/sessions/chords.ts`, `client/src/ui/sessions/chords.test.ts`
- Create: `client/src/ui/sessions/QueuePanel.tsx`
- Modify: `client/src/ui/sessions/Composer.tsx` (the push chord), `client/src/ui/sessions/SessionDetail.tsx` (the panel and the window chord)
- Modify: `client/src/ui/sessions/transcript.ts`, `client/src/ui/sessions/transcript.test.ts` ("You (queued)")
- Modify: `client/src/ui/styles/detail.css`

**Interfaces:**
- Consumes: A2-4's commands and `QueueView`.
- Produces:

```ts
// types.ts
export interface QueueView { id: string; text: string; hint: string; can_send_now: boolean; enabled: boolean; label: string }
// ComposerView gains: queue: QueueView | null
// ComposerPort gains: push(handle, rev, epoch, text, position, anchor): Promise<SendReply>; sendNow(queueId: string | null): Promise<boolean>
// ComposerStore gains: push(), sendNow(queueId?)
// chords.ts
export const MAC: boolean;
export function createPushChord(mac: boolean): { feed(e: ChordKey, hasSelection: boolean): "push" | "consumed" | "pass" };
export function isSendNowChord(e: ChordKey, mac: boolean): boolean;
```

- [ ] **Step 1: Write the failing tests.** `client/src/ui/sessions/chords.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createPushChord, isSendNowChord, type ChordKey } from "./chords";

const key = (k: string, over: Partial<ChordKey> = {}): ChordKey => ({ key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...over });

describe("composer chords", () => {
  it("Control-X then Control-S pushes on macOS, whatever is selected", () => {
    const chord = createPushChord(true);
    expect(chord.feed(key("x", { ctrlKey: true }), true)).toBe("consumed");
    expect(chord.feed(key("Control", { ctrlKey: true, repeat: true }), true)).toBe("pass");
    expect(chord.feed(key("s", { ctrlKey: true }), true)).toBe("push");
  });

  it("elsewhere Ctrl+X with a selection cuts, and anything else disarms", () => {
    const chord = createPushChord(false);
    expect(chord.feed(key("x", { ctrlKey: true }), true)).toBe("pass");
    expect(chord.feed(key("x", { ctrlKey: true }), false)).toBe("consumed");
    expect(chord.feed(key("a"), false)).toBe("pass");
    expect(chord.feed(key("s", { ctrlKey: true }), false)).toBe("pass");
  });

  it("Send now is Command or Ctrl with Shift and Return", () => {
    expect(isSendNowChord(key("Enter", { metaKey: true, shiftKey: true }), true)).toBe(true);
    expect(isSendNowChord(key("Enter", { ctrlKey: true, shiftKey: true }), true)).toBe(false);
    expect(isSendNowChord(key("Enter", { ctrlKey: true, shiftKey: true }), false)).toBe(true);
    expect(isSendNowChord(key("Enter", { ctrlKey: true }), false)).toBe(false);
  });
});
```

In `client/src/store/composer.test.ts`:
- Task 6's `view()` fixture gains `queue: null`.
- Its `fakes()` port gains:
  - `push: async (_h, rev, _epoch, text, position, anchor) => { calls.push(`push ${rev} ${text} ${position} ${anchor}`); return reply; }`;
  - `sendNow: async (id) => { calls.push(`send-now ${id}`); return true; }`.

```ts
  it("push sends the field with a push, and a sending field sends the queue now", async () => {
    const f = fakes();
    await createRoot(async () => {
      const c = createComposer(f.port, f.timers);
      c.apply(view());
      c.input("also lint", 9, 9);
      await c.push();
      expect(f.calls).toEqual(["edit 1 also lint", "push 1 also lint 9 9"]);
      c.apply(view({ sending: true }));
      await c.push();
      expect(f.calls.at(-1)).toBe("send-now null");
    });
  });
```

In `transcript.test.ts`:

```ts
  it("a queued prompt reads You (queued)", () => {
    const items = buildTranscript([{ seq: 1, type: "UserPromptQueued", at: 10, detail: "also lint" }] as ActivityEvent[]);
    expect([items[0].label, items[0].receipt]).toEqual(["You (queued)", "Queued at this time"]);
  });
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && npm test -- src/ui/sessions/chords.test.ts src/store/composer.test.ts src/ui/sessions/transcript.test.ts`
Expected: FAIL.

- [ ] **Step 3: Write `client/src/ui/sessions/chords.ts`:**

```ts
/** The keys a chord reads from a KeyboardEvent. */
export interface ChordKey { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; repeat?: boolean }

/** macOS: Qt's "Ctrl" is Command and its "Meta" the Control key. */
export const MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform || navigator.userAgent);

const MODIFIERS = new Set(["Control", "Meta", "Alt", "Shift"]);

/**
 * Send and push now (Qt's message.push): Control-X, Control-S on macOS
 * (Meta+X, Meta+S) and Ctrl+X, Ctrl+S elsewhere. Elsewhere Ctrl+X with a
 * selection is the editor's Cut and arms nothing.
 */
export function createPushChord(mac: boolean) {
  let armed = false;
  const control = (e: ChordKey) => e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
  return {
    feed(e: ChordKey, hasSelection: boolean): "push" | "consumed" | "pass" {
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      if (armed) {
        if (MODIFIERS.has(e.key)) return "pass";
        armed = false;
        return control(e) && k === "s" ? "push" : "pass";
      }
      if (control(e) && k === "x" && (mac || !hasSelection)) { armed = true; return "consumed"; }
      return "pass";
    },
  };
}

/** Send queued messages now (Qt's queue.sendNow): Command+Shift+Return on macOS, Ctrl+Shift+Return elsewhere. */
export function isSendNowChord(e: ChordKey, mac: boolean): boolean {
  return e.key === "Enter" && e.shiftKey && !e.altKey && (mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey);
}
```

- [ ] **Step 4: The store and the bridge.**

```ts
// bridge.ts
export const queueSendNow = (queueId: string | null) => desktop<boolean>("queue_send_now", { queue_id: queueId });
export const sendPush = (handle: number, rev: number, epoch: number, text: string, position: number, anchor: number) =>
  desktop<SendReply>("send_push", { handle, rev, epoch, text, position, anchor });
```

In `store/composer.ts`, `ComposerPort` gains `push` and `sendNow`; the app's port maps them to `bridge.sendPush` and `bridge.queueSendNow`. The store gains:

```ts
    /** Send and push now: a message on its way makes it Send now (MessageComposer::push). */
    async push() {
      const h = handle();
      if (h === undefined) return;
      if (view()?.sending) { await port.sendNow(null); return; }
      await flush();
      reply(await port.push(h, rev, epoch, text(), selection().end, selection().start));
    },
    async sendNow(queueId?: string) { await port.sendNow(queueId ?? null); },
```

- [ ] **Step 5: The panel.** Write `client/src/ui/sessions/QueuePanel.tsx`:

```tsx
import { Show } from "solid-js";
import { composer } from "../../store/composer";

/** "Queued in agent" (ActivityView.cpp:1280-1292): the agent's own queue, read-only, with its Send now. */
export function QueuePanel() {
  return (
    <Show when={composer.view()?.queue}>{(q) => (
      <div class="act-queue" role="region" aria-label="Queued in agent">
        <div class="act-queue-head">
          <b>Queued in agent</b><span class="grow" />
          <button class="btn small" disabled={!q().enabled} title={q().hint} onClick={() => void composer.sendNow(q().id)}>{q().label}</button>
        </div>
        <div class="act-queue-text wrap-pre" title={q().text}>{q().text}</div>
      </div>
    )}</Show>
  );
}
```

```css
.act-queue { margin: 0 0 8px; padding: 8px 12px; background: var(--z-color-surface); border: 1px solid var(--z-color-border); border-radius: var(--z-radius-md); }
.act-queue-head { display: flex; align-items: center; gap: 8px; }
.act-queue-head b { color: var(--z-color-muted); font-weight: 600; }
.act-queue-text { max-height: 100px; overflow-y: auto; user-select: text; }
```

- [ ] **Step 6: Place it and bind the chords.**
  - In `SessionDetail.tsx`, `<QueuePanel />` goes between `<ActivityView />` and the composer column, inside `activity-wrap`, so it shares the Activity column.
  - The detail view listens for the window chord while the Sessions page shows a session:

```tsx
onMount(() => {
  const onKey = (e: KeyboardEvent) => {
    if (!isSendNowChord(e, MAC) || !selected()) return;
    e.preventDefault();
    void composer.sendNow();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));
});
```

  - In `Composer.tsx`, the editor's `onKey` feeds the push chord first:

```tsx
  const chord = createPushChord(MAC);
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing) return;
    const step = chord.feed(e, editor.selectionStart !== editor.selectionEnd);
    if (step !== "pass") { e.preventDefault(); if (step === "push") void composer.push(); return; }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void composer.send(); }
    else if (e.key === "Escape" && act().stop.shown && !act().stop.disabled) { e.preventDefault(); void composer.interrupted().then(() => act().stop.action()); }
  };
```

  No button and no tooltip names either chord (owner rule): Qt has no push button, and the Send now tooltip is the queue's hint alone.
  - In `transcript.ts`, a `UserPromptQueued` item is labelled "You (queued)". This rule is checked before A2-3's "You (answer)", in Qt's order (`ActivityView.cpp:1030-1031`). The receipt rule stays: "Queued at this time" until a matching submit follows.

- [ ] **Step 7: Run the checks**

Run: `cd client && npm test -- src/ui/sessions/chords.test.ts src/store/composer.test.ts src/ui/sessions/transcript.test.ts && npm run check && npm test && python3 ../scripts/check-ui-separators.py`
Expected: PASS; no type errors; no separators.

- [ ] **Step 8: Commit**

```bash
git add client/src
git commit -m "feat(client): the Queued in agent panel, Send now, and the Send and push now chord"
```

---

### Task A2-6: OpenCode's inbox: Queued, Not delivered and Remove

**Bar:** Qt's only Remove of a queued message. The owner's daily agents are Claude, Codex and Kimi, so this task is planned here but lands after the switch unless the owner rules otherwise (open question in the report). It is independent of A2-1 to A2-3.

**Depends on:**
- R8 (`InboxFacts`, `inbox_removable`, the `REMOVE_*` texts, `Action::RemoveQueued`, `actions::allowed_with`, `Lane::Inbox`);
- A2-4;
- Task 4b (`Followed.events`) and Task 7 (`transcript.ts` local rows).

**Qt reference:**
- `tray/src/ActivityView.cpp:1034-1038, 1069-1087`;
- `tray/src/SessionsWindow.cpp`: 1011-1019, 3978-3979, 4433-4467, 4471.

**Files:**
- Modify: `client/src-tauri/src/composer/queue.rs` (`InboxView`, `inbox_part`, `queue_remove`, the result; tests)
- Modify: `client/src-tauri/src/composer/mod.rs` (`ComposerView.inbox`), `client/src-tauri/src/lib.rs` (`queue_remove`)
- Modify: `client/src/types.ts`, `client/src/bridge.ts` (`queueRemove`)
- Modify: `client/src/ui/sessions/types.ts` (`TranscriptItem.inbox`), `client/src/ui/sessions/transcript.ts`, `client/src/ui/sessions/transcript.test.ts`
- Modify: `client/src/ui/sessions/Activity.tsx` (the note, Remove and its dialog)
- Modify: `client/src/store/composer.test.ts` (the `view()` fixture gains `inbox: { shown: false, ready: false, busy: false, removing: null, run: "", conversation: "" }`)

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InboxView { pub shown: bool, pub ready: bool, pub busy: bool, pub removing: Option<String>, pub run: String, pub conversation: String }
impl Composers { pub fn queue_remove(&self, message_id: &str, run: &str, conversation: &str); }
// ComposerView gains: pub inbox: InboxView
// command: queue_remove(message_id, run, conversation)
```

```ts
export interface InboxView { shown: boolean; ready: boolean; busy: boolean; removing: string | null; run: string; conversation: string }
// TranscriptItem gains: inbox?: { state: "queued" | "not_delivered"; messageId?: string }
```

- [ ] **Step 1: Write the failing tests.** In `composer/queue.rs`'s tests:

```rust
    fn opencode(delivery: &str) -> (Value, Vec<Value>) {
        let mut details = ready();
        details["allowed_actions"] = json!(["remove_queued", "forget"]);
        let event = json!({"seq": 4, "type": "UserPromptSubmit", "detail": "later", "message_id": "msg_abc123", "delivery_state": delivery});
        (details, vec![event])
    }

    fn events(details: Value, events: Vec<Value>) -> zerus_client_core::Batch {
        let mut batch = state(details);
        batch.activity.push(zerus_client_core::ActivityOut {
            computer: zerus_client_core::ComputerId::from("studio"), key: "run:run-1".into(), kind: "events".into(),
            payload: json!({"events": events, "cursor": 4}),
        });
        batch
    }

    #[test]
    fn a_not_delivered_message_is_removed_after_the_check() {
        let root = dir("inbox");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).agent = "opencode".into();
        *lock(&fake.outcome) = Box::new(|_, id| Outcome::Done { receipt: json!({"status": "completed", "request_id": id}), cleanup: None });
        let (details, list) = opencode("not_delivered");
        c.open(Some(crate::composer::tests::session()));
        c.observe(&events(details, list));
        let view = until(&c, |v| v.inbox.ready);
        assert!(view.inbox.shown);
        c.queue_remove("msg_abc123", "run-1", "conv-1");
        until(&c, |v| !v.inbox.busy && v.inbox.removing.is_none());
        let Some((_, Action::RemoveQueued { message_id }, _)) = sent(&fake).last().cloned() else { panic!("no removal") };
        assert_eq!(message_id, "msg_abc123");
        assert_eq!(c.take_events(), vec![notice("Queued message removed.")]);
    }

    #[test]
    fn a_message_that_changed_is_not_removed() {
        let root = dir("inbox-changed");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).agent = "opencode".into();
        let (details, list) = opencode("queued");
        c.open(Some(crate::composer::tests::session()));
        c.observe(&events(details, list));
        until(&c, |v| v.inbox.ready);
        c.queue_remove("msg_abc123", "run-1", "conv-1");
        assert_eq!(c.take_events(), vec![ComposerEvent::Notice { text: "The queued message changed. Check Activity before removing it.".into(), error: true, transient: false }]);
        assert!(sent(&fake).is_empty());
    }
```

In `transcript.test.ts`:

```ts
  it("OpenCode's inbox states replace the receipt", () => {
    const items = buildTranscript([
      { seq: 1, type: "UserPromptSubmit", at: 10, detail: "a", delivery_state: "queued", message_id: "msg_a" },
      { seq: 2, type: "UserPromptSubmit", at: 11, detail: "b", delivery_state: "not_delivered", message_id: "msg_b" },
    ] as ActivityEvent[]);
    expect(items.map((i) => i.receipt)).toEqual(["Queued", "Not delivered"]);
    expect(items[1].inbox).toEqual({ state: "not_delivered", messageId: "msg_b" });
  });
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer::queue:: && npm test -- src/ui/sessions/transcript.test.ts`
Expected: FAIL.

- [ ] **Step 3: The hub part.** `Queue` gains `removing: Option<(String, String, String)>` (composer key, message id, session name). In `queue.rs`:

```rust
use zerus_client_core::actions::allowed_with;
use zerus_client_core::input_queue::InboxFacts;

/// What Activity needs for OpenCode's Remove (`ActivityView.cpp:1077-1087`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InboxView { pub shown: bool, pub ready: bool, pub busy: bool, pub removing: Option<String>, pub run: String, pub conversation: String }

impl Composers {
    pub(crate) fn inbox_part(&self, hub: &Hub) -> InboxView {
        let Some(ctx) = self.context(hub) else { return InboxView::default() };
        let details = ctx.details.as_deref().cloned().unwrap_or_default();
        let shown = details["allowed_actions"].as_array().is_some_and(|list| list.iter().any(|a| a == "remove_queued"));
        let busy = hub.queue.removing.is_some();
        InboxView {
            shown,
            ready: ctx.facts.cmd == "opencode" && ctx.online && !ctx.facts.inspect_error && !busy
                && !text(&details, "run_id").is_empty() && allowed_with(&ctx.row, &details, "remove_queued"),
            busy,
            removing: hub.queue.removing.as_ref().filter(|(key, _, _)| *key == ctx.key).map(|(_, id, _)| id.clone()),
            run: text(&details, "run_id").to_owned(),
            conversation: text(&details, "conversation_id").to_owned(),
        }
    }

    /// Remove, after the confirmation, with the identity the dialog showed
    /// (`removeQueuedMessage` 4446-4467): sent only while the same
    /// conversation still reports that message as not delivered.
    pub fn queue_remove(&self, message_id: &str, run: &str, conversation: &str) {
        let mut hub = self.lock();
        let Some(ctx) = self.context(&hub) else { return };
        let details = ctx.details.as_deref().cloned().unwrap_or_default();
        let events: Vec<Value> = hub.followed.events.iter().cloned().collect();
        let facts = InboxFacts {
            cmd: &ctx.facts.cmd, online: ctx.online, inspect_error: ctx.facts.inspect_error, busy: hub.queue.removing.is_some(),
            allowed: allowed_with(&ctx.row, &details, "remove_queued"), details: &details, events: &events,
        };
        if hub.closed || text(&details, "run_id") != run || text(&details, "conversation_id") != conversation
            || !rules::inbox_removable(&facts, message_id) {
            self.event(&mut hub, ComposerEvent::Notice { text: rules::REMOVE_CHANGED.into(), error: true, transient: false });
            drop(hub);
            self.changed();
            return;
        }
        hub.queue.removing = Some((ctx.key.clone(), message_id.to_owned(), ctx.session.name.clone()));
        drop(hub);
        self.changed();
        let target = Target { computer: ctx.session.computer.clone(), name: ctx.session.name.clone(), run_id: run.to_owned(), conversation_id: conversation.to_owned() };
        let (me, key, name, id) = (self.clone(), ctx.key.clone(), ctx.session.name.clone(), message_id.to_owned());
        (self.0.deps.spawn)(Box::new(move || {
            let outcome = (me.0.deps.dispatch)(target, Action::RemoveQueued { message_id: id }, uuid::Uuid::new_v4().to_string())
                .unwrap_or_else(|error| Outcome::Uncertain { detail: error });
            me.queue_removed(&key, &name, &outcome);
        }));
    }

    /// 1013-1019: the selected session hears the result; another hears only a failure.
    fn queue_removed(&self, key: &str, name: &str, outcome: &Outcome) {
        let mut hub = self.lock();
        hub.queue.removing = None;
        let current = hub.open_key.as_deref() == Some(key);
        let failure = match outcome {
            Outcome::Done { .. } => None,
            Outcome::Failed { detail } | Outcome::Uncertain { detail } => Some(detail.clone()),
        };
        match (current, failure) {
            (true, None) => self.event(&mut hub, ComposerEvent::Notice { text: rules::REMOVED.into(), error: false, transient: false }),
            (true, Some(detail)) => self.event(&mut hub, ComposerEvent::Notice { text: detail, error: true, transient: false }),
            (false, Some(detail)) => self.event(&mut hub, ComposerEvent::Notice {
                text: format!("Could not remove the queued message from {name}: {detail}"), error: true, transient: false }),
            (false, None) => {}
        }
        drop(hub);
        self.changed();
        if current {
            self.refresh_details();
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn queue_remove(state: tauri::State<'_, Composers>, message_id: String, run: String, conversation: String) {
    state.queue_remove(&message_id, &run, &conversation);
}
```

`ComposerView` gains `pub inbox: InboxView`, filled by `self.inbox_part(hub)`. Register `composer::queue::queue_remove` in `lib.rs`.

- [ ] **Step 4: The transcript and Activity.**
  - In `transcript.ts`, a user event with `delivery_state` `queued` or `not_delivered`:
    - takes the receipt "Queued" or "Not delivered" in place of "Sent" / "Queued at this time";
    - carries `inbox: { state, messageId: e.message_id }`.
  - In `Activity.tsx`, an item with `inbox.state === "not_delivered"` shows the note "Still waiting in OpenCode's queue. It runs first with your next message unless you remove it.". With `composer.view()?.inbox.shown` and a `messageId`, it also shows a button:
    - "Removing…" (disabled) while `inbox.removing` is this id;
    - "Remove" (disabled) while `inbox.busy`;
    - "Remove", enabled when `inbox.ready`.

    Remove opens a confirmation dialog (`<dialog class="dlg" role="alertdialog">`, as A2-3's cold dialog):
    - the title "Remove queued message";
    - the text "Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it.";
    - Cancel first (default, focused, Escape);
    - a danger button "Remove" (`btn solid-danger`), which calls `bridge.queueRemove(messageId, inbox.run, inbox.conversation)` with the identity captured when the dialog opened.

    ```ts
    // bridge.ts
    export const queueRemove = (messageId: string, run: string, conversation: string) => desktop<void>("queue_remove", { message_id: messageId, run, conversation });
    ```

- [ ] **Step 5: Run the checks**

Run: `cd client && cargo +1.99.1 test --locked --manifest-path src-tauri/Cargo.toml composer:: && npm test && npm run check && python3 ../scripts/check-ui-separators.py`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/src client/src-tauri
git commit -m "feat(client): OpenCode's queued and not delivered messages, with a confirmed Remove"
```

---

# Part A, continued

Tasks 8-12 complete Part A. Task 12 runs the smoke for Part A and Part A2.

### Task 8: Dropped files: composer attach, Terminal paste and stage

**As built (Rust backend, a2hub; webui rulings, 2026-10-29).** The frontend is a separate webui change. Where the snippets below differ, these rules and the current source win:
- `Terminals::paste(view, generation, text)` checks the named generation and queues the paste under one views lock. A missing view or a mismatch returns "Terminal changed during file transfer. Drop the files again in the intended session.". The command IPC arguments and `Ok`/`Err` meanings are unchanged.
- `terminal_drop` is async and runs its work on the blocking pool, using `actions::dispatch` once for a peer. It refuses a webview or a view other than `main`. The live handler registers it; the non-live handler lists no Terminal commands and needs no stub.
- Review follow-up: `terminal_shown(view, generation, shown)` records the tab visibility for that attach; stale updates are ignored and a new attach starts shown. Paste checks visibility with its generation under the same views lock, so a transfer finishing while the tab is hidden refuses, as Qt's `terminalDropTarget` does (2620). The frontend calls it on tab hide/show.
- A taken write lane (`dispatch` returning `Err(actions::BUSY)`) is a definite pre-dispatch refusal and returns `Ok(Some("Wait for the session update to finish."))`, an info notice (Qt 4395). No native failed or uncertain outcome is converted into that refusal.
- A drop guard releases the window-wide `dropping` claim after success, refusal, failure, cancellation or a dispatch panic. A pending peer transfer blocks local drops too, before the local paste, as Qt does (`SessionsWindow.cpp:2650-2651`).
- Receipt paths accept a leading `/` on every platform. Windows builds also accept a fully rooted alphabetic drive path or UNC path; drive-relative paths and control characters refuse. The pure receipt and platform-policy tests live under `cfg(test)` separately from the Unix fake-attach tests, so the parameterized policy test also executes on Windows; actual Windows execution remains in `zerus-5wmh.52`.
- Windows quoting continues to consume core `terminal::paste_paths` unchanged. Its pre-existing difference from Qt's `SessionCommands::powerShellQuote` (smart apostrophes and the safe bare-word whitelist, `SessionCommands.h:40-51`) is core follow-up `zerus-5wmh.61`, off the Mac switch bar; actual Windows coverage remains in `zerus-5wmh.52`.
- The fake attach must print its hello before the paste test writes input: switching the fake PTY to raw mode otherwise flushes that input. Tests also cover remote identities, one dispatch, uncertain outcomes without replay, reconnects, malformed receipts, concurrent drops and release during cancellation or unwinding.

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

**As built** (branch `work-ba0ed9fd-attachments`, the surface webui approved on 2026-10-29). Where the steps below differ, the code and this list win (Qt wins over the brief; `AttachmentViewer.h`, `ActivityView.cpp`, `SessionsWindow.cpp` and `MessageComposer.cpp` lines):
- **R2 is in.** `external_file` (#424) is on the base, so Open is wired, never disabled with "Arrives with the downloaded-file marker.".
- **The Fetcher** lives in `composer/fetch.rs` and is Qt's `AttachmentLoader` (162-243). `fetch(computer, file, priority)` runs one read per file key (computer, session, conversation, archive, agent, request, index), at most three reads at once (`pump`), and moves a priority read ahead of the waiting ones. It remembers the last 256 files read (`m_paths`) and answers them from their copies while `cached_at` accepts them. A file without `request_id` answers Qt's "This attachment has no saved file reference." without the CLI.
- **Activity Open is one fetch, then open** (`AttachmentLoader::open`, 196-211): `attachment_open(computer, file: AttachmentRef)` takes a priority read, then `external_file::open` as `Origin::OtherMachine`. It answers `Ok(None)` when opened, `Ok(revealed_notice)`, or `Err` with `reveal_failed_notice`, Qt's "No application could open this file." or the read's error. No command opens a path the webview names. The page shows "Opening attachment…" first (`SessionsWindow.cpp:1096`); an error replaces it as an error notice and a reveal as a plain one.
- **Draft Open** is `attachment_open_draft(path)`, where `path` is the view's `draft/{handle}/{list}/{index}`, resolved as the `zerus-file` scheme resolves it, so a changed list opens nothing. It prepares the one attachment into the cache and opens it as `Origin::ThisComputer` (`MessageComposer.cpp:1067-1085`). It fails with the prepare's error, "Could not open the attachment." or "Could not open the attachment with its default application.". The composer's attachment row opens its file on a click, Enter or Space, titled "Open {name}" (Qt's eventFilter, 1104-1117), closing the list first; an outcome other than a plain open shows in the composer's status line (Qt's showError), only while that composer is still the open one.
- **Thumbnails, not previews.** Qt's Activity has no hover preview: an image other than SVG gets an inline thumbnail in a fixed 96x64 cell (`ActivityView.cpp:771-777`), read at render at normal priority (2126-2131). It is pinned while mounted; a failed load reads once more, then the card shows none, as Qt shows none for a failed read. There is no text preview, no `attachment_head` and no CORS change on `zerus-file`.
- **The card** (759-782) reads `{reference }{name}`, `{n} KiB` or "File", and "Open →"; a file without `request_id` reads "Attachment: {name}" and opens nothing. Each Activity source gives its `attachmentScope()`: the live follow's conversation is its inspection's, else the row's; the archived row's is the one its last read named, with its archive ID. The main Activity's messages are the session's own (`agent_id` null).
- **The cache** keeps `limit`, counted pins by canonical path (at most 1024; `clear_pins` in `start`) and the composers' `retry_files` as its set extra. `attachment_pin` pins only a copy the cache holds.
- **The opener** is `tauri-plugin-opener`, used from Rust only: no capability gives the webview its commands.
- **The session-file dialog** (`SessionsWindow.cpp:709-877`: Download…, Download and open, Open local copy, Show file, through `HgsClient::downloadFileToCache` and the DownloadCache) is not in Task 9. Nothing in the client opens it yet: it needs Activity's Markdown links, and file references stay after the switch (ruling of 2026-10-28). It is the follow-up `zerus-5wmh.60`, whose description holds its surface: R9 and `session_files.rs` for the core owner, the dialog and the file-link activation for the UI. The composer's "Arrives with file references." control keeps its place.

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
  - In `Activity.tsx`, an attachment chip with a reference shows, on hover or after an arrow-key move (never on focus alone, round 6), a preview of at most 320 px beside it. It is an `<img>` with `onError={() => previews.failed(ref)}` for an image, or the first 2 KiB of a text file.
  - **The text preview's bytes (Task 3b review).** The app's origin (`tauri://localhost`, or `http://tauri.localhost` on Windows) is cross-origin to `zerus-file`. Task 3b's responses carry no `Access-Control-Allow-Origin`, and wry 0.57.1 registers custom schemes on Linux as secure only, never as CORS-enabled.
    - First choice: read the first 2 KiB as a raw IPC command body, `attachment_head(path) -> tauri::ipc::Response`. It resolves the path through `Cache::copy` with the scheme's rules and is refused unless the calling webview is main. This needs no CORS, and `zerus-file:` then leaves `connect-src`.
    - If Task 9 keeps `fetch`:
      - the scheme sends `Access-Control-Allow-Origin` only when the request's `Origin` exactly matches the app origin (`tauri://localhost` or `http://tauri.localhost`, and the dev server origin in dev builds only), together with `Vary: Origin`;
      - never `*`, and never credentials;
      - the `fetch` sends no `Range` header, since its preflight (OPTIONS) gets 405, and reads the first 2 KiB from the body.
    - Either way, prove it in a headless WebKitGTK run and on WebView2 before the task closes.
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

**As built** (`bb1065f6` and its review fix round 1, `10aa079c`). Where the steps below differ, the code and this list win (Qt wins over the brief; `tray/src/ProcessView.cpp` lines as of this base: `setSession` 108-144, `select` 145-157, `updateActions` 161-177, `updateBusy` 178-182, `requestOutput` 183-192, `stopSelected` and `stopNext` 193-215, the replies 78-97, `refreshDurations` 219-226). Task 11 binds to this list:
- **Commands:** `processes_follow(target)`, `processes_shown(shown)`, `processes_select(key, ids, current)` and `processes_stop(key, ids)`.
  - `key` is the `ProcessesView.key` the click saw, and `processes_stop`'s `ids` its `selected`. A selection changes nothing unless `key` is still the hub's identity. A stop goes out only when `key` and `ids` are both still the view's (Qt's `key==m_key && ids==selectedIds()`, 74), so a late click never stops a process the view selected by default meanwhile. A refused call sends nothing and shows nothing; Task 11 sends the latest view's values.
  - There is no `processes_settings` (the coordinator's ruling 1): the hub reads the five `processes/*` keys from the app's settings store at start and after every change of one (`ProcessHub::settings_from`, the store's listener in `lib.rs`). Task 11 writes them with `settings_set` and has no `bridge.processesSettings`.
  - Task 11 sends `processes_follow` again on every change of the selected row, even while `failed` is set: a follow before the data source starts answers "not started" (as Activity's `follow`), and a refused or ended follow is tried again only by a later `processes_follow`.
- **`ProcessTarget`** gains `generation: Option<String>` (`#[serde(default)]`), part of the identity. It is the session's `host_generation` from its Activity details: the topic's value is `{processes, process_work}` without it (`src/state/watch_source.rs:48`, `src/watch/processes.rs:243`), and a native job's output and stop need it (`src/state/processes.rs:458`). Task 11 passes `host_generation` of the selected session's details.
- **The details line** names Qt's host (151): "This machine" for this computer, else `machine` (`computer` when empty). Task 11 passes the row's `machine`.
- **`ProcessDeps`** gains `epoch` (seconds since the epoch, for the duration column); `clock_ms` is monotonic (an `Instant` in `lib.rs`). The closure types are aliases (`ClockMs`, `Epoch`, `Follow`, `Unfollow`, `ReadOutput`, `Stop`).
- **The views.** `RowView` and `ProcessesView` do not derive `Default` (`Tone` and `StopControl` have none). `ProcessesView.key` is Qt's view key (host, name, archive, run, conversation and generation, one per line; empty without a session): Task 11 resets its scroll when it changes. `DetailView.command` is the whole command, for Copy command; `output` is null until a read answered for this selection.
- **Rows keep Qt's tree order:** a row stays where it first appeared, a new one goes to the end of its group, and one whose group changed moves to the end of its new group. A removed current row passes the current to its nearest remaining sibling in its group (above first), else to none. The default selection (the first live, else recorded, else finished row, selected alone) applies only to a new identity's first inventory, or when the last inventory was empty (131). Task 11's Shift range uses this order.
- **View memory** (`saveView`, 107): the selection, the current process and the output of the last 30 identities, restored when one is shown again; the restored output shows until the new read answers (142). A session left before its inventory came keeps its saved view.
- **A new identity** drops the read in flight, the unsent stops, the stop in flight (its late answer changes nothing), the stop notice, the busy indicator and the selection (113). Online and live are not part of it; a session that stops being live or online ends the queue with "Machine or session became unavailable; remaining requests were cancelled.". The topic follow changes only with the computer, key, name, run or conversation, and a new key also drops the inventory and `failed`.
- **A new identity's first stop waits for the old stop's lane.** `actions` runs one process stop per computer and session name (`Lane::Process`), and an identity change can keep the name. While a stop of an earlier identity is still under way on that lane, the new queue's stop is held, and it goes out when that stop answers, so nothing answers busy.
- **Archived sessions** (`archive:ID`, or an `archive_id`) are never followed: the topic has no archived sessions. Their view is empty, without `failed`, and `notes` (and `summary_tooltip`) say "Recorded processes of archived sessions arrive with the archive inventory read." until the core's archive inventory read (zerus-5wmh.63) lands.
- **`observe`** applies only the followed key's entries. `ended` drops the inventory and the follow; a refused follow sets `failed`. Either way the next `processes_follow`, even of the same target, follows again.
- **Output reads** start at once for a new current process and when the tab shows (Qt's forced reads), otherwise on the tick, one at a time. A new current process drops the read in flight. A reply for another run or conversation shows "Session changed. Refreshing processes…" and keeps the inventory, as Qt keeps its list (93); the topic sends a new state when the session changes, so the brief's "`refresh` clears `value`" is not done.
- **`busy`** starts with the first read or stop while the tab shows (or at showing), shows after `loading_ms`, and ends when none runs or the tab hides.
- **Stops** go through `actions::dispatch_as` with a new request id each. `Failed`, `Uncertain`, `Err` and a panic become `stop_result(Err(detail))` (an empty detail is "Stop was not confirmed"). In a replay without `ZERUS_CLIENT_HGS`, Stop selected and the output read answer "Actions in replay mode need ZERUS_CLIENT_HGS." as `stop_notice` and as the detail's notice.
- **`exit_work`** calls `ProcessHub::close()`: no read or stop starts after it, and unsent stops are dropped. The ticker sleeps while the tab shows no session and ends there.
- **`background_seconds`** is read but unused: the topic keeps its own cadence (`src/watch/processes.rs`: a read at least every 10 s, at most every 2 s), so Qt's inspect intervals (`SessionsWindow.cpp:2015, 3473`) have no counterpart here. Qt re-reads the inventory every `activeSeconds` while the tab shows, so a finished process can stay in Live or Recorded up to about 10 s longer here. Core request zerus-5wmh.62 (an interval per subscription and a refresh call) closes that gap; the hub then sets `activeSeconds` while the tab shows and `background_seconds` otherwise.
- **The envelope** carries `processes_view` only when it changed; `start` sends it again to a reloaded webview.
- **Replay and fixture.** The replay holds `processes` state frames per key and serves one on `follow_processes`. Every hello lists `processes` with `processes_subscriptions: 16`; this computer's stream holds one state for claude/sample-project/api-refactor (`run:000001-run`), `{processes, process_work}` without a `run_id` as the topic sends it: `tree-cargo-test` (`process_tree`, running, stoppable, no captured output, as real OS trees), `call-npm-build` (unconfirmed, last `running`) and `call-git-fetch` (finished, exit 0); the last two have output to read. `replay-batches.json` is unchanged: the browser replay has no Rust hub.

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

**Depends on:** Tasks 1-11 and A2-1 to A2-5 as merged (A2-6 too when it lands first).

**Files:**
- Create: `client/src-tauri/tests/support/fake_hgs_phase4.py`
- Modify: `client/scripts/make-fixture.py`, then regenerate `client/fixtures/*`:
  - a warm prompt cache on `claude/sample-project/api-refactor`'s Activity state (`session_usage.prompt_cache: {status: warm, expires_at: BASE_AT + 1800}`, where S2 reads it);
  - a cold `cache_hint` on `codex/sample-project/pagination`;
  - a sent message with one attachment (`attachments: [{name: "orders.json", request_id: "req-fixture-1", index: 0, mime: "application/json", size: 812}]`) on api-refactor;
  - a paused `claude/sample-project/review` (already in the fixture) with `resumable: true`;
  - (A2) `claude/sample-project/docs-pass`'s Activity state with `phase: input`, `activity: busy` and one AskUserQuestion card:
    - `question_id: "tool:fixture-1"`, a 64-character `question_hash`, the row's run and conversation;
    - `source: hook`, `answer_transport: claude_hook`, `can_answer: true`;
    - one question "Which checks should run?" with the options "Unit" and "UI" and `allow_other: true`.
  - (A2) `kimi/sample-project/flaky-test`'s state with `phase: approval` and an approval card:
    - `source: kimi_tool_approval`, `approval: true`, `approval_choices: false`, `can_answer: true`;
    - one question "Run rm -rf build?" with the options `allow` and `deny`.
  - (A2) `codex/sample-project/release-notes`'s state with `activity: busy`, `phase: working` and `input_queue: {id: "queue-fixture", text: "↳ also update the changelog", can_send_now: true, hint: "Interrupt the current turn and send pending messages now"}`.
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
  - (A2) `answer NAME --json`: `{status: "answered", request_id, name, question_id, question_hash: expected_question_hash, run_id: expected_run_id, conversation_id: expected_conversation_id}` from its stdin;
  - (A2) `send-now NAME --json`: `{status: "submitted", request_id, name, run_id: expected_run_id, conversation_id: expected_conversation_id, queue_id}`;
  - (A2) `session-action NAME --json` with `action: remove_queued`: `{status: "completed", request_id, name, run_id: expected_run_id, conversation_id: expected_conversation_id}`;
  - `--help`: the watch-v1 marker line only;
  - anything else exits 2.

- [ ] **Step 2: The smoke hook.** In `PerfHost.tsx`, `?phase4-smoke=1` runs only when `config().actions_smoke` (replay, an explicit `ZERUS_CLIENT_HGS`, `ZERUS_CLIENT_PERF`), as the actions smoke does. It logs `phase4-smoke <step>: <outcome>` per step and exits 0, or 1 on the first failure. The steps are, on the replay:
  1. `terminal`: select `claude/sample-project/api-refactor`, open the Terminal tab, wait for "Connected: …", write `echo smoke\r`, and wait for `echo smoke` in the screen buffer.
  2. `drafts`: type "phase 4 smoke" into the composer, wait 300 ms, select another session and back, and check that the text returned.
  3. `attach`: drop `$ZERUS_CLIENT_SMOKE_FILE` (a synthetic file the run script writes) through `bridge.composerAttach`, and wait for "[File #1]".
  4. `send`: Send, wait for the local row "Submitted to terminal", and check that `log.ndjson` has one `send` call whose stdin names the file and whose argv has no message text (the hook reads the log through a `perf_log`-guarded command `smoke_log`, allowed only in a measurement run).
  5. `cold`: select `codex/sample-project/pagination`, Send, expect the "Continue with a cold cache?" dialog, choose "Send with full context", and expect a second `send` call.
  6. `start`: select the paused `claude/sample-project/review`, Send, expect "Start the session to send?", choose "Resume and send", and expect `resume` in the log, then the waiting text.
  7. `processes`: enable `processes/enabled` through the real settings store in the isolated smoke scratch, open Processes, expect three groups, and wait for the output "running 42 tests".
  8. `preview`: fetch the fixture message's `orders.json` and expect its metadata, then wait for the `pixel.png` card's `convertFileSrc` thumbnail to decode at its natural size.
  9. `question` (A2):
     - select `claude/sample-project/docs-pass`;
     - expect the card "Agent needs your answer" and `input_visible: false` (the editor stays mounted but hidden);
     - choose "Unit" and Submit;
     - expect one `answer` call whose stdin carries `question_id: "tool:fixture-1"`, the fixture's `expected_question_hash`, run and conversation, and `selected_option_ids: ["opt_0_0"]`;
     - expect the status "Answer sent. Waiting for the agent…".
  10. `approval` (A2):
      - select `kimi/sample-project/flaky-test`;
      - expect "Approve once (3)" disabled;
      - call `questions.setActive(true)` (a headless window may not report focus) and wait 3.2 s;
      - click Approve once;
      - expect one `answer` call with `selected_option_ids: ["allow"]`.
  11. `queue` (A2):
      - select `codex/sample-project/release-notes`;
      - expect "Queued in agent" with the fixture text;
      - click Send now;
      - expect one `send-now` call with `queue_id: "queue-fixture"` and the notice "Send now requested. Waiting for the agent to accept the queue.".
  12. `image-paste` (Task 6's WebKitGTK review): paste the one-pixel PNG through the composer store, open its attachment list and wait for the draft's `convertFileSrc` thumbnail to decode.
  13. `clear-close-order` (Task 6's WebKitGTK review): with the fake's `clear-context` reply delayed, confirm Clear session and require the dialog to close while the command is still running, then wait for its one receipt.

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
  - "Questions and the native queue" (A2):
    - the card's rules and its drafts' keys;
    - the answer path and its uncertain outcome;
    - the queue panel and Send now;
    - Send and push now's chord;
    - OpenCode's inbox when A2-6 is in;
  - "Phase 4 smoke run": the commands, as in Step 2.

  Note in "Rules" that Part B adds voice and video, Native UI and the external entries.

- [ ] **Step 4: File the follow-ups.**
  - One Beads child of `zerus-5wmh` per composer part outside this phase (Decisions); each description names its Qt files and states that the control keeps its place disabled with its "Arrives with …" text until then:
    - model settings and the OpenCode picker;
    - file references;
    - suggestions;
    - the subagent composer;
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
- `log.ndjson` shows exactly one `send` per Send, one `answer` per Submit or Approve once, and one `send-now` per Send now, with message and answer text only in stdin.

Keep the raw log with its sha256 and the executed test counts for the handoff.

**As built (2026-10-29):** the smoke driver is `client/src/perf/phase4Smoke.ts`, with `client/scripts/phase4-smoke.py` preparing a fresh scratch and `client/scripts/test_phase4_smoke.py` covering the fixture and fake. Every catalog row and Activity state carries `tracked`, run and conversation identity; the six driven sessions stay out of ordinary replay deltas. Resume publishes a fresh live inspection, and `smoke_resume` advances the replay's catalog and Activity to that run. The runner pins `ZERUS_CLIENT_SMOKE_NOW` only inside the guarded smoke, keeping the planned warm cache valid without changing production time. `smoke_log` reads off the main thread, both smoke commands answer only the main webview, and session selection waits out the transient "Checking the session…" state. The added `pixel.png` and Clear steps cover Task 6's requested WebKitGTK paths.

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
        c.edit(view.handle, 1, view.text_epoch, "before after".into(), 6, 6);
        assert_eq!(c.insert_voice_text(view.handle, 1, "spoken", "op-1"), Inserted::Saved);
        assert_eq!(c.current().unwrap().text, "before spoken after");
        assert_eq!(c.insert_voice_text(view.handle, 1, "spoken", "op-1"), Inserted::Saved, "the same operation is saved again, not inserted twice");
        assert_eq!(c.current().unwrap().text, "before spoken after");
        // The insert changed the text in Rust: the next edit is based on its epoch.
        let view = c.current().unwrap();
        c.edit(view.handle, 2, view.text_epoch, "changed".into(), 7, 7);
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
  - terminal launch, row 294: Task 15 (R5);
  - Actions ("request identities, expected run and conversation, … unknown outcomes are never replayed") for answers, Send now and OpenCode's Remove: R7, R8 and Tasks A2-1 to A2-6;
  - "the composer, questions and banners share the column" (Frontend): A2-3 and A2-5.

  The epic's menu entries (Open terminal, Open native DeepSeek UI, Open folder, Open shell in session folder, Show running commands) are in Tasks 11, 14 and 15.
- **Review Focus:** each line names its owning test above.
- **Not in this plan:** the composer parts listed in Decisions, filed by Task 12; automatic tool approvals (a separate opt-in, off by default; nothing in A2 answers without a click); Windows ConPTY (T1b, `zerus-5wmh.9.16`); macOS microphone and camera entitlements (M1 and M2, with the coordinator); the voice driver itself (M1c, the events session's).
- **Review of 2026-10-28** (the coordinator's amendments and a review pass): one draft store and one exit hook shared with M1c and the organization branch; R3 and R4 delivered and Task 13 rebound to them; Qt's stores read-only with a byte-for-byte test; the unknown-delivery rules made explicit and tested (stale questions, exit while saving); no digests per view; rename collisions keep both drafts; Tasks 3 and 4 split; each task adds only what it uses.


## As built: offline composer Refresh (`zerus-5wmh.9.31`)

The current Qt connection is `SessionsWindow.cpp:1031-1034`: the selected
composer's `sessionRefreshRequested` refreshes the listing and calls `inspect()`.
The earlier 905-912 reference is the OpenCode-start completion, not this signal.
The client's offline equivalent nudges that computer's existing source outside
its retry delay; it adds no poller. Online Refresh keeps its existing inspect
behavior.

`Composers::refresh` validates the open handle, captures the offline computer
under the hub lock, then calls `Deps::reconnect` after unlocking.
`AppState::reconnect` routes only to an existing Fleet; replay and a source not
yet started do nothing. `Fleet::reconnect` reaches only that computer's existing
`StreamHandle`. One bounded atomic wake coalesces pending requests, also when
the input queue is full. Online or in-flight sources, absent/ended computers
and stale wake inputs do nothing. Catalog state and every subscription remain
owned by the same supervisor. For older CLIs, the wake makes the existing
single-flight listing slot due now instead of starting a second read loop.
Its retry eligibility comes from the last listing outcome and whether that slot
is in flight, including after a periodic capability re-probe.

Fake-source regressions cover immediate retry outside a 30-second back-off,
repeated requests, unchanged other computers, retained rows/follows, online and
absent/ended refusals, queue saturation and old-CLI listing retry. Composer
regressions cover stale handles, closed/deselected views, unchanged online
inspection and a selection change while the captured callback is held.
