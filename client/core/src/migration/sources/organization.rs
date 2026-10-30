//! Organization evidence through existing source decoders. These checkpoints
//! never become a live controller, an initialization source or an apply request.
use super::{
    pages, qt_json, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream, READ_BYTES,
    READ_ENTRIES,
};
use crate::swarm::draft::Draft;
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord};

const QT_KEYS: [&str; 5] = [
    "workspace/organization",
    "workspace/organizationBeforeProjects",
    "workspace/organizationBeforeSwarm",
    "swarm/observedOrganization",
    "swarm/observedVersions",
];

type Chunks = Box<dyn Iterator<Item = SourceResult<Value>> + Send>;

/// One object's exact fields. Arrays keep their order and explicit empty shape;
/// other values, including unknown nested objects, remain original items.
fn document(scope: &'static str, object: Option<Map<String, Value>>) -> SourceResult<Chunks> {
    let mut count = 0;
    let mut fields = Map::new();
    for (key, value) in object.iter().flatten() {
        count += match value {
            Value::Array(values) => values.len().max(1),
            _ => 1,
        };
        if count > READ_ENTRIES {
            return Err(SourceError::TooLarge);
        }
        fields.insert(
            key.clone(),
            json!(if value.is_array() { "array" } else { "value" }),
        );
    }
    let header = json!({"kind":"organization-header","scope":scope,"present":object.is_some(),"fields":fields});
    let entries = object
        .into_iter()
        .flatten()
        .flat_map(move |(field, value)| -> Chunks {
            match value {
                Value::Array(values) => {
                    Box::new(values.into_iter().enumerate().map(move |(index, value)| {
                        Ok(json!({"scope":scope,"field":field,"index":index,"value":value}))
                    }))
                }
                value => Box::new(std::iter::once(Ok(
                    json!({"scope":scope,"field":field,"value":value}),
                ))),
            }
        });
    Ok(Box::new(std::iter::once(Ok(header)).chain(pages(entries))))
}

/// Qt desired state, observed base/versions and historical backups remain
/// separate original objects. They prove no current observation or edit consent.
pub fn qt_organization_checkpoints(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let source = SourceRecord {
        family: "qt-organization-checkpoints".into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    source
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    let mut streams = Vec::new();
    let mut bytes = 0;
    for key in QT_KEYS {
        let object = snapshot
            .values
            .get(key)
            .map(|value| {
                bytes += match value {
                    QtValue::Bytes(value) => value.len(),
                    QtValue::Text(value) => value.len(),
                    _ => return Err(SourceError::UnsupportedSource),
                };
                if bytes > READ_BYTES {
                    return Err(SourceError::TooLarge);
                }
                let Value::Object(object) = qt_json(value)? else {
                    return Err(SourceError::UnsupportedSource);
                };
                // Versions is an opaque causal map, not an organization schema.
                // Legacy groups/backups are retained but never converted to projects.
                if key != "swarm/observedVersions"
                    && object.get("version").is_some_and(|v| v != 1 && v != 2)
                {
                    return Err(SourceError::UnsupportedSource);
                }
                Ok(object)
            })
            .transpose()?;
        streams.push(document(key, object)?);
    }
    Ok(SourceStream::new(source, streams.into_iter().flatten()))
}

/// Next's existing organization draft, validated through its pure decoder.
/// Never call draft::load/set_aside/store or Controller::restore/due here.
pub fn next_organization_draft(bytes: &[u8]) -> SourceResult<SourceStream> {
    if bytes.len() as u64 > crate::swarm::draft::MAX_BYTES {
        return Err(SourceError::TooLarge);
    }
    let mut root: Map<String, Value> =
        serde_json::from_slice(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    Draft::from_json(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    let mut streams = Vec::new();
    for key in ["base", "desired", "versions"] {
        let Value::Object(object) = root.remove(key).unwrap() else {
            unreachable!()
        };
        streams.push(document(key, Some(object))?);
    }
    let header = json!({"kind":"draft-header","content":root});
    Ok(SourceStream::new(
        SourceRecord {
            family: "next-organization-draft".into(),
            schema: 1,
            format: "organization-draft-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(header)).chain(streams.into_iter().flatten()),
    ))
}

#[cfg(test)]
mod tests;
