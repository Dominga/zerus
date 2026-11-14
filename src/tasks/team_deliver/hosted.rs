//! Teams retains the exact input before reserving its attempt. Only the
//! witness's Absent answer permits offering that same input after a restart.
use super::*;
use crate::state::dsh::witness::{self, NotOfferedCode, Recorded, Witnessed};
use std::time::Duration;

const KEY: &str = "hosted-input:";
const CONFLICT: &str = "The DeepSeek request ID belongs to another input; nothing was sent. The task waits for the lead.";

fn ledger(config: &Config) -> Result<crate::desktop::teams::ActorLedger> {
    crate::desktop::teams::ActorLedger::open(&config.state).map_err(problem)
}

pub(super) fn reserve(config: &Config, body: &Input, to: &SessionKey, input: &Value) -> Result<()> {
    if !to.session.starts_with("dsh/") {
        return Ok(());
    }
    let mut ledger = ledger(config)?;
    let key = format!("{KEY}{}", body.id);
    let data = json!({"input":input,"to":to,"body":body,"reply":null});
    if let Some(row) = ledger.input(&body.team, &key).map_err(problem)? {
        if row.data["input"] != *input
            || row.data["to"] != json!(to)
            || row.data["body"] != json!(body)
        {
            return Err(rule(CONFLICT));
        }
        return Ok(());
    }
    // A separate bounded table retains the CLI's complete input, including
    // attachments. The database's total page budget still applies.
    ledger
        .reserve_input(&body.team, &key, data)
        .map_err(problem)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn recover(
    config: &Config,
    body: &Input,
    task: &TaskRef,
    native: &zerus_replica::execution::Delivery,
    store: &Store,
    home: &str,
    deadline: Instant,
) -> Result<Value> {
    let Some(row) = ledger(config)?
        .input(&body.team, &format!("{KEY}{}", body.id))
        .map_err(problem)?
    else {
        return Ok(body.reply(
            "uncertain",
            "The retained DeepSeek request is unavailable; nothing was sent again.",
            None,
        ));
    };
    let to: SessionKey = serde_json::from_value(row.data["to"].clone()).map_err(fail)?;
    let request = &row.data["input"];
    let retained = attempts::native(&to, request);
    if row.data["body"] != json!(body)
        || retained.machine != native.machine
        || retained.name != native.name
        || retained.run_id != native.run_id
        || retained.conversation_id != native.conversation_id
        || retained.digest != native.digest
    {
        return Ok(final_reply(body, CONFLICT));
    }
    if !row.data["reply"].is_null() {
        if row.data["reply"]["cause"] == "dsh_not_offered" {
            attempts::not_admitted(config, &task.project, &task.uuid, &body.id)?;
        } else if row.data["reply"]["state"] == "submitted" {
            attempts::settle(
                config,
                &task.project,
                &task.uuid,
                &body.id,
                &row.data["reply"]["receipt"],
            )?;
        }
        return Ok(row.data["reply"].clone());
    }
    let outcome = match witness::recorded(&native.name, request) {
        Ok(Recorded::Absent) => {
            // Still the original open effect and member. No rendering, no
            // current-run replacement, and no new request ID on this path.
            permit(store, body, home)?;
            witness::submit_witnessed(
                &native.name,
                request,
                deadline.saturating_duration_since(Instant::now()),
            )
        }
        Ok(Recorded::Conflict) => return remember(config, body, final_reply(body, CONFLICT)),
        Ok(Recorded::Outcome(outcome)) => outcome,
        Err(error) => {
            return Ok(body.reply(
                "pending",
                &format!("The DeepSeek outcome cannot be read yet: {error}. Nothing was sent."),
                None,
            ))
        }
    };
    record(config, body, task, &to, request, outcome)
}

pub(super) fn deliver(
    config: &Config,
    task: &TaskRef,
    body: &Input,
    to: &SessionKey,
    input: &Value,
    deadline: Instant,
) -> Result<Value> {
    let outcome = witness::submit_witnessed(
        &to.session,
        input,
        deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(20)),
    );
    record(config, body, task, to, input, outcome)
}

fn final_reply(body: &Input, reason: &str) -> Value {
    let mut reply = body.reply("refused", reason, None);
    reply["final"] = json!(true);
    reply
}

fn remember(config: &Config, body: &Input, reply: Value) -> Result<Value> {
    #[cfg(test)]
    if std::env::var_os("HGS_TEST_TEAM_HOSTED_BEFORE_REPLY").is_some() {
        std::process::exit(78);
    }
    let mut ledger = ledger(config)?;
    let mut row = ledger
        .input(&body.team, &format!("{KEY}{}", body.id))
        .map_err(problem)?
        .ok_or_else(|| fail("The retained DeepSeek request disappeared"))?;
    if !row.data["reply"].is_null() {
        return Ok(row.data["reply"].clone());
    }
    let mut retained = reply.clone();
    if let Some(receipt) = retained.get_mut("receipt").and_then(Value::as_object_mut) {
        receipt.retain(|key, _| {
            [
                "request_id",
                "name",
                "run_id",
                "conversation_id",
                "digest",
                "status",
                "first_message",
                "transport",
                "witness",
                "witness_version",
                "digest_scope",
            ]
            .contains(&key.as_str())
        });
    }
    row.data["reply"] = retained;
    ledger.save_input(&mut row).map_err(problem)?;
    Ok(reply)
}

fn record(
    config: &Config,
    body: &Input,
    task: &TaskRef,
    to: &SessionKey,
    request: &Value,
    outcome: Witnessed,
) -> Result<Value> {
    let binding = attempts::native(to, request);
    let not_offered = matches!(&outcome, Witnessed::NotOffered { .. });
    let mut reply = match outcome {
        Witnessed::Receipt(receipt) => {
            // The task writer verifies every receipt field against its
            // retained native binding before changing the attempt.
            if receipt["request_id"] == body.id && receipt["name"] == binding.name
                && receipt["run_id"] == binding.run_id && receipt["conversation_id"] == binding.conversation_id
                && receipt["digest"] == binding.digest && receipt["status"] == "submitted" {
                body.reply("submitted", "", Some(receipt))
            } else {
                body.reply("uncertain", "The DeepSeek receipt names another input; nothing was sent again.", None)
            }
        }
        Witnessed::NotOffered { code, binding: proof, reason } => {
            if proof.request_id != body.id || proof.name != binding.name || proof.run_id != binding.run_id
                || proof.conversation_id != binding.conversation_id || proof.digest != binding.digest {
                return remember(config, body, body.reply("uncertain", "The DeepSeek refusal names another input; nothing was sent again.", None));
            }
            match code {
                NotOfferedCode::UnsafeContent => final_reply(body, "DeepSeek refused the task text because it contains a native session mention. The task waits for the lead; this input is never retried."),
                NotOfferedCode::Canceled => final_reply(body, "The queued task input was canceled before it entered the conversation. The task waits for the lead; it will not be handed out automatically."),
                NotOfferedCode::DroppedAfterClaim => final_reply(body, "DeepSeek claimed the task input but the turn ended without recording it. The task waits for the lead; this input is never retried."),
                NotOfferedCode::HostDisposed => body.reply("refused", "The DeepSeek host stopped the member before the task input entered the conversation. The member must be observed again.", None),
                NotOfferedCode::HostRefused => body.reply("refused", &format!("DeepSeek refused the task input: {reason}"), None),
            }
        }
        Witnessed::Unknown { code, .. } => body.reply("uncertain", &format!("The DeepSeek task input is uncertain ({}); it is never sent again or promoted by recovery.", code.code()), None),
    };
    // Retain the final non-offer before changing the task's attempt. A
    // crash before this write leaves an unstarted attempt that Absent may
    // still offer; a crash after it can only settle the known refusal.
    if not_offered {
        reply["cause"] = json!("dsh_not_offered");
    }
    let reply = remember(config, body, reply)?;
    if reply["cause"] == "dsh_not_offered" {
        attempts::not_admitted(config, &task.project, &task.uuid, &body.id)?;
    } else if reply["state"] == "submitted" {
        attempts::settle(
            config,
            &task.project,
            &task.uuid,
            &body.id,
            &reply["receipt"],
        )?;
    }
    Ok(reply)
}
