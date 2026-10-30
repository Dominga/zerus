//! The person's explicit discussion mode, bound to the clicked task epoch and
//! pinned dispatch. Recovery always uses the existing outbox's original mode.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub mode: String,
    pub stage_at: i64,
    pub dispatch_id: String,
}

pub fn checked<'a>(detail: &Value, delivery: &'a Delivery) -> Result<&'a str, String> {
    let stage = detail["stage"].as_str().unwrap_or("");
    let valid = match delivery.mode.as_str() {
        "note" => matches!(stage, "todo" | "backlog" | "active" | "review" | "done"),
        "queue" => matches!(stage, "todo" | "backlog"),
        "send" => stage == "active",
        "reopen" => matches!(stage, "review" | "done"),
        _ => false,
    };
    if !valid
        || detail["writable"] != true
        || delivery.stage_at <= 0
        || detail["stage_at"].as_i64() != Some(delivery.stage_at)
        || detail["dispatch_id"].as_str().unwrap_or("") != delivery.dispatch_id
        || delivery.dispatch_id.len() > 512
        || delivery.dispatch_id.chars().any(char::is_control)
    {
        return Err("The task changed. Your comment draft was kept. Refresh the task.".into());
    }
    Ok(&delivery.mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn each_stage_has_only_its_explicit_delivery_mode_and_note() {
        for (stage, mode) in [
            ("todo", "queue"),
            ("backlog", "queue"),
            ("active", "send"),
            ("review", "reopen"),
            ("done", "reopen"),
        ] {
            let detail =
                json!({"stage":stage,"stage_at":123,"dispatch_id":"original","writable":true});
            for choice in ["queue", "send", "reopen", "note"] {
                let delivery = Delivery {
                    mode: choice.into(),
                    stage_at: 123,
                    dispatch_id: "original".into(),
                };
                assert_eq!(
                    checked(&detail, &delivery).is_ok(),
                    choice == mode || choice == "note"
                );
            }
        }
    }
    #[test]
    fn a_changed_epoch_dispatch_or_future_document_cannot_deliver() {
        let delivery = Delivery {
            mode: "send".into(),
            stage_at: 123,
            dispatch_id: "original".into(),
        };
        let detail =
            json!({"stage":"active","stage_at":123,"dispatch_id":"original","writable":true});
        for (key, value) in [
            ("stage_at", json!(124)),
            ("dispatch_id", json!("other")),
            ("writable", json!(false)),
            ("stage", json!("review")),
        ] {
            let mut changed = detail.clone();
            changed[key] = value;
            assert!(checked(&changed, &delivery).is_err());
        }
    }
}
