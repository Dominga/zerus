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
    let mut changed = false;
    for (id, status) in changes {
        let child = &mut record["subagents"][&id];
        child["process_state"] = json!(status);
        if status == "ended" {
            child["state"] = json!("ended");
            child["display_state"] = json!("ended");
            child["completion_source"] = json!("original_process_exit");
            child["current_tool"] = json!("");
            changed = true;
        } else {
            // Explicit attention wins even when the original is unbound. Also
            // recover attention hidden by an earlier saved process projection.
            let attention = ["approval", "input", "attention"]
                .contains(&string(child, "process_display_state"));
            if child["display_state"] == "unknown"
                && child["process_unknown"] == true
                && (status == "live" || attention)
            {
                let previous = child
                    .as_object_mut()
                    .unwrap()
                    .remove("process_display_state");
                if let Some(previous) = previous.filter(Value::is_string) {
                    changed |= child["display_state"] != previous;
                    child["display_state"] = previous;
                } else {
                    child.as_object_mut().unwrap().remove("display_state");
                    changed = true;
                }
            }
            // Uncertain process evidence hides Working, not a pending request,
            // paused/error display, or the provider's own Unknown evidence.
            if status == "unknown" && ["", "working"].contains(&string(child, "display_state")) {
                child["process_display_state"] = child["display_state"].clone();
                child["display_state"] = json!("unknown");
                child["process_unknown"] = json!(true);
                changed = true;
            }
        }
        if status != "unknown" || child["display_state"] != "unknown" {
            child["process_unknown"] = json!(false);
            child
                .as_object_mut()
                .unwrap()
                .remove("process_display_state");
        }
    }
    // Refreshing process metadata does not own the parent's native turn. Only
    // an external work/display transition may settle or re-derive that turn.
    if !changed {
        return;
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

/// The shortest silence the record must show before a screen read may settle it.
const SETTLE_QUIET: f64 = 90.0;
const SETTLE_ABI: &str = "screen-settle-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Settle {
    request_id: String,
    expected_run_id: String,
    expected_conversation_id: String,
    quiet_seconds: f64,
}

/// Task 181: Zerus read the session's screen twice (a finished turn, an idle
/// prompt, no agent row, no shell) while its hooks still say working: a Stop or
/// a SubagentStop never arrived. A subagent cannot outlive its parent's turn,
/// so the record is settled: idle, the parent's turn done, the children whose
/// stop never came ended (`completion_source: screen_settle`). Refused while a
/// request or compaction waits, a native child shows live evidence, or an event
/// is recent. Idempotent on the request id.
pub(crate) fn settle(name: &str, bytes: &[u8]) -> Result<Value> {
    if bytes.len() > 4096 {
        return Err("Settle request exceeds 4 KiB".into());
    }
    let request: Settle = serde_json::from_slice(bytes).map_err(|_| "Invalid settle request")?;
    if uuid::Uuid::parse_str(&request.request_id).is_err()
        || request.expected_run_id.is_empty()
        || request.expected_conversation_id.is_empty()
        || !request.quiet_seconds.is_finite()
    {
        return Err("Invalid settle identity".into());
    }
    let _guard = writer_until(std::time::Instant::now() + std::time::Duration::from_secs(2))?
        .ok_or("Session writer is busy; the settle did not run")?;
    let mut record = storage::read_bounded(name, 16 * 1024 * 1024)?;
    let (result, changed) = settle_record(&mut record, request, now())?;
    if changed {
        write(&mut record)?;
    }
    Ok(result)
}

fn settle_record(record: &mut Value, request: Settle, at: f64) -> Result<(Value, bool)> {
    if record["run_id"] != request.expected_run_id || record["conversation_id"] != request.expected_conversation_id {
        return Err("Selected session generation changed".into());
    }
    if let Some(receipt) = record["screen_settles"].get(&request.request_id) {
        return Ok((receipt["result"].clone(), false));
    }
    // A DeepSeek session's state is its host's, never this record's.
    if string(record, "agent") == "dsh" || !process_alive(record) {
        return Err("The session is not a running hook-driven agent".into());
    }
    let busy = record["activity"] == "busy" || ["working", "tool"].contains(&string(record, "phase"));
    if !busy {
        return Ok((json!({"abi":SETTLE_ABI,"request_id":request.request_id,"settled":true,"already_idle":true,"children":[]}), false));
    }
    if ["input", "approval", "question", "error", "compacting"].contains(&string(record, "phase"))
        || record["pending_approval"].is_object()
    {
        return Err("The session waits for a request or compaction".into());
    }
    let last = record["last_event_at"].as_f64().unwrap_or(0.0);
    if request.quiet_seconds < SETTLE_QUIET || (last > 0.0 && at - last < SETTLE_QUIET) {
        return Err("The session had an event too recently".into());
    }
    if telemetry::grouped_active(record) > 0 {
        return Err("A profile group still has work".into());
    }
    let clearable = |record: &Value, child: &Value| {
        child["external"] != true || original_status(record, child) == "ended"
    };
    let working: Vec<String> = record["subagents"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, child)| telemetry::child_working(record, child))
        .map(|(id, _)| id.clone())
        .collect();
    if working.iter().any(|id| !clearable(record, &record["subagents"][id])) {
        return Err("A background job still has current work evidence".into());
    }
    for id in &working {
        let child = &mut record["subagents"][id];
        child["state"] = json!("ended");
        child["display_state"] = json!("ended");
        child["current_tool"] = json!("");
        child["updated"] = json!(at);
        child["completion_source"] = json!("screen_settle");
    }
    record["activity"] = json!("idle");
    record["phase"] = json!("idle");
    record["main_done"] = json!(true);
    record["active_tools"] = json!({});
    let result = json!({"abi":SETTLE_ABI,"request_id":request.request_id,"settled":true,"children":working});
    if !record["screen_settles"].is_object() {
        record["screen_settles"] = json!({});
    }
    let receipts = record["screen_settles"].as_object_mut().unwrap();
    while receipts.len() >= CAP {
        let Some(oldest) = receipts.iter().min_by(|a, b| a.1["at"].as_f64().unwrap_or(0.0).total_cmp(&b.1["at"].as_f64().unwrap_or(0.0))).map(|(key, _)| key.clone()) else { break };
        receipts.remove(&oldest);
    }
    receipts.insert(request.request_id.clone(), json!({"at":at,"result":result}));
    Ok((result, true))
}

#[cfg(test)]
mod settle_tests {
    use super::*;

    fn stale(at: f64) -> Value {
        json!({"agent":"claude","name":"claude/x/merger","pid":std::process::id(),
            "process_start":process_start(std::process::id()),"run_id":"run","conversation_id":"conv",
            "activity":"busy","phase":"working","main_done":true,"last_event_at":at - 600.0,"active_tools":{},
            "subagents":{"agent-a":{"state":"working","display_state":"working","name":"reviewer","started":1.0,"updated":2.0,"announced":true}}})
    }
    fn request(quiet: f64) -> Settle {
        Settle { request_id: uuid::Uuid::new_v4().to_string(), expected_run_id: "run".into(), expected_conversation_id: "conv".into(), quiet_seconds: quiet }
    }

    #[test]
    fn a_finished_turn_whose_subagent_stop_never_came_is_settled() {
        let at = now();
        let mut record = stale(at);
        assert!(telemetry::work_remains(&record), "one subagent that never stopped keeps it working");
        let asked = request(600.0);
        let id = asked.request_id.clone();
        let (result, changed) = settle_record(&mut record, asked, at).unwrap();
        assert!(changed && result["settled"] == true && result["children"] == json!(["agent-a"]), "{result}");
        assert_eq!((record["activity"].clone(), record["phase"].clone(), record["main_done"].clone()), (json!("idle"), json!("idle"), json!(true)));
        assert_eq!(record["subagents"]["agent-a"]["completion_source"], "screen_settle");
        assert!(!telemetry::work_remains(&record));
        // The same request again is the same answer, nothing more written.
        let again = Settle { request_id: id, expected_run_id: "run".into(), expected_conversation_id: "conv".into(), quiet_seconds: 600.0 };
        let (second, changed) = settle_record(&mut record, again, at + 1.0).unwrap();
        assert!(!changed && second == result);
    }

    #[test]
    fn it_refuses_what_a_screen_cannot_prove() {
        let at = now();
        // A recent event, a short quiet, another generation, a question waiting.
        let mut recent = stale(at);
        recent["last_event_at"] = json!(at - 10.0);
        assert!(settle_record(&mut recent, request(600.0), at).is_err());
        assert!(settle_record(&mut stale(at), request(30.0), at).is_err());
        let mut other = stale(at);
        other["run_id"] = json!("other");
        assert!(settle_record(&mut other, request(600.0), at).is_err());
        let mut asking = stale(at);
        asking["phase"] = json!("approval");
        assert!(settle_record(&mut asking, request(600.0), at).is_err());
        // A DeepSeek record is its host's; a background job with no proof of exit stays.
        let mut dsh = stale(at);
        dsh["agent"] = json!("dsh");
        assert!(settle_record(&mut dsh, request(600.0), at).is_err());
        let mut job = stale(at);
        job["subagents"]["external:codex:x"] = json!({"external":true,"state":"working","display_state":"working","updated":2.0});
        let kept = job.clone();
        assert!(settle_record(&mut job, request(600.0), at).is_err());
        assert_eq!(job, kept, "a refusal changes nothing");
        // An idle record has nothing to settle.
        let mut idle = stale(at);
        idle["activity"] = json!("idle");
        idle["phase"] = json!("idle");
        let (result, changed) = settle_record(&mut idle, request(600.0), at).unwrap();
        assert!(!changed && result["already_idle"] == true);
    }
}

#[cfg(test)]
mod tests;
