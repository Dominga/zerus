//! The wire: one checked invocation, and one outcome from its exit.
//!
//! `done` needs a complete receipt — `ok: true`, a revision, an actor and the
//! team the call named. Anything less may have reached the store, so it is
//! `uncertain`. Only a valid refusal, with a stable code, proves `failed`; an
//! exit code alone does not. The store's `storage_failed` is not a refusal:
//! the write's result is not known, so it is `uncertain` too.
use super::types::*;
use crate::actions::{Exit, Invocation};
use crate::run::run;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// How long an action waits for `hgs` (it is this computer's hgs).
const TIMEOUT: Duration = Duration::from_secs(15);

fn outcome_failed(detail: impl Into<String>, request_id: &str) -> TeamOutcome {
    TeamOutcome::Failed {
        detail: detail.into(),
        code: String::new(),
        request_id: request_id.to_owned(),
    }
}

fn outcome_uncertain(detail: impl Into<String>, request_id: &str) -> TeamOutcome {
    TeamOutcome::Uncertain {
        detail: detail.into(),
        request_id: request_id.to_owned(),
    }
}

/// The checked call: its command and its request id. A call that is not sent
/// gives the outcome instead: `confirm` without the person's confirmation,
/// `failed` for a request that cannot be made. The request id is the call's
/// own and is never replaced here.
pub fn prepare(call: &TeamCall) -> Result<(Invocation, String), TeamOutcome> {
    let request_id = call.request_id.clone();
    if !canonical_uuid(&request_id) {
        return Err(outcome_failed("Not a request id.", &request_id));
    }
    // A create names the team it makes; every other call names the team it
    // changes.
    if !crate::teams::read::valid_team(&call.team) {
        return Err(outcome_failed("Not a team id.", &request_id));
    }
    if let Err(detail) = call.action.check() {
        return Err(outcome_failed(detail, &request_id));
    }
    if !call.confirmed {
        if let Some(confirmation) = call.action.confirmation(&call.team_name) {
            return Err(TeamOutcome::Confirm { confirmation });
        }
    }
    let revision = if !call.action.names_team() {
        // A new team has no revision to compare: the store checks zero.
        0
    } else {
        match call.expected_revision {
            Some(revision) => revision,
            None => {
                return Err(outcome_failed(
                    "The team's revision is not known; read the team first.",
                    &request_id,
                ))
            }
        }
    };
    let mut input = call.action.input();
    input.insert("team".into(), json!(call.team));
    input.insert("expected_revision".into(), json!(revision));
    input.insert("request_id".into(), json!(request_id));
    let mut args = vec!["team".to_owned(), call.action.op().to_owned()];
    if let TeamAction::Create { template, .. } | TeamAction::StartPlanning { template, .. } = &call.action {
        args.push("--template".to_owned());
        args.push(template.clone());
    }
    args.push("--json".to_owned());
    Ok((
        Invocation {
            args,
            stdin: Some(Value::Object(input).to_string().into_bytes()),
            timeout: TIMEOUT,
            request_id: Some(request_id.clone()),
        },
        request_id,
    ))
}

fn stderr_text(stderr: &str) -> Option<String> {
    let text: String = stderr.trim().chars().take(DETAIL_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

/// Whether a success reply is the complete receipt of this call: `ok: true`, a
/// revision, an actor and the call's own team.
fn complete_receipt(call: &TeamCall, receipt: &Value) -> bool {
    receipt.get("ok").and_then(Value::as_bool) == Some(true)
        && receipt.get("revision").and_then(Value::as_u64).is_some()
        && receipt.get("actor").is_some_and(Value::is_object)
        && receipt.get("team").and_then(Value::as_str) == Some(call.team.as_str())
}

const READ_FIRST: &str = "Read the team before sending it again.";

/// The team store's code for a write whose storage failed or whose result was
/// lost: nothing proves that the write did not happen, and the store processes
/// the same request id when it is repeated. (`store_busy`, by contrast, says
/// nothing was written: it is a refusal like any other code.)
pub const STORAGE_FAILED: &str = zerus_team_plan::problem::code::STORAGE_FAILED;

/// A valid coded answer: a refusal is `failed`; a storage failure is
/// `uncertain`, keeping the call's request id and the store's own sentence,
/// so the person checks it with the same id and never sends a new one.
fn coded(code: String, message: String, request_id: &str) -> TeamOutcome {
    if code == STORAGE_FAILED {
        let detail = if message.trim().is_empty() {
            format!("The team store did not confirm the change. {READ_FIRST}")
        } else {
            message
        };
        return outcome_uncertain(detail, request_id);
    }
    TeamOutcome::Failed {
        detail: message,
        code,
        request_id: request_id.to_owned(),
    }
}

/// The outcome of a call's exit. Only a valid refusal or a call that never
/// started is `failed`; everything that may have reached the store without a
/// complete receipt, a storage failure included, is `uncertain`.
pub fn classify(call: &TeamCall, exit: Exit) -> TeamOutcome {
    let request_id = call.request_id.as_str();
    match exit {
        Exit::StartFailed(error) => {
            outcome_failed(format!("hgs did not start: {error}"), request_id)
        }
        Exit::TimedOut => outcome_uncertain(
            format!("hgs did not answer in time; the change may have been made. {READ_FIRST}"),
            request_id,
        ),
        Exit::WriteFailed => outcome_uncertain(
            format!(
                "The request did not reach hgs whole; the change may have been made. {READ_FIRST}"
            ),
            request_id,
        ),
        Exit::Abnormal { stderr } => outcome_uncertain(
            stderr_text(&stderr).map_or_else(
                || format!("hgs ended abnormally. {READ_FIRST}"),
                |text| format!("{text} {READ_FIRST}"),
            ),
            request_id,
        ),
        Exit::Code {
            code,
            stdout,
            stderr,
        } if code != 0 => match super::super::refusal::parse(&stdout) {
            Some((code, message)) => coded(code, message, request_id),
            // An exit code alone cannot prove that nothing was written: a
            // post-write failure or a lost receipt can end nonzero too.
            None => outcome_uncertain(
                stderr_text(&stderr)
                    .or_else(|| stderr_text(&String::from_utf8_lossy(&stdout)))
                    .map_or_else(
                        || {
                            format!(
                                "hgs exited with code {code} without a readable refusal; the change may have been made. {READ_FIRST}"
                            )
                        },
                        |text| format!("{text} {READ_FIRST}"),
                    ),
                request_id,
            ),
        },
        Exit::Code { stdout, .. } => {
            if let Some((code, message)) = super::super::refusal::parse(&stdout) {
                return coded(code, message, request_id);
            }
            let receipt: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
            if !complete_receipt(call, &receipt) {
                return outcome_uncertain(
                    format!("hgs gave no complete receipt. {READ_FIRST}"),
                    request_id,
                );
            }
            TeamOutcome::Done {
                receipt,
                request_id: request_id.to_owned(),
            }
        }
    }
}

/// Runs one call through `hgs`: blocks for at most its timeout plus a few
/// seconds; callers run it off the UI thread. Never sends it twice.
pub fn run_action(hgs: &Path, call: &TeamCall) -> TeamOutcome {
    match prepare(call) {
        Ok((invocation, _)) => classify(call, run(hgs, &invocation)),
        Err(outcome) => outcome,
    }
}
