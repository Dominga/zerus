//! Process observations settle display state only. Never stop a process, invent
//! a provider completion, rebind a PID, or turn an opaque native job ID into one.
use super::*;
use serde::{Deserialize, Serialize};

pub(crate) const ABI: &str = "subagent-retire-v1";
const CAP: usize = 64;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Original {
    pid: u32,
    birth: String,
    run_id: String,
    conversation_id: String,
    parent_pid: u32,
    parent_birth: String,
    parent_start: String,
}

fn original(child: &Value) -> Option<Original> {
    let value = &child["original_process"];
    if value.as_object()?.len() != 7 {
        return None;
    }
    for key in [
        "birth",
        "run_id",
        "conversation_id",
        "parent_birth",
        "parent_start",
    ] {
        let text = value[key].as_str()?;
        if text.is_empty() || text.len() > 200 || text.chars().any(char::is_control) {
            return None;
        }
    }
    serde_json::from_value(value.clone()).ok()
}

pub(super) fn capture(record: &Value, pid: u32) -> Option<Value> {
    let identity = crate::platform::process_identity(pid)?;
    let parent_pid = record["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())?;
    let parent = crate::platform::process_identity(parent_pid)?;
    if pid <= 1
        || identity.birth.is_empty()
        || string(record, "run_id").is_empty()
        || string(record, "conversation_id").is_empty()
    {
        return None;
    }
    serde_json::to_value(Original {
        pid,
        birth: identity.birth,
        run_id: string(record, "run_id").into(),
        conversation_id: string(record, "conversation_id").into(),
        parent_pid,
        parent_birth: parent.birth,
        parent_start: string(record, "process_start").into(),
    })
    .ok()
}

fn original_status(record: &Value, child: &Value) -> &'static str {
    let Some(original) = original(child) else {
        return "unknown";
    };
    if original.pid <= 1
        || original.birth.is_empty()
        || original.run_id != string(record, "run_id")
        || original.conversation_id != string(record, "conversation_id")
    {
        return "unknown";
    }
    match crate::platform::process_identity(original.pid) {
        Some(current) if current.birth != original.birth => "ended",
        Some(_) => "live",
        None if !crate::platform::process_alive(original.pid) => "ended",
        None => "unknown",
    }
}
fn process(record: &Value, child: &Value) -> &'static str {
    match original_status(record, child) {
        "live" => {
            let Some(original) = original(child) else {
                return "unknown";
            };
            if record["pid"].as_u64() != Some(u64::from(original.parent_pid))
                || string(record, "process_start") != original.parent_start
                || original.parent_start.is_empty()
                || crate::platform::process_identity(original.parent_pid)
                    .is_none_or(|parent| parent.birth != original.parent_birth)
            {
                "unknown"
            } else {
                "live"
            }
        }
        status => status,
    }
}

pub(super) fn reconcile(record: &mut Value) {
    let changes: Vec<_> = record["subagents"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, child)| child["external"] == true && child["state"] == "working")
        .enumerate()
        .map(|(index, (id, child))| {
            (
                id.clone(),
                if index < CAP {
                    process(record, child)
                } else {
                    "unknown"
                },
            )
        })
        .collect();
    for (id, status) in changes {
        let child = &mut record["subagents"][&id];
        child["process_state"] = json!(status);
        if status == "ended" {
            child["state"] = json!("ended");
            child["display_state"] = json!("ended");
            child["completion_source"] = json!("original_process_exit");
            child["current_tool"] = json!("");
        } else if status == "unknown" && child["display_state"] != "unknown" {
            child["process_display_state"] = child["display_state"].clone();
            child["display_state"] = json!("unknown");
            child["process_unknown"] = json!(true);
        } else if status == "live" && child["display_state"] == "unknown" {
            // Only our own temporary process-availability projection is cleared.
            // Provider Interrupt/StopFailure unknown evidence remains unknown.
            if child["process_unknown"] == true {
                let previous = child
                    .as_object_mut()
                    .unwrap()
                    .remove("process_display_state");
                if let Some(previous) = previous.filter(Value::is_string) {
                    child["display_state"] = previous;
                } else {
                    child.as_object_mut().unwrap().remove("display_state");
                }
            }
        }
        if status != "unknown" {
            child["process_unknown"] = json!(false);
            child
                .as_object_mut()
                .unwrap()
                .remove("process_display_state");
        }
    }
    journal::settle_children(record);
    if record["main_done"] == true
        && ["idle", "working"].contains(&string(record, "phase"))
        && record["subagents"].as_object().is_some_and(|children| {
            children
                .values()
                .any(|child| telemetry::child_working(record, child))
        })
    {
        record["activity"] = json!("busy");
        record["phase"] = json!("working");
    }
}

pub(super) fn recorded_process_unknown(job: &Value) -> bool {
    job.get("os_pid").is_some() && !own_process_live(job)
}
pub(super) fn own_process_live(job: &Value) -> bool {
    let pid = job["os_pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 1);
    let birth = string(job, "process_birth");
    !birth.is_empty()
        && pid.is_some_and(|pid| {
            crate::platform::process_identity(pid).is_some_and(|identity| identity.birth == birth)
        })
}

pub(super) fn owner_unknown(record: &Value, owner: &str) -> bool {
    let child = &record["subagents"][owner];
    child["external"] == true
        && (child["state"] == "ended"
            || child["display_state"] == "unknown"
            || process(record, child) != "live")
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Retire {
    request_id: String,
    expected_run_id: String,
    expected_conversation_id: String,
    agent_id: String,
    expected_updated: f64,
}

pub(crate) fn retire(name: &str, bytes: &[u8]) -> Result<Value> {
    if bytes.len() > 8192 {
        return Err("Subagent retirement request exceeds 8 KiB".into());
    }
    let request: Retire =
        serde_json::from_slice(bytes).map_err(|_| "Invalid subagent retirement request")?;
    if uuid::Uuid::parse_str(&request.request_id).is_err()
        || request.agent_id.is_empty()
        || request.agent_id.len() > 160
        || request.agent_id.contains(['/', '\\'])
        || request.agent_id.chars().any(char::is_control)
        || !request.expected_updated.is_finite()
        || request.expected_updated <= 0.0
        || request.expected_run_id.is_empty()
        || request.expected_conversation_id.is_empty()
    {
        return Err("Invalid subagent retirement identity".into());
    }
    let _guard = writer_until(std::time::Instant::now() + std::time::Duration::from_secs(2))?
        .ok_or("Session writer is busy; retirement did not run")?;
    let mut record = storage::read_bounded(name, 16 * 1024 * 1024)?;
    let (result, changed) = retire_record(&mut record, request)?;
    if changed {
        write(&mut record)?;
    }
    Ok(result)
}

fn retire_record(record: &mut Value, request: Retire) -> Result<(Value, bool)> {
    let original = serde_json::to_value(&request).map_err(|e| e.to_string())?;
    let result = json!({"abi":ABI,"request_id":request.request_id,"accepted":true,
        "agent_id":request.agent_id,"state":"ended","native_completion":false});
    if record["run_id"] != request.expected_run_id
        || record["conversation_id"] != request.expected_conversation_id
    {
        return Err("Selected session generation changed".into());
    }
    if let Some(receipt) = record["subagent_retirements"].get(&request.request_id) {
        if receipt["original"] != original {
            return Err("Subagent retirement request identity reused".into());
        }
        if receipt["result"] != result {
            return Err("Invalid subagent retirement receipt".into());
        }
        return Ok((receipt["result"].clone(), false));
    }
    let child = record["subagents"]
        .get(&request.agent_id)
        .ok_or("Subagent is not in the selected conversation")?;
    if child["updated"].as_f64() != Some(request.expected_updated) {
        return Err("Subagent changed; inspect it again".into());
    }
    if child["group"] == true {
        return Err("A profile group is not one subagent record".into());
    }
    if child["external"] != true && telemetry::child_working(record, child) && process_alive(record)
    {
        return Err("The native subagent still has current work evidence".into());
    }
    if !["ended", "unknown"].contains(&string(child, "state"))
        && child["display_state"] != "unknown"
        && child["external"] != true
        && process_alive(record)
    {
        return Err("Subagent has no stale state to retire".into());
    }
    if original_status(record, child) == "live" {
        return Err("The original subagent process is still live".into());
    }
    if record
        .get("subagent_retirements")
        .is_some_and(|v| !v.is_object())
    {
        return Err("Invalid subagent retirement receipts".into());
    }
    if record["subagent_retirements"]
        .as_object()
        .is_some_and(|v| v.len() >= CAP)
    {
        return Err("Subagent retirement receipt limit reached".into());
    }
    // A second fresh identity observation immediately before the atomic record
    // change refuses an original that became verifiable while preparing it.
    if original_status(record, &record["subagents"][&request.agent_id]) == "live" {
        return Err("The original subagent process is still live".into());
    }
    let child = &mut record["subagents"][&request.agent_id];
    child["state"] = json!("ended");
    child["display_state"] = json!("ended");
    child["current_tool"] = json!("");
    child["updated"] = json!(now());
    child["completion_source"] = json!("explicit_retirement");
    if !record["subagent_retirements"].is_object() {
        record["subagent_retirements"] = json!({});
    }
    record["subagent_retirements"][&request.request_id] =
        json!({"original":original,"result":result});
    journal::settle_children(record);
    Ok((result, true))
}

#[cfg(test)]
mod tests;
