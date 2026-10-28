# Tauri Client Phase 2: Guarded Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The new desktop client sends messages, interrupts a turn, marks a session read, pauses, resumes, renames, archives and terminates sessions through the same guarded `hgs` calls, identities, confirmations and outcome rules as the Qt GUI (Beads `zerus-5wmh.7`).

**Architecture:** `client/core` gains a pure action layer: one module builds the exact `hgs` argument list, standard input and timeout for each action (`actions.rs`). A second module runs one invocation with a deadline (`run.rs`) and classifies the exit into done, failed or uncertain with Qt's receipt checks. A third keeps read marks durable until the host's snapshot shows them (`readmarks.rs`). `client/src-tauri` exposes one `run_action` command that runs these on a worker thread and refuses a second action of the same kind on the same session while one is in flight. The SolidJS frontend enables the menu entries phase 1 left disabled and adds the confirmations, the rename dialog, the outcome notices and a text-only composer. Unknown outcomes are shown, never replayed.

**Tech Stack:** Rust 1.99.1 (`client/core`, `client/src-tauri`, Tauri 2), SolidJS 1.9 + TypeScript + Vite, vitest, Python 3 stdlib for the fake `hgs`.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` (section "Actions"; the phase table in `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`). The behavioural reference is the Qt GUI: `tray/src/HgsClient.cpp` (`requestSendMessage`, `runSessionAction`, `requestRead`, `runWrite`, `terminateSession`, `pauseSession`, `resumeSession`, `archiveSession`, `renameSession`), `tray/src/NativeSessionCapabilities.h`, `tray/src/ReadMarks.h`, and the terminate confirmation in `tray/src/SessionsWindow.cpp` (object name `terminateSessionConfirm`).

## Global Constraints

- "Actions keep today's guarded CLI calls and their rules: request identities, expected run and conversation, destructive confirmations with the safe choice focused, and **unknown outcomes are never replayed automatically**. The client adds no new action semantics." (spec, Actions)
- Rust toolchain 1.99.1 (`cargo +1.99.1`, `rust-version = "1.99"`); `client/core` stays pure Rust without Tauri.
- The new UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5, U+2027, U+2981, U+30FB); `scripts/check-ui-separators.py` enforces it.
- No obvious keyboard hints in the UI (owner rule, round 6): no "Enter to send" text.
- Never run the real `hgs` against live sessions in tests; tests use the fake `hgs` in `client/core/tests/support`.
- Headless GUI runs only with `env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 xvfb-run -a -s "-screen 0 1440x900x24" ...` (`client/README.md`, "Headless runs").
- Repository text in English; no session names, real paths or e-mail addresses in fixtures (reserved example domains, `/home/example/...`).
- Remove-worktree cleanup after Terminate, forgetting saved or archived sessions, fork, clear and compact context are out of scope (they move with `zerus-5wmh.10`); their menu entries stay disabled.

## Review Focus

1. A session restarted between the click and the call (new run id): `hgs` refuses the stale expected run; the client shows the failure with Qt's text, keeps the composer text, and the next attempt uses the new run from the catalog.
2. Enter pressed twice or a button double-clicked: exactly one `hgs` process per session and action kind; the second press is refused with "A message to this session is already being sent." (send) or ignored while pending (other actions).
3. A slow or dropped SSH peer: the action times out, the outcome is uncertain or failed with "Check Terminal before retrying", nothing is retried, and the session's action is enabled again.
4. `hgs` missing or not executable: a definite failure before any bytes are sent (never "uncertain").
5. The selected session changes, disappears or is renamed while a confirmation or rename dialog is open: the action runs only against the session the dialog was opened for, and only if it is still listed with the same run.

---

## File Structure

```
client/core/src/actions.rs          Action, Target, Invocation, Exit, Outcome; invocation(), classify(), allowed()
client/core/src/run.rs              run(hgs, &Invocation) -> Exit: spawn, stdin, deadline, environment
client/core/src/readmarks.rs        ReadMark, ReadMarks: durable pending marks, merge, settle against rows
client/core/src/view.rs             ViewRow gains state, tracked, reply_id, attention_signature, shared_read,
                                    read_reply_id, read_attention_signature, allowed_actions, action_reasons
client/core/src/lib.rs              pub mod actions, run, readmarks; re-exports
client/core/tests/actions.rs        run() + classify() against the fake hgs
client/core/tests/support/fake_hgs_actions.py   fake hgs for action tests
client/src-tauri/src/actions.rs     run_action and pending_read_marks commands, in-flight guard
client/src-tauri/src/lib.rs         registers the commands and the read-mark store path
client/src/store/actions.ts         action state per session, outcome notices, mark-read queue on the JS side
client/src/store/actions.test.ts    vitest for the store
client/src/ui/sessions/ActionDialogs.tsx   TerminateConfirm, RenameDialog
client/src/ui/sessions/Composer.tsx        text-only composer (port of design/webui Composer, no attachments)
client/src/ui/sessions/SessionList.tsx     enables the menu entries
client/src/ui/sessions/SessionDetail.tsx   Stop button, Mark as read chip, composer
client/README.md                    "Actions" section
```

---

### Task 1: Action requests and outcome rules (`client/core/src/actions.rs`)

**Files:**
- Create: `client/core/src/actions.rs`
- Modify: `client/core/src/lib.rs` (add `pub mod actions;` and `pub use actions::{Action, Target, Invocation, Exit, Outcome};`)
- Modify: `client/core/src/view.rs` (new `ViewRow` fields, see Interfaces)
- Test: unit tests inside `actions.rs`, plus new cases in `view.rs` tests

**Interfaces:**
- Consumes: `ViewRow` (phase 1), `ComputerId` (`""` is this computer).
- Produces:

```rust
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// "" for this computer, otherwise the SSH alias (`hgs @alias`).
    pub computer: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Send { text: String },
    Interrupt { turn_started: f64 },
    MarkRead { reply_id: Option<String>, attention_signature: Option<String> },
    Pause,
    Resume,
    Rename { new_name: String },
    Archive,
    Terminate,
}

pub struct Invocation {
    pub args: Vec<String>,          // without the program; "@alias" first for a peer
    pub stdin: Option<Vec<u8>>,     // JSON for send, interrupt and read
    pub timeout: std::time::Duration,
    pub request_id: Option<String>, // send and interrupt
}

pub enum Exit {
    Code { code: i32, stdout: Vec<u8>, stderr: String },
    Abnormal { stderr: String },    // killed by a signal
    TimedOut,
    StartFailed(String),            // spawn error text
    WriteFailed,                    // stdin could not be written in full
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Done { receipt: serde_json::Value },
    Failed { detail: String },
    Uncertain { detail: String },   // send only, exactly where Qt reports uncertain
}

/// The exact `hgs` call, or the refusal Qt shows before starting a process.
pub fn invocation(target: &Target, action: &Action, request_id: &str) -> Result<Invocation, String>;
/// Qt's receipt and exit rules for that action.
pub fn classify(target: &Target, action: &Action, invocation: &Invocation, exit: Exit) -> Outcome;
/// Qt's NativeSessionCapabilities::action: reported allowed_actions win; without them
/// every operation is allowed except that an "opencode" session only renames and forgets.
pub fn allowed(row: &ViewRow, operation: &str) -> bool;
```

`ViewRow` additions (all from the catalog row, `#[serde(default)]`, snake_case JSON to the frontend):
`state: String` (`row.state`), `tracked: bool`, `reply_id: Option<String>`, `attention_signature: Option<String>`, `shared_read: bool` (the row has the key `attention_signature`), `read_reply_id: Option<String>` and `read_attention_signature: Option<String>` (from `row.read` only when `row.read.conversation_id == row.conversation_id`, as `HgsClient.cpp:188-192`), `allowed_actions: Option<Vec<String>>`, `action_reasons: BTreeMap<String, String>`.

The argument lists, inputs and timeouts (copied from `tray/src/HgsClient.cpp`; `@alias` is prepended for a peer):

| Action | Arguments | Standard input | Timeout this computer / peer |
|---|---|---|---|
| Send | `send NAME --json` | `{"request_id","text","attachments":[],"expected_run_id","expected_conversation_id"}` | 15 s / 45 s |
| Interrupt | `interrupt NAME --json` | `{"request_id","expected_run_id","expected_conversation_id","expected_turn_started"}` | 15 s / 45 s |
| MarkRead | `read NAME --json` | `{"run_id","conversation_id","reply_id"|null,"attention_signature"|null}` | 15 s / 35 s |
| Pause | `pause NAME` | none | 45 s / 45 s |
| Resume | `resume NAME -d` | none | 30 s / 30 s |
| Rename | `rename NAME NEW` | none | 5 s / 15 s |
| Archive | `archive NAME` | none | 30 s / 45 s |
| Terminate | `terminate NAME --expected-run-id RUN` (flag only when RUN is not empty) | none | 45 s / 60 s |

The timeouts and texts are Qt's (`tray/src/HgsClient.cpp`: send 15 s / 45 s at line 2100, session actions 15 s / 45 s, read 15 s / 35 s, writes as passed to `runWrite`). Refusal texts, verbatim: "Refresh the session before sending a message.", "Use at most 64 KiB of text and 8 attachments.", "Write a message or attach a file first.".

- [ ] **Step 1: Write the failing tests** (in `actions.rs`, `#[cfg(test)] mod tests`):

```rust
fn target(computer: &str) -> Target {
    Target { computer: computer.into(), name: "claude/sample-project/review".into(),
             run_id: "run-1".into(), conversation_id: "conv-1".into() }
}

#[test]
fn send_builds_the_guarded_call() {
    let inv = invocation(&target(""), &Action::Send { text: "hello".into() }, "req-1").unwrap();
    assert_eq!(inv.args, ["send", "claude/sample-project/review", "--json"]);
    let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(input, serde_json::json!({"request_id":"req-1","text":"hello","attachments":[],
        "expected_run_id":"run-1","expected_conversation_id":"conv-1"}));
    let peer = invocation(&target("mac"), &Action::Send { text: "hello".into() }, "req-1").unwrap();
    assert_eq!(peer.args[0], "@mac");
}

#[test]
fn send_refuses_like_qt_before_starting() {
    let mut t = target(""); t.run_id.clear();
    assert_eq!(invocation(&t, &Action::Send { text: "x".into() }, "r").err().unwrap(),
               "Refresh the session before sending a message.");
    assert_eq!(invocation(&target(""), &Action::Send { text: "  \n".into() }, "r").err().unwrap(),
               "Write a message or attach a file first.");
    let big = "a".repeat(MAX_MESSAGE_BYTES + 1);
    assert_eq!(invocation(&target(""), &Action::Send { text: big }, "r").err().unwrap(),
               "Use at most 64 KiB of text and 8 attachments.");
}

#[test]
fn a_valid_send_receipt_is_done_and_anything_else_is_uncertain() {
    let t = target("");
    let a = Action::Send { text: "hello".into() };
    let inv = invocation(&t, &a, "req-1").unwrap();
    let ok = br#"{"status":"submitted","request_id":"req-1","agent_id":"","name":"claude/sample-project/review","run_id":"run-1","conversation_id":"conv-1"}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: ok.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
    let wrong_run = br#"{"status":"submitted","request_id":"req-1","agent_id":"","name":"claude/sample-project/review","run_id":"run-2","conversation_id":"conv-1"}"#;
    assert_eq!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: wrong_run.to_vec(), stderr: String::new() }),
               Outcome::Uncertain { detail: "Delivery acknowledgement is invalid. Inspect Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::TimedOut),
               Outcome::Uncertain { detail: "Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::WriteFailed),
               Outcome::Uncertain { detail: "Message transfer failed. Inspect Terminal before retrying.".into() });
}

#[test]
fn send_exit_one_is_a_definite_failure_unless_delivery_is_uncertain() {
    let t = target(""); let a = Action::Send { text: "hello".into() };
    let inv = invocation(&t, &a, "req-1").unwrap();
    assert_eq!(classify(&t, &a, &inv, Exit::Code { code: 1, stdout: vec![], stderr: "hgs: stale run".into() }),
               Outcome::Failed { detail: "hgs: stale run".into() });
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 1, stdout: vec![], stderr: "hgs: delivery uncertain".into() }), Outcome::Uncertain { .. }));
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 2, stdout: vec![], stderr: String::new() }), Outcome::Uncertain { .. }));
    assert_eq!(classify(&t, &a, &inv, Exit::StartFailed("No such file".into())),
               Outcome::Failed { detail: "Cannot start hgs: No such file".into() });
}

#[test]
fn a_first_message_needs_first_message_true() {
    let mut t = target(""); t.conversation_id.clear();
    let a = Action::Send { text: "hello".into() };
    let inv = invocation(&t, &a, "req-1").unwrap();
    let without = br#"{"status":"submitted","request_id":"req-1","agent_id":"","name":"claude/sample-project/review","run_id":"run-1","conversation_id":""}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: without.to_vec(), stderr: String::new() }), Outcome::Uncertain { .. }));
    let with = br#"{"status":"submitted","request_id":"req-1","agent_id":"","name":"claude/sample-project/review","run_id":"run-1","conversation_id":"","first_message":true}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: with.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
}

#[test]
fn interrupt_checks_identity_and_submitted() {
    let t = target(""); let a = Action::Interrupt { turn_started: 1793000000.5 };
    let inv = invocation(&t, &a, "req-2").unwrap();
    assert_eq!(inv.args, ["interrupt", "claude/sample-project/review", "--json"]);
    let ok = br#"{"status":"submitted","request_id":"req-2","name":"claude/sample-project/review","run_id":"run-1","conversation_id":"conv-1"}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: ok.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
    assert_eq!(classify(&t, &a, &inv, Exit::TimedOut), Outcome::Failed { detail: "Request timed out. Check Terminal before retrying.".into() });
    assert_eq!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: b"{}".to_vec(), stderr: String::new() }),
               Outcome::Failed { detail: "Invalid acknowledgement. Check Terminal before retrying.".into() });
}

#[test]
fn mark_read_accepts_applied_or_not_found() {
    let t = target(""); let a = Action::MarkRead { reply_id: Some("rep-1".into()), attention_signature: None };
    let inv = invocation(&t, &a, "unused").unwrap();
    assert_eq!(inv.args, ["read", "claude/sample-project/review", "--json"]);
    let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(input, serde_json::json!({"run_id":"run-1","conversation_id":"conv-1","reply_id":"rep-1","attention_signature":null}));
    let applied = br#"{"ok":true,"applied":{"reply_id":true,"attention_signature":false},"read":{"conversation_id":"conv-1"}}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 0, stdout: applied.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
    let gone = br#"{"ok":false,"error":"not_found"}"#;
    assert!(matches!(classify(&t, &a, &inv, Exit::Code { code: 1, stdout: gone.to_vec(), stderr: String::new() }), Outcome::Done { .. }));
    assert_eq!(classify(&t, &a, &inv, Exit::TimedOut), Outcome::Failed { detail: "Saving read state timed out.".into() });
}

#[test]
fn writes_succeed_on_exit_zero_and_report_stderr_otherwise() {
    let t = target("mac");
    let inv = invocation(&t, &Action::Terminate, "unused").unwrap();
    assert_eq!(inv.args, ["@mac", "terminate", "claude/sample-project/review", "--expected-run-id", "run-1"]);
    assert_eq!(inv.timeout, std::time::Duration::from_secs(60));
    assert!(matches!(classify(&t, &Action::Terminate, &inv, Exit::Code { code: 0, stdout: vec![], stderr: String::new() }), Outcome::Done { .. }));
    assert_eq!(classify(&t, &Action::Terminate, &inv, Exit::Code { code: 1, stdout: vec![], stderr: " hgs: no such session \n".into() }),
               Outcome::Failed { detail: "hgs: no such session".into() });
    assert_eq!(classify(&t, &Action::Terminate, &inv, Exit::TimedOut), Outcome::Failed { detail: "timed out".into() });
    let rename = invocation(&target(""), &Action::Rename { new_name: "review-2".into() }, "unused").unwrap();
    assert_eq!(rename.args, ["rename", "claude/sample-project/review", "review-2"]);
    assert_eq!(invocation(&target(""), &Action::Resume, "u").unwrap().args, ["resume", "claude/sample-project/review", "-d"]);
}

#[test]
fn rename_uses_the_cli_name_rule() {
    for bad in ["", "a b", "a/b", "a=b", "#a"] {
        assert!(invocation(&target(""), &Action::Rename { new_name: bad.into() }, "u").is_err(), "{bad:?}");
    }
}
```

and in `view.rs` tests: a row with `"allowed_actions":["rename"]`, `"attention_signature":"sig"`, `"read":{"conversation_id":"conv-1","reply_id":"rep-0"}` projects `allowed_actions == Some(vec!["rename"])`, `shared_read == true`, `read_reply_id == Some("rep-0")`; a row whose `read.conversation_id` differs leaves `read_reply_id == None`; `allowed(&row, "pause")` is false for that row and true for a row without `allowed_actions` whose agent is `claude`; an `opencode` row without `allowed_actions` allows only `rename` and `forget`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core actions::`
Expected: compile errors (`invocation`, `classify` not found).

- [ ] **Step 3: Implement `actions.rs`** with these rules (one `match` per function; no other behaviour):
  - `invocation`: prepend `@{computer}` when `computer` is not empty; build the table's arguments. The rename name rule is `src/config.rs::check_name`: non-empty, no `/`, no `=`, no leading `#`, no whitespace, with the refusal text "Use a name without spaces, slashes or '='." Send refusals in Qt's order: empty name or run id, then the size limit, then empty text. JSON input is compact (`serde_json::to_vec`), and `request_id` is the passed id for send and interrupt.
  - `classify` for Send: `StartFailed(e)` gives `Failed("Cannot start hgs: {e}")`. `WriteFailed` and `TimedOut` give Uncertain with the texts in the tests, and so does `Abnormal`. `Code{code != 0}` is `Uncertain` when `code != 1` or stderr contains "delivery uncertain" (case-insensitive); otherwise it is `Failed(stderr.trim())`, or `Failed("Message transfer failed (exit {code}).")` with no stderr. `Code{0}` is `Done` only when stdout parses as an object with `status == "submitted"`, `request_id == request_id`, `agent_id` absent or `""`, `name == target.name`, `run_id == target.run_id`, `conversation_id` a string equal to `target.conversation_id`, and `first_message == true` when `target.conversation_id` is empty. Anything else is Uncertain("Delivery acknowledgement is invalid. Inspect Terminal before retrying.").
  - `classify` for Interrupt, as `runSessionAction` (non-scoped):
    - `TimedOut` gives Failed("Request timed out. Check Terminal before retrying.").
    - `WriteFailed` gives Failed("Transfer failed. Check Terminal before retrying.").
    - A non-zero exit gives Failed(stderr trimmed, at most 2000 chars), or "Could not complete the request. Check Terminal." with no stderr.
    - Exit 0 is Done when `request_id`, `name`, `run_id == expected_run_id`, `conversation_id == expected_conversation_id` and `status == "submitted"` all match. Otherwise it gives Failed("Invalid acknowledgement. Check Terminal before retrying.").
  - `classify` for MarkRead, as `requestRead`:
    - Done when exit 0 with `ok == true`, `applied.reply_id` and `applied.attention_signature` booleans, and `read` null or with `read.conversation_id == conversation_id`.
    - Done when exit 1 with exactly `{"ok":false,"error":"not_found"}`.
    - `TimedOut` gives Failed("Saving read state timed out.").
    - Otherwise Failed(stderr, or "Could not save read state on that machine.").
  - `classify` for Pause, Resume, Rename, Archive and Terminate (`runWrite`): Done on exit 0. Otherwise:
    - `TimedOut` gives Failed("timed out").
    - `StartFailed(e)` or `Abnormal` gives Failed(the error text).
    - A non-zero exit gives Failed(stderr trimmed).
  - `allowed`: `allowed_actions` when present, else `agent != "opencode" || operation in ["rename","forget"]`.

- [ ] **Step 4: Run the tests**

Run: `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core && cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target/client-core --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/actions.rs client/core/src/lib.rs client/core/src/view.rs
git commit -m "feat(client): guarded action requests and outcome rules"
```

---

### Task 2: Running one action (`client/core/src/run.rs`) against a fake `hgs`

**Files:**
- Create: `client/core/src/run.rs`
- Create: `client/core/tests/support/fake_hgs_actions.py`
- Create: `client/core/tests/actions.rs`
- Modify: `client/core/src/lib.rs` (`pub mod run; pub use run::run;`)

**Interfaces:**
- Consumes: `Invocation`, `Exit` (Task 1), `default_hgs()` (phase 1 `lib.rs`).
- Produces: `pub fn run(hgs: &std::path::Path, invocation: &Invocation) -> Exit` (blocking; returns within `invocation.timeout` plus 2 s).

- [ ] **Step 1: Write the fake `hgs`** (`client/core/tests/support/fake_hgs_actions.py`, stdlib only, never the real CLI):

```python
#!/usr/bin/env python3
"""A fake `hgs` for the client action tests. Reads `config.json` beside itself:
{"reply": {"exit": 0, "stdout": <object or "echo_receipt">, "stderr": "", "sleep": 0}}
Appends {"argv": [...], "stdin": "..."} to `log.ndjson` beside itself."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))["reply"]
data = sys.stdin.read() if not sys.stdin.isatty() else ""
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": sys.argv[1:], "stdin": data}) + "\n")
time.sleep(config.get("sleep", 0))
out = config.get("stdout")
if out == "echo_receipt":
    request = json.loads(data)
    args = [a for a in sys.argv[1:] if not a.startswith("@")]
    out = {"status": "submitted", "request_id": request["request_id"], "agent_id": "",
           "name": args[1], "run_id": request["expected_run_id"],
           "conversation_id": request["expected_conversation_id"]}
if out is not None:
    sys.stdout.write(json.dumps(out))
sys.stderr.write(config.get("stderr", ""))
sys.exit(config.get("exit", 0))
```

- [ ] **Step 2: Write the failing integration tests** (`client/core/tests/actions.rs`). Copy the script into a fresh temp folder as `hgs` (mode 0755) with a `config.json`, following the pattern in `client/core/tests/common` used by the phase 1 stream tests:

```rust
#[test]
fn a_send_through_the_fake_is_done_and_logged_once() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stdout":"echo_receipt"}}"#);
    let t = Target { computer: String::new(), name: "claude/sample-project/review".into(), run_id: "run-1".into(), conversation_id: "conv-1".into() };
    let a = Action::Send { text: "hello".into() };
    let inv = invocation(&t, &a, "req-1").unwrap();
    let outcome = classify(&t, &a, &inv, run(&fake.hgs, &inv));
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
    let log = fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["argv"], serde_json::json!(["send", "claude/sample-project/review", "--json"]));
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
    let exit = run_with_env(&fake.hgs, &inv, &[("HGS_SESSION", "x"), ("HGS_RUN_ID", "y")]);
    assert!(matches!(exit, Exit::Code { code: 0, .. }));
    assert_eq!(fake.log()[0]["env"], serde_json::json!({"HGS_SESSION": null, "HGS_RUN_ID": null}));
}
```

`run_with_env(hgs, inv, parent_env)` is `run` with extra variables set on the child before the two `env_remove` calls (it proves the removal wins); `run` calls it with an empty slice. Extend the fake: when `log_env` is set, the log line also holds `"env": {k: os.environ.get(k) for k in config["log_env"]}`. `Fake` and `target()` live in the test file: `Fake::new(config)` makes a fresh folder under `std::env::temp_dir()` named with the process id and a counter, copies `tests/support/fake_hgs_actions.py` there as `hgs` with mode 0755, writes `config.json`, and `log()` parses `log.ndjson` line by line.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core --test actions`
Expected: compile error (`run` not found).

- [ ] **Step 4: Implement `run.rs`**: `std::process::Command::new(hgs).args(&inv.args)` with `.env_remove("HGS_SESSION").env_remove("HGS_RUN_ID")` (as `stream.rs:152`), piped stdin, stdout and stderr. Spawn errors map to `StartFailed(e.to_string())`. Write `stdin` in full and close it; a write error maps to `WriteFailed` and kills the child. Read stdout and stderr on two threads so a full pipe cannot block. Poll `try_wait` every 20 ms until `inv.timeout`, then `kill()`, `wait()` and return `TimedOut`. A signal exit maps to `Abnormal { stderr }`; otherwise return `Code { code, stdout, stderr }` with stderr capped at 2000 chars.

- [ ] **Step 5: Run the tests**

Run: `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add client/core/src/run.rs client/core/src/lib.rs client/core/tests/actions.rs client/core/tests/support/fake_hgs_actions.py
git commit -m "feat(client): run one guarded hgs action with a deadline"
```

---

### Task 3: Durable read marks (`client/core/src/readmarks.rs`)

**Files:**
- Create: `client/core/src/readmarks.rs`
- Modify: `client/core/src/lib.rs` (`pub mod readmarks; pub use readmarks::{ReadMark, ReadMarks};`)

**Interfaces:**
- Consumes: `ViewRow` fields from Task 1.
- Produces:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadMark { pub computer: String, pub name: String, pub run_id: String, pub conversation_id: String,
                      pub reply_id: Option<String>, pub attention_signature: Option<String> }
pub struct ReadMarks { /* path + marks keyed by (computer, name) */ }
impl ReadMarks {
    pub fn load(path: std::path::PathBuf) -> Self;            // missing or unreadable file: empty
    pub fn add(&mut self, mark: ReadMark);                     // ReadMark::merged rule, then save
    pub fn pending(&self) -> Vec<ReadMark>;
    /// Drops marks the host's row already shows (read_reply_id / read_attention_signature equal,
    /// or a different run or conversation); returns true when anything changed (then saves).
    pub fn settle(&mut self, computer: &str, row: &ViewRow) -> bool;
}
```

- [ ] **Step 1: Write the failing tests** (unit tests in `readmarks.rs`, temp file under `std::env::temp_dir()` with the process id in the name):
  - `a_newer_mark_keeps_the_field_it_does_not_set`: add a mark with `reply_id: Some("r1")`, then the same run with `reply_id: None, attention_signature: Some("s1")`; `pending()` holds one mark with both `r1` and `s1` (the `ReadMark::merged` rule in `tray/src/ReadMarks.h:41-48`).
  - `a_mark_for_another_run_replaces_the_old_one`.
  - `marks_survive_a_restart`: `add`, then `ReadMarks::load(same path).pending()` returns it.
  - `the_host_snapshot_settles_a_mark`: a row with the same run and `read_reply_id == Some("r1")` settles the `r1` mark; a row with a new `run_id` drops it too; a row still showing another reply keeps it.
  - `an_unreadable_file_starts_empty`: write `not json` to the path, `load` gives no marks and does not panic.

- [ ] **Step 2: Run** `cargo +1.99.1 test --manifest-path client/core/Cargo.toml --target-dir target/client-core readmarks::`. Expected: FAIL (module missing).

- [ ] **Step 3: Implement**: a `BTreeMap<(String, String), ReadMark>` saved as JSON to `path` through a temporary file in the same folder plus `std::fs::rename`, so a crash never leaves half a file. No size bound is needed beyond one mark per session, because the catalog bounds the sessions.

- [ ] **Step 4: Run the tests.** Expected: PASS, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add client/core/src/readmarks.rs client/core/src/lib.rs
git commit -m "feat(client): durable read marks until the host shows them"
```

---

### Task 4: Tauri commands with the in-flight guard (`client/src-tauri/src/actions.rs`)

**Files:**
- Create: `client/src-tauri/src/actions.rs`
- Modify: `client/src-tauri/src/lib.rs` (module, `.manage(Actions::new(app_data_dir.join("read-marks.json")))`, add `run_action`, `pending_read_marks` to `invoke_handler`)
- Modify: `client/src-tauri/src/commands.rs` (after each applied batch row upsert, call `actions.settle(computer, &row)`; the settle needs the computer id and the row the pump already has)

**Interfaces:**
- Consumes: `invocation`, `classify`, `run`, `ReadMarks`, `default_hgs()`.
- Produces (frontend contract, JSON snake_case):

```rust
#[tauri::command]
pub async fn run_action(state: tauri::State<'_, Actions>, target: Target, action: Action) -> Result<Outcome, String>;
// Err only for "already in progress" (string "busy"); every hgs result is an Outcome.
#[tauri::command]
pub fn pending_read_marks(state: tauri::State<'_, Actions>) -> Vec<ReadMark>;
```

- [ ] **Step 1: Write the failing tests** (`#[cfg(test)]` in `actions.rs`, no Tauri runtime: test the guard type `InFlight` directly):

```rust
#[test]
fn one_action_per_session_and_kind() {
    let flight = InFlight::default();
    let key = ("".to_string(), "claude/sample-project/review".to_string(), "send");
    let first = flight.claim(key.clone()).unwrap();
    assert!(flight.claim(key.clone()).is_none());
    assert!(flight.claim(("".into(), "claude/sample-project/review".into(), "pause")).is_some());
    drop(first);
    assert!(flight.claim(key).is_some());
}
```

- [ ] **Step 2: Run** `cargo +1.99.1 test --locked` in `client/src-tauri`. Expected: FAIL (`InFlight` missing).

- [ ] **Step 3: Implement**:
  - `InFlight` is a `Mutex<HashSet<(String, String, &'static str)>>` whose `claim` returns a guard that removes the key on drop.
  - `run_action` claims `(target.computer, target.name, kind)`, generates `request_id` with `uuid::Uuid::new_v4()`, and builds the invocation. A refusal is returned as `Outcome::Failed { detail }`. Then it runs `run` on `tauri::async_runtime::spawn_blocking` and classifies.
  - For `MarkRead` it first `add`s the mark to `ReadMarks`, then sends. A `Done` outcome is left in place until `settle` drops it, and a failed one stays pending for the next retry. The retry is driven by Task 8's queue, at most once per catalog change per session, and never faster than once per 5 s.
  - Send's in-flight refusal text is "A message to this session is already being sent." (Qt). Other kinds return `Err("busy")`, which the frontend ignores.
  - Add `uuid = { version = "1", features = ["v4"] }` to `client/src-tauri/Cargo.toml` if it is not already a dependency (check `cargo tree -i uuid` first and reuse the existing version).

- [ ] **Step 4: Run** `cargo +1.99.1 test --locked` and `cargo +1.99.1 clippy --locked --all-targets -- -D warnings` in `client/src-tauri`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/actions.rs client/src-tauri/src/lib.rs client/src-tauri/src/commands.rs client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock
git commit -m "feat(client): run_action with one action per session and kind"
```

---

### Task 5: Frontend action store and notices (`client/src/store/actions.ts`)

**Files:**
- Create: `client/src/store/actions.ts`
- Create: `client/src/store/actions.test.ts`
- Modify: `client/src/types.ts` (the new `ViewRow` fields)
- Modify: `client/src/bridge.ts` (typed `runAction(target, action)` and `pendingReadMarks()` wrappers; a browser stub that resolves `{outcome:"failed", detail:"Actions need the desktop app."}`)

**Interfaces:**
- Consumes: `run_action`, `pending_read_marks` (Task 4); `RowOut` and `ViewRow` from `client/src/types.ts`, which this task extends with Task 1's new fields (`state`, `tracked`, `reply_id`, `attention_signature`, `shared_read`, `read_reply_id`, `read_attention_signature`, `allowed_actions`, `action_reasons`).
- Produces:

```ts
export type Action =
  | { kind: "send"; text: string } | { kind: "interrupt"; turn_started: number }
  | { kind: "mark_read"; reply_id: string | null; attention_signature: string | null }
  | { kind: "pause" } | { kind: "resume" } | { kind: "rename"; new_name: string }
  | { kind: "archive" } | { kind: "terminate" };
export type Outcome = { outcome: "done"; receipt: unknown } | { outcome: "failed"; detail: string } | { outcome: "uncertain"; detail: string };
export interface Target { computer: string; name: string; run_id: string; conversation_id: string }
export function targetOf(entry: RowOut): Target | undefined;   // undefined without a run id
export function pending(computer: string, name: string, kind: Action["kind"]): boolean;  // reactive
export async function perform(target: Target, action: Action): Promise<Outcome>;  // sets pending, shows notice
export function allowed(row: ViewRow, operation: string): boolean;   // same rule as core::actions::allowed
export const notices: () => Notice[];  // { id, tone: "error" | "warn" | "info", text }
```

- [ ] **Step 1: Write the failing vitest tests** (`actions.test.ts`, with `bridge` mocked through `vi.mock("../bridge")`):
  - `perform` marks the action pending until the promise settles and clears it after.
  - A `failed` outcome adds an `error` notice with the detail text unchanged. An `uncertain` outcome adds a `warn` notice with the detail and never calls `runAction` a second time; assert the mock was called exactly once after 6 s of fake timers.
  - `targetOf` returns `undefined` for a row without `run_id`, and the computer `""` for this computer.
  - `allowed` matches the core rule: reported `allowed_actions` win, and `opencode` without them allows only `rename` and `forget`.

- [ ] **Step 2: Run** `npx vitest run src/store/actions.test.ts` in `client`. Expected: FAIL.

- [ ] **Step 3: Implement** with Solid signals: `createStore<Record<string, true>>` keyed `${computer}\n${name}\n${kind}`, and a notices list capped at 5 entries. `info` notices disappear after 6 s; `error` and `warn` notices stay until dismissed. Notice texts come from Rust unchanged. Success notices: "Paused", "Resumed", "Renamed", "Moved to archive", "Terminated"; send and mark read show no success notice. The notice copy has no dot separators.

- [ ] **Step 4: Run** `npx vitest run` and `npm run check`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/store/actions.ts client/src/store/actions.test.ts client/src/bridge.ts
git commit -m "feat(client): action store with outcome notices and no automatic retry"
```

---

### Task 6: Menu entries, Stop, Mark as read and the dialogs

**Files:**
- Create: `client/src/ui/sessions/ActionDialogs.tsx`
- Modify: `client/src/ui/sessions/SessionList.tsx` (menu entries at the lines with `disabled: true` for Rename, Pause session, Move to archive, Terminate session)
- Modify: `client/src/ui/sessions/SessionDetail.tsx` (Stop button while a turn runs; Mark as read chip as in the mockup's Activity toolbar)
- Modify: `client/src/ui/components/Overlays.tsx` if the dialog host needs a slot (follow the existing menu overlay pattern)

**Interfaces:**
- Consumes: `perform`, `pending`, `allowed`, `targetOf` (Task 5); catalog rows.
- Produces: `TerminateConfirm(props: { entry: RowOut; onClose(): void })` and `RenameDialog(props: { entry: RowOut; onClose(): void })`.

Behaviour (from `SessionsWindow.cpp` around the `terminateSessionConfirm` dialog and the session menu):

| Entry | Enabled when | Runs |
|---|---|---|
| Pause session / Resume session | computer online, `allowed(row,"pause")` / `"resume"`, state `running` / not running and not archived | `pause` / `resume` |
| Rename… (F2) | `allowed(row,"rename")`, not pending | dialog, then `rename` |
| Move to archive | `allowed(row,"archive")`, state not `archived` | `archive` |
| Terminate session… | state `running`, `allowed(row,"terminate")` | confirmation, then `terminate` |
| Stop (detail header) | `turn_started` set, `allowed(row,"interrupt")` | `interrupt` with `turn_started` |
| Mark as read (menu and chip) | the row is unread or has attention | `mark_read` with `reply_id` and `attention_signature` from the row |

Terminate confirmation, as in Qt:
- **Title:** "Terminate session?".
- **Text** when the session is tracked and its agent is `codex`, `claude` or `kimi` (keep archive): "Stop {name} and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept." Otherwise: "Terminate {name}? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept." (`SessionsWindow.cpp:5772`).
- **Detail:** "Computer: {machine}".
- **Buttons:** "Stop and archive" or "Terminate session", and Cancel. Cancel has focus and is the default, and Escape cancels.
- There is no Remove-worktree checkbox in this phase.

Rename dialog: a text field with the current short name selected, and Rename and Cancel buttons. Rename is disabled while the name breaks the CLI rule (non-empty, no space, `/` or `=`, no leading `#`); under the field it says "Use a name without spaces, slashes or '='."

- [ ] **Step 1: Write the failing vitest tests** for pure helpers in a new `client/src/ui/sessions/actions-ui.ts`: `menuState(row, online) -> { pause, resume, rename, archive, terminate, stop, markRead }` booleans per the table, and `terminateCopy(row) -> { title, text, confirm }` returning the two Qt variants. Fixture rows are synthetic (`claude/sample-project/review`, `/home/example/sample-project`).

- [ ] **Step 2: Run** `npx vitest run src/ui/sessions` and expect FAIL.

- [ ] **Step 3: Implement** the helpers and the components; wire the entries. Before running a confirmed or renamed action, re-read the row by `(computer, key)` from the catalog. If it is gone or its `run_id` differs from the one the dialog opened with, show the info notice "This session changed. Open its actions again." and do nothing (Qt `selectSessionEntry(original)`).

- [ ] **Step 4: Run** `npx vitest run`, `npm run check`, `python3 ../scripts/check-ui-separators.py`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/ui/sessions
git commit -m "feat(client): session actions with Qt's confirmations"
```

---

### Task 7: Text composer and send

**Files:**
- Create: `client/src/ui/sessions/Composer.tsx` (port of `design/webui/src/screens/sessions/Composer.tsx` without attachments, voice, video and the resize grip's persistence beyond what the mockup has)
- Modify: `client/src/ui/sessions/SessionDetail.tsx` (mount the composer under Activity)
- Test: `client/src/ui/sessions/composer.test.ts`

**Interfaces:**
- Consumes: `perform`, `pending`, `targetOf` (Task 5).
- Produces: `Composer(props: { entry: RowOut })`.

Rules:
- Enter sends; Shift+Enter inserts a newline. There is no visible hint text.
- The send button is disabled while `pending(…,"send")`, while the trimmed text is empty, or while the row has no run id. The draft is kept per `(computer, name)` in memory.
- `done` clears the draft. `failed` and `uncertain` keep the draft and show the notice; the user decides whether to send again, and a new attempt gets a new request id, as in Qt.
- Text over 64 KiB disables Send and shows "Use at most 64 KiB of text and 8 attachments." under the field.

- [ ] **Step 1: Write the failing vitest tests** for a pure `composerState(text, row, sending) -> { canSend, hint }` and for `onKey(event)` returning `"send" | "newline" | null` (Enter without Shift, Shift+Enter, IME composition (`isComposing`) returns null).
- [ ] **Step 2: Run** `npx vitest run src/ui/sessions/composer.test.ts` and expect FAIL.
- [ ] **Step 3: Implement** the helpers and the component with the mockup's classes and skin variables (`design/webui` `cmp-*` classes) so the approved look holds.
- [ ] **Step 4: Run** `npx vitest run`, `npm run check`, the separator check. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add client/src/ui/sessions/Composer.tsx client/src/ui/sessions/composer.test.ts client/src/ui/sessions/SessionDetail.tsx
git commit -m "feat(client): text composer that sends through the guarded path"
```

---

### Task 8: Documentation, read-mark retry and the end-to-end check

**Files:**
- Modify: `client/README.md` ("Actions" section: the table from Task 1, the outcome rules, "unknown outcomes are never replayed", where read marks are stored)
- Modify: `client/src/store/actions.ts` (read-mark retry on catalog changes)
- Modify: `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md` (phase table: phase 2 links this plan)

- [ ] **Step 1: Write the failing vitest test**. On a catalog batch whose row still shows a pending mark unread, the store calls `perform(target, mark_read)` once for that session. A second batch within 5 s does not call it again, and a batch where the row shows it read calls nothing.
- [ ] **Step 2: Implement** the retry. It reads `pendingReadMarks()` at start and after each mark, keeps a per-session last-attempt time, retries at most once per 5 s per session, and only on a catalog change.
- [ ] **Step 3: End-to-end with the fake hgs**. Set `ZERUS_CLIENT_HGS` to a copy of `fake_hgs_actions.py` with `echo_receipt` and run the app headless on the replay fixtures with the auto-perf hooks off. A `ZERUS_CLIENT_QUERY=actions-smoke=1` hook in `PerfHost.tsx` (kept, documented in `client/README.md` beside the perf hooks) makes the app send one message, pause, rename and terminate the selected replay session, capture a headless screenshot of the Terminate confirmation to the scratch folder (never committed), then exit. Afterwards the fake's `log.ndjson` holds exactly one call per action, with the expected argument lists and inputs. Record the command and the log lines in the PR description.
- [ ] **Step 4: Run all checks**: `cargo +1.99.1 test` (core and src-tauri), clippy with `-D warnings`, `npx vitest run`, `npm run check`, `python3 scripts/check-ui-separators.py`, `python3 scripts/check-source-privacy.py`.
- [ ] **Step 5: Commit**

```bash
git add client docs/superpowers/plans
git commit -m "docs(client): guarded actions, read marks and their outcome rules"
```

---

## Out of scope (later phases)

Remove-worktree cleanup after Terminate, Forget saved/archived sessions, Fork, Clear and Compact context, queued-message removal, answering questions, attachments, voice and video, and batch actions over a multi-selection; they keep their Qt behaviour and arrive with `zerus-5wmh.9` and `zerus-5wmh.10`.
