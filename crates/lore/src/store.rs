//! Canonical JSONL: sorted keys, compact separators, LF, one record per line,
//! sorted by id (items by number), final newline. A reader refuses any breach.
use crate::{LoreError, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Canonical bytes of one record, without its newline.
pub fn canonical(record: &Value) -> String {
    // serde_json's default Map is ordered by key, and to_string is compact.
    serde_json::to_string(record).unwrap_or_else(|_| "{}".into())
}

fn id_key(record: &Value) -> (String, u32) {
    let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
    match id.split_once('#') {
        Some((plan, number)) => (plan.to_owned(), number.parse().unwrap_or(0)),
        None => (id.to_owned(), 0),
    }
}

/// Canonical file bytes for `records`, ordered as FORMAT.md requires.
pub fn serialize(records: &[Value]) -> Vec<u8> {
    let mut sorted: Vec<&Value> = records.iter().collect();
    sorted.sort_by_key(|record| id_key(record));
    let mut out = String::new();
    for record in sorted {
        out.push_str(&canonical(record));
        out.push('\n');
    }
    out.into_bytes()
}

/// Parses and checks a whole file; every refusal names the file and line.
pub fn parse(bytes: &[u8], file: &str) -> Result<Vec<Value>> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(LoreError(format!("{file}:1: a BOM is not allowed")));
    }
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        return Err(LoreError(format!("{file}: CR line ends are not allowed")));
    }
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(LoreError(format!("{file}: no final newline")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LoreError(format!("{file}: not UTF-8")))?;
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    let mut previous: Option<(String, u32)> = None;
    for (index, line) in text.split_terminator('\n').enumerate() {
        let line_no = index + 1;
        if line.is_empty() {
            return Err(LoreError(format!("{file}:{line_no}: a blank line is not allowed")));
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|error| LoreError(format!("{file}:{line_no}: not a JSON object: {error}")))?;
        if !value.is_object() {
            return Err(LoreError(format!("{file}:{line_no}: not a JSON object")));
        }
        let canonical_line = canonical(&value);
        if canonical_line != line {
            return Err(LoreError(format!("{file}:{line_no}: the line is not canonical")));
        }
        let id = value.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        if !ids.insert(id.clone()) {
            return Err(LoreError(format!("{file}:{line_no}: id {id} appears twice")));
        }
        let key = id_key(&value);
        if previous.as_ref().is_some_and(|previous| *previous > key) {
            return Err(LoreError(format!("{file}:{line_no}: records are out of order")));
        }
        previous = Some(key);
        records.push(value);
    }
    Ok(records)
}

/// One store on disk: its root and the lock path.
#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }

    pub fn read(&self, relative: &str) -> Result<Vec<Value>> {
        let bytes = std::fs::read(self.root.join(relative))
            .map_err(|error| LoreError(format!("{relative}: {error}")))?;
        parse(&bytes, relative)
    }

    /// Writes one file with the canonical bytes, replacing atomically.
    pub fn write(&self, relative: &str, records: &[Value]) -> Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| LoreError(format!("{relative}: {error}")))?;
        }
        let temp = path.with_extension("lore-tmp");
        std::fs::write(&temp, serialize(records)).map_err(|error| LoreError(format!("{relative}: {error}")))?;
        for attempt in 0..3 {
            match std::fs::rename(&temp, &path) {
                Ok(()) => return Ok(()),
                Err(_) if attempt < 2 => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(error) => return Err(LoreError(format!("{relative}: {error}"))),
            }
        }
        Ok(())
    }

    /// Adds a record to its file, refusing a duplicate id.
    pub fn add(&self, relative: &str, record: Value) -> Result<()> {
        let mut records = self.read(relative).unwrap_or_default();
        let id = record.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        if records.iter().any(|existing| existing.get("id").and_then(Value::as_str) == Some(id.as_str())) {
            return Err(LoreError(format!("{relative}: id {id} already exists")));
        }
        records.push(record);
        self.write(relative, &records)
    }
}

/// A path inside the store, never escaping it.
pub fn safe_relative(path: &Path) -> bool {
    !path.is_absolute() && path.components().all(|part| matches!(part, std::path::Component::Normal(_)))
}
