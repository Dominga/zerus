# Tauri Client Phase 4: Rust and Core Section (Terminal, Attachments, Send Flow, Processes, Media)

> **For agentic workers:** this is a section for the web client session's phase 4
> plan (`zerus-5wmh.9`). That session merges it into its own plan; the events
> session implements the tasks marked **core**. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** The Rust side of the phase 4 features, with Qt's exact behaviour:
- the session Terminal, a PTY around `hgs a`;
- the composer's attachments and the guarded send;
- the send flow around it: block reasons, the cold cache, resume-then-send,
  compact-to-send, failed local messages and durable drafts;
- the Processes tab's output and stop;
- voice and round video capture.

The webview renders. `client/core` owns processes, PTYs, limits, file staging and
outcome rules, all pure Rust without Tauri. `client/src-tauri` exposes them as
commands and channels.

**Behavioural reference (Qt).** Read the lines before each task.
- `tray/src/TerminalView.cpp`: connect, statuses, `setSession`.
- `tray/src/PtyProcess.cpp` and `tray/src/PtyProcess_win.cpp`: the PTY, its bounds and stop.
- `tray/src/TerminalScreen.cpp`: `MaxHistoryLines = 5000`.
- `tray/src/NativeSessionCapabilities.h`: `terminal`, `terminalReason`.
- `tray/src/SessionsWindow.cpp`:
  - around 4011-4026: terminal availability and its reasons;
  - around 2630-2645: pasting dropped paths into the terminal.
- `tray/src/HgsClient.cpp`:
  - `requestSendMessage`, about 1962-2104;
  - `requestStageFiles`, about 2106-2136;
  - the attachment read, about 1104-1127;
  - `requestProcess`, about 902-916.
- `tray/src/HgsClient.h`: the limits at 280-283.
- `tray/src/MessageComposer.cpp`: `attachFile`, `addAttachment` and the pasted image, about 380-400 and 968-1000.
- `tray/src/AttachmentViewer.h`: `AttachmentFiles::storeAt` and `cachedAt`.
- `tray/src/ProcessView.cpp`: `active`, `stoppable`, `status`, `outputRevision`.
- `docs/reference.md`: "Processes launched by agents".
- Send flow, in `tray/src/SessionsWindow.cpp`:
  - `messageBlockReason`, about 4386-4432;
  - `contextActionReason`, about 3660;
  - `messageAction` (retry, delete, inspect), about 4470-4527;
  - `compactToSend` and `continueAfterCompact`, about 4536-4625;
  - `canStartStopped`, `startToSend`, `continueAfterStart`, about 4672-4805;
  - `sendMessage` with the cold cache dialog, about 4806-4853;
  - `deliverMessage`, `finishMessage`, `reconcileMessages`, about 4855-4990.
- `tray/src/CacheStatus.h`: `expired`, `warning`, `chipState`, `details`.
- `tray/src/ComposerDraftStore.cpp`, `MessageComposer.cpp` (Saved drafts, about
  1232) and `tray/update-installed.py` (`drafts_dir`, `snapshot`, `changed_drafts`).
- `tray/src/speech/`: `SpeechRecorder`, `SpeechClient`, `SpeechComposer`, `RoundVideo`;
  `services/mobile/relay/src/voice_video.rs` (the video payload's limits).

## Global constraints

- Unknown outcomes are never replayed automatically. Every write keeps its
  request identity, expected run and conversation, and Qt's texts.
- Message bytes never appear in argv, a shell command or the clipboard. They go
  only to the standard input of `hgs [@host] send NAME --json`.
- Bounded everything:
  - PTY reads at most 256 KiB per batch;
  - queued terminal input at most 4 MiB;
  - text at most 64 KiB;
  - at most 8 attachments, each nonempty and at most 10 MiB, 20 MiB in total;
  - a pasted image below 32 megapixels;
  - an attachment read reply at most `10 MiB * 4/3 + 8192` bytes;
  - process output as `hgs` bounds it.
- All reads and writes run off the UI thread. Tests use fake `hgs` scripts and
  synthetic fixtures only (`/home/example/...`, `claude/sample-project/review`).
- Headless GUI runs only, as `client/README.md` ("Headless runs") states.

## Task T1 (core): the terminal attach process

**Files:** `client/core/src/terminal.rs` (new), `client/core/Cargo.toml` (one PTY crate, `portable-pty`, reviewed for Unix and ConPTY), `client/core/tests/terminal.rs`.

- [ ] `terminal::Attach::start(hgs, computer, name, run_id, cols, rows)` spawns
  `hgs [@host] a NAME --existing [--run-id RUN]` in a new PTY.
  - Environment: remove `TMUX` and `TMUX_PANE`, `HGS_SESSION` and `HGS_RUN_ID`; set `HGS_TAB=0`, `TERM=xterm-256color` and `COLORTERM=truecolor`.
  - Size: clamp to 2-1000 columns and 2-500 rows.
  - Startup errors, as Qt's texts:
    - "Attach executable is unavailable: {program}" when `hgs` is missing or not executable;
    - "Cannot open terminal: {error}" when the PTY fails.
- [ ] Reader thread: delivers output in chunks of at most 256 KiB through a
  bounded channel. When the channel is full, the reader waits, so the agent's
  output never grows the client's memory.
- [ ] `write(bytes)` queues at most 4 MiB of pending input. Beyond it, it fails
  with "Terminal input queue is full; wait before pasting more text.".
- [ ] `resize(cols, rows)`: the same clamps. On Unix this is `TIOCSWINSZ`, on
  Windows `ResizePseudoConsole`.
- [ ] `stop()`:
  - On Unix, SIGHUP to the PTY's own process group. It holds only `hgs a`, ssh and the tmux client, never a tmux server or an agent. Then close the master and reap without blocking.
  - On Windows, a kill-on-close job object, as `PtyProcess_win.cpp` does.
- [ ] `finished(code)` reports the exit code. A failed `start` reports an error,
  never a finish.
- [ ] Replay buffer: the core keeps the last 1 MiB of output per attach, so a
  remounted or reloaded view can redraw before new output. Scrollback itself is
  the webview terminal's: 5000 lines, as Qt's `MaxHistoryLines`.
- [ ] Tests run against a fake `hgs` that echoes its stdin and prints `$COLUMNS`
  and `$LINES`:
  - the arguments and environment;
  - the size clamps and resize;
  - the 4 MiB input bound;
  - bounded reads under a flood;
  - stop ending the group, with no stray processes left;
  - the exit code.

## Task T2 (core): terminal rules

**Files:** `client/core/src/terminal.rs`.

- [ ] `terminal::available(row, details, online, restored_pending) -> Result<(), String>`
  returns Qt's reasons:
  - "Machine unavailable. Reconnect when it is online.";
  - "Refresh the restored session before opening Terminal.";
  - `NativeSessionCapabilities::terminalReason`, port it with its texts;
  - "Resume this session before opening its terminal." when the state is not `running`.

  Terminal support follows `NativeSessionCapabilities::terminal`:
  - `dsh` never;
  - OpenCode by `terminal_attach_supported` when it is reported;
  - otherwise `terminal_supported` from the details, then from the row.
- [ ] `terminal::target_changed(old, new) -> bool`, Qt's `setSession` rule. A
  different endpoint, a different name without the same run, or a different
  known run reconnects and clears the screen. A later inspection that adds a run
  ID never drops an established one.
- [ ] `terminal::paste_paths(paths, windows) -> Result<String, String>`:
  - a path with a control character fails with "File names containing control characters cannot be pasted into Terminal.";
  - POSIX paths are single-quoted with `'\''`;
  - Windows paths are converted to native form and PowerShell-quoted;
  - the result is space-joined with a trailing space.
- [ ] Statuses for the view, as Qt's texts:
  - "Connect to use the session's live terminal";
  - "Connecting to {name}…";
  - "Connected: {name}";
  - "Disconnected — the session keeps running", or "Disconnected — the session continues on {machine}" when the machine is known;
  - "Attach client exited ({code}) — session output is preserved above" for a nonzero exit.
- [ ] Unit tests for each rule.

## Task T3 (Tauri, web client session): terminal commands

**Files:** `client/src-tauri/src/terminal.rs`.

- [ ] `terminal_open` returns an ID. Output and the finish go through a
  `tauri::ipc::Channel`.
- [ ] `terminal_write`, `terminal_resize` and `terminal_close`.
- [ ] One attach per view. Closing the window or switching sessions stops it
  (`TerminalView::closeEvent`, `setSession`).

## Task A1 (core): attachment staging

**Files:** `client/core/src/attachments.rs` (new), `client/core/Cargo.toml` (a content-sniffing MIME crate such as `infer`, with a fallback of `text/plain` for UTF-8 text and `application/octet-stream` otherwise), `client/core/tests/attachments.rs`.

- [ ] `Draft { text, attachments: Vec<Attachment { name, mime, bytes, reference }>, next_number }`.
- [ ] `add(draft, name, mime, bytes) -> Result<String /*reference*/, String>`, with Qt's checks and texts:
  - "Attach up to 8 files per message.";
  - "The attachment is empty.";
  - "Attachments: 10 MiB per file, 20 MiB per message.".

  The reference is `[Image #n]` for `image/*`, otherwise `[File #n]`, with `n`
  incremented until the text does not contain it. The insertion spacing rule
  (a space before when the left neighbour is not whitespace, a space after when
  at the end or the right neighbour is not whitespace) is a pure function of
  text and selection.
- [ ] `read_file(path)` runs off the UI thread. It reads at most 10 MiB + 1
  bytes and fails with "Choose a file of at most 10 MiB." or "Cannot read {name}:
  {error}". The MIME type comes from the content.
- [ ] `pasted_image(png_bytes, width, height)`: "Image is too large. Use an
  image below 32 megapixels." The name is `pasted-image.png`.
- [ ] `store_at(cache_root, name, bytes) -> Option<PathBuf>`, Qt's
  `AttachmentFiles::storeAt`:
  - the path is content-addressed by SHA-256;
  - the root and folder are owner-only, files `0600`;
  - the name is sanitized to letters, digits, `._ -`, keeping the extension up to 16 chars, at most 180 chars and 240 bytes, never splitting a surrogate pair;
  - trailing dots and spaces are stripped, `attachment` is used when empty, Windows device names get a `_` prefix, `.lnk` gets a `_` suffix;
  - an identical plain file is reused, and a link is never followed;
  - writes are atomic.

  `cached_at(cache_root, local_path)` accepts only a regular file under the
  canonical root, nonempty and at most 10 MiB.
- [ ] Unit tests for each limit, reference numbering and spacing, name
  sanitizing (devices, `.lnk`, long names, trailing dots), reuse, and refusing a
  link.

## Task A2 (core): the guarded send

**Files:** `client/core/src/actions.rs` (a `Send` action beside phase 2's), `client/core/tests/actions.rs`.

- [ ] Before running, as Qt's checks and texts:
  - "A message to this session is already being sent." (one in flight per host, name and agent);
  - "Refresh the session before sending a message." (no name or expected run);
  - "Use at most 64 KiB of text and 8 attachments.";
  - "Each attachment must be nonempty and at most 10 MiB.";
  - "Attachments exceed 20 MiB in total.";
  - "Write a message or attach a file first.";
  - "Cannot start hgs: executable not found or not executable: {path}".
- [ ] The invocation is `hgs [@host] send NAME --json`, with a timeout of 15 s
  locally and 45 s on a peer. Standard input:
  `{request_id (a fresh UUID), text, attachments: [{name, mime, data_base64, reference?}], expected_run_id, expected_conversation_id, agent_id?, expected_compaction_id?}`.
- [ ] Classification, as Qt's:
  - **Timeout:** uncertain, "Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.".
  - **Nonzero exit:** uncertain unless the exit is a normal code 1 without "delivery uncertain" in stderr. The detail is stderr (at most 4000 chars) or "Message transfer failed (exit {code}).".
  - **Failed start:** certain, "Cannot start hgs: {error}".
  - **Write failure:** uncertain, "Message transfer failed. Inspect Terminal before retrying.".
  - **Success:** only a receipt with `status: submitted` whose `request_id`, `agent_id`, `name`, `run_id` and `conversation_id` all match the request (for a first message with no conversation yet, `first_message: true`). Anything else is uncertain, "Delivery acknowledgement is invalid. Inspect Terminal before retrying.".
- [ ] Stage files for a session's machine (Qt's `requestStageFiles`) as the
  `attachment` session action:
  - `{expected_run_id, expected_conversation_id, attachments, expected_created?}`;
  - 1-8 files, each readable, nonempty and at most 10 MiB, 20 MiB in total;
  - Qt's texts: "Drop between 1 and 8 files.", "Each file must be readable, nonempty, and at most 10 MiB." and "Files exceed 20 MiB in total or changed during transfer.".
- [ ] Tests with a fake `hgs`:
  - each pre-check;
  - the exact stdin, with the message never in argv;
  - each classification;
  - an uncertain outcome is never retried.

## Task A3 (core): reading an attachment of an Activity message

**Files:** `client/core/src/reads.rs` (a `Read::Attachment` beside phase 3's reads).

- [ ] `hgs [@host] attachment NAME --request ID --index N --conversation C`.
  - Output is capped at `10 MiB * 4/3 + 8192` through `run_limited`; over the cap fails with "Attachment response is too large.".
  - The reply's metadata without `data_base64`, plus the decoded bytes. An invalid reply fails with "Invalid attachment response. Try again.".
  - The bytes are cached through `store_at` for viewing or opening.

## Task P1 (core): Processes output and stop

**Files:** `client/core/src/process_view.rs` (new), `client/core/src/actions.rs` (the stop write).

- [ ] Rules ported from `ProcessView.cpp`:
  - `active(record)`: status in starting, approval, running, stopping or unknown;
  - `stoppable(record)`: active, not unknown or stopping, and `capabilities.stop`;
  - `status_label(record)`: "Unconfirmed", "Awaiting approval", "Timed out", otherwise the status capitalized;
  - `output_revision(record)`: `{status, output_at, ended_at}`, so output is read again only on a meaningful change;
  - the grouping into live processes, "Recorded commands / unconfirmed" and "Finished".
- [ ] `Read::ProcessOutput`: `hgs [@host] processes NAME --output ID --run RUN
  --conversation C [--archive A] [--generation G]`, with a 45 s timeout. The
  reply is `{output, truncated, status}`.
- [ ] `Action::StopProcess`: the same arguments with `--stop ID`. Stops are
  sequential and never retried, and an identity change cancels unsent ones,
  following `docs/reference.md`.
- [ ] Cadence data for the view: 2.5 s while Processes is open, 10 s for
  Unconfirmed output, 0.5 s before a loading indicator. The inventory itself
  already comes from the `processes` topic (phase 2b).
- [ ] Unit tests for each rule, and fake `hgs` tests for the read and the stop.

## Send flow (design's phase 2 review)

These tasks wrap A2. The core holds the rules and the state; the webview shows
the texts and asks the questions. Every rule is a pure function or a state
machine without threads, so the tests need no process. src-tauri feeds them the
selected session's inspection states from the activity follow and runs the
actions they return on `Lane::Write`.

### Task S1 (core): send block reasons

**Files:** `client/core/src/send.rs` (new), `client/core/tests/send.rs`.

- [ ] `send::block_reason(&BlockFacts, details: Option<&Value>) -> Option<String>`,
  Qt's `messageBlockReason` in its order. `BlockFacts` carries what the
  inspection does not: a start-to-send or compaction waiting for this session, a
  context reset, a termination or model settings change in flight, `online`,
  the row state, a pending session update or rename, `cmd`, the inspection
  error and `tracked`. The 22 rules and texts, from
  "Waiting for the session to start. This exact message will be sent
  automatically when ready. Cancel send to edit." to "Waiting for the agent to
  confirm its input state.", including the agent's own `*_message_can_send`
  and `*_message_reason` fields (resume, first, interrupted, provider error).
- [ ] `send::context_action_reason(details, "compact" | "clear")` and its
  sentence form, as Qt's `contextActionReason` and `contextReasonSentence`.
- [ ] A table test with one case per rule, each proving the rules before it
  take precedence.

### Task S2 (core): the cold cache

**Files:** `client/core/src/cache.rs` (new).

- [ ] `cache::expired(details, now)`: `cache_hint.status == "cold"`, or a warm
  `prompt_cache` whose `expires_at` has passed.
- [ ] `cache::warning(details)` and `cache::chip(details, now)`
  (`{state, label, short}`: "Cold cache"/"Cold", "Clear suggested"/"/clear",
  "Cache ~{n}m"/"~{n}m") and `cache::details(details, recorded)`, with
  `CacheStatus.h`'s texts.
- [ ] `cache::cold_prompt(details, compacting, clearing) -> ColdPrompt`: the
  title "Continue with a cold cache?", the body (the warning, a blank line,
  then Qt's paragraph), and three choices with `enabled` and a reason each:
  "Send with full context", "Compact and continue" (enabled when
  `compact_context_supported` and no compaction runs) and "Clear context"
  (enabled when `clear_context_supported` and no clear runs). The send itself
  repeats the check (S3): a session that turned cold after the prompt is not
  sent without consent.

### Task S3 (core): resume-then-send and compact-to-send

**Files:** `client/core/src/send.rs`.

- [ ] `send::can_start_stopped(&BlockFacts, row)`: online, `paused` or
  `stopped`, tracked, `cmd` in codex, claude or kimi, nothing pending for the
  session and no start-to-send anywhere (Qt allows one at a time).
- [ ] `send::start_choices(row, context_usage)`: Qt's "Start the session to
  send?" question, its informative text and three choices: "Resume and send"
  and "Resume, compact and send" (both enabled only when `resumable`), and
  "Start fresh and send".
- [ ] `Continuation`, a state machine per session key. Inputs:
  - the chosen start;
  - the draft's current text and attachments;
  - action outcomes;
  - inspection states;
  - the clock.

  Outputs: one action (resume, start fresh, compact with a fresh command UUID,
  or the A2 send with `expected_compaction_id`), the composer's waiting text, or
  a cancellation text.
  - Before the start it checks the identity again: key, host, name, created,
    run and conversation. A change gives "The session changed. Your draft was
    kept without sending." and a changed draft gives "Draft changed. Send it
    when you are ready.".
  - After a start it waits for a new run. A fresh start that keeps the
    conversation, or a resume that opens another one, gives "The agent opened
    an unexpected conversation. Your draft was kept without sending.".
  - The new run must be ready: tracked, live, running, idle, no error and no
    unanswered resume or first-message gate. After 180 s it gives "The agent
    did not become ready. Check Terminal; your draft was kept without sending.".
  - A compaction must report a matching `compact_context_request.request_id`
    as `completed`, with the same run and conversation, before the send.
    `unchanged` gives "No history could be compacted. Your draft was kept
    without sending."; `failed`, `cancelled` and `uncertain` give "Compaction
    did not complete. Check Terminal; your draft was kept without sending.";
    after 600 s, "Compaction was not confirmed. Check Terminal; your draft was
    kept without sending.".
  - The intent is removed before its send is returned, so duplicate inspection
    states never send twice. No step is ever replayed after an unknown outcome.
  - The send counts as consent to the full context (Qt's `contextChosen`).
- [ ] Tests drive each transition and each cancellation with synthetic
  inspection states, and prove one send at most per continuation.

### Task S4 (core): local messages, retry, delete and inspect

**Files:** `client/core/src/local_messages.rs` (new).

- [ ] `LocalMessages`: rows per session key, at most 50 per session (oldest
  dropped first), in memory as in Qt. The pump's mirror replays them after a
  webview reload. A session renamed in place moves its rows.
- [ ] `sending(key, request_id, text, files, run, conversation)`. It replaces
  earlier failed rows with the same ID, or the same text and attachments.
- [ ] `finished(request_id, Outcome)`:
  - success sets `sent`, the receipt's `request_id` (also on each file with its
    index), `submitted_text` and `submitted_at`;
  - failure sets `error` with `uncertain`, and an uncertain detail becomes
    "Delivery is not confirmed. Check Terminal before retrying. {error}".
- [ ] `reconcile(key, details, events)` drops rows that the agent's own history
  now shows. That is a sent row matched by `attachment_messages`, by a
  `UserPromptSubmit`, `UserPromptQueued` or `TurnStarted` event, or by
  `voice_messages` for a voice row. It also drops rows from another run or
  conversation, except rows still `sending`.
- [ ] `retry_allowed(row, details, block_reason)`: only a definite `error`
  (never `uncertain`), with the current run and conversation, no block reason
  and no retry read in flight. Retry re-reads the attachments from the cache
  and checks the identity again. A cold session goes through S2's consent.
- [ ] `delete` removes an `error` row. `inspect` routes:
  - `dsh` to Native UI;
  - a session with a terminal to Terminal;
  - otherwise a refresh, with "Refreshing the native session. Check its
    provider configuration on {machine} if the error remains.".

### Task S5 (core): durable drafts

**Files:** `client/core/src/drafts.rs` (new), `client/core/tests/drafts.rs`.

- [ ] Qt's store format, version 1, so drafts can move between the clients:
  - one private (0700) folder per key, named by the SHA-256 of the key;
  - `draft.json` with `{version, key, text, position, anchor, label, form_state, next_attachment_number, sending, uncertain, attachments: [{name, mime, reference, sha256, bytes}]}`;
  - immutable `<sha256>.bin` attachment snapshots (0600).
  - Writes are atomic: temporary file, fsync, rename. Blobs are written before
    the manifest; blobs the manifest no longer names are removed after it.
- [ ] One writer thread with ordered operations. Edits of the same key
  coalesce, but a durable save, a load or the index seals the queue, so no
  later edit jumps ahead of it (Qt's `DraftWrites`). `flush()` runs on quit and
  on SIGTERM before the process exits.
- [ ] `load(key)` validates the version, the key, at most 8 attachments, and
  the sizes and hashes. A failure keeps the files and reports "Could not
  restore this draft completely. Saved files have been kept locally.". A draft
  saved as `sending` or `uncertain` loads as uncertain, with "Delivery was
  interrupted. Check Activity or Terminal before sending again.".
- [ ] `index()` lists Saved drafts: nonempty drafts only, the label, the first
  nonblank line cut to 200 chars, the modification time and the attachment
  count. Manifests over 1 MiB are skipped. It includes drafts of ended and
  archived sessions.
  - `owner(key)` parses Qt's key kinds: message `machine\nsession[\narchive]`,
    subagent (4 parts), question, voice, task, task description and task
    comment.
- [ ] `rename(old, new)` refuses an existing nonempty target with "Another draft
  already exists here. Recover the previous one from Saved drafts.".
- [ ] **Location (agreed with design).** The Tauri client and the Qt GUI can
  run at the same time during the migration, so each store has exactly one
  writer:
  - the client writes only its own root, `<app data>/drafts`, in the same
    format;
  - Saved drafts also lists Qt's store (`hgdev/hgs-tray.conf.drafts`, or the
    plist sibling on macOS);
  - "Recover" copies a Qt draft into the client's root, and the Qt copy stays.

  Qt's store is strictly read-only for the client: no locks, writes or renames.
  Qt replaces its files atomically while the client reads them. A manifest
  that fails to parse, or a blob that does not match its hash, is read once
  more before it counts as invalid.
- [ ] Follow-ups outside phase 4, filed under their phases:
  - `zerus-5wmh.13` (side-by-side period): when the owner switches the
    default, a one-time import of all Qt drafts into the client's root,
    de-duplicated by key and content hash.
  - `zerus-5wmh.12` (packaging and updates): the client's updater quits the
    client gracefully, flushes, snapshots and verifies its drafts root, as
    `tray/update-installed.py` does for Qt's.
- [ ] Tests:
  - ordering under coalescing;
  - a crash between the blob and manifest writes;
  - hash and size validation;
  - the index with ended-session drafts;
  - rename collisions;
  - private permissions on Unix.

## Media: voice and round video (layout agreed; M1 next)

Voice and round video capture, encoding and transcription are the events
session's; the recorder controls, the mode switch, the player and the progress
ring are the web client's (spec lines 212-214).

**What Qt sends.** Voice does not use the A1 staging or `send`.
- Qt talks to `hgs [@host] voice NAME --json` (actions `profiles`, `submit`,
  `status` and `cancel`, with the request identity and the expected run and
  conversation).
- For dictation into a draft, it talks to `hgs speech --json` (actions
  `draft_profiles`, `draft_submit`, `draft_status` and `draft_cancel`).
- Before the first call it checks `hgs --help` for `voice ... --json` and
  shows "Voice messages need an updated hgs CLI on the conversation computer."
  when it is missing.
- The audio is `{format: "pcm_s16le", sample_rate: 16000, channels: 1, data_base64}`,
  at most 120 s.
- A round video adds `{format: "mp4", data_base64, duration_ms, width, height,
  poster_base64?}`. `voice_video.rs` checks it: `ftyp`, `moov` and `mdat`
  boxes, at most 10 MiB and 60 s, 16-1920 px per side, and a duration within
  1 s of the PCM.
- Every player plays H.264 with AAC: WKWebView, WebKitGTK, WebView2, Android,
  iOS and Qt. So that is the encoding.

**Proposed layout:**
- `client/core/src/voice.rs`, pure:
  - the PCM converter, porting `SpeechPcmConverter` (8-192 kHz and up to 32
    channels in; 16 kHz mono s16le out; capped at 3,840,000 bytes);
  - the payload builders and reply checks (`schema: 1`, at most 1 MiB);
  - the capability probe;
  - the voice composer's states (Idle, Starting, Recording, Saving, Preview,
    Submitting, Uncertain);
  - the voice draft under the key `voice-message-draft\n{session key}` in S5's
    store.

  The core holds the captured PCM and MP4 bytes, so no file path is needed.
- `client/media` (new crate), which src-tauri uses behind a `media` feature:
  - audio through `cpal` on its own thread, with a level meter (RMS and peak,
    about 30 Hz);
  - the round video per platform: AVFoundation with `AVAssetWriter` through
    `objc2` on macOS; GStreamer (`v4l2src`/`pipewiresrc`, an H.264 and AAC
    encoder, `mp4mux`) through `gstreamer-rs` on Linux; Media Foundation's
    source reader and sink writer on Windows.

  The camera format closest to 640x480 at 15 fps or more is chosen. Video is
  24 fps at 700 kbit/s and audio 64 kbit/s mono. Recording stops at 59 s or
  10 MiB less 128 KiB. The `cpal` audio feeds both the AAC track and the PCM,
  so nothing is decoded afterwards (unlike Qt's `extractAudio`). The poster is
  a 160 px JPEG of at most 8 KiB.
- src-tauri commands:
  - `voice_start`, `voice_stop` and `voice_cancel`;
  - the level stream on a Channel;
  - a preview frame stream for the video: a square crop, about 15 fps, 240 px
    JPEG, with a mirror flag. A native preview surface cannot sit inside the
    webview on every platform.
  - The result is the voice composer's state, not a file path.
- Permission texts as Qt's: "Allow camera and microphone access, then hold
  Video again." and "A camera and microphone are required to record video.".
- macOS permissions belong to these tasks:
  - the camera and microphone prompts (`AVCaptureDevice` authorization, never
    requested before the user holds Voice or Video);
  - `NSCameraUsageDescription` and `NSMicrophoneUsageDescription` in the
    bundle's Info.plist;
  - the hardened runtime entitlements `com.apple.security.device.camera` and
    `com.apple.security.device.audio-input`.
- Capability probes (agreed with the web client): each platform backend
  answers with a reason the composer shows, never a bare code: no microphone,
  no camera, permission denied, or encoder missing.
  - Linux probes the GStreamer elements at runtime: `x264enc`, `openh264enc`
    or VA-API for H.264, and `avenc_aac` or `fdkaacenc` for AAC. Without them,
    video is unavailable with that reason and voice still works.
  - Linux prefers `pipewiresrc` where PipeWire runs, otherwise `v4l2src`.
  - The plugin packages become optdepends of the AUR packages (`zerus`,
    `zerus-git` and the binary packages) when the Tauri client ships. The M PRs
    note this but leave packaging unchanged.
- Build dependencies (ALSA and GStreamer development packages) enter CI only
  with the `media` feature and only on the jobs that build it. `cargo build`
  without `media` needs none of them.
- Playback is the web client's, from A3's cached file through a Tauri URI
  scheme. `voice.rs` exposes the duration, the waveform and the video poster
  from the message metadata, so the webview decodes nothing to show them.
- Persistence: a voice or video draft lives in S5's store under Qt's key rules
  (`voice-message-draft\n{session key}`), so a crash loses at most the unsaved
  tail. `voice.rs` documents what is written when: the recording when it
  stops, the submit's request identity before it is sent, and the outcome
  when it arrives.
- The Info.plist keys and the entitlements go in the M1 (microphone) and M2
  (camera) PRs. The coordinator is told, since the signing configuration in
  `docs/macos-distribution.md` is theirs.
- The pipeline mirrors Qt's exactly, as `SpeechRecorder`, `SpeechComposer` and
  `RoundVideo` define it: states, limits, texts, profiles, the draft, preview
  before send, and the uncertain and cancel paths. Each M task ports the Qt
  tests that cover its part.

**Proposed order:**
- M1: voice audio (core, plus `cpal` in `client/media`), on every platform.
- M2: round video on macOS.
- M3: round video on Linux.
- M4: round video on Windows.

The `getUserMedia` path stays a fallback after per-engine permission and
`MediaRecorder` format checks (spec row 296).

Native UI (DeepSeek) is the web client's (spec row 302). A rule it needs from
the core comes as a small pure function, like `terminal::available`.

## Order and collisions

T1, T2, A1, A2, A3, P1 and S1-S5 are core-only new modules. The events
session can start them now; they collide with no open branch. S3 and S4 need A2;
S2 is needed by S3; S5 is independent. M1 waits for the layout to be agreed. T3 and the views are the web
client session's. `actions.rs` and `reads.rs` gain variants only, never changed
behaviour. Each task is one PR, merged on a green CI gate.
