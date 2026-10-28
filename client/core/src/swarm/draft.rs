//! The pending organization edits on disk, so a restart sends them again, as
//! Qt does: it saves its draft with every change (`workspace/organization`,
//! tray/src/SessionsWindow.cpp:2496) and its observed base and versions
//! (SwarmController.cpp:111-130). After a restart it sends the draft from them
//! once its first read answered (SwarmController.cpp:76-89, 191-206). The
//! controller ([`super::sync::Controller::take_draft`]) says what the file
//! holds; the Tauri hub keeps it beside `organization-layout.json`.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io;
use std::path::Path;

/// The draft file in the app data directory; a replay keeps its own.
pub const FILE: &str = "organization-draft.json";
pub const REPLAY_FILE: &str = "organization-draft-replay.json";
const VERSION: u32 = 1;

/// Pending edits: the base they start from, the organization they make and
/// the versions observed with the base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub version: u32,
    pub base: Value,
    pub desired: Value,
    pub versions: Map<String, Value>,
}

impl Draft {
    pub fn new(base: Value, desired: Value, versions: Map<String, Value>) -> Self {
        Self {
            version: VERSION,
            base,
            desired,
            versions,
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("the draft serializes")
    }

    /// A draft of this version with both organizations; anything else is none.
    pub fn from_json(bytes: &[u8]) -> Option<Self> {
        let draft: Self = serde_json::from_slice(bytes).ok()?;
        (draft.version == VERSION && draft.base.is_object() && draft.desired.is_object())
            .then_some(draft)
    }
}

/// What the draft file becomes.
#[derive(Debug, PartialEq)]
pub enum DraftWrite {
    Save(Vec<u8>),
    Remove,
}

/// The saved draft; a missing or unreadable file is none.
pub fn load(path: &Path) -> Option<Draft> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| Draft::from_json(&bytes))
}

/// Replaces the file through a temporary one beside it, flushed to disk and
/// renamed over it, the write the read marks use (`fsx::write_private_atomic`).
/// A failure keeps the old file.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::fsx::write_private_atomic(path, bytes)
}

/// Saves or removes the draft; removing a missing file is no error.
pub fn store(path: &Path, change: &DraftWrite) -> io::Result<()> {
    match change {
        DraftWrite::Save(bytes) => write(path, bytes),
        DraftWrite::Remove => match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-draft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_draft_file_is_replaced_whole_and_removed_when_settled() {
        let dir = scratch("store");
        let path = dir.join(FILE);
        let draft = Draft::new(
            json!({"projects": []}),
            json!({"projects": [{"id": "alpha"}]}),
            Map::new(),
        );
        store(&path, &DraftWrite::Save(draft.to_json())).unwrap();
        assert_eq!(load(&path), Some(draft.clone()));
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            [std::ffi::OsString::from(FILE)],
            "no temporary file is left"
        );
        let newer = Draft::new(draft.desired.clone(), json!({"projects": []}), Map::new());
        store(&path, &DraftWrite::Save(newer.to_json())).unwrap();
        assert_eq!(load(&path), Some(newer));
        store(&path, &DraftWrite::Remove).unwrap();
        assert!(!path.exists());
        store(&path, &DraftWrite::Remove).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_or_other_draft_is_none() {
        assert_eq!(Draft::from_json(b"not json"), None);
        assert_eq!(
            Draft::from_json(br#"{"version": 2, "base": {}, "desired": {}, "versions": {}}"#),
            None
        );
        assert_eq!(
            Draft::from_json(br#"{"version": 1, "base": null, "desired": {}, "versions": {}}"#),
            None
        );
        assert!(
            Draft::from_json(br#"{"version": 1, "base": {}, "desired": {}, "versions": {}}"#)
                .is_some()
        );
        assert_eq!(load(&scratch("missing").join(FILE)), None);
    }
}
