//! Negotiated ordinary native sends and original-only outcome reads. Binding
//! metadata is bounded; attachment bytes stay in the ordinary send payload.
use crate::{
    error::{Error, Result},
    json, protocol,
};
use serde_json::{json, Value};

pub const SEND: &str = "send_outcome";
pub const READ: &str = "send_outcome_receipt";
pub const OPERATIONS: &[&str] = &[SEND, READ];
pub const HELP_MARKER: &str = "native-send-outcome-v2";
pub const BINDING_BYTES: usize = 4096;
pub const NATIVE_NEGATIVE_BYTES: usize = 8192;

pub fn binding_digest(binding: &Value) -> Result<String> {
    Ok(json::digest(json::canonical(binding)?))
}

pub fn native_digest(payload: &Value, encoding: &str) -> Result<String> {
    let bytes = match encoding {
        "compact" => serde_json::to_vec(payload).map_err(|_| Error::BAD)?,
        "python_default" => {
            let mut bytes = Vec::new();
            json::python_default_encode(payload, false, &mut bytes)?;
            bytes
        }
        _ => return Err(Error::BAD),
    };
    Ok(json::digest(bytes))
}

pub fn validate_binding(binding: &Value) -> Result<()> {
    protocol::fields(
        binding,
        &[
            "schema",
            "message_request_id",
            "computer_id",
            "machine_id",
            "session_id",
            "session",
            "provider",
            "run_id",
            "conversation_id",
            "native_encoding",
            "native_digest",
        ],
        &[],
    )?;
    if binding["schema"].as_u64() != Some(2) || json::size(binding, BINDING_BYTES)? > BINDING_BYTES
    {
        return Err(Error::BAD);
    }
    for field in ["message_request_id", "machine_id", "session_id"] {
        protocol::uuid(&binding[field], true)?;
    }
    for (field, maximum, empty) in [
        ("computer_id", 36, false),
        ("session", 512, false),
        ("run_id", 128, false),
        ("conversation_id", 256, true),
    ] {
        let value = protocol::text(&binding[field], maximum, empty)?;
        if value.len() > maximum {
            return Err(Error::BAD);
        }
    }
    if !matches!(
        binding["provider"].as_str(),
        Some("claude" | "codex" | "kimi")
    ) || !matches!(
        binding["native_encoding"].as_str(),
        Some("compact" | "python_default")
    ) {
        return Err(Error::BAD);
    }
    protocol::hex(&binding["native_digest"])?;
    Ok(())
}

pub fn validate(
    operation: &str,
    payload: &Value,
    id: &str,
    computer: &Value,
    session: &str,
) -> Result<()> {
    let send = operation == SEND;
    if !send && operation != READ {
        return Err(Error::BAD);
    }
    let required: &[&str] = if send {
        &[
            "schema",
            "request_id",
            "binding",
            "binding_sha256",
            "payload",
        ]
    } else {
        &["schema", "request_id", "binding", "binding_sha256"]
    };
    protocol::fields(payload, required, &[])?;
    protocol::uuid(&payload["request_id"], true)?;
    validate_binding(&payload["binding"])?;
    let binding = &payload["binding"];
    if payload["schema"].as_u64() != Some(2)
        || payload["request_id"] != id
        || &binding["computer_id"] != computer
        || binding["session"] != session
        || payload["binding_sha256"].as_str() != Some(binding_digest(binding)?.as_str())
    {
        return Err(Error::BAD);
    }
    if send {
        let original = &payload["payload"];
        if binding["message_request_id"] != id
            || original["request_id"] != id
            || original.get("agent_id").is_some()
            || original["expected_run_id"] != binding["run_id"]
            || original["expected_conversation_id"] != binding["conversation_id"]
        {
            return Err(Error::BAD);
        }
        protocol::request(
            &json!({"request_id":id,"computer_id":computer,"operation":"send",
            "session":session,"payload":original}),
        )?;
        if binding["native_digest"].as_str()
            != Some(
                native_digest(
                    original,
                    binding["native_encoding"].as_str().ok_or(Error::BAD)?,
                )?
                .as_str(),
            )
        {
            return Err(Error::BAD);
        }
    } else if binding["message_request_id"] == id {
        return Err(Error::BAD);
    }
    Ok(())
}

/// A closed native negative may cross a nonzero exit only in the negotiated
/// call. This is a candidate; project() must still compare the frozen binding.
pub fn native_negative(answer: &Value, read: bool) -> Result<()> {
    let fields = [
        "schema",
        "status",
        "proof",
        "request_id",
        "name",
        "machine_id",
        "session_id",
        "provider",
        "run_id",
        "conversation_id",
        "native_digest",
        "reason",
    ];
    protocol::fields(
        answer,
        &fields,
        if read {
            &["message_request_id", "agent_id"]
        } else {
            &[]
        },
    )?;
    if answer["schema"].as_u64() != Some(2)
        || answer["status"] != "not_offered"
        || answer["proof"] != "native_input_lock"
        || json::size(answer, NATIVE_NEGATIVE_BYTES)? > NATIVE_NEGATIVE_BYTES
    {
        return Err(Error::BAD);
    }
    for field in ["request_id", "machine_id", "session_id"] {
        protocol::uuid(&answer[field], true)?;
    }
    protocol::text(&answer["reason"], 4096, false)?;
    protocol::hex(&answer["native_digest"])?;
    Ok(())
}

/// The sole phone-visible outcome comes from an original native receipt or
/// the closed NotOffered witness, never a command's exit code or diagnostics.
pub fn project(payload: &Value, answer: &Value, initial_negative: bool) -> Result<Value> {
    let binding = &payload["binding"];
    validate_binding(binding)?;
    if answer["schema"].as_u64() != Some(2)
        || answer["request_id"] != payload["request_id"]
        || answer["name"] != binding["session"]
    {
        return Err(Error::BAD);
    }
    for field in [
        "machine_id",
        "session_id",
        "provider",
        "run_id",
        "conversation_id",
        "native_digest",
    ] {
        if answer.get(field) != binding.get(field) {
            return Err(Error::BAD);
        }
    }
    if initial_negative {
        if answer["request_id"] != binding["message_request_id"] {
            return Err(Error::BAD);
        }
    } else if answer["message_request_id"] != binding["message_request_id"]
        || answer["agent_id"] != ""
    {
        return Err(Error::BAD);
    }
    let mut result = json!({"schema":2,"request_id":payload["request_id"],"binding":binding,
        "binding_sha256":binding_digest(binding)?,"status":answer["status"]});
    match answer["status"].as_str() {
        Some("not_offered") => {
            native_negative(answer, !initial_negative)?;
            result["proof"] = json!("native_input_lock");
            result["reason"] = answer["reason"].clone();
        }
        Some("submitted")
            if !initial_negative
                && answer["proof"] == "original_receipt"
                && answer["submitted_at"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v >= 0.0)
                && answer["queued"].is_boolean() =>
        {
            result["proof"] = json!("original_receipt");
            result["submitted_at"] = answer["submitted_at"].clone();
            result["queued"] = answer["queued"].clone();
        }
        Some("unknown") if !initial_negative => {}
        _ => return Err(Error::BAD),
    }
    Ok(result)
}
