//! Historical observed edges only. Never construct a Tracker/Delivery or
//! interpret a saved token, retry flag or question ID as execution authority.
use super::{
    pages, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream, READ_BYTES, READ_ENTRIES,
};
use crate::attention::decode_observed;
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

const KEY: &str = "attention/observed";
type Chunks = Box<dyn Iterator<Item = SourceResult<Value>> + Send>;

fn document(source: SourceRecord, hosts: Option<Map<String, Value>>) -> SourceResult<SourceStream> {
    source
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    let mut count = 0;
    let mut shapes = Map::new();
    for (host, value) in hosts.iter().flatten() {
        count += value.as_object().map_or(1, |entries| entries.len().max(1));
        if count > READ_ENTRIES {
            return Err(SourceError::TooLarge);
        }
        shapes.insert(
            host.clone(),
            json!(if value.is_object() { "object" } else { "value" }),
        );
    }
    let header =
        json!({"kind":"attention-observed-header","present":hosts.is_some(),"hosts":shapes});
    super::super::bounded_json(&header, MAX_SNAPSHOT_BYTES as usize)?;
    let entries = hosts
        .into_iter()
        .flatten()
        .flat_map(|(host, value)| -> Chunks {
            match value {
                Value::Object(entries) => Box::new(
                    entries
                        .into_iter()
                        .map(move |(key, value)| Ok(json!({"host":host,"key":key,"value":value}))),
                ),
                value => Box::new(std::iter::once(Ok(json!({"host":host,"value":value})))),
            }
        });
    Ok(SourceStream::new(
        source,
        std::iter::once(Ok(header)).chain(pages(entries)),
    ))
}

fn decode(bytes: &[u8]) -> SourceResult<Map<String, Value>> {
    if bytes.len() > READ_BYTES {
        return Err(SourceError::TooLarge);
    }
    decode_observed(bytes).ok_or(SourceError::UnsupportedSource)
}

/// Preserve the original Qt object, including unknown/malformed child values
/// the existing reader retains. A missing key is distinct from an empty map.
pub fn qt_attention_observed(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let hosts = snapshot
        .values
        .get(KEY)
        .map(|value| match value {
            QtValue::Bytes(bytes) => decode(bytes),
            QtValue::Text(text) => decode(text.as_bytes()),
            _ => Err(SourceError::UnsupportedSource),
        })
        .transpose()?;
    document(
        SourceRecord {
            family: "qt-attention-observed".into(),
            schema: 1,
            format: snapshot.source.into(),
            sha256: snapshot.sha256.clone(),
            build: None,
        },
        hosts,
    )
}

/// Next's attention-observed.json, decoded without seeding or normalizing a
/// live tracker. The caller supplies bounded bytes through the existing reader.
pub fn next_attention_observed(bytes: &[u8]) -> SourceResult<SourceStream> {
    document(
        SourceRecord {
            family: "next-attention-observed".into(),
            schema: 1,
            format: "attention-observed-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        Some(decode(bytes)?),
    )
}

#[cfg(test)]
mod tests;
