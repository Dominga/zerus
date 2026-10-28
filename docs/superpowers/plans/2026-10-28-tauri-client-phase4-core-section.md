# Tauri Client Phase 4: Rust and Core Section (Terminal, Attachments, Processes)

> **For agentic workers:** this is a section for the web client session's phase 4
> plan (`zerus-5wmh.9`). That session merges it into its own plan; the events
> session implements the tasks marked **core**. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** The Rust side of three phase 4 features, with Qt's exact behaviour:
- the session Terminal, a PTY around `hgs a`;
- the composer's attachments and the guarded send;
- the Processes tab's output and stop.

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

## Order and collisions

T1, T2, A1, A2, A3 and P1 are core-only new modules. The events session can
start them now; they collide with no open branch. T3 and the views are the web
client session's. `actions.rs` and `reads.rs` gain variants only, never changed
behaviour. Each task is one PR, merged on a green CI gate.
