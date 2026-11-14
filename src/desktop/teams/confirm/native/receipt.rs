use super::*;
use crate::desktop::original_store::{self, Roots};
use sha2::{Digest, Sha256};
use zerus_client_core::desktop::{deferred_answer as answer, deferred_send as send};
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn invocation(row: &Record) -> Result<(Vec<String>, Vec<u8>, &'static str)> {
    let p = &row.plan;
    let audit = row
        .audit
        .as_ref()
        .ok_or("Missing durable audit reservation")?;
    let mut request = json!({"request_id":audit.request,"expected_run_id":p.snapshot.run,"expected_conversation_id":p.snapshot.conversation});
    let (command, folder) = match p.kind {
        Kind::Trust => {
            request["question_id"] = p.question["question_id"].clone();
            request["expected_question_hash"] = p.question["question_hash"].clone();
            request["answers"] = audit.answers.clone();
            ("answer", "question_receipts")
        }
        Kind::Resume => {
            request["action"] = json!(if p.snapshot.archive.is_some() {
                "restore"
            } else {
                "resume"
            });
            if let Some(id) = &p.snapshot.archive {
                request["archive_id"] = json!(id);
            }
            ("session-action", "action_receipts")
        }
        Kind::Brief => {
            request["text"] = json!(p.text);
            ("send", "input_receipts")
        }
        Kind::Notice => return Err("Not an effect".into()),
    };
    Ok((
        vec![command.into(), p.snapshot.name.clone(), "--json".into()],
        request.to_string().into_bytes(),
        folder,
    ))
}
pub(super) fn observe(roots: &Roots, row: &Record, input: &[u8]) -> Outcome {
    let Some(audit) = &row.audit else {
        return Outcome::Unknown("Missing audit reservation".into());
    };
    let Ok(id) = uuid::Uuid::parse_str(&audit.request) else {
        return Outcome::Unknown("Invalid original request ID".into());
    };
    let (bytes, _held) = original_store::read(roots, &id, 64 << 10, &|| {});
    let Ok(bytes) = bytes else {
        return Outcome::Unknown("The original receipt is missing, unsafe or changed".into());
    };
    project(row, input, &bytes)
}
pub(super) fn project(row: &Record, input: &[u8], bytes: &[u8]) -> Outcome {
    let p = &row.plan;
    let s = &p.snapshot;
    let Some(a) = &row.audit else {
        return Outcome::Unknown("Missing audit reservation".into());
    };
    match p.kind {
        Kind::Trust => {
            let plan = answer::Plan {
                session: s.name.clone(),
                request_id: a.request.clone(),
                run_id: s.run.clone(),
                conversation_id: s.conversation.clone(),
                question_id: p.question["question_id"].as_str().unwrap_or("").into(),
                question_hash: p.question["question_hash"].as_str().unwrap_or("").into(),
                identity_sha256: digest(input),
                message_sha256: digest(input),
                input_sha256: digest(input),
            };
            match answer::Outcome::read(&plan, bytes) {
                answer::Read::Recorded(_) => Outcome::Approved(None),
                answer::Read::Rejected => {
                    Outcome::Refused("The native CLI refused the exact question".into())
                }
                answer::Read::Unknown(why) => Outcome::Unknown(why.into()),
            }
        }
        Kind::Resume => resume(row, input, bytes),
        Kind::Brief => {
            let plan = send::Plan {
                session: s.name.clone(),
                request_id: a.request.clone(),
                run_id: s.run.clone(),
                conversation_id: s.conversation.clone(),
                message_sha256: digest(p.text.as_bytes()),
                input_sha256: digest(input),
            };
            match send::Outcome::from_receipt(&plan, bytes) {
                Ok(_) => Outcome::Approved(None),
                Err(why) => Outcome::Unknown(why.into()),
            }
        }
        Kind::Notice => Outcome::NotStarted("A notice cannot deliver".into()),
    }
}
fn resume(row: &Record, input: &[u8], bytes: &[u8]) -> Outcome {
    let p = &row.plan;
    let Ok(v) = crate::state::observe_held_lifecycle_receipt(&p.snapshot.name, input, Some(bytes))
    else {
        return Outcome::Unknown("The original lifecycle receipt is invalid".into());
    };
    if v["status"] == "failed" {
        // The observation protocol leaves the command's error out; the receipt
        // it has just matched to this exact request carries it.
        let receipt: Value = serde_json::from_slice(bytes).unwrap_or_default();
        return Outcome::Refused(refused_before_launch(
            p,
            receipt["error"].as_str().unwrap_or(""),
        ));
    }
    let t = &v["result_target"];
    if v["status"] != "completed"
        || t["run_id"].as_str().is_none_or(str::is_empty)
        || t["run_id"] == p.snapshot.run
    {
        return Outcome::Unknown("The new run was not established by the original receipt".into());
    }
    let mut target = p.snapshot.clone();
    target.run = t["run_id"].as_str().unwrap_or("").into();
    target.conversation = t["conversation_id"].as_str().unwrap_or("").into();
    target.name = t["name"].as_str().unwrap_or("").into();
    target.receipt_at = v["at"]
        .as_f64()
        .filter(|at| at.is_finite() && *at >= 0.0 && *at < i64::MAX as f64 / 1000.0)
        .map(|at| (at * 1000.0) as i64);
    if target.receipt_at.is_none() {
        return Outcome::Unknown("The completed receipt has no bounded timestamp".into());
    }
    target.archive = None;
    target.ended = false;
    target.self_exit = false;
    target.live = true;
    target.ready = false;
    target.questions.clear();
    target.exit_identity.clear();
    Outcome::Approved(Some(Box::new(target)))
}

/// The owner card's reason when the native resume or restore of an exited
/// member was refused before launch (zerus-63rru.62): the command's own
/// reason from its receipt, bounded and cleaned as the doorbell's refusal is,
/// and what the person can do.
fn refused_before_launch(p: &Plan, error: &str) -> String {
    let action = if p.snapshot.archive.is_some() {
        "restore"
    } else {
        "resume"
    };
    let reason = crate::desktop::teams::reason_text(error);
    let reason = if reason.is_empty() {
        "the command gave no reason".into()
    } else {
        reason
    };
    format!(
        "The {action} of its exited session was refused before launch: {reason}. {}",
        super::super::policy::SEAT
    )
}

#[cfg(test)]
mod tests;
