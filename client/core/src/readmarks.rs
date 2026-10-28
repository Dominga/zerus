//! Read marks the host's snapshot does not show yet. Kept in a file so a
//! restart keeps retrying (`tray/src/ReadMarks.h`).
use crate::view::ViewRow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    /// A newer mark for the same run and conversation keeps the field it does
    /// not set; a mark for another run or conversation replaces the old one.
    fn merged(old: &ReadMark, mut next: ReadMark) -> ReadMark {
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
        Self { path, marks }
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

    pub fn pending(&self) -> Vec<ReadMark> {
        self.marks.values().cloned().collect()
    }

    /// Drops the mark the row already shows (equal read_reply_id or
    /// read_attention_signature, or another run or conversation); returns true
    /// when it changed anything (then saves).
    pub fn settle(&mut self, computer: &str, row: &ViewRow) -> bool {
        let key = (computer.to_owned(), row.name.clone());
        let Some(mark) = self.marks.get(&key) else {
            return false;
        };
        let other =
            |mine: &str, theirs: &Option<String>| theirs.as_deref().is_some_and(|t| t != mine);
        let shown = |mark: &Option<String>, read: &Option<String>| mark.is_some() && mark == read;
        let settled = other(&mark.run_id, &row.run_id)
            || other(&mark.conversation_id, &row.conversation_id)
            || shown(&mark.reply_id, &row.read_reply_id)
            || shown(&mark.attention_signature, &row.read_attention_signature);
        if settled {
            self.marks.remove(&key);
            self.save();
        }
        settled
    }

    /// Writes a temporary file beside the target, then renames it over it. A
    /// failure keeps the marks in memory; the next change tries again.
    fn save(&self) {
        let list: Vec<&ReadMark> = self.marks.values().collect();
        let Ok(bytes) = serde_json::to_vec(&list) else {
            return;
        };
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        if let Some(folder) = self.path.parent().filter(|f| !f.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(folder);
        }
        if std::fs::write(&temporary, bytes).is_err()
            || std::fs::rename(&temporary, &self.path).is_err()
        {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut tmp = self.0.clone().into_os_string();
            tmp.push(".tmp");
            let _ = std::fs::remove_file(tmp);
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

    fn row(run: &str, reply: Option<&str>, signature: Option<&str>) -> ViewRow {
        ViewRow {
            name: "claude/sample-project/review".into(),
            run_id: Some(run.into()),
            conversation_id: Some("conv-1".into()),
            read_reply_id: reply.map(Into::into),
            read_attention_signature: signature.map(Into::into),
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
    fn an_unreadable_file_starts_empty() {
        let temp = Temp::new("garbage");
        std::fs::write(&temp.0, "not json").unwrap();
        assert!(ReadMarks::load(temp.0.clone()).pending().is_empty());
        assert!(ReadMarks::load(temp.0.with_extension("missing"))
            .pending()
            .is_empty());
    }
}
