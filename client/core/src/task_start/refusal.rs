//! Pure binding for the local owning-CLI Start reply. These fields are not
//! caller, input, task, floor or attempt authority. Core only compares them
//! with its OWN retained outbox plan; it never adopts a reply's plan/digest.
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) const VISIBLE: &str =
    "Refused: acknowledgement mode changed. Start again to prepare a new request.";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    schema: u32,
    request_id: String,
    plan_digest: String,
    reason: Reason,
}
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum Reason {
    AcknowledgementModeChanged,
}
impl TryFrom<String> for Reason {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "acknowledgement_mode_changed" => Ok(Self::AcknowledgementModeChanged),
            _ => Err("Unknown Start refusal reason"),
        }
    }
}

pub(super) fn matches(value: &Value, id: &str, retained: &Value) -> bool {
    let Ok(proof) = serde_json::from_value::<Proof>(value.clone()) else {
        return false;
    };
    let Reason::AcknowledgementModeChanged = proof.reason;
    proof.schema == 1
        && proof.request_id == id
        && retained["schema"] == 1
        && retained["id"] == id
        && retained["request"]["request_id"] == id
        && matches!(
            retained["acknowledgement_mode"].as_str(),
            Some("managed" | "legacy")
        )
        && plan_digest(retained).is_ok_and(|own| own == proof.plan_digest)
}

/// Shared with the CLI: sorted compact UTF-8, no normalization or floats.
/// Every plan field is encoded; the domain fixes this exact schema/encoding.
pub fn plan_digest(plan: &Value) -> Result<String, String> {
    let mut bytes = b"zerus-task-start-refusal-v1\0".to_vec();
    encode(plan, &mut bytes)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn encode(value: &Value, bytes: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => bytes.extend_from_slice(b"null"),
        Value::Bool(true) => bytes.extend_from_slice(b"true"),
        Value::Bool(false) => bytes.extend_from_slice(b"false"),
        Value::Number(n) if n.is_i64() || n.is_u64() => {
            bytes.extend_from_slice(n.to_string().as_bytes())
        }
        Value::Number(_) => return Err("Start refusal canonical encoding refuses floats".into()),
        Value::String(s) => bytes.extend(serde_json::to_vec(s).map_err(|e| e.to_string())?),
        Value::Array(a) => {
            bytes.push(b'[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                encode(v, bytes)?;
            }
            bytes.push(b']');
        }
        Value::Object(o) => {
            bytes.push(b'{');
            let mut fields = o.iter().collect::<Vec<_>>();
            fields.sort_unstable_by(|a, b| a.0.cmp(b.0));
            for (i, (key, v)) in fields.into_iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                bytes.extend(serde_json::to_vec(key).map_err(|e| e.to_string())?);
                bytes.push(b':');
                encode(v, bytes)?;
            }
            bytes.push(b'}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn canonical_utf8_integer_and_domain_are_exact_and_floats_refuse() {
        let a: Value = serde_json::from_str(
            r#"{"z":[1,18446744073709551615,null],"é":"\n猫","a":{"x":true,"b":-2}}"#,
        )
        .unwrap();
        let mut bytes = Vec::new();
        encode(&a, &mut bytes).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            "{\"a\":{\"b\":-2,\"x\":true},\"z\":[1,18446744073709551615,null],\"é\":\"\\n猫\"}"
        );
        let b = json!({"a":{"b":-2,"x":true},"é":"\n猫","z":[1,u64::MAX,null]});
        assert_eq!(plan_digest(&a).unwrap(), plan_digest(&b).unwrap());
        assert_ne!(
            plan_digest(&a).unwrap(),
            format!("{:x}", Sha256::digest(&bytes))
        );
        let mut tagged = b"zerus-task-start-refusal-v1\0".to_vec();
        tagged.extend(bytes);
        assert_eq!(
            plan_digest(&a).unwrap(),
            format!("{:x}", Sha256::digest(tagged))
        );
        for value in [json!(1.0), json!([1.5]), json!({"nested":{"x":0.0}})] {
            assert!(plan_digest(&value).is_err());
        }
    }
}
