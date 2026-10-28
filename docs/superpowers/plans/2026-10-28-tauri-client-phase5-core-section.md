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
- [ ] The project-worktree recognition rule (verified catalog containing both
  the folder and a saved project folder on that machine; warnings for unrelated
  folders).

## Task W1 (core): worktree catalog and creation

**Files:** `client/core/src/worktrees.rs` (new).

- [ ] `hgs [@host] worktrees --path P --json [--refresh]`, 6 s here and 15 s on
  a peer, replies at most 2 MiB; the verified catalog for creation
  (`--creation --path P --refresh --json`, 15 s, expected machine) with Qt's
  "Invalid verified worktree catalog." and "Invalid worktree catalog response".
- [ ] The 30 s catalog cache with in-flight sharing and generation-based
  invalidation (a removed worktree invalidates its repository; a host change
  invalidates the host), as `requestWorktrees` keeps it.
- [ ] `worktrees create --path --common-dir --branch --start ... ` with its
  receipt check and Qt's failure text ("Check {destination} before retrying.
  Any created worktree is kept.").

## Task W2 (core): worktree review and Settings > Worktrees

**Files:** `client/core/src/worktrees.rs`.

- [ ] The review model of `WorktreeReviewUi.h`, `WorktreeCleanupDialog.h`,
  `WorktreeBatchCleanupDialog.h` and `WorktreeCleanupStatus.h`: explicit
  actions only, removed/kept/unknown outcomes, and manual review for anything
  uncertain.
- [ ] Settings > Worktrees (`WorktreeSettings.h`): the per-computer policy is
  stored by `hgs`; an incomplete or malformed policy fails closed.

## Task W3 (core): Remove worktree after Terminate

**Files:** `client/core/src/worktrees.rs`, `client/core/src/actions.rs` (an
option on Terminate).

- [ ] Phase 2 parses and shows the automatic `ZERUS_WORKTREE_CLEANUP` outcome.
  Phase 5 adds the explicit Remove worktree choice: removal only after its
  confirmation and the exact successful termination, bound to the checkout
  ownership recorded at launch and its nonce; background cleanup reporting
  removed, kept and unknown, with review actions; an unknown outcome is never
  replayed, and only a verified refusal before deletion may be checked again.
  Follow the rules in `AGENTS.md` word for word.

## Task F1 (core): Fork session

- [ ] `hgs [@host] fork NAME -n TAG -d [--archive A] [--expected-run-id R]
  [--expected-conversation-id C]` as an `Action::Fork`, with the tag rule of
  `tag_problem` and Qt's dialog reasons.

## Task C1 (core): Clear session and Compact context

- [ ] `Action::ClearContext {dismiss_prompt}` and `Action::CompactContext` as
  `clear-context` and `compact-context` session actions
  (`{expected_run_id, expected_conversation_id, dismiss_prompt?}`), with the
  receipts `runSessionAction` checks (`confirmed` or `submitted` for clear).
  `dismiss_prompt` is sent only when set, since an older hgs rejects the field.
- [ ] `clearable(details)` (`clearableContext`) and the reasons from S1's
  `context_action_reason`.
- [ ] Compact-to-send (S3, phase 4) uses the same `CompactContext` action.

## Order and collisions

K1, N1, W1, F1 and C1 are new modules or additive `Action` variants; K2, W2
and W3 build on them. C1's `CompactContext` is needed by S3 in phase 4, so it
may move earlier. Each task is one PR, its surface reviewed by the web client
session and merged on a green CI gate. Beads children are filed when this
section is folded into the phase 5 plan.
