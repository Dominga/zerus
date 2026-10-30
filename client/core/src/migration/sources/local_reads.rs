//! Original local presentation/read checkpoints, never shared read authority.
use super::{
    pages, qt_json_bounded, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream,
    READ_BYTES, READ_ENTRIES,
};
use crate::marks::{self, Mark};
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord};

const MARKS: &str = "attention/sessionMarks";
const REPLIES: &str = "attention/readReplies";
type Chunks = Box<dyn Iterator<Item = SourceResult<Value>> + Send>;

// Parse only the existing tuple shape. Never rewrite its original JSON text,
// convert a raw command to a frontend agent or infer a current host identity.
fn key(key: &str, reply: bool) -> SourceResult<()> {
    if key.len() > 32 << 10 {
        return Err(SourceError::TooLarge);
    }
    let tuple: Vec<Value> =
        serde_json::from_str(key).map_err(|_| SourceError::UnsupportedSource)?;
    let text = |v: &Value, empty: bool| {
        v.as_str().is_some_and(|s| {
            (empty || !s.is_empty()) && s.len() <= 8192 && !s.chars().any(char::is_control)
        })
    };
    let shape = match tuple.as_slice() {
        [host, agent, conversation] => {
            text(host, true) && text(agent, false) && text(conversation, false)
        }
        [host, agent, name, created] if !reply => {
            text(host, true)
                && text(agent, false)
                && text(name, false)
                && created.as_i64().is_some()
        }
        _ => false,
    };
    if shape {
        Ok(())
    } else {
        Err(SourceError::UnsupportedSource)
    }
}

fn entry(key_text: &str, value: &Value, reply: bool) -> SourceResult<()> {
    key(key_text, reply)?;
    if reply {
        if !value
            .as_str()
            .is_some_and(|s| s.len() <= 8192 && !s.chars().any(char::is_control))
        {
            return Err(SourceError::UnsupportedSource);
        }
    } else {
        if !value.is_object() {
            return Err(SourceError::UnsupportedSource);
        }
        serde_json::from_value::<Mark>(value.clone())
            .map_err(|_| SourceError::UnsupportedSource)?;
    }
    Ok(())
}

/// Qt's separate original maps. Decode both before producing either; explicit
/// clears, raw keys and private extensions survive without setting any mark.
pub fn qt_local_read_marks(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let source = SourceRecord {
        family: "qt-local-read-marks".into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    source
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    let mut chunks: Vec<Chunks> = Vec::new();
    let mut total = 0;
    for scope in [MARKS, REPLIES] {
        let object = snapshot
            .values
            .get(scope)
            .map(|value| {
                total += match value {
                    QtValue::Bytes(v) => v.len(),
                    QtValue::Text(v) => v.len(),
                    _ => return Err(SourceError::UnsupportedSource),
                };
                if total > READ_BYTES {
                    return Err(SourceError::TooLarge);
                }
                let Value::Object(object) = qt_json_bounded(value, READ_BYTES)? else {
                    return Err(SourceError::UnsupportedSource);
                };
                if object.len() > READ_ENTRIES {
                    return Err(SourceError::TooLarge);
                }
                for (key, value) in &object {
                    entry(key, value, scope == REPLIES)?;
                }
                Ok(object)
            })
            .transpose()?;
        let header = json!({"kind":"local-read-header","scope":scope,"present":object.is_some()});
        let entries = object
            .into_iter()
            .flatten()
            .map(move |(key, value)| Ok(json!({"scope":scope,"key":key,"value":value})));
        chunks.push(Box::new(std::iter::once(Ok(header)).chain(pages(entries))));
    }
    Ok(SourceStream::new(source, chunks.into_iter().flatten()))
}

/// The interim ordered file, checked through its existing decoder before any
/// live cap/deduplication. Original pair order and unknown fields stay private.
pub fn next_local_read_marks(bytes: &[u8]) -> SourceResult<SourceStream> {
    if bytes.len() > READ_BYTES {
        return Err(SourceError::TooLarge);
    }
    let mut root: Map<String, Value> =
        serde_json::from_slice(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    if root.get("version").is_some_and(|v| v != 1) {
        return Err(SourceError::UnsupportedSource);
    }
    marks::decode_saved(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    let mut fields = Map::new();
    let mut chunks: Vec<Chunks> = Vec::new();
    for scope in ["marks", "replies"] {
        let values = root.remove(scope);
        fields.insert(scope.into(), json!(values.is_some()));
        let values = match values {
            None => Vec::new(),
            Some(Value::Array(values)) if values.len() <= marks::CAP => values,
            Some(Value::Array(_)) => return Err(SourceError::TooLarge),
            _ => return Err(SourceError::UnsupportedSource),
        };
        for pair in &values {
            let pair = pair
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or(SourceError::UnsupportedSource)?;
            entry(
                pair[0].as_str().ok_or(SourceError::UnsupportedSource)?,
                &pair[1],
                scope == "replies",
            )?;
        }
        chunks.push(Box::new(pages(values.into_iter().enumerate().map(
            move |(index, pair)| {
                Ok(json!({"scope":scope,"index":index,"key":pair[0],"value":pair[1]}))
            },
        ))));
    }
    let header = json!({"kind":"local-read-file-header","fields":fields,"content":root});
    Ok(SourceStream::new(
        SourceRecord {
            family: "next-local-read-marks".into(),
            schema: 1,
            format: "attention-marks-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(header)).chain(chunks.into_iter().flatten()),
    ))
}

#[cfg(test)]
mod tests;
