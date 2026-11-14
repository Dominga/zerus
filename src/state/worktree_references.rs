//! Publication exclusion for a checkout's live/saved folder references.
//! Shared ancestor keys let unrelated provider commits proceed together; a
//! retiring checkout takes only its own folder/ownership keys exclusively.
//! These retained advisory files grant no cleanup or native-input authority.
use super::*;
use crate::state::repositories::directory::Directory;
use crate::state::worktrees::canonical;
use fs2::FileExt;
use std::collections::BTreeSet;

const MAX_KEYS: usize = 128;

pub(super) struct Scope {
    state: Directory,
    directory: Directory,
    locks: Vec<(String, crate::platform::FileLock)>,
    folders: Vec<(PathBuf, PathBuf)>,
}

fn folder_key(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or("The folder reference is not UTF-8")?;
    Ok(format!(
        "{}.lock",
        storage::label_key(&format!("folder:{path}"))
    ))
}

fn owner_key(owner: &str) -> String {
    format!("{}.lock", storage::label_key(&format!("owner:{owner}")))
}

impl Scope {
    pub(super) fn releasing(checkout: &Path, owner: &str) -> Result<Self> {
        let folder = canonical(checkout.to_str().ok_or("Invalid checkout path")?);
        let keys = BTreeSet::from([folder_key(&folder)?, owner_key(owner)]);
        Self::acquire(keys, vec![(checkout.into(), folder)], true)
    }

    /// Provider writers hold their ordinary binding exclusion as before. No
    /// new global lock or wait is added, including for a first binding.
    pub(super) fn publishing(record: &Value) -> Result<Option<Self>> {
        let mut keys = BTreeSet::new();
        let mut folders = Vec::new();
        for field in ["cwd", "launch_dir"] {
            if record
                .get(field)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err("A saved session folder is unreadable; no binding was changed".into());
            }
            let path = Path::new(string(record, field));
            if !path.is_absolute() {
                continue;
            }
            let folder = canonical(string(record, field));
            for ancestor in folder.ancestors() {
                if !ancestor.as_os_str().is_empty() {
                    keys.insert(folder_key(ancestor)?);
                    if keys.len() > MAX_KEYS {
                        return Err("Folder references exceed their coordination bound".into());
                    }
                }
            }
            folders.push((path.into(), folder));
        }
        let owner = string(&record["worktree_owner"], "id");
        if !owner.is_empty() {
            keys.insert(owner_key(owner));
        }
        if keys.len() > MAX_KEYS {
            return Err("Folder references exceed their coordination bound".into());
        }
        if keys.is_empty() {
            return Ok(None);
        }
        Self::acquire(keys, folders, false).map(Some)
    }

    fn acquire(
        keys: BTreeSet<String>,
        folders: Vec<(PathBuf, PathBuf)>,
        exclusive: bool,
    ) -> Result<Self> {
        let state = Directory::open(&storage::absolute_root()?)?;
        let directory = state.ensure_child("worktree-reference-locks")?;
        let mut scope = Self {
            state,
            directory,
            locks: Vec::new(),
            folders,
        };
        for name in keys {
            let file = scope.directory.open_private_lock(&name)?;
            let acquired = if exclusive {
                FileExt::try_lock_exclusive(&file)
            } else {
                FileExt::try_lock_shared(&file)
            };
            acquired.map_err(|error| {
                if crate::platform::lock_contended(&error) {
                    "Folder references are busy; no binding or Git veto was changed".to_owned()
                } else {
                    error.to_string()
                }
            })?;
            scope
                .locks
                .push((name, crate::platform::FileLock::held(file)));
        }
        scope.verify()?;
        Ok(scope)
    }

    pub(super) fn verify(&self) -> Result<()> {
        self.state.verify()?;
        self.directory.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path {
            return Err("The original folder-reference state changed".into());
        }
        for (path, expected) in &self.folders {
            if canonical(path.to_str().ok_or("Invalid folder reference")?) != *expected {
                return Err("A folder reference changed during publication".into());
            }
        }
        for (name, held) in &self.locks {
            let current = self.directory.read_private(name)?;
            if current.metadata().map_err(|e| e.to_string())?.len() != 0
                || crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())?
                    != held.identity().map_err(|e| e.to_string())?
            {
                return Err("The original folder-reference exclusion changed".into());
            }
        }
        Ok(())
    }
}
