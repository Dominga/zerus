//! Guarded session actions: the `hgs` calls the Qt client makes
//! (`tray/src/HgsClient.cpp`) and the rules that turn their exits into outcomes.
use crate::view::ViewRow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const CLEANUP_PREFIX: &str = "ZERUS_WORKTREE_CLEANUP ";
const STDERR_CHARS: usize = 2000;

/// The session an action applies to; `computer` is "" for this computer, else
/// the SSH alias.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub computer: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Interrupt {
        turn_started: f64,
    },
    MarkRead {
        reply_id: Option<String>,
        attention_signature: Option<String>,
    },
    Pause,
    Resume,
    Rename {
        new_name: String,
        archive_id: Option<String>,
    },
    Archive,
    Terminate,
    /// `hgs kill NAME`: Terminate for a live session without an HGS archive
    /// (`killSession`; `SessionsWindow::terminateSession` picks it).
    Kill,
}

/// One `hgs` call: arguments (with a leading `@alias` for a peer), standard
/// input, the time it may take and, for Interrupt, the request identity.
#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub request_id: Option<String>,
}

/// How a call ended, as the runner observed it.
#[derive(Clone, Debug, PartialEq)]
pub enum Exit {
    Code {
        code: i32,
        stdout: Vec<u8>,
        stderr: String,
    },
    Abnormal {
        stderr: String,
    },
    TimedOut,
    StartFailed(String),
    WriteFailed,
}

/// What `hgs` reported about the worktree of an archived or terminated session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorktreeCleanup {
    pub status: String,
    pub path: String,
    pub common_dir: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Done {
        receipt: Value,
        cleanup: Option<WorktreeCleanup>,
    },
    Failed {
        detail: String,
    },
}

fn failed(detail: impl Into<String>) -> Outcome {
    Outcome::Failed {
        detail: detail.into(),
    }
}

fn done(receipt: Value, cleanup: Option<WorktreeCleanup>) -> Outcome {
    Outcome::Done { receipt, cleanup }
}

/// `SessionTag::problem` (`tray/src/SessionTag.h`): why a session tag is not
/// accepted, None when it is. `hgs rename` enforces the same rule.
pub fn tag_problem(tag: &str) -> Option<&'static str> {
    if tag.is_empty() {
        return Some("Enter a session name.");
    }
    if tag.trim() != tag {
        return Some("Remove spaces at the beginning or end.");
    }
    if tag
        .chars()
        .any(|c| matches!(c, '/' | ':' | '.') || c.is_control())
    {
        return Some("Names cannot contain /, : or . or control characters.");
    }
    None
}

/// The rename the Qt dialog makes: the agent and project of `old` stay, only
/// the tag after them changes (`src/state/rename.rs` refuses anything else).
fn rename_problem(old: &str, new: &str) -> Option<&'static str> {
    let mut parts = old.splitn(3, '/');
    let (agent, project) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let tag = (!agent.is_empty() && !project.is_empty())
        .then(|| {
            new.strip_prefix(agent)?
                .strip_prefix('/')?
                .strip_prefix(project)?
                .strip_prefix('/')
        })
        .flatten();
    match tag {
        Some(tag) => tag_problem(tag),
        None => Some("rename must keep the same agent/project and use a nonempty tag without '/', ':', '.', control characters or surrounding whitespace"),
    }
}

pub fn invocation(
    target: &Target,
    action: &Action,
    request_id: &str,
) -> Result<Invocation, String> {
    let name = target.name.clone();
    let (mut args, stdin, local, peer, request) = match action {
        Action::Interrupt { turn_started } => {
            let input = json!({
                "request_id": request_id,
                "expected_run_id": target.run_id,
                "expected_conversation_id": target.conversation_id,
                "expected_turn_started": turn_started,
            });
            (
                vec!["interrupt".to_owned(), name, "--json".to_owned()],
                Some(serde_json::to_vec(&input).map_err(|error| error.to_string())?),
                15,
                45,
                Some(request_id.to_owned()),
            )
        }
        Action::MarkRead {
            reply_id,
            attention_signature,
        } => {
            let input = json!({
                "run_id": target.run_id,
                "conversation_id": target.conversation_id,
                "reply_id": reply_id,
                "attention_signature": attention_signature,
            });
            (
                vec!["read".to_owned(), name, "--json".to_owned()],
                Some(serde_json::to_vec(&input).map_err(|error| error.to_string())?),
                15,
                35,
                None,
            )
        }
        Action::Pause => (vec!["pause".to_owned(), name], None, 45, 45, None),
        Action::Resume => (
            vec!["resume".to_owned(), name, "-d".to_owned()],
            None,
            30,
            30,
            None,
        ),
        Action::Rename {
            new_name,
            archive_id,
        } => {
            if let Some(problem) = rename_problem(&name, new_name) {
                return Err(problem.into());
            }
            let mut args = vec!["rename".to_owned(), name, new_name.clone()];
            if let Some(id) = archive_id.as_ref().filter(|id| !id.is_empty()) {
                args.extend(["--archive".to_owned(), id.clone()]);
            }
            (args, None, 5, 15, None)
        }
        Action::Archive => (vec!["archive".to_owned(), name], None, 30, 45, None),
        Action::Terminate => {
            let mut args = vec!["terminate".to_owned(), name];
            if !target.run_id.is_empty() {
                args.extend(["--expected-run-id".to_owned(), target.run_id.clone()]);
            }
            (args, None, 45, 60, None)
        }
        // A local kill is a tmux kill-session; a peer's goes through SSH.
        Action::Kill => (vec!["kill".to_owned(), name], None, 5, 15, None),
    };
    let peer_call = prepend_alias(&mut args, &target.computer);
    Ok(Invocation {
        args,
        stdin,
        timeout: Duration::from_secs(if peer_call { peer } else { local }),
        request_id: request,
    })
}

/// Puts `@alias` first for a peer; true when `computer` is a peer.
pub(crate) fn prepend_alias(args: &mut Vec<String>, computer: &str) -> bool {
    let peer = !computer.is_empty();
    if peer {
        args.insert(0, format!("@{computer}"));
    }
    peer
}

fn trimmed_stderr(stderr: &str) -> Option<String> {
    let text = stderr.trim();
    (!text.is_empty()).then(|| text.chars().take(STDERR_CHARS).collect())
}

pub fn classify(target: &Target, action: &Action, invocation: &Invocation, exit: Exit) -> Outcome {
    match action {
        Action::Interrupt { .. } => classify_interrupt(target, invocation, exit),
        Action::MarkRead { .. } => classify_read(target, exit),
        Action::Archive | Action::Terminate => classify_write(exit, true),
        _ => classify_write(exit, false),
    }
}

/// `runSessionAction`: the receipt must echo this exact request and session.
fn classify_interrupt(target: &Target, invocation: &Invocation, exit: Exit) -> Outcome {
    match exit {
        Exit::TimedOut => failed("Request timed out. Check Terminal before retrying."),
        Exit::WriteFailed => failed("Transfer failed. Check Terminal before retrying."),
        Exit::StartFailed(error) => failed(error),
        Exit::Abnormal { stderr } => failed(
            trimmed_stderr(&stderr)
                .unwrap_or_else(|| "Could not complete the request. Check Terminal.".to_owned()),
        ),
        Exit::Code { code, stderr, .. } if code != 0 => failed(
            trimmed_stderr(&stderr)
                .unwrap_or_else(|| "Could not complete the request. Check Terminal.".to_owned()),
        ),
        Exit::Code { stdout, .. } => {
            let receipt: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
            let field = |key: &str| receipt.get(key).and_then(Value::as_str);
            let matches = field("status") == Some("submitted")
                && invocation.request_id.is_some()
                && field("request_id") == invocation.request_id.as_deref()
                && field("name") == Some(target.name.as_str())
                && field("run_id") == Some(target.run_id.as_str())
                && field("conversation_id") == Some(target.conversation_id.as_str());
            if matches {
                done(receipt, None)
            } else {
                failed("Invalid acknowledgement. Check Terminal before retrying.")
            }
        }
    }
}

/// `requestRead`: applied marks, or a session that is gone (nothing to mark).
fn classify_read(target: &Target, exit: Exit) -> Outcome {
    const FALLBACK: &str = "Could not save read state on that machine.";
    match exit {
        Exit::TimedOut => failed("Saving read state timed out."),
        Exit::Code {
            code: 0, stdout, ..
        } => {
            let reply: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
            let applied = |key: &str| {
                reply
                    .get("applied")
                    .and_then(|applied| applied.get(key))
                    .is_some_and(Value::is_boolean)
            };
            let read = reply.get("read");
            let read_ok = match read {
                None | Some(Value::Null) => true,
                Some(read) => {
                    read.get("conversation_id").and_then(Value::as_str)
                        == Some(target.conversation_id.as_str())
                }
            };
            if reply.get("ok") == Some(&Value::Bool(true))
                && applied("reply_id")
                && applied("attention_signature")
                && read_ok
            {
                done(reply, None)
            } else {
                failed(FALLBACK)
            }
        }
        Exit::Code {
            code: 1,
            stdout,
            stderr,
        } => {
            let reply: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
            if reply == json!({"ok": false, "error": "not_found"}) {
                done(reply, None)
            } else {
                failed(trimmed_stderr(&stderr).unwrap_or_else(|| FALLBACK.to_owned()))
            }
        }
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } => {
            failed(trimmed_stderr(&stderr).unwrap_or_else(|| FALLBACK.to_owned()))
        }
        Exit::StartFailed(error) => failed(error),
        Exit::WriteFailed => failed(FALLBACK),
    }
}

/// `runWrite`: exit 0 is done; Archive and Terminate may report their worktree.
fn classify_write(exit: Exit, may_clean_up: bool) -> Outcome {
    match exit {
        Exit::Code {
            code: 0, stderr, ..
        } => done(
            Value::Null,
            if may_clean_up {
                cleanup_line(&stderr)
            } else {
                None
            },
        ),
        Exit::TimedOut => failed("timed out"),
        Exit::StartFailed(error) => failed(error),
        Exit::WriteFailed => failed("could not send the request"),
        Exit::Abnormal { stderr } => {
            failed(trimmed_stderr(&stderr).unwrap_or_else(|| "hgs ended abnormally".to_owned()))
        }
        Exit::Code { code, stderr, .. } => failed(
            trimmed_stderr(&stderr).unwrap_or_else(|| format!("hgs exited with code {code}")),
        ),
    }
}

/// The single worktree report line; anything else (none, several, a partial
/// object) is not trusted to name a worktree.
fn cleanup_line(stderr: &str) -> Option<WorktreeCleanup> {
    let mut lines = stderr
        .lines()
        .filter_map(|line| line.strip_prefix(CLEANUP_PREFIX));
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let cleanup: WorktreeCleanup = serde_json::from_str(line).ok()?;
    (!cleanup.path.is_empty() && !cleanup.common_dir.is_empty()).then_some(cleanup)
}

/// `NativeSessionCapabilities::action` without details: reported allowed
/// actions win; without them every operation is allowed except that an
/// "opencode" session only renames and forgets.
pub fn allowed(row: &ViewRow, operation: &str) -> bool {
    match &row.allowed_actions {
        Some(list) => list.iter().any(|action| action == operation),
        None => row.agent != "opencode" || matches!(operation, "rename" | "forget"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(computer: &str) -> Target {
        Target {
            computer: computer.into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        }
    }
    fn exit0(stdout: &[u8], stderr: &str) -> Exit {
        Exit::Code {
            code: 0,
            stdout: stdout.to_vec(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn calls_match_the_qt_client() {
        let t = target("");
        assert_eq!(
            invocation(&t, &Action::Pause, "u").unwrap().args,
            ["pause", "claude/sample-project/review"]
        );
        assert_eq!(
            invocation(&t, &Action::Resume, "u").unwrap().args,
            ["resume", "claude/sample-project/review", "-d"]
        );
        assert_eq!(
            invocation(&t, &Action::Archive, "u").unwrap().args,
            ["archive", "claude/sample-project/review"]
        );
        let rename = invocation(
            &t,
            &Action::Rename {
                new_name: "claude/sample-project/review-2".into(),
                archive_id: Some("a1".into()),
            },
            "u",
        )
        .unwrap();
        assert_eq!(
            rename.args,
            [
                "rename",
                "claude/sample-project/review",
                "claude/sample-project/review-2",
                "--archive",
                "a1"
            ]
        );
        let term = invocation(&target("mac"), &Action::Terminate, "u").unwrap();
        assert_eq!(
            term.args,
            [
                "@mac",
                "terminate",
                "claude/sample-project/review",
                "--expected-run-id",
                "run-1"
            ]
        );
        assert_eq!(term.timeout, std::time::Duration::from_secs(60));
        let mut no_run = target("");
        no_run.run_id.clear();
        assert_eq!(
            invocation(&no_run, &Action::Terminate, "u").unwrap().args,
            ["terminate", "claude/sample-project/review"]
        );
    }

    #[test]
    fn timeouts_differ_for_this_computer_and_a_peer() {
        let secs = |computer: &str, action: Action| {
            invocation(&target(computer), &action, "u")
                .unwrap()
                .timeout
                .as_secs()
        };
        let interrupt = Action::Interrupt { turn_started: 1.0 };
        let read = Action::MarkRead {
            reply_id: None,
            attention_signature: None,
        };
        let rename = || Action::Rename {
            new_name: "claude/sample-project/b".into(),
            archive_id: None,
        };
        assert_eq!(
            (secs("", interrupt.clone()), secs("p", interrupt)),
            (15, 45)
        );
        assert_eq!((secs("", read.clone()), secs("p", read)), (15, 35));
        assert_eq!(
            (secs("", Action::Pause), secs("p", Action::Pause)),
            (45, 45)
        );
        assert_eq!(
            (secs("", Action::Resume), secs("p", Action::Resume)),
            (30, 30)
        );
        assert_eq!((secs("", rename()), secs("p", rename())), (5, 15));
        assert_eq!(
            (secs("", Action::Archive), secs("p", Action::Archive)),
            (30, 45)
        );
        assert_eq!(
            (secs("", Action::Terminate), secs("p", Action::Terminate)),
            (45, 60)
        );
        assert_eq!((secs("", Action::Kill), secs("p", Action::Kill)), (5, 15));
    }

    #[test]
    fn kill_names_the_session_and_reports_no_worktree() {
        let inv = invocation(&target("mac"), &Action::Kill, "u").unwrap();
        assert_eq!(inv.args, ["@mac", "kill", "claude/sample-project/review"]);
        assert_eq!(inv.stdin, None);
        let line = "ZERUS_WORKTREE_CLEANUP {\"status\":\"removed\",\"path\":\"/home/example/wt\",\"common_dir\":\"/home/example/.git\"}\n";
        assert!(matches!(
            classify(&target(""), &Action::Kill, &inv, exit0(b"", line)),
            Outcome::Done { cleanup: None, .. }
        ));
        assert_eq!(
            classify(&target(""), &Action::Kill, &inv, Exit::TimedOut),
            Outcome::Failed {
                detail: "timed out".into()
            }
        );
    }

    #[test]
    fn interrupt_sends_its_identity_and_checks_the_receipt() {
        let t = target("");
        let a = Action::Interrupt {
            turn_started: 1793000000.5,
        };
        let inv = invocation(&t, &a, "req-2").unwrap();
        assert_eq!(
            inv.args,
            ["interrupt", "claude/sample-project/review", "--json"]
        );
        let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(
            input,
            serde_json::json!({"request_id":"req-2","expected_run_id":"run-1",
            "expected_conversation_id":"conv-1","expected_turn_started":1793000000.5})
        );
        let ok = br#"{"status":"submitted","request_id":"req-2","name":"claude/sample-project/review","run_id":"run-1","conversation_id":"conv-1"}"#;
        assert!(matches!(
            classify(&t, &a, &inv, exit0(ok, "")),
            Outcome::Done { .. }
        ));
        assert_eq!(
            classify(&t, &a, &inv, exit0(b"{}", "")),
            Outcome::Failed {
                detail: "Invalid acknowledgement. Check Terminal before retrying.".into()
            }
        );
        assert_eq!(
            classify(&t, &a, &inv, Exit::TimedOut),
            Outcome::Failed {
                detail: "Request timed out. Check Terminal before retrying.".into()
            }
        );
        assert_eq!(
            classify(&t, &a, &inv, Exit::WriteFailed),
            Outcome::Failed {
                detail: "Transfer failed. Check Terminal before retrying.".into()
            }
        );
        assert_eq!(
            classify(
                &t,
                &a,
                &inv,
                Exit::Code {
                    code: 1,
                    stdout: vec![],
                    stderr: String::new()
                }
            ),
            Outcome::Failed {
                detail: "Could not complete the request. Check Terminal.".into()
            }
        );
    }

    #[test]
    fn mark_read_accepts_applied_or_not_found() {
        let t = target("");
        let a = Action::MarkRead {
            reply_id: Some("rep-1".into()),
            attention_signature: None,
        };
        let inv = invocation(&t, &a, "u").unwrap();
        assert_eq!(inv.args, ["read", "claude/sample-project/review", "--json"]);
        let input: serde_json::Value = serde_json::from_slice(inv.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(
            input,
            serde_json::json!({"run_id":"run-1","conversation_id":"conv-1","reply_id":"rep-1","attention_signature":null})
        );
        let applied = br#"{"ok":true,"applied":{"reply_id":true,"attention_signature":false},"read":{"conversation_id":"conv-1"}}"#;
        assert!(matches!(
            classify(&t, &a, &inv, exit0(applied, "")),
            Outcome::Done { .. }
        ));
        let gone = br#"{"ok":false,"error":"not_found"}"#;
        assert!(matches!(
            classify(
                &t,
                &a,
                &inv,
                Exit::Code {
                    code: 1,
                    stdout: gone.to_vec(),
                    stderr: String::new()
                }
            ),
            Outcome::Done { .. }
        ));
        assert_eq!(
            classify(&t, &a, &inv, Exit::TimedOut),
            Outcome::Failed {
                detail: "Saving read state timed out.".into()
            }
        );
        assert_eq!(
            classify(&t, &a, &inv, exit0(b"{\"ok\":true}", "")),
            Outcome::Failed {
                detail: "Could not save read state on that machine.".into()
            }
        );
    }

    #[test]
    fn writes_succeed_on_exit_zero_and_report_stderr_otherwise() {
        let t = target("");
        let inv = invocation(&t, &Action::Pause, "u").unwrap();
        assert!(matches!(
            classify(&t, &Action::Pause, &inv, exit0(b"", "")),
            Outcome::Done { cleanup: None, .. }
        ));
        assert_eq!(
            classify(
                &t,
                &Action::Pause,
                &inv,
                Exit::Code {
                    code: 1,
                    stdout: vec![],
                    stderr: " hgs: no such session \n".into()
                }
            ),
            Outcome::Failed {
                detail: "hgs: no such session".into()
            }
        );
        assert_eq!(
            classify(&t, &Action::Pause, &inv, Exit::TimedOut),
            Outcome::Failed {
                detail: "timed out".into()
            }
        );
        assert_eq!(
            classify(
                &t,
                &Action::Pause,
                &inv,
                Exit::StartFailed("No such file or directory".into())
            ),
            Outcome::Failed {
                detail: "No such file or directory".into()
            }
        );
    }

    #[test]
    fn archive_and_terminate_carry_exactly_one_cleanup_line() {
        let t = target("");
        let inv = invocation(&t, &Action::Archive, "u").unwrap();
        let line = "ZERUS_WORKTREE_CLEANUP {\"status\":\"kept\",\"path\":\"/home/example/sample-project-wt\",\"common_dir\":\"/home/example/sample-project/.git\",\"reason\":\"unsaved work\"}\n";
        match classify(&t, &Action::Archive, &inv, exit0(b"", line)) {
            Outcome::Done {
                cleanup: Some(c), ..
            } => {
                assert_eq!(c.status, "kept");
                assert_eq!(c.reason.as_deref(), Some("unsaved work"));
            }
            other => panic!("{other:?}"),
        }
        let two = format!("{line}{line}");
        assert!(matches!(
            classify(&t, &Action::Archive, &inv, exit0(b"", &two)),
            Outcome::Done { cleanup: None, .. }
        ));
        let no_dir = "ZERUS_WORKTREE_CLEANUP {\"status\":\"removed\",\"path\":\"/home/example/wt\",\"common_dir\":\"\"}\n";
        assert!(matches!(
            classify(&t, &Action::Archive, &inv, exit0(b"", no_dir)),
            Outcome::Done { cleanup: None, .. }
        ));
        assert!(matches!(
            classify(
                &t,
                &Action::Pause,
                &invocation(&t, &Action::Pause, "u").unwrap(),
                exit0(b"", line)
            ),
            Outcome::Done { cleanup: None, .. }
        ));
    }

    #[test]
    fn rename_changes_only_the_tag_by_the_session_tag_rule() {
        let rename = |new_name: &str| {
            invocation(
                &target(""),
                &Action::Rename {
                    new_name: new_name.into(),
                    archive_id: None,
                },
                "u",
            )
            .map(|inv| inv.args)
        };
        assert_eq!(
            rename("claude/sample-project/release planning").unwrap()[2],
            "claude/sample-project/release planning"
        );
        for (bad, problem) in [
            ("claude/sample-project/", "Enter a session name."),
            (
                "claude/sample-project/ review",
                "Remove spaces at the beginning or end.",
            ),
            (
                "claude/sample-project/review ",
                "Remove spaces at the beginning or end.",
            ),
            (
                "claude/sample-project/a/b",
                "Names cannot contain /, : or . or control characters.",
            ),
            (
                "claude/sample-project/a:b",
                "Names cannot contain /, : or . or control characters.",
            ),
            (
                "claude/sample-project/v1.2",
                "Names cannot contain /, : or . or control characters.",
            ),
            (
                "claude/sample-project/a\u{7}b",
                "Names cannot contain /, : or . or control characters.",
            ),
        ] {
            assert_eq!(rename(bad), Err(problem.to_owned()), "{bad:?}");
        }
        for other in [
            "codex/sample-project/review-2",
            "claude/other-project/review-2",
            "claude/sample-project",
            "",
        ] {
            assert!(
                rename(other).unwrap_err().starts_with("rename must keep"),
                "{other:?}"
            );
        }
        assert_eq!(tag_problem("release planning"), None);
    }

    #[test]
    fn allowed_follows_reports_then_the_agent() {
        let listed = ViewRow {
            allowed_actions: Some(vec!["rename".into()]),
            agent: "claude".into(),
            ..ViewRow::default()
        };
        assert!(allowed(&listed, "rename"));
        assert!(!allowed(&listed, "pause"));
        let claude = ViewRow {
            agent: "claude".into(),
            ..ViewRow::default()
        };
        assert!(allowed(&claude, "pause"));
        let opencode = ViewRow {
            agent: "opencode".into(),
            ..ViewRow::default()
        };
        assert!(allowed(&opencode, "rename") && allowed(&opencode, "forget"));
        assert!(!allowed(&opencode, "pause"));
    }
}
