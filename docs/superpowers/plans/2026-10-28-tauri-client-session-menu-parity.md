# Tauri Client: Sessions-Menu Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client offers every remaining entry of the Qt session menu, the selection bar and the batch menu, with Qt's `hgs` calls, inputs, timeouts, enablement, confirmations, texts and outcome rules:
- the copy actions (Beads `zerus-5wmh.19`);
- attention marks, Mark needs attention, Mute and Unmute, Mark all as read and local read marks (`zerus-5wmh.16`);
- Start fresh, Forget saved session, Restore, Forget archived session and Clear archive (`zerus-5wmh.17`);
- the Archive view those archived actions need (`zerus-5wmh.32`, filed 2026-10-28 with the client owner session's agreement);
- multi-selection batch actions and the batch menu (`zerus-5wmh.20`).

Project organisation (Move to project, project header menu, grouping: `zerus-5wmh.18`, `.30`, `.31`) is not in this plan.

**Architecture:** The phase 2 action layer is extended, not replaced.
- **Pure core (`client/core`):**
  - `actions.rs` gains the new `Action` kinds with their arguments, input, timeouts and outcome rules;
  - `shell.rs` (new) ports `SessionCommands` (POSIX and PowerShell quoting);
  - `view.rs` projects the extra row fields the rules read;
  - `marks.rs` (new) keeps the device's attention marks and read replies;
  - `readsync.rs` (new) ports `ReadMarkSync` (the queued delivery of read marks);
  - `reads.rs` gains the archive page read.
- **`client/src-tauri`:** new commands (`copy_text`, `session_reference`, `copy_command`, `notifications`, `attention_marks`, `update_attention_marks`, `queue_read_marks`, `archive_page`) and the read-mark delivery worker.
- **SolidJS:** `store/marks.ts` (new) overlays the marks on catalog rows; `store/archive.ts` (new) holds archive pages; `store/batch.ts` (new) runs a selection one session at a time. Menu entries are built in `actions-ui.ts` and `ActionDialogs.tsx`, so `SessionList.tsx` changes by a few lines per entry.

Unknown outcomes are shown and never replayed.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` ("Actions"; "Live rows only; Archive stays a keyset-paged read"). Pattern: `docs/superpowers/plans/2026-10-28-tauri-client-phase2-actions.md` and `client/README.md` ("Actions"). The behavioural reference is the Qt GUI:
- `tray/src/SessionsWindow.cpp`:
  - `showSessionMenu` (5266-5382);
  - `copySessionReference` (5384-5390);
  - `runSessionMenuAction` (5392-5492);
  - the reference, notification and forget-binding results (538-555, 594-595, 1003-1007);
  - `writeDone` (1915-1972);
  - `markAllRepliesRead` (3389-3402) and the Mark all as read action (362-365);
  - the Mark read chip (3777-3780);
  - `canStartStopped` and `startFresh` (4672-4704);
  - `changeSession` (5542-5560);
  - `terminateSession` (5758-5826);
  - `archiveNameOccupied` (3148-3154);
  - `populateBatchActions` (2844-2893).
- `tray/src/SessionSelection.cpp` (whole file: applicability, the bar, the menu, `clearArchive`, the runner).
- `tray/src/HgsClient.cpp`:
  - `requestSessionReference` (592-604) and `requestArchivePage` (555-578);
  - `runWrite` (1174-1207), `killSession` (1227-1239), `startFreshSession` (1279-1286);
  - `restoreArchive` and `forgetArchive` (1303-1324), `pauseAll` and `resumeAll` (1326-1343);
  - `requestForgetBinding` and `runSessionAction` (2169-2245), `requestNotifications` (2401-2457);
  - `failureDetail` (466-479).
- `tray/src/FleetState.cpp` (keys and fingerprint 44-70, `updateUnread` 131-171, `setReviewLater` 196-202, `markSessionRead` 218-229, `unreadReplies` and `markRepliesRead` 234-262, `queueRead` 492-503, `legacyReadMarks` 513-530).
- `tray/src/ReadMarkSync.h` and `.cpp`, `tray/src/SessionNotifications.h`, `tray/src/SessionCommands.h`, `tray/src/WindowsCommandLine.h`, `tray/src/NativeSessionCapabilities.h`, `tray/src/ClipboardBackend.cpp` (`isCopyable`, 194-213), `tray/src/ArchiveBrowser.cpp`, `tray/src/ArchiveDatePicker.cpp`.

Read those lines before each task; this plan quotes the rules it depends on.

## Global Constraints

Everything in the phase 2 plan's Global Constraints applies. In addition:
- **Qt wins.** Where this plan and Qt disagree, Qt is right; fix the plan in the same PR.
- **Add nothing Qt does not have.** Where the client lacks a Qt prerequisite (drafts, worktree cleanup, the Windows session warning), the plan says so and leaves it to its owner task.
- **No dots as separators** (`scripts/check-ui-separators.py`).
- **No obvious keyboard hints.** Qt's "Deselect all (Esc)" becomes "Deselect all".
- **Bounded data:**
  - archive pages are one bounded page and two cursor anchors per computer (`ArchiveBrowser.h`);
  - batch queues hold only the selection;
  - attention marks and read replies, which Qt keeps without a bound, are capped at 20 000 entries per map, the oldest written dropped first (owner rule on bounded caches).
- **Shared files:** agreed with the client owner session (`claude/zerus-work-e94411a3/webui`, 2026-10-28).
  - `SessionList.tsx` changes only these entries: `sessionMenu()` (Mute, Mark as read or Mark needs attention, Start fresh, the copy entries, the Forget variants), `batchMenu()` and `SelectionBar()`.
  - `catalog.ts`, `types.ts`, `bridge.ts`, `store/actions.ts`, `view.rs`, `details.rs`, `actions.rs`, `reads.rs`: additive only.
  - `store/attention.ts` belongs to phase 3a (`needsAttention`, `isActive`); the marks live in `store/marks.ts`.
  - Rebase onto whatever lands first. Message the owner session before restructuring a function it also touches.
- **One PR per piece** (A to F below), each merged after the "CI gate" check is SUCCESS, rebased on main first. After each merge, message the owner session with the merge commit; it rebuilds the owner's Mac dev app.
- **Tests never run the real `hgs`.** The fake `hgs` (`client/core/tests/support/fake_hgs_actions.py`) gains an optional `"replies"` list, each `{"match": [args after an @alias], ...reply}`; the first entry whose `match` is a prefix of the arguments wins, otherwise `"reply"`.

## Review Focus

1. A row that changed between the menu and the click (new run, other conversation, other `created`, other state): nothing runs, and the notice is the Qt text for that path.
2. A confirmed destructive dialog acts only on the exact instance it named, and Cancel is focused and the default.
3. A batch never replays: a failed item counts as failed, the next item starts only after the previous outcome, and the summary says how many completed.
4. Read marks are local first and delivered later. A mark is never shown as read for a newer reply or question than the one marked.
5. Archive pages are validated as Qt validates them; an invalid page shows Qt's error and keeps no rows from it.
6. The copy actions never put a text with a line break on the clipboard.

---

## File Structure

```
client/core/src/shell.rs            Shell, shell_quote, power_shell_quote, command, open_arguments, open_command (SessionCommands)
client/core/src/actions.rs          Action: StartFresh, Restore, Forget, ForgetArchive, ForgetBinding, PauseAll, ResumeAll, Notifications, Reference
client/core/src/view.rs             ViewRow: cwd, created, terminal fields, notification fields, attention_fingerprint, needs_action,
                                    acknowledged_status, acknowledged_caption, unread_candidate, summary without the unread rule,
                                    bulk_lifecycle, archived_at, git_worktree
client/core/src/details.rs          Details: cwd, terminal_supported, terminal_attach_supported
client/core/src/marks.rs            AttentionMarks: marks (review, acknowledged) and read replies, durable, capped
client/core/src/readsync.rs         ReadSync: Qt's queued delivery of read marks (4 in flight, backoff, resend)
client/core/src/reads.rs            Read::ArchivePage and its checks
client/core/tests/actions.rs        new calls through the fake hgs
client/core/tests/support/fake_hgs_actions.py   "replies" by argument prefix
client/src-tauri/src/actions.rs     copy_text, session_reference, copy_command, notifications, attention_marks, update_attention_marks,
                                    queue_read_marks
client/src-tauri/src/readsync.rs    the delivery worker around core ReadSync
client/src-tauri/src/lib.rs         registers the commands, the clipboard plugin and the worker
client/src/types.ts, bridge.ts      the new fields, actions and commands
client/src/store/marks.ts (+test)   marks store and the row overlay
client/src/store/archive.ts (+test) archive pages per computer
client/src/store/batch.ts (+test)   selection runner
client/src/store/restores.ts (+test)   Restore, its follow-up and limit, Forget archived, the batch's archive calls
client/src/store/actions.ts         new actions, notices, the global batch busy flag
client/src/store/catalog.ts         new flat Session fields; the overlay hook
client/src/ui/sessions/actions-ui.ts (+test)   menuState additions, selection applicability, dialog copy
client/src/ui/sessions/ActionDialogs.tsx       entries, Start fresh, Forget and batch confirmations
client/src/ui/sessions/archive-entries.ts (+test)   Clear archive: the entry and its loaded rows in scope
client/src/ui/sessions/ArchiveBar.tsx          date range, Older, Newer, Refresh, status
client/src/ui/sessions/SessionList.tsx         wiring of the entries above only
client/src/ui/sessions/SessionDetail.tsx       Mark read chip detail, Start fresh header action
client/README.md                    "Actions": the new calls, marks, delivery, archive and batches
```

---

## Piece A: copy actions (`zerus-5wmh.19`), PR 1

### Task A1: `SessionCommands` in the core (`client/core/src/shell.rs`)

**Files:** Create `client/core/src/shell.rs`; modify `client/core/src/lib.rs` (`pub mod shell;`).

**Interfaces:**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell { Posix, PowerShell }
pub fn current_shell() -> Shell;                       // PowerShell on Windows, else Posix
pub fn shell_quote(value: &str) -> String;
pub fn power_shell_quote(value: &str) -> String;
pub fn command(hgs: &str, arguments: &[String], shell: Shell) -> String;
pub fn open_arguments(computer: &str, name: &str, archive_id: Option<&str>) -> Vec<String>;
pub fn open_command(hgs: &str, computer: &str, name: &str, archive_id: Option<&str>, shell: Shell) -> String;
```

Rules (`SessionCommands.h`, `WindowsCommandLine.h`):
- `shell_quote`: a non-empty value of only `A-Za-z0-9_@%+=:,./-` stays bare; otherwise it is wrapped in `'...'` with each `'` written `'\''`.
- `power_shell_quote`: a non-empty value of only `A-Za-z0-9_./\:=+-` stays bare; otherwise `'...'` with each of U+0027, U+2018, U+2019, U+201A and U+201B doubled.
- `command`, POSIX: `shell_quote(hgs or "hgs")` and each argument quoted, joined by spaces.
- `command`, PowerShell: the program is `hgs or "hgs"` with `/` replaced by `\`, then `power_shell_quote`d.
  - When any argument fails `powerShellPassesUnchanged` (empty, contains `"`, or contains a space or tab and ends with `\`), the result is `Start-Process -FilePath {program} -ArgumentList {power_shell_quote(windows_command_line(args) without its first char)} -NoNewWindow -Wait`.
  - Otherwise a quoted program gets `& ` in front, then each argument is `power_shell_quote`d.
- `windows_command_line` ports `WindowsCommandLine::build` with an empty program: each argument starts with a space; quotes are escaped with their preceding backslashes doubled; an empty value or one with a space or tab is quoted, with trailing backslashes kept outside the closing quote.
- `open_arguments`: `@computer` first for a peer; then `a NAME`, or `resume NAME --archive ID` when an archive id is given.

- [ ] **Step 1: Write the failing tests** (in `shell.rs`), with the Qt test cases for `SessionCommands` (`tray/tests/test_terminalbackend.cpp`, `tray/tests/test_windowsdesktop.cpp`), plus:

```rust
#[test]
fn posix_quotes_only_what_needs_it() {
    assert_eq!(shell_quote("claude/sample-project/review"), "claude/sample-project/review");
    assert_eq!(shell_quote("release planning"), "'release planning'");
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
    assert_eq!(shell_quote(""), "''");
    assert_eq!(open_command("/home/example/.local/bin/hgs", "mac", "claude/sample-project/review", None, Shell::Posix),
               "/home/example/.local/bin/hgs @mac a claude/sample-project/review");
    assert_eq!(open_command("hgs", "", "claude/sample-project/review", Some("0f3c2b1a-1111-4222-8333-944445555666"), Shell::Posix),
               "hgs resume claude/sample-project/review --archive 0f3c2b1a-1111-4222-8333-944445555666");
}

#[test]
fn power_shell_uses_the_call_operator_or_start_process() {
    assert_eq!(command("C:/Program Files/Zerus/hgs.exe", &["a".into(), "claude/sample-project/review".into()], Shell::PowerShell),
               "& 'C:\\Program Files\\Zerus\\hgs.exe' a claude/sample-project/review");
    assert_eq!(power_shell_quote("it\u{2019}s"), "'it\u{2019}\u{2019}s'");
    assert!(command("hgs", &["a".into(), "claude/sample-project/release notes\\".into()], Shell::PowerShell)
        .starts_with("Start-Process -FilePath hgs -ArgumentList "));
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core shell::` and expect FAIL.
- [ ] **Step 3: Implement** with plain character checks (no regex dependency).
- [ ] **Step 4: Run** the tests and clippy (`-D warnings`). Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): SessionCommands quoting for copied commands`.

### Task A2: the reference read, the copy commands and the clipboard

**Files:**
- Modify: `client/core/src/actions.rs` (`reference_invocation`, `parse_reference`), `client/core/src/view.rs` (`cwd`, `terminal_supported`, `terminal_reported`, `terminal_attach_supported`, `terminal_attach_reported`), `client/core/src/details.rs` (`cwd`, `terminal_supported: Option<bool>`, `terminal_attach_supported: Option<bool>`)
- Modify: `client/src-tauri/src/actions.rs`, `client/src-tauri/src/lib.rs`, `client/src-tauri/Cargo.toml` (`tauri-plugin-clipboard-manager = "2"`, used from Rust only; no JavaScript permission)
- Test: unit tests; `client/core/tests/actions.rs`

**Interfaces:**

```rust
// core
pub fn reference_invocation(target: &Target, archive_id: Option<&str>) -> Invocation;
pub fn parse_reference(exit: Exit) -> Result<String, String>;
// src-tauri
#[tauri::command] pub async fn session_reference(target: Target, archive_id: Option<String>) -> Result<String, String>;
#[tauri::command] pub fn copy_command(computer: String, name: String, archive_id: Option<String>) -> String;
#[tauri::command] pub fn copy_text(app: AppHandle, text: String) -> Result<(), String>;
```

Rules:
- **Reference** (`HgsClient.cpp:592-604`):
  - The call is `hgs [@host] reference NAME --json --run RUN --conversation CONVERSATION`, plus `--archive ID` when archived; 15 s here, 25 s on a peer.
  - RUN and CONVERSATION are the selected session's details values, falling back to the row's (`SessionsWindow.cpp:5384-5390`).
  - A run failure gives `failureDetail`: "timed out", "cannot start '{hgs}': {error}", the trimmed standard error (at most 4000 chars), or "exit code {n}".
  - Exit 0 with a JSON object whose `uri` starts with `zerus://session?` and is at most 8192 chars is the reference. Anything else is "Could not read a session reference. Update hgs on this computer.".
- **Copy command:** `open_command(default_hgs(), computer, name, archive_id, current_shell())`, as `TrayAgent.cpp:742-745`.
- **Clipboard** (`TrayAgent::copyText`, `ClipboardBackend::isCopyable`):
  - A text that is empty after trimming fails with "nothing to copy: the command is empty".
  - A text with `\n` or `\r` fails with "the command has a newline in it -- copying that is dangerous: the shell would run it the moment you paste".
  - Otherwise the plugin writes it.
  - The clipboard is written from Rust because the webview loses the user activation that `navigator.clipboard` needs during the reference's `hgs` call.

- [ ] **Step 1: Write the failing tests:**
  - the reference arguments here and on a peer, with and without an archive, and both timeouts;
  - `parse_reference` for a good URI, a `https://` URI, an 8193-char URI, non-JSON, exit 1 with and without stderr, `TimedOut` and `StartFailed`;
  - `view.rs`: `cwd` is the raw `cwd`; `terminal_supported` defaults to `agent != "dsh" && agent != "opencode"` when the row has none, as `HgsClient.cpp:213-217`;
  - an integration test through the fake with `{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=n"}}}`.
- [ ] **Step 2: Run** and expect FAIL. **Step 3: Implement.** **Step 4: Run** core and src-tauri tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): session reference, copied commands and a Rust clipboard`.

### Task A3: the copy entries

**Files:** `client/src/types.ts`, `client/src/bridge.ts` (`sessionReference`, `copyCommand`, `copyText`; the browser build answers with failures), `client/src/store/catalog.ts` (Session gains `cwdPath`, `terminalSupported`, `terminalReported`, `terminalAttachSupported`, `terminalAttachReported`), `client/src/ui/sessions/actions-ui.ts` (+test), `client/src/ui/sessions/ActionDialogs.tsx`, `client/src/ui/sessions/SessionList.tsx` (`sessionMenu()` copy entries only), `client/src/store/actions.ts` (`copyReference`, `copy`).

Rules (`SessionsWindow.cpp:5306-5307, 5374-5376, 5440-5447`):
- **Terminal capability:** `terminal(row, details)` ports `NativeSessionCapabilities::terminal`.
  - `dsh` (the client's `deepseek`) never has one.
  - For `opencode`, the details' `terminal_attach_supported`, then the row's when reported.
  - Then the details' `terminal_supported`.
  - Then `opencode` without a reported value has none.
  - Otherwise the row's `terminal_supported`.
- **Copy connect command / Copy restore command:**
  - The label is "Copy restore command" when archived, else "Copy connect command".
  - Enabled when `(archived || terminal(row)) && (!archived || archiveId)`, without details.
  - It copies `copyCommand(computer, name, archived ? archiveId : null)`.
- **Copy session name:** always enabled; copies the name.
- **Copy session reference:** enabled when the computer is online and the row has a run id. It calls `sessionReference`.
  - On success it copies the URI and shows the info notice "Session reference copied".
  - On failure it shows the sticky error notice with the detail.
- **Copy folder path:** enabled when the directory is not empty; it copies it. The directory is the selected session's details `cwd`, falling back to the row's `cwd` (`selectedDirectory`, 5234-5238).
- **Notices:** a copy that fails shows the sticky error "Zerus: could not copy\n{error}". A successful copy shows no notice, except the reference.
  - Phase 1's "Copied" toast for Copy session name goes, as in Qt. Tell the owner session in the PR.
- **Menu order:** Qt's. The copy command entry follows Open shell in session folder; Copy session name, Copy session reference and Copy folder path follow Move to project.

- [ ] **Step 1: Write the failing vitest tests** for `terminal()`, the copy-entry states (archived with and without archive id, `deepseek`, `opencode` with and without reported values, offline, no run id, empty folder) and the store's notices with a mocked bridge.
- [ ] **Step 2: Run** `npx vitest run src/ui/sessions src/store` in `client` and expect FAIL.
- [ ] **Step 3: Implement** the helpers and the wiring.
- [ ] **Step 4: Run** `npx vitest run`, `npm run check` and `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): copy session reference, folder path and connect or restore command`.

**PR 1:** A1 to A3, plus a `client/README.md` paragraph. Headless smoke with the fake `hgs`: open the menu on the replay session and copy each entry; record the clipboard text the fake run produced in the PR description.

---

## Piece B: attention marks, Mute and Mark all as read (`zerus-5wmh.16`), PR 2

### Task B1: the row fields the marks need (`view.rs`)

`ViewRow` additions, all `#[serde(default)]`:
- `created: i64` (`created` as a number, truncated, as `HgsClient.cpp:164`).
- `attention_fingerprint: String`: Qt's `attentionFingerprint` (`FleetState.cpp:53-70`), compact JSON of `[run_id, needs_action ? phase : "", needs_action ? attention_id : "", needs_action && attention_id == "" ? last_event_at : null, children]`.
  - `children` maps each subagent id whose `display_state`, else `state`, is `approval`, `input` or `attention` to `[phase, request, request == "" ? last_event_at : null]`.
  - `request` is the child's `attention_id`, else its `question_id`.
  - There are no children when `subagent_source == "hook_profiles"`.
  - Only stability within the client matters, not Qt's bytes.
- `needs_action: bool` (the existing `Session::needs_action`).
- `acknowledged_status`, `acknowledged_caption`: `status()` evaluated with the attention acknowledged. They equal `status` and `caption` when there is nothing to acknowledge.
- `unread_candidate: bool`: `state != "archived" && conversation_id != "" && reply_id != "" && !reply_in_progress`. Whether it shows as unread depends on the marks (Task B3).
- `summary` no longer applies the unread rule (`view.rs:444`); the overlay blanks it while the reply shows unread, which is Qt's `currentAction` rule.
- **Notifications** (`SessionNotifications::valid`, `HgsClient.cpp:180-183`):
  - `shared_notifications: bool` is the full port of `valid(preferences, conversation_id)`: exact keys, identities, revision, `suppressed` shape, 64-hex signature, sorted unique optional ids (at most 64), at most 256 children, the ASCII-escaped size limit of 384 KiB.
  - `notification_revision: Option<i64>` holds the revision.
  - `muted` is true only when the preferences are valid.

- [ ] Tests: the fingerprint changes with phase, `attention_id`, a child request and `run_id`, but not with the summary; acknowledged status for approval, input, recovery and child input; `unread_candidate` for archived, no conversation and a reply in progress; one valid and seven invalid preference objects (missing key, extra key, other conversation, negative revision, bad signature, unsorted ids, oversized).
- [ ] Commit `feat(client): row fields for attention marks and shared notifications`.

### Task B2: durable marks (`client/core/src/marks.rs`) and Qt's read-mark delivery (`readsync.rs`)

**Interfaces:**

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark { #[serde(default)] pub review: bool, #[serde(default)] pub acknowledged: String }
pub struct AttentionMarks { /* path, marks: key -> Mark, replies: key -> reply_id, both capped */ }
impl AttentionMarks {
    pub fn load(path: PathBuf) -> Self;     // missing or unreadable: empty
    pub fn snapshot(&self) -> (BTreeMap<String, Mark>, BTreeMap<String, String>);
    // A delta: Some writes (the entry becomes the newest written), None removes; then caps and saves atomically.
    pub fn update(&mut self, marks: BTreeMap<String, Option<Mark>>, replies: BTreeMap<String, Option<String>>);
}
pub struct ReadSync { /* ReadMarkSync */ }
impl ReadSync {
    pub fn new() -> Self;
    pub fn set_pending(&mut self, pending: Vec<ReadMark>);
    pub fn due(&mut self, now_ms: i64) -> Vec<ReadMark>;   // the marks to send now, marked in flight
    pub fn finished(&mut self, mark: &ReadMark, delivered: bool, applied_all: bool, now_ms: i64);
    pub fn not_sent(&mut self, mark: &ReadMark);          // its read lane was busy: due again, no failure counted
}
pub fn applied_all(mark: &ReadMark, receipt: &Value) -> bool; // every part the mark sets is `applied: true`
```

Rules:
- **Keys** are the client's own (`FleetState.cpp:44-52`):
  - reply key `["{computer}","{agent}","{conversation}"]`;
  - attention key the reply key, or `["{computer}","{agent}","{name}",created]` without a conversation.
  - The frontend keeps the merge rules (Task B3) and sends deltas: `attention_marks()` answers `{ marks, replies, order: { marks, replies } }` (maps by key, and each map's keys oldest written first, so the window's cap drops the same entries after a restart), `update_attention_marks(marks, replies)` takes maps of key to entry or null (removal). A full replace would send megabytes per click at the cap. The file is `attention-marks.json` beside `read-marks.json` (`attention-marks-replay.json` for a replay) and lists each map as `[key, value]` pairs in write order, so the cap drops the oldest written after a restart too.
- **`ReadSync`** (`ReadMarkSync.h`, `ReadMarkSync.cpp:36-95`):
  - at most 4 marks in flight; each free slot goes to the computer with the fewest in flight, then key order;
  - a mark in flight, or one already sent unchanged that is not yet due, waits;
  - delivered and fully applied: resend after 120 s; partly applied: 120 s doubled per unapplied answer, at most 960 s;
  - failed: 10 s doubled per failure, at most 300 s;
  - a changed mark resets its counters;
  - a mark no longer pending is forgotten when it finishes.
- **The worker** (`client/src-tauri/src/readsync.rs`): a thread that every 5 s, and whenever marks are added, takes `due`, runs each `MarkRead` through `run` (the phase 2 invocation and `classify`) and reports `finished`. Phase 2's frontend quiet retry goes away in Task B3, since this worker owns delivery; the Rust Read lane stays, and the worker claims it for each call. Each wake first writes the marks `queue_read_marks` added (the command itself does no file I/O), and marks for computers the local listing no longer names are dropped once the peers are known (`publishReadMarks`, `TrayAgent.cpp:578-587`; marks for this computer stay).
  - A delivery shows no notice, as Qt (`markFinished` has no visible receiver).
  - Settling stays `Actions::observe` (phase 2): a mark is saved before any call, and an empty replace never drops pending marks.
- **Queueing from the frontend:** a new command `queue_read_marks(marks: Vec<ReadMark>) -> Vec<ReadMark>` merges each mark into `ReadMarks` (`add`, which saves before any delivery), wakes the worker and returns the pending marks. Phase 2's `run_action` with `MarkRead` stays callable, but the UI no longer uses it.
- **Lanes:** the worker claims a session's Read lane for each delivery and skips a session whose Read lane is busy; Write and Interrupt lanes are untouched (Stop never waits for a write; a read never blocks a write).

- [ ] Tests: the cap drops the oldest written; a corrupt file loads empty; `ReadSync` with a fake clock covers four in flight, the fewest-busy computer, backoff 10 s, 20 s and so on to 300 s, the 120 s resend, partial applies doubling to 960 s, and a changed mark resetting.
- [ ] Commit `feat(client): durable attention marks and queued read-mark delivery`.

### Task B3: the marks store and the row overlay (`client/src/store/marks.ts`)

**Interfaces:**

```ts
export function overlay(s: Session): Pick<Session, "status" | "caption" | "attention" | "unread" | "reviewLater" | "summary" | "muted">;
export function markRead(row: ActionRow): boolean;          // FleetState::markSessionRead
export function markNeedsAttention(row: ActionRow): boolean; // setReviewLater(..., true)
export function unreadReplies(): Map<string, string>;        // FleetState::unreadReplies
export function markAllRead(): number;                       // markRepliesRead
```

Rules:
- **Overlay** (`FleetState::updateUnread`, `FleetState.cpp:131-171`), computed in `toSession` from the base fields; a marks change re-applies it to the affected rows:
  - `reviewLater = state != "archived" && marks[attentionKey].review`.
  - A pending read mark counts when it has the row's run and conversation (`sameRun`).
  - Shared read (`sharedRead`): `unread = unreadCandidate && readReplyId != replyId && !(sameRun && pending.reply_id == replyId)`.
    - The attention is acknowledged when `attentionSignature` is empty and `marks.acknowledged == fingerprint`.
    - With a signature, it is acknowledged when `attentionSignature == readAttentionSignature` or (`sameRun && pending.attention_signature == attentionSignature`).
  - Older host: `unread = unreadCandidate && replies[replyKey] != replyId`, and the attention is acknowledged when `marks.acknowledged == fingerprint`.
  - Acknowledged attention shows `acknowledgedStatus` and `acknowledgedCaption`; an unread reply blanks `summary`.
  - The muted override of Task B4 applies last.
- **Fallback keys:** a mark under the fallback key moves to the conversation key once the row has a conversation (`FleetState.cpp:136-139`).
- **`markRead`** (`FleetState.cpp:218-229`):
  - The row must match a listed row by name, archive id, conversation and `created`, not archived, with the same `replyId`, fingerprint and `attentionSignature`; otherwise it returns false.
  - It queues a read mark for a shared-read row (`queueRead`): the reply when `readReplyId != replyId`, the attention signature when `readAttentionSignature != attentionSignature`, merged with the pending one.
  - It sets `replies[replyKey] = replyId` when the reply shows unread (`markReplyRead`).
  - It sets the mark to `review = false, acknowledged = fingerprint`.
- **`markNeedsAttention`:** the same match, then `review = true` (`FleetState.cpp:196-202`).
- **`unreadReplies` and `markAllRead`** (`FleetState.cpp:234-262`):
  - `unreadReplies` lists every row whose reply is unread (Qt's `unreadReply`, so a reply still in progress counts though it does not show yet) and that is not marked for review, hidden rows included.
  - `markAllRead` marks each of them whose reply is still the snapshot's, skipping an older host's row already in `replies`, and queues the reply marks. It returns the count.
- **Pending mirror:** the store keeps the pending read marks the overlay reads: loaded at start, replaced by `queue_read_marks`' answer, and a mark is dropped when a batch row shows it settled or a non-empty snapshot of its computer no longer lists the session (the same rules as `ReadMarks::settle` and `Actions::observe`). Phase 2's frontend quiet retry (`observeBatch`'s retry) is removed; the worker owns delivery.
- **Persistence:** every change saves through `update_attention_marks` (only the changed keys, after `attention_marks` has answered; a change made before that is kept and sent then) and queues read marks through `queue_read_marks`; an answer older than the latest local change is ignored. A host that starts sharing read state gets its legacy marks queued once (`legacyReadMarks`, `TrayAgent.cpp:570-572`); the computers already done are kept in the webview's local storage, as the client settings are until the QSettings migration.

- [ ] Tests (vitest, mocked bridge): each overlay branch; a pending mark for another run is ignored; a newer reply after `markRead` shows unread again; `markRead` refuses a changed fingerprint; `markAllRead` skips review rows and counts; the fallback key migrates; the cap.
- [ ] Commit `feat(client): local attention marks and read state, Qt's overlay`.

### Task B4: Mute and Unmute

**Files:** core `actions.rs` (`help_invocation`, `has_notifications_marker`, `notifications_invocation`, `classify_notifications`), the fake `hgs` replies, `client/src-tauri/src/actions.rs` (`set_notifications(target, muted, expected_revision) -> Result<Value, String>`, which runs the probe and then the call, returning the confirmed reply), `store/actions.ts`, `actions-ui.ts`, `ActionDialogs.tsx`, `SessionList.tsx` (the Mute entry).

The change runs in its own per-session lane (`Lane::Notifications`): Qt tracks pending notification changes apart from `m_pending`, so a change never blocks a write and a write never blocks it. The frontend allows one pending change per conversation and 16 in all.

Rules:
- **The call** (`HgsClient.cpp:2401-2457`):
  - A missing or invalid identity fails before any call with "The session notification identity is unavailable. Refresh the session.". Run id: non-empty, at most 128 bytes, no control chars; conversation id: at most 256 bytes.
  - First `hgs --help` on this computer, 5 s, at most 256 KiB, which must print the exact line `session-notifications-v1` (a trailing CR allowed). Otherwise "Update the configured local hgs to support shared notification preferences.".
  - Then `hgs [@host] notifications NAME --json` with `{"run_id","conversation_id","muted","expected_revision"}`; 15 s here, 35 s on a peer; at most 512 KiB.
  - It is confirmed when exit is 0 and the reply has exactly `ok, applied, run_id, conversation_id, preferences`, with `ok == true`, a boolean `applied`, matching identities and valid preferences whose `muted` is the requested value. The revision must be `expected + 1` when applied and `expected` otherwise.
  - Otherwise the error is the trimmed stderr (at most 2000 chars) or "The shared notification change was not confirmed. Refresh the session before trying again.".
- **The entry** (`SessionsWindow.cpp:5310-5323`): not for archived rows; label "Unmute" when muted, else "Mute".
  - Enabled when the computer is online, the row has shared notifications, no preferences error, valid identities, no change pending for its conversation and fewer than 16 pending.
  - Reasons, in Qt's order:
    - "Connect to this session's host to change shared notifications.";
    - "Update hgs on this session's host to support shared notification preferences.";
    - "Refresh this session's identity before changing shared notifications.";
    - "Conflicting shared notification preferences. Refresh the session.";
    - "A shared notification change is pending. Wait for its result.".
  - The enabled hint is "Allow future notifications for this session on all devices" or "Mute notifications for this session on all devices".
- **On click** (5401-5414): the same checks again, plus the run and the preferences unchanged since the menu opened; otherwise it does nothing.
- **The result** (538-555):
  - A late answer for a row that is no longer the same name, run, conversation and `created` is dropped silently.
  - A failure shows the sticky error.
  - A success applies the preferences as a confirmed override. An override with a lower revision than the row's, or with the same revision and other preferences, is refused with "The shared notification setting changed while this request was pending. Refresh the session." (sticky).
  - The override stays until the row lists a higher revision. The row's same revision with other preferences sets the conflict error (`keepNotificationPreferences`, `FleetState.cpp:103-118`).

- [ ] Tests: the help probe and call through the fake; every confirmation field; the reasons; the override rules.
- [ ] Commit `feat(client): mute and unmute through shared notification preferences`.

### Task B5: the entries, the chip and Mark all as read

Rules:
- **Session menu** (5324-5328): for a row that is not archived, "Mark as read" when it needs attention (`needsAttention` of `store/attention.ts` after phase 3a, `store/ui.ts` until then), else "Mark needs attention" with the hint "Keep a reminder to review this session later". Both are always enabled.
- **On click** (5416-5433):
  - Mark as read runs `markRead`. Refused: the sticky "A new reply or question arrived. Open the menu again to mark it as read.".
  - Mark needs attention runs `markNeedsAttention`. Refused: the sticky "This session is no longer available. Open its actions again.".
  - No success notice.
  - This replaces phase 2's direct `hgs read` call and its "Update hgs on this session's host to share read state." refusal: an older host keeps this device's marks.
- **The Mark read chip** (3777-3780) lives in Qt's composer toolbar (`ComposerToolbar::Slot::MarkRead`, 981-983), which the client builds in phase 4 (`zerus-5wmh.9`). This task adds only its pure state, `markReadChip(row)`, with tests; phase 4 places the chip. It is active for a row that is not archived and needs attention.
  - The detail is "Marked for later. Mark as read to clear this reminder." for a review mark.
  - Otherwise "Mark this session's current notification as read. This does not answer a pending question.".
  - A click runs Mark as read (1035-1040).
- **Mark all as read** (362-365, 3389-3402; `SessionsWindowRows.cpp:552-557`):
  - The label is "Mark all as read ({n})", or "Mark all as read" when `n == 0`, where `n = unreadReplies().size`. Enabled when `n > 0`.
  - The hint is "Mark current replies across all machines as read, including hidden sessions. Pending approvals, questions and errors stay highlighted. Sessions you marked as needing attention keep their reply unread."
  - A click runs `markAllRead`. A count shows "1 reply marked as read." or "{n} replies marked as read.".
  - The entry is in `batchMenu()`. A summary button exists only if the client already has one.
- **Card and lists:** the review mark draws the card's attention icon and edge and counts in Needs attention and Active, through the existing `reviewLater` field.

- [ ] Tests for the entries and the chip; one store test that `markAllRead` queues marks the worker then delivers through the fake.
- [ ] Headless smoke on the replay fixtures: mark a session for review, restart the app, and the mark is still there.
- [ ] Commit `feat(client): Mark needs attention, Mark as read and Mark all as read with local marks`.

**PR 2:** B1 to B5 and the README ("Read marks and attention marks", "Mute").

---

## Piece C: Start fresh and Forget saved session (`zerus-5wmh.17`, live part), PR 3

### Task C1: core calls

`Action` gains:

| Action | Arguments | Input | Timeout here / peer | Qt |
|---|---|---|---|---|
| `StartFresh` | `resume NAME --fresh [--expected-run-id RUN] -d` | none | 30 s / 30 s | `HgsClient.cpp:1279-1286` |
| `Forget` | `kill NAME` | none | 5 s / 15 s | `killSession`, 1227-1239 |
| `ForgetBinding` | `session-action NAME --json` | `{"request_id","action":"forget","expected_run_id","expected_conversation_id"}` | 15 s / 45 s | 2169-2245 |

- **`ForgetBinding` outcome** (scoped `runSessionAction`):
  - `TimedOut` is "Request timed out. Check the session before retrying.".
  - `WriteFailed` is "Transfer failed. Check the session before retrying.".
  - A non-zero exit gives stderr, or "Could not complete the request. Check the session before retrying.".
  - A receipt naming this request, session, run and conversation:
    - with status `failed`: its `error`, or "The session was not forgotten.";
    - with status `uncertain`: "The result was not confirmed ({error}). Refresh before trying again.", or "The result was not confirmed. Refresh before trying again." without an error.
  - Otherwise only status `completed` with that identity is done; anything else is "Invalid acknowledgement. Check the session before retrying.".
- **`StartFresh` and `Forget`** follow `runWrite`, without a worktree line.

- [ ] Tests per row of the table and every `ForgetBinding` receipt case. Commit `feat(client): start fresh, forget and OpenCode forget calls`.

### Task C2: Start fresh

Rules (`SessionsWindow.cpp:4672-4704, 5333-5337`; header `m_freshAction`, 650 and 3809):
- **Entry:** shown for a `paused` or `stopped` row as "Start fresh…", with the hint "Start with a new conversation and the same folder, account and model". Live rows keep Clear session (`zerus-5wmh.10`); archived rows show neither.
- **Enabled** (`canStartStopped`): online, `paused` or `stopped`, tracked, agent `codex`, `claude` or `kimi`, no write pending for the session and no batch running.
- **Header action:** the same rules, visible for a stopped main context.
- **Dialog:** title "Start fresh?", text "Start {name} on {machine} with a new conversation?\n\nThe agent starts in the same folder with the same account and model, without the current conversation context. Earlier messages stay readable in Activity and in the agent’s native history, but Resume will no longer continue them.". The buttons are "Start fresh" (destructive) and Cancel, which is focused and the default; Escape cancels.
- **On confirm:** the row must still be the same key, run and conversation and still pass `canStartStopped`; otherwise the sticky "The session changed. Open its actions again.".
  - Then the info notice "Starting {name} with a new conversation…" and `StartFresh` with the row's run.
  - The outcome notices are phase 2's ("Session updated. Refreshing…" or the detail).
- `LocalSessionHost::confirmLaunch` warns only on Windows; the client does not ship there yet, the same as phase 2's Resume.

- [ ] Tests for `canStartStopped`, the dialog copy and the changed-row refusal. Commit `feat(client): Start fresh`.

### Task C3: Forget saved session and the OpenCode forget

Rules (`SessionsWindow.cpp:5758-5826`):
- **The end entry** (5378-5380): phase 2 enabled only Terminate. Forget saved session is now enabled when available and `allowed("forget")`, with Qt's reason otherwise.
- **The dialog**, by case:

| Case | Title | Text | Button | Call |
|---|---|---|---|---|
| saved (not running, not archived) | "Forget saved session?" | "Remove {name} from HGS? The agent’s native conversation history is kept." | "Forget saved session" | `Forget` |
| OpenCode binding (`forgetsBindingOnly`), with a run id | "Forget saved session?" | "Forget {name} in Zerus? The conversation stays in OpenCode; only Zerus forgets it." | "Forget saved session" | `ForgetBinding` with the details' run and conversation, else the row's |
| OpenCode binding without a run id | as above | as above | as above | `Forget` |

- **The dialog's detail and buttons:** the detail is "Computer: {machine}"; Cancel is focused and the default, and Escape cancels.
- **On confirm:** whether the row is running-and-not-a-binding, and whether it is a binding, must be as when the dialog opened; otherwise the sticky "The session state changed. Open its actions again.".
  - A scoped forget also needs the same run and conversation.
- **The OpenCode result** (1003-1007): "Zerus forgot {name}. The conversation stays in OpenCode." (info), or the sticky error.
- **The Terminate variants** are unchanged.

- [ ] Tests for each row of the table and the refusals. Commit `feat(client): forget saved sessions and OpenCode bindings`.

**PR 3:** C1 to C3 and the README.

---

## Piece F: batch actions (`zerus-5wmh.20`), PR 4

### Task F1: applicability and the runner (`store/batch.ts`)

**Interfaces:**

```ts
export type SelectionAction = "compact" | "expand" | "mark_read" | "review_later" | "pause" | "resume" | "archive" | "move" | "forget" | "terminate";
export function applies(s: Session, a: SelectionAction, online: boolean, updating: boolean): boolean;   // selectionActionApplies
export function run(a: SelectionAction, rows: SelectionRow[], options?: { clearArchive?: boolean; paged?: boolean }): Promise<void>;
// SelectionRow = ActionRow plus `status` and `reviewLater` (needed for "needs attention").
// Batch items call `perform(row, action, { quiet: true })`, which suppresses the single-session notices.
export const running: () => boolean;     // Qt's m_pending while a batch runs
```

Rules (`SessionSelection.cpp`):
- **`sameInstance`** (25-29): name, archive id, `created`, run, conversation and state.
- **Applicability** (84-108):
  - Compact and expand use the row preference. Move always applies; its write is the organization stream's move function (`zerus-5wmh.18`). Until that is merged the entry is disabled with "Arrives with project organisation.".
  - Mark read applies to a row that is not archived and needs attention; review later to one that is not archived and does not.
  - Every other action needs the computer online, no write pending for the session, and `allowed(operation)`. The operation is `pause`, `restore` or `resume`, `archive`, `forget` or `terminate`.
  - Pause: running, resumable, idle and not exited.
  - Resume: resumable, not running, and when archived an archive id and the name not occupied by a live row.
  - Archive: `paused` or `stopped`.
  - Forget:
    - an OpenCode binding (not archived) needs a run and a conversation;
    - otherwise the row must not be running, and an archived row needs its archive id.
  - Terminate: running.
- **`run`** (237-319): compact and expand change the row preferences at once.
  - Rows the compact limit refused show "{n} session rows could not be compacted because the compact row limit was reached. Expand other rows first." (sticky).
  - Mark read and review later use Task B3, then "{action}: {changed} of {targets} sessions updated." (sticky when they differ).
  - Other actions need no batch running. The targets are the applicable rows; with none, nothing happens.
- **Confirmation** for Forget and Terminate (258-289):
  - Title: the action text, or "Clear archive".
  - Text: "{action} {n} selected sessions?", or the Clear archive text.
  - Terminate adds "\n\nRunning processes will stop. Tracked Codex, Claude and Kimi conversations are kept in Archive; native conversation history is kept.".
  - Forget adds:
    - with bindings among the targets: "\n\nThese saved or archived entries are removed. Other live sessions and native conversation history are kept.";
    - without: "\n\nOnly these saved or archived entries are removed. Live sessions and native conversation history are kept.";
    - with bindings: "\n\nOpenCode sessions: only Zerus forgets them; their conversations stay in OpenCode.".
  - When some selected rows do not apply: "\n\n{n} unavailable or inapplicable sessions will be left as they are.".
  - The details list "{machine} / {name}" per target.
  - Confirm: "{action} {n} sessions", or "Clear archive". Cancel is focused and the default.
  - The "Remove working folders after stopping" checkbox is `zerus-5wmh.10`, the same as phase 2's single Terminate.
- **The queue** (314-372): one session at a time. Each item is re-read and must be the same instance and still apply; otherwise it fails with "{machine} / {name}: session changed or is unavailable".
  - Before each call, the info notice "{action}: {i} / {n}…".
  - Calls: Pause is `pause`. Resume is `resume`, or `restore` when archived (piece E; "{name}: earlier restores still need a refresh" past the restore limit). Archive is `archive`.
  - Forget is `ForgetBinding` for a binding, `ForgetArchive` when archived (piece E), else `Forget`.
  - Terminate is `terminate --expected-run-id` for a tracked `codex`, `claude` or `kimi` session, else `kill`.
  - Each outcome counts as done or failed (`finishSelectionAction`, 374-404); a failure records "{operation}: {detail}". Nothing is retried.
  - The end notice is "{action}: {done} of {n} completed.", plus " {failed} failed or changed; refresh before retrying." and the first three errors when any failed (sticky then).
  - Archive and Terminate cleanup lines show phase 2's worktree notices.
- **Busy:** while a batch runs, every write entry of every session is disabled. Qt's `m_pending` is global; the phase 2 per-session `updating` stays for single actions.

- [ ] Tests (vitest, mocked bridge, fake timers): applicability per action with synthetic rows; the confirmation copy variants; a changed row mid-batch; a failed item continuing with the next; exactly one call per target; no call after the summary.
- [ ] Commit `feat(client): selection batch actions with Qt's applicability and runner`.

### Task F2: the selection bar and the selection menu

Rules (`SessionSelection.cpp:110-217`):
- **The bar:** visible when more than one row is selected, or one in bulk-selection mode. It shows "{n} selected" and a close button with the hint "Deselect all".
  - Tools in order: compact, expand, mark read, review later, pause, resume, archive, move, forget, terminate.
  - Each tool's text is "{action} ({count})" and its hint "{action} — {count} of {n} selected sessions".
  - A tool is visible when `count > 0`, and enabled when `count > 0` and it is compact or expand or no batch runs.
  - The icons are Qt's (`selectionActionIcon`), from the client icon set. Tools that do not fit go under "More actions for selected sessions".
- **Action texts:** "Mark as read", "Mark needs attention", "Compact session rows", "Expand session rows", "Pause", "Resume / restore", "Move to archive", "Forget", "Terminate", "Move to project".
- **The context menu** of a row while more than one is selected (`showSessionMenu`, 5268) is the selection menu: the section "{n} selected sessions", the applicable actions with counts (Move to project as the owner session builds it), a separator and "Deselect all".

- [ ] Tests for the bar model (counts, visibility, enablement, hints) and the menu model. Commit `feat(client): selection bar and menu`.

### Task F3: per-machine Pause all and Resume all (`batchMenu()`)

Rules (`SessionsWindow.cpp:2844-2893`, `HgsClient.cpp:1326-1343`):
- **The menu:** one submenu per computer (this computer by its host name, peers by alias), disabled while the computer is offline.
- **Counts:** live and saved rows skip archived rows. A row of `opencode` or without `bulk_lifecycle_supported` (default true except `opencode`) only adds the disabled line "OpenCode uses individual Pause / Resume controls", and its submenu actions get the hint "OpenCode sessions are excluded; use their individual Pause / Resume controls.".
- **Pause all ({live}):** enabled when online, no batch runs, `live > 0`, and every live row is resumable, idle and not exited.
  - Otherwise, with live rows, the disabled line "Some sessions are busy or context is not tracked".
  - The call is `pause --all`, with a timeout of 15 s + 30 s × clamp(live, 1, 10000); notice "Pausing sessions on {machine}…".
- **Resume all in background ({saved}):** enabled when online, no batch runs and `saved > 0`.
  - The call is `resume --all -d`, with a timeout of 15 s + 15 s × clamp(saved, 1, 10000); notice "Resuming sessions on {machine}…".
- **The rest of the menu:** Expand all collapsed, Mark all as read (Task B5) and Clear archive (piece E), in Qt's order.
- **Outcomes:** `runWrite` with phase 2's notices; one of these runs at a time (Qt's `m_pending`).

- [ ] Tests for the submenu model and the two calls. Commit `feat(client): pause all and resume all per machine`.

**PR 4:** F1 to F3 and the README ("Batches").

---

## Piece D: the Archive view (`zerus-5wmh.32`), PR 5

### Task D1: the page read (`reads.rs`)

`Read::ArchivePage { from: f64, until: f64, cursor: String, query: String }`, per computer:
- `hgs [@host] archives --json`, with `{"from","until","limit":50,"cursor","query"}` on standard input; 30 s; at most `READ_LIMIT`.
- **Checks** (`HgsClient.cpp:563-575`). Any failure gives "Invalid archive page. Update the computer CLI and refresh.":
  - parses;
  - `version == 1`;
  - `sessions` is an array of at most 50;
  - `next_cursor` is a string of at most 1024 chars;
  - `total` is a number not below the row count;
  - `from`, `until` and `query` echo the query;
  - no `next_cursor` without rows;
  - `next_cursor` differs from a non-empty query cursor.
- **The window:** with `previous_cursor` or `position`, the former must be a string of at most 1024 chars and the latter a whole number from 0 with `position + rows <= total`; else "Invalid archive window. Refresh the archive.".
- **Each row:** `state == "archived"`, a non-empty name, `archive_id` a lowercase hyphenated UUID, and `archived_at` a number in `[from, until)`; else "Invalid archived session identity. Refresh the archive.".
- **The reply:** rows projected with `view::row` (with `archived_at`), plus `next_cursor`, `previous_cursor`, `total` and `position`.
- **Cancellation:** a newer request for the same computer cancels the older one; its answer is dropped (`cancelArchivePages`).
- **Cancel all:** leaving the filter, a hosts change, a query change and Refresh cancel every archive read (`cancel_archive_pages`, Qt `cancelArchivePages`).

- [ ] Tests for each check through `decode` and one fake `hgs` page. Commit `feat(client): archive page read`.

### Task D2: archive store and bar (`store/archive.ts`, `ArchiveBar.tsx`)

Rules (`ArchiveBrowser.cpp`, `ArchiveDatePicker.cpp`):
- **Pages:** one page per computer (rows, cursor, previous, error, total, position, loaded).
  - Refresh drops the pages and loads the first. Older loads `cursor`; Newer loads `previous`.
  - A page that comes back empty while rows were shown is the error "Archive changed. Refresh to start a new view.".
  - Pages load only while the Archive filter shows the list.
  - A query change cancels pages and reloads after 300 ms.
- **Date range:** the picker's periods and custom range. The default is All dates (`m_period = 0`); the custom range starts at the last 30 days (`m_first = today - 29`). `from >= until` loads nothing.
- **Status:**
  - "Loading archive…" while loading;
  - one computer with a window: "{a}–{b} of {total}", or "{shown} shown of {total}";
  - otherwise "{shown} loaded of {total}" once every total is known, else "{shown} loaded".
  - The per-computer windows go in the hint.
  - Errors as "{machine}: {error}", at most 240 chars shown.
- **Buttons:** "Older" and "Newer" lose their text below 310 px; "Refresh archive".
- **The filter count** shows "…" until totals are known, and "+" while more pages remain (`SessionsWindowRows.cpp:563-565`).
- **Rows:** archived rows sort by `archived_at`, newest first. They enter the list only under the Archive filter, through `model.ts`'s row source. That is one function; agree its shape with the owner session before the change.

- [ ] Tests for the store with a mocked bridge (paging, errors, debounce, cancellation, totals) and the status text. Commit `feat(client): Archive view`.

### Task D3: archived details

Selecting an archived row reads its details with `inspect --archive ID` (phase 2's `details_invocation` already takes it). Rename keeps `--archive` (phase 2).

- [ ] Test that an archived selection inspects with its archive id. Commit with D2.

**PR 5:** D1 to D3 and the README ("Archive").

---

## Piece E: Restore, Forget archived session and Clear archive (`zerus-5wmh.17`, archived part), PR 6

### Task E1: core calls

| Action | Arguments | Timeout here / peer | Qt |
|---|---|---|---|
| `Restore { archive_id }` | `resume NAME --archive ID -d` | 30 s / 30 s | `HgsClient.cpp:1303-1313` |
| `ForgetArchive { archive_id }` | `kill NAME --archive ID` | 5 s / 15 s | 1315-1324 |

An empty archive id fails before any call with "Missing archive identity". The outcomes follow `runWrite`.

- [ ] Tests. Commit `feat(client): restore and forget archived calls`.

### Task E2: the entries and the follow-up

Rules:
- **Restore session** (5302-5304, 5360-5361, 5542-5560):
  - The change entry of an archived row is enabled when available, `resumable`, `archiveId` is set, no live row on the same computer has the name (`archiveNameOccupied`). It does not read `allowed("restore")`: Qt's header (`m_changeAction`, 3897-3898) does not, and the client's right-click selects the row first, so its context menu is Qt's selected-row case, which reuses `m_changeAction` (5360). The batch's Resume / restore still needs `allowed("restore")` (`SessionSelection.cpp` 94-96).
  - A restore of the same session already pending, another single restore still running (`m_restoreWriteKey`), or 64 restores pending, gives the sticky "Finish or refresh the earlier restores before restoring another session.". In a batch, a second instance of a name restored earlier in the run meets the same limit ("{name}: earlier restores still need a refresh", `SessionSelection.cpp` 336-338), or "session changed or is unavailable" once the catalog lists its live row. A restore stays pending until its live row appears running or its call fails. Qt's limit also counts `m_restoredInspections`, which the client does not have (see the last rule below).
- **After a successful restore** (1958-1969, 3281-3336):
  - The archive refreshes (Forget keeps its refresh too). A successful rename or Move to archive also refreshes the archive (`SessionsWindow.cpp:1917`).
  - The batch's Compact/Expand and E's archived calls find loaded archived rows (the store's `archive.row(key)`), not only catalog rows.
  - When the restored row is still selected under the Archive filter, the filter becomes All; other scopes stay.
  - While its restore is pending, the archived row stays the selected row after the archive reloads (Qt's `selected()`, 3136-3139).
  - When the live row of that name appears with the restored conversation, the selection follows it; a live row without a conversation is followed too (3322-3323).
  - When it appears with another conversation, the selection clears with the sticky "The restored conversation could not be verified." Qt adds "Your archived draft remains in Saved drafts."; the client has no saved drafts before phase 4 (`zerus-5wmh.9`), which restores the full text.
  - Only the single restore of the selected row is followed (`m_restoreKey`). A batch, a state filter or search change, a Rename or a Forget archived session cancels the follow; then a verified live row only clears a selection still on the archived row (3297-3305).
  - Phase 2's details matching on run and conversation already keeps old-run details from enabling controls (Qt's `awaitingRestoredInspection`).
- **Forget archived session** (5378-5380, 5771-5774):
  - The end entry is enabled when available, `allowed("forget")` and `archiveId` is set.
  - The dialog title is "Forget archived session?", the text "Remove this archived instance of {name} from HGS? Other sessions with this name and the agent’s native conversation history are kept.", the button "Forget archived session", and the detail "Computer: {machine}".
  - The call is `ForgetArchive`; success refreshes the archive.
- **Clear archive** (`SessionSelection.cpp:171-181, 226-235`):
  - The label is "Remove loaded archived sessions…", since the client's archive is always paged.
  - It is enabled when no batch runs and some loaded archived row in the machine and project filters can be forgotten.
  - It runs the Forget batch (Task F1) with `clearArchive`. The title and confirm button are "Clear archive", and the text "Remove {n} loaded archived sessions in the current date and machine scope? Unloaded entries are kept." plus the Forget suffixes.
  - When the batch ends, the archive refreshes (1916).

- [ ] Tests for the enablement (occupied name, missing archive id), the restore limit, the follow-up cases and the Clear archive scope. Commit `feat(client): restore, forget archived sessions and clear archive`.

**PR 6:** E1 to E2 and the README.

---

## Order and handoff

| PR | Piece | Beads | Depends on |
|---|---|---|---|
| 1 | A: copy actions | `zerus-5wmh.19` | main |
| 2 | B: attention marks, Mute, Mark all as read | `zerus-5wmh.16` | PR 1 for `view.rs` and `details.rs` field additions |
| 3 | C: Start fresh, Forget saved session | `zerus-5wmh.17` | PR 1 |
| 4 | F: batch actions, Pause all and Resume all | `zerus-5wmh.20` | PRs 2 and 3 |
| 5 | D: Archive view | `zerus-5wmh.32` | PR 1; phase 3a's `model.ts` state if it landed |
| 6 | E: Restore, Forget archived, Clear archive | `zerus-5wmh.17` | PRs 4 and 5 |

Each task gets a spec review against the Qt lines quoted for it, then a quality review. The last PR gets a whole-branch review. After each merge:
- the merge commit goes to the owner session, which rebuilds the Mac dev app;
- the design session hears that the piece is ready for the owner to try.

## Owner rulings and deviations from Qt

Ruled by the design session on 2026-10-28:
1. The restore follow-up shows only "The restored conversation could not be verified." until the client has Saved drafts; `zerus-5wmh.9` restores the second sentence (Task E2).
2. Copy session name shows no notice, as Qt; phase 1's "Copied" toast goes (Task A3).
3. Batch Move to project belongs to the organization stream (`zerus-5wmh.18`, branch `work-e94411a3-org`). The batch runner offers the entry, but the write calls that stream's move function with its swarm apply rules; the organization is never written separately. Until that piece is merged, batch Move stays disabled with "Arrives with project organisation." (Task F1).

Controller rulings during Piece A:
- The browser build (no Tauri) copies through `navigator.clipboard` with the same two checks and no notice on success.
- The details header's Copy connect command stays disabled: it is Qt's `m_openAction` (Open terminal family, phase 4, `zerus-5wmh.9`), not the context menu's copy entry.
- Task A4 added the README part and the smoke steps for PR 1.
- "Session reference copied" appears only after the clipboard accepted the text; Qt shows it unconditionally and reports a copy failure separately.
- The click-time recheck compares the conversation id; `created` joins it when Piece B adds it to the row.

Controller rulings during Piece B:
- Attention marks are saved as deltas (`update_attention_marks`) by a background writer; `attention_marks` returns each map's write order.
- Read marks for computers no longer configured are dropped once the local listing names its peers, as Qt's `publishReadMarks`.
- Mark all as read is also wired to the existing list footer button and the rail menu, as Qt has both.
- The Mark read chip waits for phase 4's composer toolbar; only its state exists now.
- The muted override guards only this client's own confirmed changes; Qt also guards every row against stale polls.
- A same-revision change of other notification preferences is compared on the revision and `muted` only; the row carries no preferences digest.
- The overlay ignores Qt's offline case of `visibleUnreadReply(online)`: an offline row's reply in progress does not show unread.
- The attention and reply keys use the view agent (dsh keyed as `deepseek`) rather than Qt's raw `cmd`.
- `set_notifications` answering "busy" (its lane is taken) is shown as the pending reason "A shared notification change is pending. Wait for its result."

Controller rulings during Piece C:
- Menu-launched Start fresh and Forget saved session run the click-time pre-check before their dialogs, as the copy entries do.
- The header's Start fresh and Clear session follow Qt's main-context rule: neither shows while a subagent is open.
- Forget archived session stays disabled until the Archive view (`zerus-5wmh.32`) (lifted by Piece E).
- Task C4 added the README part and the smoke steps for PR 3.

Controller rulings during Piece F:
- The batch-running flag disables every write entry (Qt's m_pending).
- Archived Resume and Forget are not counted until Piece E binds its hooks (lifted by Piece E).
- Move to project is disabled with "Arrives with project organisation." until the organization stream's move lands.
- Pause all and Resume all are computer-wide calls on their own write claim.
- The client shows machines as headings, since its menus have no submenus.
- The selection bar needs more than one selected row, since the client has no single-row bulk mode.
- The batch Terminate omits Qt's worktree checkbox and "Stop {n} sessions" labels until `zerus-5wmh.10`.
- Task F4 added the README part and the smoke steps for PR 4.

Controller rulings during Piece D:
- Pages are kept on leaving the Archive filter and the default range is All dates, both as Qt (the first draft of D2 said otherwise).
- Archived rows are flat cards until `zerus-5wmh.40` (Qt groups them by project).
- The archived Activity pane waits for `zerus-5wmh.42`.
- The paging-computer scan reads catalog rows while some live computer has no rows (Qt rebuilds its hosts on every refresh).
- Task D4 added the README part and the smoke steps for PR 5.
- Archive reads carry the store's request number, counted from the app's start so a reload never reuses one: a read at or below the cancel mark, or older than its computer's current read, starts cancelled and never cancels a newer one (Qt's synchronous `requestArchivePage` and `cancelArchivePages`).
- The archive read's failure texts are Qt's `failureDetail` ("timed out", "cannot start '{hgs}': {error}", the trimmed standard error up to 4000 characters, or "exit code {n}"); other actions keep the client's shared texts.
- The date panel renders through a Portal, since a fixed element inside the transformed strip overlay was misplaced. As Qt's popup, it keeps a hover-opened peek open (`SessionPanelDock.h:50-56`), Escape closes it first and Tab wraps inside it.
- Archived rows show neither Open terminal nor the Mute and Mark group, as Qt's `showSessionMenu`.
- Parked: numbering from the clock assumes it does not step back across a reload; a child ended by a signal with no standard error reads "hgs ended abnormally" where Qt shows "exit code {n}".

Deviations from Qt, both from owner rules:
- Attention marks and read replies are capped at 20 000 entries per map (bounded caches); Qt keeps them without a bound.
- "Deselect all (Esc)" is "Deselect all" (no obvious keyboard hints).

Deviations from Qt after Piece D:
- The date inputs show the browser's empty date field instead of "Any date" (`ArchiveDatePicker.cpp`); the hint says "All archived dates (no date limits)".
- An empty Archive list, loading or not, shows the client's "No matching sessions" card with its "Show all sessions" button. Qt shows an empty list and says "No matching sessions" only in the footer count (`SessionsWindowRows.cpp:473`). With no live row at all the client footer is empty, where Qt's reads the same text. Both sit in `SessionList.tsx` outside the agreed ArchiveBar line, so they stay.

Controller rulings during Piece E:
- The archive reloads once after a write batch ends (Clear archive included). Qt's own path (`SessionsWindow.cpp:1916`) never runs, because its deferred step leaves the selection running, so this is kept as a deviation.
- Restore follows Qt without `allowed("restore")` in the header (`SessionsWindow.cpp:3897-3898`) and in the context menu, since every client context menu is Qt's selectedTarget case (5360). A batch's Resume / restore still reads it (`SessionSelection.cpp:94-96`).
- A second archived instance of the same name in one batch reaches the restore limit: "earlier restores still need a refresh" (`SessionSelection.cpp:336-338`).
- A thrown restore shows the sticky error notice with the error text, as a failed outcome does.
- Task E3 added the README part and the smoke steps for PR 6.

Deviations from Qt after Piece E:
- "Session restored. Refreshing its current state…" and Qt's restored-inspection set (`m_restoredInspections`) in the 64 limit are not ported; the control gating rests on the details matching run and conversation.
- "Updating session on {machine}…" is absent, as for every client change entry.
- Qt's local launch question before a restore (`confirmLocalStart`) has no client counterpart, as for Resume.
- The header's Forget archived on a retained archived source (after the archive reloaded) refuses at click time with the client's pre-check text, "This session is no longer available. Open its actions again."; Qt opens the dialog and refuses at confirmation with "…Refresh the list and open its actions again."
- The restore follow is cancelled by the state filter, the search, a running batch, Rename and Forget archived; other single writes (pause, resume, Move to archive, terminate, start fresh) do not cancel it, as they act on another selected row. Qt clears it in each of them (Move to archive: `archiveSession`, `SessionsWindow.cpp:5575`).
- Clicking the state filter that is already active does not cancel the follow (only a change of the filter does); Qt cancels on every filter-button click (`SessionsWindow.cpp:409`). The filter buttons are the owner session's `SessionList`, outside this piece.
- The restore's conversation is the row's conversation id; Qt prefers `m_details`, which the client only returns when it matches the row's conversation.
- A live row of a restored name that appears during a batch by another path is seen by the batch's occupied check only through a tracked restore (`taken`); otherwise `hgs` decides and the item counts as failed.
