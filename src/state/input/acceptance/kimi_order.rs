//! Capture native ordering BEFORE pending changes. Kimi's submit hook precedes
//! prompt construction, so its late prompt/typed callback can belong to A, not B.
use super::*;

fn scope(value: &Value, record: &Value) -> bool {
    value["run_id"] == record["run_id"] && value["conversation_id"] == record["conversation_id"]
}

fn bootstrap(record: &Value, before: f64) -> bool {
    let context = &record["input_pending_native_context"];
    let observation = &record["hook_observation"];
    !string(record, "conversation_id").is_empty()
        && string(record, "expected_id").is_empty()
        && context.as_object().is_some_and(|fields| fields.len() == 7)
        && context["version"] == 1
        && context["conversation_id"] == ""
        && context["run_id"] == record["run_id"]
        && context["request_id"] == record["input_pending_request"]
        && context["pending_at"] == record["input_pending_at"]
        && context["complete"] == true
        && context.get("after").is_some_and(Value::is_null)
        && scope(observation, record)
        && observation["events"]["SessionStart"]["last_at"]
            .as_f64()
            .is_some_and(|at| {
                at.is_finite()
                    && at >= record["input_pending_at"].as_f64().unwrap_or(f64::INFINITY)
                    && at < before
            })
}

// Some(None): no older acceptance to confuse; Some(Some(id)): positive typed
// floor; None: incomplete ordering. No native row is accepted by this snapshot.
fn snapshot(record: &Value, before: f64, first: bool) -> Option<Option<u64>> {
    let typed = &record["input_native_hook_context"];
    if typed["version"] == 1
        && typed.as_object().is_some_and(|fields| fields.len() == 7)
        && scope(typed, record)
        && typed["request_id"] == record["input_pending_request"]
        && typed["pending_at"] == record["input_pending_at"]
        && typed["seen_at"] == record["input_seen_at"]
        && typed["seen_at"]
            .as_f64()
            .is_some_and(|at| at.is_finite() && at > 0.0 && at < before)
    {
        if let Some(id) = typed["turn_id"]
            .as_u64()
            .filter(|id| *id <= i64::MAX as u64)
        {
            return Some(Some(id));
        }
    }
    let accepted = &record["input_acceptance"];
    if accepted["source"] == "native_user_input"
        && scope(accepted, record)
        && accepted["request_id"] == record["input_pending_request"]
        && accepted["pending_at"] == record["input_pending_at"]
        && accepted["at"] == record["input_seen_at"]
        && accepted["at"]
            .as_f64()
            .is_some_and(|at| at.is_finite() && at > 0.0 && at < before)
    {
        if let Some(id) = accepted["native_turn_id"]
            .as_u64()
            .filter(|id| *id <= i64::MAX as u64)
        {
            return Some(Some(id));
        }
    }
    let submitted = &record["input_native_submit_binding"];
    if record.get("input_pending_native_context").is_some()
        && submitted["exact"] == true
        && scope(submitted, record)
        && submitted["request_id"] == record["input_pending_request"]
        && submitted["pending_at"] == record["input_pending_at"]
        && submitted["seen_at"] == record["input_seen_at"]
    {
        let published = (|| {
            let boot = pending(record).is_none() && bootstrap(record, before);
            let after = if boot { None } else { pending(record)? };
            let receipt = bound_receipt(record, boot)?;
            if uncertainty::load(record).blocks(string(&receipt, "submitted_text")) {
                return None;
            }
            let source = kimi(record)?;
            let (_, Some(id)) = consumed(
                &source.rows,
                "kimi",
                record["input_pending_at"].as_f64()?,
                string(&receipt, "submitted_text"),
                after,
            )?
            else {
                return None;
            };
            // This only refines the floor of an already hook-accepted first
            // input. Its original empty-CID receipt and seen state stay intact.
            // The actual registered new conversation must prove native turn 0.
            (source.current() && (!boot || id == 0)).then_some(id)
        })();
        if let Some(id) = published {
            return Some(Some(id));
        }
    }
    // A first send with no previous native acceptance can use its positive
    // native prompt. Any legacy/nonzero acceptance needs typed ordering instead.
    let prior_hook = [
        "UserPromptSubmit",
        "UserPromptQueued",
        "TurnStarted",
        "TaskStarted",
    ]
    .iter()
    .any(|kind| record["hook_observation"]["events"][kind].is_object());
    if first
        && record
            .get("input_seen_at")
            .is_none_or(|value| value.as_f64() == Some(0.0))
        && !prior_hook
        && record.get("input_native_hook_context").is_none()
        && record.get("input_acceptance").is_none()
        && record.get("input_native_submit_binding").is_none()
    {
        return Some(None);
    }
    None
}

fn matching_prompt(record: &Value, event: &Value) -> bool {
    let Some(prompt) = hook_text("kimi", event) else {
        return false;
    };
    record["input_pending_text_sha256"]
        .as_str()
        .is_some_and(|sha| sha == format!("{:x}", Sha256::digest(prompt.as_bytes())))
}

/// Retain typed ordering only for the already complete current request, after
/// reducer/keep-warm classification and before its durable acceptance write.
pub(in crate::state) fn record_hook(record: &mut Value, event: &Value) {
    if string(record, "agent") != "kimi" {
        return;
    }
    if let Some(fields) = record.as_object_mut() {
        fields.remove("input_native_hook_context");
        fields.remove("input_native_submit_binding");
    }
    let kind = string(event, "hook_event_name");
    if matches!(kind, "UserPromptSubmit" | "UserPromptQueued") {
        record["input_native_submit_binding"] = json!({"run_id":record["run_id"],"conversation_id":record["conversation_id"],
            "request_id":record["input_pending_request"],"pending_at":record["input_pending_at"],"seen_at":record["input_seen_at"],
            "exact":matching_prompt(record,event)});
        return;
    }
    if kind != "TurnStarted" || event["origin_kind"] != "user" || !matching_prompt(record, event) {
        return;
    }
    let Some(after) = pending(record) else {
        return;
    };
    let Some(id) = event["turn_id"]
        .as_u64()
        .filter(|id| *id <= i64::MAX as u64)
    else {
        return;
    };
    if after.is_some_and(|floor| id <= floor) {
        return;
    }
    record["input_native_hook_context"] = json!({"version":1,"run_id":record["run_id"],"conversation_id":record["conversation_id"],
        "turn_id":id,"seen_at":record["input_seen_at"],"request_id":record["input_pending_request"],"pending_at":record["input_pending_at"]});
}

pub(in crate::state) fn bind(record: &mut Value, receipt: &Value) {
    if string(record, "agent") != "kimi" {
        return;
    }
    let before = receipt["submitted_at"]
        .as_f64()
        .unwrap_or(f64::NEG_INFINITY);
    let first = record.get("input_pending_at").is_none();
    let mut context = snapshot(record, before, first);
    // Unknown settlement does not cancel A. Its fingerprint ledger fences A;
    // only previously complete ordering can pass forward, never an unknown one.
    if context.is_none() && terminal::acknowledgment::matches(record) {
        context = pending(record);
    }
    record["input_pending_native_context"] = json!({"version":1,"request_id":receipt["request_id"],
        "run_id":record["run_id"],"conversation_id":receipt["conversation_id"],"pending_at":receipt["submitted_at"],
        "complete":context.is_some(),"after":context.flatten()});
}

pub(super) fn pending(record: &Value) -> Option<Option<u64>> {
    if let Some(value) = record.get("input_pending_native_context") {
        if !value.as_object().is_some_and(|fields| fields.len() == 7)
            || value.get("after").is_none()
            || value["version"] != 1
            || !scope(value, record)
            || value["request_id"] != record["input_pending_request"]
            || value["pending_at"] != record["input_pending_at"]
            || value["complete"] != true
        {
            return None;
        }
        return if value["after"].is_null() {
            Some(None)
        } else {
            value["after"]
                .as_u64()
                .filter(|id| *id <= i64::MAX as u64)
                .map(Some)
        };
    }
    // Legacy evidence may establish a typed floor before this exact pending
    // input. Unknown prior ordering is not repaired from a newly arriving hook.
    let before = record["input_pending_at"].as_f64()?;
    snapshot(
        record,
        before,
        record["input_seen_at"].as_f64() == Some(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn initial() -> Value {
        json!({"agent":"kimi","run_id":"run","conversation_id":"cid"})
    }
    fn receipt(id: &str, at: f64) -> Value {
        json!({"request_id":id,"submitted_at":at})
    }
    fn mark(record: &mut Value, id: &str, at: f64) {
        record["input_pending_request"] = json!(id);
        record["input_pending_at"] = json!(at);
    }
    #[test]
    fn first_and_positive_floor_are_bound_before_pending_and_cannot_be_filled_late() {
        let mut record = initial();
        bind(&mut record, &receipt("a", 10.0));
        mark(&mut record, "a", 10.0);
        assert_eq!(pending(&record), Some(None));
        record["input_seen_at"] = json!(11.0);
        record_hook(&mut record, &json!({"hook_event_name":"UserPromptSubmit"}));
        bind(&mut record, &receipt("b", 20.0));
        mark(&mut record, "b", 20.0);
        assert_eq!(pending(&record), None);
        record["input_seen_at"] = json!(21.0);
        record["input_pending_text_sha256"] = json!(format!("{:x}", Sha256::digest(b"text")));
        record_hook(
            &mut record,
            &json!({"hook_event_name":"TurnStarted","origin_kind":"user","turn_id":0,"prompt":"text"}),
        );
        assert_eq!(
            pending(&record),
            None,
            "late hook cannot fill the pre-send snapshot"
        );
        bind(&mut record, &receipt("c", 30.0));
        mark(&mut record, "c", 30.0);
        assert_eq!(
            pending(&record),
            None,
            "late callback cannot repair future ordering either"
        );
        record = initial();
        bind(&mut record, &receipt("a", 10.0));
        mark(&mut record, "a", 10.0);
        record["input_seen_at"] = json!(11.0);
        record["input_pending_text_sha256"] = json!(format!("{:x}", Sha256::digest(b"text")));
        record_hook(
            &mut record,
            &json!({"hook_event_name":"TurnStarted","origin_kind":"user","turn_id":0,"prompt":"text"}),
        );
        bind(&mut record, &receipt("c", 30.0));
        mark(&mut record, "c", 30.0);
        assert_eq!(pending(&record), Some(Some(0)));
        for (key, value) in [
            ("request_id", json!("other")),
            ("run_id", json!("other")),
            ("conversation_id", json!("other")),
            ("pending_at", json!(31.0)),
            ("after", json!(false)),
            ("complete", json!(false)),
        ] {
            let mut changed = record.clone();
            changed["input_pending_native_context"][key] = value;
            assert_eq!(pending(&changed), None, "{key}");
        }
        let mut changed = record.clone();
        changed["input_pending_native_context"]
            .as_object_mut()
            .unwrap()
            .remove("after");
        assert_eq!(pending(&changed), None);
    }
    #[test]
    fn bootstrap_needs_the_registered_native_conversation_and_exact_first_context() {
        let mut record = initial();
        record["conversation_id"] = json!("native-cid");
        mark(&mut record, "a", 10.0);
        record["input_pending_native_context"] = json!({"version":1,"run_id":"run","conversation_id":"", "request_id":"a","pending_at":10.0,"complete":true,"after":null});
        record["hook_observation"] = json!({"run_id":"run","conversation_id":"native-cid","events":{"SessionStart":{"last_at":11.0}}});
        assert!(bootstrap(&record, 20.0));
        for (key, value) in [
            ("request_id", json!("other")),
            ("run_id", json!("other")),
            ("pending_at", json!(9.0)),
            ("conversation_id", json!("borrowed")),
            ("after", json!(0)),
            ("complete", json!(false)),
        ] {
            let mut changed = record.clone();
            changed["input_pending_native_context"][key] = value;
            assert!(!bootstrap(&changed, 20.0), "{key}");
        }
        assert!(!bootstrap(&record, 11.0));
        record["hook_observation"]["conversation_id"] = json!("other");
        assert!(!bootstrap(&record, 20.0));
    }

    #[test]
    fn legacy_nonzero_acceptance_and_malformed_context_do_not_prove_ordering() {
        let mut record = initial();
        mark(&mut record, "a", 10.0);
        record["input_seen_at"] = json!(5.0);
        assert_eq!(pending(&record), None);
        record["input_seen_at"] = json!(0.0);
        assert_eq!(pending(&record), Some(None));
        record["input_pending_native_context"] = Value::Null;
        assert_eq!(pending(&record), None);
    }
}
