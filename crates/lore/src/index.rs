//! Disposable FTS5 index under the user cache, rebuilt from the JSONL. It is
//! never committed; losing it costs only a rebuild. Without embeddings search
//! still works and reports that vectors were not used.
use crate::search::Hit;
use crate::LoreError;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;

pub const FORMAT: &str = "1";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sync { pub changed: usize, pub removed: usize, pub rebuilt: bool }

pub struct Index {
    connection: Connection,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self, LoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| LoreError(format!("{}: {error}", path.display())))?;
        }
        let connection = Connection::open(path).map_err(|error| LoreError(format!("{}: {error}", path.display())))?;
        connection.execute_batch(
            r#"PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS records(
               id TEXT PRIMARY KEY, kind TEXT, area TEXT, title TEXT, body TEXT, refs TEXT);
             CREATE VIRTUAL TABLE IF NOT EXISTS words USING fts5(id UNINDEXED, title, body, refs, tokenize='porter');
             CREATE VIRTUAL TABLE IF NOT EXISTS terms USING fts5(id UNINDEXED, toks, tokenize="unicode61 tokenchars '_./:-'");
             PRAGMA user_version=1;"#,
        ).map_err(|error| LoreError(format!("index: {error}")))?;
        Ok(Self { connection })
    }

    /// Replaces the index with `records`; bounded by the caller's page.
    pub fn rebuild(&mut self, records: &[Value]) -> Result<(), LoreError> {
        let tx = self.connection.transaction().map_err(|error| LoreError(format!("index: {error}")))?;
        tx.execute("DELETE FROM records", []).ok();
        tx.execute("DELETE FROM words", []).ok();
        tx.execute("DELETE FROM terms", []).ok();
        for record in records {
            let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
            let kind = record.get("kind").and_then(Value::as_str).unwrap_or_default();
            let area = record.get("area").and_then(Value::as_str).unwrap_or_default();
            let title = record.get("title").and_then(Value::as_str).unwrap_or_default();
            let body = body(record);
            let refs = refs(record);
            tx.execute("INSERT OR REPLACE INTO records VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, kind, area, title, body, refs]).map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("INSERT INTO words(id,title,body,refs) VALUES (?1,?2,?3,?4)",
                params![id, title, body, refs]).map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("INSERT INTO terms(id,toks) VALUES (?1,?2)", params![id, terms_of(record)])
                .map_err(|error| LoreError(format!("index: {error}")))?;
        }
        tx.commit().map_err(|error| LoreError(format!("index: {error}")))
    }

    /// Incremental sync: only a record whose canonical bytes changed is
    /// re-indexed; a schema/version change rebuilds all. Returns what moved.
    pub fn sync(&mut self, records: &[Value]) -> Result<Sync, LoreError> {
        let tx = self.connection.transaction().map_err(|error| LoreError(format!("index: {error}")))?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS fingerprints(id TEXT PRIMARY KEY, hash TEXT);") 
            .map_err(|error| LoreError(format!("index: {error}")))?;
        let version: String = tx.query_row("SELECT value FROM meta WHERE key='version'", [], |row| row.get(0))
            .unwrap_or_default();
        let rebuild = version != FORMAT;
        if rebuild {
            tx.execute("DELETE FROM records", []).ok();
            tx.execute("DELETE FROM words", []).ok();
        tx.execute("DELETE FROM terms", []).ok();
            tx.execute("DELETE FROM fingerprints", []).ok();
        }
        let mut known = std::collections::BTreeMap::new();
        if !rebuild {
            let mut statement = tx.prepare("SELECT id,hash FROM fingerprints").map_err(|error| LoreError(format!("index: {error}")))?;
            let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(|error| LoreError(format!("index: {error}")))?;
            for row in rows.flatten() { known.insert(row.0, row.1); }
        }
        let mut current = std::collections::BTreeSet::new();
        let mut sync = Sync::default();
        for record in records {
            let id = record.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            if id.is_empty() { continue; }
            current.insert(id.clone());
            let hash = fingerprint(record);
            if known.get(&id) == Some(&hash) { continue; }
            let kind = record.get("kind").and_then(Value::as_str).unwrap_or_default();
            let area = record.get("area").and_then(Value::as_str).unwrap_or_default();
            let title = record.get("title").and_then(Value::as_str).unwrap_or_default();
            let body = body(record);
            let refs = refs(record);
            tx.execute("INSERT OR REPLACE INTO records VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, kind, area, title, body, refs]).map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("DELETE FROM words WHERE id=?1", params![id]).map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("INSERT INTO words(id,title,body,refs) VALUES (?1,?2,?3,?4)",
                params![id, title, body, refs]).map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("INSERT INTO terms(id,toks) VALUES (?1,?2)", params![id, terms_of(record)])
                .map_err(|error| LoreError(format!("index: {error}")))?;
            tx.execute("INSERT OR REPLACE INTO fingerprints(id,hash) VALUES (?1,?2)", params![id, hash])
                .map_err(|error| LoreError(format!("index: {error}")))?;
            sync.changed += 1;
        }
        for id in known.keys().filter(|id| !current.contains(*id)) {
            tx.execute("DELETE FROM records WHERE id=?1", params![id]).ok();
            tx.execute("DELETE FROM words WHERE id=?1", params![id]).ok();
            tx.execute("DELETE FROM terms WHERE id=?1", params![id]).ok();
            tx.execute("DELETE FROM fingerprints WHERE id=?1", params![id]).ok();
            sync.removed += 1;
        }
        if rebuild { sync.rebuilt = true; }
        tx.execute("CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT)", []).ok();
        tx.execute("INSERT OR REPLACE INTO meta(key,value) VALUES ('version',?1)", params![FORMAT]).ok();
        tx.commit().map_err(|error| LoreError(format!("index: {error}")))?;
        Ok(sync)
    }
    /// A quick check whether the index already holds exactly `ids`.
    pub fn matches_ids(&self, ids: &[String]) -> bool {
        let count: i64 = self.connection.query_row("SELECT count(*) FROM records", [], |row| row.get(0)).unwrap_or(-1);
        count == ids.len() as i64
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<(Vec<Hit>, bool), LoreError> {
        let phrase = fts_query(query);
        if phrase.is_empty() { return Ok((Vec::new(), false)); }
        let mut statement = self.connection.prepare(
            "SELECT records.id, records.title, bm25(words) FROM words JOIN records ON records.id=words.id
             WHERE words MATCH ?1 ORDER BY bm25(words) LIMIT ?2",
        ).map_err(|error| LoreError(format!("index: {error}")))?;
        let rows = statement.query_map(params![phrase, limit as i64], |row| {
            Ok(Hit { id: row.get(0)?, title: row.get(1)?, score: -(row.get::<_, f64>(2)? as f32) })
        }).map_err(|error| LoreError(format!("index: {error}")))?;
        Ok((rows.filter_map(Result::ok).collect(), false))
    }

    /// Combined text search: FTS5 words and identifier terms fused by RRF,
    /// plus a phrase bonus when the whole query appears. No vectors.
    pub fn search_all(&self, query: &str, limit: usize) -> Result<(Vec<Hit>, bool), LoreError> {
        let words = self.fts("words", &fts_query(query), limit)?;
        let terms = self.fts("terms", &terms_query(query), limit)?;
        let mut fused = rrf(&words, &terms, limit);
        let needle = query.to_lowercase();
        for hit in &mut fused {
            let text: Option<String> = self.connection.query_row(
                "SELECT records.title || ' ' || records.body FROM records WHERE id=?1",
                params![hit.id], |row| row.get(0)).ok();
            if text.is_some_and(|text| text.to_lowercase().contains(&needle)) { hit.score += 0.5; }
        }
        // Python's phrase rule: markup and quotes are ignored, whitespace is
        // one space. A record holding the whole query word for word jumps.
        let needle = flat(query);
        if needle.chars().count() >= 12 {
            let mut statement = self.connection.prepare("SELECT id,title,body FROM records")
                .map_err(|error| LoreError(format!("index: {error}")))?;
            let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
                .map_err(|error| LoreError(format!("index: {error}")))?;
            for row in rows.flatten() {
                if flat(&format!("{} {}", row.1, row.2)).contains(&needle) {
                    match fused.iter_mut().find(|hit| hit.id == row.0) {
                        Some(hit) => hit.score += 0.5,
                        None => fused.push(Hit { id: row.0, title: row.1, score: 0.5 }),
                    }
                }
            }
        }
        fused.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));
        fused.truncate(limit);
        Ok((fused, false))
    }

    fn fts(&self, table: &str, query: &str, limit: usize) -> Result<Vec<Hit>, LoreError> {
        if query.is_empty() { return Ok(Vec::new()); }
        let sql = format!("SELECT records.id, records.title, bm25({table}) FROM {table} JOIN records ON records.id={table}.id WHERE {table} MATCH ?1 ORDER BY bm25({table}) LIMIT ?2");
        let mut statement = self.connection.prepare(&sql).map_err(|error| LoreError(format!("index: {error}")))?;
        let rows = statement.query_map(params![query, limit as i64], |row| {
            Ok(Hit { id: row.get(0)?, title: row.get(1)?, score: -(row.get::<_, f64>(2)? as f32) })
        }).map_err(|error| LoreError(format!("index: {error}")))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
    /// Records whose refs name `file`, for `lore for <file>`.
    pub fn for_file(&self, file: &str, limit: usize) -> Result<Vec<Hit>, LoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id,title FROM records WHERE refs LIKE ?1 ORDER BY id LIMIT ?2",
        ).map_err(|error| LoreError(format!("index: {error}")))?;
        let rows = statement.query_map(params![format!("%{file}%"), limit as i64], |row| {
            Ok(Hit { id: row.get(0)?, title: row.get(1)?, score: 1.0 })
        }).map_err(|error| LoreError(format!("index: {error}")))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
}

/// Reciprocal rank fusion of two ranked lists (k = 60), deterministic by id.
pub fn rrf(a: &[Hit], b: &[Hit], limit: usize) -> Vec<Hit> {
    let mut scores: std::collections::BTreeMap<String, (f32, String)> = Default::default();
    for list in [a, b] {
        for (rank, hit) in list.iter().enumerate() {
            let entry = scores.entry(hit.id.clone()).or_insert((0.0, hit.title.clone()));
            entry.0 += 1.0 / (60.0 + rank as f32 + 1.0);
        }
    }
    let mut out: Vec<Hit> = scores.into_iter().map(|(id, (score, title))| Hit { id, title, score }).collect();
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));
    out.truncate(limit);
    out
}

/// A safe FTS5 query: every word is a quoted phrase, so punctuation in an
/// error string is a literal, never syntax.
fn terms_query(query: &str) -> String {
    query.split(|c: char| !(c.is_alphanumeric() || "_./:-".contains(c)))
        .filter(|token| token.chars().count() >= 3)
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>().join(" OR ")
}
fn fts_query(query: &str) -> String {
    query.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect::<Vec<_>>().join(" OR ")
}

fn fingerprint(record: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    crate::store::canonical(record).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn terms_of(record: &Value) -> String {
    let mut out = String::new();
    if let Some(map) = record.as_object() {
        for key in ["title", "signals", "symptom", "cause", "fix", "check", "question", "choice", "reason", "what", "body", "steps"] {
            if let Some(value) = map.get(key) {
                for text in strings(value) {
                    for token in text.split(|c: char| !(c.is_alphanumeric() || "_./:-".contains(c))) {
                        if token.chars().count() >= 3 { out.push_str(token); out.push('\n'); }
                    }
                }
            }
        }
    }
    out
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(strings).collect(),
        _ => Vec::new(),
    }
}
fn flat(text: &str) -> String {
    text.to_lowercase().replace(['`', '*', '«', '»', '"', '\''], "").split_whitespace().collect::<Vec<_>>().join(" ")
}
fn body(record: &Value) -> String {
    let mut text = String::new();
    if let Some(map) = record.as_object() {
        for (key, value) in map {
            if matches!(key.as_str(), "id" | "kind" | "area" | "title" | "refs" | "v" | "created" | "updated" | "by") { continue; }
            match value {
                Value::String(value) => { text.push_str(value); text.push('\n'); }
                Value::Array(values) => for value in values {
                    if let Some(value) = value.as_str() { text.push_str(value); text.push('\n'); }
                },
                _ => {}
            }
        }
    }
    text
}

fn refs(record: &Value) -> String {
    record.get("refs").and_then(|refs| refs.get("files")).and_then(Value::as_array)
        .map(|files| files.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp() -> (tempfile::TempDir, Index) {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(&dir.path().join("index.sqlite")).unwrap();
        (dir, index)
    }

    #[test]
    fn exact_errors_and_files_are_found_without_embeddings() {
        let (_dir, mut index) = temp();
        index.rebuild(&[
            json!({"id":"a","kind":"fault","area":"git","title":"Tar pipe","symptom":"tar: not found","refs":{"files":["src/tar.rs"]}}),
            json!({"id":"b","kind":"decision","area":"build","title":"Use tar","choice":"tar xf"}),
        ]).unwrap();
        let (hits, used_vectors) = index.search("tar: not found", 10).unwrap();
        assert_eq!(hits[0].id, "a");
        assert!(!used_vectors, "search says vectors were not used");
        assert_eq!(index.for_file("src/tar.rs", 10).unwrap()[0].id, "a");
        let (hits, _) = index.search("tar", 10).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn rrf_prefers_a_record_ranked_by_both_lists() {
        let a = vec![Hit { id: "a".into(), title: "A".into(), score: 1.0 }, Hit { id: "b".into(), title: "B".into(), score: 0.5 }];
        let b = vec![Hit { id: "b".into(), title: "B".into(), score: 1.0 }, Hit { id: "c".into(), title: "C".into(), score: 0.2 }];
        let fused = rrf(&a, &b, 10);
        assert_eq!(fused[0].id, "b");
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn sync_reindexes_only_changed_records() {
        let (_dir, mut index) = temp();
        let one = json!({"id":"a","kind":"fault","area":"git","title":"A","symptom":"x"});
        let two = json!({"id":"b","kind":"fault","area":"git","title":"B","symptom":"y"});
        let first = index.sync(&[one.clone(), two.clone()]).unwrap();
        assert_eq!((first.changed, first.removed), (2, 0));
        assert!(first.rebuilt, "the first sync builds the index");
        let same = index.sync(&[one.clone(), two.clone()]).unwrap();
        assert_eq!((same.changed, same.removed), (0, 0));
        let mut changed = one.clone();
        changed["symptom"] = json!("changed");
        let changed = index.sync(&[changed, two]).unwrap();
        assert_eq!((changed.changed, changed.removed), (1, 0));
        let removed = index.sync(&[one]).unwrap();
        assert_eq!((removed.changed, removed.removed), (1, 1)); // restore one, remove b
    }
}











