//! Version-1 canonical admission encoding: sorted object keys, compact JSON,
//! UTF-8 strings without Unicode normalization, integer numbers only. Typed
//! closed-schema validation precedes this encoder. No float is rounded/coerced.
use serde_json::Value;
use sha2::{Digest, Sha256};
const DOMAIN: &[u8] = b"zerus-task-control-admission-v1\0";

pub(super) fn digest(value: &Value) -> Result<String, String> {
    tagged_digest(DOMAIN, value)
}
pub(crate) fn tagged_digest(domain: &[u8], value: &Value) -> Result<String, String> {
    let mut bytes = domain.to_vec();
    encode(value, &mut bytes)?;
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
        Value::Number(_) => return Err("Task-control canonical encoding refuses floats".into()),
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
    fn fixed_utf8_order_integer_and_escape_encoding_is_independent_of_input_order() {
        let a: Value = serde_json::from_str(
            r#"{"z":[1,18446744073709551615,null],"é":"\n猫","a":{"x":true,"b":-2}}"#,
        )
        .unwrap();
        let mut bytes = Vec::new();
        encode(&a, &mut bytes).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "{\"a\":{\"b\":-2,\"x\":true},\"z\":[1,18446744073709551615,null],\"é\":\"\\n猫\"}"
        );
        let b = json!({"a":{"b":-2,"x":true},"é":"\n猫","z":[1,u64::MAX,null]});
        assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
        let mut untagged = Vec::new();
        encode(&a, &mut untagged).unwrap();
        assert_ne!(
            digest(&a).unwrap(),
            format!("{:x}", Sha256::digest(untagged))
        );
    }
    #[test]
    fn floats_are_refused_even_when_they_have_an_integer_value() {
        for a in [json!(1.0), json!([1.5]), json!({"nested":{"x":0.0}})] {
            assert!(digest(&a).is_err());
        }
    }
}
