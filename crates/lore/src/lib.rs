//! RustCodeLore: the record kinds of FORMAT.md v3, strict validation and the
//! canonical JSONL store. This is the library half of the port; the CLI and
//! search index come later.
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub mod embed;
pub mod schema;
pub mod search;
pub mod store;
pub mod writer;

pub use schema::{validate, RecordError};
pub use store::{canonical, parse, serialize, Store};
pub use writer::{Duplicates, Writer};

pub const FORMAT_VERSION: u64 = 3;

/// A refusal from the store or the writer: the message names the file and line.
#[derive(Debug)]
pub struct LoreError(pub String);

impl std::fmt::Display for LoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}
impl std::error::Error for LoreError {}

pub type Result<T> = std::result::Result<T, LoreError>;

/// Reads `lore/<area>.jsonl` (or a plan/source file) under `root`.
pub fn read_file(root: &Path, relative: &str) -> Result<Vec<Value>> {
    let bytes = std::fs::read(root.join(relative)).map_err(|error| LoreError(format!("{relative}: {error}")))?;
    parse(&bytes, relative)
}

/// The kinds, statuses and field ceilings of FORMAT.md v3.
pub fn kinds() -> &'static [&'static str] { &["fault", "reading", "decision", "topic", "recipe", "plan", "item", "source"] }

/// A shared duplicate signal: lower-case words of the story fields.
pub fn signals(record: &Value) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    for key in ["title", "symptom", "cause", "question", "choice", "what", "body", "goal", "description"] {
        if let Some(text) = record.get(key).and_then(Value::as_str) {
            for word in text.to_lowercase().split(|ch: char| !ch.is_alphanumeric()) {
                if word.len() >= 4 { words.insert(word.to_owned()); }
            }
        }
    }
    words
}

/// Jaccard similarity of two records' signals.
pub fn similarity(a: &Value, b: &Value) -> f64 {
    let (a, b) = (signals(a), signals(b));
    if a.is_empty() || b.is_empty() { return 0.0; }
    let shared = a.intersection(&b).count() as f64;
    shared / (a.len() + b.len() - shared as usize) as f64
}

/// The map a record was parsed into, kept for sorting and comparisons.
pub fn object(record: &Value) -> &Map<String, Value> {
    record.as_object().unwrap_or_else(|| {
        static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(Map::new)
    })
}

