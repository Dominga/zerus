//! A phone's signed latch statement on a child's question route
//! (zerus-sa9qn.50). A Rust-only mutation gated by its own help marker. This
//! connector only forwards the statement and signature to its own computer's
//! desktop service, which runs this connector's workspace verifier itself
//! and applies the latch at most once; the forwarder grants nothing. Peers
//! and Windows gateways do not offer it: a peer is not this workspace's
//! gateway.
use serde_json::Value;

pub const OPERATION: &str = "session_question_route_person";
pub const HELP_MARKER: &str = "question-route-person-v1";
pub const REVISION_OPERATION: &str = "session_question_route_person_status";
pub const REVISION_HELP_MARKER: &str = "question-route-person-v2";
pub const UPGRADE: &str =
    "Update the Zerus app on this phone to change this child's question routing.";
pub const UNAVAILABLE: &str = "native person question route ABI is unavailable";
pub const PEER: &str = "the person's question route is set on the phone's gateway only";
const STATEMENT: [&str; 10] = [
    "workspace_id",
    "device_id",
    "gateway_id",
    "session_id",
    "run_id",
    "conversation_id",
    "person_off",
    "request_id",
    "issued_at",
    "expires_at",
];
/// Closed refusals before changing the latch value. A stale v2 revision
/// may still durably fence that latch against legacy statements.
const REFUSALS: [&str; 7] = [
    "not_verified",
    "stale_decision",
    "stale_identity",
    "not_found",
    "capacity",
    "invalid_request",
    "upgrade_required",
];

fn exact(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn text(value: &Value, maximum: usize) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= maximum && !s.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
    })
}

pub fn validate(payload: &Value) -> Result<(), String> {
    let statement = &payload["statement"];
    let revisioned = statement.get("expected_revision").is_some();
    let shaped = exact(payload, &["statement", "signature"])
        && if revisioned {
            let mut keys = STATEMENT.to_vec();
            keys.push("expected_revision");
            exact(statement, &keys)
                && statement["expected_revision"]
                    .as_u64()
                    .is_some_and(|r| r <= 9_007_199_254_740_991)
        } else {
            exact(statement, &STATEMENT)
        };
    let fields = [
        ("workspace_id", 128),
        ("device_id", 128),
        ("gateway_id", 128),
        ("session_id", 36),
        ("request_id", 36),
        ("run_id", 128),
        ("conversation_id", 256),
    ]
    .iter()
    .all(|(key, maximum)| text(&statement[*key], *maximum));
    let times = match (
        statement["issued_at"].as_u64(),
        statement["expires_at"].as_u64(),
    ) {
        (Some(issued), Some(expires)) => issued < expires && expires - issued <= 300,
        _ => false,
    };
    if !shaped
        || !fields
        || !times
        || !statement["person_off"].is_boolean()
        || payload["signature"].as_str().is_none_or(|s| s.len() != 86)
    {
        return Err("session_question_route_person requires one exact signed statement".into());
    }
    Ok(())
}

/// The service's request for this payload.
pub fn request(payload: &Value) -> Value {
    serde_json::json!({"op": "question_route_person", "statement": payload["statement"],
        "signature": payload["signature"]})
}

/// The service's exact receipt of this statement, or its closed refusal.
pub fn result_valid(result: &Value, payload: &Value) -> bool {
    let statement = &payload["statement"];
    if result["request_id"] != statement["request_id"] || result["op"] != "question_route_child" {
        return false;
    }
    if result["accepted"] == false {
        return exact(result, &["request_id", "op", "accepted", "durable", "code"])
            && result["durable"] == false
            && result["code"]
                .as_str()
                .is_some_and(|code| REFUSALS.contains(&code));
    }
    exact(
        result,
        &[
            "request_id",
            "op",
            "accepted",
            "durable",
            "session_id",
            "person_off",
            "revision",
            "applied",
            "code",
        ],
    ) && result["accepted"] == true
        && result["durable"] == true
        && result["session_id"] == statement["session_id"]
        && result["person_off"] == statement["person_off"]
        && result["revision"].as_u64().is_some_and(|r| {
            statement
                .get("expected_revision")
                .is_none_or(|expected| expected.as_u64().and_then(|n| n.checked_add(1)) == Some(r))
        })
        && result["applied"].is_boolean()
        && result["code"] == ""
}

pub fn validate_status(payload: &Value) -> Result<(), String> {
    if !exact(
        payload,
        &["session_id", "expected_run_id", "expected_conversation_id"],
    ) || !text(&payload["session_id"], 36)
        || !text(&payload["expected_run_id"], 128)
        || !text(&payload["expected_conversation_id"], 256)
    {
        return Err(
            "session_question_route_person_status requires the exact session, run and conversation"
                .into(),
        );
    }
    Ok(())
}
pub fn status_request(payload: &Value, request_id: &str) -> Value {
    serde_json::json!({"op":"question_route_child_status", "request_id":request_id,
        "session_id":payload["session_id"], "run_id":payload["expected_run_id"], "conversation_id":payload["expected_conversation_id"]})
}
pub fn status_valid(result: &Value, payload: &Value, request_id: &str) -> bool {
    exact(
        result,
        &[
            "request_id",
            "op",
            "accepted",
            "durable",
            "session_id",
            "run_id",
            "conversation_id",
            "person_off",
            "revision",
        ],
    ) && result["request_id"] == request_id
        && result["op"] == "question_route_child_status"
        && result["accepted"] == true
        && result["durable"] == false
        && result["session_id"] == payload["session_id"]
        && result["run_id"] == payload["expected_run_id"]
        && result["conversation_id"] == payload["expected_conversation_id"]
        && result["person_off"].is_boolean()
        && result["revision"]
            .as_u64()
            .is_some_and(|r| r <= 9_007_199_254_740_991)
}
