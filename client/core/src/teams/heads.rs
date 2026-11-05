//! The `team` topic of a computer's watch stream (task 8): every team's head
//! as keyed rows (`team:<id>` with its `revision`). [`TeamHeads`] keeps the
//! rows a stream sent and gives the revisions by team once they are
//! complete; the Teams poller then reads a team again only when its revision
//! moved ([`crate::TeamsModel::heads`]). Until a snapshot is complete, after
//! a `gap` or a failure, and on a stream without the topic, there are no
//! heads and the poller polls as before.
use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use zerus_watch_protocol::{Delta, Snapshot};

/// A team's revision by its id.
pub type Heads = BTreeMap<String, i64>;

/// The team rows of one stream.
#[derive(Debug, Default)]
pub struct TeamHeads {
    rows: Option<BTreeMap<String, Value>>,
    staged: Option<BTreeMap<String, Value>>,
}

impl TeamHeads {
    /// A snapshot page; the heads once its last page arrived.
    pub fn snapshot(&mut self, snapshot: &Snapshot) -> Option<Arc<Heads>> {
        if snapshot.page == 0 {
            self.staged = Some(BTreeMap::new());
        }
        // A page without its first is a broken snapshot: wait for the next.
        let staged = self.staged.as_mut()?;
        for item in snapshot.items.iter().flatten() {
            if let Some(key) = item["key"].as_str() {
                staged.insert(key.to_owned(), item["row"].clone());
            }
        }
        if !snapshot.last {
            return None;
        }
        self.rows = self.staged.take();
        self.heads()
    }

    /// A delta; the heads when a snapshot came before it.
    pub fn delta(&mut self, delta: &Delta) -> Option<Arc<Heads>> {
        let rows = self.rows.as_mut()?;
        for removal in &delta.remove {
            rows.remove(&removal.key);
        }
        for item in &delta.upsert {
            rows.insert(item.key.clone(), item.row.clone());
        }
        self.heads()
    }

    /// A gap, a failure or a new stream: the rows are unknown until the next
    /// complete snapshot.
    pub fn reset(&mut self) {
        self.rows = None;
        self.staged = None;
    }

    /// The revisions by team id; None while the rows are unknown.
    pub fn heads(&self) -> Option<Arc<Heads>> {
        let rows = self.rows.as_ref()?;
        Some(Arc::new(
            rows.iter()
                .filter_map(|(key, row)| {
                    Some((
                        key.strip_prefix("team:")?.to_owned(),
                        row["revision"].as_i64()?,
                    ))
                })
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zerus_watch_protocol::{CatalogItem, Removal};

    fn page(page: u32, last: bool, items: Vec<Value>) -> Snapshot {
        Snapshot {
            topic: "team".into(),
            key: None,
            items: Some(items),
            value: None,
            page,
            last,
            cursor: 1,
        }
    }

    fn row(id: &str, revision: i64) -> Value {
        json!({"key": format!("team:{id}"), "row": {"id": id, "revision": revision, "status": "running"}})
    }

    #[test]
    fn a_paged_snapshot_gives_heads_once_complete_then_deltas_change_them() {
        let mut heads = TeamHeads::default();
        assert_eq!(heads.heads(), None);
        assert_eq!(
            heads.snapshot(&page(0, false, vec![row("a", 3)])),
            None,
            "not complete yet"
        );
        let first = heads
            .snapshot(&page(
                1,
                true,
                vec![
                    row("b", 1),
                    json!({"key":"teams","row":{"count":2,"truncated":false}}),
                ],
            ))
            .unwrap();
        assert_eq!(*first, Heads::from([("a".into(), 3), ("b".into(), 1)]));
        let delta = Delta {
            topic: "team".into(),
            upsert: vec![CatalogItem {
                key: "team:a".into(),
                row: json!({"id":"a","revision":4}),
            }],
            remove: vec![Removal {
                key: "team:b".into(),
                reason: None,
            }],
            cursor: 2,
        };
        assert_eq!(
            *heads.delta(&delta).unwrap(),
            Heads::from([("a".into(), 4)])
        );
    }

    #[test]
    fn a_gap_or_a_broken_snapshot_leaves_no_heads_until_the_next_snapshot() {
        let mut heads = TeamHeads::default();
        heads.snapshot(&page(0, true, vec![row("a", 1)]));
        heads.reset();
        assert_eq!(heads.heads(), None);
        let delta = Delta {
            topic: "team".into(),
            upsert: vec![],
            remove: vec![],
            cursor: 3,
        };
        assert_eq!(heads.delta(&delta), None, "a delta needs a snapshot first");
        assert_eq!(
            heads.snapshot(&page(1, true, vec![row("a", 2)])),
            None,
            "a page without its first"
        );
        assert_eq!(
            *heads.snapshot(&page(0, true, vec![row("a", 2)])).unwrap(),
            Heads::from([("a".into(), 2)])
        );
    }
}
