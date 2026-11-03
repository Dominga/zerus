//! Bounded failure diagnostics only; never receipt validation or authority.
use serde_json::Value;

pub fn readiness_difference(frame: &Value, expected: &Value) -> String {
    if frame["abi"] != "desktop-revoke-hold-v1" {
        return "abi".into();
    }
    if frame["held"] != true {
        if frame["held"] == false
            && frame.get("configuration") == expected.get("configuration")
            && frame.get("request_id") == expected.get("request_id")
        {
            let code = frame["code"]
                .as_str()
                .filter(|code| {
                    matches!(
                        *code,
                        "request_not_found"
                            | "request_conflict"
                            | "revocation_pending"
                            | "configuration_changed"
                            | "writer_authority_changed"
                            | "writer_epoch_changed"
                            | "unsupported_schema"
                            | "operation_resources_not_drained"
                            | "handoff_unavailable"
                            | "notification_host_not_drained"
                            | "invalid_request_id"
                            | "revoke_hold_refused"
                    )
                })
                .unwrap_or("unrecognized_code");
            return format!("declined:{code}");
        }
        return "held".into();
    }
    for field in [
        "request_id",
        "configuration",
        "writer_epoch",
        "accepted",
        "durable",
        "revoked",
        "checkpointed",
    ] {
        if frame["receipt"].get(field) != expected.get(field) {
            return format!("receipt.{field}");
        }
    }
    "receipt.fields".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_declines_are_not_mislabelled_as_process_identity_mismatches() {
        let expected = json!({"configuration":"fixture","request_id":"original"});
        let mut frame = json!({"abi":"desktop-revoke-hold-v1","held":false,
            "configuration":"fixture","request_id":"original","code":"writer_epoch_changed"});
        assert_eq!(
            readiness_difference(&frame, &expected),
            "declined:writer_epoch_changed"
        );
        frame["code"] = "untrusted child text".into();
        assert_eq!(
            readiness_difference(&frame, &expected),
            "declined:unrecognized_code"
        );
        frame["request_id"] = "another".into();
        assert_eq!(readiness_difference(&frame, &expected), "held");
    }

    #[test]
    fn only_static_receipt_field_names_are_reported_without_exposing_values() {
        let expected = json!({"request_id":"original","configuration":"fixture","writer_epoch":null,
            "accepted":true,"durable":true,"revoked":true,"checkpointed":true});
        for field in [
            "request_id",
            "configuration",
            "writer_epoch",
            "accepted",
            "durable",
            "revoked",
            "checkpointed",
        ] {
            let mut receipt = expected.clone();
            receipt[field] = "untrusted changed value".into();
            let frame = json!({"abi":"desktop-revoke-hold-v1","held":true,"receipt":receipt});
            assert_eq!(
                readiness_difference(&frame, &expected),
                format!("receipt.{field}")
            );
        }
        let mut frame = json!({"abi":"desktop-revoke-hold-v1","held":true,"receipt":expected});
        frame["receipt"]["untrusted extra key"] = true.into();
        assert_eq!(readiness_difference(&frame, &expected), "receipt.fields");
        frame["abi"] = "unknown".into();
        assert_eq!(readiness_difference(&frame, &expected), "abi");
    }
}
