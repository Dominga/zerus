//! Read marks the host's snapshot does not show yet. Kept in a file so a
//! restart keeps retrying (`tray/src/ReadMarks.h`).
use crate::view::ViewRow;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadMark {
    pub computer: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub reply_id: Option<String>,
    pub attention_signature: Option<String>,
}

impl ReadMark {
    /// Marks neither a reply nor an attention signature (`ReadMark::isEmpty`).
    pub fn is_empty(&self) -> bool {
        self.reply_id.as_deref().is_none_or(str::is_empty)
            && self
                .attention_signature
                .as_deref()
                .is_none_or(str::is_empty)
    }

    /// A newer mark for the same run and conversation keeps the field it does
    /// not set; a mark for another run or conversation replaces the old one.
    pub(crate) fn merged(old: &ReadMark, mut next: ReadMark) -> ReadMark {
        if old.run_id != next.run_id || old.conversation_id != next.conversation_id {
            return next;
        }
        if next.reply_id.is_none() {
            next.reply_id.clone_from(&old.reply_id);
        }
        if next.attention_signature.is_none() {
            next.attention_signature
                .clone_from(&old.attention_signature);
        }
        next
    }
}

pub struct ReadMarks {
    path: PathBuf,
    marks: BTreeMap<(String, String), ReadMark>,
    /// Marks added by [`ReadMarks::add_all`] and not yet written.
    unsaved: bool,
}

impl ReadMarks {
    /// A missing or unreadable file gives no marks.
    pub fn load(path: PathBuf) -> Self {
        let list: Vec<ReadMark> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let marks = list
            .into_iter()
            .map(|mark| ((mark.computer.clone(), mark.name.clone()), mark))
            .collect();
        Self {
            path,
            marks,
            unsaved: false,
        }
    }

    /// Merges the mark into what is pending for its session, then saves.
    pub fn add(&mut self, mark: ReadMark) {
        let key = (mark.computer.clone(), mark.name.clone());
        let mark = match self.marks.get(&key) {
            Some(old) => ReadMark::merged(old, mark),
            None => mark,
        };
        self.marks.insert(key, mark);
        self.save();
    }

    /// Merges each mark that is not empty (`ReadMarkStore::add`); an empty
    /// field counts as unset, as in Qt. Only in memory: the delivery worker
    /// writes them ([`ReadMarks::flush`]) before it sends any, so the caller
    /// does no file I/O.
    pub fn add_all(&mut self, marks: impl IntoIterator<Item = ReadMark>) {
        let mut added = false;
        for mut mark in marks.into_iter().filter(|mark| !mark.is_empty()) {
            mark.reply_id = mark.reply_id.filter(|id| !id.is_empty());
            mark.attention_signature = mark.attention_signature.filter(|s| !s.is_empty());
            let key = (mark.computer.clone(), mark.name.clone());
            let mark = match self.marks.get(&key) {
                Some(old) => ReadMark::merged(old, mark),
                None => mark,
            };
            self.marks.insert(key, mark);
            added = true;
        }
        self.unsaved |= added;
    }

    /// Writes marks [`ReadMarks::add_all`] added since the last save.
    pub fn flush(&mut self) {
        if self.unsaved {
            self.save();
        }
    }

    pub fn pending(&self) -> Vec<ReadMark> {
        self.marks.values().cloned().collect()
    }

    /// No mark waits; a batch then has nothing to settle.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// A complete snapshot of `computer` lists `names`: its marks for sessions
    /// it no longer lists can never apply and are dropped (`updateUnread` in
    /// `tray/src/FleetState.cpp`). Returns true when it dropped any (then saves).
    pub fn drop_unlisted(&mut self, computer: &str, names: &HashSet<&str>) -> bool {
        let before = self.marks.len();
        self.marks
            .retain(|(owner, name), _| owner != computer || names.contains(name.as_str()));
        let dropped = self.marks.len() != before;
        if dropped {
            self.save();
        }
        dropped
    }

    /// Drops the marks for computers this computer no longer lists as peers
    /// (`publishReadMarks` in `tray/src/TrayAgent.cpp`): they can never be
    /// written. This computer's marks (`computer` "") stay. Returns true when
    /// it dropped any (then saves).
    pub fn drop_orphans(&mut self, peers: &BTreeSet<String>) -> bool {
        let before = self.marks.len();
        self.marks
            .retain(|(owner, _), _| owner.is_empty() || peers.contains(owner));
        let dropped = self.marks.len() != before;
        if dropped {
            self.save();
        }
        dropped
    }

    /// Drops a mark that is finished (`settled()` in `tray/src/FleetState.cpp`):
    /// the row names another run or conversation (or none), or every part is
    /// either empty, no longer the row's current value, or already shown as
    /// read by the host. Returns true when it changed anything (then saves).
    pub fn settle(&mut self, computer: &str, row: &ViewRow) -> bool {
        let key = (computer.to_owned(), row.name.clone());
        let Some(mark) = self.marks.get(&key) else {
            return false;
        };
        let part = |mine: &Option<String>, current: &Option<String>, read: &Option<String>| {
            mine.as_deref().is_none_or(str::is_empty) || mine != current || read == mine
        };
        let settled = row.run_id.as_deref() != Some(mark.run_id.as_str())
            || row.conversation_id.as_deref() != Some(mark.conversation_id.as_str())
            || (part(&mark.reply_id, &row.reply_id, &row.read_reply_id)
                && part(
                    &mark.attention_signature,
                    &row.attention_signature,
                    &row.read_attention_signature,
                ));
        if settled {
            self.marks.remove(&key);
            self.save();
        }
        settled
    }

    /// A failure keeps the marks in memory; the next change tries again.
    fn save(&mut self) {
        self.unsaved = false;
        let list: Vec<&ReadMark> = self.marks.values().collect();
        let written = serde_json::to_vec(&list)
            .map_err(std::io::Error::from)
            .and_then(|bytes| write_atomically(&self.path, &bytes));
        if let Err(error) = written {
            eprintln!(
                "zerus-client: could not save read marks to {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Writes a temporary file beside `path`, then renames it over it, so a
/// reader sees the old file or the new one, never a part. Creates the folder.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut temporary = path.to_path_buf().into_os_string();
    temporary.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = PathBuf::from(temporary);
    if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder)?;
    }
    let written =
        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    struct Temp(PathBuf);
    impl Temp {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "zerus-readmarks-{}-{test}.json",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Temp(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let prefix = format!("{}.", self.0.file_name().unwrap().to_string_lossy());
            for entry in std::fs::read_dir(self.0.parent().unwrap())
                .unwrap()
                .flatten()
            {
                if entry.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    fn mark(run: &str, reply: Option<&str>, signature: Option<&str>) -> ReadMark {
        ReadMark {
            computer: "mac".into(),
            name: "claude/sample-project/review".into(),
            run_id: run.into(),
            conversation_id: "conv-1".into(),
            reply_id: reply.map(Into::into),
            attention_signature: signature.map(Into::into),
        }
    }

    /// A row whose current reply is `r1` and signature `s1`, showing the
    /// given read parts.
    fn row(run: &str, read_reply: Option<&str>, read_signature: Option<&str>) -> ViewRow {
        ViewRow {
            name: "claude/sample-project/review".into(),
            run_id: Some(run.into()),
            conversation_id: Some("conv-1".into()),
            reply_id: Some("r1".into()),
            attention_signature: Some("s1".into()),
            read_reply_id: read_reply.map(Into::into),
            read_attention_signature: read_signature.map(Into::into),
            ..ViewRow::default()
        }
    }

    #[test]
    fn a_newer_mark_keeps_the_field_it_does_not_set() {
        let temp = Temp::new("merge");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), None));
        marks.add(mark("run-1", None, Some("s1")));
        assert_eq!(marks.pending(), vec![mark("run-1", Some("r1"), Some("s1"))]);
    }

    #[test]
    fn a_mark_for_another_run_replaces_the_old_one() {
        let temp = Temp::new("replace");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), Some("s1")));
        marks.add(mark("run-2", None, Some("s2")));
        assert_eq!(marks.pending(), vec![mark("run-2", None, Some("s2"))]);
    }

    #[test]
    fn marks_survive_a_restart() {
        let temp = Temp::new("restart");
        ReadMarks::load(temp.0.clone()).add(mark("run-1", Some("r1"), None));
        assert_eq!(
            ReadMarks::load(temp.0.clone()).pending(),
            vec![mark("run-1", Some("r1"), None)]
        );
    }

    #[test]
    fn the_host_snapshot_settles_a_mark() {
        let temp = Temp::new("settle");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), None));
        assert!(!marks.settle("mac", &row("run-1", Some("r0"), None)));
        assert_eq!(marks.pending().len(), 1);
        assert!(marks.settle("mac", &row("run-1", Some("r1"), None)));
        assert!(marks.pending().is_empty());
        assert!(ReadMarks::load(temp.0.clone()).pending().is_empty());
        marks.add(mark("run-1", Some("r1"), None));
        assert!(marks.settle("mac", &row("run-2", None, None)));
        assert!(marks.pending().is_empty());
    }

    #[test]
    fn a_mark_with_two_parts_waits_for_both() {
        let temp = Temp::new("both");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), Some("s1")));
        assert!(!marks.settle("mac", &row("run-1", Some("r1"), None)));
        assert!(!marks.settle("mac", &row("run-1", None, Some("s1"))));
        assert_eq!(marks.pending().len(), 1);
        assert!(marks.settle("mac", &row("run-1", Some("r1"), Some("s1"))));
        assert!(marks.pending().is_empty());
    }

    #[test]
    fn a_part_no_longer_current_counts_as_settled() {
        let temp = Temp::new("stale");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r0"), Some("s1")));
        // The reply moved on (row.reply_id is r1), so only the signature is left.
        assert!(!marks.settle("mac", &row("run-1", None, None)));
        assert!(marks.settle("mac", &row("run-1", None, Some("s1"))));
    }

    #[test]
    fn a_row_without_run_or_conversation_settles() {
        let temp = Temp::new("ids");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), None));
        let mut bare = row("run-1", None, None);
        bare.run_id = None;
        assert!(marks.settle("mac", &bare));
        marks.add(mark("run-1", Some("r1"), None));
        let mut other = row("run-1", None, None);
        other.conversation_id = Some("conv-2".into());
        assert!(marks.settle("mac", &other));
    }

    #[test]
    fn an_empty_part_is_satisfied() {
        let temp = Temp::new("empty");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), None));
        assert!(!marks.settle("mac", &row("run-1", None, None)));
        assert!(marks.settle("mac", &row("run-1", Some("r1"), None)));
    }

    #[test]
    fn a_complete_snapshot_drops_marks_of_sessions_it_no_longer_lists() {
        let temp = Temp::new("unlisted");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add(mark("run-1", Some("r1"), None));
        let mut other = mark("run-2", Some("r2"), None);
        other.name = "claude/sample-project/other".into();
        marks.add(other.clone());
        let mut peer = mark("run-3", Some("r3"), None);
        peer.computer = "laptop".into();
        marks.add(peer.clone());
        assert!(!marks.is_empty());
        let listed: HashSet<&str> = ["claude/sample-project/review"].into();
        assert!(!marks.drop_unlisted(
            "mac",
            &[
                "claude/sample-project/review",
                "claude/sample-project/other",
            ]
            .into()
        ));
        assert!(marks.drop_unlisted("mac", &listed));
        assert_eq!(marks.pending(), vec![peer, mark("run-1", Some("r1"), None)]);
        assert_eq!(ReadMarks::load(temp.0.clone()).pending().len(), 2);
        assert!(marks.drop_unlisted("laptop", &HashSet::new()));
        assert!(marks.drop_unlisted("mac", &HashSet::new()));
        assert!(marks.is_empty());
    }

    #[test]
    fn adding_several_merges_them_and_skips_empty_marks() {
        let temp = Temp::new("add-all");
        let mut marks = ReadMarks::load(temp.0.clone());
        let mut other = mark("run-2", Some("r2"), None);
        other.name = "claude/sample-project/other".into();
        marks.add_all([
            mark("run-1", Some("r1"), None),
            // An empty field is no field, as in Qt: it keeps the older value.
            mark("run-1", Some(""), Some("s1")),
            mark("run-1", None, None),
            other.clone(),
        ]);
        let expected = vec![other, mark("run-1", Some("r1"), Some("s1"))];
        assert_eq!(marks.pending(), expected);
        // Kept in memory until flushed: the caller does no file I/O.
        assert!(!temp.0.exists());
        marks.flush();
        assert_eq!(ReadMarks::load(temp.0.clone()).pending(), expected);
        // Only empty marks: nothing changes, and nothing is written.
        let temp = Temp::new("add-all-empty");
        let mut empty = ReadMarks::load(temp.0.clone());
        empty.add_all([mark("run-1", Some(""), None)]);
        empty.flush();
        assert!(!temp.0.exists());
    }

    #[test]
    fn a_flush_writes_only_unsaved_marks() {
        let temp = Temp::new("flush");
        let mut marks = ReadMarks::load(temp.0.clone());
        marks.add_all([mark("run-1", Some("r1"), None)]);
        marks.flush();
        std::fs::remove_file(&temp.0).unwrap();
        // Nothing new: no write.
        marks.flush();
        assert!(!temp.0.exists());
        // Any save writes the added marks too, and leaves nothing to flush.
        marks.add_all([mark("run-1", None, Some("s1"))]);
        let mut other = mark("run-2", Some("r2"), None);
        other.name = "claude/sample-project/other".into();
        marks.add(other);
        assert_eq!(ReadMarks::load(temp.0.clone()).pending().len(), 2);
        std::fs::remove_file(&temp.0).unwrap();
        marks.flush();
        assert!(!temp.0.exists());
    }

    #[test]
    fn marks_for_computers_no_longer_configured_are_dropped() {
        let temp = Temp::new("orphans");
        let mut marks = ReadMarks::load(temp.0.clone());
        let on = |computer: &str| {
            let mut m = mark("run-1", Some("r1"), None);
            m.computer = computer.into();
            m
        };
        marks.add(on(""));
        marks.add(on("laptop"));
        marks.add(on("removed"));
        let peers: BTreeSet<String> = ["laptop".to_owned(), "studio".to_owned()].into();
        assert!(marks.drop_orphans(&peers));
        // This computer's marks always stay.
        assert_eq!(marks.pending(), vec![on(""), on("laptop")]);
        assert_eq!(ReadMarks::load(temp.0.clone()).pending().len(), 2);
        assert!(!marks.drop_orphans(&peers));
        assert!(marks.drop_orphans(&BTreeSet::new()));
        assert_eq!(marks.pending(), vec![on("")]);
    }

    #[test]
    fn a_mark_without_reply_or_signature_is_empty() {
        assert!(mark("run-1", None, None).is_empty());
        assert!(mark("run-1", Some(""), Some("")).is_empty());
        assert!(!mark("run-1", Some("r1"), None).is_empty());
        assert!(!mark("run-1", None, Some("s1")).is_empty());
    }

    #[test]
    fn an_unreadable_file_starts_empty() {
        let temp = Temp::new("garbage");
        std::fs::write(&temp.0, "not json").unwrap();
        assert!(ReadMarks::load(temp.0.clone()).pending().is_empty());
        assert!(ReadMarks::load(temp.0.with_extension("missing"))
            .pending()
            .is_empty());
    }
}
