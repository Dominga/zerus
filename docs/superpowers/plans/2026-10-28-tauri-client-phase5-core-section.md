# Tauri Client Phase 5: Rust and Core Section (Tasks, New Session, Worktrees, Fork, Clear)

> **For agentic workers:** this is a section for the web client session's phase 5
> plan (`zerus-5wmh.10`). That session merges it into its own plan; the events
> session implements the tasks marked **core**. It is a draft from the Qt
> sources: each task names its Qt reference, and the implementing PR ports the
> exact rules and texts with tests, as the phase 4 tasks did. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Rust side of phase 5, with Qt's exact behaviour:
- Tasks: the writes and the rules behind the board and the inspector (the
  reads already come from the `tasks` topic, `zerus-5wmh.22`);
- New session: the launch, folder checks, cloning and the generated worktree;
- worktrees: the catalog, creation, the cleanup review, and the Remove
  worktree choice after Terminate;
- Fork session, Clear session and Compact context with their inspect-based
  reasons (the owner's phase 5 comment of 2026-10-28).

The webview renders. `client/core` owns the `hgs` calls, limits and outcome
rules, all pure Rust without Tauri; `client/src-tauri` exposes them as
commands through the one dispatcher (`actions::dispatch`) and its lanes.

**Behavioural reference (Qt):**
- Tasks: `tray/src/HgsClient.cpp` (`requestTask`, `taskTimeoutMs`, about
  720-756); `tray/src/TasksView.cpp` (the step runner `run`, `targetArguments`,
  moves, stages, rename, comments, about 400-800); `tray/src/TaskBoard.cpp`,
  `TaskRow.cpp`, `TaskStageFilter.h`, `TaskDrag.h`, `TaskContent.h`,
  `TaskInspector.cpp`, `TaskDialogs.cpp`, `TaskMessageOutbox.h`; and the task
  launch in `tray/src/SessionsWindow.cpp` (about 2982).
- New session: `tray/src/NewSessionDialog.cpp` (folder checks, cloning, the
  generated branch `work-<8 hex>` and worktree path, the launch, about
  150-1313), `tray/src/LocalLaunchWarning.h`, and `HgsClient::launchDetachedSession`
  and `launchNativeSession` (about 1131-1150).
- Worktrees: `HgsClient::requestWorktrees` and `createWorktree` (about
  1412-1600), `tray/src/WorktreesDialog.cpp`, `WorktreePanel.h`,
  `WorktreeInspectionPanel.h`, `NewWorktreeDialog.h`, `WorktreeReviewUi.h`,
  `WorktreeCleanupDialog.h`, `WorktreeBatchCleanupDialog.h`,
  `WorktreeCleanupStatus.h`, `WorktreeSettings.h`; `docs/reference.md`
  (worktree cleanup section); and `AGENTS.md`/`CLAUDE.md` (worktree rules).
- Fork, clear, compact: `HgsClient::forkSession` (about 1209),
  `requestClearContext` and `requestCompactContext` (about 2138-2150), and
  `SessionsWindow` (`clearContext`, `clearableContext`, `contextActionReason`,
  the fork dialog).

## Global constraints

- Unknown outcomes are never replayed. Every write keeps its request identity,
  expected run and conversation, and the expected machine
  (`guardMachine`), with Qt's texts.
- Inspecting or opening a dialog never creates a worktree; the generated
  worktree is created only after Start session. Ordinary non-Git folders stay
  supported; Git is never initialized automatically.
- Destructive actions need an explicit confirmation in the view; the core
  refuses any removal not bound to the exact ownership and identity recorded
  for it.
- Bounded everything: task replies at most 36 MiB (Qt's limit), worktree
  catalogs at most 2 MiB, paged task lists from the `tasks` topic.
- All calls run off the UI thread. Tests use fake `hgs` scripts and synthetic
  fixtures only.

## Task K1 (core): task writes

**Files:** `client/core/src/tasks_write.rs` (new), `client/core/tests/tasks_write.rs`.

- [ ] `TaskCall {args, input}` for `hgs task ...` on this computer, with Qt's
  limits:
  - 90 s for `send`, `cancel` and `message`, otherwise 30 s;
  - output at most 36 MiB ("Task response exceeds 36 MiB");
  - standard input always given and closed, even empty, so a command that
    reads a text never waits;
  - errors: "hgs task timed out", "cannot start '{hgs}': {error}", stderr (at
    most 4000 chars) or "exit code {n}", and "Invalid task response".
- [ ] The subcommands the views use, each as a typed builder with
  `--project`: `add` (with `--target`), `get`, `move` (`--top`, `--before`,
  `--after`), `stage` (backlog, todo, done), `rename` (title on stdin), `edit`
  (`--content-json`), `comment` (text, or `--content-json` with files),
  `target` (a session, `any` or `new-session`, with its mode), `send`,
  `cancel` and `review`.
- [ ] Qt's step runner: a list of calls run in order, stopping at the first
  failure, with the reply of each step available to the next.
- [ ] The message outbox (`TaskMessageOutbox.h`): a private recovery snapshot
  per task before a send, so an interrupted send is recovered or reported,
  never repeated. It lives beside the drafts root (S5), with Qt's texts.

## Task K2 (core): task board and inspector rules

**Files:** `client/core/src/task_view.rs` (new).

- [ ] Stages and their filter (`TaskStageFilter.h`), `canStageTask`, drag
  anchors for moves (`TaskDrag.h`), row labels and states (`TaskRow.cpp`), and
  the content model (`TaskContent.h`).
- [ ] The inspector's reads and edits: `get` with comments, the description
  and comment content JSON with attachments (A1's staging), and the target
  choice, as `TaskInspector.cpp` and `TaskDialogs.cpp` do.
- [ ] The task launch from a session (`SessionsWindow`, about 2982) and its
  receipt.

## Task N1 (core): New session

**Files:** `client/core/src/launch.rs` (new).

- [ ] `launch(host, agent, target, name, account?, launch_id, model?, effort?,
  expected_machine)`: `hgs [@host] AGENT TARGET --new -n NAME --launch-id ID`,
  `-d` except for `dsh` and `opencode`, `--account`, and for OpenCode `--model`
  with an optional `--effort`; 90 s; the expected-machine guard.
- [ ] The native launch (`launchNativeSession`) and the local Windows-session
  warning (`LocalLaunchWarning.h`), as a pure rule the view asks before a local
  launch.
- [ ] The folder check, cloning ("Clone and start"), using an existing
  checkout, and the generated branch and worktree path, ported from
  `NewSessionDialog.cpp` as a pure form model: what Start does, its button text
  ("Start session", "Clone and start", "Use checkout and start", "Creating
  worktree…", "Checking folder…", "Cloning…", "Checking…") and its reasons.
- [ ] The launch options `AGENTS.md` keeps, with Qt's texts:
  - the direct-folder launch, "Start directly in the working folder."
    (`NewSessionDialog.cpp` 207-208);
  - the explicit Worktree options, custom or existing (210-216);
  - "This folder is outside the project.\nStarting here will add it to this
    project." (683);
  - "New branch: {branch}\nStart from: HEAD" (704);
  - "Project changed. Select the checkout again before starting." (737);
  - "This computer is unavailable. Choose another folder or reconnect it."
    (719).
- [ ] Opening the dialog or inspecting the form never creates a worktree; the
  generated worktree is created only after Start session, and only for a
  verified Git folder. Ordinary non-Git folders are supported, and the client
  never runs `git init`. The core tests pin each of these.
- [ ] The project-worktree recognition rule (verified catalog containing both
  the folder and a saved project folder on that machine; warnings for unrelated
  folders).

## Task W1 (core): worktree catalog and creation

**Files:** `client/core/src/worktrees.rs` (new).

- [ ] `hgs [@host] worktrees --path P --json [--refresh]`, 6 s here and 15 s on
  a peer, replies at most 2 MiB; the verified catalog for creation
  (`--creation --path P --refresh --json`, 15 s, expected machine) with Qt's
  "Invalid verified worktree catalog." and "Invalid worktree catalog response".
- [ ] The catalog cache, as `requestWorktrees` keeps it:
  - a snapshot is fresh while `sampled_at` is under 30 s old and not `stale`;
  - in-flight reads of the same folder share one call;
  - generations invalidate it: a removed or created worktree invalidates its
    repository, a host change the host; a generation mismatch reports
    "Folder changed. Refresh its catalog.";
  - snapshots persist across restarts (Qt's `workspace/worktreeSnapshots`):
    at most 64 entries and 8 MiB, the oldest `sampled_at` evicted first, keyed
    with the hgs executable. The client keeps them in its own private settings
    file, never Qt's.
- [ ] Creation, `HgsClient::createWorktree` (about 1565):
  `hgs [@host] worktrees create --path P --common-dir C --branch B --base BASE
  --destination D --request-id ID --json [--receipt]`, 330 s, with the
  expected-machine guard.
  - Success needs the reply's `request_id` to equal the token, `status`
    `created`, the same `branch` and `common_dir`, and an absolute `path`.
  - Texts: "Worktree creation is not confirmed. Check its original result."
    (receipt mode, the reply's `detail` first); "Creation could not be
    confirmed. Refresh the catalog and check the destination before
    retrying."; on failure, the error then "\nCheck {destination} before
    retrying. Any created worktree is kept.".
  - The cache is invalidated on both outcomes.
- [ ] The receipt read, `worktreeReceipt` (about 1592): `hgs [@host] worktrees
  ACTION --request-id ID --json`, 15 s, valid only with `schema` 1, the same
  `request_id` and a string `status`; otherwise "Invalid worktree receipt.".
- [ ] An older hgs that prints the catalog usage for review or remove gives
  "Update Zerus on this computer to clean up worktrees." or "Update Zerus on
  {host} to clean up worktrees." (`cleanupError`).

## Task W2 (core): worktree review and Settings > Worktrees

**Files:** `client/core/src/worktrees.rs`.

- [ ] The review model of `WorktreeReviewUi.h`, `WorktreeCleanupDialog.h`,
  `WorktreeBatchCleanupDialog.h` and `WorktreeCleanupStatus.h`: explicit
  actions only, removed/kept/unknown outcomes, and manual review for anything
  uncertain.
- [ ] Settings > Worktrees (`WorktreeSettings.h`): the per-computer policy is
  stored by `hgs`; an incomplete or malformed policy fails closed and never
  re-enables default cleanup. Age and naming never authorize removal;
  branches and ignored data are kept; queue reads are bounded and indexed.
- [ ] The manual acceptance of unreadable process paths: it lasts five minutes,
  applies to the exact checkout review only and tolerates unrelated process
  churn; fresh known-use, session, ownership and file protections still apply,
  and automatic cleanup never uses it (`AGENTS.md`).

## Task W3 (core): Remove worktree after Terminate

**Files:** `client/core/src/worktrees.rs`, `client/core/src/actions.rs` (the
removal call).

Phase 2 parses and shows the automatic `ZERUS_WORKTREE_CLEANUP` outcome of
Archive and Terminate. Phase 5 adds the explicit choice, from
`SessionsWindow.cpp` (about 5778-5840) and `HgsClient::removeWorktree` (about
1742):

- [ ] `removal_offered(row, inspection)`: the checkbox appears only when the
  session is live, keeps its archive (a tracked codex, claude or kimi
  session), and has a linked worktree (the row's `git_worktree`, or
  `git_worktree` in an inspection of the exact run).
- [ ] The texts the dialog shows: the label "Remove the working folder after
  stopping"; the tooltip "Cleanup runs in the background. Unsaved work,
  unknown local files and folders in use are kept for review."; the
  informative text "Computer: {machine}\nThe conversation and branch are kept.
  Folders that need review stay on disk."; the confirm button "Stop and remove
  folder" or "Stop and archive" with the checkbox; Cancel is the default.
- [ ] The preference `remove_worktree_after_termination` defaults to off and
  persists (Qt's `workspace/removeWorktreeAfterTermination`).
- [ ] After the dialog the session is checked again: "The session state
  changed. Open its actions again.".
- [ ] The removal is bound to the inspection of the exact run:
  `worktree_owner_id`, the run, and `git_root` or `git_common_dir`. It runs
  only after the termination succeeded exactly, in the background after the
  original confirmation.
- [ ] The call: `hgs [@host] worktrees remove --path P --common-dir C
  --fingerprint F --request-id ID --json [--delete-branch]
  [--ignored-fingerprint I] [--discard-fingerprint D] [--process-fingerprint
  R] [--preserve-archive-restore]`, 330 s, output at most 64 KiB. Its identity
  is the request ID and path; `removed` or `forgotten` succeed.
- [ ] Outcomes are removed, kept or unknown, each with review actions. An
  unknown outcome is never replayed; only a verified refusal before deletion
  may be checked again. Missing or replaced ownership, unknown local data,
  source changes, other occupants and uncertain process use go to manual
  review. The core tests pin each rule of `AGENTS.md`.
- [ ] The multi-selection Terminate (`SessionSelection.cpp` 257-309) offers the
  same option. It belongs to the session-menu parity batch task
  (`zerus-5wmh.20`); the core gives it the same rules and call.

## Task F1 (core): Fork session

- [ ] `hgs [@host] fork NAME -n TAG -d [--archive A] [--expected-run-id R]
  [--expected-conversation-id C]` as an `Action::Fork`, 60 s here and 90 s on
  a peer (`runWrite`), with the tag rule of `tag_problem` and Qt's dialog
  reasons.

## Task C1 (core): Clear session and Compact context

- [ ] `Action::ClearContext {dismiss_prompt}` and `Action::CompactContext` as
  `clear-context` and `compact-context` session actions
  (`{expected_run_id, expected_conversation_id, dismiss_prompt?}`), with the
  receipts `runSessionAction` checks (`confirmed` or `submitted` for clear).
  `dismiss_prompt` is sent only when set, since an older hgs rejects the field.
- [ ] `clearable(details)` (`clearableContext`) and the reasons from S1's
  `context_action_reason`.
- [ ] Compact-to-send (S3, phase 4) uses the same `CompactContext` action.

## Views and the session menu

The views are the web client's: the Tasks board and inspector, the New session
dialog, the Projects > Worktrees panel, the Settings > Worktrees page, the
Terminate dialog's Remove worktree choice and its outcome review, the Fork
dialog, and the Clear and Compact dialogs (built on S2's `ColdPrompt`). Fork,
Clear and Compact also appear in the session menu: whoever builds those views
checks the session-menu parity plan (`a56256ac`) so nothing is built twice.

## Order and collisions

K1, N1, W1, F1 and C1 are new modules or additive `Action` variants; K2, W2
and W3 build on them. C1's `CompactContext` is needed by S3 in phase 4, so it
may move earlier. Each task is one PR, its surface reviewed by the web client
session and merged on a green CI gate. Beads children: `zerus-5wmh.10.1` K1, `.10.2` K2, `.10.3` N1,
`.10.4` W1, `.10.5` W2, `.10.6` W3, `.10.7` F1; C1 moved to phase 4 as
`zerus-5wmh.9.18`.
