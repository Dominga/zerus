//! Additive API-v1 opt-in outcome requests; no relay server or native process.
use serde_json::{json, Value};
use zerus_relay::{json as codec, protocol};

const ORIGINAL: &str = "22222222-2222-4222-8222-222222222222";
const READ: &str = "11111111-1111-4111-8111-111111111111";

fn original(files: bool) -> Value {
    json!({"request_id":ORIGINAL,"expected_run_id":"run","expected_conversation_id":"conversation",
        "text":"  e\u{301}\nkept original  ","attachments":if files {
            json!([{"name":"note.txt","mime":"text/plain","data_base64":"aGVsbG8="}])
        } else { json!([]) }})
}

fn binding(payload: &Value) -> Value {
    json!({"schema":2,"message_request_id":ORIGINAL,"computer_id":"fixture",
        "machine_id":"33333333-3333-4333-8333-333333333333",
        "session_id":"44444444-4444-4444-8444-444444444444","session":"codex/example/send",
        "provider":"codex","run_id":"run","conversation_id":"conversation","native_encoding":"compact",
        "native_digest":codec::digest(serde_json::to_vec(payload).unwrap())})
}

fn request(operation: &str, id: &str, payload: Value) -> Value {
    json!({"request_id":id,"computer_id":"fixture","operation":operation,
        "session":"codex/example/send","payload":payload})
}

#[test]
fn negotiated_native_outcome_send_and_read_bind_the_original_with_or_without_attachments() {
    for files in [false, true] {
        let original = original(files);
        let binding = binding(&original);
        let digest = codec::digest(codec::canonical(&binding).unwrap());
        let send = request(
            "send_outcome",
            ORIGINAL,
            json!({"schema":2,"request_id":ORIGINAL,
            "binding":binding,"binding_sha256":digest,"payload":original}),
        );
        assert!(
            protocol::request(&send).is_ok(),
            "opt-in ordinary send was not admitted: {send}"
        );
        let read = request(
            "send_outcome_receipt",
            READ,
            json!({"schema":2,"request_id":READ,
            "binding":send["payload"]["binding"],"binding_sha256":send["payload"]["binding_sha256"]}),
        );
        assert!(
            protocol::request(&read).is_ok(),
            "read-only outcome recovery was not admitted: {read}"
        );
        assert_eq!(read["payload"]["binding"]["message_request_id"], ORIGINAL);
        assert_ne!(read["request_id"], ORIGINAL);
    }
}

#[test]
fn shared_outcomes_require_the_complete_original_binding_and_closed_native_proof() {
    use zerus_relay::send_outcome as contract;
    let vectors: Value =
        serde_json::from_str(include_str!("../../vectors/send_outcome/outcomes.json")).unwrap();
    for v in vectors["vectors"].as_array().unwrap() {
        for op in ["send", "read"] {
            assert!(protocol::request(&v[op]).is_ok());
        }
        assert_eq!(
            contract::project(&v["send"]["payload"], &v["negative"], true).unwrap(),
            v["not_sent"]
        );
        for (native, result) in [
            ("read_negative", "read_not_sent"),
            ("read_positive", "read_submitted"),
            ("read_unknown", "unknown"),
        ] {
            assert_eq!(
                contract::project(&v["read"]["payload"], &v[native], false).unwrap(),
                v[result]
            );
        }
        for field in [
            "request_id",
            "name",
            "machine_id",
            "session_id",
            "provider",
            "run_id",
            "conversation_id",
            "native_digest",
            "proof",
            "schema",
        ] {
            let mut changed = v["negative"].clone();
            changed[field] = json!("changed");
            assert!(
                contract::project(&v["send"]["payload"], &changed, true).is_err(),
                "{field}"
            );
        }
        let mut retryable = v["negative"].clone();
        retryable["status"] = json!("refused_before_input");
        assert!(contract::project(&v["send"]["payload"], &retryable, true).is_err());
        let mut changed = v["send"].clone();
        changed["payload"]["payload"]["text"] = json!("changed bytes");
        assert!(protocol::request(&changed).is_err());
    }
}
