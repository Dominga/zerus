//! Worktree cleanup review, ported from the Qt client: the review call and
//! its rows' rules, the removal call and its outcomes, and the per-computer
//! cleanup policy of Settings > Worktrees.
//!
//! The rules keep what `AGENTS.md` asks: only an explicit action removes a
//! folder; local data, running programs, sessions and incomplete checks
//! protect it; an incomplete or malformed policy fails closed; an unknown
//! outcome is never retried. A manual acceptance of unreadable process paths
//! is passed only with the user's consent, and `hgs` keeps it for five
//! minutes on the exact checkout review.
//!
//! Reference: `tray/src/HgsClient.cpp` (`reviewWorktrees`, `worktreeReview`,
//! `removeWorktree`), `tray/src/WorktreeReviewUi.h`,
//! `tray/src/WorktreeCleanupDialog.h`, `tray/src/WorktreeCleanupStatus.h`,
//! `tray/src/WorktreeSettings.h`.
use crate::actions::{Exit, Invocation};
use crate::worktrees::cleanup_error;
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// The largest review reply.
pub const REVIEW_LIMIT: usize = 4 * 1024 * 1024;
/// The largest removal reply.
pub const REMOVE_LIMIT: usize = 64 * 1024;
/// The largest policy reply.
pub const POLICY_LIMIT: usize = 128 * 1024;
/// The help text of a CLI with cleanup settings.
pub const POLICY_MARKER: &str = "worktree-maintenance-v1";

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

fn call(args: Vec<String>, stdin: Option<Vec<u8>>, seconds: u64) -> Invocation {
    Invocation {
        args,
        stdin,
        timeout: Duration::from_secs(seconds),
        request_id: None,
    }
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap_or("")
}

fn count(row: &Value, key: &str) -> i64 {
    row[key].as_i64().unwrap_or(0)
}

/// `hgs [@host] worktrees review --path P --json [--worktree W]`, 30 s here
/// and 45 s on a peer, at most 4 MiB.
pub fn review_invocation(computer: &ComputerId, path: &str, worktree: &str) -> Invocation {
    let mut args = hgs_args(computer, &["worktrees", "review", "--path", path, "--json"]);
    if !worktree.is_empty() {
        args.extend(["--worktree".to_owned(), worktree.to_owned()]);
    }
    call(args, None, if computer.is_local() { 30 } else { 45 })
}

/// A review reply: an object with a `worktrees` array and a string `state`.
/// `changed` when the repository's catalogs were invalidated meanwhile.
pub fn review(exit: Exit, host: &str, changed: bool) -> Result<Value, String> {
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
    if changed {
        return Err("Folder changed. Refresh its cleanup review.".into());
    }
    let data: Value = if stdout.len() <= REVIEW_LIMIT {
        serde_json::from_slice(&stdout).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    if data["worktrees"].is_array() && data["state"].is_string() {
        Ok(data)
    } else {
        Err("Invalid worktree review response".into())
    }
}

/// A repository's latest review: a targeted review of one worktree replaces
/// only that worktree's row (`reviewWorktrees`).
pub fn merge_review(previous: Option<&Value>, reply: &Value, worktree: &str, now: f64) -> Value {
    let mut merged = if worktree.is_empty() {
        reply.clone()
    } else {
        previous.cloned().unwrap_or_else(|| reply.clone())
    };
    let mut rows = Vec::new();
    if !worktree.is_empty() {
        rows.extend(
            merged["worktrees"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| text(row, "path") != worktree)
                .cloned(),
        );
    }
    for row in reply["worktrees"].as_array().into_iter().flatten() {
        if !worktree.is_empty() && text(row, "path") != worktree {
            continue;
        }
        let mut row = row.clone();
        row["sampled_at"] = reply["sampled_at"].clone();
        rows.push(row);
    }
    if !worktree.is_empty() {
        let mut targeted = merged["targeted_paths"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !targeted.iter().any(|path| path == worktree) {
            targeted.push(json!(worktree));
        }
        merged["targeted_paths"] = Value::Array(targeted);
    }
    merged["worktrees"] = Value::Array(rows);
    merged["sampled_at"] = reply["sampled_at"].clone();
    merged["received_at"] = json!(now.floor());
    merged
}

/// The review with only rows sampled under `max_age` seconds ago, or nothing
/// when the review itself is older (`worktreeReview`).
pub fn current_review(review: &Value, max_age: f64, now: f64) -> Option<Value> {
    let sampled = review["sampled_at"].as_f64().unwrap_or(0.0);
    let age = now.floor() - sampled.trunc();
    if !(0.0..max_age).contains(&age) {
        return None;
    }
    let mut result = review.clone();
    let rows: Vec<Value> = review["worktrees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            let at = row["sampled_at"].as_f64().unwrap_or(sampled);
            (0.0..max_age).contains(&(now.floor() - at.trunc()))
        })
        .cloned()
        .collect();
    result["worktrees"] = Value::Array(rows);
    Some(result)
}

/// Whether a row has one of these reason codes.
pub fn has_reason(row: &Value, codes: &[&str]) -> bool {
    row["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|reason| codes.contains(&text(reason, "code")))
}

/// Ignored files outside Git: caches, builds or unique data.
pub fn local_data(row: &Value) -> bool {
    count(row, "ignored_count") > 0
        || row["ignored"]
            .as_array()
            .is_some_and(|ignored| !ignored.is_empty())
        || row["ignored_bytes"].as_f64().unwrap_or(0.0) > 0.0
}

/// A row's group: 0 cleanup candidates, 1 review work, 2 in use, 3 not
/// checked, 4 main folders.
pub fn category(row: &Value, verified: bool) -> u8 {
    if text(row, "kind") != "linked" {
        return 4;
    }
    if local_data(row) && text(row, "ignored_fingerprint").is_empty() {
        return 3;
    }
    if !verified
        || text(row, "verdict").is_empty()
        || has_reason(
            row,
            &[
                "status_unavailable",
                "processes_unavailable",
                "integration_unavailable",
                "ignored_unavailable",
            ],
        )
    {
        return 3;
    }
    if has_reason(
        row,
        &["processes", "sessions", "locked", "nested", "submodules"],
    ) {
        return 2;
    }
    if (text(row, "verdict") == "ready" && !local_data(row)) || text(row, "verdict") == "missing" {
        return 0;
    }
    1
}

pub fn category_name(category: u8) -> &'static str {
    match category {
        0 => "Cleanup candidates",
        1 => "Review work",
        2 => "In use",
        3 => "Not checked",
        _ => "Main folders",
    }
}

/// How the worktree's code relates to its base.
pub fn integration(row: &Value) -> String {
    let data = &row["integration"];
    let base = text(data, "base");
    match text(data, "kind") {
        "ancestor" => format!("Merged into {base}"),
        "same_tree" => format!("Same files as {base}; original branch kept"),
        "equivalent_patches" => {
            format!("Equivalent patches in {base} history; original branch kept")
        }
        "remaining" => format!("Changes to compare with {base}"),
        _ if row["merged"] == json!(true) => "Merged".into(),
        _ => "Integration not verified".into(),
    }
}

/// A byte count as Qt's `QLocale::formattedDataSize` writes it: `512 bytes`,
/// `1.50 KiB`, `2.00 MiB`…
pub fn data_size(bytes: f64) -> String {
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024.0 {
        return format!("{} bytes", bytes.max(0.0) as i64);
    }
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1024.0 * 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.2} {}", value / 1024.0, UNITS[unit])
}

/// The size column.
pub fn size(row: &Value) -> String {
    if text(row, "verdict") == "missing" {
        return "Metadata only".into();
    }
    if row.get("disk_bytes").is_none() {
        return "Not measured".into();
    }
    let prefix = if row["disk_complete"] == json!(true) {
        ""
    } else {
        "≥ "
    };
    format!(
        "{prefix}{}",
        data_size(row["disk_bytes"].as_f64().unwrap_or(0.0))
    )
}

/// The row's summary, as separate parts the view lays out (Qt joined them
/// with a dot, which the new client's UI does not use).
pub fn summary(row: &Value) -> Vec<String> {
    if text(row, "verdict") == "missing" {
        return vec!["Folder is gone; forget its Git record".into()];
    }
    let mut parts = vec![integration(row)];
    if local_data(row) && text(row, "ignored_fingerprint").is_empty() {
        parts.push("Update Zerus on this computer to review local files before cleanup".into());
    }
    if count(row, "changes") > 0 {
        parts.push(format!("{} changed", count(row, "changes")));
    }
    if count(row, "untracked") > 0 {
        parts.push(format!("{} untracked", count(row, "untracked")));
    }
    if local_data(row) {
        parts.push("Local files outside Git".into());
    }
    for reason in row["reasons"].as_array().into_iter().flatten() {
        if !matches!(text(reason, "code"), "changes" | "untracked") {
            parts.push(text(reason, "message").into());
        }
    }
    parts
}

/// The lines that explain a removal decision.
pub fn decision(row: &Value) -> Vec<String> {
    let mut lines = vec![format!("Code: {} (local Git refs).", integration(row))];
    if text(row, "verdict") == "missing" {
        lines.push(
            "The folder is already gone. Cleanup only forgets its Git worktree record.".into(),
        );
        return lines;
    }
    if row.get("changes").is_some() && row.get("untracked").is_some() {
        let (changes, untracked) = (count(row, "changes"), count(row, "untracked"));
        lines.push(if changes > 0 || untracked > 0 {
            format!("Unsaved work: {changes} changed and {untracked} untracked files.")
        } else {
            "Unsaved work: no changed or untracked files.".into()
        });
    } else {
        lines.push("Unsaved work: see the current file check below.".into());
    }
    if local_data(row) {
        lines.push(format!(
            "Local data: {} outside Git. These files may include caches, builds or unique data; Git cannot restore them.",
            data_size(row["ignored_bytes"].as_f64().unwrap_or(0.0))
        ));
        lines.push("Automatic cleanup keeps folders with any local data. Inspect the files before choosing to delete them.".into());
    } else if row.get("ignored_count").is_some() {
        lines.push("Local data: no ignored files found.".into());
    }
    for reason in row["reasons"].as_array().into_iter().flatten() {
        lines.push(text(reason, "message").into());
    }
    for note in row["notes"].as_array().into_iter().flatten() {
        if text(note, "code") != "ignored_data" {
            lines.push(text(note, "message").into());
        }
    }
    let unreadable = row["unreadable_processes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for process in unreadable.iter().take(5) {
        lines.push(format!(
            "Cannot check: {} (PID {})",
            text(process, "command"),
            process["pid"].as_i64().unwrap_or(0)
        ));
    }
    if unreadable.len() > 5 {
        lines.push(format!(
            "{} more processes could not be checked.",
            unreadable.len() - 5
        ));
    }
    let mut seen = std::collections::HashSet::new();
    lines.retain(|line| seen.insert(line.clone()));
    lines
}

/// A checkout blocked only because the OS hides some programs' folder paths:
/// the user may accept that after checking.
pub fn process_reviewable(row: &Value) -> bool {
    let reasons = row["reasons"].as_array().cloned().unwrap_or_default();
    text(row, "kind") == "linked"
        && text(row, "verdict") == "blocked"
        && reasons.len() == 1
        && text(&reasons[0], "code") == "processes_unavailable"
        && !text(row, "fingerprint").is_empty()
        && !text(row, "process_fingerprint").is_empty()
        && (!local_data(row) || !text(row, "ignored_fingerprint").is_empty())
}

/// A Zerus-owned checkout that can go after its session ends without asking
/// again: integrated code, nothing unsaved, measured, and no local data
/// except generated files.
pub fn exit_cleanup_ready(row: &Value) -> bool {
    let kind = text(&row["integration"], "kind");
    text(row, "kind") == "linked"
        && row["zerus_owned"] == json!(true)
        && row["archive_restore_ready"] == json!(true)
        && matches!(text(row, "verdict"), "ready" | "review")
        && !text(row, "fingerprint").is_empty()
        && row["reasons"].as_array().is_some_and(Vec::is_empty)
        && row["changes"].as_i64() == Some(0)
        && row["untracked"].as_i64() == Some(0)
        && row["disk_complete"] == json!(true)
        && row["ignored_complete"] == json!(true)
        && matches!(kind, "ancestor" | "same_tree" | "equivalent_patches")
        && row["ignored_count"]
            .as_i64()
            .is_some_and(|count| count >= 0)
        && (row["ignored_count"].as_i64() == Some(0) || row["generated_files_only"] == json!(true))
}

/// Whether a review row may be removed by an explicit action.
pub fn removable(row: &Value) -> bool {
    text(row, "kind") == "linked"
        && matches!(text(row, "verdict"), "ready" | "review" | "missing")
        && !text(row, "fingerprint").is_empty()
        && (!local_data(row) || !text(row, "ignored_fingerprint").is_empty())
}

/// What a removal confirms and sends. `delete_branch` only for a merged
/// branch; the ignored fingerprint deletes the reviewed local files; the
/// process fingerprint is the user's acceptance of unreadable processes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    pub path: String,
    pub common_dir: String,
    pub fingerprint: String,
    #[serde(default)]
    pub delete_branch: bool,
    #[serde(default)]
    pub ignored_fingerprint: String,
    #[serde(default)]
    pub discard_fingerprint: String,
    #[serde(default)]
    pub process_fingerprint: String,
    #[serde(default)]
    pub preserve_archive_restore: bool,
}

/// `hgs [@host] worktrees remove --path P --common-dir C --fingerprint F
/// --request-id ID --json [...]`, 330 s, at most 64 KiB.
pub fn remove_invocation(computer: &ComputerId, removal: &Removal, request_id: &str) -> Invocation {
    let mut args = hgs_args(
        computer,
        &[
            "worktrees",
            "remove",
            "--path",
            &removal.path,
            "--common-dir",
            &removal.common_dir,
            "--fingerprint",
            &removal.fingerprint,
            "--request-id",
            request_id,
            "--json",
        ],
    );
    if removal.delete_branch {
        args.push("--delete-branch".into());
    }
    for (flag, value) in [
        ("--ignored-fingerprint", &removal.ignored_fingerprint),
        ("--discard-fingerprint", &removal.discard_fingerprint),
        ("--process-fingerprint", &removal.process_fingerprint),
    ] {
        if !value.is_empty() {
            args.extend([flag.to_owned(), value.clone()]);
        }
    }
    if removal.preserve_archive_restore {
        args.push("--preserve-archive-restore".into());
    }
    call(args, None, 330)
}

/// How a removal ended. Kept and unconfirmed are verified refusals with
/// their detail; anything else is unknown and never retried.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Removed {
    /// `removed`, or `forgotten` for a folder already gone.
    Done {
        reply: Value,
    },
    /// `hgs` kept the folder, or could not confirm, and said why.
    Refused {
        reply: Value,
        detail: String,
    },
    Unknown {
        detail: String,
    },
}

/// The removal's outcome (`removeWorktree`). The caller invalidates the
/// repository's catalogs whatever it is.
pub fn removed(exit: Exit, host: &str, request_id: &str, removal: &Removal) -> Removed {
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
    let data: Value = if stdout.len() <= REMOVE_LIMIT {
        serde_json::from_slice(&stdout).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let identity = text(&data, "request_id") == request_id && text(&data, "path") == removal.path;
    if finished && code == 0 && identity && matches!(text(&data, "status"), "removed" | "forgotten")
    {
        return Removed::Done { reply: data };
    }
    let verified = finished
        && code != 0
        && identity
        && text(&data, "common_dir") == removal.common_dir
        && text(&data, "operation") == "worktree_remove"
        && data["outcome_version"] == json!(1)
        && matches!(text(&data, "status"), "kept" | "unconfirmed");
    let detail: String = if verified {
        text(&data, "detail").chars().take(4000).collect()
    } else {
        stderr.trim().chars().take(4000).collect()
    };
    let detail = if detail.is_empty() {
        "Removal could not be confirmed. Check the folder before deciding what to do next."
            .to_owned()
    } else {
        cleanup_error(host, &detail)
    };
    if verified {
        Removed::Refused {
            reply: data,
            detail,
        }
    } else {
        Removed::Unknown { detail }
    }
}

/// The removal confirmation's texts for these rows: the button, the consent
/// a process acceptance or local-file deletion needs (the button stays
/// disabled until it is given), and the warning line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveConfirm {
    pub button: String,
    pub consent: Option<String>,
    pub process_warning: Option<String>,
}

pub fn remove_confirm(rows: &[&Value]) -> RemoveConfirm {
    let local = rows.iter().any(|row| local_data(row));
    let process = rows.iter().any(|row| process_reviewable(row));
    let consent = if process {
        let mut text = "I checked these folders are not in use; remove them anyway".to_owned();
        if local {
            text.push_str(". Delete the reviewed local files outside Git too.");
        }
        Some(text)
    } else if local {
        Some("I reviewed the local files outside Git and want to delete them too".to_owned())
    } else {
        None
    };
    RemoveConfirm {
        button: if rows.len() == 1 {
            "Remove worktree".into()
        } else {
            format!("Remove {} worktrees", rows.len())
        },
        consent,
        process_warning: process.then(|| {
            "The OS hides some programs’ folder paths. Confirm only after checking that these folders are not in use."
                .to_owned()
        }),
    }
}

/// The termination cleanup's status line (`WorktreeCleanupStatus`).
pub fn cleanup_status(
    busy: bool,
    removed: usize,
    kept: usize,
    unknown: usize,
    target: Option<&str>,
    overflow: usize,
) -> String {
    let mut text = if busy {
        "Cleaning working folders…".to_owned()
    } else if kept == 1 && unknown == 0 && removed == 0 && target.is_some() {
        format!("Folder kept: {}.", target.unwrap_or(""))
    } else if removed == 1 && kept == 0 && unknown == 0 && target.is_some() {
        format!("Folder removed: {}.", target.unwrap_or(""))
    } else {
        format!("Working folders: {removed} removed  {kept} kept  {unknown} unknown")
    };
    if !busy && unknown > 0 {
        text.push_str(" An unknown result will not be retried.");
    }
    if overflow > 0 {
        text.push_str(&format!(
            " {overflow} additional folders need review in Projects > Worktrees."
        ));
    }
    text
}

/// The most cleanup targets the status keeps; more go to Projects > Worktrees.
pub const CLEANUP_TARGETS: usize = 64;

/// A computer's cleanup policy (Settings > Worktrees).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub enabled: bool,
    pub after_session: bool,
    pub unused_integrated: bool,
    pub missing: bool,
    pub grace_hours: u32,
    /// The rest of the policy as read, sent back unchanged.
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

/// `hgs [@host] worktrees policy get --json`, or `set` with the policy on
/// standard input; 15 s, at most 128 KiB. Probe the help for
/// [`POLICY_MARKER`] first.
pub fn policy_invocation(computer: &ComputerId, set: Option<&Policy>) -> Invocation {
    let action = if set.is_some() { "set" } else { "get" };
    let stdin = set.map(|policy| serde_json::to_vec(policy).unwrap_or_default());
    call(
        hgs_args(computer, &["worktrees", "policy", action, "--json"]),
        stdin,
        15,
    )
}

pub fn policy_supported(help: &str) -> Result<(), String> {
    if help.contains(POLICY_MARKER) {
        Ok(())
    } else {
        Err("This computer is unavailable or needs an updated hgs CLI for cleanup settings.".into())
    }
}

/// A policy reply: anything incomplete or malformed fails closed and leaves
/// no policy to apply.
pub fn policy(exit: Exit, set: bool) -> Result<Policy, String> {
    let failed = || {
        if set {
            "Save was not confirmed. Refresh before changing settings again."
        } else {
            "Could not read cleanup settings."
        }
        .to_owned()
    };
    let Exit::Code {
        code: 0, stdout, ..
    } = exit
    else {
        return Err(failed());
    };
    let reply: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
    let policy = &reply["policy"];
    let grace = policy["grace_hours"].as_i64().unwrap_or(0);
    let valid = reply["schema"].as_i64() == Some(1)
        && policy["version"].as_i64() == Some(1)
        && policy["revision"].is_number()
        && (1..=8760).contains(&grace)
        && ["enabled", "after_session", "unused_integrated", "missing"]
            .iter()
            .all(|key| policy[key].is_boolean());
    if !valid {
        return Err("Computer returned invalid cleanup settings.".into());
    }
    serde_json::from_value(policy.clone())
        .map_err(|_| "Computer returned invalid cleanup settings.".to_owned())
}

/// Which policy controls are enabled: none without a valid policy or while
/// busy; the rules only with cleanup enabled; the grace only with the
/// periodic rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyControls {
    pub apply: bool,
    pub enabled: bool,
    pub rules: bool,
    pub grace: bool,
}

pub fn policy_controls(loaded: bool, busy: bool, enabled: bool, unused: bool) -> PolicyControls {
    let ready = loaded && !busy;
    PolicyControls {
        apply: ready,
        enabled: ready,
        rules: ready && enabled,
        grace: ready && enabled && unused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked(verdict: &str) -> Value {
        json!({"kind": "linked", "verdict": verdict, "path": "/home/example/repo-work-1", "fingerprint": "f1",
               "integration": {"kind": "ancestor", "base": "main"}, "changes": 0, "untracked": 0, "ignored_count": 0,
               "reasons": []})
    }

    #[test]
    fn rows_fall_into_qt_groups() {
        assert_eq!(category(&json!({"kind": "main"}), true), 4);
        assert_eq!(category(&linked("ready"), true), 0);
        assert_eq!(category(&linked("missing"), true), 0);
        assert_eq!(category(&linked("review"), true), 1);
        assert_eq!(category(&linked("ready"), false), 3, "unverified");
        let mut busy = linked("blocked");
        busy["reasons"] = json!([{"code": "sessions", "message": "A session uses it."}]);
        assert_eq!(category(&busy, true), 2);
        let mut unknown = linked("blocked");
        unknown["reasons"] = json!([{"code": "processes_unavailable", "message": "x"}]);
        assert_eq!(category(&unknown, true), 3);
        // Local data with no fingerprint to review it with is not checked.
        let mut local = linked("ready");
        local["ignored_count"] = json!(3);
        assert_eq!(category(&local, true), 3);
        local["ignored_fingerprint"] = json!("i1");
        assert_eq!(
            category(&local, true),
            1,
            "ready with local data still needs review"
        );
        assert_eq!(category_name(2), "In use");
    }

    #[test]
    fn texts_follow_qt() {
        let mut row = linked("review");
        row["changes"] = json!(2);
        row["ignored_bytes"] = json!(1536.0);
        row["ignored_count"] = json!(1);
        row["reasons"] = json!([{"code": "changes", "message": "2 changed files"}, {"code": "locked", "message": "Locked by git."}]);
        assert_eq!(
            summary(&row),
            [
                "Merged into main",
                "Update Zerus on this computer to review local files before cleanup",
                "2 changed",
                "Local files outside Git",
                "Locked by git."
            ]
        );
        let lines = decision(&row);
        assert_eq!(lines[0], "Code: Merged into main (local Git refs).");
        assert_eq!(lines[1], "Unsaved work: 2 changed and 0 untracked files.");
        assert!(lines.contains(&"Local data: 1.50 KiB outside Git. These files may include caches, builds or unique data; Git cannot restore them.".to_owned()));
        let mut hidden = linked("blocked");
        hidden["unreadable_processes"] = json!((0..7)
            .map(|pid| json!({"command": "node", "pid": pid}))
            .collect::<Vec<_>>());
        let lines = decision(&hidden);
        assert!(lines.contains(&"Cannot check: node (PID 4)".to_owned()));
        assert!(!lines.contains(&"Cannot check: node (PID 5)".to_owned()));
        assert!(lines.contains(&"2 more processes could not be checked.".to_owned()));
        assert_eq!(
            decision(&linked("missing"))[1],
            "The folder is already gone. Cleanup only forgets its Git worktree record."
        );
        assert_eq!(
            size(&json!({"disk_bytes": 3221225472.0, "disk_complete": false})),
            "≥ 3.00 GiB"
        );
        assert_eq!(size(&json!({"verdict": "ready"})), "Not measured");
        assert_eq!(data_size(512.0), "512 bytes");
        assert_eq!(integration(&json!({"merged": true})), "Merged");
        assert_eq!(integration(&json!({})), "Integration not verified");
    }

    #[test]
    fn only_explicit_reviewed_removals_qualify() {
        assert!(removable(&linked("ready")));
        assert!(!removable(&linked("blocked")));
        assert!(!removable(
            &json!({"kind": "main", "verdict": "ready", "fingerprint": "f"})
        ));
        let mut local = linked("ready");
        local["ignored_count"] = json!(2);
        assert!(!removable(&local), "local data needs its fingerprint");
        local["ignored_fingerprint"] = json!("i1");
        assert!(removable(&local));
        let mut hidden = linked("blocked");
        hidden["reasons"] = json!([{"code": "processes_unavailable", "message": "x"}]);
        assert!(!process_reviewable(&hidden), "needs a process fingerprint");
        hidden["process_fingerprint"] = json!("p1");
        assert!(process_reviewable(&hidden));
        let confirm = remove_confirm(&[&hidden, &local]);
        assert_eq!(confirm.button, "Remove 2 worktrees");
        assert_eq!(
            confirm.consent.as_deref(),
            Some("I checked these folders are not in use; remove them anyway. Delete the reviewed local files outside Git too.")
        );
        assert!(confirm.process_warning.is_some());
        assert_eq!(
            remove_confirm(&[&local]).consent.as_deref(),
            Some("I reviewed the local files outside Git and want to delete them too")
        );
        assert_eq!(remove_confirm(&[&linked("ready")]).consent, None);
        // Exit cleanup asks nothing only for a clean, integrated, owned checkout.
        let mut owned = linked("ready");
        owned["zerus_owned"] = json!(true);
        owned["archive_restore_ready"] = json!(true);
        owned["disk_complete"] = json!(true);
        owned["ignored_complete"] = json!(true);
        assert!(exit_cleanup_ready(&owned));
        owned["ignored_count"] = json!(4);
        assert!(!exit_cleanup_ready(&owned));
        owned["generated_files_only"] = json!(true);
        assert!(exit_cleanup_ready(&owned));
        owned["integration"]["kind"] = json!("remaining");
        assert!(!exit_cleanup_ready(&owned));
    }

    #[test]
    fn removals_report_done_refused_or_unknown() {
        let removal = Removal {
            path: "/r-w".into(),
            common_dir: "/r/.git".into(),
            fingerprint: "f1".into(),
            delete_branch: true,
            process_fingerprint: "p1".into(),
            ..Removal::default()
        };
        let inv = remove_invocation(&ComputerId::from("studio"), &removal, "req-1");
        assert_eq!(
            inv.args,
            [
                "@studio",
                "worktrees",
                "remove",
                "--path",
                "/r-w",
                "--common-dir",
                "/r/.git",
                "--fingerprint",
                "f1",
                "--request-id",
                "req-1",
                "--json",
                "--delete-branch",
                "--process-fingerprint",
                "p1"
            ]
        );
        assert_eq!(inv.timeout, Duration::from_secs(330));
        let exit = |code: i32, reply: Value, stderr: &str| Exit::Code {
            code,
            stdout: reply.to_string().into_bytes(),
            stderr: stderr.into(),
        };
        let ok = json!({"request_id": "req-1", "path": "/r-w", "status": "removed"});
        assert!(matches!(
            removed(exit(0, ok, ""), "", "req-1", &removal),
            Removed::Done { .. }
        ));
        let kept = json!({"request_id": "req-1", "path": "/r-w", "common_dir": "/r/.git", "operation": "worktree_remove",
                          "outcome_version": 1, "status": "kept", "detail": "A program uses the folder."});
        assert_eq!(
            removed(exit(1, kept.clone(), ""), "", "req-1", &removal),
            Removed::Refused {
                reply: kept,
                detail: "A program uses the folder.".into()
            }
        );
        // Another request's reply, a timeout or silence are unknown, never retried.
        let other = json!({"request_id": "req-2", "path": "/r-w", "status": "removed"});
        assert_eq!(
            removed(exit(0, other, ""), "", "req-1", &removal),
            Removed::Unknown { detail: "Removal could not be confirmed. Check the folder before deciding what to do next.".into() }
        );
        assert!(matches!(
            removed(Exit::TimedOut, "", "req-1", &removal),
            Removed::Unknown { .. }
        ));
        assert_eq!(
            removed(
                exit(2, Value::Null, "usage: hgs worktrees --path P"),
                "studio",
                "req-1",
                &removal
            ),
            Removed::Unknown {
                detail: "Update Zerus on studio to clean up worktrees.".into()
            }
        );
    }

    #[test]
    fn reviews_merge_targets_and_expire() {
        let full = json!({"state": "ok", "sampled_at": 100.0, "worktrees": [linked("ready"), {"kind": "linked", "path": "/b", "verdict": "review"}]});
        let merged = merge_review(None, &full, "", 100.0);
        assert_eq!(merged["worktrees"].as_array().unwrap().len(), 2);
        let target = json!({"state": "ok", "sampled_at": 200.0, "worktrees": [{"kind": "linked", "path": "/b", "verdict": "ready"}]});
        let merged = merge_review(Some(&merged), &target, "/b", 200.0);
        let rows = merged["worktrees"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["verdict"], "ready");
        assert_eq!(rows[0]["sampled_at"], json!(100.0));
        assert_eq!(merged["targeted_paths"], json!(["/b"]));
        let current = current_review(&merged, 60.0, 230.0).unwrap();
        assert_eq!(
            current["worktrees"].as_array().unwrap().len(),
            1,
            "the older row expired"
        );
        assert!(current_review(&merged, 60.0, 300.0).is_none());
        assert_eq!(
            review(
                Exit::Code {
                    code: 0,
                    stdout: b"{}".to_vec(),
                    stderr: String::new()
                },
                "",
                false
            ),
            Err("Invalid worktree review response".into())
        );
        assert_eq!(
            review(
                Exit::Code {
                    code: 0,
                    stdout: full.to_string().into_bytes(),
                    stderr: String::new()
                },
                "",
                true
            ),
            Err("Folder changed. Refresh its cleanup review.".into())
        );
        assert_eq!(
            review_invocation(&ComputerId::local(), "/r", "/b").args,
            [
                "worktrees",
                "review",
                "--path",
                "/r",
                "--json",
                "--worktree",
                "/b"
            ]
        );
    }

    #[test]
    fn a_policy_fails_closed() {
        let exit = |reply: Value| Exit::Code {
            code: 0,
            stdout: reply.to_string().into_bytes(),
            stderr: String::new(),
        };
        let good = json!({"schema": 1, "policy": {"version": 1, "revision": 4, "enabled": true, "after_session": true,
                                                   "unused_integrated": false, "missing": true, "grace_hours": 72}});
        let policy = policy(exit(good.clone()), false).unwrap();
        assert_eq!((policy.enabled, policy.grace_hours), (true, 72));
        assert_eq!(policy.rest["revision"], json!(4), "sent back unchanged");
        let invalid = "Computer returned invalid cleanup settings.";
        for (key, value) in [
            ("grace_hours", json!(0)),
            ("grace_hours", json!(9000)),
            ("missing", json!("yes")),
            ("version", json!(2)),
        ] {
            let mut bad = good.clone();
            bad["policy"][key] = value;
            assert_eq!(
                super::policy(exit(bad), false),
                Err(invalid.into()),
                "{key}"
            );
        }
        let mut partial = good.clone();
        partial["policy"].as_object_mut().unwrap().remove("enabled");
        assert_eq!(super::policy(exit(partial), false), Err(invalid.into()));
        assert_eq!(
            super::policy(Exit::TimedOut, true),
            Err("Save was not confirmed. Refresh before changing settings again.".into())
        );
        assert_eq!(
            super::policy(Exit::TimedOut, false),
            Err("Could not read cleanup settings.".into())
        );
        assert!(policy_supported("... worktree-maintenance-v1 ...").is_ok());
        assert!(policy_supported("old help").is_err());
        let set = policy_invocation(&ComputerId::local(), Some(&policy));
        assert_eq!(set.args, ["worktrees", "policy", "set", "--json"]);
        let sent: Value = serde_json::from_slice(set.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(sent, good["policy"]);
        assert_eq!(
            policy_controls(false, false, true, true),
            PolicyControls {
                apply: false,
                enabled: false,
                rules: false,
                grace: false
            }
        );
        assert_eq!(
            policy_controls(true, false, true, false),
            PolicyControls {
                apply: true,
                enabled: true,
                rules: true,
                grace: false
            }
        );
        assert_eq!(
            cleanup_status(false, 0, 1, 1, Some("repo-work-1"), 2),
            "Working folders: 0 removed  1 kept  1 unknown An unknown result will not be retried. 2 additional folders need review in Projects > Worktrees."
        );
        assert_eq!(
            cleanup_status(false, 1, 0, 0, Some("repo-work-1"), 0),
            "Folder removed: repo-work-1."
        );
    }
}
