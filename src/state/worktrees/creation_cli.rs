//! Presentation of the already-recorded result for plain UUID CLI callers.
//! The receipt call has returned; the original job may still be running.
use crate::state::Result;
use serde_json::Value;

pub(super) fn outcome(receipt: Value) -> Result<Value> {
    if receipt["status"] == "created" {
        return Ok(receipt);
    }
    let id = receipt["request_id"]
        .as_str()
        .ok_or("Original worktree request identity is unavailable; do not retry creation.")?;
    let mut message = if receipt["status"] == "failed" {
        String::new()
    } else {
        "Worktree creation outcome is unconfirmed.\n".into()
    };
    if let Some(detail) = receipt["detail"].as_str().filter(|text| !text.is_empty()) {
        message.extend(detail.chars().take(4096));
        message.push('\n');
    }
    message.push_str(&format!(
        "Check the original request: hgs worktrees status --request-id {id} --json. Do not retry creation."
    ));
    Err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn only_the_recorded_created_result_keeps_success_and_all_its_fields() {
        let receipt = json!({"status":"created","request_id":ID,"path":"/checkout",
            "effects_started":true,"creation_owner":{"id":"original"}});
        assert_eq!(outcome(receipt.clone()).unwrap(), receipt);
    }

    #[test]
    fn failure_detail_is_retained_without_claiming_no_effects() {
        for effects in [false, true] {
            let receipt = json!({"status":"failed","request_id":ID,"effects_started":effects,
                "detail":"synthetic Git error\nAny created files and branch have been kept."});
            let original = receipt.clone();
            let error = outcome(receipt.clone()).unwrap_err();
            assert!(error.starts_with(receipt["detail"].as_str().unwrap()));
            assert!(error.contains(&format!("status --request-id {ID} --json")));
            assert!(error.ends_with("Do not retry creation."));
            assert!(!error.contains("no effects"));
            assert_eq!(receipt, original);
        }
    }

    #[test]
    fn running_unknown_and_fenced_results_only_direct_the_original_lookup() {
        for status in [
            "running",
            "unconfirmed",
            "unknown",
            "fenced",
            "cancelled",
            "not_found",
        ] {
            let error = outcome(json!({"request_id":ID,"status":status,
                "detail":"retained original diagnostic","past_outcome":"unknown"}))
            .unwrap_err();
            assert!(error.starts_with("Worktree creation outcome is unconfirmed.\n"));
            assert!(error.contains("retained original diagnostic"));
            assert!(error.contains(ID));
            assert!(!error.contains("failed"));
            assert!(error.ends_with("Do not retry creation."));
        }
    }

    #[test]
    fn missing_or_large_details_cannot_hide_the_lookup_or_retry_refusal() {
        for detail in [Value::Null, json!(""), json!("x".repeat(5000))] {
            let error =
                outcome(json!({"request_id":ID,"status":"failed","detail":detail})).unwrap_err();
            assert!(error.len() < 4400);
            assert!(error.contains(ID));
            assert!(error.ends_with("Do not retry creation."));
        }
    }
}
