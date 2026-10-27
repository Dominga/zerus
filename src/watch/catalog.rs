//! Catalog rows: keys, change detection with volatile observation fields
//! masked, removal reasons and paging.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use zerus_watch_protocol::{self as protocol, reason};

/// Observation timestamps that change on every listing without any change in
/// the session. Change detection ignores them; the rows still carry them.
const VOLATILE_ROW: [&[&str]; 2] = [&["goal_observed_at"], &["process_work", "sampled_at"]];
/// Meta fields that change on every listing; sent at most every 10 s.
pub(super) const VOLATILE_META: [&[&str]; 2] = [&["metrics"], &["desktop_presence"]];

pub(super) type Digest32 = [u8; 32];

/// `value` without the fields at `paths`.
pub(super) fn masked(value: &Value, paths: &[&[&str]]) -> Value {
    let mut value = value.clone();
    for path in paths {
        let Some((last, parents)) = path.split_last() else { continue };
        let mut node = Some(&mut value);
        for parent in parents {
            node = node.and_then(|node| node.get_mut(*parent));
        }
        if let Some(object) = node.and_then(Value::as_object_mut) {
            object.remove(*last);
        }
    }
    value
}

pub(super) fn digest(value: &Value) -> Digest32 {
    Sha256::digest(value.to_string().as_bytes()).into()
}

/// A row's digest with its volatile fields masked.
pub(super) fn row_digest(row: &Value) -> Digest32 {
    digest(&masked(row, &VOLATILE_ROW))
}

/// A row's key: its run, or its name for a row without one.
pub(super) fn key(row: &Value) -> String {
    protocol::catalog_key(row).unwrap_or_else(|| format!("name:{}", row.get("name").and_then(Value::as_str).unwrap_or("")))
}

/// The listing's rows with their keys, and its other fields. Two rows with the
/// same key (never expected) keep apart: the later one falls back to its name,
/// then to its position.
pub(super) fn split(mut listing: Value) -> (Vec<(String, Value)>, Value) {
    let sessions = listing
        .as_object_mut()
        .and_then(|object| object.remove("sessions"))
        .and_then(|sessions| match sessions {
            Value::Array(rows) => Some(rows),
            _ => None,
        })
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(sessions.len());
    for (index, row) in sessions.into_iter().enumerate() {
        let name = row.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
        let mut key = key(&row);
        if seen.contains(&key) {
            key = protocol::key_for(None, &name);
        }
        if seen.contains(&key) {
            key = format!("name:{name}#{index}");
        }
        seen.insert(key.clone());
        rows.push((key, row));
    }
    (rows, listing)
}

/// What the client holds of one row.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sent {
    pub name: String,
    pub run_id: String,
    pub digest: Digest32,
}

impl Sent {
    pub(super) fn of(row: &Value, digest: Digest32) -> Self {
        let text = |key| row.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        Self { name: text("name"), run_id: text("run_id"), digest }
    }
}

/// Rows to upsert (indices into the current rows) and keys to remove.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Change {
    pub upsert: Vec<usize>,
    pub remove: Vec<String>,
}

impl Change {
    pub(super) fn is_empty(&self) -> bool {
        self.upsert.is_empty() && self.remove.is_empty()
    }
}

pub(super) fn diff(sent: &BTreeMap<String, Sent>, rows: &[(String, Value)], digests: &[Digest32]) -> Change {
    let mut change = Change::default();
    for (index, ((key, _), digest)) in rows.iter().zip(digests).enumerate() {
        if sent.get(key).is_none_or(|old| old.digest != *digest) {
            change.upsert.push(index);
        }
    }
    let current: BTreeSet<&str> = rows.iter().map(|(key, _)| key.as_str()).collect();
    change.remove = sent.keys().filter(|key| !current.contains(key.as_str())).cloned().collect();
    change
}

/// Why `old` left the catalog: its name lists under another run (replaced),
/// its run is archived, its binding is gone (forgotten), or it ended.
pub(super) fn removal_reason(
    old: &Sent,
    listed_names: &BTreeSet<&str>,
    archived: impl Fn(&str) -> bool,
    saved: impl Fn(&str) -> bool,
) -> &'static str {
    if listed_names.contains(old.name.as_str()) {
        return reason::REPLACED;
    }
    if old.run_id.is_empty() {
        return reason::ENDED;
    }
    if archived(&old.run_id) {
        return reason::ARCHIVED;
    }
    if !saved(&old.name) {
        return reason::FORGOTTEN;
    }
    reason::ENDED
}

/// Splits `items` into pages of at most `limit` bytes by `size`. A single item
/// larger than `limit` gets a page of its own.
pub(super) fn pages<T>(items: Vec<T>, size: impl Fn(&T) -> usize, limit: usize) -> Vec<Vec<T>> {
    let mut pages = Vec::new();
    let mut page = Vec::new();
    let mut bytes = 0;
    for item in items {
        let item_bytes = size(&item) + 1;
        if !page.is_empty() && bytes + item_bytes > limit {
            pages.push(std::mem::take(&mut page));
            bytes = 0;
        }
        bytes += item_bytes;
        page.push(item);
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(name: &str, run: &str, phase: &str, observed: f64) -> Value {
        json!({"name":name,"run_id":run,"phase":phase,"goal_observed_at":observed,
            "process_work":{"count":0,"sampled_at":observed}})
    }

    fn sent_of(rows: &[(String, Value)]) -> BTreeMap<String, Sent> {
        rows.iter().map(|(key, row)| (key.clone(), Sent::of(row, row_digest(row)))).collect()
    }

    #[test]
    fn observation_timestamps_alone_change_nothing() {
        let a = row("codex/p/t", "r1", "idle", 1.0);
        let b = row("codex/p/t", "r1", "idle", 2.0);
        assert_ne!(a, b);
        assert_eq!(row_digest(&a), row_digest(&b));
        assert_ne!(row_digest(&a), row_digest(&row("codex/p/t", "r1", "working", 1.0)));
        // The rows themselves keep the fields.
        assert_eq!(masked(&a, &VOLATILE_ROW)["process_work"], json!({"count":0}));
        assert_eq!(a["process_work"]["sampled_at"], 1.0);
        assert_eq!(masked(&json!({"process_work":null}), &VOLATILE_ROW), json!({"process_work":null}));
    }

    #[test]
    fn rows_key_by_run_then_name_and_duplicates_stay_apart() {
        let listing = json!({"host":"h","ok":true,"sessions":[
            {"name":"codex/p/a","run_id":"r1"},{"name":"sh/p"},{"name":"codex/p/b","run_id":"r1"},{"name":"sh/p"}]});
        let (rows, meta) = split(listing);
        let keys: Vec<_> = rows.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["run:r1", "name:sh/p", "name:codex/p/b", "name:sh/p#3"]);
        assert_eq!(meta, json!({"host":"h","ok":true}));
        assert_eq!(split(json!({"ok":false})).0, vec![]);
    }

    #[test]
    fn diff_upserts_changed_rows_and_removes_missing_keys() {
        let (before, _) = split(json!({"sessions":[row("codex/p/a","r1","idle",1.), row("codex/p/b","r2","idle",1.), row("codex/p/c","r3","idle",1.)]}));
        let sent = sent_of(&before);
        let (after, _) = split(json!({"sessions":[row("codex/p/a","r1","idle",9.), row("codex/p/renamed","r2","idle",9.), row("codex/p/d","r4","idle",9.)]}));
        let digests: Vec<_> = after.iter().map(|(_, row)| row_digest(row)).collect();
        let change = diff(&sent, &after, &digests);
        // r1 changed only its timestamps; a rename upserts the same run key.
        assert_eq!(change.upsert, [1, 2]);
        assert_eq!(after[1].0, "run:r2");
        assert_eq!(change.remove, ["run:r3"]);
        assert!(diff(&sent_of(&after), &after, &digests).is_empty());
    }

    #[test]
    fn removals_explain_replacement_archive_forgetting_and_ending() {
        let old = Sent { name: "codex/p/a".into(), run_id: "r1".into(), digest: [0; 32] };
        let none = BTreeSet::new();
        let listed: BTreeSet<&str> = ["codex/p/a"].into();
        assert_eq!(removal_reason(&old, &listed, |_| true, |_| true), reason::REPLACED);
        assert_eq!(removal_reason(&old, &none, |run| run == "r1", |_| true), reason::ARCHIVED);
        assert_eq!(removal_reason(&old, &none, |_| false, |_| false), reason::FORGOTTEN);
        assert_eq!(removal_reason(&old, &none, |_| false, |_| true), reason::ENDED);
        let untracked = Sent { name: "sh/p".into(), run_id: String::new(), digest: [0; 32] };
        assert_eq!(removal_reason(&untracked, &none, |_| true, |_| false), reason::ENDED);
    }

    #[test]
    fn pages_respect_the_byte_limit_and_isolate_oversized_items() {
        let pages = pages(vec![4, 4, 4, 20, 4], |n| *n, 10);
        assert_eq!(pages, vec![vec![4, 4], vec![4], vec![20], vec![4]]);
        let empty: Vec<Vec<usize>> = super::pages(Vec::new(), |n: &usize| *n, 10);
        assert_eq!(empty, vec![Vec::<usize>::new()]);
    }
}
