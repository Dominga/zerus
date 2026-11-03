//! Partial-removal leftovers: folders Git stopped tracking while some files
//! remained (`worktree-leftovers-v1`). The owning CLI reviews them afresh; a
//! row here is display data plus the exact identity an explicit action sends
//! back. Nothing here retries a removal or deletes without the owner's
//! confirmation, and an unknown outcome is never replayed.
use crate::actions::{Exit, Invocation};
use crate::worktree_review::{data_size, REMOVE_LIMIT, REVIEW_LIMIT};
use crate::worktrees::cleanup_error;
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// The help token of a CLI that reviews leftovers and can keep their folders.
pub const LEFTOVERS_MARKER: &str = "worktree-leftovers-v1";
/// The help token of a CLI that can delete a reviewed leftover's files.
pub const DELETE_MARKER: &str = "worktree-leftover-delete-v1";
/// The help token of a CLI whose keep-folder reports a request-bound outcome.
pub const KEEP_MARKER: &str = "worktree-leftover-keep-v1";
/// The most leftover rows the client takes from one reply. The CLI sends at
/// most 32 and marks the rest partial.
pub const ROWS: usize = 64;
const TEXT: usize = 4096;
const PATH: usize = 8192;

/// An exact help token, never a substring of another word.
pub fn has_marker(help: &str, marker: &str) -> bool {
    help.split_ascii_whitespace().any(|word| word == marker)
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap_or("")
}
fn bounded(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
fn line(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(TEXT)
        .collect::<String>()
        .trim()
        .to_owned()
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One reviewed leftover. `fingerprint` binds an action to the review the
/// owner saw; the process fingerprint never leaves the native side.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Leftover {
    #[serde(skip)]
    pub path: String,
    #[serde(skip)]
    pub common_dir: String,
    pub branch: String,
    /// `None` when the branch could not be checked.
    pub branch_retained: Option<bool>,
    pub bytes: Option<u64>,
    pub complete: bool,
    /// Why the removal stopped, as the owning computer recorded it.
    pub stopped: String,
    /// The removal was interrupted before its outcome was recorded.
    pub interrupted: bool,
    /// What prevents deleting the files now.
    pub reasons: Vec<String>,
    pub sessions: Vec<String>,
    pub processes: u64,
    pub fingerprint: String,
    #[serde(skip)]
    pub process_fingerprint: String,
    /// Deleting is possible after the owner accepts programs whose folders
    /// cannot be read; the checkbox is shown only then.
    pub consent: bool,
    pub deletable: bool,
    /// False once the repository itself is gone: only Keep folder remains.
    pub repository: bool,
    pub reserved_at: Option<f64>,
    /// The explicit action's outcome in this window; a fresh review clears it.
    pub outcome: Option<Settled>,
}

/// A leftover row of a review (`repository`) or of the vanished-repository
/// listing. Anything malformed is not shown.
pub fn leftover(row: &Value, repository: bool) -> Option<Leftover> {
    let removal = &row["removal"];
    let path = text(row, "path");
    let common_dir = text(row, "common_dir");
    let fingerprint = text(row, "fingerprint");
    if text(row, "kind") != "leftover"
        || text(row, "verdict") != "partially_removed"
        || !bounded(path, PATH)
        || !bounded(common_dir, PATH)
        || !digest(fingerprint)
        || text(removal, "path") != path
        || (row["repository_available"] == false) == repository
    {
        return None;
    }
    let process_fingerprint = text(row, "process_fingerprint");
    let process_fingerprint = if repository && bounded(process_fingerprint, 256) {
        process_fingerprint.to_owned()
    } else {
        String::new()
    };
    let strings = |key: &str| -> Vec<String> {
        row[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| bounded(s, 1024))
            .take(50)
            .map(str::to_owned)
            .collect()
    };
    let reasons = row["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|reason| line(text(reason, "message")))
        .filter(|message| !message.is_empty())
        .take(20)
        .collect();
    let deletable = repository && row["deletable"] == true;
    Some(Leftover {
        path: path.into(),
        common_dir: common_dir.into(),
        branch: line(text(row, "branch")),
        branch_retained: row["branch_retained"].as_bool(),
        bytes: row["size"].as_u64(),
        complete: row["size_complete"] == true,
        stopped: line(text(removal, "detail")),
        interrupted: text(removal, "status") == "pending",
        reasons,
        sessions: strings("sessions"),
        processes: row["process_count"].as_u64().unwrap_or(0),
        fingerprint: fingerprint.into(),
        consent: !deletable && !process_fingerprint.is_empty(),
        process_fingerprint,
        deletable,
        repository,
        reserved_at: removal["reserved_at"].as_f64().filter(|n| n.is_finite()),
        outcome: None,
    })
}

/// The remaining size, a lower bound when the listing was incomplete.
pub fn size(leftover: &Leftover) -> String {
    match leftover.bytes {
        None => "Not measured".into(),
        Some(bytes) if leftover.complete => data_size(bytes as f64),
        Some(bytes) => format!("≥ {}", data_size(bytes as f64)),
    }
}

/// The row's one-line state: what stopped deletion now, or why it remains.
pub fn summary(leftover: &Leftover) -> String {
    if !leftover.repository {
        // The row's state already says the repository is missing.
        return "Keep the folder or clean it up yourself.".into();
    }
    if let Some(reason) = leftover.reasons.first() {
        return reason.clone();
    }
    if leftover.interrupted {
        return "The removal was interrupted. Review the remaining files.".into();
    }
    if !leftover.stopped.is_empty() {
        return leftover.stopped.clone();
    }
    "Git no longer tracks this folder, but files remain.".into()
}

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}
fn call(args: Vec<String>, seconds: u64) -> Invocation {
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(seconds),
        request_id: None,
    }
}

/// `hgs [@host] worktrees leftovers --json`: one bounded listing per
/// computer of leftovers whose repository is gone, 45 s here and 60 s on a
/// peer, at most 4 MiB.
pub fn listing_invocation(computer: &ComputerId) -> Invocation {
    call(
        hgs_args(computer, &["worktrees", "leftovers", "--json"]),
        if computer.is_local() { 45 } else { 60 },
    )
}

/// The listing's rows, at most [`ROWS`], and whether more remain.
pub fn listing(exit: Exit, host: &str) -> Result<(Vec<Value>, bool), String> {
    let stdout = match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => stdout,
        Exit::Code { stderr, code, .. } => {
            let detail = stderr.trim();
            return Err(cleanup_error(
                host,
                &if detail.is_empty() {
                    format!("exit code {code}")
                } else {
                    detail.to_owned()
                },
            ));
        }
        Exit::Abnormal { stderr } => return Err(cleanup_error(host, stderr.trim())),
        Exit::TimedOut => return Err("timed out".into()),
        Exit::StartFailed(error) => return Err(error),
        Exit::WriteFailed => return Err("hgs did not take the request".into()),
    };
    let value: Value = if stdout.len() <= REVIEW_LIMIT {
        serde_json::from_slice(&stdout).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let Some(rows) = value["leftovers"]
        .as_array()
        .filter(|_| value["schema"] == 1)
    else {
        return Err("Invalid leftover listing".into());
    };
    Ok((
        rows.iter().take(ROWS).cloned().collect(),
        rows.len() > ROWS || value["partial"] == true,
    ))
}

/// `hgs [@host] worktrees keep-folder --path P --common-dir C --fingerprint F
/// --request-id ID --json`, 60 s, at most 64 KiB.
pub fn keep_invocation(computer: &ComputerId, leftover: &Leftover, request_id: &str) -> Invocation {
    call(
        hgs_args(
            computer,
            &[
                "worktrees",
                "keep-folder",
                "--path",
                &leftover.path,
                "--common-dir",
                &leftover.common_dir,
                "--fingerprint",
                &leftover.fingerprint,
                "--request-id",
                request_id,
                "--json",
            ],
        ),
        60,
    )
}

/// `hgs [@host] worktrees delete-leftover --path P --common-dir C
/// --fingerprint F --confirm-delete --request-id ID --json
/// [--process-fingerprint X]`, 360 s, at most 64 KiB. The process
/// fingerprint goes only with the owner's consent to a row that offered it.
pub fn delete_invocation(
    computer: &ComputerId,
    leftover: &Leftover,
    request_id: &str,
    consent: bool,
) -> Invocation {
    let mut args = hgs_args(
        computer,
        &[
            "worktrees",
            "delete-leftover",
            "--path",
            &leftover.path,
            "--common-dir",
            &leftover.common_dir,
            "--fingerprint",
            &leftover.fingerprint,
            "--confirm-delete",
            "--request-id",
            request_id,
            "--json",
        ],
    );
    if consent && leftover.consent {
        args.extend([
            "--process-fingerprint".into(),
            leftover.process_fingerprint.clone(),
        ]);
    }
    call(args, 360)
}

/// How an explicit leftover action ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Settled {
    /// The remaining files are gone and the record with them.
    Deleted,
    /// The folder stays and only its record was forgotten.
    Forgotten,
    /// A verified refusal before anything changed.
    Refused { detail: String },
    /// Anything else; never retried.
    Unknown { detail: String },
}

fn reply(exit: Exit) -> (bool, i32, Value, String) {
    let (finished, code, stdout, stderr) = match exit {
        Exit::Code {
            code,
            stdout,
            stderr,
        } => (true, code, stdout, stderr),
        Exit::Abnormal { stderr } => (false, -1, Vec::new(), stderr),
        Exit::TimedOut | Exit::WriteFailed => (false, -1, Vec::new(), String::new()),
        Exit::StartFailed(error) => (false, -1, Vec::new(), error),
    };
    let data = if stdout.len() <= REMOVE_LIMIT {
        serde_json::from_slice(&stdout).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    (finished, code, data, stderr)
}
fn detail(host: &str, text: &str, fallback: &str) -> String {
    let text: String = text.trim().chars().take(TEXT).collect();
    if text.is_empty() {
        fallback.to_owned()
    } else {
        cleanup_error(host, &text)
    }
}

/// Keep folder's outcome. Only the CLI's request-bound refusal, given before
/// the record was touched, is "nothing changed"; its missing-record refusal
/// means the record is already gone, which is what Keep folder asks for. An
/// unreachable computer, a crash or any other reply is unknown.
pub fn kept(exit: Exit, host: &str, request_id: &str, leftover: &Leftover) -> Settled {
    let (finished, code, data, stderr) = reply(exit);
    let identity = text(&data, "request_id") == request_id && text(&data, "path") == leftover.path;
    if finished && code == 0 && identity && text(&data, "status") == "kept" {
        return Settled::Forgotten;
    }
    let verified = finished
        && code != 0
        && identity
        && text(&data, "common_dir") == leftover.common_dir
        && text(&data, "operation") == "worktree_leftover_keep"
        && data["outcome_version"] == 1;
    if verified && text(&data, "status") == "kept" {
        let reason = text(&data, "detail");
        if reason.starts_with("No partial-removal record exists") {
            return Settled::Forgotten;
        }
        return Settled::Refused {
            detail: detail(
                host,
                reason,
                "The folder and its record were left as they were.",
            ),
        };
    }
    Settled::Unknown {
        detail: detail(
            host,
            if verified {
                text(&data, "detail")
            } else {
                &stderr
            },
            "Keeping the folder could not be confirmed. Review again.",
        ),
    }
}

/// Delete remaining files' outcome: deleted, a verified refusal before any
/// file changed, or unknown (including a deletion that stopped partway).
pub fn deleted(exit: Exit, host: &str, request_id: &str, leftover: &Leftover) -> Settled {
    let (finished, code, data, stderr) = reply(exit);
    let identity = text(&data, "request_id") == request_id
        && text(&data, "path") == leftover.path
        && text(&data, "common_dir") == leftover.common_dir;
    if finished && code == 0 && identity && text(&data, "status") == "deleted" {
        return Settled::Deleted;
    }
    let verified = finished
        && code != 0
        && identity
        && text(&data, "operation") == "worktree_leftover_delete"
        && data["outcome_version"] == 1;
    if verified && text(&data, "status") == "kept" {
        return Settled::Refused {
            detail: detail(host, text(&data, "detail"), "Nothing was deleted."),
        };
    }
    let reason = if verified {
        text(&data, "detail")
    } else {
        &stderr
    };
    Settled::Unknown {
        detail: detail(
            host,
            reason,
            "Deleting the remaining files could not be confirmed. Review again.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn row() -> Value {
        json!({"path":"/srv/example/w","kind":"leftover","verdict":"partially_removed",
            "common_dir":"/srv/example/.git","branch":"feature/w","branch_head":"a1",
            "branch_retained":true,"size":2048,"size_complete":true,"sessions":[],
            "process_count":0,"reasons":[],"deletable":true,"fingerprint":FINGERPRINT,
            "removal":{"path":"/srv/example/w","status":"partial","reserved_at":10.0,
                "detail":"Permission denied"}})
    }
    fn exit(code: i32, stdout: Value, stderr: &str) -> Exit {
        Exit::Code {
            code,
            stdout: stdout.to_string().into_bytes(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn markers_are_exact_help_tokens() {
        let help = "Worktree leftover ABI: worktree-leftovers-v1 (review)\n  worktree-leftover-delete-v1\n";
        assert!(has_marker(help, LEFTOVERS_MARKER));
        assert!(has_marker(help, DELETE_MARKER));
        assert!(!has_marker("worktree-leftovers-v10", LEFTOVERS_MARKER));
        assert!(!has_marker("(worktree-leftovers-v1)", LEFTOVERS_MARKER));
        assert!(!has_marker("old help", DELETE_MARKER));
    }

    #[test]
    fn rows_keep_identity_natively_and_refuse_malformed_or_mismatched_input() {
        let parsed = leftover(&row(), true).unwrap();
        assert_eq!(parsed.path, "/srv/example/w");
        assert_eq!(parsed.common_dir, "/srv/example/.git");
        assert!(parsed.deletable && !parsed.consent && parsed.repository);
        assert_eq!(parsed.branch_retained, Some(true));
        assert_eq!(size(&parsed), "2.00 KiB");
        assert_eq!(summary(&parsed), "Permission denied");
        let shown = serde_json::to_value(&parsed).unwrap();
        assert!(shown.get("path").is_none() && shown.get("process_fingerprint").is_none());
        assert_eq!(shown["fingerprint"], FINGERPRINT);
        for (key, value) in [
            ("kind", json!("linked")),
            ("verdict", json!("ready")),
            ("fingerprint", json!("short")),
            ("path", json!("/srv/example/other")),
            ("common_dir", json!("")),
        ] {
            let mut bad = row();
            bad[key] = value;
            assert!(leftover(&bad, true).is_none(), "{key}");
        }
        // A listing row must be a vanished repository's, and a review's must not.
        assert!(leftover(&row(), false).is_none());
        let mut orphan = row();
        orphan["repository_available"] = json!(false);
        assert!(leftover(&orphan, true).is_none());
        let orphan = leftover(&orphan, false).unwrap();
        assert!(!orphan.deletable && !orphan.consent);
        assert_eq!(summary(&orphan), "Keep the folder or clean it up yourself.");
    }

    #[test]
    fn process_consent_is_offered_only_when_it_is_the_sole_obstacle() {
        let mut value = row();
        value["deletable"] = json!(false);
        value["size_complete"] = json!(false);
        value["reasons"] = json!([{"code":"processes_unavailable","message":"Hidden programs."}]);
        value["process_fingerprint"] = json!("v2:100:abc");
        value["branch_retained"] = Value::Null;
        let parsed = leftover(&value, true).unwrap();
        assert!(parsed.consent && !parsed.deletable);
        assert_eq!(parsed.branch_retained, None);
        assert_eq!(size(&parsed), "≥ 2.00 KiB");
        assert_eq!(summary(&parsed), "Hidden programs.");
        let computer = ComputerId("example-peer".into());
        let without = delete_invocation(&computer, &parsed, "r1", false);
        assert!(!without.args.contains(&"--process-fingerprint".to_owned()));
        let with = delete_invocation(&computer, &parsed, "r1", true);
        assert_eq!(
            with.args[with.args.len() - 2..],
            ["--process-fingerprint", "v2:100:abc"]
        );
        // Consent never adds a fingerprint the row did not offer.
        let plain = leftover(&row(), true).unwrap();
        let args = delete_invocation(&computer, &plain, "r1", true).args;
        assert!(!args.contains(&"--process-fingerprint".to_owned()));
    }

    #[test]
    fn invocations_bind_path_repository_fingerprint_and_request() {
        let parsed = leftover(&row(), true).unwrap();
        let computer = ComputerId("example-peer".into());
        let keep = keep_invocation(&computer, &parsed, "r1");
        assert_eq!(
            keep.args,
            [
                "@example-peer",
                "worktrees",
                "keep-folder",
                "--path",
                "/srv/example/w",
                "--common-dir",
                "/srv/example/.git",
                "--fingerprint",
                FINGERPRINT,
                "--request-id",
                "r1",
                "--json"
            ]
        );
        let delete = delete_invocation(&ComputerId::local(), &parsed, "r2", false);
        assert_eq!(
            delete.args,
            [
                "worktrees",
                "delete-leftover",
                "--path",
                "/srv/example/w",
                "--common-dir",
                "/srv/example/.git",
                "--fingerprint",
                FINGERPRINT,
                "--confirm-delete",
                "--request-id",
                "r2",
                "--json"
            ]
        );
        assert_eq!(delete.timeout, Duration::from_secs(360));
        assert_eq!(
            listing_invocation(&computer).args,
            ["@example-peer", "worktrees", "leftovers", "--json"]
        );
    }

    #[test]
    fn delete_outcomes_are_request_bound_and_never_guess() {
        let parsed = leftover(&row(), true).unwrap();
        let done = json!({"status":"deleted","path":"/srv/example/w",
            "common_dir":"/srv/example/.git","request_id":"r1"});
        assert_eq!(
            deleted(exit(0, done.clone(), ""), "", "r1", &parsed),
            Settled::Deleted
        );
        // Another request's reply proves nothing.
        assert!(matches!(
            deleted(exit(0, done, ""), "", "r2", &parsed),
            Settled::Unknown { .. }
        ));
        let refusal = |status: &str| {
            json!({"operation":"worktree_leftover_delete","outcome_version":1,"status":status,
                "path":"/srv/example/w","common_dir":"/srv/example/.git","request_id":"r1",
                "detail":"Zerus sessions use this folder."})
        };
        assert_eq!(
            deleted(exit(1, refusal("kept"), "x"), "", "r1", &parsed),
            Settled::Refused {
                detail: "Zerus sessions use this folder.".into()
            }
        );
        // A deletion that stopped partway is unknown, never a refusal.
        assert_eq!(
            deleted(exit(1, refusal("unconfirmed"), "x"), "", "r1", &parsed),
            Settled::Unknown {
                detail: "Zerus sessions use this folder.".into()
            }
        );
        for lost in [
            Exit::TimedOut,
            Exit::WriteFailed,
            Exit::Abnormal {
                stderr: String::new(),
            },
            exit(1, Value::Null, "boom"),
        ] {
            assert!(matches!(
                deleted(lost, "", "r1", &parsed),
                Settled::Unknown { .. }
            ));
        }
    }

    #[test]
    fn keep_outcomes_are_refused_only_on_the_request_bound_refusal() {
        let parsed = leftover(&row(), true).unwrap();
        let done = json!({"status":"kept","path":"/srv/example/w","request_id":"r1"});
        assert_eq!(
            kept(exit(0, done.clone(), ""), "", "r1", &parsed),
            Settled::Forgotten
        );
        assert!(matches!(
            kept(exit(0, done, ""), "", "r2", &parsed),
            Settled::Unknown { .. }
        ));
        let outcome = |status: &str, detail: &str| {
            json!({"operation":"worktree_leftover_keep","outcome_version":1,"status":status,
                "path":"/srv/example/w","common_dir":"/srv/example/.git","request_id":"r1",
                "detail":detail})
        };
        assert_eq!(
            kept(
                exit(
                    1,
                    outcome("kept", "The leftover folder changed since the review."),
                    "x"
                ),
                "",
                "r1",
                &parsed
            ),
            Settled::Refused {
                detail: "The leftover folder changed since the review.".into()
            }
        );
        // The record is already gone: what Keep folder asks for.
        assert_eq!(
            kept(
                exit(
                    1,
                    outcome("kept", "No partial-removal record exists for this folder."),
                    "x"
                ),
                "",
                "r1",
                &parsed
            ),
            Settled::Forgotten
        );
        assert!(matches!(
            kept(
                exit(1, outcome("unconfirmed", "database is locked"), "x"),
                "",
                "r1",
                &parsed
            ),
            Settled::Unknown { .. }
        ));
        // Another request's or repository's refusal proves nothing.
        let mut other = outcome("kept", "changed");
        other["common_dir"] = json!("/srv/other/.git");
        assert!(matches!(
            kept(exit(1, other, "x"), "", "r1", &parsed),
            Settled::Unknown { .. }
        ));
        // A dropped link (hgs maps SSH 255 to 3), a crash or an older CLI's
        // plain error never reads as "nothing changed".
        for lost in [
            exit(3, Value::Null, "hgs: example-peer unreachable"),
            exit(101, Value::Null, "thread 'main' panicked"),
            exit(
                1,
                Value::Null,
                "No partial-removal record exists for this folder.",
            ),
            exit(
                1,
                Value::Null,
                "The leftover folder changed since the review.",
            ),
            Exit::TimedOut,
        ] {
            assert!(matches!(
                kept(lost, "", "r1", &parsed),
                Settled::Unknown { .. }
            ));
        }
    }

    #[test]
    fn listings_are_bounded_and_report_partial() {
        let rows: Vec<_> = (0..70).map(|_| row()).collect();
        let (taken, partial) = listing(
            exit(0, json!({"schema":1,"leftovers":rows,"partial":false}), ""),
            "",
        )
        .unwrap();
        assert_eq!((taken.len(), partial), (ROWS, true));
        let (taken, partial) = listing(
            exit(
                0,
                json!({"schema":1,"leftovers":[row()],"partial":true}),
                "",
            ),
            "",
        )
        .unwrap();
        assert_eq!((taken.len(), partial), (1, true));
        assert!(listing(exit(0, json!({"leftovers":[]}), ""), "").is_err());
        assert!(listing(Exit::TimedOut, "").is_err());
    }
}
