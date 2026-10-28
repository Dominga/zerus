//! The device's attention marks and read replies (`m_attentionMarks` and
//! `m_readReplies` in `tray/src/FleetState.cpp`), kept in a file so a restart
//! keeps them. The frontend owns their rules; this keeps what it writes.
//!
//! Qt keeps both maps without a bound. Here each holds at most [`CAP`]
//! entries, the oldest written dropped first, and the file lists entries in
//! write order so that order survives a restart.
//!
//! [`AttentionMarks::update`] changes memory only; the app saves on a writer
//! thread from [`AttentionMarks::contents`], so no caller waits for the disk.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use crate::readmarks::write_atomically;

/// At most this many entries per map (owner rule on bounded caches).
pub const CAP: usize = 20_000;

/// One session's attention mark: marked for review, and the attention
/// fingerprint the person acknowledged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    #[serde(default)]
    pub review: bool,
    #[serde(default)]
    pub acknowledged: String,
}

pub struct AttentionMarks {
    path: PathBuf,
    marks: Written<Mark>,
    replies: Written<String>,
}

impl AttentionMarks {
    /// A missing or unreadable file gives no marks. A file over the cap keeps
    /// its newest entries.
    pub fn load(path: PathBuf) -> Self {
        let file: File = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut marks = Written::default();
        for (key, mark) in file.marks {
            marks.put(key, mark);
        }
        let mut replies = Written::default();
        for (key, reply) in file.replies {
            replies.put(key, reply);
        }
        marks.cap(CAP);
        replies.cap(CAP);
        Self {
            path,
            marks,
            replies,
        }
    }

    /// The marks and read replies by key.
    pub fn snapshot(&self) -> (BTreeMap<String, Mark>, BTreeMap<String, String>) {
        (self.marks.snapshot(), self.replies.snapshot())
    }

    /// The keys of the marks and of the read replies, oldest written first,
    /// so a reader can drop the same entries the cap drops here.
    pub fn order(&self) -> (Vec<String>, Vec<String>) {
        (self.marks.keys_in_order(), self.replies.keys_in_order())
    }

    /// `Some` writes the entry (it becomes the newest written) unless it holds
    /// that value already, `None` removes it. Then each map keeps its [`CAP`]
    /// newest entries. Memory only: returns true when anything changed, so
    /// the caller saves.
    pub fn update(
        &mut self,
        marks: BTreeMap<String, Option<Mark>>,
        replies: BTreeMap<String, Option<String>>,
    ) -> bool {
        let changed = self.marks.apply(marks) | self.replies.apply(replies);
        if changed {
            self.marks.cap(CAP);
            self.replies.cap(CAP);
        }
        changed
    }

    /// A copy of what the file holds, to write without holding these marks.
    pub fn contents(&self) -> Contents {
        Contents {
            path: self.path.clone(),
            file: File {
                marks: self.marks.in_order(),
                replies: self.replies.in_order(),
            },
        }
    }

    /// Replaces the file atomically with the current marks.
    pub fn save(&self) -> io::Result<()> {
        self.contents().write()
    }
}

/// The marks as [`AttentionMarks::contents`] copied them.
pub struct Contents {
    path: PathBuf,
    file: File,
}

impl Contents {
    /// Replaces the file atomically (a temporary file beside it, then a
    /// rename), creating its folder.
    pub fn write(&self) -> io::Result<()> {
        write_atomically(&self.path, &serde_json::to_vec(&self.file)?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The file: each map as `[key, value]` pairs, oldest written first.
#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    marks: Vec<(String, Mark)>,
    #[serde(default)]
    replies: Vec<(String, String)>,
}

/// A map that remembers the order its keys were last written in.
struct Written<V> {
    entries: HashMap<String, (u64, V)>,
    order: BTreeMap<u64, String>,
    next: u64,
}

impl<V> Default for Written<V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            next: 0,
        }
    }
}

impl<V: Clone + PartialEq> Written<V> {
    /// Writes or removes each entry; true when it changed any. Writing the
    /// value an entry holds is no change and keeps its place.
    fn apply(&mut self, changes: BTreeMap<String, Option<V>>) -> bool {
        let mut changed = false;
        for (key, value) in changes {
            changed |= match value {
                Some(value) if self.entries.get(&key).is_some_and(|(_, old)| *old == value) => {
                    false
                }
                Some(value) => {
                    self.put(key, value);
                    true
                }
                None => self.remove(&key),
            };
        }
        changed
    }

    /// The entry becomes the newest written.
    fn put(&mut self, key: String, value: V) {
        if let Some((written, _)) = self.entries.remove(&key) {
            self.order.remove(&written);
        }
        self.order.insert(self.next, key.clone());
        self.entries.insert(key, (self.next, value));
        self.next += 1;
    }

    fn remove(&mut self, key: &str) -> bool {
        let Some((written, _)) = self.entries.remove(key) else {
            return false;
        };
        self.order.remove(&written);
        true
    }

    /// Drops the oldest written entries beyond `cap`.
    fn cap(&mut self, cap: usize) {
        while self.entries.len() > cap {
            let Some((_, key)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&key);
        }
    }

    fn snapshot(&self) -> BTreeMap<String, V> {
        self.entries
            .iter()
            .map(|(key, (_, value))| (key.clone(), value.clone()))
            .collect()
    }

    /// The keys, oldest written first.
    fn keys_in_order(&self) -> Vec<String> {
        self.order.values().cloned().collect()
    }

    /// A copy, oldest written first.
    fn in_order(&self) -> Vec<(String, V)> {
        self.order
            .values()
            .filter_map(|key| {
                self.entries
                    .get(key)
                    .map(|(_, value)| (key.clone(), value.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "zerus-attention-marks-{}-{test}.json",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Temp(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn mark(review: bool, acknowledged: &str) -> Mark {
        Mark {
            review,
            acknowledged: acknowledged.into(),
        }
    }

    fn marks(entries: &[(&str, Option<Mark>)]) -> BTreeMap<String, Option<Mark>> {
        entries
            .iter()
            .map(|(key, mark)| ((*key).to_owned(), mark.clone()))
            .collect()
    }

    fn replies(entries: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
        entries
            .iter()
            .map(|(key, reply)| ((*key).to_owned(), reply.map(Into::into)))
            .collect()
    }

    const KEY: &str = r#"["mac","claude","conv-1"]"#;
    const FALLBACK: &str = r#"["mac","claude","claude/sample-project/review",1700000000]"#;

    #[test]
    fn an_update_writes_and_removes_and_survives_a_restart() {
        let temp = Temp::new("update");
        let mut store = AttentionMarks::load(temp.0.clone());
        assert_eq!(store.snapshot(), (BTreeMap::new(), BTreeMap::new()));
        assert!(store.update(
            marks(&[
                (KEY, Some(mark(true, ""))),
                (FALLBACK, Some(mark(false, "fp"))),
            ]),
            replies(&[(KEY, Some("r1"))]),
        ));
        assert!(store.update(marks(&[(FALLBACK, None)]), BTreeMap::new()));
        let expected = (
            BTreeMap::from([(KEY.to_owned(), mark(true, ""))]),
            BTreeMap::from([(KEY.to_owned(), "r1".to_owned())]),
        );
        assert_eq!(store.snapshot(), expected);
        // An update only changes memory; the caller saves.
        assert!(!temp.0.exists());
        store.save().unwrap();
        assert_eq!(AttentionMarks::load(temp.0.clone()).snapshot(), expected);
        // Removing a key that is not there, writing the value already stored,
        // or an empty update, changes nothing.
        assert!(!store.update(marks(&[("missing", None)]), replies(&[("missing", None)])));
        assert!(!store.update(
            marks(&[(KEY, Some(mark(true, "")))]),
            replies(&[(KEY, Some("r1"))])
        ));
        assert!(!store.update(BTreeMap::new(), BTreeMap::new()));
        assert!(store.update(BTreeMap::new(), replies(&[(KEY, Some("r2"))])));
        store.contents().write().unwrap();
        assert_eq!(
            AttentionMarks::load(temp.0.clone()).snapshot().1,
            BTreeMap::from([(KEY.to_owned(), "r2".to_owned())])
        );
    }

    #[test]
    fn a_mark_without_fields_reads_as_the_default() {
        let temp = Temp::new("defaults");
        std::fs::write(
            &temp.0,
            format!(r#"{{"marks":[[{KEY:?},{{}}],[{FALLBACK:?},{{"review":true}}]]}}"#),
        )
        .unwrap();
        let (marks, replies) = AttentionMarks::load(temp.0.clone()).snapshot();
        assert_eq!(marks[KEY], Mark::default());
        assert_eq!(marks[FALLBACK], mark(true, ""));
        assert!(replies.is_empty());
    }

    #[test]
    fn the_file_lists_entries_in_write_order() {
        let temp = Temp::new("order");
        let mut store = AttentionMarks::load(temp.0.clone());
        store.update(
            marks(&[("b", Some(mark(true, "")))]),
            replies(&[("y", Some("r1"))]),
        );
        store.update(
            marks(&[("a", Some(mark(false, "fp")))]),
            replies(&[("x", Some("r2"))]),
        );
        // Writing an old key again makes it the newest; the same value again
        // is no change and keeps its place.
        store.update(marks(&[("b", Some(mark(false, "")))]), BTreeMap::new());
        store.update(BTreeMap::new(), replies(&[("y", Some("r1"))]));
        store.save().unwrap();
        let file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&temp.0).unwrap()).unwrap();
        assert_eq!(
            file,
            serde_json::json!({
                "marks": [["a", {"review": false, "acknowledged": "fp"}],
                          ["b", {"review": false, "acknowledged": ""}]],
                "replies": [["y", "r1"], ["x", "r2"]],
            })
        );
    }

    #[test]
    fn the_order_lists_keys_oldest_written_first_and_survives_a_restart() {
        let temp = Temp::new("keys");
        let mut store = AttentionMarks::load(temp.0.clone());
        store.update(
            marks(&[("z", Some(mark(true, "")))]),
            replies(&[("z", Some("r1"))]),
        );
        // One update applies its entries in key order.
        store.update(
            marks(&[("m", Some(mark(true, ""))), ("a", Some(mark(true, "")))]),
            replies(&[("a", Some("r2"))]),
        );
        // A rewrite moves its key to the newest; the same value keeps its place.
        store.update(
            marks(&[("z", Some(mark(false, "fp")))]),
            replies(&[("z", Some("r1"))]),
        );
        let expected = (
            vec!["a".to_owned(), "m".to_owned(), "z".to_owned()],
            vec!["z".to_owned(), "a".to_owned()],
        );
        assert_eq!(store.order(), expected);
        store.save().unwrap();
        assert_eq!(AttentionMarks::load(temp.0.clone()).order(), expected);
    }

    #[test]
    fn the_cap_drops_the_oldest_written() {
        let temp = Temp::new("cap");
        let mut store = AttentionMarks::load(temp.0.clone());
        store.update(
            marks(&[("old-a", Some(mark(true, "")))]),
            replies(&[("old-a", Some("r"))]),
        );
        store.update(
            marks(&[("old-b", Some(mark(true, "")))]),
            replies(&[("old-b", Some("r"))]),
        );
        // Rewritten, old-a is newer than old-b now.
        store.update(
            marks(&[("old-a", Some(mark(false, "fp")))]),
            replies(&[("old-a", Some("r2"))]),
        );
        let bulk = |prefix: &str, count: usize| -> Vec<String> {
            (0..count).map(|i| format!("{prefix}-{i:05}")).collect()
        };
        let first = bulk("bulk", CAP - 1);
        store.update(
            first
                .iter()
                .map(|key| (key.clone(), Some(mark(true, ""))))
                .collect(),
            first
                .iter()
                .map(|key| (key.clone(), Some("r".to_owned())))
                .collect(),
        );
        let (marks_now, replies_now) = store.snapshot();
        assert_eq!(marks_now.len(), CAP);
        assert_eq!(replies_now.len(), CAP);
        assert!(!marks_now.contains_key("old-b") && !replies_now.contains_key("old-b"));
        assert_eq!(marks_now["old-a"], mark(false, "fp"));
        assert_eq!(replies_now["old-a"], "r2");
        store.save().unwrap();
        // The order survives a restart: old-a goes next, then the bulk in key order.
        let mut reloaded = AttentionMarks::load(temp.0.clone());
        reloaded.update(marks(&[("new-1", Some(mark(true, "")))]), BTreeMap::new());
        let (marks_now, replies_now) = reloaded.snapshot();
        assert_eq!(marks_now.len(), CAP);
        assert!(!marks_now.contains_key("old-a"));
        assert!(marks_now.contains_key("new-1") && marks_now.contains_key(&first[0]));
        assert_eq!(replies_now.len(), CAP);
        reloaded.update(marks(&[("new-2", Some(mark(true, "")))]), BTreeMap::new());
        assert!(!reloaded.snapshot().0.contains_key(&first[0]));
        reloaded.save().unwrap();
        assert_eq!(AttentionMarks::load(temp.0.clone()).snapshot().0.len(), CAP);
    }

    #[test]
    fn a_file_over_the_cap_loads_its_newest_entries() {
        let temp = Temp::new("over");
        let entries: Vec<(String, String)> = (0..CAP + 2)
            .map(|i| (format!("k{i:05}"), "r".to_owned()))
            .collect();
        std::fs::write(
            &temp.0,
            serde_json::to_vec(&serde_json::json!({ "replies": entries })).unwrap(),
        )
        .unwrap();
        let replies = AttentionMarks::load(temp.0.clone()).snapshot().1;
        assert_eq!(replies.len(), CAP);
        assert!(!replies.contains_key("k00000") && !replies.contains_key("k00001"));
        assert!(replies.contains_key("k00002"));
    }

    #[test]
    fn a_corrupt_or_missing_file_loads_empty() {
        let temp = Temp::new("corrupt");
        for garbage in [
            "not json",
            r#"{"marks":{"a":1}}"#,
            r#"{"marks":[["a"]]}"#,
            "[]",
        ] {
            std::fs::write(&temp.0, garbage).unwrap();
            assert_eq!(
                AttentionMarks::load(temp.0.clone()).snapshot(),
                (BTreeMap::new(), BTreeMap::new()),
                "{garbage}"
            );
        }
        let missing = temp.0.with_extension("missing");
        assert_eq!(
            AttentionMarks::load(missing).snapshot(),
            (BTreeMap::new(), BTreeMap::new())
        );
    }

    #[test]
    fn a_failed_save_reports_its_error() {
        let temp = Temp::new("blocked");
        // A file where the folder should be.
        std::fs::write(&temp.0, "x").unwrap();
        let mut store = AttentionMarks::load(temp.0.join("attention-marks.json"));
        store.update(marks(&[(KEY, Some(mark(true, "")))]), BTreeMap::new());
        assert!(store.save().is_err());
    }

    #[test]
    fn a_save_leaves_no_temporary_file() {
        let folder = std::env::temp_dir().join(format!(
            "zerus-attention-marks-{}-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join("attention-marks.json");
        // The folder is created on the first save.
        let mut store = AttentionMarks::load(path.clone());
        store.update(marks(&[(KEY, Some(mark(true, "")))]), BTreeMap::new());
        store.save().unwrap();
        let names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["attention-marks.json"]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
