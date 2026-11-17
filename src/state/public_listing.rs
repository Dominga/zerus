//! Omit raw recovery bookkeeping and selected-only model choices from lists.
//! Canonical data, internal summaries, seeds and selected inspection are unchanged.
use serde_json::Value;

pub(super) const SELECTED_ONLY_FIELDS: [&str; 6] = [
    "recovery_user_text",
    "recovery_user_at",
    "recovery_success_at",
    "recovery_cancel_at",
    "keep_warm_prompt_at",
    "model_options",
];

pub(super) fn project(rows: &mut [Value]) {
    for row in rows.iter_mut().filter_map(Value::as_object_mut) {
        for key in SELECTED_ONLY_FIELDS {
            row.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn projection_removes_only_the_six_top_level_properties() {
        let source = json!({
            "name":"claude/example/session", "run_id":"run", "conversation_id":"conversation",
            "phase":"working", "activity":"busy", "reply_id":"reply",
            "pending_questions":[{"id":"question"}], "optional_questions_complete":true,
            "subagents":{"count":1}, "allowed_actions":["inspect"],
            "recovery":{"id":"job","state":"waiting","recovery_user_text":"nested unchanged"},
            "recovery_capability":true, "recovery_error_class":"quota",
            "model":"fixture-a", "effort":"high", "effort_options":["low","high"],
            "model_options":[{"id":"fixture-a","effort_options":["low","high"]}],
            "catalog":{"model_options":[{"id":"nested-kept"}]},
            "recovery_user_text":"quoted \"text\"\nКириллица", "recovery_user_at":1,
            "recovery_success_at":2, "recovery_cancel_at":3, "keep_warm_prompt_at":4,
            "recovery_future_field":"not a prefix filter"
        });
        let mut expected = source.clone();
        for key in SELECTED_ONLY_FIELDS {
            expected.as_object_mut().unwrap().remove(key);
        }
        let mut rows = vec![source.clone(), Value::Null];
        project(&mut rows);
        assert_eq!(rows, vec![expected, Value::Null]);
        assert_eq!(source["recovery_user_text"], "quoted \"text\"\nКириллица");
        assert_eq!(rows[0]["recovery"], source["recovery"]);
        let once = rows.clone();
        project(&mut rows);
        assert_eq!(rows, once);
    }
}
