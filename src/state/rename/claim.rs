//! Read-only claim admission. Never complete a rename or invoke its backend.
use super::*;
use crate::state::repositories::directory::Directory;

const ENTRIES: usize = 4096;
const WAIT: &str = "Session rename is unfinished; finish recovery before answering";

pub(super) fn ready(root: &Path, record: &Value, name: &str) -> Result<()> {
    ready_with(
        root,
        record,
        name,
        ENTRIES,
        storage::ID_RECORD_BYTES as usize,
    )
}

fn ready_with(
    root: &Path,
    record: &Value,
    name: &str,
    entries: usize,
    mut bytes: usize,
) -> Result<()> {
    let path = root.join("renames");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    let directory = Directory::open(&path)?;
    for (count, entry) in fs::read_dir(&path).map_err(|e| e.to_string())?.enumerate() {
        if count >= entries {
            return Err("Rename admission inventory exceeds its bound".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let leaf = entry.file_name();
        let leaf = leaf.to_str().ok_or("Invalid rename intent filename")?;
        let wire = directory.read_file(leaf, bytes)?;
        bytes -= wire.len();
        let intent: Value = serde_json::from_slice(&wire).map_err(|e| e.to_string())?;
        if intent["version"] != 1 || intent.get("record").is_none() {
            return Err("Invalid rename intent at claim admission".into());
        }
        validate(string(&intent, "old"), string(&intent, "new"))?;
        let source = &intent["record"];
        let related = if source.is_object() {
            let run = string(source, "run_id");
            if run.is_empty() {
                return Err("Rename intent lost its run identity".into());
            }
            run == string(record, "run_id")
                || !string(record, "session_id").is_empty()
                    && source["session_id"] == record["session_id"]
        } else if source.is_null() {
            // Untracked native renames have no run proof to distinguish a
            // reused label; any matching original/current label must refuse.
            [
                name,
                string(record, "name"),
                string(record, "launch_label"),
                string(record, "terminal_name"),
            ]
            .iter()
            .filter(|label| !label.is_empty())
            .any(|label| [string(&intent, "old"), string(&intent, "new")].contains(label))
        } else {
            return Err("Invalid rename source at claim admission".into());
        };
        if related {
            return Err(WAIT.into());
        }
    }
    directory.verify()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Value {
        json!({"name":"claude/project/current","run_id":"original-run","session_id":"original-session"})
    }
    fn intent(record: Value) -> Value {
        json!({"version":1,"old":"claude/project/old","new":"claude/project/current","record":record})
    }
    fn write(root: &Path, value: &Value) -> PathBuf {
        let directory = root.join("renames");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("intent.json");
        fs::write(&path, value.to_string()).unwrap();
        path
    }

    #[test]
    fn pending_run_session_or_untracked_name_refuses_but_another_run_allows() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert!(ready(&root, &record(), "claude/project/old").is_ok());
        for source in [
            record(),
            json!({"run_id":"later-run","session_id":"original-session"}),
            Value::Null,
        ] {
            let path = write(&root, &intent(source));
            let before = fs::read(&path).unwrap();
            assert_eq!(
                ready(&root, &record(), "claude/project/old").unwrap_err(),
                WAIT
            );
            assert_eq!(fs::read(path).unwrap(), before);
        }
        write(
            &root,
            &intent(json!({"run_id":"another-run","session_id":"another-session"})),
        );
        assert!(
            ready(&root, &record(), "claude/project/old").is_ok(),
            "reused label is not run identity"
        );
    }

    #[test]
    fn malformed_oversize_or_incomplete_inventory_cannot_clear_admission() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for value in [
            json!({}),
            json!({"version":2}),
            intent(json!({})),
            intent(json!(false)),
        ] {
            write(&root, &value);
            assert!(ready(&root, &record(), "claude/project/old").is_err());
        }
        let path = write(&root, &intent(json!({"run_id":"another-run"})));
        assert!(ready_with(&root, &record(), "claude/project/old", ENTRIES, 4).is_err());
        assert!(ready_with(&root, &record(), "claude/project/old", 0, 4096).is_err());
        let second = path.with_file_name("second.json");
        fs::copy(&path, &second).unwrap();
        let total = fs::read(&path).unwrap().len() * 2;
        assert!(
            ready_with(&root, &record(), "claude/project/old", ENTRIES, total - 1).is_err(),
            "the byte allowance is shared by the whole inventory"
        );
        fs::remove_file(second).unwrap();
        fs::write(&path, b"{").unwrap();
        assert!(ready(&root, &record(), "claude/project/old").is_err());
    }

    #[test]
    fn linked_or_special_intents_refuse_without_following_or_waiting() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = write(&root, &intent(record()));
        fs::remove_file(&path).unwrap();
        symlink(root.join("missing"), &path).unwrap();
        assert!(ready(&root, &record(), "claude/project/old").is_err());
        fs::remove_file(&path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(ready(&root, &record(), "claude/project/old").is_err());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(root.join("renames")).unwrap();
        fs::create_dir(root.join("elsewhere")).unwrap();
        symlink(root.join("elsewhere"), root.join("renames")).unwrap();
        assert!(ready(&root, &record(), "claude/project/old").is_err());
    }
}
