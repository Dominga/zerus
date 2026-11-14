//! Read-only hints; a reader neither opens nor repairs a project store.
use super::*;

impl ProjectStore {
    /// Reads the atomically replaced journal without acquiring the project's
    /// lock, listing directories or starting a journal. Unknown hints require
    /// the caller to observe its live documents through their trusted reader.
    pub fn read_changes_since(
        root: &Path,
        project: &str,
        generation: &str,
        offset: u64,
    ) -> Changes {
        let unknown = || Changes {
            generation: String::new(),
            offset: 0,
            documents: None,
        };
        if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
            return unknown();
        }
        let path = root
            .join(format!("{:x}", Sha256::digest(project.as_bytes())))
            .join(CHANGES);
        let Hint::Found(journal) = read_journal(&path) else {
            return unknown();
        };
        let Some(end) = journal.base.checked_add(journal.body.len() as u64) else {
            return unknown();
        };
        let mut changes = Changes {
            generation: journal.generation,
            offset: end,
            documents: None,
        };
        if changes.generation != generation || offset < journal.base || offset > end {
            return changes;
        }
        let mut documents = BTreeSet::new();
        for line in
            journal.body[(offset - journal.base) as usize..].split_inclusive(|&b| b == b'\n')
        {
            match std::str::from_utf8(line)
                .ok()
                .and_then(|l| l.strip_suffix('\n'))
            {
                Some(document) if document == "project" || ids::is_uuid(document) => {
                    documents.insert(document.to_owned());
                }
                _ => return changes,
            }
        }
        changes.documents = Some(documents);
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_hint_never_creates_or_repairs_a_missing_project() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("missing");
        let c = ProjectStore::read_changes_since(&root, "project", "", 0);
        assert!(c.documents.is_none());
        assert!(!root.exists());
    }
    #[test]
    fn read_only_hint_reads_while_the_project_lock_is_held() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project").unwrap();
        let generation = ids::new_uuid();
        let task = ids::new_uuid();
        let body = format!("{task}\n");
        assert!(write_journal(
            &store.dir.join(CHANGES),
            &generation,
            0,
            body.as_bytes()
        ));
        let before = fs::read(store.dir.join(CHANGES)).unwrap();
        let c = ProjectStore::read_changes_since(temp.path(), "project", &generation, 0);
        assert_eq!(c.documents, Some([task].into()));
        assert_eq!(c.offset, body.len() as u64);
        assert_eq!(fs::read(store.dir.join(CHANGES)).unwrap(), before);
        let next =
            ProjectStore::read_changes_since(temp.path(), "project", &c.generation, c.offset);
        assert_eq!(next.documents, Some(BTreeSet::new()));
        assert!(
            ProjectStore::read_changes_since(temp.path(), "project", "another", 0)
                .documents
                .is_none()
        );
        assert!(
            ProjectStore::read_changes_since(temp.path(), "project", &generation, u64::MAX)
                .documents
                .is_none()
        );
    }
    #[test]
    fn read_only_hint_leaves_unusable_journals_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project").unwrap();
        let path = store.dir.join(CHANGES);
        for bytes in [b"malformed".to_vec(), vec![b'x'; CHANGES_READ as usize + 1]] {
            fs::write(&path, &bytes).unwrap();
            assert!(
                ProjectStore::read_changes_since(temp.path(), "project", "", 0)
                    .documents
                    .is_none()
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_file(&path).unwrap();
        assert!(
            ProjectStore::read_changes_since(temp.path(), "project", "", 0)
                .documents
                .is_none()
        );
        assert!(!path.exists());
    }
}
