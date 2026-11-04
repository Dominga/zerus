//! Owner feedback 2026-11-04 (items 9-12): a request object that names a field
//! the operation does not know is refused with that field, the fields it does
//! take and an example, instead of the field being dropped in silence. The
//! accepted fields are read from the serde definition itself, so the list
//! cannot drift from the model.
use anyhow::{bail, Context};
use serde::de::{self, DeserializeOwned, Visitor};
use serde_json::Value;

/// The field names `T`'s derived `Deserialize` reads (serde passes them to
/// `deserialize_struct`); empty for a type that is not a plain struct.
pub(crate) fn fields<T: DeserializeOwned>() -> &'static [&'static str] {
    struct Probe;
    #[derive(Debug)]
    struct Stop(Option<&'static [&'static str]>);
    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("probe")
        }
    }
    impl std::error::Error for Stop {}
    impl de::Error for Stop {
        fn custom<M: std::fmt::Display>(_: M) -> Self {
            Stop(None)
        }
    }
    impl<'de> de::Deserializer<'de> for Probe {
        type Error = Stop;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
            Err(Stop(None))
        }
        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            Err(Stop(Some(fields)))
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }
    match T::deserialize(Probe) {
        Err(Stop(Some(fields))) => fields,
        _ => &[],
    }
}

/// A hint for a field people often write by another name.
fn hint(what: &str, key: &str) -> &'static str {
    match (what, key) {
        (_, "text" | "body" | "details") => " (a task's text is \"description\")",
        (_, "name") if what.contains("member") || what.contains("lead") || what.contains("Member") || what.contains("Lead") => " (a member is named by \"id\" and \"role\")",
        (_, "with" | "to" | "new") => " (name the new value directly: \"agent\", \"model\", \"effort\", \"account\" or \"prompt\")",
        (_, "models") => " (a list of {\"model\":\"...\",\"effort\":\"...\"} or \"model@effort\" strings)",
        _ => "",
    }
}

/// Refuses every key of `value` outside `allowed`, naming the first unknown
/// one, the accepted fields and an example. `value` must be an object.
pub(crate) fn check_keys(value: &Value, allowed: &[&str], what: &str, example: &str) -> anyhow::Result<()> {
    let Some(object) = value.as_object() else {
        bail!("{what} is a JSON object, such as {example}");
    };
    let unknown: Vec<&str> = object.keys().map(String::as_str).filter(|key| !allowed.contains(key)).collect();
    if let Some(first) = unknown.first() {
        let mut known: Vec<&str> = allowed.to_vec();
        known.sort_unstable();
        bail!(
            "{what} has no field \"{first}\"{}{}; it takes: {}. Example: {example}",
            hint(what, first),
            if unknown.len() > 1 { format!(" (unknown too: {})", unknown[1..].join(", ")) } else { String::new() },
            known.join(", ")
        );
    }
    Ok(())
}

/// Parses `value` as `T`, refusing unknown fields (with `extra` keys allowed
/// besides `T`'s own) and naming serde's own reason with an example.
pub(crate) fn parse<T: DeserializeOwned>(value: &Value, extra: &[&str], what: &str, example: &str) -> anyhow::Result<T> {
    let mut allowed: Vec<&str> = fields::<T>().to_vec();
    allowed.extend_from_slice(extra);
    check_keys(value, &allowed, what, example)?;
    serde_json::from_value(value.clone()).map_err(|error| anyhow::anyhow!("{error}")).with_context(|| format!("{what} is invalid; example: {example}"))
}

pub(crate) const MEMBER_EXAMPLE: &str =
    r#"{"id":"dev","role":"Backend","agent":"claude","account":"work","model":"opus","effort":"medium"}"#;
pub(crate) const LEAD_EXAMPLE: &str = r#"{"id":"lead","role":"Lead","agent":"claude","account":"work"}"#;
pub(crate) const TASK_EXAMPLE: &str =
    r#"{"id":1,"title":"Build the page","description":"What to do and how to check it","assignee":"dev"}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_fields_come_from_the_model() {
        let known = fields::<super::super::model::Member>();
        for field in ["id", "role", "agent", "account", "model", "effort", "prompt", "models", "folder"] {
            assert!(known.contains(&field), "{field} missing from {known:?}");
        }
        assert!(fields::<super::super::model::Task>().contains(&"description"));
    }

    #[test]
    fn an_unknown_field_is_named_with_its_fix_and_an_example() {
        let error = parse::<super::super::model::Task>(&json!({"id":1,"title":"t","text":"lost"}), &[], "A task", TASK_EXAMPLE)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no field \"text\""), "{error}");
        assert!(error.contains("\"description\""), "{error}");
        assert!(error.contains("Example:"), "{error}");
        let error = parse::<super::super::model::Member>(&json!({"id":"d","agent":"claude","with":{"model":"x"}}), &[], "A member", MEMBER_EXAMPLE)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no field \"with\""), "{error}");
        let error = parse::<super::super::model::Member>(&json!("dev"), &[], "The lead", LEAD_EXAMPLE).unwrap_err().to_string();
        assert!(error.contains("The lead is a JSON object"), "{error}");
    }

    #[test]
    fn a_wrong_type_names_serdes_reason_and_the_example() {
        let error = format!(
            "{:#}",
            parse::<super::super::model::Member>(&json!({"id":"d","agent":"claude","models":"opus"}), &[], "A member", MEMBER_EXAMPLE).unwrap_err()
        );
        assert!(error.contains("A member is invalid; example:"), "{error}");
        assert!(error.contains("invalid type"), "{error}");
    }
}
