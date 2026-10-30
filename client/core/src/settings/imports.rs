//! Versioned first-G3 imports of previously unhandled UI families. The old
//! settings seed is never repeated. Values and their handled record commit
//! together in the existing settings writer, with Next's choices taking priority.
use super::{Contents, QtSnapshot, QtStore, QtValue, MAX_STORE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 1;
const MAX_KEYS: usize = 4096;
const FAMILIES: &[&str] = &[
    "machine-appearance",
    "project-view",
    "row-density",
    "task-preferences",
];

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    families: BTreeMap<String, FamilyRecord>,
    quarantine: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilyRecord {
    version: u32,
    family: String,
    schema: u32,
    source: String,
    source_path: String,
    at: u64,
    keys: Vec<String>,
    sha256: Option<String>,
    source_mtime_after_switch: Option<bool>,
}

fn journal(contents: &Contents) -> Result<Journal, &'static str> {
    let Some(saved) = contents.extra.get("g3") else {
        return Ok(Journal {
            version: VERSION,
            ..Journal::default()
        });
    };
    let journal: Journal =
        serde_json::from_value(saved.clone()).map_err(|_| "invalid G3 family record")?;
    if journal.version != VERSION || journal.families.len() > 256 || journal.quarantine.len() > 256
    {
        return Err("unsupported G3 family record");
    }
    // An unknown family is retained, never interpreted or imported again.
    for (family, record) in &journal.families {
        let digest = match &record.sha256 {
            Some(hash) => {
                record.source != "none"
                    && hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }
            None => record.source == "none" && record.keys.is_empty(),
        };
        if record.family != *family
            || record.version != VERSION
            || record.schema != 1
            || !digest
            || record.keys.len() > MAX_KEYS
            || record.source_path.is_empty()
            || !["none", "qt-ini", "qt-plist", "qt-defaults"].contains(&record.source.as_str())
        {
            return Err("invalid handled G3 family");
        }
    }
    Ok(journal)
}

pub(super) fn pending(contents: &Contents) -> Result<bool, &'static str> {
    let journal = journal(contents)?;
    Ok(FAMILIES
        .iter()
        .any(|family| !journal.families.contains_key(*family)))
}

fn matches(family: &str, key: &str) -> bool {
    match family {
        "machine-appearance" => {
            key.starts_with("machines/colors/") || key.starts_with("machines/vivid/")
        }
        "project-view" => key == "workspace/allSwarmProjects",
        "row-density" => key == crate::row_preferences::KEY,
        "task-preferences" => key == "tasks/onlySession",
        _ => false,
    }
}

fn converted(key: &str, value: &QtValue) -> Result<Value, &'static str> {
    if key == crate::row_preferences::KEY {
        return crate::row_preferences::from_qt(value).ok_or("invalid packed row-density keys");
    }
    let kind = super::find(key).ok_or("invalid preference identity")?.kind;
    if key.starts_with("machines/colors/") {
        let text = value.to_text();
        if !matches!(value, QtValue::Text(_) | QtValue::Bytes(_))
            || crate::trayicon::parse_color(&text).is_none()
        {
            return Err("invalid machine color");
        }
    } else {
        let valid = match value {
            QtValue::Bool(_) | QtValue::Int(_) => true,
            QtValue::Double(n) => n.is_finite(),
            QtValue::Text(s) => {
                ["", "0", "1", "true", "false"].contains(&s.to_ascii_lowercase().as_str())
            }
            _ => false,
        };
        if !valid {
            return Err("invalid preference flag");
        }
    }
    Ok(super::from_qt(kind, value))
}

pub(super) fn apply(
    contents: &mut Contents,
    store: &QtStore,
    snapshot: Option<&QtSnapshot>,
) -> bool {
    let Ok(mut journal) = journal(contents) else {
        return false;
    };
    let at = super::seconds(std::time::SystemTime::now()).unwrap_or(0);
    let old_seed_at = contents.seed.as_ref().and_then(|s| s["at"].as_u64());
    let changed = snapshot
        .and_then(|s| s.modified_at)
        .zip(old_seed_at)
        .map(|(mtime, seed)| mtime > seed);
    let handled_keys: BTreeSet<String> = contents
        .seed
        .as_ref()
        .and_then(|s| s["keys"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut dirty = false;
    for family in FAMILIES {
        if journal.families.contains_key(*family) {
            continue;
        }
        let entries: Vec<_> = snapshot
            .into_iter()
            .flat_map(|s| s.values.iter())
            .filter(|(key, _)| matches(family, key))
            .take(MAX_KEYS + 1)
            .collect();
        let mut candidate = contents.clone();
        let converted: Result<Map<String, Value>, _> = if entries.len() > MAX_KEYS {
            Err("family exceeds its key limit")
        } else {
            entries
                .iter()
                // Existing Next values and keys handled by the old seed are
                // not candidates. Malformed unused Qt values cannot turn a
                // deliberate Next choice into an import/retry requirement.
                .filter(|(key, _)| {
                    !contents.values.contains_key(*key) && !handled_keys.contains(*key)
                })
                .map(|(key, value)| converted(key, value).map(|v| ((*key).clone(), v)))
                .collect()
        };
        let mut record = json!({
            "version": VERSION, "family": family, "schema": 1,
            "source": snapshot.map_or("none", |s| s.source),
            "source_path": store.path().to_string_lossy(), "at": at,
            "keys": entries.iter().take(MAX_KEYS).map(|(k, _)| *k).collect::<Vec<_>>(),
            "sha256": snapshot.map(|s| &s.sha256), "source_mtime_after_switch": changed,
        });
        let values = match converted {
            Ok(values) => values,
            Err(reason) => {
                // Private metadata points to the unchanged source. Source
                // values (which might contain secrets) never enter a report.
                record["reason"] = json!(reason);
                record["preserved_in"] = json!("source");
                journal.quarantine.insert((*family).into(), record);
                candidate
                    .extra
                    .insert("g3".into(), serde_json::to_value(&journal).unwrap());
                if candidate.to_bytes().len() as u64 <= MAX_STORE {
                    *contents = candidate;
                    dirty = true;
                }
                continue;
            }
        };
        for (key, value) in values {
            candidate.values.entry(key).or_insert(value);
        }
        journal
            .families
            .insert((*family).into(), serde_json::from_value(record).unwrap());
        candidate
            .extra
            .insert("g3".into(), serde_json::to_value(&journal).unwrap());
        if candidate.to_bytes().len() as u64 <= MAX_STORE {
            *contents = candidate;
            dirty = true;
        } else {
            journal.families.remove(*family);
        }
    }
    dirty
}
