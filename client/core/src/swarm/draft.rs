//! The pending organization edits on disk, so a restart sends them again, as
//! Qt does: it saves its draft with every change (`workspace/organization`,
//! tray/src/SessionsWindow.cpp:2496) and its observed base and versions
//! (SwarmController.cpp:111-130). After a restart it sends the draft from them
//! once its first read answered (SwarmController.cpp:76-89, 191-206). The
//! controller ([`super::sync::Controller::take_draft`]) says what the file
//! holds; the Tauri hub keeps it beside `organization-layout.json`.
//!
//! A draft is applied without the user looking at it again, so loading fails
//! closed: a file that is too large, not a draft, of another version or not
//! shaped like hgs's organizations is [`Load::Dropped`], never applied. The
//! hub then tells the user and moves the file aside ([`set_aside`]); it is
//! never deleted silently.
use super::projects;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The draft file in the app data directory; a replay keeps its own.
pub const FILE: &str = "organization-draft.json";
pub const REPLAY_FILE: &str = "organization-draft-replay.json";
/// The largest draft file read: base, desired state and versions, each at
/// most one `swarm get` reply ([`crate::reads::READ_LIMIT`]).
pub const MAX_BYTES: u64 = 3 * crate::reads::READ_LIMIT as u64;
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

/// Why `organization` cannot be a draft's `side`: hgs lists `projects` as
/// objects with distinct, non-empty string IDs, and `sessions`, when
/// present, as strings (src/swarm/model.rs:107-110).
fn problem(side: &str, organization: &Value) -> Option<String> {
    let Some(list) = organization.get("projects").and_then(Value::as_array) else {
        return Some(format!(
            "the draft's {side} organization has no project list"
        ));
    };
    let mut ids = BTreeSet::new();
    for project in list {
        if !project.is_object() {
            return Some(format!(
                "a project in the draft's {side} organization is not an object"
            ));
        }
        let Some(id) = project
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return Some(format!(
                "a project in the draft's {side} organization has no ID"
            ));
        };
        if !ids.insert(id) {
            return Some(format!(
                "the draft's {side} organization lists project {id} twice"
            ));
        }
        match project.get("sessions") {
            None => {}
            Some(Value::Array(sessions)) if sessions.iter().all(Value::is_string) => {}
            Some(_) => {
                return Some(format!(
                    "project {id} in the draft's {side} organization has a malformed session list"
                ))
            }
        }
    }
    None
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

    /// A draft of this version whose organizations have hgs's shape, or why
    /// it is not one. A desired organization without a project against a
    /// base with some is refused too: no edit removes the last project
    /// (Delete needs another project to take its sessions, `edit::apply`).
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let draft: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("the draft file is not a draft: {error}"))?;
        if draft.version != VERSION {
            return Err(format!(
                "the draft file has version {}, not {VERSION}",
                draft.version
            ));
        }
        if let Some(problem) =
            problem("base", &draft.base).or_else(|| problem("desired", &draft.desired))
        {
            return Err(problem);
        }
        if projects(&draft.desired).is_empty() && !projects(&draft.base).is_empty() {
            return Err("the draft's desired organization lists no project".into());
        }
        Ok(draft)
    }
}

/// What the draft file becomes.
#[derive(Debug, PartialEq)]
pub enum DraftWrite {
    Save(Vec<u8>),
    Remove,
}

/// What the draft file held at start.
#[derive(Debug, PartialEq)]
pub enum Load {
    /// No draft file: nothing was pending.
    Missing,
    Loaded(Draft),
    /// The file cannot be used, for the reason given; it is never applied.
    /// The hub tells the user and moves the file aside with [`set_aside`].
    Dropped(String),
}

/// The saved draft, read up to [`MAX_BYTES`].
pub fn load(path: &Path) -> Load {
    load_at_most(path, MAX_BYTES)
}

fn load_at_most(path: &Path, limit: u64) -> Load {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Load::Missing,
        Err(error) => return Load::Dropped(format!("the draft file cannot be read: {error}")),
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(limit + 1).read_to_end(&mut bytes) {
        return Load::Dropped(format!("the draft file cannot be read: {error}"));
    }
    if bytes.len() as u64 > limit {
        return Load::Dropped(format!("the draft file is larger than {limit} bytes"));
    }
    match Draft::from_json(&bytes) {
        Ok(draft) => Load::Loaded(draft),
        Err(reason) => Load::Dropped(reason),
    }
}

/// Moves a dropped draft file aside as `<file>.<unix seconds>.bad`, or
/// `<file>.<seconds>-<n>.bad` when that name is taken, so it is neither
/// deleted nor overwritten. Gives the new path.
pub fn set_aside(path: &Path, now_secs: u64) -> io::Result<PathBuf> {
    for n in 0..100u32 {
        let mut name = path.as_os_str().to_owned();
        name.push(if n == 0 {
            format!(".{now_secs}.bad")
        } else {
            format!(".{now_secs}-{n}.bad")
        });
        let aside = PathBuf::from(name);
        if matches!(std::fs::symlink_metadata(&aside), Err(error) if error.kind() == io::ErrorKind::NotFound)
        {
            std::fs::rename(path, &aside)?;
            return Ok(aside);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name to set the draft file aside",
    ))
}

/// Replaces the file through a temporary one beside it that only this user
/// may read, flushed to disk and renamed over it (`fsx::write_private_atomic`).
/// A failure keeps the old file.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::fsx::write_private_atomic(path, bytes, true)
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-draft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// An organization as `swarm get` lists it, with alpha, beta and the Inbox.
    fn three() -> Value {
        json!({"version": 2, "default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "folders": [], "sessions": ["workstation\nclaude/sample-project/review"]},
            {"id": "beta", "name": "Beta", "folders": [], "sessions": []},
            {"id": "ungrouped", "name": "Inbox", "folders": [], "sessions": []}]})
    }

    fn file(dir: &Path, value: &Value) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    fn dropped(load: Load) -> String {
        match load {
            Load::Dropped(reason) => reason,
            other => panic!("not dropped: {other:?}"),
        }
    }

    #[test]
    fn the_draft_file_is_replaced_whole_and_removed_when_settled() {
        let dir = scratch("store");
        let path = dir.join(FILE);
        assert_eq!(load(&path), Load::Missing);
        let draft = Draft::new(
            json!({"projects": []}),
            json!({"projects": [{"id": "alpha"}]}),
            Map::new(),
        );
        store(&path, &DraftWrite::Save(draft.to_json())).unwrap();
        assert_eq!(load(&path), Load::Loaded(draft.clone()));
        assert_eq!(names(&dir), [FILE], "no temporary file is left");
        let newer = Draft::new(draft.desired.clone(), three(), Map::new());
        store(&path, &DraftWrite::Save(newer.to_json())).unwrap();
        assert_eq!(load(&path), Load::Loaded(newer));
        store(&path, &DraftWrite::Remove).unwrap();
        assert_eq!(load(&path), Load::Missing);
        store(&path, &DraftWrite::Remove).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_version_or_shape_is_not_a_draft() {
        assert!(Draft::from_json(b"not json").is_err());
        let draft = |base: Value, desired: Value| {
            serde_json::to_vec(
                &json!({"version": 1, "base": base, "desired": desired, "versions": {}}),
            )
            .unwrap()
        };
        assert_eq!(
            Draft::from_json(br#"{"version": 2, "base": {"projects": []}, "desired": {"projects": []}, "versions": {}}"#),
            Err("the draft file has version 2, not 1".into())
        );
        assert_eq!(
            Draft::from_json(&draft(json!(null), three())),
            Err("the draft's base organization has no project list".into())
        );
        assert_eq!(
            Draft::from_json(&draft(
                three(),
                json!({"projects": [{"id": "alpha"}, "beta"]})
            )),
            Err("a project in the draft's desired organization is not an object".into())
        );
        assert_eq!(
            Draft::from_json(&draft(
                three(),
                json!({"projects": [{"id": "alpha"}, {"id": "alpha"}]})
            )),
            Err("the draft's desired organization lists project alpha twice".into())
        );
        assert_eq!(
            Draft::from_json(&draft(
                three(),
                json!({"projects": [{"id": "alpha", "sessions": 5}]})
            )),
            Err(
                "project alpha in the draft's desired organization has a malformed session list"
                    .into()
            )
        );
        assert!(Draft::from_json(&draft(three(), three())).is_ok());
        assert!(
            Draft::from_json(&draft(json!({"projects": []}), json!({"projects": []}))).is_ok(),
            "an empty catalog stays empty"
        );
    }

    // Review I2 (fix round 1): a draft is applied without the user seeing it again.
    #[test]
    fn a_draft_without_projects_against_a_listed_base_is_dropped_and_set_aside() {
        let dir = scratch("empty");
        let path = file(
            &dir,
            &json!({"version": 1, "base": three(), "desired": {"projects": []}, "versions": {}}),
        );
        assert_eq!(
            dropped(load(&path)),
            "the draft's desired organization lists no project"
        );
        let aside = set_aside(&path, 1_761_600_000).unwrap();
        assert_eq!(aside, dir.join(format!("{FILE}.1761600000.bad")));
        assert!(!path.exists() && aside.exists(), "moved, not deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_without_an_id_is_dropped() {
        let dir = scratch("id");
        let mut desired = three();
        desired["projects"][1].as_object_mut().unwrap().remove("id");
        let path = file(
            &dir,
            &json!({"version": 1, "base": three(), "desired": desired, "versions": {}}),
        );
        assert_eq!(
            dropped(load(&path)),
            "a project in the draft's desired organization has no ID"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_draft_is_dropped() {
        let dir = scratch("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, br#"{"version": 1, "base": {"projects": ["#).unwrap();
        assert!(dropped(load(&path)).starts_with("the draft file is not a draft: "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_draft_is_dropped_after_reading_at_most_one_byte_past_the_limit() {
        let dir = scratch("large");
        let draft = json!({"version": 1, "base": three(), "desired": three(), "versions": {}});
        let path = file(&dir, &draft);
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(
            matches!(load_at_most(&path, size), Load::Loaded(_)),
            "at the limit"
        );
        assert_eq!(
            dropped(load_at_most(&path, size - 1)),
            format!("the draft file is larger than {} bytes", size - 1)
        );
        assert_eq!(MAX_BYTES, 3 * (8 << 20));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_draft_set_aside_twice_in_one_second_keeps_both_files() {
        let dir = scratch("aside");
        let path = file(&dir, &json!("first"));
        let first = set_aside(&path, 7).unwrap();
        let path = file(&dir, &json!("second"));
        let second = set_aside(&path, 7).unwrap();
        assert_eq!(
            names(&dir),
            [format!("{FILE}.7-1.bad"), format!("{FILE}.7.bad")]
        );
        assert_eq!(std::fs::read(first).unwrap(), br#""first""#);
        assert_eq!(std::fs::read(second).unwrap(), br#""second""#);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
