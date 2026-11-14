//! Bind a prepared restore to its complete normalized, selected archive.
use super::*;
use crate::state::record_source::Digest;

pub(super) struct Source(Digest);

impl Source {
    /// Capture before removing legacy compatibility fields. Keep only a fixed
    /// digest, not another archive copy or a cache indexed by session count.
    pub(super) fn capture(archive: &Value) -> Self {
        Self(Digest::capture(archive))
    }

    /// Called with the state writer held, before worktree Restore and again
    /// before publishing its pending binding and starting the native runner.
    /// Reread one normalized archive, including unknown and negative evidence.
    pub(super) fn check(&self, name: &str, id: &str) -> Result<Value> {
        let changed = || {
            format!("{name}: archived source changed or is unavailable; select the archive again before restoring")
        };
        let current = archive::read_archive(name, id).map_err(|_| changed())?;
        if !self.0.matches(&current) {
            return Err(changed());
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_and_type_is_bound_including_unknown_negative_evidence() {
        let record = json!({"run_id":"run", "conversation_id":"conversation",
            "layout":"id", "terminal_name":"old-terminal", "base":["claude"],
            "future_unknown_field":{"opaque":"must bind the content too"},
            "layout_transition":{"opaque":[true, 4, "proof"]}});
        let original = Source::capture(&record).0;
        for key in record
            .as_object()
            .unwrap()
            .keys()
            .chain(["unknown_field".into()].iter())
        {
            for value in [
                Value::Null,
                json!(false),
                json!(0),
                json!(""),
                json!([]),
                json!({}),
            ] {
                let mut changed = record.clone();
                changed[key] = value;
                assert_ne!(
                    Source::capture(&changed).0,
                    original,
                    "{key} is source evidence"
                );
            }
            let mut changed = record.clone();
            changed.as_object_mut().unwrap().remove(key);
            if changed != record {
                assert_ne!(
                    Source::capture(&changed).0,
                    original,
                    "removing {key} changes the source"
                );
            }
        }
        assert_ne!(
            Source::capture(&json!({})).0,
            Source::capture(&json!({"layout_transition":null})).0
        );
    }

    #[test]
    fn canonical_stream_ignores_formatting_but_preserves_structure() {
        let first: Value = serde_json::from_str(r#"{"z":[1,{"b":2,"a":3}],"a":null}"#).unwrap();
        let second: Value =
            serde_json::from_str("{\n \"a\":null, \"z\":[1, {\"a\":3, \"b\":2}]} ").unwrap();
        assert_eq!(Source::capture(&first).0, Source::capture(&second).0);
        for (left, right) in [
            (json!([1, 2]), json!([2, 1])),
            (json!(["ab", "c"]), json!(["a", "bc"])),
            (json!({"ab":"c"}), json!({"a":"bc"})),
            (json!("1"), json!(1)),
            (json!(false), Value::Null),
            (json!([[]]), json!([])),
        ] {
            assert_ne!(Source::capture(&left).0, Source::capture(&right).0);
        }
    }
}
