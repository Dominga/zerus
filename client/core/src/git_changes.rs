//! The inspector's Changes card, ported from the Qt client: a folder's
//! `hgs git-status` (its changed files with their added and deleted lines)
//! and its `hgs git-diff`.
//!
//! Every call blocks; callers run it off the UI thread and bound how many
//! run at once (src-tauri `composer::changes`).
//!
//! Reference: `tray/src/HgsClient.cpp` (`requestGitStatus`,
//! `storeGitStatus`, `pumpGitStatus`, `requestGitDiff`) and
//! `tray/src/SessionsWindow.cpp` (`showDiff`).
use crate::actions::{Exit, Invocation};
use crate::ComputerId;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The largest `git-status` reply; a larger one reads as unavailable.
pub const STATUS_LIMIT: usize = 32 * 1024;
/// The largest `git-diff` reply; a larger one reads as an empty object.
pub const DIFF_LIMIT: usize = 512 * 1024;

fn invocation(args: Vec<String>, seconds: u64) -> Invocation {
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(seconds),
        request_id: None,
    }
}

/// `hgs [@host] git-status --path P --json`: 25 s here and 30 s on a peer,
/// since git-status bounds each of its git reads and all of them together
/// take at most 19 s.
pub fn status_invocation(computer: &ComputerId, path: &str) -> Invocation {
    let args = crate::worktrees::hgs_args(computer, &["git-status", "--path", path, "--json"]);
    invocation(args, if computer.is_local() { 25 } else { 30 })
}

/// `hgs [@host] git-diff --path P --json`: 8 s here and 15 s on a peer.
pub fn diff_invocation(computer: &ComputerId, path: &str) -> Invocation {
    let args = crate::worktrees::hgs_args(computer, &["git-diff", "--path", path, "--json"]);
    invocation(args, if computer.is_local() { 8 } else { 15 })
}

/// Runs a call bounded at `limit`; as in Qt, a larger reply reads as an
/// empty object.
pub fn run(hgs: &Path, invocation: &Invocation, limit: usize) -> Exit {
    crate::run::run_limited(hgs, invocation, &[], limit).unwrap_or_else(|_| Exit::Code {
        code: 0,
        stdout: b"{}".to_vec(),
        stderr: String::new(),
    })
}

/// What a folder without a readable status shows (Qt `storeGitStatus`).
pub fn unavailable() -> Value {
    json!({"state": "unavailable"})
}

/// The status as Qt keeps it: an object with a string `state`; anything
/// else (a failed or timed-out call, a reply over the bound, one without a
/// state) reads as `{"state":"unavailable"}`.
pub fn status(exit: Exit) -> Value {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => match serde_json::from_slice::<Value>(&stdout) {
            Ok(value @ Value::Object(_)) if value.get("state").is_some_and(Value::is_string) => {
                value
            }
            _ => unavailable(),
        },
        _ => unavailable(),
    }
}

/// The diff's reply: Ok with the object (one without a string `state`, or a
/// reply that was not an object, reads as `unavailable`), Err with why the
/// call failed (Qt's "The diff could not be read: %1").
pub fn diff(exit: Exit) -> Result<Value, String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => {
            let mut value = match serde_json::from_slice::<Value>(&stdout) {
                Ok(value @ Value::Object(_)) => value,
                _ => json!({}),
            };
            if !value.get("state").is_some_and(Value::is_string) {
                value["state"] = json!("unavailable");
            }
            Ok(value)
        }
        Exit::Code { code, stderr, .. } => Err(failure(&stderr, Some(code))),
        Exit::Abnormal { stderr } => Err(failure(&stderr, None)),
        Exit::TimedOut => Err("timed out".into()),
        Exit::StartFailed(error) => Err(error),
        Exit::WriteFailed => Err("hgs did not take the request".into()),
    }
}

fn failure(stderr: &str, code: Option<i32>) -> String {
    let text = stderr.trim();
    if !text.is_empty() {
        text.chars().take(4000).collect()
    } else {
        match code {
            Some(code) => format!("exit code {code}"),
            None => "hgs ended abnormally".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(stdout: &str) -> Exit {
        Exit::Code {
            code: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: String::new(),
        }
    }

    #[test]
    fn status_and_diff_name_the_folder_and_the_peer_with_qts_timeouts() {
        let local = status_invocation(&ComputerId::local(), "/work/orbit");
        assert_eq!(
            local.args,
            ["git-status", "--path", "/work/orbit", "--json"]
        );
        assert_eq!(local.timeout, Duration::from_secs(25));
        assert!(local.stdin.is_none() && local.request_id.is_none());
        let peer = status_invocation(&ComputerId::from("studio"), "/work/orbit");
        assert_eq!(
            peer.args,
            ["@studio", "git-status", "--path", "/work/orbit", "--json"]
        );
        assert_eq!(peer.timeout, Duration::from_secs(30));

        let local = diff_invocation(&ComputerId::local(), "C:\\work\\orbit");
        assert_eq!(
            local.args,
            ["git-diff", "--path", "C:\\work\\orbit", "--json"]
        );
        assert_eq!(local.timeout, Duration::from_secs(8));
        let peer = diff_invocation(&ComputerId::from("studio"), "/work/orbit");
        assert_eq!(
            peer.args,
            ["@studio", "git-diff", "--path", "/work/orbit", "--json"]
        );
        assert_eq!(peer.timeout, Duration::from_secs(15));
    }

    #[test]
    fn a_status_without_a_state_or_a_failed_call_reads_as_unavailable() {
        let reply = r#"{"state":"ok","path":"/work/orbit","changed_files":1,"changes":{"added":3,"deleted":1,"tracked_files":1,"untracked_files":0,"truncated":false,"files":[{"path":"a.rs","added":3,"deleted":1}]}}"#;
        let value = status(done(reply));
        assert_eq!(value["state"], "ok");
        assert_eq!(value["changes"]["files"][0]["path"], "a.rs");
        assert_eq!(
            status(done(r#"{"state":"folder_unavailable"}"#))["state"],
            "folder_unavailable"
        );
        for exit in [
            done(r#"{"path":"/work/orbit"}"#),
            done(r#"{"state":7}"#),
            done("[1,2]"),
            done("not json"),
            done("{}"),
            Exit::Code {
                code: 2,
                stdout: Vec::new(),
                stderr: "no such folder".into(),
            },
            Exit::TimedOut,
            Exit::Abnormal {
                stderr: String::new(),
            },
            Exit::StartFailed("hgs is missing".into()),
            Exit::WriteFailed,
        ] {
            assert_eq!(status(exit), unavailable());
        }
    }

    #[test]
    fn a_diff_keeps_its_reply_and_words_a_failure() {
        let reply = r#"{"state":"ok","path":"/work/orbit","diff":"diff --git a/a.rs b/a.rs\n","truncated":true,"bytes":300000}"#;
        let value = diff(done(reply)).unwrap();
        assert_eq!(value["state"], "ok");
        assert_eq!(value["truncated"], true);
        assert_eq!(value["bytes"], 300000);
        assert_eq!(
            diff(done(r#"{"state":"too_large"}"#)).unwrap()["state"],
            "too_large"
        );
        // A reply over the bound reads as {} (see `run`): its state is unavailable.
        assert_eq!(diff(done("{}")).unwrap(), json!({"state":"unavailable"}));
        assert_eq!(diff(done("[]")).unwrap(), json!({"state":"unavailable"}));
        assert_eq!(
            diff(Exit::Code {
                code: 1,
                stdout: Vec::new(),
                stderr: "  not a repository \n".into()
            }),
            Err("not a repository".into())
        );
        assert_eq!(
            diff(Exit::Code {
                code: 3,
                stdout: Vec::new(),
                stderr: String::new()
            }),
            Err("exit code 3".into())
        );
        assert_eq!(diff(Exit::TimedOut), Err("timed out".into()));
        assert_eq!(
            diff(Exit::StartFailed("hgs is missing".into())),
            Err("hgs is missing".into())
        );
    }
}
