//! Versioned ordinary-send proof. Both writes and reads keep the original
//! identity; only native's closed negative can settle a message as not sent.
use crate::{
    execute::Failure,
    native::{Call, Native},
    runtime::Connector,
};
use serde_json::{json, Value};
use zerus_relay::send_outcome as contract;

pub const MISMATCH: &str = "native send outcome identity did not match; delivery remains unknown";
pub const UNAVAILABLE: &str = "native terminal send outcome ABI is unavailable";

/// Recognize only the exact opt-in argv, including a fixed peer envelope.
/// No ordinary failing native command gains a JSON success path.
pub fn negotiated(call: &Call) -> bool {
    fn argv(args: &[String]) -> bool {
        let args = match args {
            [guard, _, tail @ ..] if guard == "--expect-machine" => tail,
            _ => args,
        };
        matches!(args, [command, _, flag, json] if command == "send" && flag == "--delivery-outcome-v2" && json == "--json")
            || matches!(args, [command, _, selector, _, flag, json] if command == "send" && selector == "--session-id" && flag == "--delivery-outcome-v2" && json == "--json")
    }
    let args = match call.argv.as_slice() {
        [guard, _, tail @ ..] if guard == "--expect-machine" => tail,
        other => other,
    };
    if argv(args) {
        return true;
    }
    if matches!(args, [swarm, helper, json] if swarm == "swarm" && matches!(helper.as_str(), "mobile-peer" | "__mobile-peer-local") && json == "--json")
    {
        return call
            .payload
            .as_ref()
            .and_then(|v| v["argv"].as_array())
            .and_then(|v| {
                v.iter()
                    .map(|v| v.as_str().map(String::from))
                    .collect::<Option<Vec<_>>>()
            })
            .is_some_and(|args| argv(&args));
    }
    false
}

pub fn call(payload: &Value) -> Result<Call, Failure> {
    let binding = &payload["binding"];
    let query = json!({"schema":2,"request_id":payload["request_id"],"message_request_id":binding["message_request_id"],
        "expected_machine_id":binding["machine_id"],"expected_session_id":binding["session_id"],
        "expected_run_id":binding["run_id"],"expected_conversation_id":binding["conversation_id"],
        "provider":binding["provider"],"agent_id":"","native_digest":binding["native_digest"]});
    let mut call = Call::new([
        "__state",
        "delivery",
        "receipt",
        binding["session"].as_str().ok_or(Failure::Generic)?,
        "--json",
    ])
    .payload(query)
    .timeout(5.0);
    call.max_output_bytes = Some(contract::NATIVE_NEGATIVE_BYTES);
    Ok(call)
}

pub fn project(payload: &Value, answer: &Value, initial_negative: bool) -> Result<Value, Failure> {
    contract::project(payload, answer, initial_negative).map_err(|_| Failure::text(MISMATCH))
}

impl<N: Native + 'static> Connector<N> {
    pub(crate) async fn send_outcome_receipt(&self, payload: &Value) -> Result<Value, Failure> {
        // This original-only read never resolves a current label or repairs a
        // listing. The native receipt binds the historical machine/SID itself.
        let answer = self.call(call(payload)?).await?;
        project(payload, &answer, false)
    }
}

/// Only the bounded, flat native proof frame uses this closed decoder. Serde's
/// struct visitor rejects duplicate (including escaped-equivalent) field names;
/// the legacy native codec intentionally retains Python's last-member behavior.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct NativeProofFrame {
    schema: Value,
    request_id: Value,
    name: Value,
    machine_id: Value,
    session_id: Value,
    provider: Value,
    run_id: Value,
    conversation_id: Value,
    native_digest: Value,
    status: Value,
    proof: Option<Value>,
    reason: Option<Value>,
    message_request_id: Option<Value>,
    agent_id: Option<Value>,
    submitted_at: Option<Value>,
    queued: Option<Value>,
}

pub fn closed_frame(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice::<NativeProofFrame>(bytes).is_ok()
}

pub fn receipt_read(call: &Call) -> bool {
    let args = match call.argv.as_slice() {
        [guard, _, tail @ ..] if guard == "--expect-machine" => tail,
        other => other,
    };
    let (argv, payload) = if matches!(args, [swarm, helper, json] if swarm == "swarm" && matches!(helper.as_str(), "mobile-peer" | "__mobile-peer-local") && json == "--json")
    {
        let Some(payload) = &call.payload else {
            return false;
        };
        let Some(argv) = payload["argv"].as_array().and_then(|args| {
            args.iter()
                .map(|s| s.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
        }) else {
            return false;
        };
        (argv, &payload["payload"])
    } else {
        (
            call.argv.clone(),
            call.payload.as_ref().unwrap_or(&Value::Null),
        )
    };
    let argv = match argv.as_slice() {
        [guard, _, tail @ ..] if guard == "--expect-machine" => tail,
        other => other,
    };
    matches!(argv, [state, delivery, receipt, _, json] if state == "__state" && delivery == "delivery" && receipt == "receipt" && json == "--json")
        && payload["schema"] == 2
}
