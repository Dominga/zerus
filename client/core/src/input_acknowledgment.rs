//! An explicit person's settlement of one unknown input, never native acceptance.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PendingInput {
    pub original_request_id: String,
    pub pending_at: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgment {
    pub request_id: String,
    pub name: String,
    pub expected_run_id: String,
    pub expected_conversation_id: String,
    pub original_request_id: String,
    pub pending_at: f64,
    pub terminal_binding_id: String,
    pub expires_at: f64,
}

fn identity(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
pub fn canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
impl Acknowledgment {
    pub fn valid(&self) -> bool {
        canonical(&self.request_id)
            && canonical(&self.original_request_id)
            && identity(&self.name, 4096)
            && identity(&self.expected_run_id, 128)
            && identity(&self.expected_conversation_id, 256)
            && self.pending_at.is_finite()
            && self.pending_at > 0.0
            && self.expires_at.is_finite()
            && self.expires_at > 0.0
            && self.terminal_binding_id.len() == 64
            && self
                .terminal_binding_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_review_is_closed_and_never_invents_a_conversation() {
        let value = serde_json::json!({"request_id":"11111111-1111-4111-8111-111111111111",
            "name":"claude/example/review","expected_run_id":"run","expected_conversation_id":"conversation",
            "original_request_id":"22222222-2222-4222-8222-222222222222","pending_at":10.0,
            "terminal_binding_id":"a".repeat(64),"expires_at":15.0});
        assert!(serde_json::from_value::<Acknowledgment>(value.clone())
            .unwrap()
            .valid());
        for (field, wrong) in [
            ("expected_conversation_id", serde_json::json!("")),
            ("original_request_id", serde_json::json!("../other")),
            ("pending_at", serde_json::json!(0)),
            ("terminal_binding_id", serde_json::json!("short")),
        ] {
            let mut changed = value.clone();
            changed[field] = wrong;
            assert!(!serde_json::from_value::<Acknowledgment>(changed)
                .unwrap()
                .valid());
        }
        for field in ["confirmed", "role", "agent", "authority"] {
            let mut changed = value.clone();
            changed[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<Acknowledgment>(changed).is_err());
        }
    }
}
