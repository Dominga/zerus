//! One refusal parser for the reads and the actions: `hgs` answers a refusal
//! as `{"ok": false, "error": {"code", "message"}}`, and only a stable code
//! proves that nothing was written or read. A reply with a missing, empty or
//! malformed code is not a refusal at all.

use serde_json::Value;

/// The longest refusal message kept.
pub const MESSAGE_CHARS: usize = 2000;
/// The longest code a refusal may carry (the store's are short snake_case).
const CODE_CHARS: usize = 64;

/// The refusal an `hgs` reply carries, when it carries a valid one: `ok` is
/// false and the error names a well-formed stable code. A message may be
/// empty; the code may not.
pub(crate) fn parse(stdout: &[u8]) -> Option<(String, String)> {
    parse_with_field(stdout).map(|(code, message, _)| (code, message))
}

pub(crate) fn parse_with_field(stdout: &[u8]) -> Option<(String, String, Option<String>)> {
    let reply: Value = serde_json::from_slice(stdout).ok()?;
    if reply.get("ok").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let error = reply.get("error")?;
    let code = error.get("code").and_then(Value::as_str)?;
    if !well_formed(code) {
        return None;
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(MESSAGE_CHARS)
        .collect();
    let field = error
        .get("field")
        .and_then(Value::as_str)
        .filter(|field| {
            !field.is_empty()
                && field.len() <= 128
                && field.split('.').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                })
        })
        .map(str::to_owned);
    Some((code.to_owned(), message, field))
}

/// A stable code: lowercase letters, digits and underscores, short.
fn well_formed(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= CODE_CHARS
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_well_formed_refusal_counts() {
        let parse = |text: &str| parse(text.as_bytes());
        assert_eq!(
            parse(r#"{"ok":false,"error":{"code":"stale_revision","message":"Moved."}}"#),
            Some(("stale_revision".into(), "Moved.".into()))
        );
        assert_eq!(
            parse(r#"{"ok":false,"error":{"code":"teams_inactive"}}"#),
            Some(("teams_inactive".into(), String::new()))
        );
        for bad in [
            r#"{"ok":false,"error":{"code":""}}"#,
            r#"{"ok":false,"error":{}}"#,
            r#"{"ok":false,"error":"nope"}"#,
            r#"{"ok":false,"error":{"code":"Stale"}}"#,
            r#"{"ok":false,"error":{"code":"has space"}}"#,
            r#"{"ok":false,"error":{"code":"with\nnewline"}}"#,
            r#"{"ok":false}"#,
            r#"{"ok":true,"error":{"code":"stale_revision"}}"#,
            "not json",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(parse(&format!(
            r#"{{"ok":false,"error":{{"code":"{}"}}}}"#,
            "x".repeat(CODE_CHARS + 1)
        ))
        .is_none());
    }
}
