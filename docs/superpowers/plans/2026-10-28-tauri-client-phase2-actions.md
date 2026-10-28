# Tauri Client Phase 2: Guarded Session Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client interrupts a turn, marks a session read, pauses, resumes, renames, archives and terminates sessions. It uses the same guarded `hgs` calls, identities, enablement rules, confirmations, notices and outcome rules as the Qt GUI (Beads `zerus-5wmh.7`).

**Architecture:**
- **Pure action layer in `client/core`:**
  - `actions.rs` builds the exact `hgs` argument list, standard input and timeout for each action. It classifies the exit with Qt's receipt rules into done or failed, including the automatic worktree cleanup line of archive and terminate.
  - `run.rs` runs one invocation with a deadline.
  - `details.rs` reads the selected session's `hgs inspect` fields that the enablement rules need.
  - `readmarks.rs` keeps shared read marks durable until the host's snapshot shows them.
- **`client/src-tauri`:** exposes `run_action` and `inspect_session`, and refuses a second action on a session while one is in flight.
- **SolidJS frontend:** enables the session menu entries phase 1 left disabled, following Qt's exact enablement, labels and confirmations.

Unknown outcomes are shown and never replayed.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` (section "Actions"); phase table in `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`. The behavioural reference is the Qt GUI:
- `tray/src/HgsClient.cpp`: `runSessionAction`, `requestRead`, `runWrite`, `requestInspection`, `terminateSession`, `pauseSession`, `resumeSession`, `archiveSession`, `renameSession`.
- `tray/src/NativeSessionCapabilities.h`.
- `tray/src/ReadMarks.h`.
- `tray/src/SessionsWindow.cpp`:
  - the header actions, around lines 3890-3912;
  - the session context menu, around 5270-5380;
  - the write notices, around 1915-1994;
  - the terminate confirmation, around 5760-5800 (object name `terminateSessionConfirm`).

Read those lines before each task; this plan quotes the rules it depends on.

## Global Constraints

- "Actions keep today's guarded CLI calls and their rules: request identities, expected run and conversation, destructive confirmations with the safe choice focused, and **unknown outcomes are never replayed automatically**. The client adds no new action semantics." (spec, Actions)
- Rust toolchain 1.99.1 (`cargo +1.99.1`, `rust-version = "1.99"`); `client/core` stays pure Rust without Tauri.
- The new UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); `scripts/check-ui-separators.py` enforces it.
- No obvious keyboard hints in the UI (owner rule, round 6).
- **Notices are Qt's texts:**
  - After a successful write: "Session renamed. Refreshing…" for rename, and "Session updated. Refreshing…" for pause, resume, archive and terminate (`SessionsWindow.cpp:1971`).
  - On failure: the detail `hgs` printed.
  - Worktree cleanup: "Worktree removed automatically. The branch and archived conversation are kept." and "Worktree kept for review: {reason}" (`SessionsWindow.cpp:1987-1992`).
  - Interrupt and mark read show no success notice.
- Never run the real `hgs` against live sessions in tests; tests use the fake `hgs` in `client/core/tests/support`.
- Headless GUI runs only with `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...` (`client/README.md`, "Headless runs").
- Repository text in English; fixtures synthetic (reserved example domains, `/home/example/...`, `claude/sample-project/review`).
- Send is not in this phase: it moves to phase 4 with the full composer (`zerus-5wmh.9`). That phase needs Qt's block reasons, the cold cache confirmation, resume-then-send, local failed messages and durable drafts.

## Review Focus

1. A session restarted between the menu opening and the click (new run id): the action runs only if the row still has the run the menu or dialog was built for; otherwise the notice says "This session changed. Open its actions again." and nothing runs. Terminate also passes `--expected-run-id`, so `hgs` refuses a stale run.
2. A double click or a repeated shortcut: exactly one `hgs` process per session while an action is pending; every action entry of that session shows disabled ("Updating…" in Qt) until the outcome arrives.
3. A slow or dropped SSH peer: the action times out with Qt's text, nothing is retried, and the session's actions are enabled again.
4. `hgs` missing or not executable: a definite failure with the spawn error, before any input is written.
5. Archive or terminate prints a `ZERUS_WORKTREE_CLEANUP` line with status `kept` or `unconfirmed`: the notice stays until dismissed and nothing removes the folder or retries.

---

## File Structure

```
client/core/src/actions.rs        Action, Target, Invocation, Exit, Outcome, WorktreeCleanup; invocation(), classify(), allowed()
client/core/src/run.rs            run(), run_with_env(): spawn, stdin, deadline, environment
client/core/src/details.rs        Details (inspect fields used by enablement); details_invocation(), parse_details()
client/core/src/readmarks.rs      ReadMark, ReadMarks: durable pending shared read marks
client/core/src/view.rs           ViewRow gains state, tracked, resumable, activity, process_state, archive_id,
                                  reply_id, attention_signature, shared_read, read_reply_id,
                                  read_attention_signature, allowed_actions, action_reasons
client/core/src/lib.rs            new modules and re-exports
client/core/tests/actions.rs      run() + classify() against the fake hgs
client/core/tests/support/fake_hgs_actions.py   fake hgs for action tests
client/src-tauri/src/actions.rs   run_action, inspect_session, pending_read_marks; in-flight guard
client/src-tauri/src/lib.rs       registers commands and the read-mark store path
client/src-tauri/src/commands.rs  settles read marks against applied rows
client/src/types.ts               the new ViewRow fields, Details, Action, Outcome, Target
client/src/bridge.ts              runAction, inspectSession, pendingReadMarks wrappers
client/src/store/actions.ts       pending state per session, notices, details cache, read-mark retry
client/src/store/actions.test.ts
client/src/ui/sessions/actions-ui.ts      menuState(), terminateCopy(), renameError() (pure, tested)
client/src/ui/sessions/actions-ui.test.ts
client/src/ui/sessions/ActionDialogs.tsx  TerminateConfirm, RenameDialog
client/src/ui/sessions/SessionList.tsx    context menu entries
client/src/ui/sessions/SessionDetail.tsx  header actions, Stop, Mark as read chip
client/README.md                  "Actions" section
```

---

### Task 1: Action requests and outcome rules (`client/core/src/actions.rs`)

**Files:**
- Create: `client/core/src/actions.rs`
- Modify: `client/core/src/lib.rs` (`pub mod actions;` and `pub use actions::{Action, Target, Invocation, Exit, Outcome, WorktreeCleanup};`)
- Modify: `client/core/src/view.rs` (new `ViewRow` fields)
- Test: unit tests in `actions.rs` and `view.rs`

**Interfaces:**
- Consumes: `ViewRow` (phase 1); computer `""` is this computer, otherwise the SSH alias.
- Produces:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target { pub computer: String, pub name: String, pub run_id: String, pub conversation_id: String }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Interrupt { turn_started: f64 },
    MarkRead { reply_id: Option<String>, attention_signature: Option<String> },
    Pause,
    Resume,
    Rename { new_name: String, archive_id: Option<String> },
    Archive,
    Terminate,
}

pub struct Invocation { pub args: Vec<String>, pub stdin: Option<Vec<u8>>, pub timeout: std::time::Duration, pub request_id: Option<String> }

pub enum Exit {
    Code { code: i32, stdout: Vec<u8>, stderr: String },
    Abnormal { stderr: String },
    TimedOut,
    StartFailed(String),
    WriteFailed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorktreeCleanup { pub status: String, pub path: String, pub common_dir: String, pub reason: Option<String> }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Done { receipt: serde_json::Value, cleanup: Option<WorktreeCleanup> },
    Failed { detail: String },
}

pub fn invocation(target: &Target, action: &Action, request_id: &str) -> Result<Invocation, String>;
pub fn classify(target: &Target, action: &Action, invocation: &Invocation, exit: Exit) -> Outcome;
/// NativeSessionCapabilities::action without details: reported allowed_actions win; without them
/// every operation is allowed except that an "opencode" session only renames and forgets.
pub fn allowed(row: &ViewRow, operation: &str) -> bool;
```

`ViewRow` additions, all `#[serde(default)]`, from the catalog row:
- **State:** `state: String`, `tracked: bool`, `resumable: bool`, `activity: String`, `process_state: String`, `archive_id: Option<String>`.
- **Reply identity:** `reply_id: Option<String>`, `attention_signature: Option<String>`, and `shared_read: bool` (the row has the key `attention_signature`).
- **Read state:** `read_reply_id: Option<String>` and `read_attention_signature: Option<String>`, taken from `row.read` only when `row.read.conversation_id == row.conversation_id` (`HgsClient.cpp:186-192`).
- **Capabilities:** `allowed_actions: Option<Vec<String>>`, `action_reasons: BTreeMap<String, String>`.

Calls, inputs and timeouts (from `tray/src/HgsClient.cpp`; `@alias` first for a peer):

| Action | Arguments | Standard input | Timeout this computer / peer |
|---|---|---|---|
| Interrupt | `interrupt NAME --json` | `{"request_id","expected_run_id","expected_conversation_id","expected_turn_started"}` | 15 s / 45 s |
| MarkRead | `read NAME --json` | `{"run_id","conversation_id","reply_id"|null,"attention_signature"|null}` | 15 s / 35 s |
| Pause | `pause NAME` | none | 45 s / 45 s |
| Resume | `resume NAME -d` | none | 30 s / 30 s |
| Rename | `rename NAME NEW` plus `--archive ID` when archived | none | 5 s / 15 s |
| Archive | `archive NAME` | none | 30 s / 45 s |
| Terminate | `terminate NAME --expected-run-id RUN` (flag only when RUN is not empty) | none | 45 s / 60 s |

- [ ] **Step 1: Write the failing tests** (in `actions.rs`):

```rust
fn target(computer: &str) -> Target {
    Target { computer: computer.into(), name: "claude/sample-project/review".into(),
             run_id: "run-1".into(), conversation_id: "conv-1".into() }
}
fn exit0(stdout: &[u8], stderr: &str) -> Exit { Exit::Code { code: 0, stdout: stdout.to_vec(), stderr: stderr.into() } }

#[test]
fn calls_match_the_qt_client() {
    let t = target("");
    assert_eq!(invocation(&t, &Action::Pause, "u").unwrap().args, ["pause", "claude/sample-project/review"]);
    assert_eq!(invocation(&t, &Action::Resume, "u").unwrap().args, ["resume", "claude/sample-project/review", "-d"]);
    assert_eq!(invocation(&t, &Action::Archive, "u").unwrap().args, ["archive", "claude/sample-project/review"]);
    let rename = invocation(&t, &Action::Rename { new_name: "claude/sample-project/review-2".into(), archive_id: Some("a1".into()) }, "u").unwrap();
    assert_eq!(rename.args, ["rename", "claude/sample-project/review", "claude/sample-project/review-2", "--archive", "a1"]);
    let term = invocation(&target("mac"), &Action::Terminate, "u").unwrap();
    assert_eq!(term.args, ["@mac", "terminate", "claude/sample-project/review", "--expected-run-id", "run-1"]);
    assert_eq!(term.timeout, std::time::Duration::from_secs(60));
    let mut no_run = target(""); no_run.run_id.clear();
    assert_eq!(invocation(&no_run, &Action::Terminate, "u").unwrap().args, ["terminate", "claude/sample-project/review"]);
}

#[test]
fn interrupt_sends_its_identity_and_checks_the_receipt() {
    let t = target(""); let a = Action::Interrupt { turn_started: 1793000000.5 };
    let inv = invocation(&t, &a, "req-2").unwrap();
    assert_eq!(inv.args, ["interrupt", "claude/sample-project/review", "--json"]);
    let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(input, serde_json::json!({"request_id":"req-2","expected_run_id":"run-1",
        "expected_conversation_id":"conv-1","expected_turn_started":1793000000.5}));
    let ok = br#"{"status":"submitted","request_id":"req-2","name":"claude/sample-project/review","run_id":"run-1","conversation_id":"conv-1"}"#;
    assert!(matches!(classify(&t, &a, &inv, exit0(ok, "")), Outcome::Done { .. }));
    assert_eq!(classify(&t, &a, &inv, exit0(b"{}", "")),
               Outcome::Failed { detail: "Invalid acknowledgement. Check Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::TimedOut),
               Outcome::Failed { detail: "Request timed out. Check Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::WriteFailed),
               Outcome::Failed { detail: "Transfer failed. Check Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::Code { code: 1, stdout: vec![], stderr: String::new() }),
               Outcome::Failed { detail: "Could not complete the request. Check Terminal.".into() });
}

#[test]
fn mark_read_accepts_applied_or_not_found() {
    let t = target(""); let a = Action::MarkRead { reply_id: Some("rep-1".into()), attention_signature: None };
    let inv = invocation(&t, &a, "u").unwrap();
    assert_eq!(inv.args, ["read", "claude/sample-project/review", "--json"]);
    let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(input, serde_json::json!({"run_id":"run-1","conversation_id":"conv-1","reply_id":"rep-1","attention_signature":null}));
    let applied = br#"{"ok":true,"applied":{"reply_id":true,"attention_signature":false},"read":{"conversation_id":"conv-1"}}"#;
    assert!(matches!(classify(&t, &a, &inv, exit0(applied, "")), Outcome::Done { .. }));
    let gone = br#"{"ok":false,"error":"not_found"}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 1, stdout: gone.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
    assert_eq!(classify(&t, &a, &inv, Exit::TimedOut), Outcome::Failed { detail: "Saving read state timed out.".into() });
    assert_eq!(classify(&t, &a, &inv, exit0(b"{\"ok\":true}", "")),
               Outcome::Failed { detail: "Could not save read state on that machine.".into() });
}

#[test]
fn writes_succeed_on_exit_zero_and_report_stderr_otherwise() {
    let t = target(""); let inv = invocation(&t, &Action::Pause, "u").unwrap();
    assert!(matches!(classify(&t, &Action::Pause, &inv, exit0(b"", "")), Outcome::Done { cleanup: None, .. }));
    assert_eq!(classify(&t, &Action::Pause, &inv, Exit::Code { code: 1, stdout: vec![], stderr: " hgs: no such session \n".into() }),
               Outcome::Failed { detail: "hgs: no such session".into() });
    assert_eq!(classify(&t, &Action::Pause, &inv, Exit::TimedOut), Outcome::Failed { detail: "timed out".into() });
    assert_eq!(classify(&t, &Action::Pause, &inv, Exit::StartFailed("No such file or directory".into())),
               Outcome::Failed { detail: "No such file or directory".into() });
}

#[test]
fn archive_and_terminate_carry_exactly_one_cleanup_line() {
    let t = target(""); let inv = invocation(&t, &Action::Archive, "u").unwrap();
    let line = "ZERUS_WORKTREE_CLEANUP {\"status\":\"kept\",\"path\":\"/home/example/sample-project-wt\",\"common_dir\":\"/home/example/sample-project/.git\",\"reason\":\"unsaved work\"}\n";
    match classify(&t, &Action::Archive, &inv, exit0(b"", line)) {
        Outcome::Done { cleanup: Some(c), .. } => { assert_eq!(c.status, "kept"); assert_eq!(c.reason.as_deref(), Some("unsaved work")); }
        other => panic!("{other:?}"),
    }
    let two = format!("{line}{line}");
    assert!(matches!(classify(&t, &Action::Archive, &inv, exit0(b"", &two)), Outcome::Done { cleanup: None, .. }));
    let no_dir = "ZERUS_WORKTREE_CLEANUP {\"status\":\"removed\",\"path\":\"/home/example/wt\",\"common_dir\":\"\"}\n";
    assert!(matches!(classify(&t, &Action::Archive, &inv, exit0(b"", no_dir)), Outcome::Done { cleanup: None, .. }));
    assert!(matches!(classify(&t, &Action::Pause, &invocation(&t, &Action::Pause, "u").unwrap(), exit0(b"", line)), Outcome::Done { cleanup: None, .. }));
}

#[test]
fn rename_uses_the_cli_name_rule() {
    for bad in ["", "claude/a b", "claude/a=b", "#claude/a", "claude/", "/review"] {
        assert!(invocation(&target(""), &Action::Rename { new_name: bad.into(), archive_id: None }, "u").is_err(), "{bad:?}");
    }
}
```

In `view.rs` tests:
- A row with `"allowed_actions":["rename"]`, `"attention_signature":"sig"` and `"read":{"conversation_id":"conv-1","reply_id":"rep-0"}` projects `allowed_actions == Some(vec!["rename"])`, `shared_read == true` and `read_reply_id == Some("rep-0")`.
- A row whose `read.conversation_id` differs leaves `read_reply_id == None`.
- `allowed(&row, "pause")` is false for that row, and true for a `claude` row without `allowed_actions`.
- An `opencode` row without `allowed_actions` allows only `rename` and `forget`.

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core actions::`. Expected: compile errors (`invocation`, `classify` not found).

- [ ] **Step 3: Implement `actions.rs`:**
  - `invocation` prepends `@{computer}` for a peer and follows the table.
  - The rename rule follows Qt and `src/config.rs::check_name`: both name sections (before and after the first `/`) are non-empty, with no whitespace, no `=` and no leading `#`. The refusal text is "Use a name without spaces or '=', with a project and a session part."
  - JSON input is compact (`serde_json::to_vec`); `request_id` is the passed id for interrupt.
  - `classify` for Interrupt follows `runSessionAction` (non-scoped):
    - `TimedOut`, `WriteFailed` and non-zero exits map to the failure texts in the tests.
    - A non-zero exit with stderr reports that stderr, trimmed and at most 2000 chars.
    - `StartFailed(e)` reports `e`.
    - Exit 0 is Done only when `request_id`, `name`, `run_id == expected_run_id`, `conversation_id == expected_conversation_id` and `status == "submitted"` all match.
  - `classify` for MarkRead follows `requestRead`:
    - Done when exit 0 with `ok == true`, boolean `applied.reply_id` and `applied.attention_signature`, and `read` either null or with `read.conversation_id == conversation_id`.
    - Done when exit 1 with exactly `{"ok":false,"error":"not_found"}`.
    - `TimedOut` gives "Saving read state timed out."; anything else gives stderr or "Could not save read state on that machine.".
  - `classify` for writes follows `runWrite`:
    - Done on exit 0.
    - `TimedOut` gives "timed out"; `StartFailed(e)` or `Abnormal` gives the error text; otherwise stderr trimmed.
    - For Archive and Terminate only, count the stderr lines starting with `ZERUS_WORKTREE_CLEANUP `. Exactly one line, parsing as an object with non-empty `path` and `common_dir`, becomes `cleanup`.
  - `allowed` follows the doc comment.

- [ ] **Step 4: Run** the tests and `cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core --all-targets -- -D warnings`. Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/actions.rs client/core/src/lib.rs client/core/src/view.rs
git commit -m "feat(client): guarded session action requests and Qt's outcome rules"
```

---

### Task 2: Running one action (`client/core/src/run.rs`) against a fake `hgs`

**Files:**
- Create: `client/core/src/run.rs`
- Create: `client/core/tests/support/fake_hgs_actions.py`
- Create: `client/core/tests/actions.rs`
- Modify: `client/core/src/lib.rs` (`pub mod run; pub use run::{run, run_with_env};`)

**Interfaces:**
- Consumes: `Invocation`, `Exit`, `invocation`, `classify` (Task 1).
- Produces: `pub fn run(hgs: &Path, inv: &Invocation) -> Exit` and `pub fn run_with_env(hgs: &Path, inv: &Invocation, parent_env: &[(&str, &str)]) -> Exit` (blocking; returns within `inv.timeout` plus 2 s; `run` is `run_with_env` with an empty slice).

- [ ] **Step 1: Write the fake `hgs`** (stdlib only, never the real CLI):

```python
#!/usr/bin/env python3
"""A fake `hgs` for the client action tests. Reads `config.json` beside itself:
{"reply": {"exit": 0, "stdout": <object or "echo_interrupt">, "stderr": "", "sleep": 0, "log_env": []}}
Appends {"argv": [...], "stdin": "...", "env": {...}} to `log.ndjson` beside itself."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))["reply"]
data = sys.stdin.read()
entry = {"argv": sys.argv[1:], "stdin": data}
if config.get("log_env"):
    entry["env"] = {k: os.environ.get(k) for k in config["log_env"]}
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps(entry) + "\n")
time.sleep(config.get("sleep", 0))
out = config.get("stdout")
if out == "echo_interrupt":
    request = json.loads(data)
    args = [a for a in sys.argv[1:] if not a.startswith("@")]
    out = {"status": "submitted", "request_id": request["request_id"], "name": args[1],
           "run_id": request["expected_run_id"], "conversation_id": request["expected_conversation_id"]}
if out is not None:
    sys.stdout.write(json.dumps(out))
sys.stderr.write(config.get("stderr", ""))
sys.exit(config.get("exit", 0))
```

- [ ] **Step 2: Write the failing integration tests** (`client/core/tests/actions.rs`). `Fake::new(config)` makes a fresh folder under `std::env::temp_dir()` named with the process id and a counter, copies `tests/support/fake_hgs_actions.py` there as `hgs` with mode 0755 and writes `config.json`; `log()` parses `log.ndjson`; `Drop` removes the folder. `target()` is the Task 1 target on this computer.

```rust
#[test]
fn an_interrupt_through_the_fake_is_done_and_logged_once() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stdout":"echo_interrupt"}}"#);
    let t = target(); let a = Action::Interrupt { turn_started: 1.5 };
    let inv = invocation(&t, &a, "req-1").unwrap();
    assert!(matches!(classify(&t, &a, &inv, run(&fake.hgs, &inv)), Outcome::Done { .. }));
    assert_eq!(fake.log().len(), 1);
    assert_eq!(fake.log()[0]["argv"], serde_json::json!(["interrupt", "claude/sample-project/review", "--json"]));
}

#[test]
fn a_terminate_with_a_cleanup_line_reports_it() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stderr":"ZERUS_WORKTREE_CLEANUP {\"status\":\"unconfirmed\",\"path\":\"/home/example/wt\",\"common_dir\":\"/home/example/repo/.git\",\"reason\":\"in use\"}\n"}}"#);
    let t = target(); let inv = invocation(&t, &Action::Terminate, "u").unwrap();
    match classify(&t, &Action::Terminate, &inv, run(&fake.hgs, &inv)) {
        Outcome::Done { cleanup: Some(c), .. } => assert_eq!(c.status, "unconfirmed"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hanging_hgs_times_out_and_is_killed() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"sleep":30}}"#);
    let mut inv = invocation(&target(), &Action::Pause, "u").unwrap();
    inv.timeout = std::time::Duration::from_millis(500);
    let started = std::time::Instant::now();
    assert!(matches!(run(&fake.hgs, &inv), Exit::TimedOut));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn a_missing_hgs_fails_to_start() {
    let inv = invocation(&target(), &Action::Pause, "u").unwrap();
    assert!(matches!(run(std::path::Path::new("/nonexistent/hgs"), &inv), Exit::StartFailed(_)));
}

#[test]
fn the_agent_attribution_is_not_inherited() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"log_env":["HGS_SESSION","HGS_RUN_ID"]}}"#);
    let inv = invocation(&target(), &Action::Pause, "u").unwrap();
    assert!(matches!(run_with_env(&fake.hgs, &inv, &[("HGS_SESSION", "x"), ("HGS_RUN_ID", "y")]), Exit::Code { code: 0, .. }));
    assert_eq!(fake.log()[0]["env"], serde_json::json!({"HGS_SESSION": null, "HGS_RUN_ID": null}));
}
```

- [ ] **Step 3: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core --test actions`. Expected: compile error (`run` missing).

- [ ] **Step 4: Implement `run.rs`:**
  - Build `Command::new(hgs).args(&inv.args)`, apply `parent_env`, then `.env_remove("HGS_SESSION").env_remove("HGS_RUN_ID")` (as `stream.rs:152`), with stdin, stdout and stderr piped.
  - A spawn error maps to `StartFailed(e.to_string())`.
  - Write all of stdin when there is any, then close it. A short or failed write gives `WriteFailed` and kills the child.
  - Read stdout and stderr on two threads.
  - Poll `try_wait` every 20 ms until the deadline, then `kill()`, `wait()` and return `TimedOut`.
  - A signal exit maps to `Abnormal { stderr }`. Cap stderr at 64 KiB while reading, so a noisy child cannot grow memory.

- [ ] **Step 5: Run** all core tests and clippy. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add client/core/src/run.rs client/core/src/lib.rs client/core/tests/actions.rs client/core/tests/support/fake_hgs_actions.py
git commit -m "feat(client): run one guarded hgs action with a deadline"
```

---

### Task 3: Selected-session details (`client/core/src/details.rs`)

The header enablement in Qt reads inspect fields of the selected session (`m_details`), for example `auth_required`, `expected_id`, `resume_message_can_send`, `allowed_actions` and `action_reasons`. Details are adopted only when they name the row's run and conversation (`SessionsWindow.cpp:5343-5346`).

**Files:**
- Create: `client/core/src/details.rs`
- Modify: `client/core/src/lib.rs`

**Interfaces:**
- Consumes: `Target`, `Invocation`, `Exit` (Task 1).
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Details {
    pub run_id: String, pub conversation_id: String,
    pub auth_required: bool, pub expected_id: String, pub resume_message_can_send: bool,
    pub allowed_actions: Option<Vec<String>>, pub action_reasons: BTreeMap<String, String>,
}
/// `hgs [@host] inspect NAME --after 0 --skip-processes [--archive ID]`, 5 s here / 15 s on a peer (HgsClient.cpp:925-931).
pub fn details_invocation(target: &Target, archive_id: Option<&str>) -> Invocation;
/// The details when exit 0 and the JSON names target.run_id and target.conversation_id; otherwise None.
pub fn parse_details(target: &Target, exit: Exit) -> Option<Details>;
```

- [ ] **Step 1: Write the failing tests** (in `details.rs`):

```rust
#[test]
fn inspect_call_matches_the_qt_client() {
    let t = Target { computer: "mac".into(), name: "claude/sample-project/review".into(), run_id: "run-1".into(), conversation_id: "conv-1".into() };
    let inv = details_invocation(&t, Some("a1"));
    assert_eq!(inv.args, ["@mac", "inspect", "claude/sample-project/review", "--after", "0", "--skip-processes", "--archive", "a1"]);
    assert_eq!(inv.timeout, std::time::Duration::from_secs(15));
}

#[test]
fn details_are_adopted_only_for_the_listed_run() {
    let t = Target { computer: String::new(), name: "claude/sample-project/review".into(), run_id: "run-1".into(), conversation_id: "conv-1".into() };
    let body = br#"{"run_id":"run-1","conversation_id":"conv-1","expected_id":"e1","resume_message_can_send":false}"#;
    let d = parse_details(&t, Exit::Code { code: 0, stdout: body.to_vec(), stderr: String::new() }).unwrap();
    assert_eq!(d.expected_id, "e1"); assert!(!d.resume_message_can_send); assert_eq!(d.allowed_actions, None);
    let other = br#"{"run_id":"run-2","conversation_id":"conv-1"}"#;
    assert_eq!(parse_details(&t, Exit::Code { code: 0, stdout: other.to_vec(), stderr: String::new() }), None);
    assert_eq!(parse_details(&t, Exit::Code { code: 1, stdout: body.to_vec(), stderr: String::new() }), None);
    assert_eq!(parse_details(&t, Exit::Code { code: 0, stdout: b"not json".to_vec(), stderr: String::new() }), None);
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core details::` and expect FAIL.
- [ ] **Step 3: Implement** with `serde_json::Value` reads (unknown fields ignored; missing booleans false; missing `allowed_actions` stays `None`).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/core/src/details.rs client/core/src/lib.rs
git commit -m "feat(client): selected-session details for action enablement"
```

---

### Task 4: Durable shared read marks (`client/core/src/readmarks.rs`)

**Files:**
- Create: `client/core/src/readmarks.rs`
- Modify: `client/core/src/lib.rs`

**Interfaces:**
- Consumes: `ViewRow` fields from Task 1.
- Produces:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadMark { pub computer: String, pub name: String, pub run_id: String, pub conversation_id: String,
                      pub reply_id: Option<String>, pub attention_signature: Option<String> }
pub struct ReadMarks { /* path and marks keyed by (computer, name) */ }
impl ReadMarks {
    pub fn load(path: PathBuf) -> Self;                 // missing or unreadable file: empty, no panic
    pub fn add(&mut self, mark: ReadMark);              // ReadMark::merged (tray/src/ReadMarks.h:41-48), then save
    pub fn pending(&self) -> Vec<ReadMark>;
    /// Drops the mark the row already shows (equal read_reply_id or read_attention_signature,
    /// or another run or conversation); returns true when it changed anything (then saves).
    pub fn settle(&mut self, computer: &str, row: &ViewRow) -> bool;
}
```

- [ ] **Step 1: Write the failing tests** (temp file under `std::env::temp_dir()` named with the process id and test name):
  - `a_newer_mark_keeps_the_field_it_does_not_set`: add `reply_id: Some("r1")`, then the same run with `reply_id: None, attention_signature: Some("s1")`; `pending()` holds one mark with `r1` and `s1`.
  - `a_mark_for_another_run_replaces_the_old_one`.
  - `marks_survive_a_restart`: `add`, then `ReadMarks::load(same path).pending()` returns it.
  - `the_host_snapshot_settles_a_mark`: the same run with `read_reply_id == Some("r1")` drops it; a row with a new `run_id` drops it; a row showing another reply keeps it.
  - `an_unreadable_file_starts_empty`: the path holds `not json`; `load` gives no marks.
- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core readmarks::` and expect FAIL.
- [ ] **Step 3: Implement** a `BTreeMap<(String, String), ReadMark>` saved as JSON through a temporary file in the same folder plus `std::fs::rename`.
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/core/src/readmarks.rs client/core/src/lib.rs
git commit -m "feat(client): durable shared read marks until the host shows them"
```

---

### Task 5: Tauri commands and the in-flight guard (`client/src-tauri/src/actions.rs`)

**Files:**
- Create: `client/src-tauri/src/actions.rs`
- Modify: `client/src-tauri/src/lib.rs`: the module; `.manage(Actions::new(app.path().app_data_dir()?.join("read-marks.json")))`; `run_action`, `inspect_session` and `pending_read_marks` in `invoke_handler`.
- Modify: `client/src-tauri/src/commands.rs` (call `actions.settle(&computer, &row)` for each upserted row of an applied batch)

**Interfaces:**
- Consumes: `invocation`, `classify`, `run`, `details_invocation`, `parse_details`, `ReadMarks`, `default_hgs()`.
- Produces (frontend contract, snake_case JSON):

```rust
#[tauri::command]
pub async fn run_action(state: State<'_, Actions>, target: Target, action: Action) -> Result<Outcome, String>; // Err("busy") while one runs
#[tauri::command]
pub async fn inspect_session(target: Target, archive_id: Option<String>) -> Option<Details>;
#[tauri::command]
pub fn pending_read_marks(state: State<'_, Actions>) -> Vec<ReadMark>;
```

- [ ] **Step 1: Write the failing test** for the guard type:

```rust
#[test]
fn one_action_per_session_at_a_time() {
    let flight = InFlight::default();
    let first = flight.claim(("".into(), "claude/sample-project/review".into())).unwrap();
    assert!(flight.claim(("".into(), "claude/sample-project/review".into())).is_none());
    assert!(flight.claim(("mac".into(), "claude/sample-project/review".into())).is_some());
    drop(first);
    assert!(flight.claim(("".into(), "claude/sample-project/review".into())).is_some());
}
```

(One guard per session for every kind: Qt's `updating` disables all of a session's actions while one runs.)

- [ ] **Step 2: Run** `cargo +1.99.1 test --locked` in `client/src-tauri`. Expected: FAIL.
- [ ] **Step 3: Implement:**
  - `InFlight` is a `Mutex<HashSet<(String, String)>>` whose claim guard removes the key on drop.
  - `run_action` claims the session, makes the request id with `uuid::Uuid::new_v4()` (reuse the dependency if `cargo tree -i uuid` shows one) and builds the invocation; a refusal returns `Outcome::Failed`. It runs `run` on `tauri::async_runtime::spawn_blocking`, then classifies.
  - For `MarkRead` it first `add`s the mark to `ReadMarks`, so the mark survives a crash during the call.
  - `inspect_session` runs `details_invocation` the same way, without the guard. The store keeps at most one inspect in flight per session (Task 6).
- [ ] **Step 4: Run** tests and `cargo +1.99.1 clippy --locked --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/actions.rs client/src-tauri/src/lib.rs client/src-tauri/src/commands.rs client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock
git commit -m "feat(client): run_action and inspect_session with one action per session"
```

---

### Task 6: Frontend store, notices, details and read-mark retry (`client/src/store/actions.ts`)

**Files:**
- Create: `client/src/store/actions.ts`, `client/src/store/actions.test.ts`
- Modify: `client/src/types.ts` (the new `ViewRow` fields; `Details`, `Action`, `Outcome`, `Target`)
- Modify: `client/src/bridge.ts` (`runAction`, `inspectSession`, `pendingReadMarks`; in the browser build they resolve `{outcome:"failed", detail:"Actions need the desktop app."}`, `null` and `[]`)

**Interfaces:**
- Consumes: the Task 5 commands; `RowOut` from `client/src/types.ts`; the catalog store (phase 1, `client/src/store/catalog.ts`).
- Produces:

```ts
export function targetOf(entry: RowOut): Target | undefined;   // undefined without a run id
export function updating(computer: string, name: string): boolean;   // reactive: an action of that session is pending
export async function perform(entry: RowOut, action: Action): Promise<Outcome>;
export function detailsFor(entry: RowOut): Details | undefined;  // reactive; fetched once per selection and run
export const notices: () => Notice[];   // { id, tone: "error" | "warn" | "info", text, sticky }
export function dismiss(id: number): void;
```

Rules:
- `perform` re-reads the row by `(computer, key)` from the catalog. If the row is gone or its `run_id` differs from `entry.row.run_id`, it shows the info notice "This session changed. Open its actions again." and returns without a call.
- **Success notices:** rename shows "Session renamed. Refreshing…"; pause, resume, archive and terminate show "Session updated. Refreshing…" (info, 6 s). Interrupt and mark read show none.
- **Failures:** an error notice with the detail, which stays until dismissed.
- **Cleanup notices:**
  - `removed`: info "Worktree removed automatically. The branch and archived conversation are kept."
  - `kept` and `unconfirmed`: a sticky warning "Worktree kept for review: {reason}".
  - Nothing ever retries.
- `detailsFor` calls `inspectSession` when a session is selected and again when its `run_id`, `conversation_id` or `state` changes. Only one inspect runs per session at a time, and only details whose identities match the row are kept.
- **Read-mark retry:** load `pendingReadMarks()` at start. When a batch upserts a row with a pending mark that still shows unread, call `perform(entry, mark_read)` again, at most once per 5 s per session.

- [ ] **Step 1: Write the failing vitest tests** (`vi.mock("../bridge")`, fake timers):
  - pending state is set and cleared;
  - a changed run refuses with the notice text and no call;
  - the success text per action, and none for interrupt and mark read;
  - a failed outcome makes a sticky error;
  - a `kept` cleanup makes a sticky warning with the reason;
  - `runAction` is called exactly once per `perform`, even 10 s later;
  - details are fetched once per selection, refetched on a run change, and dropped on mismatch;
  - a pending read mark is retried on a batch, and not again within 5 s.
- [ ] **Step 2: Run** `npx vitest run src/store/actions.test.ts` in `client` and expect FAIL.
- [ ] **Step 3: Implement** with Solid stores. Notices are capped at 5; the copy has no dot separators.
- [ ] **Step 4: Run** `npx vitest run` and `npm run check`. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/src/store/actions.ts client/src/store/actions.test.ts client/src/types.ts client/src/bridge.ts
git commit -m "feat(client): action store with Qt's notices and no automatic retry"
```

---

### Task 7: Menu entries, header actions, Stop, Mark as read and the dialogs

**Files:**
- Create: `client/src/ui/sessions/actions-ui.ts`, `client/src/ui/sessions/actions-ui.test.ts`, `client/src/ui/sessions/ActionDialogs.tsx`
- Modify: `client/src/ui/sessions/SessionList.tsx`, `client/src/ui/sessions/SessionDetail.tsx`

**Interfaces:**
- Consumes: `perform`, `updating`, `detailsFor`, `targetOf` (Task 6).
- Produces:

```ts
export interface MenuState {
  change: { label: "Pause session" | "Resume session" | "Restore session" | "Updating…"; enabled: boolean };
  rename: boolean;
  archive: { visible: boolean; enabled: boolean; reason?: string };
  end: { label: "Terminate session…" | "Forget saved session…" | "Forget archived session…"; enabled: boolean; reason?: string };
  stop: boolean;
  markRead: { label: "Mark as read" | "Mark needs attention"; enabled: boolean; reason?: string };
}
export function menuState(entry: RowOut, online: boolean, updating: boolean, details?: Details): MenuState;
export function terminateCopy(entry: RowOut): { title: string; text: string; detail: string; confirm: string };
export function renameError(name: string): string | undefined;
```

Rules (Qt `SessionsWindow.cpp` 3890-3912 for the selected session with details, and 5270-5380 for the context menu):
- `available = online && !updating`.
- **Change entry** (header and menu, without details):
  - `canPause = state == "running" && resumable && activity == "idle" && process_state != "exited"`.
  - For the selected session with details, `activity == "idle"` may instead be `details.auth_required`. Additionally `details.expected_id == "" || details.resume_message_can_send`.
  - Enabled when `available` and either:
    - not archived, and (`canPause && allowed("pause")`) or (`state != "running" && resumable && allowed("resume")`);
    - or archived, which shows "Restore session" disabled in this phase (Restore moves to `zerus-5wmh.17`).
  - The label is "Updating…" while updating, otherwise as in the type.
- **Rename:** `allowed("rename") && available`, both name sections non-empty, and `archive_id` present when archived.
- **Move to archive:** visible only when `agent != "dsh"` and `state` is `paused` or `stopped`; enabled when `!updating && allowed("archive")`; the reason is `action_reasons.archive`.
- **End entry:** operation `terminate` when `state == "running"` and not forgets-binding-only (`agent == "opencode" && allowed("forget") && !allowed("terminate")`); otherwise `forget`.
  - The label is "Terminate session…", "Forget saved session…", or "Forget archived session…" when archived.
  - Terminate is enabled when `available && allowed("terminate")`. The Forget variants stay disabled in this phase with the reason "Forgetting sessions arrives in a later version." (`zerus-5wmh.17`).
- **Stop:** `turn_started` set, `allowed("interrupt")` and `available`.
- **Mark as read:** shown when the row needs attention (`attention` set, or `unread`, matching Qt `SessionRows::needsAttention` for the fields the client has). It runs `mark_read` with the row's `reply_id` and `attention_signature`.
  - When the host has no shared read state (`!shared_read`), it is disabled with the reason "Update hgs on this session's host to share read state." (the local fallback is `zerus-5wmh.16`).
  - Otherwise the entry is "Mark needs attention", disabled with the reason "Arrives with attention marks." (`zerus-5wmh.16`).
- **Terminate confirmation** (`SessionsWindow.cpp:5771-5797`):
  - Title "Terminate session?".
  - When `tracked` and the agent is `codex`, `claude` or `kimi`, the text is "Stop {name} and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept." and the confirm button is "Stop and archive".
  - Otherwise the text is "Terminate {name}? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept." and the button is "Terminate session".
  - The detail is "Computer: {machine}".
  - Cancel is focused and is the default button, and Escape cancels. There is no Remove-worktree checkbox in this phase (`zerus-5wmh.10`).
  - On confirm, the row is checked again as in `perform`.
- **Rename dialog:**
  - The field starts with the full current name, with the session part selected.
  - `renameError` returns "Use a name without spaces or '=', with a project and a session part." for names Task 1's rule refuses. Rename is disabled while there is an error or the name is unchanged.
  - Escape cancels.

- [ ] **Step 1: Write the failing vitest tests** for `menuState`: one case per rule above with synthetic rows, including an `opencode` row, a `dsh` row, an archived row with and without `archive_id`, a running `claude` row in idle and in working, `process_state == "exited"`, and details with `expected_id` set and `resume_message_can_send` false. Also test `terminateCopy` for both variants and `renameError`.
- [ ] **Step 2: Run** `npx vitest run src/ui/sessions` and expect FAIL.
- [ ] **Step 3: Implement** the helpers, `ActionDialogs.tsx` (the mockup's dialog classes and skin variables) and the wiring. In the context menu and the header, replace the phase 1 `disabled: true` entries with `menuState`, and show disabled entries' reasons as their tooltip, as Qt does with `hint`.
- [ ] **Step 4: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py`. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/src/ui/sessions
git commit -m "feat(client): session actions with Qt's enablement and confirmations"
```

---

### Task 8: Documentation and the end-to-end check

**Files:**
- Modify: `client/README.md` ("Actions" section: the call table, the outcome rules, "unknown outcomes are never replayed", where read marks are stored)
- Modify: `client/src/ui/PerfHost.tsx` (an `actions-smoke=1` query hook beside the perf hooks, documented in the README)
- Modify: `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md` (the phase table: phase 2 without send; send in phase 4)

- [ ] **Step 1: End-to-end with the fake `hgs`.** Copy `fake_hgs_actions.py` as `hgs` with `{"reply":{"exit":0,"stdout":"echo_interrupt"}}` into a scratch folder and point `ZERUS_CLIENT_HGS` at it.
  - Run the release app headless on the replay fixtures with `ZERUS_CLIENT_QUERY=actions-smoke=1` and the Headless constraint's environment.
  - The hook selects the replay session `claude/sample-project/review`, runs pause, resume, rename (to `claude/sample-project/review-renamed`), archive and terminate through `perform` (confirming dialogs programmatically), then exits.
  - Afterwards `log.ndjson` holds exactly one call per action, with the table's argument lists.
  - Record the command and the log lines in the PR description. Screenshots stay in the scratch folder.
- [ ] **Step 2: Run all checks:** `cargo +1.99.1 test`, plus clippy with `-D warnings`, in `client/core` and `client/src-tauri`; then `npx vitest run`, `npm run check`, `python3 scripts/check-ui-separators.py` and `python3 scripts/check-source-privacy.py`.
- [ ] **Step 3: Commit**

```bash
git add client docs/superpowers/plans
git commit -m "docs(client): guarded session actions and their outcome rules"
```

---

## Phases of the remaining Qt session-menu entries

| Entry | Phase and Beads task |
|---|---|
| Send, the composer guards, cold cache confirmation, resume-then-send, local failed messages, durable drafts | Phase 4, `zerus-5wmh.9` |
| Open terminal, Open native DeepSeek UI, Open folder, Open shell in session folder, Show running commands | Phase 4, `zerus-5wmh.9` |
| Copy session reference, Copy folder path, Copy connect command, Copy restore command | Phase 4, `zerus-5wmh.19` |
| Mark needs attention, Mute and Unmute, Mark all as read, local read marks for older hosts | Phase 3, `zerus-5wmh.16` |
| Move to project (and New project), Rename project, Project settings, Sort sessions by name, Delete project | Phase 3, `zerus-5wmh.18` |
| Start fresh, Restore session, Forget saved session, Forget archived session, Clear archive | Phase 5, `zerus-5wmh.17` |
| Remove worktree after Terminate and the worktree review offer, Fork session, Clear session, Compact context | Phase 5, `zerus-5wmh.10` |
| Multi-selection batch actions | After phase 2, `zerus-5wmh.20` |
| Expand or compact session row, Expand all collapsed, Copy session name | Done in phase 1 |
