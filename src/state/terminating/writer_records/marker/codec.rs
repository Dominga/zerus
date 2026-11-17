use super::*;
use serde::{
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserializer,
};
use std::{collections::BTreeSet, fmt, io::Write};

pub(super) fn encoded(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Limited {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("Marker metadata exceeds its bound"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Limited {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| super::super::REFUSED)?;
    Ok(output.bytes)
}
pub(super) fn text<'a>(value: &'a Value, key: &str, limit: usize) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
        .ok_or_else(|| super::super::REFUSED.into())
}
fn pid(value: &Value) -> Result<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| super::super::REFUSED.into())
}

/// Immutable target correlation only. No PID, pathname or stored native birth
/// in this projection establishes target/process/consent authority.
fn target(record: &Value) -> Result<serde_json::Map<String, Value>> {
    let refuse = || super::super::REFUSED.to_string();
    if !record.is_object()
        || record["version"] != 1
        || record.get("terminating").is_some()
        || record.get("layout_transition").is_some()
        || record.get("pausing").is_some()
        || record["resume_pending"] == true
        || record
            .get("resume_pending")
            .is_some_and(|v| !v.is_boolean() && !v.is_null())
        || record
            .get("expected_id")
            .is_some_and(|v| !v.is_null() && v != "")
        || record
            .get("archive_id")
            .is_some_and(|v| !v.is_null() && v != "")
        || record.get("rename_shadow").is_some()
        || record.get("rename_target").is_some()
    {
        return Err(refuse());
    }
    for key in ["session_id", "run_id"] {
        if !super::super::uuid(text(record, key, 36)?) {
            return Err(refuse());
        }
    }
    let agent = text(record, "agent", 64)?;
    if !matches!(agent, "claude" | "codex" | "kimi")
        || !text(record, "name", 512)?.starts_with(&format!("{agent}/"))
    {
        return Err(refuse());
    }
    text(record, "conversation_id", 1024)?;
    pid(&record["pid"])?;
    pid(&record["supervisor"]["pid"])?;
    text(&record["supervisor"], "start", 128)?;
    let keys = [
        "version",
        "session_id",
        "run_id",
        "name",
        "conversation_id",
        "agent",
        "pid",
        "process_start",
        "supervisor",
        "pane",
        "terminal_name",
        "cwd",
        "launch_dir",
        "hgs",
    ];
    for key in ["process_start", "pane", "terminal_name"] {
        text(record, key, 512)?;
    }
    if !text(record, "pane", 128)?
        .strip_prefix('%')
        .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(refuse());
    }
    for key in ["cwd", "launch_dir", "hgs"] {
        if !std::path::Path::new(text(record, key, 4096)?).is_absolute() {
            return Err(refuse());
        }
    }
    Ok(keys
        .into_iter()
        .map(|k| (k.into(), record[k].clone()))
        .collect())
}
pub(super) fn binding(record: &Value) -> Result<Value> {
    let mut fields = target(record)?;
    let mut configuration = serde_json::Map::new();
    for key in [
        "layout",
        "worktree_owner",
        "original_owner",
        "legacy_shadow",
        "process_index_pin",
        "base",
        "created",
        "launch_id",
        "startup_kind",
        "account_id",
        "agent_home",
        "permission_mode",
    ] {
        if let Some(value) = record.get(key) {
            if key == "layout" && value != "id" {
                return Err(super::super::REFUSED.into());
            }
            if key == "legacy_shadow"
                && (text(value, "name", 512).is_err() || value["run_id"] != record["run_id"])
            {
                return Err(super::super::REFUSED.into());
            }
            configuration.insert(key.into(), value.clone());
        }
    }
    fields.insert(
        "configuration_record_sha256".into(),
        crate::state::record_source::Digest::capture(&Value::Object(configuration))
            .hex()
            .into(),
    );
    let value = Value::Object(fields);
    encoded(&value, MARKER_BYTES)?;
    Ok(value)
}
pub(super) fn validate(data: &Data) -> Result<()> {
    let bad = || super::super::REFUSED.to_string();
    if data.version != 1
        || data.phase != "reserved"
        || !super::super::uuid(&data.request_id)
        || !super::super::digest(&data.intent_sha256)
        || !super::super::digest(&data.correlation.source_sha256)
        || !super::super::digest(&data.correlation.configuration_sha256)
        || !(super::super::Owner {
            pid: data.owner_pid,
            birth: data.owner_birth.clone(),
        })
        .valid()
    {
        return Err(bad());
    }
    for birth in [
        &data.correlation.root_birth,
        &data.correlation.supervisor_birth,
    ] {
        if !(super::super::Owner {
            pid: 1,
            birth: birth.clone(),
        })
        .valid()
        {
            return Err(bad());
        }
    }
    let mut projected = target(&data.binding)?;
    let configuration = text(&data.binding, "configuration_record_sha256", 64)?;
    if !super::super::digest(configuration) {
        return Err(bad());
    }
    projected.insert("configuration_record_sha256".into(), configuration.into());
    if Value::Object(projected) != data.binding {
        return Err(bad());
    }
    if !data
        .correlation
        .terminal_session
        .strip_prefix('$')
        .is_some_and(|v| !v.is_empty() && v.len() <= 96 && v.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(bad());
    }
    encoded(data, MARKER_BYTES)?;
    Ok(())
}
pub(super) fn decode(value: &Value) -> Result<Data> {
    let bytes = encoded(value, MARKER_BYTES)?;
    let data: Data = serde_json::from_slice(&bytes).map_err(|_| super::super::REFUSED)?;
    validate(&data)?;
    Ok(data)
}

/// Preserve ordinary metadata, but never silently discard duplicate keys
/// before validating the marker/source. Bound both recursion and node work.
pub(in crate::state::terminating::writer_records) fn record(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() || bytes.len() > RECORD_BYTES {
        return Err(super::super::REFUSED.into());
    }
    struct Seed<'a> {
        left: &'a mut usize,
        depth: usize,
    }
    impl<'de> DeserializeSeed<'de> for Seed<'_> {
        type Value = Value;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
            if self.depth > 64 || *self.left == 0 {
                return Err(serde::de::Error::custom("record complexity"));
            }
            *self.left -= 1;
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Seed<'_> {
        type Value = Value;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("bounded unique JSON")
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Value, E> {
            Ok(Value::Bool(v))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Value, E> {
            Ok(v.into())
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Value, E> {
            Ok(v.into())
        }
        fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Value, E> {
            serde_json::Number::from_f64(v)
                .map(Value::Number)
                .ok_or_else(|| E::custom("nonfinite JSON"))
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Value, E> {
            Ok(v.into())
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Value, E> {
            Ok(v.into())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Value, A::Error> {
            let mut out = Vec::new();
            while let Some(v) = seq.next_element_seed(Seed {
                left: self.left,
                depth: self.depth + 1,
            })? {
                out.push(v);
            }
            Ok(Value::Array(out))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
            let mut keys = BTreeSet::new();
            let mut out = serde_json::Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key.clone()) {
                    return Err(serde::de::Error::custom("duplicate JSON key"));
                }
                let value = map.next_value_seed(Seed {
                    left: self.left,
                    depth: self.depth + 1,
                })?;
                out.insert(key, value);
            }
            Ok(Value::Object(out))
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let mut left = 65536;
    let value = Seed {
        left: &mut left,
        depth: 0,
    }
    .deserialize(&mut decoder)
    .map_err(|_| super::super::REFUSED)?;
    decoder.end().map_err(|_| super::super::REFUSED)?;
    Ok(value)
}
