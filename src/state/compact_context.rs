//! Native manual compaction. A submitted command is not a completed compaction.
use super::*;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request { pub request_id: String, pub expected_run_id: String, pub expected_conversation_id: String }

pub(super) fn pending(record: &Value) -> bool {
    ["submitted", "compacting", "uncertain"].contains(&string(&record["compact_context_request"], "status"))
}
pub(super) fn check_continuation(record: &Value, id: &str) -> Result<()> {
    if id.is_empty() { return Ok(()); }
    let request = &record["compact_context_request"];
    if request["request_id"] != id || request["status"] != "completed" || !request["superseded_at"].is_null()
        || request["run_id"] != record["run_id"] || request["conversation_id"] != record["conversation_id"]
        || record["activity"] != "idle" || record["phase"] != "idle" {
        return Err("Compaction is not confirmed for this conversation; your message was not sent".into());
    }
    Ok(())
}
pub(super) fn observe(record: &mut Value, event: &Value) {
    if !string(event, "agent_id").is_empty() { return; }
    let mut request = record["compact_context_request"].clone();
    if !request.is_object() || request["run_id"] != record["run_id"]
        || request["conversation_id"] != record["conversation_id"] { return; }
    let status = string(&request, "status").to_owned();
    let kind = string(event, "hook_event_name");
    match kind {
        "PreCompact" if status == "submitted" && event["trigger"] == "manual" => {
            request["status"] = json!("compacting");
            request["turn_id"] = event["turn_id"].clone();
        }
        "PostCompact" if status == "compacting" && event["trigger"] == "manual"
            && request["turn_id"] == event["turn_id"] => {
            request["status"] = json!("completed");
            request["completed_at"] = json!(now());
        }
        "Interrupt" | "StopFailure" | "SessionEnd" | "UserPromptSubmit" | "PermissionRequest" => match status.as_str() {
            "submitted" | "compacting" | "uncertain" => request["status"] = json!("cancelled"),
            // A later turn cannot undo a finished compaction; it only ends the
            // continuation that compaction confirmed.
            "completed" if request["superseded_at"].is_null() => request["superseded_at"] = json!(now()),
            _ => return,
        },
        _ => return,
    }
    record["compact_context_request"] = request;
}
pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" { return Err("usage: hgs compact-context <session> --json".into()); }
    if opencode::exists(&args[0]) {return Err("OpenCode compaction is not yet qualified in this adapter".into());}
    if dsh::exists(&args[0]) { return dsh::dispatch("compact-context", args); }
    let mut bytes = Vec::new(); io::stdin().take(8193).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len() > 8192 { return Err("Compact request is too large".into()); }
    let request: Request = serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(|e|e.to_string())?;
    let receipt = submit(&args[0], request, None)?;
    println!("{receipt}"); Ok(0)
}

/// Shared native command path, including every manual compact guard.
pub(super) fn submit(name: &str, request: Request, prepared: Option<&Value>) -> Result<Value> {
    let uuid = uuid::Uuid::parse_str(&request.request_id).map_err(|_|"Invalid request identity")?;
    let guard = lock(None)?;
    let mut record = input::checked_ready(name, &request.expected_run_id, &request.expected_conversation_id)?;
    if prepared.is_some_and(|original| !kimi_expiry::same_identity(original,&record)
        || !kimi_expiry::owned(&record) || record["kimi_pre_send"] != original["kimi_pre_send"]) {
        return Err("Session process or pre-send custodian changed before compaction; no keys were typed".into());
    }
    if let Some(original) = prepared {
        let current = kimi_expiry::decision(&record);
        if current.as_ref().is_none_or(|current| ["model","cache_duration","min_tokens_to_hint","last_active_at","tokens"]
            .iter().any(|key| current[*key] != original["last_send_compaction"][*key])) {
            return Err("Cache-expiry inputs changed before compaction; no compact keys were typed".into());
        }
    }
    if !clear_context::available(&record, true) || record["pending_settings"].is_object() {
        return Err("Wait for the agent to become idle before compacting context".into());
    }
    input::checked_terminal(&record, true)?;
    let path = absolute_root()?.join("compact_receipts").join(format!("{uuid}.json"));
    if path.exists() { return Err("This compact was already attempted. Check Terminal before trying again".into()); }
    recovery::cancel_for_input(&record)?;
    let mut receipt = json!({"request_id":request.request_id,"name":name,"run_id":request.expected_run_id,
        "conversation_id":request.expected_conversation_id,"status":"submitted","at":now()});
    record["compact_context_request"] = receipt.clone();
    write(&mut record)?;
    atomic(&path, &json!({"status":"in_progress"}).to_string())?;
    let result: Result<()> = (|| {
        // Kimi 2.5.3 computeArgumentHint matches /^\/(\S+)( ?)$/.
        // Its hint rendering loses the composer border and hardware cursor.
        // Two trailing spaces hide that hint; Kimi trims the empty instruction.
        // Keep the exact command verifier below unchanged.
        // Evidence: artifacts/test-results/kimi-compact/diagnosis/RAW-SHA256SUMS
        // SHA-256: ddf8b09fc81ebfec93fdcae7a175a52e33920d6c0154ebf98e4b2ba2763ef383
        // Qwen Code names it /compress (0.27.4).
        let command = match string(&record, "agent") { "kimi" => "/compact  ", "qwen" => "/compress", _ => "/compact" };
        let typed = command.trim_end();
        backend::current().send(string(&record,"pane"), &[backend::Key::Literal(command)])?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = kimi_tui_choice::identity(&record)?;
            if clear_context::ready(&current, typed)? { break; }
            if Instant::now() >= deadline { return Err("Native /compact input could not be verified".into()); }
            std::thread::sleep(Duration::from_millis(40));
        }
        std::thread::sleep(clear_context::COMMAND_SETTLE);
        let current = input::checked_ready(name, &request.expected_run_id, &request.expected_conversation_id)?;
        kimi_tui_choice::identity(&record)?;
        input::checked_terminal(&current, false)?;
        if !clear_context::ready(&current, typed)? { return Err(format!("The native input changed before {typed}")); }
        backend::current().send(string(&current,"pane"), &[backend::Key::Named("Enter")])?;
        Ok(())
    })();
    // From the typed command on, every error may follow input (zerus-1wchz).
    let uncertain = |error: String| {
        format!("Compact delivery uncertain: {error}. Check Terminal before retrying")
    };
    if let Err(error) = result {
        receipt["status"] = json!("uncertain");
        record["compact_context_request"] = receipt.clone();
        let recorded = write(&mut record).and_then(|()| atomic(&path, &receipt.to_string()));
        return Err(uncertain(match recorded {
            Ok(()) => error,
            Err(failed) => format!("{error}; {failed}"),
        }));
    }
    drop(guard); // Native hooks must be free to report start/completion.
    atomic(&path, &receipt.to_string()).map_err(uncertain)?;
    Ok(receipt)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_matching_success_allows_continuation() {
        let r = json!({"run_id":"r","conversation_id":"c","activity":"idle","phase":"idle",
            "compact_context_request":{"request_id":"q","run_id":"r","conversation_id":"c","status":"submitted"}});
        let pre = json!({"hook_event_name":"PreCompact","trigger":"manual","turn_id":"t"});
        let post = json!({"hook_event_name":"PostCompact","trigger":"manual","turn_id":"t"});
        let mut state = r.clone(); observe(&mut state,&post); assert!(check_continuation(&state,"q").is_err());
        observe(&mut state,&pre); observe(&mut state,&post); assert!(check_continuation(&state,"q").is_ok());
        assert!(check_continuation(&state,"other").is_err());
        for interruption in ["Interrupt","StopFailure","SessionEnd","UserPromptSubmit"] {
            let mut state = r.clone(); observe(&mut state,&pre);
            observe(&mut state,&json!({"hook_event_name":interruption})); observe(&mut state,&post);
            assert!(check_continuation(&state,"q").is_err());
        }
        // A later turn keeps the result and ends only its continuation.
        for later in ["UserPromptSubmit","Interrupt","StopFailure","SessionEnd","PermissionRequest"] {
            let mut state = r.clone(); observe(&mut state,&pre); observe(&mut state,&post);
            let completed_at = state["compact_context_request"]["completed_at"].clone();
            observe(&mut state,&json!({"hook_event_name":later}));
            assert_eq!(state["compact_context_request"]["status"], "completed", "{later}");
            assert_eq!(state["compact_context_request"]["completed_at"], completed_at);
            assert!(check_continuation(&state,"q").is_err(), "{later}");
            let superseded = state["compact_context_request"].clone();
            observe(&mut state,&json!({"hook_event_name":"UserPromptSubmit"}));
            assert_eq!(state["compact_context_request"], superseded);
        }
        for finished in ["cancelled","failed","unchanged"] {
            let mut state = r.clone(); state["compact_context_request"]["status"] = json!(finished);
            let before = state.clone(); observe(&mut state,&json!({"hook_event_name":"UserPromptSubmit"}));
            assert_eq!(state, before);
        }
        let mut state = r.clone(); state["compact_context_request"]["status"] = json!("uncertain");
        observe(&mut state,&json!({"hook_event_name":"UserPromptSubmit"}));
        assert_eq!(state["compact_context_request"]["status"], "cancelled");
        for field in ["trigger","turn_id","agent_id"] {
            let mut state = r.clone(); observe(&mut state,&pre); let mut wrong=post.clone(); wrong[field]=json!("other");
            observe(&mut state,&wrong); assert!(check_continuation(&state,"q").is_err());
        }
    }
}
