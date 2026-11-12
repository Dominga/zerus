//! Owner feedback 2026-11-04 (items 9-12): a request object that names a field
//! the operation does not know is refused with that field, the fields it does
//! take and an example, instead of the field being dropped in silence. The
//! accepted fields are read from the serde definition itself, so the list
//! cannot drift from the model.
use anyhow::bail;
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
        (_, "agent" | "account" | "computer") if what.contains("update") => " (agent, account and computer stay with Swap member)",
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
/// besides `T`'s own). Task #26: a value serde refuses names its field and
/// serde's reason in the message itself: team 33fbafeb's lead saw only "A
/// member is invalid" and an example for a member whose `folder` was the
/// catalog's `read_only`.
pub(crate) fn parse<T: DeserializeOwned>(value: &Value, extra: &[&str], what: &str, example: &str) -> anyhow::Result<T> {
    let mut allowed: Vec<&str> = fields::<T>().to_vec();
    allowed.extend_from_slice(extra);
    check_keys(value, &allowed, what, example)?;
    let error = match serde_json::from_value::<T>(value.clone()) {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error.to_string(),
    };
    let who = match value["id"].as_str().map(str::to_owned).or_else(|| value["id"].as_u64().map(|id| id.to_string())) {
        Some(id) if !id.is_empty() => format!("{what} \"{id}\""),
        _ => what.to_owned(),
    };
    match failing_field::<T>(value, &error) {
        Some(field) => bail!("{who} is invalid: field \"{field}\": {error}; example: {example}"),
        None => bail!("{who} is invalid: {error}; example: {example}"),
    }
}

/// The field whose value serde refused: the one whose removal makes the rest
/// parse, else the one whose removal changes the refusal into another one
/// (two bad fields), else a required field serde then misses. None when no
/// single field explains it.
fn failing_field<T: DeserializeOwned>(value: &Value, error: &str) -> Option<String> {
    let object = value.as_object()?;
    let without = |key: &str| {
        let mut rest = object.clone();
        rest.remove(key);
        serde_json::from_value::<T>(Value::Object(rest)).map(drop).map_err(|error| error.to_string())
    };
    let missing = |key: &str, other: &str| other.starts_with(&format!("missing field `{key}`"));
    let mut required = None;
    for key in object.keys() {
        match without(key) {
            Ok(()) => return Some(key.clone()),
            Err(other) if missing(key, &other) => {
                required.get_or_insert_with(|| key.clone());
            }
            Err(other) if other != error => return Some(key.clone()),
            Err(_) => {}
        }
    }
    // A required field's own value: the only one whose removal is missed.
    required.filter(|_| !error.starts_with("missing field"))
}

pub(crate) const MEMBER_EXAMPLE: &str =
    r#"{"id":"dev","role":"Backend","agent":"claude","account":"work","model":"opus","effort":"medium"}"#;
/// Task #6: a `members_update` entry; the fields it takes are listed by the
/// refusal itself.
pub(crate) const MEMBER_UPDATE_EXAMPLE: &str = r#"{"id":"dev","model":"opus","effort":"high"}"#;
pub(crate) const LEAD_EXAMPLE: &str = r#"{"id":"lead","role":"Lead","agent":"claude","account":"work"}"#;
pub(crate) const TASK_EXAMPLE: &str = r#"{"id":2,"title":"Build the page","description":"What to do and how to check it","assignee":"dev","deps":[{"task":1,"kind":"hard"}]}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Task #465: every member field the desktop sends (the list shared with
    /// client/core) is one this model takes, so the strict check never
    /// refuses a New team lead or member for a client-only key.
    #[test]
    fn the_desktops_member_fields_are_the_models() {
        let shared: Value = serde_json::from_str(include_str!("../../../client/core/src/teams/member_fields.json")).unwrap();
        let known = fields::<super::super::model::Member>();
        let sent = shared["fields"].as_array().unwrap();
        assert!(!sent.is_empty());
        for field in sent {
            let field = field.as_str().unwrap();
            assert!(known.contains(&field), "the desktop sends {field}, which a member does not take: {known:?}");
        }
        assert!(!known.contains(&"lead"), "lead is the desktop's own key");
    }

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
        assert!(error.contains("A member \"d\" is invalid: field \"models\""), "{error}");
        assert!(error.contains("invalid type") && error.contains("example:"), "{error}");
    }

    /// Task #26: every refused field is named in the message itself, not
    /// only in a cause a caller may not print.
    #[test]
    fn a_refused_value_names_its_field() {
        use super::super::model::{Member, Task};
        let member = |value: Value| parse::<Member>(&value, &[], "A member", MEMBER_EXAMPLE).unwrap_err().to_string();
        let error = member(json!({"id":"qa","agent":"claude","folder":"elsewhere"}));
        assert!(error.contains("A member \"qa\" is invalid: field \"folder\": unknown variant `elsewhere`"), "{error}");
        let error = member(json!({"id":"qa","agent":"claude","folder":"nowhere","models":"opus"}));
        assert!(error.contains("field \"folder\"") || error.contains("field \"models\""), "{error}");
        let error = member(json!({"id":7,"agent":"claude"}));
        assert!(error.contains("field \"id\""), "{error}");
        let task = |value: Value| parse::<Task>(&value, &[], "A task", TASK_EXAMPLE).unwrap_err().to_string();
        let error = task(json!({"id":2,"title":"Follow-up","assignee":"dev","deps":[1]}));
        assert!(error.contains("A task \"2\" is invalid: field \"deps\""), "{error}");
        let error = task(json!({"id":2,"title":["no"]}));
        assert!(error.contains("field \"title\""), "{error}");
    }
}
